use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use rusqlite::{
    Connection, OptionalExtension, Row, params, params_from_iter,
    types::{Type, Value as SqlValue},
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::domain::{
    note::{Attachment, Note, NoteDraft, NoteLink, validate_document},
    organization::{Notebook, Space, Tag},
};
use crate::search::{SearchQuery, parse_query};

const MIGRATION_1: &str = include_str!("../migrations/0001_initial.sql");
const MIGRATION_2: &str = include_str!("../migrations/0002_note_drafts.sql");
const MIGRATION_3: &str = include_str!("../migrations/0003_search_productivity.sql");
const MIGRATION_4: &str = include_str!("../migrations/0004_attachment_search_text.sql");
const MIGRATION_5: &str = include_str!("../migrations/0005_settings_and_index_repair.sql");
const MIGRATION_6: &str = include_str!("../migrations/0006_tasks_reminders_events.sql");
const MIGRATION_7: &str = include_str!("../migrations/0007_trash_for_items.sql");
const MIGRATION_8: &str = include_str!("../migrations/0008_revisions_templates_flags.sql");
const MIGRATION_9: &str = include_str!("../migrations/0009_ai_semantic_index.sql");
const MIGRATION_10: &str = include_str!("../migrations/0010_ai_history.sql");
const MIGRATION_11: &str = include_str!("../migrations/0011_image_text.sql");
const MIGRATION_12: &str = include_str!("../migrations/0012_external_calendars.sql");
const MIGRATION_13: &str = include_str!("../migrations/0013_calendar_subscriptions.sql");
const MIGRATION_14: &str = include_str!("../migrations/0014_sync.sql");
pub const CURRENT_SCHEMA_VERSION: u32 = 14;
const MAX_TITLE_LENGTH: usize = 512;
const MAX_ORGANIZATION_NAME_LENGTH: usize = 200;
const MAX_TAG_NAME_LENGTH: usize = 100;
const MAX_DESCRIPTION_LENGTH: usize = 4096;
const MAX_PAGE_SIZE: u32 = 500;
pub(crate) const MAX_ATTACHMENT_BYTES: usize = 25 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Errore SQLite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Errore di serializzazione del documento: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Errore di accesso al file del database: {0}")]
    Io(#[from] std::io::Error),
    #[error("Il database è stato aggiornato da una versione più recente dell'app")]
    NewerDatabaseVersion,
    #[error(
        "Questo evento appartiene a un calendario esterno: va modificato tramite il suo account"
    )]
    ExternalEvent,
    #[error("Account o calendario esterno non trovato")]
    CalendarNotFound,
    #[error(
        "Il database ha lo schema {0}, diverso da quello di questa versione: apri prima Greatnote per aggiornarlo"
    )]
    IncompatibleSchema(u32),
    #[error("Titolo non valido: massimo {MAX_TITLE_LENGTH} caratteri")]
    InvalidTitle,
    #[error("Nome non valido: deve contenere da 1 a {MAX_ORGANIZATION_NAME_LENGTH} caratteri")]
    InvalidOrganizationName,
    #[error("Nome tag non valido: deve contenere da 1 a {MAX_TAG_NAME_LENGTH} caratteri")]
    InvalidTagName,
    #[error("Descrizione non valida: massimo {MAX_DESCRIPTION_LENGTH} caratteri")]
    InvalidDescription,
    #[error("Documento non valido: {0}")]
    InvalidDocument(&'static str),
    #[error("Ricerca non valida: {0}")]
    InvalidSearchQuery(String),
    #[error("Vista di ricerca non valida")]
    InvalidSearchScope,
    #[error("Intervallo di paginazione non valido")]
    InvalidPage,
    #[error("Nota non trovata")]
    NoteNotFound,
    #[error("Allegato non valido: {0}")]
    InvalidAttachment(&'static str),
    #[error("Il formato del file non è supportato")]
    UnsupportedAttachmentType,
    #[error("Allegato duplicato: questo file è già presente nella nota")]
    DuplicateAttachment,
    #[error("Allegato non trovato")]
    AttachmentNotFound,
    #[error("L'allegato è mancante o non supera la verifica di integrità")]
    AttachmentIntegrity,
    #[error("Collegamento tra note non valido")]
    InvalidNoteLink,
    #[error("Spazio non trovato")]
    SpaceNotFound,
    #[error("Taccuino non trovato")]
    NotebookNotFound,
    #[error("Tag non trovato")]
    TagNotFound,
    #[error("Blocco del database non disponibile")]
    DatabaseLock,
    #[error("Archivio ZIP non leggibile: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("Backup non valido: {0}")]
    InvalidBackup(String),
    #[error("Il backup è cifrato: inserisci la password per verificarlo")]
    BackupPassphraseRequired,
    #[error("Password errata")]
    WrongBackupPassphrase,
    #[error("La password deve contenere almeno {0} caratteri")]
    WeakBackupPassphrase(usize),
    #[error("Importazione non valida: {0}")]
    InvalidImport(String),
    #[error("Esportazione non riuscita: {0}")]
    Export(String),
    #[error("Impostazione non valida: {0}")]
    InvalidSetting(&'static str),
    #[error("Operazione disponibile solo per un database su file")]
    InMemoryDatabase,
    #[error("Data o orario non validi: {0}")]
    InvalidDateTime(String),
    #[error("Fuso orario non riconosciuto")]
    InvalidTimeZone,
    #[error("Attività non valida: {0}")]
    InvalidTask(&'static str),
    #[error("Attività non trovata")]
    TaskNotFound,
    #[error("Evento non valido: {0}")]
    InvalidEvent(&'static str),
    #[error("Evento non trovato")]
    EventNotFound,
    #[error("Promemoria non valido: {0}")]
    InvalidReminder(&'static str),
    #[error("Promemoria non trovato")]
    ReminderNotFound,
    #[error("Esiste già un tag con questo nome")]
    DuplicateTagName,
    #[error("Un tag con questo nome è nel cestino: ripristinalo oppure eliminalo definitivamente")]
    TagNameInTrash,
    #[error("Elemento non trovato nel cestino")]
    TrashItemNotFound,
    #[error("Revisione non trovata")]
    RevisionNotFound,
    #[error("Modello non trovato")]
    TemplateNotFound,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SearchResult {
    pub note: Note,
    pub highlighted_title: String,
    pub snippet: String,
}

pub struct Database {
    connection: Mutex<Connection>,
    path: Option<PathBuf>,
}

impl Database {
    /// Apre il database. Prima di aggiornare uno schema esistente salva un
    /// backup completo nella cartella `backups` accanto al file.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        let connection = open_connection(&path)?;
        let version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > 0 && version < CURRENT_SCHEMA_VERSION {
            let data_directory = path.parent().ok_or(StorageError::InMemoryDatabase)?;
            crate::backup::create_backup_with_connection(
                &connection,
                data_directory,
                crate::backup::BackupKind::PreMigration,
            )?;
        }
        migrate(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
            path: Some(path),
        })
    }

    pub fn in_memory() -> Result<Self, StorageError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(connection: Connection) -> Result<Self, StorageError> {
        configure_connection(&connection)?;
        migrate(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
            path: None,
        })
    }

    /// Connessione già aperta in sola lettura (server MCP), senza migrazioni.
    pub(crate) fn from_read_only(connection: Connection, path: PathBuf) -> Self {
        Self {
            connection: Mutex::new(connection),
            path: Some(path),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub(crate) fn connection(&self) -> Result<MutexGuard<'_, Connection>, StorageError> {
        self.connection
            .lock()
            .map_err(|_| StorageError::DatabaseLock)
    }

    pub fn create_note(&self, title: &str, content: &Value) -> Result<Note, StorageError> {
        self.create_note_in_notebook(title, content, None)
    }

    pub fn create_note_in_notebook(
        &self,
        title: &str,
        content: &Value,
        notebook_id: Option<&str>,
    ) -> Result<Note, StorageError> {
        let title = normalize_title(title)?;
        validate_document(content).map_err(StorageError::InvalidDocument)?;
        let id = Uuid::new_v4().to_string();
        let content_json = serde_json::to_string(content)?;

        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if let Some(notebook_id) = notebook_id {
            let notebook_exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM notebooks WHERE id = ?1 AND deleted_at IS NULL)",
                [notebook_id],
                |row| row.get(0),
            )?;
            if !notebook_exists {
                return Err(StorageError::NotebookNotFound);
            }
        }
        transaction.execute(
            "INSERT INTO notes (id, notebook_id, title, content_json)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, notebook_id, title, content_json],
        )?;
        sync_search_index(&transaction, &id, &title, content)?;
        transaction.commit()?;
        get_note(&connection, &id)?.ok_or(StorageError::NoteNotFound)
    }

    pub fn update_note(
        &self,
        id: &str,
        title: &str,
        content: &Value,
    ) -> Result<Note, StorageError> {
        self.update_note_from(id, title, content, None)
    }

    /// Come [`Database::update_note`], indicando la versione (`updated_at`)
    /// su cui l'editor stava lavorando. Se nel frattempo un altro
    /// dispositivo ha cambiato il testo, quella versione viene conservata
    /// nella cronologia e segnalata come conflitto prima di salvare.
    pub fn update_note_from(
        &self,
        id: &str,
        title: &str,
        content: &Value,
        base_updated_at: Option<&str>,
    ) -> Result<Note, StorageError> {
        let title = normalize_title(title)?;
        validate_document(content).map_err(StorageError::InvalidDocument)?;
        let content_json = serde_json::to_string(content)?;

        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        // Un salvataggio senza differenze non cambia la data di modifica:
        // altrimenti la sola apertura di una nota la sposterebbe in cima.
        let unchanged: Option<bool> = transaction
            .query_row(
                "SELECT title = ?1 AND content_json = ?2 FROM notes
                 WHERE id = ?3 AND deleted_at IS NULL",
                params![title, content_json, id],
                |row| row.get(0),
            )
            .optional()?;
        match unchanged {
            None => return Err(StorageError::NoteNotFound),
            Some(true) => {}
            Some(false) => {
                if let Some(base) = base_updated_at {
                    crate::sync::keep_overwritten_remote_edit(&transaction, id, base)?;
                }
                crate::library::record_revision_if_due(&transaction, id)?;
                transaction.execute(
                    "UPDATE notes
                     SET title = ?1, content_json = ?2,
                         updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                     WHERE id = ?3",
                    params![title, content_json, id],
                )?;
                sync_search_index(&transaction, id, &title, content)?;
            }
        }
        transaction.execute("DELETE FROM note_drafts WHERE note_id = ?1", [id])?;
        transaction.execute("DELETE FROM sync_remote_edits WHERE note_id = ?1", [id])?;
        transaction.commit()?;
        get_note(&connection, id)?.ok_or(StorageError::NoteNotFound)
    }

    pub fn save_note_draft(
        &self,
        note_id: &str,
        title: &str,
        content: &Value,
    ) -> Result<NoteDraft, StorageError> {
        let title = normalize_title(title)?;
        validate_document(content).map_err(StorageError::InvalidDocument)?;
        let content_json = serde_json::to_string(content)?;
        let connection = self.connection()?;
        let note_exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM notes WHERE id = ?1 AND deleted_at IS NULL)",
            [note_id],
            |row| row.get(0),
        )?;
        if !note_exists {
            return Err(StorageError::NoteNotFound);
        }
        connection.execute(
            "INSERT INTO note_drafts (note_id, title, content_json, saved_at)
             VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
             ON CONFLICT(note_id) DO UPDATE SET
                 title = excluded.title,
                 content_json = excluded.content_json,
                 saved_at = excluded.saved_at",
            params![note_id, title, content_json],
        )?;
        get_note_draft(&connection, note_id)?.ok_or(StorageError::NoteNotFound)
    }

    pub fn list_note_drafts(&self) -> Result<Vec<NoteDraft>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT note_drafts.note_id, note_drafts.title,
                    note_drafts.content_json, note_drafts.saved_at
             FROM note_drafts
             INNER JOIN notes ON notes.id = note_drafts.note_id
             WHERE notes.deleted_at IS NULL
             ORDER BY note_drafts.saved_at DESC, note_drafts.note_id",
        )?;
        let rows = statement.query_map([], note_draft_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn discard_note_draft(&self, note_id: &str) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute("DELETE FROM note_drafts WHERE note_id = ?1", [note_id])?;
        Ok(())
    }

    pub fn list_notes(&self, limit: u32, offset: u32) -> Result<Vec<Note>, StorageError> {
        let connection = self.connection()?;
        list_notes_with_query(
            &connection,
            "SELECT id, notebook_id, title, content_json, created_at, updated_at, is_pinned,
                    archived_at
             FROM notes
             WHERE deleted_at IS NULL AND archived_at IS NULL
             ORDER BY updated_at DESC, id
             LIMIT ?1 OFFSET ?2",
            limit,
            offset,
        )
    }

    pub fn search_notes(
        &self,
        input: &str,
        scope: &str,
        scope_id: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<SearchResult>, StorageError> {
        validate_page(limit)?;
        let query = parse_query(input).map_err(StorageError::InvalidSearchQuery)?;
        let mut conditions = Vec::new();
        let mut values = Vec::new();
        let mut order = "notes.updated_at DESC, notes.id";

        match scope {
            "all" => {
                conditions.push("notes.deleted_at IS NULL".to_owned());
                if query.is_archived {
                    conditions.push("notes.archived_at IS NOT NULL".to_owned());
                } else {
                    conditions.push("notes.archived_at IS NULL".to_owned());
                }
            }
            "pinned" => {
                conditions.push("notes.deleted_at IS NULL".to_owned());
                conditions.push("notes.archived_at IS NULL".to_owned());
                conditions.push("notes.is_pinned = 1".to_owned());
            }
            "archived" => {
                conditions.push("notes.deleted_at IS NULL".to_owned());
                conditions.push("notes.archived_at IS NOT NULL".to_owned());
            }
            "recent" => {
                conditions.push("notes.deleted_at IS NULL".to_owned());
                conditions.push("notes.archived_at IS NULL".to_owned());
                conditions.push(
                    "EXISTS (SELECT 1 FROM note_visits
                     WHERE note_visits.note_id = notes.id)"
                        .to_owned(),
                );
                order = "note_visits.last_opened_at DESC, notes.updated_at DESC, notes.id";
            }
            "trash" => conditions.push("notes.deleted_at IS NOT NULL".to_owned()),
            "space" => {
                let id = scope_id.ok_or(StorageError::InvalidSearchScope)?;
                conditions.extend([
                    "notes.deleted_at IS NULL".to_owned(),
                    "notes.archived_at IS NULL".to_owned(),
                    "EXISTS (SELECT 1 FROM notebooks
                     WHERE notebooks.id = notes.notebook_id AND notebooks.space_id = ?)"
                        .to_owned(),
                ]);
                values.push(SqlValue::Text(id.to_owned()));
            }
            "notebook" => {
                let id = scope_id.ok_or(StorageError::InvalidSearchScope)?;
                conditions.extend([
                    "notes.deleted_at IS NULL".to_owned(),
                    "notes.archived_at IS NULL".to_owned(),
                    "notes.notebook_id = ?".to_owned(),
                ]);
                values.push(SqlValue::Text(id.to_owned()));
            }
            "unfiled" => conditions.extend([
                "notes.deleted_at IS NULL".to_owned(),
                "notes.archived_at IS NULL".to_owned(),
                "notes.notebook_id IS NULL".to_owned(),
            ]),
            "tag" => {
                let id = scope_id.ok_or(StorageError::InvalidSearchScope)?;
                conditions.extend([
                    "notes.deleted_at IS NULL".to_owned(),
                    "notes.archived_at IS NULL".to_owned(),
                    "EXISTS (SELECT 1 FROM note_tags
                     WHERE note_tags.note_id = notes.id AND note_tags.tag_id = ?)"
                        .to_owned(),
                ]);
                values.push(SqlValue::Text(id.to_owned()));
            }
            _ => return Err(StorageError::InvalidSearchScope),
        }

        let match_expression = fts_expression(&query);
        if !match_expression.is_empty() {
            conditions.push("note_search MATCH ?".to_owned());
            values.push(SqlValue::Text(match_expression));
        }
        append_search_filters(&query, &mut conditions, &mut values);
        values.push(SqlValue::Integer(i64::from(limit)));
        values.push(SqlValue::Integer(i64::from(offset)));

        let search_from = match (query.terms.is_empty(), scope == "recent") {
            (true, true) => "notes INNER JOIN note_visits ON note_visits.note_id = notes.id",
            (false, true) => {
                "note_search INNER JOIN notes ON notes.id = note_search.note_id
                 INNER JOIN note_visits ON note_visits.note_id = notes.id"
            }
            (true, false) => "notes",
            (false, false) => "note_search INNER JOIN notes ON notes.id = note_search.note_id",
        };
        let snippet = if query.terms.is_empty() {
            "''"
        } else {
            // Colonna scelta da FTS5: testo della nota o allegati (anche il
            // testo letto dalle immagini), dove si trova la corrispondenza.
            "snippet(note_search, -1, char(57344), char(57345), '…', 12)"
        };
        let highlighted_title = if query.terms.is_empty() {
            "notes.title"
        } else {
            "highlight(note_search, 1, char(57344), char(57345))"
        };
        let order = if scope == "recent" || query.terms.is_empty() {
            order
        } else {
            "bm25(note_search), notes.updated_at DESC, notes.id"
        };
        let sql = format!(
            "SELECT notes.id, notes.notebook_id, notes.title, notes.content_json,
                    notes.created_at, notes.updated_at, notes.is_pinned, notes.archived_at,
                    {highlighted_title}, {snippet}
             FROM {search_from}
             WHERE {}
             ORDER BY {order}
             LIMIT ? OFFSET ?",
            conditions.join(" AND ")
        );

        let connection = self.connection()?;
        let mut statement = connection.prepare(&sql)?;
        let rows = statement.query_map(params_from_iter(values.iter()), |row| {
            Ok(SearchResult {
                note: note_from_row(row)?,
                highlighted_title: row.get(8)?,
                snippet: row.get(9)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn record_note_visit(&self, note_id: &str) -> Result<(), StorageError> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "INSERT INTO note_visits (note_id, last_opened_at)
             SELECT id, strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             FROM notes WHERE id = ?1 AND deleted_at IS NULL AND archived_at IS NULL
             ON CONFLICT(note_id) DO UPDATE SET
                 last_opened_at = excluded.last_opened_at",
            [note_id],
        )?;
        if changed == 0 {
            return Err(StorageError::NoteNotFound);
        }
        Ok(())
    }

    pub fn set_note_pinned(&self, note_id: &str, pinned: bool) -> Result<Note, StorageError> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE notes
             SET is_pinned = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?2 AND deleted_at IS NULL",
            params![pinned, note_id],
        )?;
        if changed == 0 {
            return Err(StorageError::NoteNotFound);
        }
        get_note(&connection, note_id)?.ok_or(StorageError::NoteNotFound)
    }

    pub fn set_note_archived(&self, note_id: &str, archived: bool) -> Result<Note, StorageError> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE notes
             SET archived_at = CASE
                    WHEN ?1 THEN strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                    ELSE NULL
                 END,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?2 AND deleted_at IS NULL",
            params![archived, note_id],
        )?;
        if changed == 0 {
            return Err(StorageError::NoteNotFound);
        }
        get_note(&connection, note_id)?.ok_or(StorageError::NoteNotFound)
    }

    pub fn get_note(&self, note_id: &str) -> Result<Note, StorageError> {
        let connection = self.connection()?;
        get_note(&connection, note_id)?.ok_or(StorageError::NoteNotFound)
    }

    pub fn add_attachment(
        &self,
        note_id: &str,
        original_filename: &str,
        bytes: &[u8],
        data_directory: &Path,
    ) -> Result<Attachment, StorageError> {
        let filename = normalize_filename(original_filename)?;
        let (mime_type, extension) = sniff_attachment_type(bytes)?;
        validate_attachment_extension(&filename, mime_type)?;
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        let search_text = attachment_search_text(&filename, mime_type, bytes);
        let attachment_id = Uuid::new_v4().to_string();
        let stored_filename = format!("{attachment_id}.{extension}");
        let path = write_attachment_file(data_directory, &stored_filename, bytes)?;

        let metadata_result = (|| {
            let mut connection = self.connection()?;
            let transaction = connection.transaction()?;
            let note_exists: bool = transaction.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM notes
                    WHERE id = ?1 AND deleted_at IS NULL
                )",
                [note_id],
                |row| row.get(0),
            )?;
            if !note_exists {
                return Err(StorageError::NoteNotFound);
            }
            let duplicate: bool = transaction.query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM attachments
                    WHERE note_id = ?1 AND sha256 = ?2 AND deleted_at IS NULL
                )",
                params![note_id, sha256],
                |row| row.get(0),
            )?;
            if duplicate {
                return Err(StorageError::DuplicateAttachment);
            }
            transaction.execute(
                "INSERT INTO attachments (
                    id, note_id, original_filename, stored_filename,
                    mime_type, size_bytes, sha256, search_text
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    attachment_id,
                    note_id,
                    filename,
                    stored_filename,
                    mime_type,
                    bytes.len() as i64,
                    sha256,
                    search_text
                ],
            )?;
            transaction.commit()?;
            get_attachment(&connection, &attachment_id)?.ok_or(StorageError::AttachmentNotFound)
        })();

        if metadata_result.is_err()
            && let Err(error) = fs::remove_file(&path)
        {
            return Err(StorageError::Io(error));
        }
        metadata_result
    }

    pub fn list_attachments(&self, note_id: &str) -> Result<Vec<Attachment>, StorageError> {
        let connection = self.connection()?;
        let note_exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM notes WHERE id = ?1 AND deleted_at IS NULL)",
            [note_id],
            |row| row.get(0),
        )?;
        if !note_exists {
            return Err(StorageError::NoteNotFound);
        }
        let mut statement = connection.prepare(
            "SELECT id, note_id, original_filename, mime_type, size_bytes, sha256, created_at,
                    ocr_status, ocr_text, ocr_engine, ocr_error
             FROM attachments
             WHERE note_id = ?1 AND deleted_at IS NULL
             ORDER BY created_at, id",
        )?;
        let rows = statement.query_map([note_id], attachment_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn verified_attachment_path(
        &self,
        note_id: &str,
        attachment_id: &str,
        data_directory: &Path,
    ) -> Result<std::path::PathBuf, StorageError> {
        let connection = self.connection()?;
        verified_attachment(&connection, note_id, attachment_id, data_directory)
            .map(|(path, _)| path)
    }

    pub fn delete_attachment(
        &self,
        note_id: &str,
        attachment_id: &str,
        data_directory: &Path,
    ) -> Result<(), StorageError> {
        let connection = self.connection()?;
        let stored_filename: String = connection
            .query_row(
                "SELECT stored_filename
                 FROM attachments
                 WHERE id = ?1 AND note_id = ?2 AND deleted_at IS NULL",
                params![attachment_id, note_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(StorageError::AttachmentNotFound)?;
        let path = attachment_path(data_directory, &stored_filename)?;
        let metadata =
            fs::symlink_metadata(&path).map_err(|_| StorageError::AttachmentIntegrity)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(StorageError::AttachmentIntegrity);
        }
        let staged_path = path.with_extension("deleting");
        fs::rename(&path, &staged_path)?;

        let update_result = connection.execute(
            "UPDATE attachments SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND note_id = ?2 AND deleted_at IS NULL",
            params![attachment_id, note_id],
        );
        if let Err(error) = update_result {
            if let Err(restore_error) = fs::rename(&staged_path, &path) {
                return Err(StorageError::Io(restore_error));
            }
            return Err(StorageError::Sqlite(error));
        }
        fs::remove_file(staged_path)?;
        Ok(())
    }

    pub fn create_note_link(
        &self,
        source_note_id: &str,
        target_note_id: &str,
        label: Option<&str>,
    ) -> Result<(), StorageError> {
        if source_note_id == target_note_id {
            return Err(StorageError::InvalidNoteLink);
        }
        let label = label.map(str::trim).filter(|value| !value.is_empty());
        if label.is_some_and(|value| value.chars().count() > 200) {
            return Err(StorageError::InvalidNoteLink);
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let notes_exist: bool = transaction.query_row(
            "SELECT
                EXISTS(SELECT 1 FROM notes
                       WHERE id = ?1 AND deleted_at IS NULL)
                AND EXISTS(SELECT 1 FROM notes
                           WHERE id = ?2 AND deleted_at IS NULL AND archived_at IS NULL)",
            params![source_note_id, target_note_id],
            |row| row.get(0),
        )?;
        if !notes_exist {
            return Err(StorageError::NoteNotFound);
        }
        transaction.execute(
            "INSERT INTO note_links (source_note_id, target_note_id, label)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(source_note_id, target_note_id) DO UPDATE SET label = excluded.label",
            params![source_note_id, target_note_id, label],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn list_note_links(&self, source_note_id: &str) -> Result<Vec<NoteLink>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT notes.id, notes.title, note_links.label
             FROM note_links
             INNER JOIN notes ON notes.id = note_links.target_note_id
             WHERE note_links.source_note_id = ?1
               AND notes.deleted_at IS NULL AND notes.archived_at IS NULL
             ORDER BY notes.title COLLATE NOCASE, notes.id",
        )?;
        let rows = statement.query_map([source_note_id], |row| {
            Ok(NoteLink {
                target_note_id: row.get(0)?,
                title: row.get(1)?,
                label: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn delete_note_link(
        &self,
        source_note_id: &str,
        target_note_id: &str,
    ) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute(
            "DELETE FROM note_links WHERE source_note_id = ?1 AND target_note_id = ?2",
            params![source_note_id, target_note_id],
        )?;
        Ok(())
    }

    pub fn list_deleted_notes(&self, limit: u32, offset: u32) -> Result<Vec<Note>, StorageError> {
        let connection = self.connection()?;
        list_notes_with_query(
            &connection,
            "SELECT id, notebook_id, title, content_json, created_at, updated_at, is_pinned,
                    archived_at
             FROM notes
             WHERE deleted_at IS NOT NULL
             ORDER BY deleted_at DESC, id
             LIMIT ?1 OFFSET ?2",
            limit,
            offset,
        )
    }

    pub fn list_notes_by_tag(
        &self,
        tag_id: &str,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Note>, StorageError> {
        validate_page(limit)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT notes.id, notes.notebook_id, notes.title, notes.content_json,
                    notes.created_at, notes.updated_at, notes.is_pinned, notes.archived_at
             FROM notes
             INNER JOIN note_tags ON note_tags.note_id = notes.id
             WHERE note_tags.tag_id = ?1
               AND notes.deleted_at IS NULL AND notes.archived_at IS NULL
             ORDER BY notes.updated_at DESC, notes.id
             LIMIT ?2 OFFSET ?3",
        )?;
        let rows = statement.query_map(params![tag_id, limit, offset], note_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn move_note(
        &self,
        note_id: &str,
        notebook_id: Option<&str>,
    ) -> Result<Note, StorageError> {
        let connection = self.connection()?;
        if let Some(notebook_id) = notebook_id {
            ensure_active(
                &connection,
                "notebooks",
                notebook_id,
                StorageError::NotebookNotFound,
            )?;
        }
        let changed = connection.execute(
            "UPDATE notes
             SET notebook_id = ?1, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?2 AND deleted_at IS NULL",
            params![notebook_id, note_id],
        )?;
        if changed == 0 {
            return Err(StorageError::NoteNotFound);
        }
        get_note(&connection, note_id)?.ok_or(StorageError::NoteNotFound)
    }

    pub fn trash_note(&self, note_id: &str) -> Result<(), StorageError> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE notes
             SET deleted_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND deleted_at IS NULL",
            [note_id],
        )?;
        if changed == 0 {
            return Err(StorageError::NoteNotFound);
        }
        Ok(())
    }

    pub fn restore_note(&self, note_id: &str) -> Result<Note, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE notes
             SET deleted_at = NULL, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND deleted_at IS NOT NULL",
            [note_id],
        )?;
        if changed == 0 {
            return Err(StorageError::NoteNotFound);
        }
        let note = get_note(&transaction, note_id)?.ok_or(StorageError::NoteNotFound)?;
        // Garantisce che la nota ripristinata sia di nuovo ricercabile.
        sync_search_index(&transaction, &note.id, &note.title, &note.content)?;
        transaction.commit()?;
        Ok(note)
    }

    /// Elimina definitivamente una nota che si trova nel cestino, con
    /// allegati, collegamenti, tag, bozze, promemoria e indice di ricerca.
    /// Attività ed eventi collegati restano, senza nota.
    pub fn delete_note_permanently(
        &self,
        note_id: &str,
        data_directory: &Path,
    ) -> Result<(), StorageError> {
        let deleted = self.purge_trashed_notes(Some(note_id), data_directory)?;
        if deleted == 0 {
            return Err(StorageError::NoteNotFound);
        }
        Ok(())
    }

    pub(crate) fn purge_trashed_notes(
        &self,
        note_id: Option<&str>,
        data_directory: &Path,
    ) -> Result<u64, StorageError> {
        // Solo note già nel cestino: l'eliminazione definitiva è sempre il
        // secondo passo, dopo la cancellazione logica.
        const TRASHED: &str = "SELECT id FROM notes
             WHERE deleted_at IS NOT NULL AND (?1 IS NULL OR id = ?1)";
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let files = {
            let mut statement = transaction.prepare(&format!(
                "SELECT stored_filename FROM attachments
                 WHERE note_id IN ({TRASHED}) AND deleted_at IS NULL"
            ))?;
            statement
                .query_map([note_id], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        transaction.execute(
            &format!(
                "DELETE FROM reminders WHERE entity_type = 'note' AND entity_id IN ({TRASHED})"
            ),
            [note_id],
        )?;
        transaction.execute(
            &format!("DELETE FROM note_search WHERE note_id IN ({TRASHED})"),
            [note_id],
        )?;
        // Le altre relazioni seguono i vincoli dello schema: CASCADE per tag,
        // collegamenti, allegati, bozze e visite; SET NULL per attività ed eventi.
        let deleted = transaction.execute(
            "DELETE FROM notes WHERE deleted_at IS NOT NULL AND (?1 IS NULL OR id = ?1)",
            [note_id],
        )?;
        // Un file resta se un'altra riga lo usa ancora (es. archivio reimportato).
        let mut orphaned = Vec::with_capacity(files.len());
        for file in files {
            let still_used: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM attachments WHERE stored_filename = ?1)",
                [&file],
                |row| row.get(0),
            )?;
            if !still_used {
                orphaned.push(file);
            }
        }
        transaction.commit()?;

        // I file si rimuovono dopo il commit: se il commit fallisce le note
        // restano integre. Un file non rimosso diventa solo un file orfano,
        // segnalato dal controllo d'integrità.
        for file in orphaned {
            let Ok(path) = attachment_path(data_directory, &file) else {
                continue;
            };
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.is_file() => {
                    if let Err(error) = fs::remove_file(&path) {
                        tracing::warn!("file allegato non rimosso: {error}");
                    }
                }
                _ => {}
            }
        }
        Ok(deleted as u64)
    }

    pub fn list_spaces(&self) -> Result<Vec<Space>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, description, sort_order, created_at, updated_at
             FROM spaces WHERE deleted_at IS NULL
             ORDER BY sort_order, name COLLATE NOCASE, id",
        )?;
        let rows = statement.query_map([], space_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn create_space(&self, name: &str, description: &str) -> Result<Space, StorageError> {
        let name = normalize_organization_name(name)?;
        let description = normalize_description(description)?;
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO spaces (id, name, description) VALUES (?1, ?2, ?3)",
            params![id, name, description],
        )?;
        get_space(&connection, &id)?.ok_or(StorageError::SpaceNotFound)
    }

    pub fn update_space(
        &self,
        id: &str,
        name: &str,
        description: &str,
    ) -> Result<Space, StorageError> {
        let name = normalize_organization_name(name)?;
        let description = normalize_description(description)?;
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE spaces
             SET name = ?1, description = ?2,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?3",
            params![name, description, id],
        )?;
        if changed == 0 {
            return Err(StorageError::SpaceNotFound);
        }
        get_space(&connection, id)?.ok_or(StorageError::SpaceNotFound)
    }

    pub fn list_notebooks(&self) -> Result<Vec<Notebook>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, space_id, name, description, sort_order, created_at, updated_at
             FROM notebooks WHERE deleted_at IS NULL
             ORDER BY sort_order, name COLLATE NOCASE, id",
        )?;
        let rows = statement.query_map([], notebook_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn create_notebook(
        &self,
        space_id: Option<&str>,
        name: &str,
        description: &str,
    ) -> Result<Notebook, StorageError> {
        let name = normalize_organization_name(name)?;
        let description = normalize_description(description)?;
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        if let Some(space_id) = space_id {
            ensure_active(&connection, "spaces", space_id, StorageError::SpaceNotFound)?;
        }
        connection.execute(
            "INSERT INTO notebooks (id, space_id, name, description)
             VALUES (?1, ?2, ?3, ?4)",
            params![id, space_id, name, description],
        )?;
        get_notebook(&connection, &id)?.ok_or(StorageError::NotebookNotFound)
    }

    pub fn update_notebook(
        &self,
        id: &str,
        space_id: Option<&str>,
        name: &str,
        description: &str,
    ) -> Result<Notebook, StorageError> {
        let name = normalize_organization_name(name)?;
        let description = normalize_description(description)?;
        let connection = self.connection()?;
        if let Some(space_id) = space_id {
            ensure_active(&connection, "spaces", space_id, StorageError::SpaceNotFound)?;
        }
        let changed = connection.execute(
            "UPDATE notebooks
             SET space_id = ?1, name = ?2, description = ?3,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?4",
            params![space_id, name, description, id],
        )?;
        if changed == 0 {
            return Err(StorageError::NotebookNotFound);
        }
        get_notebook(&connection, id)?.ok_or(StorageError::NotebookNotFound)
    }

    pub fn list_tags(&self) -> Result<Vec<Tag>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, color, created_at, updated_at
             FROM tags WHERE deleted_at IS NULL
             ORDER BY name COLLATE NOCASE, id",
        )?;
        let rows = statement.query_map([], tag_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn create_tag(&self, name: &str, color: Option<&str>) -> Result<Tag, StorageError> {
        let name = normalize_tag_name(name)?;
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        ensure_tag_name_available(&connection, &name, None)?;
        connection.execute(
            "INSERT INTO tags (id, name, color) VALUES (?1, ?2, ?3)",
            params![id, name, color],
        )?;
        get_tag(&connection, &id)?.ok_or(StorageError::TagNotFound)
    }

    pub fn update_tag(
        &self,
        id: &str,
        name: &str,
        color: Option<&str>,
    ) -> Result<Tag, StorageError> {
        let name = normalize_tag_name(name)?;
        let connection = self.connection()?;
        ensure_tag_name_available(&connection, &name, Some(id))?;
        let changed = connection.execute(
            "UPDATE tags
             SET name = ?1, color = ?2,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?3",
            params![name, color, id],
        )?;
        if changed == 0 {
            return Err(StorageError::TagNotFound);
        }
        get_tag(&connection, id)?.ok_or(StorageError::TagNotFound)
    }

    pub fn list_note_tags(&self, note_id: &str) -> Result<Vec<Tag>, StorageError> {
        let connection = self.connection()?;
        let note_exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM notes WHERE id = ?1 AND deleted_at IS NULL)",
            [note_id],
            |row| row.get(0),
        )?;
        if !note_exists {
            return Err(StorageError::NoteNotFound);
        }
        let mut statement = connection.prepare(
            "SELECT tags.id, tags.name, tags.color, tags.created_at, tags.updated_at
             FROM tags
             INNER JOIN note_tags ON note_tags.tag_id = tags.id
             WHERE note_tags.note_id = ?1
             ORDER BY tags.name COLLATE NOCASE, tags.id",
        )?;
        let rows = statement.query_map([note_id], tag_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn set_note_tags(&self, note_id: &str, tag_ids: &[String]) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let note_exists: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM notes WHERE id = ?1 AND deleted_at IS NULL)",
            [note_id],
            |row| row.get(0),
        )?;
        if !note_exists {
            return Err(StorageError::NoteNotFound);
        }
        for tag_id in tag_ids {
            let tag_exists: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM tags WHERE id = ?1 AND deleted_at IS NULL)",
                [tag_id],
                |row| row.get(0),
            )?;
            if !tag_exists {
                return Err(StorageError::TagNotFound);
            }
        }

        transaction.execute("DELETE FROM note_tags WHERE note_id = ?1", [note_id])?;
        for tag_id in tag_ids {
            transaction.execute(
                "INSERT INTO note_tags (note_id, tag_id) VALUES (?1, ?2)",
                params![note_id, tag_id],
            )?;
        }
        transaction.execute(
            "UPDATE notes SET updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1",
            [note_id],
        )?;
        transaction.commit()?;
        Ok(())
    }
}

pub(crate) fn attachment_search_text(filename: &str, mime_type: &str, bytes: &[u8]) -> String {
    let mut search_text = filename.to_owned();
    if mime_type == "text/plain"
        && let Ok(text) = std::str::from_utf8(bytes)
    {
        search_text.push(' ');
        search_text.extend(text.chars().take(100_000));
    }
    search_text
}

/// Percorso e contenuto di un allegato dopo la verifica di dimensione e
/// SHA-256; rifiuta collegamenti simbolici e file alterati.
pub(crate) fn verified_attachment(
    connection: &Connection,
    note_id: &str,
    attachment_id: &str,
    data_directory: &Path,
) -> Result<(PathBuf, Vec<u8>), StorageError> {
    let (stored_filename, expected_sha256, expected_size): (String, String, i64) = connection
        .query_row(
            "SELECT stored_filename, sha256, size_bytes
             FROM attachments
             WHERE id = ?1 AND note_id = ?2 AND deleted_at IS NULL",
            params![attachment_id, note_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(StorageError::AttachmentNotFound)?;
    let path = attachment_path(data_directory, &stored_filename)?;
    let metadata = fs::symlink_metadata(&path).map_err(|_| StorageError::AttachmentIntegrity)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_ATTACHMENT_BYTES as u64
        || metadata.len() != expected_size as u64
    {
        return Err(StorageError::AttachmentIntegrity);
    }
    let bytes = fs::read(&path)?;
    if format!("{:x}", Sha256::digest(&bytes)) != expected_sha256 {
        return Err(StorageError::AttachmentIntegrity);
    }
    Ok((path, bytes))
}

/// Scrive un nuovo file allegato senza sovrascrivere file esistenti e lo
/// sincronizza su disco; in caso di errore non lascia file parziali.
pub(crate) fn write_attachment_file(
    data_directory: &Path,
    stored_filename: &str,
    bytes: &[u8],
) -> Result<PathBuf, StorageError> {
    let directory = data_directory.join("attachments");
    fs::create_dir_all(&directory)?;
    ensure_attachment_directory(&directory)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
    }
    let path = attachment_path(data_directory, stored_filename)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)?;
    if let Err(error) = file.write_all(bytes).and_then(|()| file.sync_all()) {
        drop(file);
        if let Err(cleanup_error) = fs::remove_file(&path) {
            return Err(StorageError::Io(cleanup_error));
        }
        return Err(StorageError::Io(error));
    }
    Ok(path)
}

pub(crate) fn normalize_filename(filename: &str) -> Result<String, StorageError> {
    let basename = filename
        .rsplit(['/', '\\'])
        .next()
        .ok_or(StorageError::InvalidAttachment("nome file mancante"))?;
    let normalized = basename
        .chars()
        .filter(|character| !character.is_control())
        .collect::<String>()
        .trim()
        .to_owned();
    if normalized.is_empty()
        || normalized == "."
        || normalized == ".."
        || normalized.chars().count() > 255
    {
        return Err(StorageError::InvalidAttachment("nome file non valido"));
    }
    Ok(normalized)
}

pub(crate) fn sniff_attachment_type(
    bytes: &[u8],
) -> Result<(&'static str, &'static str), StorageError> {
    if bytes.is_empty() || bytes.len() > MAX_ATTACHMENT_BYTES {
        return Err(StorageError::InvalidAttachment(
            "il file deve avere dimensioni comprese tra 1 byte e 25 MiB",
        ));
    }
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(("image/png", "png"));
    }
    if bytes.starts_with(b"\xff\xd8\xff") {
        return Ok(("image/jpeg", "jpg"));
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Ok(("image/gif", "gif"));
    }
    if bytes.starts_with(b"%PDF-") {
        return Ok(("application/pdf", "pdf"));
    }
    if let Ok(text) = std::str::from_utf8(bytes)
        && !text
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
    {
        return Ok(("text/plain", "txt"));
    }
    Err(StorageError::UnsupportedAttachmentType)
}

fn validate_attachment_extension(filename: &str, mime_type: &str) -> Result<(), StorageError> {
    let extension = Path::new(filename)
        .extension()
        .and_then(|value| value.to_str())
        .map(str::to_ascii_lowercase);
    let matches_type = matches!(
        (extension.as_deref(), mime_type),
        (None, _)
            | (Some("txt" | "md" | "csv"), "text/plain")
            | (Some("png"), "image/png")
            | (Some("jpg" | "jpeg"), "image/jpeg")
            | (Some("gif"), "image/gif")
            | (Some("pdf"), "application/pdf")
    );
    if matches_type {
        Ok(())
    } else {
        Err(StorageError::UnsupportedAttachmentType)
    }
}

pub(crate) fn attachment_path(
    data_directory: &Path,
    stored_filename: &str,
) -> Result<std::path::PathBuf, StorageError> {
    let (id, extension) = stored_filename
        .split_once('.')
        .ok_or(StorageError::AttachmentIntegrity)?;
    if Uuid::parse_str(id).is_err() || !matches!(extension, "png" | "jpg" | "gif" | "pdf" | "txt") {
        return Err(StorageError::AttachmentIntegrity);
    }
    let directory = data_directory.join("attachments");
    ensure_attachment_directory(&directory)?;
    Ok(directory.join(stored_filename))
}

pub(crate) fn ensure_attachment_directory(directory: &Path) -> Result<(), StorageError> {
    let metadata =
        fs::symlink_metadata(directory).map_err(|_| StorageError::AttachmentIntegrity)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(StorageError::AttachmentIntegrity);
    }
    Ok(())
}

pub(crate) fn normalize_title(title: &str) -> Result<String, StorageError> {
    let trimmed = title.trim();
    if trimmed.chars().count() > MAX_TITLE_LENGTH {
        return Err(StorageError::InvalidTitle);
    }

    if trimmed.is_empty() {
        Ok("Nota senza titolo".to_owned())
    } else {
        Ok(trimmed.to_owned())
    }
}

pub(crate) fn normalize_organization_name(name: &str) -> Result<String, StorageError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_ORGANIZATION_NAME_LENGTH {
        return Err(StorageError::InvalidOrganizationName);
    }
    Ok(trimmed.to_owned())
}

pub(crate) fn normalize_tag_name(name: &str) -> Result<String, StorageError> {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_TAG_NAME_LENGTH {
        return Err(StorageError::InvalidTagName);
    }
    Ok(trimmed.to_owned())
}

fn normalize_description(description: &str) -> Result<String, StorageError> {
    let trimmed = description.trim();
    if trimmed.chars().count() > MAX_DESCRIPTION_LENGTH {
        return Err(StorageError::InvalidDescription);
    }
    Ok(trimmed.to_owned())
}

fn list_notes_with_query(
    connection: &Connection,
    query: &str,
    limit: u32,
    offset: u32,
) -> Result<Vec<Note>, StorageError> {
    validate_page(limit)?;
    let mut statement = connection.prepare(query)?;
    let rows = statement.query_map(params![limit, offset], note_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

pub(crate) fn validate_page(limit: u32) -> Result<(), StorageError> {
    if limit == 0 || limit > MAX_PAGE_SIZE {
        return Err(StorageError::InvalidPage);
    }
    Ok(())
}

/// Espressione FTS5 (passata come parametro): ogni termine tra virgolette;
/// le parole non scritte tra virgolette valgono anche come prefisso, così la
/// ricerca trova mentre si scrive e parole di cui si ricorda solo l'inizio.
fn fts_expression(query: &SearchQuery) -> String {
    query
        .terms
        .iter()
        .enumerate()
        .map(|(index, term)| {
            let exact = query.quoted.get(index).copied().unwrap_or(false);
            let quoted = format!("\"{}\"", term.replace('"', "\"\""));
            if exact { quoted } else { format!("{quoted}*") }
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

fn append_search_filters(
    query: &SearchQuery,
    conditions: &mut Vec<String>,
    values: &mut Vec<SqlValue>,
) {
    for tag in &query.tags {
        conditions.push(
            "EXISTS (SELECT 1 FROM note_tags
             INNER JOIN tags ON tags.id = note_tags.tag_id
             WHERE note_tags.note_id = notes.id AND tags.name = ? COLLATE NOCASE)"
                .to_owned(),
        );
        values.push(SqlValue::Text(tag.clone()));
    }
    for notebook in &query.notebooks {
        conditions.push(
            "EXISTS (SELECT 1 FROM notebooks
             WHERE notebooks.id = notes.notebook_id AND notebooks.name = ? COLLATE NOCASE)"
                .to_owned(),
        );
        values.push(SqlValue::Text(notebook.clone()));
    }
    for space in &query.spaces {
        conditions.push(
            "EXISTS (SELECT 1 FROM notebooks
             INNER JOIN spaces ON spaces.id = notebooks.space_id
             WHERE notebooks.id = notes.notebook_id AND spaces.name = ? COLLATE NOCASE)"
                .to_owned(),
        );
        values.push(SqlValue::Text(space.clone()));
    }
    if let Some((start, end)) = &query.created_range {
        conditions.push("date(notes.created_at) BETWEEN date(?) AND date(?)".to_owned());
        values.push(SqlValue::Text(start.clone()));
        values.push(SqlValue::Text(end.clone()));
    }
    if let Some(before) = &query.before {
        conditions.push("date(notes.created_at) < date(?)".to_owned());
        values.push(SqlValue::Text(before.clone()));
    }
    if let Some(after) = &query.after {
        conditions.push("date(notes.created_at) > date(?)".to_owned());
        values.push(SqlValue::Text(after.clone()));
    }
    if let Some(days) = query.updated_days {
        conditions.push("notes.updated_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?)".to_owned());
        values.push(SqlValue::Text(format!("-{days} days")));
    }
    if query.has_attachment {
        conditions.push(
            "EXISTS (SELECT 1 FROM attachments
             WHERE attachments.note_id = notes.id AND attachments.deleted_at IS NULL)"
                .to_owned(),
        );
    }
    if query.is_pinned {
        conditions.push("notes.is_pinned = 1".to_owned());
    }
    if query.is_archived {
        conditions.push("notes.archived_at IS NOT NULL".to_owned());
    }
    if query.has_open_task {
        conditions.push(
            "EXISTS (SELECT 1 FROM tasks
             WHERE tasks.note_id = notes.id AND tasks.status = 'open'
               AND tasks.deleted_at IS NULL)"
                .to_owned(),
        );
    }
}

pub(crate) fn sync_search_index(
    connection: &Connection,
    note_id: &str,
    title: &str,
    content: &Value,
) -> Result<(), StorageError> {
    let mut text = Vec::new();
    collect_text(content, &mut text);
    let attachment_text: String = connection.query_row(
        "SELECT COALESCE(group_concat(search_text, ' '), '')
         FROM attachments WHERE note_id = ?1 AND deleted_at IS NULL",
        [note_id],
        |row| row.get(0),
    )?;
    connection.execute("DELETE FROM note_search WHERE note_id = ?1", [note_id])?;
    connection.execute(
        "INSERT INTO note_search (note_id, title, content, attachments)
         VALUES (?1, ?2, ?3, ?4)",
        params![note_id, title, text.join(" "), attachment_text],
    )?;
    Ok(())
}

pub(crate) fn collect_text(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::Object(object) => {
            if let Some(text) = object.get("text").and_then(Value::as_str) {
                output.push(text.to_owned());
            }
            for (key, child) in object {
                if key != "text" {
                    collect_text(child, output);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_text(item, output);
            }
        }
        _ => {}
    }
}

fn migrate(connection: &Connection) -> Result<(), StorageError> {
    let mut version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version == 0 {
        connection.execute_batch(MIGRATION_1)?;
        version = 1;
    }
    if version == 1 {
        connection.execute_batch(MIGRATION_2)?;
        version = 2;
    }
    if version == 2 {
        connection.execute_batch(MIGRATION_3)?;
        version = 3;
    }
    if version == 3 {
        let transaction = connection.unchecked_transaction()?;
        let has_search_text: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM pragma_table_info('attachments')
                 WHERE name = 'search_text'
             )",
            [],
            |row| row.get(0),
        )?;
        if !has_search_text {
            transaction.execute_batch(
                "ALTER TABLE attachments ADD COLUMN search_text TEXT NOT NULL DEFAULT ''",
            )?;
        }
        let has_attachment_index: bool = transaction.query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM pragma_table_info('note_search')
                 WHERE name = 'attachments'
             )",
            [],
            |row| row.get(0),
        )?;
        if !has_attachment_index {
            transaction.execute_batch(
                "DROP TABLE note_search;
                 CREATE VIRTUAL TABLE note_search USING fts5(
                     note_id UNINDEXED,
                     title,
                     content,
                     attachments,
                     tokenize = 'unicode61 remove_diacritics 2'
                 );
                 INSERT INTO note_search (note_id, title, content, attachments)
                 SELECT notes.id, notes.title,
                        COALESCE((
                            SELECT group_concat(node.value, ' ')
                            FROM json_tree(notes.content_json) AS node
                            WHERE node.type = 'text'
                        ), ''),
                        COALESCE((
                            SELECT group_concat(attachments.search_text, ' ')
                            FROM attachments
                            WHERE attachments.note_id = notes.id
                              AND attachments.deleted_at IS NULL
                        ), '')
                 FROM notes;",
            )?;
        }
        transaction.execute_batch(MIGRATION_4)?;
        transaction.commit()?;
        version = 4;
    }
    if version == 4 {
        connection.execute_batch(MIGRATION_5)?;
        version = 5;
    }
    if version == 5 {
        connection.execute_batch(MIGRATION_6)?;
        version = 6;
    }
    if version == 6 {
        connection.execute_batch(MIGRATION_7)?;
        version = 7;
    }
    if version == 7 {
        connection.execute_batch(MIGRATION_8)?;
        version = 8;
    }
    if version == 8 {
        connection.execute_batch(MIGRATION_9)?;
        version = 9;
    }
    if version == 9 {
        connection.execute_batch(MIGRATION_10)?;
        version = 10;
    }
    if version == 10 {
        connection.execute_batch(MIGRATION_11)?;
        version = 11;
    }
    if version == 11 {
        connection.execute_batch(MIGRATION_12)?;
        version = 12;
    }
    if version == 12 {
        connection.execute_batch(MIGRATION_13)?;
        version = 13;
    }
    if version == 13 {
        connection.execute_batch(MIGRATION_14)?;
        version = 14;
    }
    if version == CURRENT_SCHEMA_VERSION {
        repair_ai_index_schema(connection)?;
    }
    if version != CURRENT_SCHEMA_VERSION {
        return Err(StorageError::NewerDatabaseVersion);
    }
    Ok(())
}

/// Alcuni database di sviluppo sono stati portati alla v9 con una bozza
/// dello schema dell'indice semantico (senza `note_updated_at`). L'indice
/// contiene solo dati ricalcolabili: se lo schema non coincide le due tabelle
/// vengono ricreate con la migrazione definitiva. Le note non sono toccate.
fn repair_ai_index_schema(connection: &Connection) -> Result<(), StorageError> {
    let current: bool = connection.query_row(
        "SELECT EXISTS(
             SELECT 1 FROM pragma_table_info('ai_index_notes') WHERE name = 'note_updated_at'
         ) AND EXISTS(
             SELECT 1 FROM pragma_table_info('ai_index_notes') WHERE name = 'source_hash'
         )",
        [],
        |row| row.get(0),
    )?;
    if current {
        return Ok(());
    }
    tracing::warn!("schema dell'indice semantico non aggiornato: l'indice viene ricreato");
    // Le definizioni sono quelle della migrazione 9, senza toccare la
    // versione dello schema.
    let tables: String = MIGRATION_9
        .lines()
        .filter(|line| {
            let line = line.trim();
            line != "BEGIN IMMEDIATE;"
                && line != "COMMIT;"
                && !line.starts_with("PRAGMA user_version")
        })
        .collect::<Vec<_>>()
        .join("\n");
    connection.execute_batch(&format!(
        "BEGIN IMMEDIATE;
         DROP TABLE IF EXISTS ai_chunks;
         DROP TABLE IF EXISTS ai_index_notes;
         {tables}
         COMMIT;"
    ))?;
    Ok(())
}

pub(crate) fn configure_connection(connection: &Connection) -> Result<(), StorageError> {
    connection.busy_timeout(Duration::from_secs(5))?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(())
}

pub(crate) fn open_connection(path: &Path) -> Result<Connection, StorageError> {
    let connection = Connection::open(path)?;
    configure_connection(&connection)?;
    Ok(connection)
}

/// Apre e aggiorna un database (usato anche su copie in ripristino).
pub(crate) fn open_and_migrate(path: &Path) -> Result<Connection, StorageError> {
    let connection = open_connection(path)?;
    migrate(&connection)?;
    Ok(connection)
}

pub(crate) fn get_note(connection: &Connection, id: &str) -> Result<Option<Note>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT id, notebook_id, title, content_json, created_at, updated_at, is_pinned,
                    archived_at
             FROM notes WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            note_from_row,
        )
        .optional()
}

/// Allegato attivo per id (con lo stato della lettura del testo).
pub(crate) fn get_attachment_row(
    connection: &Connection,
    id: &str,
) -> Result<Option<Attachment>, StorageError> {
    Ok(get_attachment(connection, id)?)
}

fn get_attachment(
    connection: &Connection,
    id: &str,
) -> Result<Option<Attachment>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT id, note_id, original_filename, mime_type, size_bytes, sha256, created_at,
                    ocr_status, ocr_text, ocr_engine, ocr_error
             FROM attachments WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            attachment_from_row,
        )
        .optional()
}

fn attachment_from_row(row: &Row<'_>) -> Result<Attachment, rusqlite::Error> {
    let size_bytes: i64 = row.get(4)?;
    Ok(Attachment {
        id: row.get(0)?,
        note_id: row.get(1)?,
        original_filename: row.get(2)?,
        mime_type: row.get(3)?,
        size_bytes: u64::try_from(size_bytes).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(4, Type::Integer, Box::new(error))
        })?,
        sha256: row.get(5)?,
        created_at: row.get(6)?,
        ocr_status: row.get(7)?,
        ocr_text: row.get(8)?,
        ocr_engine: row.get(9)?,
        ocr_error: row.get(10)?,
    })
}

pub(crate) fn note_from_row(row: &Row<'_>) -> Result<Note, rusqlite::Error> {
    let content_json: String = row.get(3)?;
    let content = serde_json::from_str(&content_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(3, Type::Text, Box::new(error))
    })?;

    Ok(Note {
        id: row.get(0)?,
        notebook_id: row.get(1)?,
        title: row.get(2)?,
        content,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        is_pinned: row.get(6)?,
        archived_at: row.get(7)?,
    })
}

fn space_from_row(row: &Row<'_>) -> Result<Space, rusqlite::Error> {
    Ok(Space {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        sort_order: row.get(3)?,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn notebook_from_row(row: &Row<'_>) -> Result<Notebook, rusqlite::Error> {
    Ok(Notebook {
        id: row.get(0)?,
        space_id: row.get(1)?,
        name: row.get(2)?,
        description: row.get(3)?,
        sort_order: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn tag_from_row(row: &Row<'_>) -> Result<Tag, rusqlite::Error> {
    Ok(Tag {
        id: row.get(0)?,
        name: row.get(1)?,
        color: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

/// Verifica che un contenitore esista e non sia nel cestino. `table` è
/// sempre un nome fisso scelto nel codice, mai input dell'utente.
pub(crate) fn ensure_active(
    connection: &Connection,
    table: &'static str,
    id: &str,
    error: StorageError,
) -> Result<(), StorageError> {
    debug_assert!(matches!(table, "spaces" | "notebooks" | "tags"));
    let active: bool = connection.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id = ?1 AND deleted_at IS NULL)"),
        [id],
        |row| row.get(0),
    )?;
    if active { Ok(()) } else { Err(error) }
}

/// I nomi dei tag sono unici anche rispetto ai tag nel cestino.
fn ensure_tag_name_available(
    connection: &Connection,
    name: &str,
    except_id: Option<&str>,
) -> Result<(), StorageError> {
    let existing: Option<bool> = connection
        .query_row(
            "SELECT deleted_at IS NOT NULL FROM tags
             WHERE name = ?1 COLLATE NOCASE AND (?2 IS NULL OR id != ?2)",
            params![name, except_id],
            |row| row.get(0),
        )
        .optional()?;
    match existing {
        None => Ok(()),
        Some(true) => Err(StorageError::TagNameInTrash),
        Some(false) => Err(StorageError::DuplicateTagName),
    }
}

fn get_space(connection: &Connection, id: &str) -> Result<Option<Space>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT id, name, description, sort_order, created_at, updated_at
             FROM spaces WHERE id = ?1",
            [id],
            space_from_row,
        )
        .optional()
}

fn get_notebook(connection: &Connection, id: &str) -> Result<Option<Notebook>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT id, space_id, name, description, sort_order, created_at, updated_at
             FROM notebooks WHERE id = ?1",
            [id],
            notebook_from_row,
        )
        .optional()
}

fn get_tag(connection: &Connection, id: &str) -> Result<Option<Tag>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT id, name, color, created_at, updated_at FROM tags WHERE id = ?1",
            [id],
            tag_from_row,
        )
        .optional()
}

fn get_note_draft(
    connection: &Connection,
    note_id: &str,
) -> Result<Option<NoteDraft>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT note_id, title, content_json, saved_at
             FROM note_drafts WHERE note_id = ?1",
            [note_id],
            note_draft_from_row,
        )
        .optional()
}

fn note_draft_from_row(row: &Row<'_>) -> Result<NoteDraft, rusqlite::Error> {
    let content_json: String = row.get(2)?;
    let content = serde_json::from_str(&content_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(2, Type::Text, Box::new(error))
    })?;
    Ok(NoteDraft {
        note_id: row.get(0)?,
        title: row.get(1)?,
        content,
        saved_at: row.get(3)?,
    })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use rusqlite::{Connection, params};
    use serde_json::json;

    use super::{Database, MIGRATION_1, MIGRATION_2, NoteLink, StorageError};
    use crate::domain::note::empty_document;

    #[test]
    fn creates_and_updates_notes_in_sqlite() {
        let database = Database::in_memory().expect("database initializes");
        let created = database
            .create_note("  Prima nota  ", &empty_document())
            .expect("note is created");
        assert_eq!(created.title, "Prima nota");
        assert_eq!(created.content, empty_document());

        let updated_content = json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [{"type": "text", "text": "Contenuto persistente"}]
            }]
        });
        let updated = database
            .update_note(&created.id, "Nota aggiornata", &updated_content)
            .expect("note is updated");
        assert_eq!(updated.title, "Nota aggiornata");
        assert_eq!(updated.content, updated_content);

        let listed = database.list_notes(10, 0).expect("notes can be listed");
        assert_eq!(listed, vec![updated]);
    }

    #[test]
    fn trashed_notes_can_be_deleted_permanently_and_trash_emptied() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let data = directory.path();
        let database = Database::open(data.join("gna.sqlite3")).expect("database");
        let kept = database
            .create_note("Resta", &empty_document())
            .expect("note");
        let doomed = database
            .create_note(
                "Da eliminare",
                &json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"parolaunica"}]}]}),
            )
            .expect("note");
        let attachment = database
            .add_attachment(&doomed.id, "a.txt", b"allegato da rimuovere", data)
            .expect("attachment");
        let tag = database.create_tag("tag", None).expect("tag");
        database
            .set_note_tags(&doomed.id, std::slice::from_ref(&tag.id))
            .expect("tags");
        database
            .create_note_link(&kept.id, &doomed.id, None)
            .expect("link");
        let task = database
            .create_task(&crate::domain::planner::TaskInput {
                title: "Collegata".to_owned(),
                description: String::new(),
                priority: 0,
                due_date: None,
                note_id: Some(doomed.id.clone()),
                flagged: false,
            })
            .expect("task");
        database
            .create_reminder(
                crate::domain::planner::ReminderEntity::Note,
                &doomed.id,
                "2099-01-01T09:00",
                Some("Europe/Rome"),
                crate::time::now(),
            )
            .expect("reminder");
        let file = data
            .join("attachments")
            .join(format!("{}.txt", attachment.id));
        assert!(file.exists());

        // Una nota non nel cestino non può essere eliminata definitivamente.
        assert!(matches!(
            database.delete_note_permanently(&doomed.id, data),
            Err(StorageError::NoteNotFound)
        ));
        database.trash_note(&doomed.id).expect("trash");
        database
            .delete_note_permanently(&doomed.id, data)
            .expect("permanent delete");

        assert!(!file.exists());
        assert!(
            database
                .list_deleted_notes(10, 0)
                .expect("trash")
                .is_empty()
        );
        assert!(matches!(
            database.restore_note(&doomed.id),
            Err(StorageError::NoteNotFound)
        ));
        assert!(
            database
                .list_note_links(&kept.id)
                .expect("links")
                .is_empty()
        );
        assert!(
            database
                .search_notes("parolaunica", "all", None, 10, 0)
                .expect("search")
                .is_empty()
        );
        assert!(database.list_reminders().expect("reminders").is_empty());
        assert!(
            database
                .get_task(&task.id)
                .expect("task survives")
                .note_id
                .is_none()
        );
        assert!(database.check_integrity().expect("integrity").ok);

        // Svuotare il cestino elimina tutte e sole le note cestinate.
        for title in ["Uno", "Due"] {
            let note = database
                .create_note(title, &empty_document())
                .expect("note");
            database.trash_note(&note.id).expect("trash");
        }
        assert_eq!(database.empty_trash(data).expect("empty").notes, 2);
        assert_eq!(database.empty_trash(data).expect("already empty").notes, 0);
        let remaining = database.list_notes(10, 0).expect("notes");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, kept.id);
    }

    #[test]
    fn saving_an_unchanged_note_keeps_its_position_in_the_list() {
        let database = Database::in_memory().expect("database initializes");
        let older = database
            .create_note("Più vecchia", &empty_document())
            .expect("note");
        std::thread::sleep(std::time::Duration::from_millis(5));
        let newer = database
            .create_note("Più recente", &empty_document())
            .expect("note");
        std::thread::sleep(std::time::Duration::from_millis(5));
        database
            .save_note_draft(&older.id, "Più vecchia", &empty_document())
            .expect("draft");

        // Aprire e lasciare una nota senza modificarla non la sposta.
        let same = database
            .update_note(&older.id, "Più vecchia", &empty_document())
            .expect("no-op save");
        assert_eq!(same.updated_at, older.updated_at);
        assert!(database.list_note_drafts().expect("drafts").is_empty());
        let order: Vec<_> = database
            .list_notes(10, 0)
            .expect("notes")
            .into_iter()
            .map(|note| note.id)
            .collect();
        assert_eq!(order, [newer.id.clone(), older.id.clone()]);

        let changed = database
            .update_note(&older.id, "Modificata", &empty_document())
            .expect("real save");
        assert!(changed.updated_at > older.updated_at);
        assert_eq!(database.list_notes(10, 0).expect("notes")[0].id, older.id);
        assert!(matches!(
            database.update_note("missing", "x", &empty_document()),
            Err(StorageError::NoteNotFound)
        ));
    }

    #[test]
    fn notes_survive_reopening_the_database() {
        let directory = tempfile::tempdir().expect("temporary directory is created");
        let path = directory.path().join("gna.sqlite3");
        let created = {
            let database = Database::open(&path).expect("database opens");
            database
                .create_note("Persistente", &empty_document())
                .expect("note is created")
        };

        let reopened = Database::open(path).expect("database reopens");
        let listed = reopened.list_notes(10, 0).expect("notes can be listed");
        assert_eq!(listed, vec![created]);
    }

    #[test]
    fn rejects_invalid_documents_and_unbounded_pages() {
        let database = Database::in_memory().expect("database initializes");

        assert!(matches!(
            database.create_note("Nota", &json!({"type": "html"})),
            Err(StorageError::InvalidDocument(_))
        ));
        assert!(matches!(
            database.list_notes(501, 0),
            Err(StorageError::InvalidPage)
        ));
        assert!(matches!(
            database.create_space("Spazio", &"x".repeat(4097)),
            Err(StorageError::InvalidDescription)
        ));
    }

    #[test]
    fn bundled_sqlite_supports_fts5() {
        let database = Database::in_memory().expect("database initializes");
        let connection = database.connection().expect("database lock is available");

        let fts5_table_exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'note_search')",
                [],
                |row| row.get(0),
            )
            .expect("FTS5 table is available");
        assert!(fts5_table_exists);
    }

    #[test]
    fn full_text_search_indexes_and_updates_titles_and_rich_content() {
        let database = Database::in_memory().expect("database initializes");
        let document = json!({
            "type": "doc",
            "content": [{
                "type": "paragraph",
                "content": [{"type": "text", "text": "contenuto indicizzabile"}]
            }]
        });
        let note = database
            .create_note("Progetto quercia", &document)
            .expect("note is created");

        let title_results = database
            .search_notes("quercia", "all", None, 10, 0)
            .expect("title is searchable");
        assert_eq!(title_results[0].note.id, note.id);
        assert!(
            title_results[0]
                .highlighted_title
                .contains("\u{e000}quercia\u{e001}")
        );
        assert_eq!(title_results[0].note.id, note.id);

        let content_results = database
            .search_notes("indicizzabile", "all", None, 10, 0)
            .expect("content is searchable");
        assert_eq!(content_results[0].note.id, note.id);
        assert!(
            content_results[0]
                .snippet
                .contains("\u{e000}indicizzabile\u{e001}"),
            "unexpected highlighted snippet: {:?}",
            content_results[0].snippet
        );

        database
            .update_note(&note.id, "Piano aggiornato", &empty_document())
            .expect("note is updated");
        assert!(
            database
                .search_notes("quercia", "all", None, 10, 0)
                .expect("old title search succeeds")
                .is_empty()
        );
        assert!(
            database
                .search_notes("indicizzabile", "all", None, 10, 0)
                .expect("old content search succeeds")
                .is_empty()
        );
        assert_eq!(
            database
                .search_notes("aggiornato", "all", None, 10, 0)
                .expect("new title search succeeds")[0]
                .note
                .id,
            note.id
        );
    }

    #[test]
    fn query_filters_are_bound_and_scoped_to_views() {
        let database = Database::in_memory().expect("database initializes");
        let notebook = database
            .create_notebook(None, "Personale", "")
            .expect("notebook is created");
        let tag = database
            .create_tag("Importante", None)
            .expect("tag is created");
        let note = database
            .create_note_in_notebook("Piano protetto", &empty_document(), Some(&notebook.id))
            .expect("note is created");
        database
            .set_note_tags(&note.id, std::slice::from_ref(&tag.id))
            .expect("tag is set");
        database
            .set_note_pinned(&note.id, true)
            .expect("note is pinned");

        assert_eq!(
            database
                .search_notes(
                    "tag:importante notebook:personale is:pinned \"Piano protetto\"",
                    "all",
                    None,
                    10,
                    0,
                )
                .expect("field and full-text filters work")[0]
                .note
                .id,
            note.id
        );
        assert!(
            database
                .search_notes(r#"" OR * ""#, "all", None, 10, 0)
                .expect("special characters are treated as text")
                .is_empty()
        );
        assert!(
            database
                .search_notes("tag:Importante", "tag", Some("wrong-id"), 10, 0)
                .expect("scope mismatch returns no notes")
                .is_empty()
        );
        assert!(matches!(
            database.search_notes("created:bad..range", "all", None, 10, 0),
            Err(StorageError::InvalidSearchQuery(_))
        ));
        assert!(matches!(
            database.search_notes("", "unknown", None, 10, 0),
            Err(StorageError::InvalidSearchScope)
        ));
    }

    #[test]
    fn pinned_archived_and_recent_views_are_persisted() {
        let database = Database::in_memory().expect("database initializes");
        let note = database
            .create_note("Nota da ritrovare", &empty_document())
            .expect("note is created");
        database
            .record_note_visit(&note.id)
            .expect("visit is recorded");
        assert_eq!(
            database
                .search_notes("", "recent", None, 10, 0)
                .expect("recent notes load")[0]
                .note
                .id,
            note.id
        );
        database
            .set_note_pinned(&note.id, true)
            .expect("note can be pinned");
        assert_eq!(
            database
                .search_notes("", "pinned", None, 10, 0)
                .expect("pinned notes load")[0]
                .note
                .id,
            note.id
        );
        database
            .set_note_archived(&note.id, true)
            .expect("note can be archived");
        assert!(
            database
                .search_notes("", "all", None, 10, 0)
                .expect("active notes load")
                .is_empty()
        );
        assert_eq!(
            database
                .search_notes("", "archived", None, 10, 0)
                .expect("archive loads")[0]
                .note
                .id,
            note.id
        );
        assert_eq!(
            database
                .search_notes("is:archived", "all", None, 10, 0)
                .expect("archived query filter works")[0]
                .note
                .id,
            note.id
        );
    }

    #[test]
    fn attachments_are_safely_stored_verified_indexed_and_removed() {
        let database = Database::in_memory().expect("database initializes");
        let directory = tempfile::tempdir().expect("attachment directory exists");
        let note = database
            .create_note("Nota con allegato", &empty_document())
            .expect("note is created");
        let attachment = database
            .add_attachment(
                &note.id,
                "../../appunti riservati.txt",
                b"testo allegato ricercabile",
                directory.path(),
            )
            .expect("text attachment is stored");
        assert_eq!(attachment.original_filename, "appunti riservati.txt");
        assert_eq!(attachment.mime_type, "text/plain");
        assert_eq!(attachment.size_bytes, 26);
        assert_eq!(
            database
                .search_notes("ricercabile", "all", None, 10, 0)
                .expect("plain-text attachment contents are indexed")[0]
                .note
                .id,
            note.id
        );
        let long_text = format!(
            "inizioindicizzato {} esclusofuorilimite",
            "token ".repeat(20_000)
        );
        let long_attachment = database
            .add_attachment(
                &note.id,
                "testo-lungo.txt",
                long_text.as_bytes(),
                directory.path(),
            )
            .expect("long text is attached");
        assert_eq!(
            database
                .search_notes("inizioindicizzato", "all", None, 10, 0)
                .expect("text attachment prefix is indexed")[0]
                .note
                .id,
            note.id
        );
        assert!(
            database
                .search_notes("esclusofuorilimite", "all", None, 10, 0)
                .expect("text after the indexing cap is omitted")
                .is_empty()
        );

        let path = database
            .verified_attachment_path(&note.id, &attachment.id, directory.path())
            .expect("attachment hash and location are valid");
        assert!(path.starts_with(directory.path().join("attachments")));
        assert_eq!(
            fs::read(&path).expect("stored file is readable"),
            b"testo allegato ricercabile"
        );
        assert!(matches!(
            database.add_attachment(
                &note.id,
                "duplicato.txt",
                b"testo allegato ricercabile",
                directory.path()
            ),
            Err(StorageError::DuplicateAttachment)
        ));
        assert!(matches!(
            database.add_attachment(
                &note.id,
                "falso.pdf",
                b"contenuto non PDF",
                directory.path()
            ),
            Err(StorageError::UnsupportedAttachmentType)
        ));

        fs::write(&path, b"contenuto alterato").expect("test can corrupt attachment");
        assert!(matches!(
            database.verified_attachment_path(&note.id, &attachment.id, directory.path()),
            Err(StorageError::AttachmentIntegrity)
        ));
        database
            .delete_attachment(&note.id, &attachment.id, directory.path())
            .expect("attachment is removed");
        database
            .delete_attachment(&note.id, &long_attachment.id, directory.path())
            .expect("long attachment is removed");
        assert!(
            database
                .list_attachments(&note.id)
                .expect("attachment metadata loads")
                .is_empty()
        );
        assert!(
            database
                .search_notes("ricercabile", "all", None, 10, 0)
                .expect("deleted attachment content is removed from the index")
                .is_empty()
        );
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn attachments_reject_a_symlinked_storage_directory() {
        use std::os::unix::fs::symlink;

        let database = Database::in_memory().expect("database initializes");
        let directory = tempfile::tempdir().expect("data directory exists");
        let outside = tempfile::tempdir().expect("external directory exists");
        let note = database
            .create_note("Nota", &empty_document())
            .expect("note is created");
        symlink(outside.path(), directory.path().join("attachments"))
            .expect("test symlink is created");

        assert!(matches!(
            database.add_attachment(&note.id, "segreto.txt", b"contenuto", directory.path()),
            Err(StorageError::AttachmentIntegrity)
        ));
        assert!(
            fs::read_dir(outside.path())
                .expect("external directory can be read")
                .next()
                .is_none()
        );
    }

    #[test]
    fn internal_note_links_are_validated_and_persisted() {
        let database = Database::in_memory().expect("database initializes");
        let source = database
            .create_note("Origine", &empty_document())
            .expect("source note is created");
        let target = database
            .create_note("Destinazione", &empty_document())
            .expect("target note is created");
        database
            .create_note_link(&source.id, &target.id, Some("Approfondisci"))
            .expect("note can be linked");
        database
            .create_note_link(&source.id, &target.id, Some("Vai alla nota"))
            .expect("repeated link creation updates its label");
        assert_eq!(
            database.list_note_links(&source.id).expect("links load"),
            vec![NoteLink {
                target_note_id: target.id.clone(),
                title: target.title.clone(),
                label: Some("Vai alla nota".to_owned()),
            }]
        );
        assert!(matches!(
            database.create_note_link(&source.id, &source.id, None),
            Err(StorageError::InvalidNoteLink)
        ));
        assert!(matches!(
            database.create_note_link(&source.id, "missing-note", None),
            Err(StorageError::NoteNotFound)
        ));
        database
            .delete_note_link(&source.id, &target.id)
            .expect("link can be removed");
        assert!(
            database
                .list_note_links(&source.id)
                .expect("link is removed")
                .is_empty()
        );
    }

    #[test]
    fn normalizes_an_empty_title() {
        let database = Database::in_memory().expect("database initializes");
        let note = database
            .create_note("   ", &empty_document())
            .expect("empty title gets a safe default");

        assert_eq!(note.title, "Nota senza titolo");
    }

    #[test]
    fn organization_crud_preserves_notes_when_containers_are_deleted() {
        let database = Database::in_memory().expect("database initializes");
        let space = database
            .create_space(" Lavoro ", "Progetti")
            .expect("space is created");
        let notebook = database
            .create_notebook(Some(&space.id), "Progetto", "")
            .expect("notebook is created");
        let note = database
            .create_note("Piano", &empty_document())
            .expect("note is created");

        let moved = database
            .move_note(&note.id, Some(&notebook.id))
            .expect("note can be assigned to a notebook");
        assert_eq!(moved.notebook_id.as_deref(), Some(notebook.id.as_str()));

        let renamed_space = database
            .update_space(&space.id, "Clienti", "Account")
            .expect("space can be renamed");
        assert_eq!(renamed_space.name, "Clienti");
        database
            .move_item_to_trash(crate::trash::TrashKind::Space, &space.id)
            .expect("space can be trashed");
        assert_eq!(
            database.list_notebooks().expect("notebooks load")[0].space_id,
            None
        );
        database
            .update_notebook(&notebook.id, None, "Piano", "Descrizione")
            .expect("notebook can be edited");

        database
            .move_item_to_trash(crate::trash::TrashKind::Notebook, &notebook.id)
            .expect("notebook can be trashed");
        let remaining = database.list_notes(10, 0).expect("notes remain available");
        assert_eq!(remaining[0].id, note.id);
        assert_eq!(remaining[0].notebook_id, None);
    }

    #[test]
    fn creates_notes_in_the_selected_notebook() {
        let database = Database::in_memory().expect("database initializes");
        let notebook = database
            .create_notebook(None, "Personale", "")
            .expect("notebook is created");

        let note = database
            .create_note_in_notebook("Appunti", &empty_document(), Some(&notebook.id))
            .expect("note is created in the notebook");
        assert_eq!(note.notebook_id.as_deref(), Some(notebook.id.as_str()));

        let listed = database.list_notes(10, 0).expect("notes load");
        assert_eq!(listed[0].notebook_id.as_deref(), Some(notebook.id.as_str()));
        assert!(matches!(
            database.create_note_in_notebook(
                "Senza taccuino valido",
                &empty_document(),
                Some("missing-notebook")
            ),
            Err(StorageError::NotebookNotFound)
        ));
    }

    #[test]
    fn note_tags_are_transactional_and_cascade_when_tag_is_deleted() {
        let database = Database::in_memory().expect("database initializes");
        let note = database
            .create_note("Nota", &empty_document())
            .expect("note is created");
        let tag = database
            .create_tag("Ricerca", None)
            .expect("tag is created");
        database
            .set_note_tags(&note.id, std::slice::from_ref(&tag.id))
            .expect("tag is assigned");
        assert_eq!(
            database.list_note_tags(&note.id).expect("note tags load"),
            vec![tag.clone()]
        );
        assert_eq!(
            database
                .list_notes_by_tag(&tag.id, 10, 0)
                .expect("tag can filter notes")[0]
                .id,
            note.id
        );

        assert!(matches!(
            database.set_note_tags(&note.id, &["missing-tag".to_owned()]),
            Err(StorageError::TagNotFound)
        ));
        assert_eq!(
            database
                .list_note_tags(&note.id)
                .expect("failed updates preserve existing tags"),
            vec![tag.clone()]
        );

        database
            .move_item_to_trash(crate::trash::TrashKind::Tag, &tag.id)
            .expect("tag is trashed");
        assert!(matches!(
            database.set_note_tags(&note.id, std::slice::from_ref(&tag.id)),
            Err(StorageError::TagNotFound)
        ));
        assert!(
            database
                .list_note_tags(&note.id)
                .expect("tag relationship is removed")
                .is_empty()
        );
        assert!(
            database
                .list_notes_by_tag(&tag.id, 10, 0)
                .expect("deleted tag has no matching notes")
                .is_empty()
        );
    }

    #[test]
    fn notes_can_be_trashed_and_restored_after_reopening() {
        let directory = tempfile::tempdir().expect("temporary directory is created");
        let path = directory.path().join("gna.sqlite3");
        let note_id = {
            let database = Database::open(&path).expect("database opens");
            let note = database
                .create_note("Recuperabile", &empty_document())
                .expect("note is created");
            database
                .trash_note(&note.id)
                .expect("note can be moved to trash");
            assert!(
                database
                    .list_notes(10, 0)
                    .expect("active notes load")
                    .is_empty()
            );
            assert_eq!(
                database.list_deleted_notes(10, 0).expect("trash loads")[0].id,
                note.id
            );
            note.id
        };

        let reopened = Database::open(&path).expect("database reopens");
        let restored = reopened
            .restore_note(&note_id)
            .expect("note can be restored after reopening");
        assert_eq!(restored.title, "Recuperabile");
        assert_eq!(
            reopened
                .list_deleted_notes(10, 0)
                .expect("trash loads")
                .len(),
            0
        );
    }

    #[test]
    fn drafts_survive_reopening_and_are_cleared_when_applied() {
        let directory = tempfile::tempdir().expect("temporary directory is created");
        let path = directory.path().join("gna.sqlite3");
        let note = {
            let database = Database::open(&path).expect("database opens");
            let note = database
                .create_note("Nota salvata", &empty_document())
                .expect("note is created");
            let draft_content = json!({
                "type": "doc",
                "content": [{
                    "type": "paragraph",
                    "content": [{"type": "text", "text": "Bozza recuperata"}]
                }]
            });
            database
                .save_note_draft(&note.id, "Titolo bozza", &draft_content)
                .expect("draft is stored");
            note
        };

        let reopened = Database::open(&path).expect("database reopens");
        let drafts = reopened.list_note_drafts().expect("drafts are recovered");
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].note_id, note.id);
        assert_eq!(drafts[0].title, "Titolo bozza");
        assert_eq!(
            reopened.list_notes(10, 0).expect("saved notes load")[0].title,
            "Nota salvata"
        );

        reopened
            .update_note(&note.id, &drafts[0].title, &drafts[0].content)
            .expect("draft can be applied");
        assert!(
            reopened
                .list_note_drafts()
                .expect("applied drafts are cleared")
                .is_empty()
        );

        let second_note = reopened
            .create_note("Da scartare", &empty_document())
            .expect("another note is created");
        reopened
            .save_note_draft(&second_note.id, "Bozza", &empty_document())
            .expect("another draft is stored");
        reopened
            .discard_note_draft(&second_note.id)
            .expect("draft can be discarded");
        assert!(
            reopened
                .list_note_drafts()
                .expect("discarded drafts are removed")
                .is_empty()
        );
    }

    #[test]
    fn draft_ai_index_schema_is_rebuilt_on_open() {
        let directory = tempfile::tempdir().expect("temporary directory is created");
        let path = directory.path().join("gna.sqlite3");
        let note = {
            let database = Database::open(&path).expect("database opens");
            database
                .create_note("Resta", &empty_document())
                .expect("note is created")
        };
        {
            // Schema della bozza: senza `note_updated_at`.
            let connection = Connection::open(&path).expect("raw connection");
            connection
                .execute_batch(
                    "DROP TABLE ai_chunks;
                     DROP TABLE ai_index_notes;
                     CREATE TABLE ai_index_notes (
                         note_id TEXT PRIMARY KEY NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
                         model TEXT NOT NULL,
                         source_hash TEXT NOT NULL,
                         indexed_at TEXT NOT NULL DEFAULT 'x'
                     );
                     CREATE TABLE ai_chunks (note_id TEXT NOT NULL, chunk_index INTEGER NOT NULL);",
                )
                .expect("draft schema");
        }
        let database = Database::open(&path).expect("database opens and repairs");
        let settings = crate::ai::settings::AiSettings {
            embedding_model: "e".to_owned(),
            ..Default::default()
        };
        let status = database.ai_index_status(&settings).expect("status works");
        assert_eq!(status.pending_notes, 1);
        assert_eq!(
            database.get_note(&note.id).expect("note kept").title,
            "Resta"
        );
    }

    #[test]
    fn version_four_databases_reindex_trashed_notes_missing_from_search() {
        let directory = tempfile::tempdir().expect("temporary directory is created");
        let path = directory.path().join("gna.sqlite3");
        let note_id = {
            let database = Database::open(&path).expect("database opens");
            let note = database
                .create_note(
                    "Nota nel cestino",
                    &json!({"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"parola recuperabile"}]}]}),
                )
                .expect("note is created");
            database.trash_note(&note.id).expect("note is trashed");
            note.id
        };
        {
            // Riproduce lo stato lasciato dalla vecchia ricostruzione v3→v4.
            let connection = Connection::open(&path).expect("raw connection");
            connection
                .execute_batch(
                    "DELETE FROM note_search;
                     ALTER TABLE attachments DROP COLUMN ocr_text;
                     ALTER TABLE attachments DROP COLUMN ocr_status;
                     ALTER TABLE attachments DROP COLUMN ocr_engine;
                     ALTER TABLE attachments DROP COLUMN ocr_error;
                     ALTER TABLE attachments DROP COLUMN ocr_at;
                     DROP TABLE ai_history;
                     DROP TABLE ai_chunks;
                     DROP TABLE ai_index_notes;
                     DROP TABLE note_revisions;
                     DROP TABLE templates;
                     DROP TABLE trash_relations;
                     DROP INDEX spaces_deleted_at;
                     DROP INDEX notebooks_deleted_at;
                     DROP INDEX tags_deleted_at;
                     ALTER TABLE tags DROP COLUMN deleted_at;
                     DROP TABLE reminders;
                     DROP TABLE events;
                     DROP TRIGGER IF EXISTS sync_spaces_insert;
                     DROP TRIGGER IF EXISTS sync_spaces_update;
                     DROP TRIGGER IF EXISTS sync_spaces_delete;
                     DROP TRIGGER IF EXISTS sync_notebooks_insert;
                     DROP TRIGGER IF EXISTS sync_notebooks_update;
                     DROP TRIGGER IF EXISTS sync_notebooks_delete;
                     DROP TRIGGER IF EXISTS sync_tags_insert;
                     DROP TRIGGER IF EXISTS sync_tags_update;
                     DROP TRIGGER IF EXISTS sync_tags_delete;
                     DROP TRIGGER IF EXISTS sync_notes_insert;
                     DROP TRIGGER IF EXISTS sync_notes_update;
                     DROP TRIGGER IF EXISTS sync_notes_delete;
                     DROP TRIGGER IF EXISTS sync_note_tags_insert;
                     DROP TRIGGER IF EXISTS sync_note_tags_update;
                     DROP TRIGGER IF EXISTS sync_note_tags_delete;
                     DROP TRIGGER IF EXISTS sync_note_links_insert;
                     DROP TRIGGER IF EXISTS sync_note_links_update;
                     DROP TRIGGER IF EXISTS sync_note_links_delete;
                     DROP TRIGGER IF EXISTS sync_attachments_insert;
                     DROP TRIGGER IF EXISTS sync_attachments_update;
                     DROP TRIGGER IF EXISTS sync_attachments_delete;
                     DROP TRIGGER IF EXISTS sync_tasks_insert;
                     DROP TRIGGER IF EXISTS sync_tasks_update;
                     DROP TRIGGER IF EXISTS sync_tasks_delete;
                     DROP TRIGGER IF EXISTS sync_events_insert;
                     DROP TRIGGER IF EXISTS sync_events_update;
                     DROP TRIGGER IF EXISTS sync_events_delete;
                     DROP TRIGGER IF EXISTS sync_reminders_insert;
                     DROP TRIGGER IF EXISTS sync_reminders_update;
                     DROP TRIGGER IF EXISTS sync_reminders_delete;
                     DROP TRIGGER IF EXISTS sync_templates_insert;
                     DROP TRIGGER IF EXISTS sync_templates_update;
                     DROP TRIGGER IF EXISTS sync_templates_delete;
                     DROP TRIGGER IF EXISTS sync_trash_relations_insert;
                     DROP TRIGGER IF EXISTS sync_trash_relations_update;
                     DROP TRIGGER IF EXISTS sync_trash_relations_delete;
                     DROP TABLE sync_outbox;
                     DROP TABLE sync_remote_edits;
                     DROP TABLE sync_aliases;
                     DROP TABLE sync_missing_blobs;
                     DROP TABLE sync_conflicts;
                     DROP TABLE sync_state;
                     DROP TABLE sync_devices;
                     DROP TABLE sync_versions;
                     DROP TABLE sync_control;
                     DROP TABLE sync_dirty;
                     DROP TABLE calendar_subscriptions;
                     DROP TABLE external_calendars;
                     DROP TABLE calendar_accounts;
                     DROP TABLE tasks;
                     DROP TABLE settings;
                     ALTER TABLE notes DROP COLUMN source;
                     PRAGMA user_version = 4;",
                )
                .expect("legacy v4 state is recreated");
        }
        let database = Database::open(&path).expect("version four database migrates");
        database.restore_note(&note_id).expect("note is restored");
        let results = database
            .search_notes("recuperabile", "all", None, 10, 0)
            .expect("search runs");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].note.id, note_id);
    }

    #[test]
    fn existing_version_one_databases_receive_all_migrations_and_keep_searchable_notes() {
        let directory = tempfile::tempdir().expect("temporary directory is created");
        let path = directory.path().join("legacy.sqlite3");
        let connection = rusqlite::Connection::open(&path).expect("database opens");
        connection
            .execute_batch(MIGRATION_1)
            .expect("version one schema is created");
        connection
            .execute(
                "INSERT INTO notes (id, title, content_json)
                 VALUES (?1, ?2, ?3)",
                params![
                    "legacy-note",
                    "Nota esistente",
                    r#"{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"contenuto legacy ricercabile"}]}]}"#
                ],
            )
            .expect("legacy note is written before migration");
        drop(connection);

        let migrated = Database::open(&path).expect("version one database migrates");
        {
            let connection = migrated.connection().expect("database lock is available");
            let version: u32 = connection
                .query_row("PRAGMA user_version", [], |row| row.get(0))
                .expect("schema version is readable");
            assert_eq!(version, super::CURRENT_SCHEMA_VERSION);
        }
        let backups = crate::backup::list_backups(directory.path()).expect("backups are listed");
        assert_eq!(backups.len(), 1, "a pre-migration backup is created");
        assert_eq!(backups[0].kind, crate::backup::BackupKind::PreMigration);
        let notes = migrated.list_notes(10, 0).expect("legacy note remains");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].title, "Nota esistente");
        assert_eq!(
            migrated
                .search_notes("esistente", "all", None, 10, 0)
                .expect("legacy notes are indexed")[0]
                .note
                .id,
            "legacy-note"
        );
        assert_eq!(
            migrated
                .search_notes("legacy", "all", None, 10, 0)
                .expect("legacy text is indexed")[0]
                .note
                .id,
            "legacy-note"
        );
    }

    #[test]
    fn version_three_databases_without_attachment_search_text_migrate_before_note_edits() {
        let connection = Connection::open_in_memory().expect("database opens");
        connection
            .execute_batch(MIGRATION_1)
            .expect("version one schema is created");
        connection
            .execute_batch(MIGRATION_2)
            .expect("version two schema is created");
        connection
            .execute(
                "INSERT INTO notes (id, title, content_json)
                 VALUES ('legacy-note', 'Nota esistente', ?1)",
                [serde_json::to_string(&empty_document()).expect("document serializes")],
            )
            .expect("legacy note is written before migration");
        connection
            .execute_batch(
                "CREATE VIRTUAL TABLE note_search USING fts5(
                    note_id UNINDEXED, title, content,
                     tokenize = 'unicode61 remove_diacritics 2'
                 );
                 INSERT INTO note_search (note_id, title, content)
                 VALUES ('legacy-note', 'Nota esistente', '');
                 CREATE TABLE note_visits (
                     note_id TEXT PRIMARY KEY NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
                     last_opened_at TEXT NOT NULL DEFAULT
                         (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
                 );
                 CREATE TABLE attachments (
                     id TEXT PRIMARY KEY NOT NULL,
                     note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
                     original_filename TEXT NOT NULL,
                     stored_filename TEXT NOT NULL UNIQUE,
                     mime_type TEXT NOT NULL,
                     size_bytes INTEGER NOT NULL,
                     sha256 TEXT NOT NULL,
                     created_at TEXT NOT NULL DEFAULT
                         (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                     deleted_at TEXT
                 );
                 CREATE INDEX attachments_note_id_idx ON attachments(note_id);
                 INSERT INTO attachments (
                     id, note_id, original_filename, stored_filename,
                     mime_type, size_bytes, sha256
                 ) VALUES (
                     'legacy-attachment', 'legacy-note', 'legacy-attachment.txt',
                     'legacy-attachment.txt', 'text/plain', 1,
                     '0000000000000000000000000000000000000000000000000000000000000000'
                 );
                 CREATE TABLE note_links (
                     source_note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
                     target_note_id TEXT NOT NULL REFERENCES notes(id) ON DELETE CASCADE,
                     label TEXT,
                     created_at TEXT NOT NULL DEFAULT
                         (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
                     PRIMARY KEY (source_note_id, target_note_id),
                     CHECK (source_note_id <> target_note_id)
                 );
                 CREATE INDEX note_links_target_id_idx ON note_links(target_note_id);
                 PRAGMA user_version = 3;",
            )
            .expect("legacy version three schema is created without search_text");

        let database = Database::from_connection(connection)
            .expect("legacy database is migrated before it is used");
        let edited = database
            .update_note("legacy-note", "Nota aggiornata", &empty_document())
            .expect("editing a note no longer queries a missing column");
        assert_eq!(edited.title, "Nota aggiornata");
        assert_eq!(
            database
                .search_notes("legacy-attachment", "all", None, 10, 0)
                .expect("legacy attachment filename is indexed")[0]
                .note
                .id,
            "legacy-note"
        );

        let directory = tempfile::tempdir().expect("attachment directory exists");
        database
            .add_attachment(
                "legacy-note",
                "nuovo-allegato.txt",
                b"contenuto ricercabile",
                directory.path(),
            )
            .expect("attachments can be added after migration");
        assert_eq!(
            database
                .search_notes("ricercabile", "all", None, 10, 0)
                .expect("new attachment contents are indexed")[0]
                .note
                .id,
            "legacy-note"
        );
    }

    #[test]
    fn backup_from_older_schema_restores_and_migrates() {
        // Backup creato da una versione dell'app con lo schema 2.
        let old_install = tempfile::tempdir().expect("old data directory");
        let connection =
            Connection::open(old_install.path().join("gna.sqlite3")).expect("legacy database");
        connection
            .execute_batch(MIGRATION_1)
            .expect("legacy migration 1");
        connection
            .execute_batch(MIGRATION_2)
            .expect("legacy migration 2");
        connection
            .execute(
                "INSERT INTO notes (id, title, content_json) VALUES (?1, ?2, ?3)",
                params![
                    "legacy-note",
                    "Verbale storico",
                    json!({"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": "decisione archiviata"}]}]})
                        .to_string()
                ],
            )
            .expect("legacy note");
        let backup = crate::backup::create_backup_with_connection(
            &connection,
            old_install.path(),
            crate::backup::BackupKind::Manual,
        )
        .expect("legacy backup");
        drop(connection);
        let backup_file = old_install.path().join("backups").join(&backup.file_name);

        // Ripristino su un'installazione nuova con lo schema corrente.
        let new_install = tempfile::tempdir().expect("new data directory");
        let database =
            Database::open(new_install.path().join("gna.sqlite3")).expect("current database");
        database
            .create_note("Nota da sostituire", &empty_document())
            .expect("note before restore");
        let verification = database
            .verify_backup_file(&backup_file)
            .expect("legacy backup verifies");
        assert_eq!(verification.schema_version, 2);
        assert_eq!(verification.note_count, 1);

        database
            .restore_backup_file(&backup_file)
            .expect("legacy backup restores");
        let schema: u32 = database
            .connection()
            .expect("connection")
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("schema version");
        assert_eq!(schema, super::CURRENT_SCHEMA_VERSION);
        let notes = database.list_notes(10, 0).expect("notes");
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].title, "Verbale storico");
        assert_eq!(
            database
                .search_notes("archiviata", "all", None, 10, 0)
                .expect("search after restore")[0]
                .note
                .id,
            "legacy-note"
        );
        assert!(database.check_integrity().expect("integrity").ok);

        // Il database ripristinato e migrato sopravvive al riavvio.
        drop(database);
        let reopened =
            Database::open(new_install.path().join("gna.sqlite3")).expect("database reopens");
        assert_eq!(reopened.list_notes(10, 0).expect("notes").len(), 1);
    }
}
