//! Joplin: archivio JEX ("Esporta › JEX - Joplin Export File").
//!
//! Il JEX è un archivio tar con un file `<id>.md` per ogni elemento (nota,
//! cartella, tag, risorsa, associazione nota-tag) e i file delle risorse in
//! `resources/`. Ogni elemento ha titolo, corpo e in coda le proprietà
//! `chiave: valore` (fra cui `type_`). L'archivio viene letto in memoria
//! entro i limiti dell'importazione; nulla viene estratto su disco.

use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::Read,
    path::Path,
};

use super::{
    ExternalNote, MarkdownLink, Rewrite, attachment_marker, finish, html_with_title,
    markdown_with_title, rewrite_markdown,
};
use crate::{
    content::escape_html,
    storage::{MAX_ATTACHMENT_BYTES, StorageError},
    transfer::{
        Collection, ImportPlan, ImportedAttachment, MAX_IMPORT_FILE_BYTES, MAX_IMPORT_FILES,
        MAX_IMPORT_TOTAL_BYTES, ParsedNote, PlannedLink, PlannedNote,
    },
};

const TYPE_NOTE: u32 = 1;
const TYPE_FOLDER: u32 = 2;
const TYPE_RESOURCE: u32 = 4;
const TYPE_TAG: u32 = 5;
const TYPE_NOTE_TAG: u32 = 6;
const MAX_ENTRIES: usize = 200_000;
const MAX_FOLDER_DEPTH: usize = 16;

#[derive(Debug, Default)]
struct Item {
    title: String,
    body: String,
    props: HashMap<String, String>,
}

impl Item {
    fn prop(&self, key: &str) -> Option<&str> {
        self.props
            .get(key)
            .map(String::as_str)
            .filter(|value| !value.is_empty())
    }

    fn kind(&self) -> Option<u32> {
        self.prop("type_")?.parse().ok()
    }
}

/// Elemento serializzato: `titolo\n\ncorpo\n\nchiave: valore…`.
fn parse_item(text: &str) -> Option<Item> {
    let text = text.replace("\r\n", "\n");
    let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
    let mut props = HashMap::new();
    let mut body_end = lines.len();
    for (index, line) in lines.iter().enumerate().rev() {
        if line.is_empty() {
            body_end = index;
            break;
        }
        let (key, value) = line.split_once(':')?;
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_lowercase() || c == '_') {
            return None;
        }
        props.insert(
            key.to_owned(),
            value
                .strip_prefix(' ')
                .unwrap_or(value)
                .replace("\\n", "\n"),
        );
        body_end = index;
    }
    if !props.contains_key("type_") {
        return None;
    }
    let body_lines = &lines[..body_end];
    let title = body_lines.first().copied().unwrap_or_default().to_owned();
    // Dopo il titolo c'è una riga vuota di separazione.
    let body = body_lines.get(2..).unwrap_or_default().join("\n");
    Some(Item { title, body, props })
}

/// Id Joplin di 32 caratteri esadecimali all'inizio di `:/id…`.
fn joplin_id(target: &str) -> Option<&str> {
    let id = target.strip_prefix(":/")?.get(..32)?;
    id.chars().all(|c| c.is_ascii_hexdigit()).then_some(id)
}

struct Archive {
    items: HashMap<String, Item>,
    blobs: HashMap<String, Vec<u8>>,
}

fn read_archive(path: &Path) -> Result<Archive, StorageError> {
    let invalid = |message: &str| StorageError::InvalidImport(message.to_owned());
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(invalid("il file selezionato non è valido"));
    }
    let mut archive = tar::Archive::new(File::open(path)?);
    let mut items = HashMap::new();
    let mut blobs = HashMap::new();
    let mut total: u64 = 0;
    let entries = archive
        .entries()
        .map_err(|_| invalid("il file non è un archivio JEX di Joplin"))?;
    for (index, entry) in entries.enumerate() {
        if index >= MAX_ENTRIES {
            return Err(invalid("l'archivio contiene troppi elementi"));
        }
        let entry = entry.map_err(|_| invalid("archivio JEX danneggiato"))?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let entry_path = entry
            .path()
            .map_err(|_| invalid("archivio JEX danneggiato"))?
            .into_owned();
        let size = entry.size();
        let name = entry_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned();
        let in_resources = entry_path
            .parent()
            .and_then(|parent| parent.file_name())
            .is_some_and(|parent| parent == "resources");
        let limit = if in_resources {
            MAX_ATTACHMENT_BYTES as u64
        } else {
            MAX_IMPORT_FILE_BYTES
        };
        if size > limit {
            // Le risorse troppo grandi vengono segnalate sulla nota.
            continue;
        }
        total += size;
        if total > MAX_IMPORT_TOTAL_BYTES {
            return Err(invalid(
                "l'archivio supera 512 MiB: esporta da Joplin un taccuino alla volta",
            ));
        }
        let mut bytes = Vec::with_capacity(size as usize);
        entry
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| invalid("archivio JEX danneggiato"))?;
        let stem = name.split('.').next().unwrap_or_default().to_owned();
        if in_resources {
            blobs.insert(stem, bytes);
        } else if name.ends_with(".md")
            && let Ok(text) = String::from_utf8(bytes)
            && let Some(item) = parse_item(&text)
        {
            let id = item.prop("id").unwrap_or(&stem).to_owned();
            items.insert(id, item);
        }
    }
    if items.is_empty() {
        return Err(invalid("il file non è un archivio JEX di Joplin"));
    }
    Ok(Archive { items, blobs })
}

/// Piano per un archivio JEX: cartelle (anche annidate), tag, risorse e
/// collegamenti tra note.
pub fn plan_jex_import(path: &Path) -> Result<ImportPlan, StorageError> {
    let Archive { items, blobs } = read_archive(path)?;
    let of_kind = |kind: u32| {
        items
            .iter()
            .filter(move |(_, item)| item.kind() == Some(kind))
    };
    let folders: HashMap<&str, &Item> = of_kind(TYPE_FOLDER)
        .map(|(id, item)| (id.as_str(), item))
        .collect();
    let tags: HashMap<&str, &str> = of_kind(TYPE_TAG)
        .map(|(id, item)| (id.as_str(), item.title.as_str()))
        .collect();
    let resources: HashMap<&str, &Item> = of_kind(TYPE_RESOURCE)
        .map(|(id, item)| (id.as_str(), item))
        .collect();
    let mut note_tags: HashMap<&str, Vec<&str>> = HashMap::new();
    for (_, link) in of_kind(TYPE_NOTE_TAG) {
        if let (Some(note), Some(tag)) = (link.prop("note_id"), link.prop("tag_id"))
            && let Some(name) = tags.get(tag)
        {
            note_tags.entry(note).or_default().push(name);
        }
    }
    let note_ids: HashSet<&str> = of_kind(TYPE_NOTE).map(|(id, _)| id.as_str()).collect();
    let mut notes: Vec<(&String, &Item)> = of_kind(TYPE_NOTE).collect();
    notes.sort_by(|(_, a), (_, b)| {
        folder_path(&folders, a.prop("parent_id"))
            .cmp(&folder_path(&folders, b.prop("parent_id")))
            .then_with(|| a.title.cmp(&b.title))
    });
    if notes.len() > MAX_IMPORT_FILES {
        return Err(StorageError::InvalidImport(format!(
            "l'archivio contiene più di {MAX_IMPORT_FILES} note: esporta un taccuino alla volta"
        )));
    }

    let mut budget = MAX_IMPORT_TOTAL_BYTES;
    let mut files = Vec::new();
    for (id, item) in notes {
        let path = folder_path(&folders, item.prop("parent_id"));
        let source_name = if path.is_empty() {
            item.title.clone()
        } else {
            format!("{} › {}", path.join(" › "), item.title)
        };
        let outcome = build_note(
            id,
            item,
            path,
            note_tags.get(id.as_str()).cloned().unwrap_or_default(),
            &resources,
            &blobs,
            &note_ids,
            &mut budget,
        );
        files.push(PlannedNote {
            source_name,
            outcome,
        });
    }
    Ok(ImportPlan::from_collection(
        files,
        Collection {
            kind: "joplin",
            label: "Joplin".to_owned(),
        },
    ))
}

fn folder_path(folders: &HashMap<&str, &Item>, parent: Option<&str>) -> Vec<String> {
    let mut path = Vec::new();
    let mut current = parent;
    let mut visited = HashSet::new();
    while let Some(id) = current {
        if path.len() >= MAX_FOLDER_DEPTH || !visited.insert(id) {
            break;
        }
        let Some(folder) = folders.get(id) else {
            break;
        };
        path.push(folder.title.trim().to_owned());
        current = folder.prop("parent_id");
    }
    path.reverse();
    path
}

#[allow(clippy::too_many_arguments)]
fn build_note(
    id: &str,
    item: &Item,
    notebook_path: Vec<String>,
    tags: Vec<&str>,
    resources: &HashMap<&str, &Item>,
    blobs: &HashMap<String, Vec<u8>>,
    note_ids: &HashSet<&str>,
    budget: &mut u64,
) -> Result<ParsedNote, String> {
    let title = if item.title.trim().is_empty() {
        "Nota senza titolo".to_owned()
    } else {
        item.title.trim().to_owned()
    };
    let mut warnings = Vec::new();
    let mut attachments: Vec<ImportedAttachment> = Vec::new();
    let mut attached: HashSet<String> = HashSet::new();
    let mut links: Vec<PlannedLink> = Vec::new();
    // Restituisce il nome dell'allegato per una risorsa citata nel testo.
    let mut attach = |resource_id: &str,
                      warnings: &mut Vec<String>,
                      attachments: &mut Vec<ImportedAttachment>|
     -> Option<String> {
        let resource = resources.get(resource_id)?;
        let extension = resource.prop("file_extension").unwrap_or("bin");
        let name = resource
            .prop("filename")
            .or(Some(resource.title.trim()).filter(|title| !title.is_empty()))
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{resource_id}.{extension}"));
        if attached.insert(resource_id.to_owned()) {
            match blobs.get(resource_id) {
                Some(bytes) => attachments.push(ImportedAttachment {
                    filename: name.clone(),
                    bytes: bytes.clone(),
                }),
                None => warnings.push(format!("risorsa mancante o troppo grande: {name}")),
            }
        }
        Some(name)
    };
    let html = item.prop("markup_language") == Some("2");
    let document = if html {
        let body = replace_html_resources(&item.body, &mut |resource_id| {
            attach(resource_id, &mut warnings, &mut attachments)
        });
        for target in referenced_ids(&item.body) {
            if note_ids.contains(target) && target != id {
                links.push(PlannedLink {
                    target_key: target.to_owned(),
                    label: None,
                });
            }
        }
        html_with_title(&title, &body)
    } else {
        let body = rewrite_markdown(
            &item.body,
            &mut |link: MarkdownLink<'_>| {
                let Some(target) = joplin_id(link.target) else {
                    return Rewrite::Keep;
                };
                if note_ids.contains(target) {
                    if target != id {
                        links.push(PlannedLink {
                            target_key: target.to_owned(),
                            label: Some(link.label.trim().to_owned()).filter(|l| !l.is_empty()),
                        });
                    }
                    return Rewrite::Replace(link.label.to_owned());
                }
                match attach(target, &mut warnings, &mut attachments) {
                    Some(name) => Rewrite::Replace(attachment_marker(&name)),
                    None => Rewrite::Replace(link.label.to_owned()),
                }
            },
            None,
        );
        markdown_with_title(&title, &body)
    };
    let mut note = ExternalNote::new("joplin", title, document);
    note.tags = tags.into_iter().map(str::to_owned).collect();
    note.created_at = item
        .prop("user_created_time")
        .or(item.prop("created_time"))
        .map(str::to_owned);
    note.updated_at = item
        .prop("user_updated_time")
        .or(item.prop("updated_time"))
        .map(str::to_owned);
    note.attachments = attachments;
    note.notebook_path = notebook_path;
    note.link_keys = vec![id.to_owned()];
    note.links = links;
    note.warnings = warnings;
    if item.prop("is_todo") == Some("1") {
        note.warnings
            .push("attività di Joplin importata come nota".to_owned());
    }
    finish(note, budget)
}

/// Id Joplin citati nel testo (`:/id`).
fn referenced_ids(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(index) = rest.find(":/") {
        if let Some(id) = joplin_id(&rest[index..]) {
            found.push(id);
        }
        rest = &rest[index + 2..];
    }
    found
}

/// Nelle note HTML, le immagini `<img src=":/id">` diventano segnaposto.
fn replace_html_resources(html: &str, attach: &mut dyn FnMut(&str) -> Option<String>) -> String {
    let mut output = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.to_ascii_lowercase().find("<img") {
        output.push_str(&rest[..start]);
        let after = &rest[start..];
        let end = after.find('>').map_or(after.len(), |end| end + 1);
        let tag = &after[..end];
        let resource = tag
            .find(":/")
            .and_then(|index| joplin_id(&tag[index..]))
            .and_then(&mut *attach);
        match resource {
            Some(name) => output.push_str(&format!(
                "<span> {} </span>",
                escape_html(&attachment_marker(&name))
            )),
            None => output.push_str(tag),
        }
        rest = &after[end..];
    }
    output.push_str(rest);
    output
}

/// Usato dai test per costruire archivi JEX sintetici.
#[cfg(test)]
pub(crate) fn write_test_jex(path: &Path, entries: &[(&str, &[u8])]) {
    let mut builder = tar::Builder::new(File::create(path).expect("create"));
    for (name, bytes) in entries {
        let mut header = tar::Header::new_ustar();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder
            .append_data(&mut header, name, *bytes)
            .expect("append");
    }
    builder.finish().expect("finish");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::to_plain_text;

    const FOLDER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1";
    const CHILD: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa2";
    const NOTE: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb1";
    const OTHER: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb2";
    const RESOURCE: &str = "ccccccccccccccccccccccccccccccc1";
    const TAG: &str = "ddddddddddddddddddddddddddddddd1";

    fn sample(path: &Path) {
        let folder = format!("Lavoro\n\nid: {FOLDER}\nparent_id: \ntype_: 2");
        let child = format!("Progetti\n\nid: {CHILD}\nparent_id: {FOLDER}\ntype_: 2");
        let note = format!(
            "Piano\n\n# Piano\n\nVedi [l'altra](:/{OTHER}) e ![schema](:/{RESOURCE}).\n\n```\n[codice](:/{OTHER})\n```\n\nid: {NOTE}\nparent_id: {CHILD}\ncreated_time: 2023-05-01T10:00:00.000Z\nupdated_time: 2023-05-02T10:00:00.000Z\nuser_created_time: 2023-04-30T09:00:00.000Z\nuser_updated_time: 2023-05-02T11:00:00.000Z\nmarkup_language: 1\nis_todo: 0\ntype_: 1"
        );
        let other = format!(
            "Altra\n\n<p>Testo <b>HTML</b> <img src=\":/{RESOURCE}\"/></p>\n\nid: {OTHER}\nparent_id: \nmarkup_language: 2\ntype_: 1"
        );
        let resource = format!(
            "schema.png\n\nid: {RESOURCE}\nmime: image/png\nfilename: schema.png\nfile_extension: png\ntype_: 4"
        );
        let tag = format!("progetto\n\nid: {TAG}\ntype_: 5");
        let note_tag = format!(
            "\n\nid: eeeeeeeeeeeeeeeeeeeeeeeeeeeeeee1\nnote_id: {NOTE}\ntag_id: {TAG}\ntype_: 6"
        );
        let png = b"\x89PNG\r\n\x1a\nfinto".to_vec();
        write_test_jex(
            path,
            &[
                (&format!("{FOLDER}.md"), folder.as_bytes()),
                (&format!("{CHILD}.md"), child.as_bytes()),
                (&format!("{NOTE}.md"), note.as_bytes()),
                (&format!("{OTHER}.md"), other.as_bytes()),
                (&format!("{RESOURCE}.md"), resource.as_bytes()),
                (&format!("{TAG}.md"), tag.as_bytes()),
                ("eeeeeeeeeeeeeeeeeeeeeeeeeeeeeee1.md", note_tag.as_bytes()),
                (&format!("resources/{RESOURCE}.png"), &png),
            ],
        );
    }

    #[test]
    fn parses_items_with_trailing_properties() {
        let item = parse_item("Titolo\n\nRiga 1\n\nRiga 2\n\nid: abc\ntype_: 1\n").expect("item");
        assert_eq!(item.title, "Titolo");
        assert_eq!(item.body, "Riga 1\n\nRiga 2");
        assert_eq!(item.prop("id"), Some("abc"));
        assert!(parse_item("solo testo\nsenza proprietà").is_none());
    }

    #[test]
    fn imports_folders_tags_resources_and_links() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("export.jex");
        sample(&path);
        let plan = plan_jex_import(&path).expect("plan");
        assert_eq!(plan.files.len(), 2);
        let notes: HashMap<String, &ParsedNote> = plan
            .files
            .iter()
            .map(|planned| {
                let parsed = planned.outcome.as_ref().expect("note");
                (parsed.title.clone(), parsed)
            })
            .collect();
        let piano = notes["Piano"];
        assert_eq!(piano.notebook_path, vec!["Lavoro", "Progetti"]);
        assert_eq!(piano.tags, vec!["progetto"]);
        assert_eq!(
            piano.created_at.as_deref(),
            Some("2023-04-30T09:00:00.000Z")
        );
        assert_eq!(piano.attachments.len(), 1);
        assert_eq!(piano.links.len(), 1);
        assert_eq!(piano.links[0].target_key, OTHER);
        let text = to_plain_text(&piano.document);
        assert!(text.contains("Vedi l'altra e 📎 schema.png."), "{text}");
        assert!(text.contains(&format!("[codice](:/{OTHER})")), "{text}");
        assert!(!text.starts_with("Piano"), "{text}");

        let altra = notes["Altra"];
        assert!(altra.notebook_path.is_empty());
        assert_eq!(altra.attachments.len(), 1);
        assert!(to_plain_text(&altra.document).contains("📎 schema.png"));
        assert_eq!(altra.link_keys, vec![OTHER]);
    }

    #[test]
    fn rejects_non_jex_files() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("finto.jex");
        fs::write(&path, b"non un archivio").expect("write");
        assert!(plan_jex_import(&path).is_err());
        let empty = directory.path().join("vuoto.jex");
        write_test_jex(&empty, &[("readme.txt", b"ciao")]);
        assert!(plan_jex_import(&empty).is_err());
    }
}
