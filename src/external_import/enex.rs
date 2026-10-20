//! Evernote: file ENEX (XML) esportati da Evernote, uno per taccuino.
//!
//! Il file viene letto in streaming con `quick-xml`, che non risolve DTD né
//! entità esterne. Il contenuto ENML (XHTML) passa dalla stessa conversione
//! sanificata dell'importazione HTML; gli allegati (`<resource>`) sono
//! collegati al testo tramite l'hash MD5 indicato da `<en-media>`.

use std::{
    collections::HashMap,
    fs::{self, File},
    io::BufReader,
    path::{Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use md5::{Digest, Md5};
use quick_xml::{Reader, events::Event};

use super::{ExternalNote, attachment_marker, finish, html_with_title};
use crate::{
    content::escape_html,
    storage::{MAX_ATTACHMENT_BYTES, StorageError},
    transfer::{
        Collection, ImportPlan, ImportedAttachment, MAX_IMPORT_FILES, MAX_IMPORT_TOTAL_BYTES,
        ParsedNote, PlannedNote, display_name,
    },
};

/// Contenuto ENML massimo di una nota.
const MAX_ENML_BYTES: usize = 8 * 1024 * 1024;
/// Base64 di un allegato al limite di 25 MiB, con margine per gli a capo.
const MAX_RESOURCE_BASE64: usize = MAX_ATTACHMENT_BYTES / 3 * 4 * 11 / 10 + 4096;
const MAX_TAGS: usize = 100;
const MAX_RESOURCES: usize = 200;

#[derive(Default)]
struct RawResource {
    data: String,
    mime: String,
    file_name: String,
    too_large: bool,
}

#[derive(Default)]
struct RawNote {
    title: String,
    content: String,
    created: String,
    updated: String,
    tags: Vec<String>,
    resources: Vec<RawResource>,
    content_too_large: bool,
}

/// Piano per uno o più file ENEX: ogni file diventa un taccuino con il suo
/// nome (Evernote esporta un file per taccuino).
pub fn plan_enex_import(paths: &[PathBuf]) -> Result<ImportPlan, StorageError> {
    let mut files = Vec::new();
    let mut budget = MAX_IMPORT_TOTAL_BYTES;
    for path in paths {
        let notebook = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("Evernote")
            .to_owned();
        let source = display_name(path, path.parent().unwrap_or(Path::new(".")));
        match read_enex(path, &notebook, &source, &mut budget, &mut files) {
            Ok(()) => {}
            Err(message) => files.push(PlannedNote {
                source_name: source,
                outcome: Err(message),
            }),
        }
        if files.len() > MAX_IMPORT_FILES {
            return Err(StorageError::InvalidImport(format!(
                "puoi importare al massimo {MAX_IMPORT_FILES} note alla volta"
            )));
        }
    }
    Ok(ImportPlan::from_collection(
        files,
        Collection {
            kind: "evernote",
            label: "Evernote".to_owned(),
        },
    ))
}

fn read_enex(
    path: &Path,
    notebook: &str,
    source: &str,
    budget: &mut u64,
    files: &mut Vec<PlannedNote>,
) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "file non leggibile".to_owned())?;
    if !metadata.is_file() {
        return Err("i collegamenti simbolici e i file speciali non sono importati".to_owned());
    }
    let file = File::open(path).map_err(|_| "file non leggibile".to_owned())?;
    let mut reader = Reader::from_reader(BufReader::new(file));
    reader.config_mut().trim_text(false);
    let mut buffer = Vec::new();
    let mut stack: Vec<String> = Vec::new();
    let mut note: Option<RawNote> = None;
    let mut seen_export = false;
    let mut count = 0usize;
    loop {
        let event = reader.read_event_into(&mut buffer).map_err(|error| {
            format!(
                "file ENEX non valido (posizione {}): {error}",
                reader.buffer_position()
            )
        })?;
        match event {
            Event::Start(element) => {
                let name = element.local_name().as_ref().to_owned();
                match name.as_str() {
                    "en-export" => seen_export = true,
                    "note" if stack.last().is_some_and(|parent| parent == "en-export") => {
                        note = Some(RawNote::default());
                    }
                    "resource" => {
                        if let Some(note) = note.as_mut()
                            && note.resources.len() < MAX_RESOURCES
                        {
                            note.resources.push(RawResource::default());
                        }
                    }
                    "tag" => {
                        if let Some(note) = note.as_mut()
                            && stack.last().is_some_and(|parent| parent == "note")
                            && note.tags.len() < MAX_TAGS
                        {
                            note.tags.push(String::new());
                        }
                    }
                    _ => {}
                }
                stack.push(name);
            }
            Event::End(_) => {
                let name = stack.pop().unwrap_or_default();
                if name == "note"
                    && stack.last().is_some_and(|parent| parent == "en-export")
                    && let Some(raw) = note.take()
                {
                    count += 1;
                    let title = raw.title.trim().to_owned();
                    let source_name = format!(
                        "{source} › {}",
                        if title.is_empty() {
                            format!("nota {count}")
                        } else {
                            title.clone()
                        }
                    );
                    files.push(PlannedNote {
                        source_name,
                        outcome: build_note(raw, notebook, budget),
                    });
                    if files.len() > MAX_IMPORT_FILES {
                        return Ok(());
                    }
                }
            }
            Event::Text(text) => append(&stack, note.as_mut(), &text.into_inner()),
            Event::CData(data) => append(&stack, note.as_mut(), &data.into_inner()),
            Event::GeneralRef(reference) => {
                let resolved = match reference.resolve_char_ref() {
                    Ok(Some(character)) => Some(character.to_string()),
                    _ => match reference.into_inner().as_ref() {
                        "amp" => Some("&".to_owned()),
                        "lt" => Some("<".to_owned()),
                        "gt" => Some(">".to_owned()),
                        "quot" => Some("\"".to_owned()),
                        "apos" => Some("'".to_owned()),
                        // Entità sconosciute non vengono mai risolte.
                        _ => None,
                    },
                };
                if let Some(text) = resolved {
                    append(&stack, note.as_mut(), &text);
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    if !seen_export {
        return Err("non è un file ENEX di Evernote".to_owned());
    }
    if count == 0 {
        return Err("il file ENEX non contiene note".to_owned());
    }
    Ok(())
}

/// Accoda il testo all'elemento corrente della nota.
fn append(stack: &[String], note: Option<&mut RawNote>, text: &str) {
    let Some(note) = note else {
        return;
    };
    let tail: Vec<&str> = stack.iter().rev().take(3).map(String::as_str).collect();
    match tail.as_slice() {
        ["title", "note", ..] => push_limited(&mut note.title, text, 4096),
        ["created", "note", ..] => push_limited(&mut note.created, text, 64),
        ["updated", "note", ..] => push_limited(&mut note.updated, text, 64),
        ["tag", "note", ..] => {
            if let Some(tag) = note.tags.last_mut() {
                push_limited(tag, text, 400);
            }
        }
        ["content", "note", ..] => {
            if note.content.len() + text.len() > MAX_ENML_BYTES {
                note.content_too_large = true;
            } else {
                note.content.push_str(text);
            }
        }
        ["data", "resource", ..] => {
            if let Some(resource) = note.resources.last_mut() {
                if resource.data.len() + text.len() > MAX_RESOURCE_BASE64 {
                    resource.too_large = true;
                    resource.data.clear();
                } else if !resource.too_large {
                    resource.data.push_str(text);
                }
            }
        }
        ["mime", "resource", ..] => {
            if let Some(resource) = note.resources.last_mut() {
                push_limited(&mut resource.mime, text, 200);
            }
        }
        ["file-name", "resource-attributes", "resource"] => {
            if let Some(resource) = note.resources.last_mut() {
                push_limited(&mut resource.file_name, text, 400);
            }
        }
        _ => {}
    }
}

fn push_limited(target: &mut String, text: &str, max: usize) {
    if target.len() + text.len() <= max {
        target.push_str(text);
    }
}

/// `20240115T093000Z` → `2024-01-15T09:30:00Z`.
fn enex_timestamp(value: &str) -> Option<String> {
    let value = value.trim();
    let bytes = value.as_bytes();
    if bytes.len() != 16 || bytes[8] != b'T' || bytes[15] != b'Z' {
        return None;
    }
    let digits = |range: std::ops::Range<usize>| {
        value
            .get(range)
            .filter(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
    };
    Some(format!(
        "{}-{}-{}T{}:{}:{}Z",
        digits(0..4)?,
        digits(4..6)?,
        digits(6..8)?,
        digits(9..11)?,
        digits(11..13)?,
        digits(13..15)?
    ))
}

fn extension_for(mime: &str) -> &'static str {
    match mime.trim() {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "application/pdf" => "pdf",
        "text/plain" => "txt",
        _ => "bin",
    }
}

fn build_note(raw: RawNote, notebook: &str, budget: &mut u64) -> Result<ParsedNote, String> {
    if raw.content_too_large {
        return Err("il contenuto della nota supera 8 MiB".to_owned());
    }
    let title = if raw.title.trim().is_empty() {
        "Nota senza titolo".to_owned()
    } else {
        raw.title.trim().to_owned()
    };
    let mut warnings = Vec::new();
    let mut attachments = Vec::new();
    let mut by_hash: HashMap<String, String> = HashMap::new();
    for (index, resource) in raw.resources.into_iter().enumerate() {
        let name = if resource.file_name.trim().is_empty() {
            format!("allegato-{}.{}", index + 1, extension_for(&resource.mime))
        } else {
            resource.file_name.trim().to_owned()
        };
        if resource.too_large {
            warnings.push(format!("allegato oltre 25 MiB ignorato: {name}"));
            continue;
        }
        let compact: String = resource
            .data
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        let Ok(bytes) = BASE64.decode(compact.as_bytes()) else {
            warnings.push(format!("allegato non decodificabile ignorato: {name}"));
            continue;
        };
        let hash = format!("{:x}", Md5::digest(&bytes));
        by_hash.insert(hash, name.clone());
        attachments.push(ImportedAttachment {
            filename: name,
            bytes,
        });
    }
    let html = enml_to_html(&raw.content, &by_hash, &mut warnings);
    let mut note = ExternalNote::new("evernote", title.clone(), html_with_title(&title, &html));
    note.tags = raw.tags;
    note.created_at = enex_timestamp(&raw.created);
    note.updated_at = enex_timestamp(&raw.updated).or_else(|| note.created_at.clone());
    note.attachments = attachments;
    note.notebook_path = vec![notebook.to_owned()];
    note.warnings = warnings;
    finish(note, budget)
}

/// Valore di un attributo in un tag ENML (`hash="…"`).
fn attribute(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut search = 0;
    while let Some(found) = lower[search..].find(name) {
        let start = search + found;
        let before_ok = start == 0 || lower.as_bytes()[start - 1].is_ascii_whitespace();
        let rest = &tag[start + name.len()..];
        let rest_trimmed = rest.trim_start();
        if before_ok && let Some(value) = rest_trimmed.strip_prefix('=') {
            let value = value.trim_start();
            let quote = value.chars().next()?;
            if quote == '"' || quote == '\'' {
                let inner = &value[1..];
                return inner.find(quote).map(|end| inner[..end].to_owned());
            }
        }
        search = start + name.len();
    }
    None
}

/// Sostituisce gli elementi propri di Evernote con HTML comune: allegati con
/// un segnaposto, caselle `en-todo` con ☐/☑, testo cifrato con un avviso.
fn enml_to_html(
    enml: &str,
    by_hash: &HashMap<String, String>,
    warnings: &mut Vec<String>,
) -> String {
    let mut output = String::with_capacity(enml.len());
    let mut rest = enml;
    while let Some(start) = rest.find("<en-") {
        output.push_str(&rest[..start]);
        let after = &rest[start..];
        let Some(end) = after.find('>') else {
            rest = "";
            break;
        };
        let tag = &after[..=end];
        let name: String = tag[1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        let mut consumed = end + 1;
        match name.as_str() {
            "en-media" => {
                let label = attribute(tag, "hash")
                    .and_then(|hash| by_hash.get(&hash.to_ascii_lowercase()).cloned())
                    .unwrap_or_else(|| "allegato".to_owned());
                output.push_str(&format!(
                    "<span> {} </span>",
                    escape_html(&attachment_marker(&label))
                ));
                if let Some(close) = after[consumed..].trim_start().strip_prefix("</en-media>") {
                    consumed = after.len() - close.len();
                }
            }
            "en-todo" => {
                let checked = attribute(tag, "checked").is_some_and(|value| value == "true");
                output.push_str(if checked { "☑ " } else { "☐ " });
                if let Some(close) = after[consumed..].trim_start().strip_prefix("</en-todo>") {
                    consumed = after.len() - close.len();
                }
            }
            "en-crypt" => {
                warnings.push("testo cifrato di Evernote non importato".to_owned());
                output.push_str("<p>[testo cifrato di Evernote non importato]</p>");
                consumed = after
                    .find("</en-crypt>")
                    .map_or(after.len(), |close| close + "</en-crypt>".len());
            }
            // `en-note` e gli altri elementi restano e vengono trattati come
            // contenitori dalla conversione HTML.
            _ => output.push_str(tag),
        }
        rest = &after[consumed..];
    }
    output.push_str(rest);
    output
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::content::to_plain_text;

    const PNG: &[u8] = b"\x89PNG\r\n\x1a\nfinto";

    pub(crate) fn sample_enex() -> String {
        let data = BASE64.encode(PNG);
        let hash = format!("{:x}", Md5::digest(PNG));
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE en-export SYSTEM "http://xml.evernote.com/pub/evernote-export4.dtd">
<en-export export-date="20240101T000000Z" application="Evernote">
  <note>
    <title>Riunione &amp; piano</title>
    <created>20240115T093000Z</created>
    <updated>20240116T100000Z</updated>
    <tag>lavoro</tag>
    <tag>Riunioni</tag>
    <content><![CDATA[<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE en-note SYSTEM "http://xml.evernote.com/pub/enml2.dtd">
<en-note><h1>Riunione &amp; piano</h1><div><en-todo checked="true"/>Fatto</div><div><en-todo/>Da fare</div><div>Schema: <en-media hash="{hash}" type="image/png"/></div><script>alert(1)</script><en-crypt>abc</en-crypt></en-note>]]></content>
    <resource>
      <data encoding="base64">
{data}
      </data>
      <mime>image/png</mime>
      <resource-attributes><file-name>schema.png</file-name></resource-attributes>
    </resource>
  </note>
  <note>
    <title>Seconda</title>
    <content><![CDATA[<en-note><p>Testo</p></en-note>]]></content>
  </note>
</en-export>"#
        )
    }

    #[test]
    fn parses_notes_resources_todos_and_dates() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("Lavoro.enex");
        fs::write(&path, sample_enex()).expect("write");
        let plan = plan_enex_import(&[path]).expect("plan");
        assert_eq!(plan.files.len(), 2);
        let first = plan.files[0].outcome.as_ref().expect("first note");
        assert_eq!(first.title, "Riunione & piano");
        assert_eq!(first.notebook_path, vec!["Lavoro"]);
        assert_eq!(first.tags, vec!["lavoro", "Riunioni"]);
        assert_eq!(first.created_at.as_deref(), Some("2024-01-15T09:30:00Z"));
        assert_eq!(first.updated_at.as_deref(), Some("2024-01-16T10:00:00Z"));
        assert_eq!(first.attachments.len(), 1);
        assert_eq!(first.attachments[0].filename, "schema.png");
        let text = to_plain_text(&first.document);
        assert!(text.contains("☑ Fatto"), "{text}");
        assert!(text.contains("☐ Da fare"), "{text}");
        assert!(text.contains("📎 schema.png"), "{text}");
        assert!(!text.contains("alert"), "{text}");
        assert!(
            !text.contains("Riunione & piano"),
            "titolo duplicato: {text}"
        );
        assert!(
            first
                .warnings
                .iter()
                .any(|warning| warning.contains("cifrato"))
        );
        assert_eq!(
            plan.collection.as_ref().expect("collection").kind,
            "evernote"
        );
    }

    #[test]
    fn rejects_files_that_are_not_enex() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("altro.enex");
        fs::write(&path, "<html><body>no</body></html>").expect("write");
        let plan = plan_enex_import(std::slice::from_ref(&path)).expect("plan");
        assert!(plan.files[0].outcome.is_err());
        fs::write(&path, "<en-export><note><title>x").expect("write");
        let plan = plan_enex_import(&[path]).expect("plan");
        assert!(plan.files.iter().all(|note| note.outcome.is_err()));
    }

    #[test]
    fn external_entities_are_never_resolved() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("xxe.enex");
        fs::write(
            &path,
            r#"<?xml version="1.0"?><!DOCTYPE en-export [<!ENTITY xxe SYSTEM "file:///etc/passwd">]>
<en-export><note><title>A &xxe; B</title><content><![CDATA[<en-note>ok</en-note>]]></content></note></en-export>"#,
        )
        .expect("write");
        let plan = plan_enex_import(&[path]).expect("plan");
        let note = plan.files[0].outcome.as_ref().expect("note");
        assert_eq!(note.title, "A  B");
    }
}
