//! Obsidian: cartella di un vault (file Markdown e allegati).
//!
//! Le cartelle diventano taccuini, il nome del file è il titolo. Vengono
//! riconosciuti il front matter YAML (`tags`, `aliases`, `created`,
//! `updated`), i `#tag` nel testo, i collegamenti `[[nota]]` e gli
//! incorporamenti `![[file]]`. Le cartelle nascoste (`.obsidian`, `.trash`)
//! sono ignorate; gli allegati vengono letti solo dentro il vault e senza
//! seguire collegamenti simbolici.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
};

use super::{
    ExternalNote, MarkdownLink, Rewrite, WikiLink, attachment_marker, finish, markdown_with_title,
    percent_decode, rewrite_markdown, utc_from_system_time,
};
use crate::{
    content::is_utc_timestamp,
    storage::{MAX_ATTACHMENT_BYTES, StorageError},
    transfer::{
        Collection, ImportPlan, ImportedAttachment, MAX_FOLDER_DEPTH, MAX_IMPORT_FILE_BYTES,
        MAX_IMPORT_FILES, MAX_IMPORT_TOTAL_BYTES, ParsedNote, PlannedLink, PlannedNote,
        portable_path, read_limited,
    },
};

/// File del vault, con indici per nome e per percorso relativo.
struct Vault {
    root: PathBuf,
    notes: Vec<PathBuf>,
    /// Nome file in minuscolo → percorsi (per `![[immagine.png]]`).
    by_name: HashMap<String, Vec<PathBuf>>,
    /// Percorso o nome di una nota (minuscolo, senza `.md`) → chiave
    /// canonica (percorso relativo).
    note_keys: HashMap<String, String>,
}

fn scan_vault(root: &Path) -> Result<Vault, StorageError> {
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() {
        return Err(StorageError::InvalidImport(
            "la cartella selezionata non è valida".to_owned(),
        ));
    }
    let mut notes = Vec::new();
    let mut by_name: HashMap<String, Vec<PathBuf>> = HashMap::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut files = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        let mut entries = fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            // `file_type` non segue i collegamenti simbolici: vengono ignorati.
            let file_type = entry.file_type()?;
            let path = entry.path();
            if file_type.is_dir() {
                if depth < MAX_FOLDER_DEPTH {
                    pending.push((path, depth + 1));
                }
            } else if file_type.is_file() {
                files += 1;
                if files > MAX_IMPORT_FILES * 4 {
                    return Err(StorageError::InvalidImport(
                        "il vault contiene troppi file: importa una cartella alla volta".to_owned(),
                    ));
                }
                if name.to_ascii_lowercase().ends_with(".md") {
                    notes.push(path.clone());
                }
                by_name.entry(name.to_lowercase()).or_default().push(path);
            }
        }
    }
    if notes.len() > MAX_IMPORT_FILES {
        return Err(StorageError::InvalidImport(format!(
            "il vault contiene più di {MAX_IMPORT_FILES} note: importa una cartella alla volta"
        )));
    }
    notes.sort();
    let mut note_keys = HashMap::new();
    for note in &notes {
        let relative = note.strip_prefix(root).unwrap_or(note);
        let key = note_key(relative);
        note_keys.insert(key.clone(), key.clone());
        if let Some(stem) = key.rsplit('/').next() {
            note_keys
                .entry(stem.to_owned())
                .or_insert_with(|| key.clone());
        }
    }
    Ok(Vault {
        root: root.to_path_buf(),
        notes,
        by_name,
        note_keys,
    })
}

/// Chiave di collegamento: percorso relativo senza `.md`, in minuscolo, con
/// `/` come separatore.
fn note_key(relative: &Path) -> String {
    let text = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    let lower = text.to_lowercase();
    lower.strip_suffix(".md").unwrap_or(&lower).to_owned()
}

/// Normalizza la destinazione di un collegamento: senza `#titolo`, `^blocco`
/// ed estensione `.md`.
fn link_key(target: &str) -> String {
    let target = target.split(['#', '^']).next().unwrap_or_default().trim();
    let target = target.replace('\\', "/");
    let lower = target.trim_start_matches("./").to_lowercase();
    lower.strip_suffix(".md").unwrap_or(&lower).to_owned()
}

/// Unisce un percorso relativo alla cartella della nota, risolvendo `..`;
/// `None` se esce dal vault.
fn join_relative(base: &Path, target: &str) -> Option<String> {
    let mut parts: Vec<String> = base
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other.to_owned()),
        }
    }
    Some(parts.join("/"))
}

impl Vault {
    /// Chiave canonica di un collegamento a una nota: percorso esatto, poi
    /// relativo alla nota, poi per nome. Se non corrisponde a nessuna nota
    /// resta la chiave originale (può essere un alias).
    fn canonical_link(&self, note_relative: &Path, target: &str) -> String {
        let key = link_key(target);
        if let Some(found) = self.note_keys.get(&key) {
            return found.clone();
        }
        let base = note_relative.parent().unwrap_or(Path::new(""));
        if let Some(joined) = join_relative(base, &key)
            && let Some(found) = self.note_keys.get(&joined)
        {
            return found.clone();
        }
        key.rsplit('/')
            .next()
            .and_then(|stem| self.note_keys.get(stem))
            .cloned()
            .unwrap_or(key)
    }

    /// Risolve un file citato da una nota: percorso relativo alla nota, poi
    /// al vault, infine per nome (come fa Obsidian). Il file deve stare
    /// dentro il vault anche dopo aver risolto collegamenti simbolici.
    fn resolve_file(&self, note: &Path, reference: &str) -> Option<PathBuf> {
        let reference = percent_decode(reference.split(['#', '|']).next()?.trim());
        let reference = reference.replace('\\', "/");
        if reference.is_empty() || Path::new(&reference).is_absolute() || reference.contains(':') {
            return None;
        }
        let note_directory = note
            .parent()
            .and_then(|parent| parent.strip_prefix(&self.root).ok())
            .unwrap_or(Path::new(""));
        let candidates = [
            join_relative(note_directory, &reference),
            join_relative(Path::new(""), &reference),
        ];
        for relative in candidates.into_iter().flatten() {
            let candidate = self.root.join(&relative);
            if self.inside(&candidate) {
                return Some(candidate);
            }
        }
        let name = Path::new(&reference)
            .file_name()?
            .to_string_lossy()
            .to_lowercase();
        self.by_name
            .get(&name)?
            .iter()
            .find(|candidate| self.inside(candidate))
            .cloned()
    }

    /// File regolare (non collegamento simbolico) dentro il vault.
    fn inside(&self, candidate: &Path) -> bool {
        let regular = fs::symlink_metadata(candidate).is_ok_and(|metadata| metadata.is_file());
        let (Ok(root), Ok(file)) = (self.root.canonicalize(), candidate.canonicalize()) else {
            return false;
        };
        regular && file.starts_with(root)
    }
}

/// Front matter YAML semplice: valori su una riga, elenchi `[a, b]` o con
/// trattini, stringhe con virgolette.
#[derive(Default)]
struct FrontMatter {
    tags: Vec<String>,
    aliases: Vec<String>,
    created: Option<String>,
    updated: Option<String>,
}

fn split_front_matter(text: &str) -> (FrontMatter, &str) {
    let mut front = FrontMatter::default();
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return (front, text);
    };
    let Some(end) = rest.find("\n---") else {
        return (front, text);
    };
    let yaml = &rest[..end];
    let after = &rest[end + 4..];
    let body = after
        .strip_prefix("\r\n")
        .or_else(|| after.strip_prefix('\n'))
        .unwrap_or(after);
    let mut current: Option<String> = None;
    for line in yaml.lines().take(500) {
        let trimmed = line.trim();
        if let Some(item) = trimmed.strip_prefix("- ") {
            let value = unquote(item);
            match current.as_deref() {
                Some("tags" | "tag") => front.tags.push(value),
                Some("aliases" | "alias") => front.aliases.push(value),
                _ => {}
            }
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        let key = key.trim().to_lowercase();
        let value = value.trim();
        current = Some(key.clone());
        let list = || -> Vec<String> {
            if let Some(inner) = value.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
                inner
                    .split(',')
                    .map(unquote)
                    .filter(|v| !v.is_empty())
                    .collect()
            } else if value.is_empty() {
                Vec::new()
            } else {
                // `tags: a b` o `tags: a, b`.
                value
                    .split([',', ' '])
                    .map(unquote)
                    .filter(|v| !v.is_empty())
                    .collect()
            }
        };
        match key.as_str() {
            "tags" | "tag" => front.tags.extend(list()),
            "aliases" | "alias" => front.aliases.extend(list()),
            "created" | "date" | "creation date" => front.created = timestamp(&unquote(value)),
            "updated" | "modified" | "last modified" => front.updated = timestamp(&unquote(value)),
            _ => {}
        }
    }
    (front, body)
}

fn unquote(value: &str) -> String {
    value
        .trim()
        .trim_matches(|c| c == '"' || c == '\'')
        .trim()
        .to_owned()
}

/// Date del front matter: timestamp UTC o giorno `AAAA-MM-GG`.
fn timestamp(value: &str) -> Option<String> {
    if is_utc_timestamp(value) {
        return Some(value.to_owned());
    }
    value
        .get(..10)
        .filter(|day| day.parse::<jiff::civil::Date>().is_ok())
        .map(|day| format!("{day}T00:00:00Z"))
}

/// `#tag` nel testo, fuori dal codice: lettere, cifre, `_`, `-` e `/` per i
/// tag annidati, con almeno un carattere non numerico.
fn inline_tags(text: &str) -> Vec<String> {
    let mut tags = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        for (index, segment) in line.split('`').enumerate() {
            if index % 2 == 1 {
                continue;
            }
            let mut previous = ' ';
            let characters: Vec<char> = segment.chars().collect();
            let mut position = 0;
            while position < characters.len() {
                let character = characters[position];
                if character == '#' && previous.is_whitespace() {
                    let tag: String = characters[position + 1..]
                        .iter()
                        .take_while(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | '/'))
                        .collect();
                    if !tag.is_empty() && tag.chars().any(|c| !c.is_ascii_digit()) {
                        position += tag.chars().count();
                        tags.push(tag);
                    }
                }
                previous = characters[position];
                position += 1;
            }
        }
    }
    tags
}

/// Piano per un vault Obsidian: le note alla radice vanno nel taccuino con il
/// nome del vault, quelle nelle cartelle in un taccuino per cartella.
pub fn plan_obsidian_import(root: &Path) -> Result<ImportPlan, StorageError> {
    let vault = scan_vault(root)?;
    let label = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Obsidian".to_owned());
    let mut budget = MAX_IMPORT_TOTAL_BYTES;
    let mut files = Vec::new();
    for path in &vault.notes {
        let relative = path.strip_prefix(&vault.root).unwrap_or(path);
        files.push(PlannedNote {
            source_name: portable_path(relative),
            outcome: build_note(&vault, path, relative, &mut budget),
        });
    }
    Ok(ImportPlan::from_collection(
        files,
        Collection {
            kind: "obsidian",
            label,
        },
    ))
}

fn build_note(
    vault: &Vault,
    path: &Path,
    relative: &Path,
    budget: &mut u64,
) -> Result<ParsedNote, String> {
    let bytes = read_limited(path, MAX_IMPORT_FILE_BYTES)?;
    *budget = budget
        .checked_sub(bytes.len() as u64)
        .ok_or_else(|| "limite complessivo dell'importazione superato".to_owned())?;
    let text =
        String::from_utf8(bytes).map_err(|_| "il file non è codificato in UTF-8".to_owned())?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let (front, body) = split_front_matter(text);
    let title = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Nota senza titolo".to_owned());

    #[derive(Default)]
    struct Shared {
        warnings: Vec<String>,
        attachments: Vec<ImportedAttachment>,
        attached: HashSet<PathBuf>,
        links: Vec<PlannedLink>,
    }
    let shared = RefCell::new(Shared::default());
    let attach = |file: PathBuf| -> String {
        let name = file
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "allegato".to_owned());
        let mut state = shared.borrow_mut();
        if state.attached.insert(file.clone()) {
            match read_limited(&file, MAX_ATTACHMENT_BYTES as u64) {
                Ok(bytes) => state.attachments.push(ImportedAttachment {
                    filename: name.clone(),
                    bytes,
                }),
                Err(message) => state.warnings.push(format!("{name}: {message}")),
            }
        }
        name
    };
    let link_to = |target: &str, label: Option<&str>| {
        let key = vault.canonical_link(relative, target);
        if !key.is_empty() {
            shared.borrow_mut().links.push(PlannedLink {
                target_key: key,
                label: label
                    .map(str::trim)
                    .filter(|label| !label.is_empty())
                    .map(str::to_owned),
            });
        }
    };
    let is_note = |reference: &str| {
        let lower = reference
            .split(['#', '|'])
            .next()
            .unwrap_or_default()
            .to_lowercase();
        lower.ends_with(".md") || Path::new(&lower).extension().is_none()
    };
    let mut on_wiki = |link: WikiLink<'_>| -> String {
        let display = link
            .alias
            .filter(|alias| !alias.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| {
                link.target
                    .split(['#', '^'])
                    .next()
                    .unwrap_or(link.target)
                    .trim()
                    .to_owned()
            });
        if link.embed && !is_note(link.target) {
            return match vault.resolve_file(path, link.target) {
                Some(file) => attachment_marker(&attach(file)),
                None => {
                    shared
                        .borrow_mut()
                        .warnings
                        .push(format!("file incorporato non trovato: {}", link.target));
                    display
                }
            };
        }
        link_to(link.target, link.alias);
        if link.embed {
            format!("↪ {display}")
        } else {
            display
        }
    };
    let rewritten = rewrite_markdown(
        body,
        &mut |link: MarkdownLink<'_>| {
            let target = link.target;
            let lower = target.to_ascii_lowercase();
            if lower.contains("://") || lower.starts_with("mailto:") || target.starts_with('#') {
                return Rewrite::Keep;
            }
            if is_note(target) {
                link_to(&percent_decode(target), Some(link.label));
                return Rewrite::Replace(link.label.to_owned());
            }
            match vault.resolve_file(path, target) {
                Some(file) => Rewrite::Replace(attachment_marker(&attach(file))),
                None => Rewrite::Keep,
            }
        },
        Some(&mut on_wiki),
    );
    let Shared {
        warnings,
        attachments,
        links,
        ..
    } = shared.into_inner();

    let mut tags = front.tags;
    tags.extend(inline_tags(body));
    let mut link_keys = vec![note_key(relative), title.to_lowercase()];
    link_keys.extend(front.aliases.iter().map(|alias| alias.to_lowercase()));
    let metadata = fs::metadata(path).ok();
    let modified = metadata
        .as_ref()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(utc_from_system_time);
    let created = metadata
        .as_ref()
        .and_then(|metadata| metadata.created().ok())
        .and_then(utc_from_system_time);

    let mut note = ExternalNote::new(
        "obsidian",
        title.clone(),
        markdown_with_title(&title, &rewritten),
    );
    note.tags = tags;
    note.created_at = front.created.or(created).or(modified.clone());
    note.updated_at = front.updated.or(modified);
    note.attachments = attachments;
    note.notebook_path = relative
        .parent()
        .map(|parent| {
            parent
                .components()
                .filter_map(|component| match component {
                    Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    note.link_keys = link_keys;
    note.links = links;
    note.warnings = warnings;
    finish(note, budget)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::content::to_plain_text;

    pub(crate) fn vault() -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("directory");
        let root = directory.path();
        fs::create_dir_all(root.join(".obsidian")).expect("config");
        fs::write(root.join(".obsidian/app.json"), "{}").expect("config file");
        fs::create_dir_all(root.join("Progetti/Casa")).expect("folders");
        fs::create_dir_all(root.join("allegati")).expect("attachments");
        fs::write(root.join("allegati/pianta.png"), b"\x89PNG\r\n\x1a\nfinto").expect("png");
        fs::write(root.join("allegati/video.mp4"), [0u8, 1, 2, 3]).expect("binary");
        fs::write(
            root.join("Indice.md"),
            "---\ntags:\n  - mappa\n  - \"#generale\"\naliases: [Home]\ncreated: 2024-02-01\n---\n# Indice\n\nVai a [[Progetti/Casa/Ristrutturazione|lavori]] e [[Spesa]].\n\n![[pianta.png]] ![[video.mp4]]\n\nTag #idea e #2024 e `#codice`.\n",
        )
        .expect("index");
        fs::write(
            root.join("Progetti/Casa/Ristrutturazione.md"),
            "Budget nel [preventivo](../../Indice.md) e ![foto](../../allegati/pianta.png).\n\n![[Indice#Sezione]]\n",
        )
        .expect("note");
        fs::write(root.join("Progetti/Spesa.md"), "Latte\n").expect("note");
        directory
    }

    #[test]
    fn imports_folders_front_matter_tags_links_and_embeds() {
        let directory = vault();
        let plan = plan_obsidian_import(directory.path()).expect("plan");
        assert_eq!(plan.files.len(), 3, "le cartelle nascoste sono ignorate");
        let notes: HashMap<String, &ParsedNote> = plan
            .files
            .iter()
            .map(|planned| {
                let parsed = planned.outcome.as_ref().expect("note");
                (parsed.title.clone(), parsed)
            })
            .collect();

        let index = notes["Indice"];
        assert!(index.notebook_path.is_empty());
        assert_eq!(index.tags, vec!["mappa", "generale", "idea"]);
        assert_eq!(index.created_at.as_deref(), Some("2024-02-01T00:00:00Z"));
        assert!(index.link_keys.contains(&"home".to_owned()));
        let targets: Vec<&str> = index
            .links
            .iter()
            .map(|link| link.target_key.as_str())
            .collect();
        assert_eq!(
            targets,
            vec!["progetti/casa/ristrutturazione", "progetti/spesa"]
        );
        assert_eq!(
            index.attachments.len(),
            1,
            "il video non è un tipo supportato"
        );
        assert!(
            index
                .warnings
                .iter()
                .any(|warning| warning.contains("video.mp4"))
        );
        let text = to_plain_text(&index.document);
        assert!(text.contains("Vai a lavori e Spesa."), "{text}");
        assert!(text.contains("📎 pianta.png"), "{text}");
        assert!(
            text.contains("`#codice`") || text.contains("#codice"),
            "{text}"
        );

        let work = notes["Ristrutturazione"];
        assert_eq!(work.notebook_path, vec!["Progetti", "Casa"]);
        assert_eq!(work.attachments.len(), 1);
        let text = to_plain_text(&work.document);
        assert!(
            text.contains("Budget nel preventivo e 📎 pianta.png."),
            "{text}"
        );
        assert!(text.contains("↪ Indice"), "{text}");
        let targets: Vec<&str> = work
            .links
            .iter()
            .map(|link| link.target_key.as_str())
            .collect();
        assert!(targets.contains(&"indice"));
        assert_eq!(notes["Spesa"].notebook_path, vec!["Progetti"]);
        assert_eq!(
            plan.collection.as_ref().expect("collection").kind,
            "obsidian"
        );
    }

    #[test]
    fn attachments_outside_the_vault_are_refused() {
        let outside = tempfile::tempdir().expect("outside");
        fs::write(outside.path().join("segreto.txt"), "riservato").expect("secret");
        let directory = tempfile::tempdir().expect("vault");
        let secret = outside.path().join("segreto.txt");
        fs::write(
            directory.path().join("Nota.md"),
            format!(
                "![x](../{}/segreto.txt) ![y]({}) ![[../segreto.txt]]\n",
                outside.path().file_name().expect("name").to_string_lossy(),
                secret.display()
            ),
        )
        .expect("note");
        let plan = plan_obsidian_import(directory.path()).expect("plan");
        let note = plan.files[0].outcome.as_ref().expect("note");
        assert!(note.attachments.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_leaving_the_vault_are_not_followed() {
        let outside = tempfile::tempdir().expect("outside");
        fs::write(outside.path().join("segreto.png"), b"\x89PNG\r\n\x1a\nx").expect("secret");
        let directory = tempfile::tempdir().expect("vault");
        std::os::unix::fs::symlink(outside.path(), directory.path().join("link")).expect("symlink");
        std::os::unix::fs::symlink(
            outside.path().join("segreto.png"),
            directory.path().join("diretto.png"),
        )
        .expect("symlink");
        fs::write(
            directory.path().join("Nota.md"),
            "![a](link/segreto.png) ![[diretto.png]] ![[segreto.png]]\n",
        )
        .expect("note");
        let plan = plan_obsidian_import(directory.path()).expect("plan");
        assert_eq!(
            plan.files.len(),
            1,
            "le cartelle collegate non vengono esplorate"
        );
        let note = plan.files[0].outcome.as_ref().expect("note");
        assert!(note.attachments.is_empty());
    }

    #[test]
    fn front_matter_variants() {
        let (front, body) =
            split_front_matter("---\ntags: [a, \"b\"]\nupdated: 2024-03-01T10:00:00Z\n---\ntesto");
        assert_eq!(front.tags, vec!["a", "b"]);
        assert_eq!(front.updated.as_deref(), Some("2024-03-01T10:00:00Z"));
        assert_eq!(body, "testo");
        let (front, _) = split_front_matter("---\ntags: uno due\n---\n");
        assert_eq!(front.tags, vec!["uno", "due"]);
        let (front, body) = split_front_matter("niente front matter");
        assert!(front.tags.is_empty());
        assert_eq!(body, "niente front matter");
    }
}
