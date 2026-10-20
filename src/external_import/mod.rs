//! Importazione da altre app di note: Evernote (ENEX), Joplin (JEX) e
//! vault Obsidian (ADR 0011).
//!
//! Ogni importatore produce un [`ImportPlan`] come le altre importazioni:
//! l'anteprima non modifica nulla e il commit avviene in un'unica
//! transazione. In più conserva il taccuino d'origine di ogni nota e i
//! collegamenti tra note. I file vengono solo letti: niente viene estratto o
//! scritto fuori dalla cartella dati dell'app.

pub mod enex;
pub mod joplin;
pub mod obsidian;

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use crate::{
    content::{escape_html, html_to_document, is_utc_timestamp, markdown_to_document},
    domain::note::validate_document,
    storage::{MAX_ATTACHMENT_BYTES, normalize_tag_name, normalize_title, sniff_attachment_type},
    transfer::{ImportedAttachment, ParsedNote, PlannedLink, fingerprint},
};

/// Testo che sostituisce nel contenuto un allegato incorporato (immagine,
/// PDF…): l'allegato è nel pannello della nota, il testo ne indica la
/// posizione originale.
pub(crate) fn attachment_marker(filename: &str) -> String {
    format!("📎 {filename}")
}

/// Converte il Markdown di una nota il cui titolo è noto: un titolo di primo
/// livello identico in testa al testo viene tolto (duplicato), gli altri
/// restano nel contenuto.
pub(crate) fn markdown_with_title(title: &str, body: &str) -> Value {
    let header = serde_json::to_string(title.trim()).unwrap_or_else(|_| "\"\"".to_owned());
    markdown_to_document(&format!("---\ntitle: {header}\n---\n{body}")).document
}

/// Come [`markdown_with_title`] per un corpo HTML (sanificato).
pub(crate) fn html_with_title(title: &str, body: &str) -> Value {
    html_to_document(&format!(
        "<html><head><title>{}</title></head><body>{body}</body></html>",
        escape_html(title.trim())
    ))
    .document
}

/// Sostituisce i segnaposto «📎 nome» delle immagini importate con immagini
/// nel testo (`images`: nome del file → id dell'allegato). Un paragrafo con
/// un segnaposto viene diviso: testo prima, immagine, testo dopo. Nelle voci
/// di elenco il primo paragrafo resta tale, come richiede l'editor.
pub(crate) fn markers_to_images(document: &Value, images: &HashMap<String, String>) -> Value {
    if images.is_empty() {
        return document.clone();
    }
    let mut document = document.clone();
    if let Some(content) = document.get_mut("content").and_then(Value::as_array_mut) {
        let blocks = std::mem::take(content);
        *content = transform_blocks(blocks, images, false);
    }
    document
}

fn transform_blocks(
    blocks: Vec<Value>,
    images: &HashMap<String, String>,
    keep_first: bool,
) -> Vec<Value> {
    let mut output = Vec::with_capacity(blocks.len());
    for (index, mut block) in blocks.into_iter().enumerate() {
        let kind = block
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        if kind == "paragraph" && !(keep_first && index == 0) {
            output.extend(split_paragraph(block, images));
            continue;
        }
        if !matches!(kind.as_str(), "paragraph" | "heading" | "codeBlock")
            && let Some(content) = block.get_mut("content").and_then(Value::as_array_mut)
        {
            let children = std::mem::take(content);
            let first_is_paragraph = matches!(kind.as_str(), "listItem" | "taskItem");
            *content = transform_blocks(children, images, first_is_paragraph);
        }
        output.push(block);
    }
    output
}

fn split_paragraph(paragraph: Value, images: &HashMap<String, String>) -> Vec<Value> {
    let inline: Vec<Value> = paragraph
        .get("content")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let has_marker = inline.iter().any(|node| {
        node.get("text")
            .and_then(Value::as_str)
            .is_some_and(|text| {
                images
                    .keys()
                    .any(|name| text.contains(&attachment_marker(name)))
            })
    });
    if !has_marker {
        return vec![paragraph];
    }
    let attrs = paragraph.get("attrs").cloned();
    let make_paragraph = |content: Vec<Value>| {
        let mut node = json!({ "type": "paragraph", "content": content });
        if let Some(attrs) = &attrs {
            node["attrs"] = attrs.clone();
        }
        node
    };
    let mut output = Vec::new();
    let mut current: Vec<Value> = Vec::new();
    let flush = |current: &mut Vec<Value>, output: &mut Vec<Value>| {
        let meaningful = current.iter().any(|node| {
            node.get("text")
                .and_then(Value::as_str)
                .is_none_or(|text| !text.trim().is_empty())
        });
        if meaningful {
            output.push(make_paragraph(std::mem::take(current)));
        } else {
            current.clear();
        }
    };
    for node in inline {
        let Some(text) = node.get("text").and_then(Value::as_str).map(str::to_owned) else {
            current.push(node);
            continue;
        };
        let mut rest = text.as_str();
        loop {
            let found = images
                .iter()
                .filter_map(|(name, id)| {
                    rest.find(&attachment_marker(name))
                        .map(|position| (position, name, id))
                })
                .min_by_key(|(position, name, _)| (*position, std::cmp::Reverse(name.len())));
            let Some((position, name, id)) = found else {
                break;
            };
            if position > 0 {
                let mut before = node.clone();
                before["text"] = Value::String(rest[..position].to_owned());
                current.push(before);
            }
            flush(&mut current, &mut output);
            output.push(json!({
                "type": "image",
                "attrs": { "attachmentId": id, "alt": name }
            }));
            rest = &rest[position + attachment_marker(name).len()..];
        }
        if !rest.is_empty() {
            let mut after = node.clone();
            after["text"] = Value::String(rest.to_owned());
            current.push(after);
        }
    }
    flush(&mut current, &mut output);
    output
}

/// Nota letta da un'altra app, prima delle verifiche comuni.
pub(crate) struct ExternalNote {
    pub title: String,
    pub document: Value,
    pub tags: Vec<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub attachments: Vec<ImportedAttachment>,
    pub notebook_path: Vec<String>,
    pub link_keys: Vec<String>,
    pub links: Vec<PlannedLink>,
    pub warnings: Vec<String>,
    pub source: &'static str,
}

impl ExternalNote {
    pub(crate) fn new(source: &'static str, title: String, document: Value) -> Self {
        Self {
            title,
            document,
            tags: Vec::new(),
            created_at: None,
            updated_at: None,
            attachments: Vec::new(),
            notebook_path: Vec::new(),
            link_keys: Vec::new(),
            links: Vec::new(),
            warnings: Vec::new(),
            source,
        }
    }
}

fn clip_chars(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Verifiche comuni: documento valido, titolo e tag normalizzati, date UTC
/// valide, allegati di tipo supportato entro i limiti di dimensione.
pub(crate) fn finish(note: ExternalNote, budget: &mut u64) -> Result<ParsedNote, String> {
    let ExternalNote {
        title,
        document,
        tags,
        created_at,
        updated_at,
        attachments,
        notebook_path,
        link_keys,
        links,
        mut warnings,
        source,
    } = note;
    validate_document(&document).map_err(str::to_owned)?;
    let cleaned_title = title
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>();
    let title = normalize_title(&clip_chars(cleaned_title.trim(), 512))
        .map_err(|error| error.to_string())?;

    let mut seen = HashSet::new();
    let tags: Vec<String> = tags
        .iter()
        .filter_map(|tag| normalize_tag_name(tag.trim().trim_start_matches('#')).ok())
        .filter(|tag| seen.insert(tag.to_lowercase()))
        .take(100)
        .collect();

    let mut kept = Vec::new();
    let mut hashes = HashSet::new();
    for attachment in attachments {
        if attachment.bytes.len() > MAX_ATTACHMENT_BYTES {
            warnings.push(format!(
                "allegato oltre 25 MiB ignorato: {}",
                attachment.filename
            ));
            continue;
        }
        if sniff_attachment_type(&attachment.bytes).is_err() {
            warnings.push(format!(
                "allegato di tipo non supportato ignorato: {}",
                attachment.filename
            ));
            continue;
        }
        // Lo stesso file incorporato più volte viene importato una volta.
        if !hashes.insert(crate::transfer::content_hash(&attachment.bytes)) {
            continue;
        }
        match budget.checked_sub(attachment.bytes.len() as u64) {
            Some(remaining) => *budget = remaining,
            None => {
                warnings.push(format!(
                    "allegato ignorato per limite complessivo dell'importazione: {}",
                    attachment.filename
                ));
                continue;
            }
        }
        kept.push(attachment);
    }

    let valid = |value: Option<String>| value.filter(|value| is_utc_timestamp(value));
    let notebook_path = notebook_path
        .into_iter()
        .map(|part| part.trim().to_owned())
        .filter(|part| !part.is_empty())
        .take(16)
        .collect();
    Ok(ParsedNote {
        fingerprint: fingerprint(&title, &document),
        title,
        document,
        tags,
        created_at: valid(created_at),
        updated_at: valid(updated_at),
        attachments: kept,
        attachment_sources: Vec::new(),
        source,
        warnings,
        notebook_path,
        link_keys,
        links,
    })
}

/// Collegamento o immagine Markdown trovato nel testo.
/// Per `![…](…)` (immagine o allegato incorporato) il segnaposto sostituisce
/// anche il punto esclamativo.
pub(crate) struct MarkdownLink<'a> {
    pub label: &'a str,
    /// Destinazione senza titolo né parentesi angolari.
    pub target: &'a str,
}

/// Collegamento wiki di Obsidian: `[[nota]]`, `[[nota|alias]]`, `![[file]]`.
pub(crate) struct WikiLink<'a> {
    pub embed: bool,
    pub target: &'a str,
    pub alias: Option<&'a str>,
}

pub(crate) enum Rewrite {
    Keep,
    Replace(String),
}

/// Riscrive collegamenti e incorporamenti nel Markdown, lasciando invariati
/// i blocchi e gli span di codice.
pub(crate) fn rewrite_markdown(
    text: &str,
    on_link: &mut dyn FnMut(MarkdownLink<'_>) -> Rewrite,
    mut on_wiki: Option<&mut dyn FnMut(WikiLink<'_>) -> String>,
) -> String {
    let mut output = String::with_capacity(text.len());
    let mut fence: Option<String> = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if let Some(marker) = &fence {
            if trimmed.starts_with(marker.as_str()) {
                fence = None;
            }
            output.push_str(line);
            continue;
        }
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fence = Some(trimmed[..3].to_owned());
            output.push_str(line);
            continue;
        }
        // Gli span di codice (tra backtick) restano invariati.
        for (index, segment) in line.split('`').enumerate() {
            if index > 0 {
                output.push('`');
            }
            if index % 2 == 1 {
                output.push_str(segment);
            } else {
                output.push_str(&rewrite_segment(segment, on_link, &mut on_wiki));
            }
        }
    }
    output
}

fn rewrite_segment(
    segment: &str,
    on_link: &mut dyn FnMut(MarkdownLink<'_>) -> Rewrite,
    on_wiki: &mut Option<&mut dyn FnMut(WikiLink<'_>) -> String>,
) -> String {
    let mut output = String::with_capacity(segment.len());
    let mut index = 0;
    while index < segment.len() {
        let rest = &segment[index..];
        let embed = rest.starts_with('!');
        let start = if embed { 1 } else { 0 };
        if let Some(wiki) = on_wiki.as_mut()
            && rest[start..].starts_with("[[")
            && let Some(end) = rest[start + 2..].find("]]")
        {
            let inner = &rest[start + 2..start + 2 + end];
            if !inner.contains('\n') && !inner.trim().is_empty() {
                let (target, alias) = match inner.split_once('|') {
                    Some((target, alias)) => (target.trim(), Some(alias.trim())),
                    None => (inner.trim(), None),
                };
                output.push_str(&wiki(WikiLink {
                    embed,
                    target,
                    alias,
                }));
                index += start + 2 + end + 2;
                continue;
            }
        }
        if rest[start..].starts_with('[')
            && let Some((label_end, target_start, target_end, total)) =
                markdown_link_bounds(&rest[start..])
        {
            let label = &rest[start + 1..start + label_end];
            let raw_target = &rest[start + target_start..start + target_end];
            let target = link_target(raw_target);
            match on_link(MarkdownLink { label, target }) {
                Rewrite::Keep => output.push_str(&rest[..start + total]),
                Rewrite::Replace(text) => output.push_str(&text),
            }
            index += start + total;
            continue;
        }
        let character = rest.chars().next().unwrap_or(' ');
        output.push(character);
        index += character.len_utf8();
    }
    output
}

/// Limiti di `[etichetta](destinazione)` a partire da `[`: fine
/// dell'etichetta, inizio e fine della destinazione, lunghezza totale.
fn markdown_link_bounds(text: &str) -> Option<(usize, usize, usize, usize)> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut label_end = None;
    for (position, byte) in bytes.iter().enumerate() {
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    label_end = Some(position);
                    break;
                }
            }
            b'\n' => return None,
            _ => {}
        }
    }
    let label_end = label_end?;
    if bytes.get(label_end + 1) != Some(&b'(') {
        return None;
    }
    let target_start = label_end + 2;
    let mut depth = 1usize;
    for (offset, byte) in bytes[target_start..].iter().enumerate() {
        match byte {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    let end = target_start + offset;
                    return Some((label_end, target_start, end, end + 1));
                }
            }
            b'\n' => return None,
            _ => {}
        }
    }
    None
}

/// Destinazione senza titolo (`"…"`) e parentesi angolari.
fn link_target(raw: &str) -> &str {
    let raw = raw.trim();
    if let Some(inner) = raw.strip_prefix('<')
        && let Some(end) = inner.find('>')
    {
        return &inner[..end];
    }
    raw.split_once(char::is_whitespace)
        .map_or(raw, |(target, _)| target)
}

/// Decodifica le sequenze `%XX` di un percorso (es. spazi nei link
/// Markdown di Obsidian). Sequenze non valide restano invariate.
pub(crate) fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && bytes[index + 1].is_ascii_hexdigit()
            && bytes[index + 2].is_ascii_hexdigit()
        {
            let hex = [bytes[index + 1], bytes[index + 2]];
            let hex = std::str::from_utf8(&hex).unwrap_or("00");
            decoded.push(u8::from_str_radix(hex, 16).unwrap_or(0));
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).unwrap_or_else(|_| text.to_owned())
}

/// Converte un istante in formato UTC del database.
pub(crate) fn utc_from_system_time(time: std::time::SystemTime) -> Option<String> {
    jiff::Timestamp::try_from(time)
        .ok()
        .map(crate::time::format_utc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        storage::Database,
        transfer::{ImportDestination, ImportItemStatus},
    };

    #[test]
    fn obsidian_vault_is_imported_with_structure_links_and_attachments() {
        let vault = obsidian::tests::vault();
        let data = tempfile::tempdir().expect("data");
        let database = Database::open(data.path().join("gna.sqlite3")).expect("database");
        let plan = obsidian::plan_obsidian_import(vault.path()).expect("plan");
        let preview = database.preview_import(&plan).expect("preview");
        assert_eq!(preview.kind, "obsidian");
        assert_eq!((preview.new_count, preview.notebook_count), (3, 3));
        assert!(preview.link_count >= 3);
        assert!(preview.suggested_space.is_some());
        let report = database
            .commit_import(
                &plan,
                &ImportDestination::KeepStructure {
                    space_name: Some("Vault".to_owned()),
                },
            )
            .expect("commit");
        assert_eq!(report.imported, 3);
        assert_eq!(report.notebooks, 3);
        assert_eq!(report.attachments, 2);
        assert!(report.links >= 3, "{report:?}");

        let spaces = database.list_spaces().expect("spaces");
        assert_eq!(spaces.len(), 1);
        let mut names: Vec<String> = database
            .list_notebooks()
            .expect("notebooks")
            .into_iter()
            .filter(|notebook| notebook.space_id.as_deref() == Some(spaces[0].id.as_str()))
            .map(|notebook| notebook.name)
            .collect();
        names.sort();
        let root = vault
            .path()
            .file_name()
            .expect("name")
            .to_string_lossy()
            .into_owned();
        let mut expected = vec![root, "Progetti".to_owned(), "Progetti › Casa".to_owned()];
        expected.sort();
        assert_eq!(names, expected);

        let notes = database.list_notes(10, 0).expect("notes");
        let index = notes
            .iter()
            .find(|note| note.title == "Indice")
            .expect("index");
        let links = database.list_note_links(&index.id).expect("links");
        assert_eq!(links.len(), 2);
        let tags: Vec<String> = database
            .list_note_tags(&index.id)
            .expect("tags")
            .into_iter()
            .map(|tag| tag.name)
            .collect();
        assert_eq!(tags.len(), 3);
        assert_eq!(index.created_at, "2024-02-01T00:00:00Z");
        assert_eq!(
            database.list_attachments(&index.id).expect("files").len(),
            1
        );

        // L'immagine incorporata è tornata nel testo e punta all'allegato.
        let files = database.list_attachments(&index.id).expect("files");
        let content = serde_json::to_string(&database.get_note(&index.id).expect("note").content)
            .expect("json");
        assert!(content.contains(r#""type":"image""#), "{content}");
        assert!(content.contains(&files[0].id), "{content}");

        // Una seconda importazione riconosce i duplicati.
        let again = obsidian::plan_obsidian_import(vault.path()).expect("plan");
        let preview = database.preview_import(&again).expect("preview");
        assert_eq!(preview.duplicate_count, 3);
        assert!(
            preview
                .items
                .iter()
                .all(|item| item.status == ImportItemStatus::Duplicate)
        );
    }

    #[test]
    fn collections_can_also_be_flattened_into_one_notebook() {
        let directory = tempfile::tempdir().expect("data");
        let path = directory.path().join("Lavoro.enex");
        std::fs::write(&path, enex::tests::sample_enex()).expect("write");
        let database = Database::open(directory.path().join("gna.sqlite3")).expect("database");
        let plan = enex::plan_enex_import(&[path]).expect("plan");
        let report = database
            .commit_import(
                &plan,
                &ImportDestination::NewNotebook {
                    name: "Da Evernote".to_owned(),
                },
            )
            .expect("commit");
        assert_eq!(report.imported, 2);
        assert_eq!(report.attachments, 1);
        let notebooks = database.list_notebooks().expect("notebooks");
        assert_eq!(notebooks.len(), 1);
        assert_eq!(notebooks[0].name, "Da Evernote");
    }

    #[test]
    fn rewrites_links_but_not_code() {
        let text = "Vedi [nota](:/abc \"titolo\") e ![img](<a b.png>)\n`[x](y)` e [[Altra|alias]]\n```\n[z](w)\n```\n";
        let mut seen = Vec::new();
        let mut wiki = |link: WikiLink<'_>| format!("<{}:{:?}>", link.target, link.alias);
        let output = rewrite_markdown(
            text,
            &mut |link| {
                seen.push((link.label.to_owned(), link.target.to_owned()));
                Rewrite::Replace(format!("{{{}}}", link.target))
            },
            Some(&mut wiki),
        );
        assert_eq!(
            seen,
            vec![
                ("nota".to_owned(), ":/abc".to_owned()),
                ("img".to_owned(), "a b.png".to_owned()),
            ]
        );
        assert_eq!(
            output,
            "Vedi {:/abc} e {a b.png}\n`[x](y)` e <Altra:Some(\"alias\")>\n```\n[z](w)\n```\n"
        );
    }

    #[test]
    fn titles_are_not_duplicated_but_other_headings_stay() {
        let same = markdown_with_title("Spesa", "# Spesa\n\nlatte");
        assert_eq!(crate::content::to_plain_text(&same), "latte");
        let different = markdown_with_title("Spesa", "# Lista\n\nlatte");
        assert_eq!(crate::content::to_plain_text(&different), "Lista\n\nlatte");
        let html = html_with_title("A & B", "<h1>A &amp; B</h1><p>x</p>");
        assert_eq!(crate::content::to_plain_text(&html), "x");
    }

    #[test]
    fn image_markers_become_images_in_the_text() {
        let document = json!({"type": "doc", "content": [
            {"type": "paragraph", "content": [
                {"type": "text", "text": "Prima 📎 a.png dopo"},
                {"type": "text", "text": " 📎 a.png", "marks": [{"type": "bold"}]}
            ]},
            {"type": "bulletList", "content": [{"type": "listItem", "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "📎 a.png"}]},
                {"type": "paragraph", "content": [{"type": "text", "text": "📎 a.png"}]}
            ]}]},
            {"type": "paragraph", "content": [{"type": "text", "text": "📎 documento.pdf"}]}
        ]});
        let images = HashMap::from([("a.png".to_owned(), "id-1".to_owned())]);
        let converted = markers_to_images(&document, &images);
        let blocks = converted["content"].as_array().expect("blocks");
        let kinds: Vec<&str> = blocks
            .iter()
            .map(|block| block["type"].as_str().unwrap_or(""))
            .collect();
        assert_eq!(
            kinds,
            vec![
                "paragraph",
                "image",
                "paragraph",
                "image",
                "bulletList",
                "paragraph"
            ]
        );
        assert_eq!(blocks[1]["attrs"]["attachmentId"], "id-1");
        assert_eq!(blocks[2]["content"][0]["text"], " dopo");
        let item = &blocks[4]["content"][0]["content"];
        assert_eq!(
            item[0]["type"], "paragraph",
            "il primo paragrafo della voce resta"
        );
        assert_eq!(item[1]["type"], "image");
        crate::domain::note::validate_document(&converted).expect("valid document");
        assert_eq!(
            blocks[5]["content"][0]["text"], "📎 documento.pdf",
            "solo le immagini"
        );
    }

    #[test]
    fn percent_decoding_is_lenient() {
        assert_eq!(percent_decode("a%20b%C3%A8"), "a bè");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }
}
