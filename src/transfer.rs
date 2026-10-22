//! Importazione ed esportazione in formati aperti.
//!
//! - Nota singola: Markdown (con front matter), HTML autonomo e testo.
//! - ZIP Markdown di un taccuino o di tutte le note, con allegati.
//! - Archivio completo GNA (JSON + allegati), reimportabile senza perdita.
//!
//! L'importazione è in due fasi: un piano analizza i file e produce
//! un'anteprima (dry-run); il commit applica le sole voci nuove in un'unica
//! transazione, rimuovendo i file scritti se la transazione fallisce.

use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{self, Read, Write},
    path::{Component, Path, PathBuf},
};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zip::{ZipArchive, ZipWriter};

use crate::{
    backup::{
        HashingWriter, ManifestFile, create_private_file, is_stored_attachment_name,
        read_zip_entry, zip_options,
    },
    content::{
        FrontMatter, encrypted_text_as_code, html_to_document, is_utc_timestamp,
        markdown_to_document, text_to_document, to_html, to_markdown, to_plain_text,
    },
    domain::{
        note::{Note, validate_document},
        organization::{Notebook, Space, Tag},
    },
    storage::{
        CURRENT_SCHEMA_VERSION, Database, MAX_ATTACHMENT_BYTES, StorageError,
        attachment_search_text, get_note, normalize_filename, normalize_organization_name,
        normalize_tag_name, normalize_title, note_from_row, sniff_attachment_type,
        sync_search_index, write_attachment_file,
    },
};

const ARCHIVE_FORMAT: &str = "gna-archive";
const ARCHIVE_FORMAT_VERSION: u32 = 1;
const ARCHIVE_MANIFEST: &str = "manifest.json";
pub(crate) const MAX_IMPORT_FILE_BYTES: u64 = 5 * 1024 * 1024;
pub(crate) const MAX_IMPORT_FILES: usize = 5_000;
pub(crate) const MAX_IMPORT_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
pub(crate) const MAX_FOLDER_DEPTH: usize = 16;
const MAX_ARCHIVE_NOTE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_ARCHIVE_MANIFEST_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ARCHIVE_ENTRIES: usize = 500_000;
const MAX_FILE_NAME_CHARS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExportFormat {
    Markdown,
    Html,
    Text,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportedNote {
    pub file_name: String,
    pub contents: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExportSummary {
    pub note_count: u64,
    pub attachment_count: u64,
}

// ---------------------------------------------------------------------------
// Nomi file

/// Nome file portabile (Linux/Windows) senza separatori né nomi riservati.
pub fn sanitize_file_name(name: &str, fallback: &str) -> String {
    let mut sanitized: String = name
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .take(MAX_FILE_NAME_CHARS)
        .collect();
    sanitized = sanitized.trim().trim_matches('.').trim().to_owned();
    if sanitized.is_empty() {
        sanitized = fallback.to_owned();
    }
    let stem = sanitized
        .split('.')
        .next()
        .unwrap_or("")
        .to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        sanitized.insert(0, '_');
    }
    sanitized
}

/// Riserva un nome univoco (senza distinzione maiuscole/minuscole).
fn unique_name(used: &mut HashSet<String>, stem: &str, extension: &str) -> String {
    let mut counter = 1;
    loop {
        let candidate = if counter == 1 {
            format!("{stem}{extension}")
        } else {
            format!("{stem} ({counter}){extension}")
        };
        if used.insert(candidate.to_lowercase()) {
            return candidate;
        }
        counter += 1;
    }
}

fn split_extension(file_name: &str) -> (&str, &str) {
    match file_name.rfind('.') {
        Some(index) if index > 0 => (&file_name[..index], &file_name[index..]),
        _ => (file_name, ""),
    }
}

/// Scrive un file sostituendo la destinazione solo a scrittura completata.
pub fn write_file_atomically(destination: &Path, bytes: &[u8]) -> Result<(), StorageError> {
    let partial = partial_path(destination)?;
    let result = (|| {
        let mut file = create_private_file(&partial)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&partial, destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

/// Copia un file nella destinazione scelta senza lasciare copie parziali.
pub fn copy_file_atomically(source: &Path, destination: &Path) -> Result<(), StorageError> {
    let partial = partial_path(destination)?;
    let result = (|| {
        let mut target = create_private_file(&partial)?;
        io::copy(&mut File::open(source)?, &mut target)?;
        target.sync_all()?;
        drop(target);
        fs::rename(&partial, destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

pub(crate) fn partial_path(destination: &Path) -> Result<PathBuf, StorageError> {
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| StorageError::Export("percorso di destinazione non valido".to_owned()))?;
    Ok(destination.with_file_name(format!(".{file_name}.{}.partial", Uuid::new_v4().simple())))
}

fn with_zip_file<T>(
    destination: &Path,
    write: impl FnOnce(&mut ZipWriter<File>) -> Result<T, StorageError>,
) -> Result<T, StorageError> {
    let partial = partial_path(destination)?;
    let result = (|| {
        let mut zip = ZipWriter::new(create_private_file(&partial)?);
        let value = write(&mut zip)?;
        let file = zip.finish()?;
        file.sync_all()?;
        drop(file);
        fs::rename(&partial, destination)?;
        Ok(value)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

// ---------------------------------------------------------------------------
// Lettura dati per l'esportazione

struct NoteExportData {
    note: Note,
    notebook: Option<String>,
    tags: Vec<String>,
}

fn load_note_export(connection: &Connection, note: Note) -> Result<NoteExportData, StorageError> {
    let notebook = match &note.notebook_id {
        Some(id) => connection
            .query_row("SELECT name FROM notebooks WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .optional()?,
        None => None,
    };
    let mut statement = connection.prepare(
        "SELECT tags.name FROM tags
         INNER JOIN note_tags ON note_tags.tag_id = tags.id
         WHERE note_tags.note_id = ?1
         ORDER BY tags.name COLLATE NOCASE",
    )?;
    let tags = statement
        .query_map([&note.id], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(NoteExportData {
        note,
        notebook,
        tags,
    })
}

fn front_matter(data: &NoteExportData, attachments: Vec<String>) -> FrontMatter {
    FrontMatter {
        title: Some(data.note.title.clone()),
        created_at: Some(data.note.created_at.clone()),
        updated_at: Some(data.note.updated_at.clone()),
        tags: data.tags.clone(),
        notebook: data.notebook.clone(),
        attachments,
        image_paths: std::collections::HashMap::new(),
    }
}

struct StoredAttachment {
    id: String,
    original_filename: String,
    stored_filename: String,
    mime_type: String,
    size_bytes: u64,
    sha256: String,
    created_at: String,
}

fn load_attachments(
    connection: &Connection,
    note_id: &str,
) -> Result<Vec<StoredAttachment>, StorageError> {
    let mut statement = connection.prepare(
        "SELECT id, original_filename, stored_filename, mime_type, size_bytes, sha256, created_at
         FROM attachments
         WHERE note_id = ?1 AND deleted_at IS NULL
         ORDER BY created_at, id",
    )?;
    let rows = statement.query_map([note_id], |row| {
        Ok(StoredAttachment {
            id: row.get(0)?,
            original_filename: row.get(1)?,
            stored_filename: row.get(2)?,
            mime_type: row.get(3)?,
            size_bytes: u64::try_from(row.get::<_, i64>(4)?).unwrap_or(0),
            sha256: row.get(5)?,
            created_at: row.get(6)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Copia un allegato nell'archivio verificandone l'hash: un allegato
/// danneggiato interrompe l'esportazione invece di propagarsi.
fn copy_attachment_to_zip(
    zip: &mut ZipWriter<File>,
    data_directory: &Path,
    attachment: &StoredAttachment,
    entry: &str,
) -> Result<ManifestFile, StorageError> {
    if !is_stored_attachment_name(&attachment.stored_filename) {
        return Err(StorageError::AttachmentIntegrity);
    }
    let path = data_directory
        .join("attachments")
        .join(&attachment.stored_filename);
    let metadata = fs::symlink_metadata(&path).map_err(|_| StorageError::AttachmentIntegrity)?;
    if !metadata.is_file() || metadata.len() != attachment.size_bytes {
        return Err(StorageError::Export(format!(
            "l'allegato «{}» è mancante o danneggiato",
            attachment.original_filename
        )));
    }
    zip.start_file(entry, zip_options())?;
    let mut writer = HashingWriter::new(&mut *zip);
    io::copy(&mut File::open(&path)?, &mut writer)?;
    let (sha256, size) = writer.finish();
    if sha256 != attachment.sha256 {
        return Err(StorageError::Export(format!(
            "l'allegato «{}» non supera la verifica d'integrità",
            attachment.original_filename
        )));
    }
    Ok(ManifestFile {
        path: entry.to_owned(),
        sha256,
        size,
    })
}

// ---------------------------------------------------------------------------
// Formato archivio

#[derive(Debug, Serialize, Deserialize)]
struct ArchiveManifest {
    format: String,
    version: u32,
    exported_at: String,
    schema_version: u32,
    app_version: String,
    spaces: Vec<Space>,
    notebooks: Vec<Notebook>,
    tags: Vec<Tag>,
    notes: Vec<ManifestFile>,
    attachments: Vec<ManifestFile>,
    /// Attività, eventi e promemoria: campi facoltativi, assenti negli
    /// archivi creati prima della fase 4.
    #[serde(flatten, default)]
    planner: crate::planner::PlannerArchive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArchiveLink {
    target_note_id: String,
    label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArchiveAttachment {
    id: String,
    original_filename: String,
    mime_type: String,
    size_bytes: u64,
    sha256: String,
    created_at: String,
    path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ArchiveNote {
    id: String,
    notebook_id: Option<String>,
    title: String,
    content: Value,
    created_at: String,
    updated_at: String,
    is_pinned: bool,
    archived_at: Option<String>,
    tag_ids: Vec<String>,
    links: Vec<ArchiveLink>,
    attachments: Vec<ArchiveAttachment>,
}

fn json_entry<W: Write + io::Seek>(
    zip: &mut ZipWriter<W>,
    path: &str,
    value: &impl Serialize,
) -> Result<ManifestFile, StorageError> {
    let bytes = serde_json::to_vec_pretty(value)?;
    zip.start_file(path, zip_options())?;
    zip.write_all(&bytes)?;
    Ok(ManifestFile {
        path: path.to_owned(),
        sha256: format!("{:x}", Sha256::digest(&bytes)),
        size: bytes.len() as u64,
    })
}

impl Database {
    pub fn export_note(
        &self,
        note_id: &str,
        format: ExportFormat,
        language: &str,
    ) -> Result<ExportedNote, StorageError> {
        let connection = self.connection()?;
        let note = get_note(&connection, note_id)?.ok_or(StorageError::NoteNotFound)?;
        let data = load_note_export(&connection, note)?;
        let stem = sanitize_file_name(&data.note.title, "Nota");
        let (extension, contents) = match format {
            ExportFormat::Markdown => (
                "md",
                to_markdown(
                    &data.note.title,
                    &data.note.content,
                    &front_matter(&data, Vec::new()),
                ),
            ),
            ExportFormat::Html => (
                "html",
                to_html(&data.note.title, &data.note.content, language),
            ),
            ExportFormat::Text => {
                // Il testo cifrato resta nel file (cifrato), non solo il segnaposto.
                let body = to_plain_text(&encrypted_text_as_code(&data.note.content));
                ("txt", format!("{}\n\n{body}\n", data.note.title))
            }
        };
        Ok(ExportedNote {
            file_name: format!("{stem}.{extension}"),
            contents,
        })
    }

    /// Esporta un taccuino (o tutte le note se `notebook_id` è `None`) in
    /// uno ZIP di file Markdown con gli allegati in cartelle relative.
    pub fn export_markdown_zip(
        &self,
        notebook_id: Option<&str>,
        unfiled_folder: &str,
        destination: &Path,
    ) -> Result<ExportSummary, StorageError> {
        let data_directory = self.data_directory()?.to_path_buf();
        let connection = self.connection()?;
        let notes = {
            let mut statement = connection.prepare(
                "SELECT id, notebook_id, title, content_json, created_at, updated_at, is_pinned,
                        archived_at
                 FROM notes
                 WHERE deleted_at IS NULL AND (?1 IS NULL OR notebook_id = ?1)
                 ORDER BY title COLLATE NOCASE, id",
            )?;
            statement
                .query_map([notebook_id], note_from_row)?
                .collect::<Result<Vec<_>, _>>()?
        };
        if let Some(notebook_id) = notebook_id {
            let exists: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM notebooks WHERE id = ?1 AND deleted_at IS NULL)",
                [notebook_id],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(StorageError::NotebookNotFound);
            }
        }

        with_zip_file(destination, |zip| {
            let mut folders: HashMap<Option<String>, (String, HashSet<String>)> = HashMap::new();
            let mut used_folders = HashSet::new();
            let mut summary = ExportSummary {
                note_count: 0,
                attachment_count: 0,
            };
            for note in notes {
                let data = load_note_export(&connection, note)?;
                let (folder, used) =
                    folders
                        .entry(data.note.notebook_id.clone())
                        .or_insert_with(|| {
                            let base = sanitize_file_name(
                                data.notebook.as_deref().unwrap_or(unfiled_folder),
                                "Note",
                            );
                            (unique_name(&mut used_folders, &base, ""), HashSet::new())
                        });
                let stem = sanitize_file_name(&data.note.title, "Nota");
                let file_name = unique_name(used, &stem, ".md");
                let file_stem = split_extension(&file_name).0.to_owned();

                let mut attachment_paths = Vec::new();
                let mut image_paths = HashMap::new();
                let mut used_attachments = HashSet::new();
                for attachment in load_attachments(&connection, &data.note.id)? {
                    let original = sanitize_file_name(&attachment.original_filename, "allegato");
                    let (stem, extension) = split_extension(&original);
                    let name = unique_name(&mut used_attachments, stem, extension);
                    let relative = format!("attachments/{file_stem}/{name}");
                    copy_attachment_to_zip(
                        zip,
                        &data_directory,
                        &attachment,
                        &format!("{folder}/{relative}"),
                    )?;
                    image_paths.insert(attachment.id.clone(), relative.clone());
                    attachment_paths.push(relative);
                    summary.attachment_count += 1;
                }
                let mut matter = front_matter(&data, attachment_paths);
                matter.image_paths = image_paths;
                let markdown = to_markdown(&data.note.title, &data.note.content, &matter);
                zip.start_file(format!("{folder}/{file_name}"), zip_options())?;
                zip.write_all(markdown.as_bytes())?;
                summary.note_count += 1;
            }
            Ok(summary)
        })
    }

    /// Esporta l'archivio completo (note non cestinate, organizzazione,
    /// collegamenti e allegati) nel formato documentato `gna-archive` v1.
    pub fn export_archive(&self, destination: &Path) -> Result<ExportSummary, StorageError> {
        let data_directory = self.data_directory()?.to_path_buf();
        let connection = self.connection()?;
        let exported_at: String =
            connection.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%fZ', 'now')", [], |row| {
                row.get(0)
            })?;
        let spaces = {
            let mut statement = connection.prepare(
                "SELECT id, name, description, sort_order, created_at, updated_at
                 FROM spaces WHERE deleted_at IS NULL ORDER BY sort_order, id",
            )?;
            statement
                .query_map([], |row| {
                    Ok(Space {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        description: row.get(2)?,
                        sort_order: row.get(3)?,
                        created_at: row.get(4)?,
                        updated_at: row.get(5)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let notebooks = {
            let mut statement = connection.prepare(
                "SELECT id, space_id, name, description, sort_order, created_at, updated_at
                 FROM notebooks WHERE deleted_at IS NULL ORDER BY sort_order, id",
            )?;
            statement
                .query_map([], |row| {
                    Ok(Notebook {
                        id: row.get(0)?,
                        space_id: row.get(1)?,
                        name: row.get(2)?,
                        description: row.get(3)?,
                        sort_order: row.get(4)?,
                        created_at: row.get(5)?,
                        updated_at: row.get(6)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let tags = {
            let mut statement = connection.prepare(
                "SELECT id, name, color, created_at, updated_at FROM tags
                     WHERE deleted_at IS NULL ORDER BY id",
            )?;
            statement
                .query_map([], |row| {
                    Ok(Tag {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        color: row.get(2)?,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let planner = crate::planner::export_planner(&connection)?;
        let notes = {
            let mut statement = connection.prepare(
                "SELECT id, notebook_id, title, content_json, created_at, updated_at, is_pinned,
                        archived_at
                 FROM notes WHERE deleted_at IS NULL ORDER BY created_at, id",
            )?;
            statement
                .query_map([], note_from_row)?
                .collect::<Result<Vec<_>, _>>()?
        };

        with_zip_file(destination, |zip| {
            let mut note_files = Vec::new();
            let mut attachment_files = Vec::new();
            for note in notes {
                let tag_ids = connection
                    .prepare("SELECT tag_id FROM note_tags WHERE note_id = ?1 ORDER BY tag_id")?
                    .query_map([&note.id], |row| row.get(0))?
                    .collect::<Result<Vec<String>, _>>()?;
                let links = connection
                    .prepare(
                        "SELECT target_note_id, label FROM note_links
                         WHERE source_note_id = ?1 ORDER BY target_note_id",
                    )?
                    .query_map([&note.id], |row| {
                        Ok(ArchiveLink {
                            target_note_id: row.get(0)?,
                            label: row.get(1)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                let mut attachments = Vec::new();
                for attachment in load_attachments(&connection, &note.id)? {
                    let path = format!("attachments/{}", attachment.stored_filename);
                    attachment_files.push(copy_attachment_to_zip(
                        zip,
                        &data_directory,
                        &attachment,
                        &path,
                    )?);
                    attachments.push(ArchiveAttachment {
                        id: attachment.id,
                        original_filename: attachment.original_filename,
                        mime_type: attachment.mime_type,
                        size_bytes: attachment.size_bytes,
                        sha256: attachment.sha256,
                        created_at: attachment.created_at,
                        path,
                    });
                }
                let archive_note = ArchiveNote {
                    id: note.id.clone(),
                    notebook_id: note.notebook_id,
                    title: note.title,
                    content: note.content,
                    created_at: note.created_at,
                    updated_at: note.updated_at,
                    is_pinned: note.is_pinned,
                    archived_at: note.archived_at,
                    tag_ids,
                    links,
                    attachments,
                };
                note_files.push(json_entry(
                    zip,
                    &format!("notes/{}.json", note.id),
                    &archive_note,
                )?);
            }
            let summary = ExportSummary {
                note_count: note_files.len() as u64,
                attachment_count: attachment_files.len() as u64,
            };
            let manifest = ArchiveManifest {
                format: ARCHIVE_FORMAT.to_owned(),
                version: ARCHIVE_FORMAT_VERSION,
                exported_at,
                schema_version: CURRENT_SCHEMA_VERSION,
                app_version: env!("CARGO_PKG_VERSION").to_owned(),
                spaces,
                notebooks,
                tags,
                notes: note_files,
                attachments: attachment_files,
                planner,
            };
            json_entry(zip, ARCHIVE_MANIFEST, &manifest)?;
            Ok(summary)
        })
    }
}

// ---------------------------------------------------------------------------
// Piani d'importazione

pub(crate) struct ImportedAttachment {
    pub(crate) filename: String,
    pub(crate) bytes: Vec<u8>,
}

/// Collegamento da una nota importata a un'altra dello stesso import,
/// identificata dalla chiave dell'app di origine.
pub(crate) struct PlannedLink {
    pub(crate) target_key: String,
    pub(crate) label: Option<String>,
}

pub(crate) struct ParsedNote {
    pub(crate) title: String,
    pub(crate) document: Value,
    pub(crate) tags: Vec<String>,
    pub(crate) created_at: Option<String>,
    pub(crate) updated_at: Option<String>,
    pub(crate) attachments: Vec<ImportedAttachment>,
    /// Percorsi canonici degli allegati letti, per non importarli come note.
    pub(crate) attachment_sources: Vec<PathBuf>,
    pub(crate) source: &'static str,
    pub(crate) fingerprint: String,
    pub(crate) warnings: Vec<String>,
    /// Taccuino d'origine (cartelle annidate); vuoto se alla radice.
    pub(crate) notebook_path: Vec<String>,
    /// Chiavi con cui altre note dello stesso import possono citarla.
    pub(crate) link_keys: Vec<String>,
    pub(crate) links: Vec<PlannedLink>,
}

pub(crate) struct PlannedNote {
    pub(crate) source_name: String,
    pub(crate) outcome: Result<ParsedNote, String>,
}

/// Raccolta proveniente da un'altra app (Evernote, Joplin, Obsidian): la
/// struttura dei taccuini può essere mantenuta.
pub(crate) struct Collection {
    pub(crate) kind: &'static str,
    /// Nome proposto per lo spazio che raccoglie i taccuini importati.
    pub(crate) label: String,
}

struct ArchivePlan {
    path: PathBuf,
    manifest: ArchiveManifest,
    notes: Vec<ArchiveNote>,
}

pub struct ImportPlan {
    pub(crate) files: Vec<PlannedNote>,
    archive: Option<ArchivePlan>,
    pub(crate) collection: Option<Collection>,
}

impl ImportPlan {
    pub(crate) fn from_collection(files: Vec<PlannedNote>, collection: Collection) -> Self {
        Self {
            files,
            archive: None,
            collection: Some(collection),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImportItemStatus {
    New,
    Duplicate,
    Error,
    Imported,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportItem {
    pub source: String,
    pub title: String,
    pub status: ImportItemStatus,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportPreview {
    pub kind: &'static str,
    pub items: Vec<ImportItem>,
    pub new_count: u64,
    pub duplicate_count: u64,
    pub error_count: u64,
    pub attachment_count: u64,
    pub notebook_count: u64,
    pub tag_count: u64,
    /// Attività ed eventi contenuti in un archivio completo.
    pub task_count: u64,
    pub event_count: u64,
    /// Collegamenti tra note riconosciuti (Joplin, Obsidian).
    pub link_count: u64,
    /// Per le raccolte da altre app: nome proposto per lo spazio.
    pub suggested_space: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    pub imported: u64,
    pub skipped: u64,
    pub failed: u64,
    pub attachments: u64,
    /// Attività, eventi e promemoria importati da un archivio completo.
    pub planner_items: u64,
    pub notebook_id: Option<String>,
    /// Taccuini e collegamenti creati mantenendo la struttura d'origine.
    pub notebooks: u64,
    pub links: u64,
    pub items: Vec<ImportItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ImportDestination {
    NewNotebook {
        name: String,
    },
    Notebook {
        id: String,
    },
    Unfiled,
    /// Un taccuino per ogni taccuino/cartella d'origine, eventualmente
    /// raccolti in un nuovo spazio.
    KeepStructure {
        space_name: Option<String>,
    },
}

/// Impronta SHA-256 di un contenuto binario.
pub(crate) fn content_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn fingerprint(title: &str, document: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(title.trim().as_bytes());
    hasher.update([0]);
    // Senza spazi: dividere un paragrafo attorno a un'immagine (o
    // riformattarlo) non deve far sembrare nuova una nota già importata.
    let compact: String = to_plain_text(document)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    hasher.update(compact.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[derive(Clone, Copy)]
enum FileKind {
    Markdown,
    Html,
    Text,
}

fn file_kind(path: &Path) -> Option<FileKind> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    match extension.as_str() {
        "md" | "markdown" | "mdown" | "mkd" => Some(FileKind::Markdown),
        "html" | "htm" | "xhtml" => Some(FileKind::Html),
        "txt" | "text" => Some(FileKind::Text),
        _ => None,
    }
}

pub(crate) fn read_limited(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "file non leggibile".to_owned())?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err("i collegamenti simbolici e i file speciali non sono importati".to_owned());
    }
    if metadata.len() > limit {
        return Err(format!(
            "il file supera il limite di {} MiB",
            limit / (1024 * 1024)
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(limit + 1).read_to_end(&mut bytes))
        .map_err(|_| "file non leggibile".to_owned())?;
    if bytes.len() as u64 > limit {
        return Err("il file è cambiato durante la lettura".to_owned());
    }
    Ok(bytes)
}

/// Risolve un allegato referenziato dal front matter, solo all'interno della
/// cartella radice consentita e senza seguire collegamenti simbolici.
fn resolve_attachment(root: &Path, base: &Path, reference: &str) -> Result<PathBuf, String> {
    let relative = Path::new(reference);
    if reference.is_empty()
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        return Err(format!("percorso allegato non consentito: {reference}"));
    }
    let candidate = base.join(relative);
    let canonical_root = root
        .canonicalize()
        .map_err(|_| "cartella non leggibile".to_owned())?;
    let mut current = candidate.as_path();
    while let Some(parent) = current.parent() {
        if fs::symlink_metadata(current).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
            return Err(format!(
                "allegato tramite collegamento simbolico: {reference}"
            ));
        }
        if parent == root {
            break;
        }
        current = parent;
    }
    let canonical = candidate
        .canonicalize()
        .map_err(|_| format!("allegato non trovato: {reference}"))?;
    if !canonical.starts_with(&canonical_root) {
        return Err(format!(
            "allegato fuori dalla cartella importata: {reference}"
        ));
    }
    Ok(canonical)
}

fn parse_import_file(path: &Path, root: &Path, budget: &mut u64) -> Result<ParsedNote, String> {
    let kind = file_kind(path).ok_or_else(|| "formato non supportato".to_owned())?;
    let bytes = read_limited(path, MAX_IMPORT_FILE_BYTES)?;
    *budget = budget
        .checked_sub(bytes.len() as u64)
        .ok_or_else(|| "limite complessivo dell'importazione superato".to_owned())?;
    let text =
        String::from_utf8(bytes).map_err(|_| "il file non è codificato in UTF-8".to_owned())?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("Nota importata")
        .to_owned();
    let (parsed, source) = match kind {
        FileKind::Markdown => (markdown_to_document(text), "markdown"),
        FileKind::Html => (html_to_document(text), "html"),
        FileKind::Text => (
            crate::content::ParsedDocument {
                title: None,
                document: text_to_document(text),
                front_matter: FrontMatter::default(),
            },
            "text",
        ),
    };
    validate_document(&parsed.document).map_err(str::to_owned)?;
    let title = normalize_title(parsed.title.as_deref().unwrap_or(&stem))
        .or_else(|_| {
            normalize_title(
                &parsed
                    .title
                    .unwrap_or_default()
                    .chars()
                    .take(500)
                    .collect::<String>(),
            )
        })
        .map_err(|error| error.to_string())?;

    let mut warnings = Vec::new();
    let mut attachments = Vec::new();
    let mut attachment_sources = Vec::new();
    let base = path.parent().unwrap_or(root);
    for reference in &parsed.front_matter.attachments {
        let resolved = resolve_attachment(root, base, reference).and_then(|resolved| {
            read_limited(&resolved, MAX_ATTACHMENT_BYTES as u64).map(|bytes| (resolved, bytes))
        });
        match resolved {
            Ok((resolved, bytes)) => {
                if sniff_attachment_type(&bytes).is_err() {
                    warnings.push(format!(
                        "allegato di tipo non supportato ignorato: {reference}"
                    ));
                    continue;
                }
                match budget.checked_sub(bytes.len() as u64) {
                    Some(remaining) => *budget = remaining,
                    None => {
                        warnings.push(format!(
                            "allegato ignorato per limite di dimensione: {reference}"
                        ));
                        continue;
                    }
                }
                let filename = resolved
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("allegato")
                    .to_owned();
                attachment_sources.push(resolved);
                attachments.push(ImportedAttachment { filename, bytes });
            }
            Err(message) => warnings.push(message),
        }
    }
    let tags = parsed
        .front_matter
        .tags
        .iter()
        .filter_map(|tag| normalize_tag_name(tag).ok())
        .collect();
    Ok(ParsedNote {
        fingerprint: fingerprint(&title, &parsed.document),
        title,
        document: parsed.document,
        tags,
        created_at: parsed.front_matter.created_at,
        updated_at: parsed.front_matter.updated_at,
        attachments,
        attachment_sources,
        source,
        warnings,
        notebook_path: Vec::new(),
        link_keys: Vec::new(),
        links: Vec::new(),
    })
}

pub(crate) fn display_name(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .or_else(|| path.file_name().map(Path::new))
        .map(portable_path)
        .unwrap_or_else(|| "file".to_owned())
}

/// Percorso relativo con `/` come separatore su ogni sistema, così i report
/// d'importazione sono uguali su Linux e Windows.
pub(crate) fn portable_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Piano per singoli file selezionati (Markdown, HTML o testo).
pub fn plan_files_import(paths: &[PathBuf]) -> Result<ImportPlan, StorageError> {
    if paths.len() > MAX_IMPORT_FILES {
        return Err(StorageError::InvalidImport(format!(
            "puoi importare al massimo {MAX_IMPORT_FILES} file alla volta"
        )));
    }
    let mut budget = MAX_IMPORT_TOTAL_BYTES;
    let files = paths
        .iter()
        .map(|path| {
            let root = path.parent().unwrap_or(Path::new("."));
            PlannedNote {
                source_name: display_name(path, root),
                outcome: parse_import_file(path, root, &mut budget),
            }
        })
        .collect();
    Ok(ImportPlan {
        files,
        archive: None,
        collection: None,
    })
}

/// Piano per una cartella di Markdown/testo/HTML, esplorata in profondità
/// limitata ignorando file nascosti e collegamenti simbolici.
pub fn plan_folder_import(root: &Path) -> Result<ImportPlan, StorageError> {
    let metadata = fs::symlink_metadata(root)?;
    if !metadata.is_dir() {
        return Err(StorageError::InvalidImport(
            "la cartella selezionata non è valida".to_owned(),
        ));
    }
    let mut found = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    while let Some((directory, depth)) = pending.pop() {
        let mut entries = fs::read_dir(&directory)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let name = entry.file_name();
            if name.to_string_lossy().starts_with('.') {
                continue;
            }
            let file_type = entry.file_type()?;
            let path = entry.path();
            if file_type.is_dir() && depth < MAX_FOLDER_DEPTH {
                pending.push((path, depth + 1));
            } else if file_type.is_file() && file_kind(&path).is_some() {
                found.push(path);
                if found.len() > MAX_IMPORT_FILES {
                    return Err(StorageError::InvalidImport(format!(
                        "la cartella contiene più di {MAX_IMPORT_FILES} note"
                    )));
                }
            }
        }
    }
    found.sort();
    let mut budget = MAX_IMPORT_TOTAL_BYTES;
    let planned: Vec<(PathBuf, PlannedNote)> = found
        .into_iter()
        .map(|path| {
            let note = PlannedNote {
                source_name: display_name(&path, root),
                outcome: parse_import_file(&path, root, &mut budget),
            };
            (path, note)
        })
        .collect();
    // I file usati come allegati da una nota non diventano note autonome.
    let referenced: HashSet<PathBuf> = planned
        .iter()
        .filter_map(|(_, note)| note.outcome.as_ref().ok())
        .flat_map(|parsed| parsed.attachment_sources.iter().cloned())
        .collect();
    let files = planned
        .into_iter()
        .filter(|(path, _)| {
            !path
                .canonicalize()
                .is_ok_and(|canonical| referenced.contains(&canonical))
        })
        .map(|(_, note)| note)
        .collect();
    Ok(ImportPlan {
        files,
        archive: None,
        collection: None,
    })
}

fn is_valid_archive_note_path(path: &str) -> bool {
    path.strip_prefix("notes/")
        .and_then(|name| name.strip_suffix(".json"))
        .is_some_and(|id| Uuid::parse_str(id).is_ok())
}

fn verified_entry<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
    file: &ManifestFile,
    limit: u64,
) -> Result<Vec<u8>, StorageError> {
    if file.size > limit {
        return Err(StorageError::InvalidImport(format!(
            "voce troppo grande: {}",
            file.path
        )));
    }
    let bytes = read_zip_entry(archive, &file.path, limit)?;
    if bytes.len() as u64 != file.size || format!("{:x}", Sha256::digest(&bytes)) != file.sha256 {
        return Err(StorageError::InvalidImport(format!(
            "verifica d'integrità fallita per {}",
            file.path
        )));
    }
    Ok(bytes)
}

/// Piano per un archivio completo GNA: verifica manifest e note subito, gli
/// allegati al momento del commit.
pub fn plan_archive_import(path: &Path) -> Result<ImportPlan, StorageError> {
    let mut archive = ZipArchive::new(File::open(path)?)?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(StorageError::InvalidImport(
            "troppe voci nell'archivio".to_owned(),
        ));
    }
    let manifest: ArchiveManifest = serde_json::from_slice(&read_zip_entry(
        &mut archive,
        ARCHIVE_MANIFEST,
        MAX_ARCHIVE_MANIFEST_BYTES,
    )?)
    .map_err(|_| StorageError::InvalidImport("manifest dell'archivio non valido".to_owned()))?;
    if manifest.format != ARCHIVE_FORMAT || manifest.version != ARCHIVE_FORMAT_VERSION {
        return Err(StorageError::InvalidImport(
            "il file non è un archivio GNA supportato".to_owned(),
        ));
    }
    let attachment_paths: HashSet<&str> = manifest
        .attachments
        .iter()
        .map(|file| file.path.as_str())
        .collect();
    let mut notes = Vec::new();
    let mut files = Vec::new();
    for file in &manifest.notes {
        if !is_valid_archive_note_path(&file.path) {
            return Err(StorageError::InvalidImport(format!(
                "voce non valida: {}",
                file.path
            )));
        }
        let outcome = verified_entry(&mut archive, file, MAX_ARCHIVE_NOTE_BYTES)
            .and_then(|bytes| {
                serde_json::from_slice::<ArchiveNote>(&bytes)
                    .map_err(|_| StorageError::InvalidImport("nota non leggibile".to_owned()))
            })
            .and_then(|note| {
                validate_document(&note.content).map_err(StorageError::InvalidDocument)?;
                normalize_title(&note.title)?;
                if Uuid::parse_str(&note.id).is_err()
                    || format!("notes/{}.json", note.id) != file.path
                {
                    return Err(StorageError::InvalidImport(
                        "identificativo non valido".to_owned(),
                    ));
                }
                for attachment in &note.attachments {
                    let valid = attachment
                        .path
                        .strip_prefix("attachments/")
                        .is_some_and(is_stored_attachment_name)
                        && attachment_paths.contains(attachment.path.as_str());
                    if !valid {
                        return Err(StorageError::InvalidImport(
                            "allegato non presente nell'archivio".to_owned(),
                        ));
                    }
                }
                Ok(note)
            });
        match outcome {
            Ok(note) => {
                files.push(PlannedNote {
                    source_name: file.path.clone(),
                    outcome: Ok(ParsedNote {
                        fingerprint: String::new(),
                        title: note.title.clone(),
                        document: Value::Null,
                        tags: Vec::new(),
                        created_at: None,
                        updated_at: None,
                        attachments: Vec::new(),
                        attachment_sources: Vec::new(),
                        source: "gna-archive",
                        warnings: Vec::new(),
                        notebook_path: Vec::new(),
                        link_keys: Vec::new(),
                        links: Vec::new(),
                    }),
                });
                notes.push(note);
            }
            Err(error) => files.push(PlannedNote {
                source_name: file.path.clone(),
                outcome: Err(error.to_string()),
            }),
        }
    }
    Ok(ImportPlan {
        files,
        collection: None,
        archive: Some(ArchivePlan {
            path: path.to_path_buf(),
            manifest,
            notes,
        }),
    })
}

fn existing_fingerprints(connection: &Connection) -> Result<HashSet<String>, StorageError> {
    let mut statement =
        connection.prepare("SELECT title, content_json FROM notes WHERE deleted_at IS NULL")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut fingerprints = HashSet::new();
    for row in rows {
        let (title, content) = row?;
        if let Ok(document) = serde_json::from_str::<Value>(&content) {
            fingerprints.insert(fingerprint(&title, &document));
        }
    }
    Ok(fingerprints)
}

fn note_exists(connection: &Connection, id: &str) -> Result<bool, StorageError> {
    Ok(connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM notes WHERE id = ?1)",
        [id],
        |row| row.get(0),
    )?)
}

fn warning_message(warnings: &[String]) -> Option<String> {
    (!warnings.is_empty()).then(|| warnings.join("; "))
}

impl Database {
    /// Anteprima (dry-run): nessun dato viene modificato.
    pub fn preview_import(&self, plan: &ImportPlan) -> Result<ImportPreview, StorageError> {
        let connection = self.connection()?;
        let mut items = Vec::new();
        let (mut new_count, mut duplicate_count, mut error_count, mut attachment_count) =
            (0, 0, 0, 0);
        let mut fingerprints = if plan.archive.is_none() {
            existing_fingerprints(&connection)?
        } else {
            HashSet::new()
        };
        let mut archive_notes = plan.archive.iter().flat_map(|archive| archive.notes.iter());
        for planned in &plan.files {
            let item = match &planned.outcome {
                Err(message) => {
                    error_count += 1;
                    ImportItem {
                        source: planned.source_name.clone(),
                        title: String::new(),
                        status: ImportItemStatus::Error,
                        message: Some(message.clone()),
                    }
                }
                Ok(parsed) => {
                    let (duplicate, attachments) = if plan.archive.is_some() {
                        let note = archive_notes.next();
                        (
                            note.map(|note| note_exists(&connection, &note.id))
                                .transpose()?
                                .unwrap_or(false),
                            note.map_or(0, |note| note.attachments.len()),
                        )
                    } else {
                        (
                            !fingerprints.insert(parsed.fingerprint.clone()),
                            parsed.attachments.len(),
                        )
                    };
                    if duplicate {
                        duplicate_count += 1;
                    } else {
                        new_count += 1;
                        attachment_count += attachments as u64;
                    }
                    ImportItem {
                        source: planned.source_name.clone(),
                        title: parsed.title.clone(),
                        status: if duplicate {
                            ImportItemStatus::Duplicate
                        } else {
                            ImportItemStatus::New
                        },
                        message: warning_message(&parsed.warnings),
                    }
                }
            };
            items.push(item);
        }
        let (notebook_count, tag_count) = match &plan.archive {
            Some(archive) => (
                archive.manifest.notebooks.len() as u64,
                archive.manifest.tags.len() as u64,
            ),
            None => {
                let parsed: Vec<&ParsedNote> = plan
                    .files
                    .iter()
                    .filter_map(|planned| planned.outcome.as_ref().ok())
                    .collect();
                let tags: HashSet<String> = parsed
                    .iter()
                    .flat_map(|parsed| parsed.tags.iter().map(|tag| tag.to_lowercase()))
                    .collect();
                let notebooks: HashSet<&Vec<String>> =
                    parsed.iter().map(|parsed| &parsed.notebook_path).collect();
                let notebooks = if plan.collection.is_some() {
                    notebooks.len() as u64
                } else {
                    0
                };
                (notebooks, tags.len() as u64)
            }
        };
        let link_count = plan
            .files
            .iter()
            .filter_map(|planned| planned.outcome.as_ref().ok())
            .map(|parsed| parsed.links.len() as u64)
            .sum();
        Ok(ImportPreview {
            link_count,
            suggested_space: plan
                .collection
                .as_ref()
                .map(|collection| collection.label.clone()),
            kind: match (&plan.archive, &plan.collection) {
                (Some(_), _) => "archive",
                (None, Some(collection)) => collection.kind,
                (None, None) => "files",
            },
            items,
            new_count,
            duplicate_count,
            error_count,
            attachment_count,
            notebook_count,
            tag_count,
            task_count: plan
                .archive
                .as_ref()
                .map_or(0, |archive| archive.manifest.planner.tasks.len() as u64),
            event_count: plan
                .archive
                .as_ref()
                .map_or(0, |archive| archive.manifest.planner.events.len() as u64),
        })
    }

    /// Applica il piano in un'unica transazione. In caso di errore nessuna
    /// nota viene importata e i file allegati già scritti vengono rimossi.
    pub fn commit_import(
        &self,
        plan: &ImportPlan,
        destination: &ImportDestination,
    ) -> Result<ImportReport, StorageError> {
        let data_directory = self.data_directory()?.to_path_buf();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let mut written = Vec::new();
        let result = match &plan.archive {
            Some(archive) => {
                commit_archive(&transaction, archive, plan, &data_directory, &mut written)
            }
            None => commit_files(
                &transaction,
                plan,
                destination,
                &data_directory,
                &mut written,
            ),
        };
        match result.and_then(|report| transaction.commit().map(|()| report).map_err(Into::into)) {
            Ok(report) => Ok(report),
            Err(error) => {
                for path in written {
                    if let Err(cleanup) = fs::remove_file(&path) {
                        tracing::warn!(
                            "allegato importato non rimosso dopo il rollback: {cleanup}"
                        );
                    }
                }
                Err(error)
            }
        }
    }
}

/// Toglie dal cestino un contenitore citato da un'importazione: senza questo
/// le note importate finirebbero collegate a un elemento invisibile.
fn untrash(
    transaction: &Transaction<'_>,
    table: &'static str,
    id: &str,
) -> Result<(), StorageError> {
    debug_assert!(matches!(table, "spaces" | "notebooks" | "tags"));
    transaction.execute(
        &format!("UPDATE {table} SET deleted_at = NULL WHERE id = ?1 AND deleted_at IS NOT NULL"),
        [id],
    )?;
    transaction.execute(
        "DELETE FROM trash_relations WHERE item_type = ?1 AND item_id = ?2",
        params![table.trim_end_matches('s'), id],
    )?;
    Ok(())
}

fn find_or_create_tag(transaction: &Transaction<'_>, name: &str) -> Result<String, StorageError> {
    if let Some(id) = transaction
        .query_row(
            "SELECT id FROM tags WHERE name = ?1 COLLATE NOCASE",
            [name],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    {
        // Un tag nel cestino citato dai file importati torna disponibile.
        untrash(transaction, "tags", &id)?;
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    transaction.execute(
        "INSERT INTO tags (id, name) VALUES (?1, ?2)",
        params![id, name],
    )?;
    Ok(id)
}

struct NewAttachment<'a> {
    note_id: &'a str,
    id: String,
    filename: &'a str,
    bytes: &'a [u8],
    created_at: Option<&'a str>,
}

fn insert_attachment(
    transaction: &Transaction<'_>,
    data_directory: &Path,
    attachment: NewAttachment<'_>,
    written: &mut Vec<PathBuf>,
) -> Result<bool, StorageError> {
    let NewAttachment {
        note_id,
        id: attachment_id,
        filename,
        bytes,
        created_at,
    } = attachment;
    let filename = normalize_filename(filename)?;
    let Ok((mime_type, extension)) = sniff_attachment_type(bytes) else {
        return Ok(false);
    };
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    let duplicate: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM attachments
                       WHERE note_id = ?1 AND sha256 = ?2 AND deleted_at IS NULL)",
        params![note_id, sha256],
        |row| row.get(0),
    )?;
    if duplicate {
        return Ok(false);
    }
    let stored_filename = format!("{attachment_id}.{extension}");
    written.push(write_attachment_file(
        data_directory,
        &stored_filename,
        bytes,
    )?);
    transaction.execute(
        "INSERT INTO attachments (
             id, note_id, original_filename, stored_filename, mime_type, size_bytes, sha256,
             search_text, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                   COALESCE(?9, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')))",
        params![
            attachment_id,
            note_id,
            filename,
            stored_filename,
            mime_type,
            bytes.len() as i64,
            sha256,
            attachment_search_text(&filename, mime_type, bytes),
            created_at.filter(|value| is_utc_timestamp(value)),
        ],
    )?;
    Ok(true)
}

fn commit_files(
    transaction: &Transaction<'_>,
    plan: &ImportPlan,
    destination: &ImportDestination,
    data_directory: &Path,
    written: &mut Vec<PathBuf>,
) -> Result<ImportReport, StorageError> {
    let notebook_id = match destination {
        ImportDestination::NewNotebook { name } => {
            let name = normalize_organization_name(name)?;
            let id = Uuid::new_v4().to_string();
            transaction.execute(
                "INSERT INTO notebooks (id, name, description) VALUES (?1, ?2, '')",
                params![id, name],
            )?;
            Some(id)
        }
        ImportDestination::Notebook { id } => {
            let exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM notebooks WHERE id = ?1 AND deleted_at IS NULL)",
                [id],
                |row| row.get(0),
            )?;
            if !exists {
                return Err(StorageError::NotebookNotFound);
            }
            Some(id.clone())
        }
        ImportDestination::Unfiled => None,
        ImportDestination::KeepStructure { .. } => None,
    };
    let mut structure = match destination {
        ImportDestination::KeepStructure { space_name } => Some(NotebookStructure::new(
            transaction,
            space_name.as_deref(),
            plan.collection
                .as_ref()
                .map_or("Importazione", |collection| collection.label.as_str()),
        )?),
        _ => None,
    };
    let mut fingerprints = existing_fingerprints(transaction)?;
    let mut report = ImportReport {
        imported: 0,
        skipped: 0,
        failed: 0,
        attachments: 0,
        planner_items: 0,
        notebook_id: notebook_id.clone(),
        notebooks: 0,
        links: 0,
        items: Vec::new(),
    };
    // Chiave d'origine → id della nota importata, per i collegamenti.
    let mut keys: HashMap<String, String> = HashMap::new();
    let mut pending_links: Vec<(String, &PlannedLink)> = Vec::new();
    for planned in &plan.files {
        let parsed = match &planned.outcome {
            Err(message) => {
                report.failed += 1;
                report.items.push(ImportItem {
                    source: planned.source_name.clone(),
                    title: String::new(),
                    status: ImportItemStatus::Failed,
                    message: Some(message.clone()),
                });
                continue;
            }
            Ok(parsed) => parsed,
        };
        if !fingerprints.insert(parsed.fingerprint.clone()) {
            report.skipped += 1;
            report.items.push(ImportItem {
                source: planned.source_name.clone(),
                title: parsed.title.clone(),
                status: ImportItemStatus::Skipped,
                message: None,
            });
            continue;
        }
        let id = Uuid::new_v4().to_string();
        let notebook_id = match structure.as_mut() {
            Some(structure) => Some(structure.notebook_for(transaction, &parsed.notebook_path)?),
            None => notebook_id.clone(),
        };
        for key in &parsed.link_keys {
            keys.entry(key.clone()).or_insert_with(|| id.clone());
        }
        pending_links.extend(parsed.links.iter().map(|link| (id.clone(), link)));
        transaction.execute(
            "INSERT INTO notes (id, notebook_id, title, content_json, created_at, updated_at, source)
             VALUES (?1, ?2, ?3, ?4,
                     COALESCE(?5, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                     COALESCE(?6, ?5, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                     ?7)",
            params![
                id,
                notebook_id,
                parsed.title,
                serde_json::to_string(&parsed.document)?,
                parsed.created_at,
                parsed.updated_at,
                parsed.source,
            ],
        )?;
        sync_search_index(transaction, &id, &parsed.title, &parsed.document)?;
        for tag in &parsed.tags {
            let tag_id = find_or_create_tag(transaction, tag)?;
            transaction.execute(
                "INSERT OR IGNORE INTO note_tags (note_id, tag_id) VALUES (?1, ?2)",
                params![id, tag_id],
            )?;
        }
        let mut warnings = parsed.warnings.clone();
        let mut images: HashMap<String, String> = HashMap::new();
        for attachment in &parsed.attachments {
            let attachment_id = Uuid::new_v4().to_string();
            let stored = insert_attachment(
                transaction,
                data_directory,
                NewAttachment {
                    note_id: &id,
                    id: attachment_id.clone(),
                    filename: &attachment.filename,
                    bytes: &attachment.bytes,
                    created_at: None,
                },
                written,
            )?;
            if stored {
                report.attachments += 1;
                if sniff_attachment_type(&attachment.bytes)
                    .is_ok_and(|(mime, _)| mime.starts_with("image/"))
                {
                    images.insert(attachment.filename.clone(), attachment_id);
                }
            } else {
                warnings.push(format!("allegato ignorato: {}", attachment.filename));
            }
        }
        if plan.collection.is_some() && !images.is_empty() {
            // Le immagini incorporate tornano nel testo, al loro posto.
            let document = crate::external_import::markers_to_images(&parsed.document, &images);
            transaction.execute(
                "UPDATE notes SET content_json = ?1 WHERE id = ?2",
                params![serde_json::to_string(&document)?, id],
            )?;
            sync_search_index(transaction, &id, &parsed.title, &document)?;
        }
        report.imported += 1;
        report.items.push(ImportItem {
            source: planned.source_name.clone(),
            title: parsed.title.clone(),
            status: ImportItemStatus::Imported,
            message: warning_message(&warnings),
        });
    }
    for (source_id, link) in pending_links {
        let Some(target_id) = keys.get(&link.target_key) else {
            continue;
        };
        if *target_id == source_id {
            continue;
        }
        report.links += transaction.execute(
            "INSERT OR IGNORE INTO note_links (source_note_id, target_note_id, label)
             VALUES (?1, ?2, ?3)",
            params![source_id, target_id, link.label],
        )? as u64;
    }
    if let Some(structure) = structure {
        report.notebooks = structure.created;
        report.notebook_id = structure.first_notebook;
    }
    Ok(report)
}

/// Taccuini creati per mantenere la struttura d'origine. Greatnote ha
/// taccuini piatti dentro gli spazi: le cartelle annidate diventano taccuini
/// con il percorso nel nome ("Lavoro › Progetti").
struct NotebookStructure {
    space_id: Option<String>,
    root_name: String,
    by_path: HashMap<Vec<String>, String>,
    created: u64,
    first_notebook: Option<String>,
}

impl NotebookStructure {
    fn new(
        transaction: &Transaction<'_>,
        space_name: Option<&str>,
        root_name: &str,
    ) -> Result<Self, StorageError> {
        let space_id = match space_name.map(str::trim).filter(|name| !name.is_empty()) {
            Some(name) => {
                let name = normalize_organization_name(name)?;
                let id = Uuid::new_v4().to_string();
                transaction.execute(
                    "INSERT INTO spaces (id, name, description) VALUES (?1, ?2, '')",
                    params![id, name],
                )?;
                Some(id)
            }
            None => None,
        };
        Ok(Self {
            space_id,
            root_name: root_name.to_owned(),
            by_path: HashMap::new(),
            created: 0,
            first_notebook: None,
        })
    }

    fn notebook_for(
        &mut self,
        transaction: &Transaction<'_>,
        path: &[String],
    ) -> Result<String, StorageError> {
        if let Some(id) = self.by_path.get(path) {
            return Ok(id.clone());
        }
        let joined = if path.is_empty() {
            self.root_name.clone()
        } else {
            path.join(" › ")
        };
        let name = notebook_name(&joined);
        // Senza un nuovo spazio, un taccuino con lo stesso nome già presente
        // fuori dagli spazi viene riutilizzato (reimportazioni).
        let existing = if self.space_id.is_none() {
            transaction
                .query_row(
                    "SELECT id FROM notebooks
                     WHERE name = ?1 AND space_id IS NULL AND deleted_at IS NULL
                     ORDER BY created_at LIMIT 1",
                    [&name],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
        } else {
            None
        };
        let id = match existing {
            Some(id) => id,
            None => {
                let id = Uuid::new_v4().to_string();
                transaction.execute(
                    "INSERT INTO notebooks (id, space_id, name, description, sort_order)
                     VALUES (?1, ?2, ?3, '', ?4)",
                    params![id, self.space_id, name, self.created as i64],
                )?;
                self.created += 1;
                id
            }
        };
        self.first_notebook.get_or_insert_with(|| id.clone());
        self.by_path.insert(path.to_vec(), id.clone());
        Ok(id)
    }
}

/// Nome di taccuino valido: spazi normalizzati, al massimo 200 caratteri.
fn notebook_name(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let cleaned = if cleaned.is_empty() {
        "Importazione".to_owned()
    } else {
        cleaned
    };
    if cleaned.chars().count() <= 200 {
        cleaned
    } else {
        cleaned.chars().take(199).collect::<String>() + "…"
    }
}

fn valid_timestamp(value: &str) -> Option<&str> {
    Some(value).filter(|value| is_utc_timestamp(value))
}

fn commit_archive(
    transaction: &Transaction<'_>,
    archive_plan: &ArchivePlan,
    plan: &ImportPlan,
    data_directory: &Path,
    written: &mut Vec<PathBuf>,
) -> Result<ImportReport, StorageError> {
    let manifest = &archive_plan.manifest;
    let mut archive = ZipArchive::new(File::open(&archive_plan.path)?)?;
    let attachment_entries: HashMap<&str, &ManifestFile> = manifest
        .attachments
        .iter()
        .map(|file| (file.path.as_str(), file))
        .collect();

    for space in &manifest.spaces {
        if Uuid::parse_str(&space.id).is_err() {
            continue;
        }
        let Ok(name) = normalize_organization_name(&space.name) else {
            continue;
        };
        transaction.execute(
            "INSERT OR IGNORE INTO spaces (id, name, description, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4,
                     COALESCE(?5, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                     COALESCE(?6, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')))",
            params![
                space.id,
                name,
                space.description.chars().take(4096).collect::<String>(),
                space.sort_order,
                valid_timestamp(&space.created_at),
                valid_timestamp(&space.updated_at)
            ],
        )?;
        untrash(transaction, "spaces", &space.id)?;
    }
    for notebook in &manifest.notebooks {
        if Uuid::parse_str(&notebook.id).is_err() {
            continue;
        }
        let Ok(name) = normalize_organization_name(&notebook.name) else {
            continue;
        };
        transaction.execute(
            "INSERT OR IGNORE INTO notebooks
                 (id, space_id, name, description, sort_order, created_at, updated_at)
             VALUES (?1, (SELECT id FROM spaces WHERE id = ?2), ?3, ?4, ?5,
                     COALESCE(?6, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                     COALESCE(?7, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')))",
            params![
                notebook.id,
                notebook.space_id,
                name,
                notebook.description.chars().take(4096).collect::<String>(),
                notebook.sort_order,
                valid_timestamp(&notebook.created_at),
                valid_timestamp(&notebook.updated_at)
            ],
        )?;
        untrash(transaction, "notebooks", &notebook.id)?;
    }
    let mut tag_map = HashMap::new();
    for tag in &manifest.tags {
        let Ok(name) = normalize_tag_name(&tag.name) else {
            continue;
        };
        let existing: Option<String> = transaction
            .query_row("SELECT id FROM tags WHERE id = ?1", [&tag.id], |row| {
                row.get(0)
            })
            .optional()?;
        let id = match existing {
            Some(id) => {
                untrash(transaction, "tags", &id)?;
                id
            }
            None => find_or_create_tag(transaction, &name)?,
        };
        tag_map.insert(tag.id.clone(), id);
    }

    let mut report = ImportReport {
        imported: 0,
        skipped: 0,
        failed: 0,
        attachments: 0,
        planner_items: 0,
        notebook_id: None,
        notebooks: 0,
        links: 0,
        items: Vec::new(),
    };
    let mut imported_ids = Vec::new();
    let mut notes = archive_plan.notes.iter();
    for planned in &plan.files {
        let parsed = match &planned.outcome {
            Err(message) => {
                report.failed += 1;
                report.items.push(ImportItem {
                    source: planned.source_name.clone(),
                    title: String::new(),
                    status: ImportItemStatus::Failed,
                    message: Some(message.clone()),
                });
                continue;
            }
            Ok(parsed) => parsed,
        };
        let Some(note) = notes.next() else { break };
        if note_exists(transaction, &note.id)? {
            report.skipped += 1;
            report.items.push(ImportItem {
                source: planned.source_name.clone(),
                title: parsed.title.clone(),
                status: ImportItemStatus::Skipped,
                message: None,
            });
            continue;
        }
        let title = normalize_title(&note.title)?;
        transaction.execute(
            "INSERT INTO notes (id, notebook_id, title, content_json, created_at, updated_at,
                                is_pinned, archived_at, source)
             VALUES (?1, (SELECT id FROM notebooks WHERE id = ?2), ?3, ?4,
                     COALESCE(?5, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                     COALESCE(?6, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                     ?7, ?8, 'gna-archive')",
            params![
                note.id,
                note.notebook_id,
                title,
                serde_json::to_string(&note.content)?,
                valid_timestamp(&note.created_at),
                valid_timestamp(&note.updated_at),
                note.is_pinned,
                note.archived_at
                    .as_deref()
                    .filter(|value| is_utc_timestamp(value)),
            ],
        )?;
        sync_search_index(transaction, &note.id, &title, &note.content)?;
        for tag_id in &note.tag_ids {
            if let Some(mapped) = tag_map.get(tag_id) {
                transaction.execute(
                    "INSERT OR IGNORE INTO note_tags (note_id, tag_id) VALUES (?1, ?2)",
                    params![note.id, mapped],
                )?;
            }
        }
        let mut warnings = Vec::new();
        let mut remapped: HashMap<String, String> = HashMap::new();
        for attachment in &note.attachments {
            let entry = attachment_entries
                .get(attachment.path.as_str())
                .ok_or_else(|| StorageError::InvalidImport("allegato mancante".to_owned()))?;
            let bytes = verified_entry(&mut archive, entry, MAX_ATTACHMENT_BYTES as u64)?;
            if bytes.len() as u64 != attachment.size_bytes
                || format!("{:x}", Sha256::digest(&bytes)) != attachment.sha256
            {
                return Err(StorageError::InvalidImport(format!(
                    "allegato danneggiato: {}",
                    attachment.original_filename
                )));
            }
            let attachment_id = match Uuid::parse_str(&attachment.id) {
                Ok(_)
                    if !transaction.query_row(
                        "SELECT EXISTS(SELECT 1 FROM attachments WHERE id = ?1)",
                        [&attachment.id],
                        |row| row.get::<_, bool>(0),
                    )? =>
                {
                    attachment.id.clone()
                }
                _ => {
                    let fresh = Uuid::new_v4().to_string();
                    remapped.insert(attachment.id.clone(), fresh.clone());
                    fresh
                }
            };
            if insert_attachment(
                transaction,
                data_directory,
                NewAttachment {
                    note_id: &note.id,
                    id: attachment_id,
                    filename: &attachment.original_filename,
                    bytes: &bytes,
                    created_at: Some(&attachment.created_at),
                },
                written,
            )? {
                report.attachments += 1;
            } else {
                warnings.push(format!(
                    "allegato ignorato: {}",
                    attachment.original_filename
                ));
            }
        }
        if !remapped.is_empty() {
            // Le immagini nel testo seguono gli allegati con un nuovo id.
            let mut content = note.content.clone();
            crate::domain::note::remap_image_attachments(&mut content, &remapped);
            transaction.execute(
                "UPDATE notes SET content_json = ?1 WHERE id = ?2",
                params![serde_json::to_string(&content)?, note.id],
            )?;
        }
        imported_ids.push(note);
        report.imported += 1;
        report.items.push(ImportItem {
            source: planned.source_name.clone(),
            title,
            status: ImportItemStatus::Imported,
            message: warning_message(&warnings),
        });
    }
    report.planner_items =
        crate::planner::import_planner(transaction, &manifest.planner, crate::time::now())?;
    for note in imported_ids {
        for link in &note.links {
            if link.target_note_id == note.id {
                continue;
            }
            transaction.execute(
                "INSERT OR IGNORE INTO note_links (source_note_id, target_note_id, label)
                 SELECT ?1, ?2, ?3
                 WHERE EXISTS (SELECT 1 FROM notes WHERE id = ?2)",
                params![
                    note.id,
                    link.target_note_id,
                    link.label
                        .as_deref()
                        .map(|label| label.chars().take(200).collect::<String>())
                ],
            )?;
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write, path::PathBuf};

    use serde_json::json;

    use super::*;
    use crate::domain::note::empty_document;

    fn document(text: &str) -> Value {
        json!({ "type": "doc", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }] })
    }

    fn open(directory: &Path) -> Database {
        Database::open(directory.join("gna.sqlite3")).expect("database opens")
    }

    #[test]
    fn importing_a_trashed_tag_name_restores_the_tag() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = open(directory.path());
        let tag = database.create_tag("progetto", None).expect("tag");
        database
            .move_item_to_trash(crate::trash::TrashKind::Tag, &tag.id)
            .expect("trash");
        {
            let mut connection = database.connection().expect("connection");
            let transaction = connection.transaction().expect("transaction");
            assert_eq!(
                find_or_create_tag(&transaction, "Progetto").expect("tag"),
                tag.id
            );
            transaction.commit().expect("commit");
        }
        assert_eq!(database.list_tags().expect("tags")[0].id, tag.id);
        assert!(database.list_trash_items().expect("trash").is_empty());
    }

    #[test]
    fn file_names_are_portable_and_unique() {
        assert_eq!(
            sanitize_file_name("a/b\\c:d*e?f\"g<h>i|j", "x"),
            "a_b_c_d_e_f_g_h_i_j"
        );
        assert_eq!(sanitize_file_name("  ..  ", "Nota"), "Nota");
        assert_eq!(sanitize_file_name("con.txt", "x"), "_con.txt");
        assert_eq!(sanitize_file_name("COM1", "x"), "_COM1");
        assert_eq!(
            sanitize_file_name("../../etc/passwd", "x"),
            "_.._etc_passwd"
        );
        let mut used = HashSet::new();
        assert_eq!(unique_name(&mut used, "Nota", ".md"), "Nota.md");
        assert_eq!(unique_name(&mut used, "nota", ".md"), "nota (2).md");
    }

    #[test]
    fn single_note_exports_include_metadata_and_escape_html() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = open(directory.path());
        let notebook = database
            .create_notebook(None, "Lavoro", "")
            .expect("notebook");
        let note = database
            .create_note_in_notebook(
                "<Piano>",
                &document("Testo <b>non</b> HTML"),
                Some(&notebook.id),
            )
            .expect("note");
        let tag = database.create_tag("progetto", None).expect("tag");
        database.set_note_tags(&note.id, &[tag.id]).expect("tags");

        let markdown = database
            .export_note(&note.id, ExportFormat::Markdown, "it")
            .expect("md");
        assert_eq!(markdown.file_name, "_Piano_.md");
        assert!(markdown.contents.contains("tags: [\"progetto\"]"));
        assert!(markdown.contents.contains("notebook: \"Lavoro\""));
        let html = database
            .export_note(&note.id, ExportFormat::Html, "it")
            .expect("html");
        assert!(html.contents.contains("Testo &lt;b&gt;non&lt;/b&gt; HTML"));
        let text = database
            .export_note(&note.id, ExportFormat::Text, "it")
            .expect("txt");
        assert_eq!(text.contents, "<Piano>\n\nTesto <b>non</b> HTML\n");
        assert!(matches!(
            database.export_note("missing", ExportFormat::Text, "it"),
            Err(StorageError::NoteNotFound)
        ));
    }

    #[test]
    fn markdown_zip_export_round_trips_through_folder_import() {
        let source_directory = tempfile::tempdir().expect("temporary directory");
        let source = open(source_directory.path());
        let notebook = source
            .create_notebook(None, "Ricette", "")
            .expect("notebook");
        let note = source
            .create_note_in_notebook("Pane", &document("farina e acqua"), Some(&notebook.id))
            .expect("note");
        source
            .create_note_in_notebook("Pane", &document("altra versione"), Some(&notebook.id))
            .expect("same title");
        source
            .add_attachment(
                &note.id,
                "dosi.txt",
                b"500 g farina",
                source_directory.path(),
            )
            .expect("attachment");
        let tag = source.create_tag("cucina", None).expect("tag");
        source.set_note_tags(&note.id, &[tag.id]).expect("tags");

        let zip_path = source_directory.path().join("ricette.zip");
        let summary = source
            .export_markdown_zip(Some(&notebook.id), "Senza taccuino", &zip_path)
            .expect("export");
        assert_eq!(
            summary,
            ExportSummary {
                note_count: 2,
                attachment_count: 1
            }
        );

        let extracted = tempfile::tempdir().expect("extract directory");
        let mut archive = ZipArchive::new(File::open(&zip_path).expect("zip")).expect("archive");
        let mut names = Vec::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).expect("entry");
            let name = entry.enclosed_name().expect("safe name");
            // Nome grezzo nell'archivio: sempre con `/`, su ogni sistema.
            names.push(entry.name().expect("entry name").to_string());
            let target = extracted.path().join(&name);
            fs::create_dir_all(target.parent().expect("parent")).expect("dirs");
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("read");
            fs::write(target, bytes).expect("write");
        }
        names.sort();
        assert_eq!(names.len(), 3);
        assert_eq!(&names[..2], ["Ricette/Pane (2).md", "Ricette/Pane.md"]);
        assert!(names[2].starts_with("Ricette/attachments/Pane"));
        assert!(names[2].ends_with("/dosi.txt"));

        let target_directory = tempfile::tempdir().expect("target directory");
        let target = open(target_directory.path());
        let plan = plan_folder_import(&extracted.path().join("Ricette")).expect("plan");
        let preview = target.preview_import(&plan).expect("preview");
        assert_eq!(
            (
                preview.new_count,
                preview.duplicate_count,
                preview.error_count
            ),
            (2, 0, 0)
        );
        assert_eq!(preview.attachment_count, 1);
        assert!(target.list_notes(10, 0).expect("dry run").is_empty());

        let report = target
            .commit_import(
                &plan,
                &ImportDestination::NewNotebook {
                    name: "Importazione".to_owned(),
                },
            )
            .expect("commit");
        assert_eq!((report.imported, report.attachments), (2, 1));
        let notes = target.list_notes(10, 0).expect("notes");
        assert!(
            notes
                .iter()
                .all(|note| note.notebook_id == report.notebook_id)
        );
        let imported = notes
            .iter()
            .find(|note| to_plain_text(&note.content) == "farina e acqua")
            .expect("imported note");
        assert_eq!(imported.created_at, note.created_at);
        assert_eq!(
            target.list_note_tags(&imported.id).expect("tags")[0].name,
            "cucina"
        );
        assert_eq!(
            target
                .list_attachments(&imported.id)
                .expect("attachments")
                .len(),
            1
        );
        assert_eq!(
            target
                .search_notes("500", "all", None, 10, 0)
                .expect("search")
                .len(),
            1
        );

        // Una seconda importazione riconosce i duplicati.
        let preview = target.preview_import(&plan).expect("preview");
        assert_eq!((preview.new_count, preview.duplicate_count), (0, 2));
        let report = target
            .commit_import(&plan, &ImportDestination::Unfiled)
            .expect("commit");
        assert_eq!((report.imported, report.skipped), (0, 2));
    }

    #[test]
    fn archive_round_trip_preserves_organization_links_and_attachments() {
        let source_directory = tempfile::tempdir().expect("temporary directory");
        let source = open(source_directory.path());
        let space = source.create_space("Lavoro", "progetti").expect("space");
        let notebook = source
            .create_notebook(Some(&space.id), "Clienti", "")
            .expect("notebook");
        let first = source
            .create_note_in_notebook(
                "Cliente A",
                &document("contratto firmato"),
                Some(&notebook.id),
            )
            .expect("note");
        let second = source
            .create_note("Riunione", &document("verbale"))
            .expect("note");
        source.set_note_pinned(&first.id, true).expect("pin");
        source.set_note_archived(&second.id, true).expect("archive");
        source
            .create_note_link(&second.id, &first.id, Some("vedi"))
            .expect("link");
        let tag = source.create_tag("importante", None).expect("tag");
        source
            .set_note_tags(&first.id, std::slice::from_ref(&tag.id))
            .expect("tags");
        source
            .add_attachment(
                &first.id,
                "contratto.txt",
                b"clausola riservata",
                source_directory.path(),
            )
            .expect("attachment");
        let trashed = source
            .create_note("Cestinata", &empty_document())
            .expect("note");
        let linked_task = source
            .create_task(&crate::domain::planner::TaskInput {
                title: "Rinnovo".to_owned(),
                description: "contratto".to_owned(),
                priority: 2,
                due_date: Some("2026-11-01".to_owned()),
                note_id: Some(first.id.clone()),
                flagged: false,
                recurrence: None,
            })
            .expect("task");
        let orphan_task = source
            .create_task(&crate::domain::planner::TaskInput {
                title: "Da nota cestinata".to_owned(),
                description: String::new(),
                priority: 0,
                due_date: None,
                note_id: Some(trashed.id.clone()),
                flagged: false,
                recurrence: None,
            })
            .expect("task");
        source.trash_note(&trashed.id).expect("trash");
        source
            .set_task_completed(
                &orphan_task.id,
                true,
                crate::time::now(),
                &crate::time::Zone::system(),
            )
            .expect("done");
        let event = source
            .create_event(&crate::domain::planner::EventInput {
                title: "Firma".to_owned(),
                description: String::new(),
                all_day: false,
                start: "2026-11-02T10:00".to_owned(),
                end: "2026-11-02T11:00".to_owned(),
                timezone: Some("Europe/Rome".to_owned()),
                note_id: None,
            })
            .expect("event");
        let created_at = crate::time::parse_utc("2026-10-14T00:00:00Z").expect("now");
        source
            .create_reminder(
                crate::domain::planner::ReminderEntity::Task,
                &linked_task.id,
                "2026-10-31T09:00",
                Some("Europe/Rome"),
                created_at,
            )
            .expect("future reminder");
        source
            .create_reminder(
                crate::domain::planner::ReminderEntity::Event,
                &event.id,
                "2026-10-14T09:00",
                Some("Europe/Rome"),
                created_at,
            )
            .expect("past reminder");

        let archive_path = source_directory.path().join("export.zip");
        let summary = source.export_archive(&archive_path).expect("export");
        assert_eq!(
            summary,
            ExportSummary {
                note_count: 2,
                attachment_count: 1
            }
        );

        let target_directory = tempfile::tempdir().expect("target directory");
        let target = open(target_directory.path());
        target
            .create_tag("IMPORTANTE", None)
            .expect("existing tag with same name");
        let plan = plan_archive_import(&archive_path).expect("plan");
        let preview = target.preview_import(&plan).expect("preview");
        assert_eq!(
            (preview.kind, preview.new_count, preview.notebook_count),
            ("archive", 2, 1)
        );
        assert_eq!((preview.task_count, preview.event_count), (2, 1));
        let report = target
            .commit_import(&plan, &ImportDestination::Unfiled)
            .expect("commit");
        assert_eq!((report.imported, report.attachments), (2, 1));
        assert_eq!(report.planner_items, 5);

        let task = target.get_task(&linked_task.id).expect("task");
        assert_eq!(task.note_id.as_deref(), Some(first.id.as_str()));
        assert_eq!(
            (
                task.priority,
                task.due_date.as_deref(),
                task.description.as_str()
            ),
            (2, Some("2026-11-01"), "contratto")
        );
        assert_eq!(
            task.next_reminder_at.as_deref(),
            Some("2026-10-31T08:00:00Z")
        );
        let orphan = target.get_task(&orphan_task.id).expect("orphan task");
        assert!(orphan.note_id.is_none());
        assert!(orphan.completed_at.is_some());
        let rome = crate::time::Zone::resolve(Some("Europe/Rome")).expect("tz");
        let day = crate::time::parse_date("2026-11-02").expect("date");
        assert_eq!(
            target.list_events(day, day, &rome).expect("events")[0].id,
            event.id
        );
        // Il promemoria già passato non genera una notifica arretrata.
        let reminders = target.list_reminders().expect("reminders");
        assert_eq!(reminders.len(), 2);
        assert!(
            target
                .take_due_reminders(crate::time::parse_utc("2026-10-20T00:00:00Z").expect("t"))
                .expect("due")
                .is_empty()
        );

        let restored = target.get_note(&first.id).expect("first note");
        assert_eq!(restored.notebook_id.as_deref(), Some(notebook.id.as_str()));
        assert!(restored.is_pinned);
        assert_eq!(restored.created_at, first.created_at);
        assert!(
            target
                .get_note(&second.id)
                .expect("second")
                .archived_at
                .is_some()
        );
        assert_eq!(
            target.list_note_links(&second.id).expect("links")[0].target_note_id,
            first.id
        );
        let tags = target.list_note_tags(&first.id).expect("tags");
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "IMPORTANTE");
        let spaces = target.list_spaces().expect("spaces");
        assert_eq!(spaces[0].id, space.id);
        let attachments = target.list_attachments(&first.id).expect("attachments");
        target
            .verified_attachment_path(&first.id, &attachments[0].id, target_directory.path())
            .expect("attachment integrity");
        assert_eq!(
            target
                .search_notes("riservata", "all", None, 10, 0)
                .expect("search")
                .len(),
            1
        );
        assert!(target.check_integrity().expect("integrity").ok);

        // Reimportare lo stesso archivio non duplica nulla.
        let report = target
            .commit_import(&plan, &ImportDestination::Unfiled)
            .expect("again");
        assert_eq!((report.imported, report.skipped), (0, 2));
        assert_eq!(report.planner_items, 0);
    }

    #[test]
    fn failed_archive_import_rolls_back_notes_and_files() {
        let source_directory = tempfile::tempdir().expect("temporary directory");
        let source = open(source_directory.path());
        let first = source.create_note("Primo", &document("uno")).expect("note");
        let second = source
            .create_note("Secondo", &document("due"))
            .expect("note");
        source
            .add_attachment(&first.id, "a.txt", b"primo", source_directory.path())
            .expect("attachment");
        source
            .add_attachment(&second.id, "b.txt", b"secondo", source_directory.path())
            .expect("attachment");
        // Ordine deterministico: il primo allegato viene scritto prima dell'errore.
        source
            .connection()
            .expect("connection")
            .execute(
                "UPDATE notes SET created_at = '2020-01-01T00:00:00.000Z' WHERE id = ?1",
                [&first.id],
            )
            .expect("created_at");
        let archive_path = source_directory.path().join("export.zip");
        source.export_archive(&archive_path).expect("export");
        let plan = plan_archive_import(&archive_path).expect("plan");

        // L'archivio viene alterato tra anteprima e commit.
        let mut archive = ZipArchive::new(File::open(&archive_path).expect("open")).expect("zip");
        let tampered: PathBuf = source_directory.path().join("tampered.zip");
        let mut writer = ZipWriter::new(File::create(&tampered).expect("create"));
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).expect("entry");
            let name = entry.name().expect("name").to_string();
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes).expect("read");
            if bytes == b"secondo" {
                bytes = b"SECONDO".to_vec();
            }
            writer.start_file(name, zip_options()).expect("start");
            writer.write_all(&bytes).expect("write");
        }
        writer.finish().expect("finish");
        fs::rename(&tampered, &archive_path).expect("replace");

        let target_directory = tempfile::tempdir().expect("target directory");
        let target = open(target_directory.path());
        assert!(
            target
                .commit_import(&plan, &ImportDestination::Unfiled)
                .is_err()
        );
        assert!(target.list_notes(10, 0).expect("notes").is_empty());
        let leftovers = fs::read_dir(target_directory.path().join("attachments"))
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn import_rejects_unsafe_inputs_and_reports_errors() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path().join("import");
        fs::create_dir_all(root.join("sub")).expect("dirs");
        fs::write(root.join("ok.md"), "# Titolo\n\ncorpo").expect("write");
        fs::write(root.join("latin1.txt"), [0x63, 0x61, 0x66, 0xe8]).expect("write");
        fs::write(
            root.join("pagina.html"),
            "<title>Pagina</title><p>ciao<script>x()</script></p>",
        )
        .expect("write");
        fs::write(directory.path().join("segreto.txt"), "segreto").expect("write");
        fs::write(
            root.join("sub").join("fuga.md"),
            "---\nattachments: [\"../../segreto.txt\", \"/etc/passwd\"]\n---\n# Fuga\n",
        )
        .expect("write");
        fs::write(root.join(".nascosto.md"), "ignorato").expect("write");
        fs::write(root.join("immagine.bin"), [0u8; 4]).expect("write");
        #[cfg(unix)]
        std::os::unix::fs::symlink(directory.path().join("segreto.txt"), root.join("link.md"))
            .expect("symlink");

        let database = open(directory.path());
        let plan = plan_folder_import(&root).expect("plan");
        let preview = database.preview_import(&plan).expect("preview");
        let sources: Vec<_> = preview
            .items
            .iter()
            .map(|item| item.source.as_str())
            .collect();
        assert_eq!(
            sources,
            vec!["latin1.txt", "ok.md", "pagina.html", "sub/fuga.md"]
        );
        assert_eq!(preview.error_count, 1);
        let escape = preview
            .items
            .iter()
            .find(|item| item.source == "sub/fuga.md")
            .expect("item");
        assert_eq!(escape.status, ImportItemStatus::New);
        assert!(
            escape
                .message
                .as_deref()
                .unwrap_or("")
                .contains("non consentito")
        );
        let report = database
            .commit_import(&plan, &ImportDestination::Unfiled)
            .expect("commit");
        assert_eq!(
            (report.imported, report.failed, report.attachments),
            (3, 1, 0)
        );
        let html_note = database
            .list_notes(10, 0)
            .expect("notes")
            .into_iter()
            .find(|note| note.title == "Pagina")
            .expect("html note");
        assert!(!html_note.content.to_string().contains("x()"));

        assert!(plan_archive_import(&root.join("ok.md")).is_err());
        assert!(matches!(
            database.commit_import(
                &plan_files_import(&[root.join("ok.md")]).expect("plan"),
                &ImportDestination::Notebook {
                    id: "missing".to_owned()
                }
            ),
            Err(StorageError::NotebookNotFound)
        ));
    }
}
