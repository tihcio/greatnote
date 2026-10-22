//! Funzioni di organizzazione delle note: cronologia delle revisioni,
//! modelli, duplicazione e copia, elenco dei file, contatori della barra
//! laterale e informazioni su una nota.

use std::{collections::HashMap, fs, path::Path};

use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    domain::note::{Note, validate_document},
    storage::{
        Database, StorageError, collect_text, get_note, normalize_organization_name,
        normalize_title, sync_search_index,
    },
};

/// Una nuova istantanea al massimo ogni 10 minuti di modifiche continue.
const REVISION_INTERVAL: &str = "-10 minutes";
/// Istantanee conservate per nota (le più vecchie vengono eliminate).
const MAX_REVISIONS_PER_NOTE: u32 = 50;
const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

// ---------------------------------------------------------------------------
// Revisioni

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteRevision {
    pub id: String,
    pub note_id: String,
    pub title: String,
    pub created_at: String,
    /// Testo semplice (anteprima e confronto), non il documento completo.
    pub preview: String,
    pub word_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteRevisionDetail {
    pub revision: NoteRevision,
    pub content: Value,
}

fn plain_text(content: &Value) -> String {
    let mut parts = Vec::new();
    collect_text(content, &mut parts);
    parts.join(" ")
}

fn word_count(text: &str) -> u64 {
    text.split_whitespace().count() as u64
}

fn revision_from_parts(
    id: String,
    note_id: String,
    title: String,
    content: &Value,
    created_at: String,
) -> NoteRevision {
    let text = plain_text(content);
    NoteRevision {
        id,
        note_id,
        title,
        created_at,
        preview: text.chars().take(300).collect(),
        word_count: word_count(&text),
    }
}

/// Salva la versione attuale della nota come revisione, se l'ultima è più
/// vecchia dell'intervallo (o non esiste). Chiamata prima di sovrascrivere il
/// contenuto, nella stessa transazione.
pub(crate) fn record_revision_if_due(
    transaction: &Transaction<'_>,
    note_id: &str,
) -> Result<(), StorageError> {
    // Nessuna revisione recente e la nota non è appena stata creata (una
    // nota nuova non genera revisioni del suo contenuto vuoto).
    let due: bool = transaction.query_row(
        "SELECT NOT EXISTS(
             SELECT 1 FROM note_revisions
             WHERE note_id = ?1 AND created_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?2))
         AND NOT EXISTS(
             SELECT 1 FROM notes
             WHERE id = ?1 AND created_at > strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?2)
               AND NOT EXISTS (SELECT 1 FROM note_revisions WHERE note_id = ?1))",
        params![note_id, REVISION_INTERVAL],
        |row| row.get(0),
    )?;
    if due {
        insert_revision(transaction, note_id)?;
    }
    Ok(())
}

fn insert_revision(connection: &Connection, note_id: &str) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO note_revisions (id, note_id, title, content_json)
         SELECT ?1, id, title, content_json FROM notes WHERE id = ?2",
        params![Uuid::new_v4().to_string(), note_id],
    )?;
    connection.execute(
        "DELETE FROM note_revisions
         WHERE note_id = ?1 AND id NOT IN (
             SELECT id FROM note_revisions WHERE note_id = ?1
             ORDER BY created_at DESC, rowid DESC LIMIT ?2)",
        params![note_id, MAX_REVISIONS_PER_NOTE],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Modelli

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub title: String,
    pub content: Value,
    /// Modello predefinito dell'app: non modificabile né eliminabile.
    pub builtin: bool,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

fn heading(level: u8, text: &str) -> Value {
    json!({ "type": "heading", "attrs": { "level": level },
            "content": [{ "type": "text", "text": text }] })
}

fn bullets(items: &[&str]) -> Value {
    json!({ "type": "bulletList", "content": items.iter().map(|item| json!({
        "type": "listItem",
        "content": [if item.is_empty() {
            json!({ "type": "paragraph" })
        } else {
            json!({ "type": "paragraph", "content": [{ "type": "text", "text": item }] })
        }]
    })).collect::<Vec<_>>() })
}

fn builtin_templates(english: bool) -> Vec<Template> {
    let template = |id: &str, name: &str, title: &str, content: Vec<Value>| Template {
        id: format!("builtin:{id}"),
        name: name.to_owned(),
        title: title.to_owned(),
        content: json!({ "type": "doc", "content": content }),
        builtin: true,
        created_at: None,
        updated_at: None,
    };
    if english {
        vec![
            template(
                "meeting",
                "Meeting notes",
                "Meeting",
                vec![
                    heading(2, "Attendees"),
                    bullets(&[""]),
                    heading(2, "Agenda"),
                    bullets(&[""]),
                    heading(2, "Decisions"),
                    bullets(&[""]),
                    heading(2, "Action items"),
                    bullets(&[""]),
                ],
            ),
            template(
                "journal",
                "Daily journal",
                "Journal",
                vec![
                    heading(2, "Today I am grateful for"),
                    bullets(&[""]),
                    heading(2, "Priorities"),
                    bullets(&["", "", ""]),
                    heading(2, "Notes"),
                    json!({ "type": "paragraph" }),
                ],
            ),
            template(
                "project",
                "Project plan",
                "Project",
                vec![
                    heading(2, "Goal"),
                    json!({ "type": "paragraph" }),
                    heading(2, "Milestones"),
                    bullets(&[""]),
                    heading(2, "Risks"),
                    bullets(&[""]),
                    heading(2, "Next steps"),
                    bullets(&[""]),
                ],
            ),
        ]
    } else {
        vec![
            template(
                "meeting",
                "Verbale di riunione",
                "Riunione",
                vec![
                    heading(2, "Partecipanti"),
                    bullets(&[""]),
                    heading(2, "Ordine del giorno"),
                    bullets(&[""]),
                    heading(2, "Decisioni"),
                    bullets(&[""]),
                    heading(2, "Azioni"),
                    bullets(&[""]),
                ],
            ),
            template(
                "journal",
                "Diario giornaliero",
                "Diario",
                vec![
                    heading(2, "Oggi sono grato per"),
                    bullets(&[""]),
                    heading(2, "Priorità"),
                    bullets(&["", "", ""]),
                    heading(2, "Appunti"),
                    json!({ "type": "paragraph" }),
                ],
            ),
            template(
                "project",
                "Piano di progetto",
                "Progetto",
                vec![
                    heading(2, "Obiettivo"),
                    json!({ "type": "paragraph" }),
                    heading(2, "Tappe"),
                    bullets(&[""]),
                    heading(2, "Rischi"),
                    bullets(&[""]),
                    heading(2, "Prossimi passi"),
                    bullets(&[""]),
                ],
            ),
        ]
    }
}

fn template_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Template> {
    let content_json: String = row.get(3)?;
    let content = serde_json::from_str(&content_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(Template {
        id: row.get(0)?,
        name: row.get(1)?,
        title: row.get(2)?,
        content,
        builtin: false,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

fn get_template(
    connection: &Connection,
    id: &str,
    english: bool,
) -> Result<Template, StorageError> {
    if id.starts_with("builtin:") {
        return builtin_templates(english)
            .into_iter()
            .find(|template| template.id == id)
            .ok_or(StorageError::TemplateNotFound);
    }
    connection
        .query_row(
            "SELECT id, name, title, content_json, created_at, updated_at
             FROM templates WHERE id = ?1 AND deleted_at IS NULL",
            [id],
            template_from_row,
        )
        .optional()?
        .ok_or(StorageError::TemplateNotFound)
}

// ---------------------------------------------------------------------------
// File, contatori, informazioni

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    pub id: String,
    pub note_id: String,
    pub note_title: String,
    pub original_filename: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidebarCounts {
    pub all: u64,
    pub pinned: u64,
    pub archived: u64,
    pub unfiled: u64,
    pub trash: u64,
    pub files: u64,
    pub notebooks: HashMap<String, u64>,
    pub spaces: HashMap<String, u64>,
    pub tags: HashMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteInfo {
    pub id: String,
    pub title: String,
    pub notebook_name: Option<String>,
    pub space_name: Option<String>,
    pub tags: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
    pub archived_at: Option<String>,
    pub is_pinned: bool,
    pub source: Option<String>,
    pub word_count: u64,
    pub character_count: u64,
    pub attachment_count: u64,
    pub attachment_bytes: u64,
    pub link_count: u64,
    pub backlink_count: u64,
    pub open_task_count: u64,
    pub reminder_count: u64,
    pub revision_count: u64,
}

fn count(connection: &Connection, sql: &str, id: &str) -> Result<u64, StorageError> {
    let value: i64 = connection.query_row(sql, [id], |row| row.get(0))?;
    Ok(u64::try_from(value).unwrap_or(0))
}

fn grouped_counts(
    connection: &Connection,
    sql: &str,
) -> Result<HashMap<String, u64>, StorageError> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (id, value) = row?;
        map.insert(id, u64::try_from(value).unwrap_or(0));
    }
    Ok(map)
}

impl Database {
    // -----------------------------------------------------------------------
    // Revisioni

    pub fn list_note_revisions(&self, note_id: &str) -> Result<Vec<NoteRevision>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, note_id, title, content_json, created_at FROM note_revisions
             WHERE note_id = ?1 ORDER BY created_at DESC, rowid DESC",
        )?;
        let rows = statement.query_map([note_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        })?;
        let mut revisions = Vec::new();
        for row in rows {
            let (id, note_id, title, content_json, created_at) = row?;
            let content: Value = serde_json::from_str(&content_json)?;
            revisions.push(revision_from_parts(
                id, note_id, title, &content, created_at,
            ));
        }
        Ok(revisions)
    }

    /// Elimina tutte le revisioni di una nota (per esempio dopo averne
    /// cifrato una parte, che le revisioni conservano in chiaro). Con
    /// `secure_delete` SQLite sovrascrive anche le pagine liberate.
    pub fn delete_note_revisions(&self, note_id: &str) -> Result<usize, StorageError> {
        let connection = self.connection()?;
        connection.pragma_update(None, "secure_delete", true)?;
        let deleted =
            connection.execute("DELETE FROM note_revisions WHERE note_id = ?1", [note_id]);
        connection.pragma_update(None, "secure_delete", false)?;
        Ok(deleted?)
    }

    pub fn get_note_revision(&self, revision_id: &str) -> Result<NoteRevisionDetail, StorageError> {
        let connection = self.connection()?;
        let (id, note_id, title, content_json, created_at): (
            String,
            String,
            String,
            String,
            String,
        ) = connection
            .query_row(
                "SELECT id, note_id, title, content_json, created_at
                     FROM note_revisions WHERE id = ?1",
                [revision_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or(StorageError::RevisionNotFound)?;
        let content: Value = serde_json::from_str(&content_json)?;
        Ok(NoteRevisionDetail {
            revision: revision_from_parts(id, note_id, title, &content, created_at),
            content,
        })
    }

    /// Ripristina una revisione. La versione corrente viene prima salvata
    /// come revisione, così il ripristino si può a sua volta annullare.
    pub fn restore_note_revision(&self, revision_id: &str) -> Result<Note, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let (note_id, title, content_json): (String, String, String) = transaction
            .query_row(
                "SELECT note_revisions.note_id, note_revisions.title, note_revisions.content_json
                 FROM note_revisions
                 JOIN notes ON notes.id = note_revisions.note_id AND notes.deleted_at IS NULL
                 WHERE note_revisions.id = ?1",
                [revision_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or(StorageError::RevisionNotFound)?;
        let content: Value = serde_json::from_str(&content_json)?;
        insert_revision(&transaction, &note_id)?;
        transaction.execute(
            &format!(
                "UPDATE notes SET title = ?1, content_json = ?2, updated_at = {NOW} WHERE id = ?3"
            ),
            params![title, content_json, note_id],
        )?;
        sync_search_index(&transaction, &note_id, &title, &content)?;
        transaction.execute("DELETE FROM note_drafts WHERE note_id = ?1", [&note_id])?;
        let note = get_note(&transaction, &note_id)?.ok_or(StorageError::NoteNotFound)?;
        transaction.commit()?;
        Ok(note)
    }

    // -----------------------------------------------------------------------
    // Modelli

    /// Modelli predefiniti (nella lingua indicata) seguiti da quelli
    /// dell'utente in ordine alfabetico.
    pub fn list_templates(&self, english: bool) -> Result<Vec<Template>, StorageError> {
        let connection = self.connection()?;
        let mut templates = builtin_templates(english);
        let mut statement = connection.prepare(
            "SELECT id, name, title, content_json, created_at, updated_at
             FROM templates WHERE deleted_at IS NULL
             ORDER BY name COLLATE NOCASE, id",
        )?;
        let rows = statement.query_map([], template_from_row)?;
        for row in rows {
            templates.push(row?);
        }
        Ok(templates)
    }

    /// Salva il contenuto attuale di una nota come nuovo modello.
    pub fn create_template_from_note(
        &self,
        note_id: &str,
        name: &str,
    ) -> Result<Template, StorageError> {
        let name = normalize_organization_name(name)?;
        let connection = self.connection()?;
        let note = get_note(&connection, note_id)?.ok_or(StorageError::NoteNotFound)?;
        let id = Uuid::new_v4().to_string();
        connection.execute(
            "INSERT INTO templates (id, name, title, content_json) VALUES (?1, ?2, ?3, ?4)",
            params![id, name, note.title, serde_json::to_string(&note.content)?],
        )?;
        get_template(&connection, &id, false)
    }

    pub fn rename_template(&self, id: &str, name: &str) -> Result<Template, StorageError> {
        let name = normalize_organization_name(name)?;
        let connection = self.connection()?;
        let changed = connection.execute(
            &format!(
                "UPDATE templates SET name = ?1, updated_at = {NOW}
                 WHERE id = ?2 AND deleted_at IS NULL"
            ),
            params![name, id],
        )?;
        if changed == 0 {
            return Err(StorageError::TemplateNotFound);
        }
        get_template(&connection, id, false)
    }

    /// Crea una nota dal modello, nel taccuino indicato (se attivo).
    pub fn create_note_from_template(
        &self,
        template_id: &str,
        notebook_id: Option<&str>,
        english: bool,
    ) -> Result<Note, StorageError> {
        let template = {
            let connection = self.connection()?;
            get_template(&connection, template_id, english)?
        };
        validate_document(&template.content).map_err(StorageError::InvalidDocument)?;
        self.create_note_in_notebook(&template.title, &template.content, notebook_id)
    }

    // -----------------------------------------------------------------------
    // Duplicazione e copia

    /// Copia una nota (contenuto, tag e allegati) nello stesso taccuino o in
    /// quello indicato. Collegamenti, attività e promemoria non vengono
    /// copiati. `title_suffix` distingue un duplicato dall'originale.
    pub fn copy_note(
        &self,
        note_id: &str,
        notebook_id: Option<Option<&str>>,
        title_suffix: Option<&str>,
        data_directory: &Path,
    ) -> Result<Note, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let source = get_note(&transaction, note_id)?.ok_or(StorageError::NoteNotFound)?;
        let target_notebook = match notebook_id {
            Some(target) => target.map(str::to_owned),
            None => source.notebook_id.clone(),
        };
        if let Some(target) = &target_notebook {
            crate::storage::ensure_active(
                &transaction,
                "notebooks",
                target,
                StorageError::NotebookNotFound,
            )?;
        }
        let mut title = source.title.clone();
        if let Some(suffix) = title_suffix {
            title.push_str(suffix);
        }
        let title = normalize_title(&title.chars().take(512).collect::<String>())?;
        let id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO notes (id, notebook_id, title, content_json, source)
             VALUES (?1, ?2, ?3, ?4, 'copy')",
            params![
                id,
                target_notebook,
                title,
                serde_json::to_string(&source.content)?
            ],
        )?;
        transaction.execute(
            "INSERT INTO note_tags (note_id, tag_id)
             SELECT ?1, tag_id FROM note_tags
             JOIN tags ON tags.id = note_tags.tag_id AND tags.deleted_at IS NULL
             WHERE note_id = ?2",
            params![id, note_id],
        )?;

        // Allegati: nuovi file con nuovi nomi, così le due note restano
        // indipendenti (eliminarne una non tocca i file dell'altra).
        let attachments = {
            let mut statement = transaction.prepare(
                "SELECT id, stored_filename FROM attachments
                 WHERE note_id = ?1 AND deleted_at IS NULL",
            )?;
            statement
                .query_map([note_id], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()?
        };
        let mut written = Vec::new();
        let mut content = source.content.clone();
        let result = (|| -> Result<(), StorageError> {
            let mut remapped = std::collections::HashMap::new();
            for (attachment_id, stored) in &attachments {
                let (_, bytes) = crate::storage::verified_attachment(
                    &transaction,
                    note_id,
                    attachment_id,
                    data_directory,
                )?;
                let extension = stored.rsplit_once('.').map_or("bin", |(_, ext)| ext);
                let new_id = Uuid::new_v4().to_string();
                let new_stored = format!("{new_id}.{extension}");
                let path =
                    crate::storage::write_attachment_file(data_directory, &new_stored, &bytes)?;
                written.push(path);
                transaction.execute(
                    "INSERT INTO attachments (id, note_id, original_filename, stored_filename,
                         mime_type, size_bytes, sha256, search_text, ocr_text, ocr_status,
                         ocr_engine, ocr_error, ocr_at)
                     SELECT ?1, ?2, original_filename, ?3, mime_type, size_bytes, sha256,
                            search_text, ocr_text, ocr_status, ocr_engine, ocr_error, ocr_at
                     FROM attachments WHERE id = ?4",
                    params![new_id, id, new_stored, attachment_id],
                )?;
                remapped.insert(attachment_id.clone(), new_id);
            }
            // Le immagini nel testo puntano agli allegati della copia.
            if !remapped.is_empty() {
                crate::domain::note::remap_image_attachments(&mut content, &remapped);
                transaction.execute(
                    "UPDATE notes SET content_json = ?1 WHERE id = ?2",
                    params![serde_json::to_string(&content)?, id],
                )?;
            }
            sync_search_index(&transaction, &id, &title, &content)?;
            Ok(())
        })();
        if let Err(error) = result {
            for path in written {
                let _ = fs::remove_file(path);
            }
            return Err(error);
        }
        let note = get_note(&transaction, &id)?.ok_or(StorageError::NoteNotFound)?;
        if let Err(error) = transaction.commit() {
            for path in written {
                let _ = fs::remove_file(path);
            }
            return Err(error.into());
        }
        Ok(note)
    }

    // -----------------------------------------------------------------------
    // File, contatori, informazioni

    /// Allegati di tutte le note attive, dal più recente.
    pub fn list_files(
        &self,
        search: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<FileEntry>, StorageError> {
        crate::storage::validate_page(limit)?;
        let search = search.map(str::trim).filter(|value| !value.is_empty());
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT attachments.id, attachments.note_id, notes.title,
                    attachments.original_filename, attachments.mime_type,
                    attachments.size_bytes, attachments.created_at
             FROM attachments
             JOIN notes ON notes.id = attachments.note_id AND notes.deleted_at IS NULL
             WHERE attachments.deleted_at IS NULL
               AND (?1 IS NULL
                    OR instr(lower(attachments.original_filename), lower(?1)) > 0
                    OR instr(lower(notes.title), lower(?1)) > 0)
             ORDER BY attachments.created_at DESC, attachments.id
             LIMIT ?2 OFFSET ?3",
        )?;
        let rows = statement.query_map(params![search, limit, offset], |row| {
            Ok(FileEntry {
                id: row.get(0)?,
                note_id: row.get(1)?,
                note_title: row.get(2)?,
                original_filename: row.get(3)?,
                mime_type: row.get(4)?,
                size_bytes: u64::try_from(row.get::<_, i64>(5)?).unwrap_or(0),
                created_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn sidebar_counts(&self) -> Result<SidebarCounts, StorageError> {
        let connection = self.connection()?;
        let (all, pinned, archived, unfiled, trashed_notes): (i64, i64, i64, i64, i64) = connection
            .query_row(
                "SELECT
                     COALESCE(SUM(deleted_at IS NULL AND archived_at IS NULL), 0),
                     COALESCE(SUM(deleted_at IS NULL AND archived_at IS NULL AND is_pinned = 1), 0),
                     COALESCE(SUM(deleted_at IS NULL AND archived_at IS NOT NULL), 0),
                     COALESCE(SUM(deleted_at IS NULL AND archived_at IS NULL
                                  AND notebook_id IS NULL), 0),
                     COALESCE(SUM(deleted_at IS NOT NULL), 0)
                 FROM notes",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )?;
        let trashed_items: i64 = connection.query_row(
            "SELECT (SELECT COUNT(*) FROM spaces WHERE deleted_at IS NOT NULL)
                  + (SELECT COUNT(*) FROM notebooks WHERE deleted_at IS NOT NULL)
                  + (SELECT COUNT(*) FROM tags WHERE deleted_at IS NOT NULL)
                  + (SELECT COUNT(*) FROM tasks WHERE deleted_at IS NOT NULL)
                  + (SELECT COUNT(*) FROM events WHERE deleted_at IS NOT NULL)
                  + (SELECT COUNT(*) FROM templates WHERE deleted_at IS NOT NULL)",
            [],
            |row| row.get(0),
        )?;
        let files: i64 = connection.query_row(
            "SELECT COUNT(*) FROM attachments
             JOIN notes ON notes.id = attachments.note_id AND notes.deleted_at IS NULL
             WHERE attachments.deleted_at IS NULL",
            [],
            |row| row.get(0),
        )?;
        let active = "notes.deleted_at IS NULL AND notes.archived_at IS NULL";
        Ok(SidebarCounts {
            all: u64::try_from(all).unwrap_or(0),
            pinned: u64::try_from(pinned).unwrap_or(0),
            archived: u64::try_from(archived).unwrap_or(0),
            unfiled: u64::try_from(unfiled).unwrap_or(0),
            trash: u64::try_from(trashed_notes + trashed_items).unwrap_or(0),
            files: u64::try_from(files).unwrap_or(0),
            notebooks: grouped_counts(
                &connection,
                &format!(
                    "SELECT notebook_id, COUNT(*) FROM notes
                     WHERE {active} AND notebook_id IS NOT NULL GROUP BY notebook_id"
                ),
            )?,
            spaces: grouped_counts(
                &connection,
                &format!(
                    "SELECT notebooks.space_id, COUNT(*) FROM notes
                     JOIN notebooks ON notebooks.id = notes.notebook_id
                     WHERE {active} AND notebooks.space_id IS NOT NULL
                     GROUP BY notebooks.space_id"
                ),
            )?,
            tags: grouped_counts(
                &connection,
                &format!(
                    "SELECT note_tags.tag_id, COUNT(*) FROM note_tags
                     JOIN notes ON notes.id = note_tags.note_id
                     WHERE {active} GROUP BY note_tags.tag_id"
                ),
            )?,
        })
    }

    pub fn note_info(&self, note_id: &str) -> Result<NoteInfo, StorageError> {
        let connection = self.connection()?;
        let (note, notebook_name, space_name, source): (
            Note,
            Option<String>,
            Option<String>,
            Option<String>,
        ) = connection
            .query_row(
                "SELECT notes.id, notes.notebook_id, notes.title, notes.content_json,
                            notes.created_at, notes.updated_at, notes.is_pinned,
                            notes.archived_at, notebooks.name, spaces.name, notes.source
                     FROM notes
                     LEFT JOIN notebooks ON notebooks.id = notes.notebook_id
                     LEFT JOIN spaces ON spaces.id = notebooks.space_id
                     WHERE notes.id = ?1 AND notes.deleted_at IS NULL",
                [note_id],
                |row| {
                    Ok((
                        crate::storage::note_from_row(row)?,
                        row.get(8)?,
                        row.get(9)?,
                        row.get(10)?,
                    ))
                },
            )
            .optional()?
            .ok_or(StorageError::NoteNotFound)?;
        let text = plain_text(&note.content);
        let tags = connection
            .prepare(
                "SELECT tags.name FROM tags JOIN note_tags ON note_tags.tag_id = tags.id
                 WHERE note_tags.note_id = ?1 AND tags.deleted_at IS NULL
                 ORDER BY tags.name COLLATE NOCASE",
            )?
            .query_map([note_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        let attachment_bytes: i64 = connection.query_row(
            "SELECT COALESCE(SUM(size_bytes), 0) FROM attachments
             WHERE note_id = ?1 AND deleted_at IS NULL",
            [note_id],
            |row| row.get(0),
        )?;
        Ok(NoteInfo {
            id: note.id.clone(),
            title: note.title,
            notebook_name,
            space_name,
            tags,
            created_at: note.created_at,
            updated_at: note.updated_at,
            archived_at: note.archived_at,
            is_pinned: note.is_pinned,
            source,
            word_count: word_count(&text),
            character_count: text
                .chars()
                .filter(|character| !character.is_whitespace())
                .count() as u64,
            attachment_count: count(
                &connection,
                "SELECT COUNT(*) FROM attachments WHERE note_id = ?1 AND deleted_at IS NULL",
                note_id,
            )?,
            attachment_bytes: u64::try_from(attachment_bytes).unwrap_or(0),
            link_count: count(
                &connection,
                "SELECT COUNT(*) FROM note_links WHERE source_note_id = ?1",
                note_id,
            )?,
            backlink_count: count(
                &connection,
                "SELECT COUNT(*) FROM note_links WHERE target_note_id = ?1",
                note_id,
            )?,
            open_task_count: count(
                &connection,
                "SELECT COUNT(*) FROM tasks
                 WHERE note_id = ?1 AND status = 'open' AND deleted_at IS NULL",
                note_id,
            )?,
            reminder_count: count(
                &connection,
                "SELECT COUNT(*) FROM reminders
                 WHERE entity_type = 'note' AND entity_id = ?1 AND status != 'done'",
                note_id,
            )?,
            revision_count: count(
                &connection,
                "SELECT COUNT(*) FROM note_revisions WHERE note_id = ?1",
                note_id,
            )?,
        })
    }
}

const UI_PREFERENCES_KEY: &str = "ui.preferences";
/// Limite della colonna `settings.value`.
const MAX_PREFERENCES_BYTES: usize = 4096;

impl Database {
    /// Preferenze dell'interfaccia come oggetto JSON (tema, zoom, barra
    /// laterale…). Il frontend applica i valori predefiniti ai campi assenti.
    pub fn ui_preferences(&self) -> Result<Value, StorageError> {
        let connection = self.connection()?;
        let stored: Option<String> = connection
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [UI_PREFERENCES_KEY],
                |row| row.get(0),
            )
            .optional()?;
        Ok(stored
            .and_then(|value| serde_json::from_str::<Value>(&value).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({})))
    }

    pub fn set_ui_preferences(&self, preferences: &Value) -> Result<Value, StorageError> {
        if !preferences.is_object() {
            return Err(StorageError::InvalidSetting(
                "le preferenze devono essere un oggetto",
            ));
        }
        let serialized = serde_json::to_string(preferences)?;
        if serialized.len() > MAX_PREFERENCES_BYTES {
            return Err(StorageError::InvalidSetting("preferenze troppo grandi"));
        }
        let connection = self.connection()?;
        connection.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET
                 value = excluded.value,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![UI_PREFERENCES_KEY, serialized],
        )?;
        Ok(preferences.clone())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{
        domain::note::empty_document,
        storage::{Database, StorageError},
        trash::TrashKind,
    };

    fn document(text: &str) -> serde_json::Value {
        json!({ "type": "doc", "content": [{ "type": "paragraph",
            "content": [{ "type": "text", "text": text }] }] })
    }

    /// Sposta indietro nel tempo la creazione della nota e le revisioni,
    /// per simulare modifiche distanti più di 10 minuti.
    fn age(database: &Database, note_id: &str) {
        let connection = database.connection().expect("connection");
        connection
            .execute(
                "UPDATE notes SET created_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now', '-1 hour')
                 WHERE id = ?1",
                [note_id],
            )
            .expect("age note");
        connection
            .execute(
                "UPDATE note_revisions
                 SET created_at = strftime('%Y-%m-%dT%H:%M:%fZ', created_at, '-1 hour')
                 WHERE note_id = ?1",
                [note_id],
            )
            .expect("age revisions");
    }

    #[test]
    fn revisions_are_recorded_at_intervals_and_restorable() {
        let database = Database::in_memory().expect("database");
        let note = database
            .create_note("Bozza", &document("uno"))
            .expect("note");
        // Una nota appena creata non genera revisioni a ogni battuta.
        database
            .update_note(&note.id, "Bozza", &document("due"))
            .expect("save");
        assert!(
            database
                .list_note_revisions(&note.id)
                .expect("revisions")
                .is_empty()
        );

        age(&database, &note.id);
        database
            .update_note(&note.id, "Bozza", &document("tre"))
            .expect("save");
        database
            .update_note(&note.id, "Bozza", &document("quattro"))
            .expect("save");
        let revisions = database.list_note_revisions(&note.id).expect("revisions");
        assert_eq!(revisions.len(), 1);
        assert_eq!(revisions[0].preview, "due");

        age(&database, &note.id);
        database
            .update_note(&note.id, "Finale", &document("cinque"))
            .expect("save");
        let revisions = database.list_note_revisions(&note.id).expect("revisions");
        assert_eq!(revisions.len(), 2);
        assert_eq!(revisions[0].preview, "quattro");

        let oldest = revisions[1].id.clone();
        let detail = database.get_note_revision(&oldest).expect("detail");
        assert_eq!(detail.content, document("due"));
        let restored = database.restore_note_revision(&oldest).expect("restore");
        assert_eq!(
            (restored.title.as_str(), &restored.content),
            ("Bozza", &document("due"))
        );
        // La versione sostituita resta recuperabile.
        let revisions = database.list_note_revisions(&note.id).expect("revisions");
        assert_eq!(revisions.len(), 3);
        assert_eq!(revisions[0].preview, "cinque");
        assert_eq!(
            database
                .search_notes("due", "all", None, 10, 0)
                .expect("search")
                .len(),
            1
        );
        assert!(matches!(
            database.restore_note_revision("missing"),
            Err(StorageError::RevisionNotFound)
        ));
    }

    #[test]
    fn revisions_of_one_note_can_be_deleted() {
        let database = Database::in_memory().expect("database");
        let note = database
            .create_note("Segreto", &document("PIN 1"))
            .expect("note");
        let other = database.create_note("Altra", &document("a")).expect("note");
        for (id, text) in [(&note.id, "PIN 2"), (&other.id, "b")] {
            age(&database, id);
            database
                .update_note(id, "x", &document(text))
                .expect("save");
        }
        assert_eq!(database.delete_note_revisions(&note.id).expect("delete"), 1);
        assert!(
            database
                .list_note_revisions(&note.id)
                .expect("revisions")
                .is_empty()
        );
        assert_eq!(
            database
                .list_note_revisions(&other.id)
                .expect("revisions")
                .len(),
            1
        );
    }

    #[test]
    fn revisions_are_capped_per_note() {
        let database = Database::in_memory().expect("database");
        let note = database
            .create_note("Molte", &empty_document())
            .expect("note");
        {
            let connection = database.connection().expect("connection");
            for _ in 0..60 {
                super::insert_revision(&connection, &note.id).expect("revision");
            }
        }
        assert_eq!(
            database
                .list_note_revisions(&note.id)
                .expect("revisions")
                .len(),
            super::MAX_REVISIONS_PER_NOTE as usize
        );
    }

    #[test]
    fn templates_create_notes_and_go_through_trash() {
        let database = Database::in_memory().expect("database");
        let italian = database.list_templates(false).expect("templates");
        assert_eq!(
            italian.iter().filter(|template| template.builtin).count(),
            3
        );
        assert_eq!(italian[0].name, "Verbale di riunione");
        assert_eq!(
            database.list_templates(true).expect("templates")[0].name,
            "Meeting notes"
        );

        let from_builtin = database
            .create_note_from_template("builtin:meeting", None, false)
            .expect("note from builtin");
        assert_eq!(from_builtin.title, "Riunione");

        let source = database
            .create_note("Settimana", &document("lunedì"))
            .expect("note");
        let template = database
            .create_template_from_note(&source.id, "Settimanale")
            .expect("template");
        assert!(!template.builtin);
        let notebook = database
            .create_notebook(None, "Diario", "")
            .expect("notebook");
        let created = database
            .create_note_from_template(&template.id, Some(&notebook.id), false)
            .expect("note from template");
        assert_eq!(created.content, document("lunedì"));
        assert_eq!(created.notebook_id.as_deref(), Some(notebook.id.as_str()));
        assert_eq!(
            database
                .rename_template(&template.id, "Settimana tipo")
                .expect("rename")
                .name,
            "Settimana tipo"
        );
        assert!(database.rename_template(&template.id, "  ").is_err());

        database
            .move_item_to_trash(TrashKind::Template, &template.id)
            .expect("trash template");
        assert_eq!(database.list_templates(false).expect("templates").len(), 3);
        assert!(matches!(
            database.create_note_from_template(&template.id, None, false),
            Err(StorageError::TemplateNotFound)
        ));
        let items = database.list_trash_items().expect("trash");
        assert_eq!(
            (items[0].kind, items[0].title.as_str()),
            (TrashKind::Template, "Settimana tipo")
        );
        database
            .restore_trash_item(TrashKind::Template, &template.id)
            .expect("restore");
        assert_eq!(database.list_templates(false).expect("templates").len(), 4);
        assert!(matches!(
            database.move_item_to_trash(TrashKind::Template, "builtin:meeting"),
            Err(StorageError::TemplateNotFound)
        ));
    }

    #[test]
    fn copies_have_independent_attachments_and_keep_tags() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let data = directory.path();
        let database = Database::open(data.join("gna.sqlite3")).expect("database");
        let notebook = database
            .create_notebook(None, "Origine", "")
            .expect("notebook");
        let other = database
            .create_notebook(None, "Destinazione", "")
            .expect("notebook");
        let source = database
            .create_note_in_notebook("Contratto", &document("clausole"), Some(&notebook.id))
            .expect("note");
        let tag = database.create_tag("legale", None).expect("tag");
        database
            .set_note_tags(&source.id, std::slice::from_ref(&tag.id))
            .expect("tags");
        database
            .add_attachment(&source.id, "firma.txt", b"firmato", data)
            .expect("attachment");

        let duplicate = database
            .copy_note(&source.id, None, Some(" (copia)"), data)
            .expect("duplicate");
        assert_eq!(duplicate.title, "Contratto (copia)");
        assert_eq!(duplicate.notebook_id.as_deref(), Some(notebook.id.as_str()));
        assert_eq!(
            database.list_note_tags(&duplicate.id).expect("tags")[0].id,
            tag.id
        );

        let copied = database
            .copy_note(&source.id, Some(Some(&other.id)), None, data)
            .expect("copy to");
        assert_eq!(copied.title, "Contratto");
        assert_eq!(copied.notebook_id.as_deref(), Some(other.id.as_str()));
        let unfiled = database
            .copy_note(&source.id, Some(None), None, data)
            .expect("copy unfiled");
        assert!(unfiled.notebook_id.is_none());

        // Eliminare l'originale non tocca i file delle copie.
        database.trash_note(&source.id).expect("trash");
        database
            .delete_note_permanently(&source.id, data)
            .expect("purge");
        let attachments = database.list_attachments(&copied.id).expect("attachments");
        database
            .verified_attachment_path(&copied.id, &attachments[0].id, data)
            .expect("copy keeps its file");
        assert_eq!(
            database
                .search_notes("clausole", "all", None, 10, 0)
                .expect("search")
                .len(),
            3
        );

        database
            .move_item_to_trash(TrashKind::Notebook, &other.id)
            .expect("trash notebook");
        assert!(matches!(
            database.copy_note(&copied.id, Some(Some(&other.id)), None, data),
            Err(StorageError::NotebookNotFound)
        ));
        assert!(database.check_integrity().expect("integrity").ok);
    }

    #[test]
    fn ui_preferences_persist_and_are_validated() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("gna.sqlite3");
        {
            let database = Database::open(&path).expect("database");
            assert_eq!(database.ui_preferences().expect("defaults"), json!({}));
            database
                .set_ui_preferences(&json!({ "theme": "dark", "zoom": 1.1 }))
                .expect("save");
            assert!(database.set_ui_preferences(&json!(["no"])).is_err());
            assert!(
                database
                    .set_ui_preferences(&json!({ "x": "y".repeat(5000) }))
                    .is_err()
            );
        }
        let database = Database::open(&path).expect("reopen");
        assert_eq!(database.ui_preferences().expect("load")["theme"], "dark");
    }

    #[test]
    fn files_counts_and_note_info_reflect_the_library() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let data = directory.path();
        let database = Database::open(data.join("gna.sqlite3")).expect("database");
        let space = database.create_space("Lavoro", "").expect("space");
        let notebook = database
            .create_notebook(Some(&space.id), "Clienti", "")
            .expect("notebook");
        let note = database
            .create_note_in_notebook("Offerta", &document("tre parole qui"), Some(&notebook.id))
            .expect("note");
        let other = database
            .create_note("Altra", &empty_document())
            .expect("note");
        database
            .create_note_link(&other.id, &note.id, None)
            .expect("link");
        let tag = database.create_tag("vendite", None).expect("tag");
        database
            .set_note_tags(&note.id, std::slice::from_ref(&tag.id))
            .expect("tags");
        database
            .add_attachment(&note.id, "preventivo.txt", b"12345", data)
            .expect("attachment");
        database.set_note_pinned(&note.id, true).expect("pin");
        let trashed = database
            .create_note("Via", &empty_document())
            .expect("note");
        database
            .add_attachment(&trashed.id, "nascosto.txt", b"x", data)
            .expect("attachment");
        database.trash_note(&trashed.id).expect("trash");

        let files = database.list_files(None, 50, 0).expect("files");
        assert_eq!(files.len(), 1);
        assert_eq!(
            (files[0].note_title.as_str(), files[0].size_bytes),
            ("Offerta", 5)
        );
        assert_eq!(
            database
                .list_files(Some("PREVENTIVO"), 50, 0)
                .expect("files")
                .len(),
            1
        );
        assert!(
            database
                .list_files(Some("%"), 50, 0)
                .expect("files")
                .is_empty()
        );

        let counts = database.sidebar_counts().expect("counts");
        assert_eq!(
            (
                counts.all,
                counts.pinned,
                counts.unfiled,
                counts.trash,
                counts.files
            ),
            (2, 1, 1, 1, 1)
        );
        assert_eq!(counts.notebooks.get(&notebook.id), Some(&1));
        assert_eq!(counts.spaces.get(&space.id), Some(&1));
        assert_eq!(counts.tags.get(&tag.id), Some(&1));

        let info = database.note_info(&note.id).expect("info");
        assert_eq!(info.notebook_name.as_deref(), Some("Clienti"));
        assert_eq!(info.space_name.as_deref(), Some("Lavoro"));
        assert_eq!(info.tags, ["vendite"]);
        assert_eq!((info.word_count, info.character_count), (3, 12));
        assert_eq!((info.attachment_count, info.attachment_bytes), (1, 5));
        assert_eq!((info.link_count, info.backlink_count), (0, 1));
        assert!(info.is_pinned);
        assert!(matches!(
            database.note_info(&trashed.id),
            Err(StorageError::NoteNotFound)
        ));
    }
}
