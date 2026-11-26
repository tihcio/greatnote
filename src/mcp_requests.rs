//! Richieste distruttive del server MCP: spostare una nota nel cestino o
//! riscriverne il testo.
//!
//! Il server MCP non le applica: le accoda in `mcp_requests` e risponde al
//! client che serve la conferma dell'utente. L'app mostra le richieste in
//! attesa con l'anteprima (titolo e testo attuali e proposti) e le applica
//! solo dopo «Conferma». Prima di riscrivere una nota salva la versione
//! attuale nella cronologia, così anche una conferma sbagliata si annulla;
//! lo spostamento nel cestino si annulla dal cestino (ADR 0014).

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::{
    storage::{Database, StorageError, get_note},
    time,
};

/// Richieste in attesa oltre le quali il server rifiuta le nuove: un agente
/// che si ripete non riempie l'elenco dell'utente.
pub const MAX_PENDING_REQUESTS: usize = 20;
/// Una richiesta non decisa scade dopo questo intervallo (modificatore di
/// `strftime`).
const REQUEST_LIFETIME: &str = "-7 days";
/// Richieste già decise conservate per `request_status` e per il registro.
const DECIDED_KEPT: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpAction {
    TrashNote,
    ReplaceNote,
}

impl McpAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::TrashNote => "trash_note",
            Self::ReplaceNote => "replace_note",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "trash_note" => Some(Self::TrashNote),
            "replace_note" => Some(Self::ReplaceNote),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpRequestStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
    Failed,
}

impl McpRequestStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "pending" => Some(Self::Pending),
            "approved" => Some(Self::Approved),
            "rejected" => Some(Self::Rejected),
            "expired" => Some(Self::Expired),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

/// Richiesta con quanto serve all'anteprima nell'app.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpRequest {
    pub id: String,
    pub action: McpAction,
    pub note_id: String,
    /// Titolo e testo attuali della nota (vuoti se la nota non c'è più).
    pub note_title: String,
    pub note_content: Value,
    /// Titolo e testo proposti (solo per `replace_note`).
    pub title: Option<String>,
    pub content: Option<Value>,
    pub reason: String,
    pub status: McpRequestStatus,
    pub error: Option<String>,
    pub created_at: String,
    pub decided_at: Option<String>,
    /// La nota è cambiata dopo la richiesta: la proposta non tiene conto
    /// delle modifiche successive.
    pub note_changed: bool,
    /// La nota è nel cestino o è stata eliminata.
    pub note_missing: bool,
}

const COLUMNS: &str = "id, action, note_id, title, content_json, reason, base_updated_at,
                       status, error, created_at, decided_at";

struct Row {
    id: String,
    action: String,
    note_id: String,
    title: Option<String>,
    content_json: Option<String>,
    reason: String,
    base_updated_at: String,
    status: String,
    error: Option<String>,
    created_at: String,
    decided_at: Option<String>,
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok(Row {
        id: row.get(0)?,
        action: row.get(1)?,
        note_id: row.get(2)?,
        title: row.get(3)?,
        content_json: row.get(4)?,
        reason: row.get(5)?,
        base_updated_at: row.get(6)?,
        status: row.get(7)?,
        error: row.get(8)?,
        created_at: row.get(9)?,
        decided_at: row.get(10)?,
    })
}

fn invalid() -> StorageError {
    StorageError::McpRequestNotFound
}

fn complete(connection: &Connection, row: Row) -> Result<McpRequest, StorageError> {
    let action = McpAction::parse(&row.action).ok_or_else(invalid)?;
    let status = McpRequestStatus::parse(&row.status).ok_or_else(invalid)?;
    let content = row
        .content_json
        .as_deref()
        .map(serde_json::from_str)
        .transpose()?;
    let note = get_note(connection, &row.note_id)?;
    let (note_title, note_content, note_changed) = match &note {
        Some(note) => (
            note.title.clone(),
            note.content.clone(),
            note.updated_at != row.base_updated_at,
        ),
        None => (String::new(), Value::Null, false),
    };
    Ok(McpRequest {
        id: row.id,
        action,
        note_id: row.note_id,
        note_title,
        note_content,
        title: row.title,
        content,
        reason: row.reason,
        status,
        error: row.error,
        created_at: row.created_at,
        decided_at: row.decided_at,
        note_changed,
        note_missing: note.is_none(),
    })
}

fn load(connection: &Connection, id: &str) -> Result<McpRequest, StorageError> {
    let row = connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM mcp_requests WHERE id = ?1"),
            [id],
            read_row,
        )
        .optional()?
        .ok_or_else(invalid)?;
    complete(connection, row)
}

/// Scadenza delle richieste vecchie e pulizia di quelle decise da tempo.
fn expire(connection: &Connection) -> Result<(), StorageError> {
    connection.execute(
        "UPDATE mcp_requests
         SET status = 'expired', decided_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
         WHERE status = 'pending'
           AND created_at < strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?1)",
        [REQUEST_LIFETIME],
    )?;
    connection.execute(
        "DELETE FROM mcp_requests
         WHERE status <> 'pending' AND id NOT IN (
             SELECT id FROM mcp_requests WHERE status <> 'pending'
             ORDER BY decided_at DESC LIMIT ?1)",
        [DECIDED_KEPT as i64],
    )?;
    Ok(())
}

/// Nuova richiesta da accodare (dal server MCP).
pub struct NewMcpRequest<'a> {
    pub action: McpAction,
    pub note_id: &'a str,
    pub title: Option<&'a str>,
    pub content: Option<&'a Value>,
    pub reason: &'a str,
}

impl Database {
    /// Accoda una richiesta per una nota attiva. Non modifica la nota.
    pub fn queue_mcp_request(
        &self,
        request: &NewMcpRequest<'_>,
    ) -> Result<McpRequest, StorageError> {
        if let Some(title) = request.title {
            crate::storage::normalize_title(title)?;
        }
        if let Some(content) = request.content {
            crate::domain::note::validate_document(content)
                .map_err(StorageError::InvalidDocument)?;
        }
        let content_json = request.content.map(serde_json::to_string).transpose()?;
        let reason: String = request.reason.trim().chars().take(500).collect();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        expire(&transaction)?;
        let pending: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM mcp_requests WHERE status = 'pending'",
            [],
            |row| row.get(0),
        )?;
        if pending as usize >= MAX_PENDING_REQUESTS {
            return Err(StorageError::TooManyMcpRequests(MAX_PENDING_REQUESTS));
        }
        let base_updated_at: String = transaction
            .query_row(
                "SELECT updated_at FROM notes WHERE id = ?1 AND deleted_at IS NULL",
                [request.note_id],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(StorageError::NoteNotFound)?;
        let id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO mcp_requests
                 (id, action, note_id, title, content_json, reason, base_updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                request.action.as_str(),
                request.note_id,
                request.title,
                content_json,
                reason,
                base_updated_at
            ],
        )?;
        transaction.commit()?;
        load(&connection, &id)
    }

    /// Una richiesta, in qualunque stato.
    pub fn mcp_request(&self, id: &str) -> Result<McpRequest, StorageError> {
        load(&*self.connection()?, id)
    }

    /// Richieste in attesa di conferma, dalla più vecchia (per l'app: chiude
    /// quelle scadute).
    pub fn pending_mcp_requests(&self) -> Result<Vec<McpRequest>, StorageError> {
        let connection = self.connection()?;
        expire(&connection)?;
        let rows = connection
            .prepare(&format!(
                "SELECT {COLUMNS} FROM mcp_requests
                 WHERE status = 'pending'
                   AND created_at >= strftime('%Y-%m-%dT%H:%M:%fZ', 'now', ?1)
                 ORDER BY created_at, rowid"
            ))?
            .query_map([REQUEST_LIFETIME], read_row)?
            .collect::<Result<Vec<_>, _>>()?;
        rows.into_iter()
            .map(|row| complete(&connection, row))
            .collect()
    }

    /// Applica una richiesta confermata dall'utente. La richiesta viene
    /// segnata come decisa prima di toccare la nota, così una doppia
    /// conferma non la applica due volte; se l'operazione fallisce resta
    /// `failed` con il motivo.
    pub fn approve_mcp_request(&self, id: &str) -> Result<McpRequest, StorageError> {
        let request = self.claim(id, McpRequestStatus::Approved)?;
        let outcome = match request.action {
            McpAction::TrashNote => self.trash_note(&request.note_id).map(|_| ()),
            McpAction::ReplaceNote => self.apply_replacement(&request),
        };
        if let Err(error) = outcome {
            self.connection()?.execute(
                "UPDATE mcp_requests SET status = 'failed', error = ?2 WHERE id = ?1",
                params![id, error.to_string()],
            )?;
            return Err(error);
        }
        self.mcp_request(id)
    }

    pub fn reject_mcp_request(&self, id: &str) -> Result<McpRequest, StorageError> {
        self.claim(id, McpRequestStatus::Rejected)?;
        self.mcp_request(id)
    }

    /// Passa una richiesta ancora in attesa allo stato indicato.
    fn claim(&self, id: &str, status: McpRequestStatus) -> Result<McpRequest, StorageError> {
        let connection = self.connection()?;
        expire(&connection)?;
        let changed = connection.execute(
            "UPDATE mcp_requests
             SET status = ?2, decided_at = ?3
             WHERE id = ?1 AND status = 'pending'",
            params![id, status.as_str(), time::format_utc(time::now())],
        )?;
        if changed == 0 {
            return Err(StorageError::McpRequestNotFound);
        }
        load(&connection, id)
    }

    fn apply_replacement(&self, request: &McpRequest) -> Result<(), StorageError> {
        if request.note_missing {
            return Err(StorageError::NoteNotFound);
        }
        let content = request.content.as_ref().ok_or_else(invalid)?;
        let title = request.title.as_deref().unwrap_or(&request.note_title);
        {
            // La versione attuale resta sempre nella cronologia, anche se ne
            // è stata salvata una da poco.
            let connection = self.connection()?;
            crate::library::record_revision_now(&connection, &request.note_id)?;
        }
        self.update_note(&request.note_id, title, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn document(text: &str) -> Value {
        json!({ "type": "doc", "content": [
            { "type": "paragraph", "content": [{ "type": "text", "text": text }] }
        ]})
    }

    fn replace<'a>(note_id: &'a str, content: &'a Value) -> NewMcpRequest<'a> {
        NewMcpRequest {
            action: McpAction::ReplaceNote,
            note_id,
            title: Some("Nuovo titolo"),
            content: Some(content),
            reason: "  sistemata la formattazione  ",
        }
    }

    #[test]
    fn replacement_waits_for_confirmation_and_keeps_the_old_text() {
        let database = Database::in_memory().expect("db");
        let note = database
            .create_note("Titolo", &document("testo originale"))
            .expect("note");
        let proposed = document("testo riscritto");
        let request = database
            .queue_mcp_request(&replace(&note.id, &proposed))
            .expect("queued");
        assert_eq!(request.status, McpRequestStatus::Pending);
        assert_eq!(request.reason, "sistemata la formattazione");
        assert_eq!(request.note_title, "Titolo");
        assert!(!request.note_changed);
        // Accodare non cambia la nota.
        assert_eq!(
            database.get_note(&note.id).expect("note").content,
            document("testo originale")
        );
        assert_eq!(database.pending_mcp_requests().expect("pending").len(), 1);

        let approved = database.approve_mcp_request(&request.id).expect("approve");
        assert_eq!(approved.status, McpRequestStatus::Approved);
        assert!(approved.decided_at.is_some());
        let updated = database.get_note(&note.id).expect("note");
        assert_eq!(updated.title, "Nuovo titolo");
        assert_eq!(updated.content, proposed);
        // Il testo di prima è nella cronologia.
        let revisions = database.list_note_revisions(&note.id).expect("revisions");
        let first = database
            .get_note_revision(&revisions[0].id)
            .expect("revision");
        assert_eq!(first.content, document("testo originale"));
        // Una seconda conferma non fa nulla.
        assert!(matches!(
            database.approve_mcp_request(&request.id),
            Err(StorageError::McpRequestNotFound)
        ));
        assert!(database.pending_mcp_requests().expect("pending").is_empty());
    }

    #[test]
    fn trash_requests_can_be_rejected_or_approved() {
        let database = Database::in_memory().expect("db");
        let note = database
            .create_note("Da buttare", &document("x"))
            .expect("note");
        let trash = NewMcpRequest {
            action: McpAction::TrashNote,
            note_id: &note.id,
            title: None,
            content: None,
            reason: "",
        };
        let first = database.queue_mcp_request(&trash).expect("queued");
        let rejected = database.reject_mcp_request(&first.id).expect("reject");
        assert_eq!(rejected.status, McpRequestStatus::Rejected);
        assert!(database.get_note(&note.id).is_ok());

        let second = database.queue_mcp_request(&trash).expect("queued");
        database.approve_mcp_request(&second.id).expect("approve");
        assert!(matches!(
            database.get_note(&note.id),
            Err(StorageError::NoteNotFound)
        ));
        // Nota nel cestino: nessuna nuova richiesta.
        assert!(matches!(
            database.queue_mcp_request(&trash),
            Err(StorageError::NoteNotFound)
        ));
    }

    #[test]
    fn later_edits_are_flagged_and_failures_recorded() {
        let database = Database::in_memory().expect("db");
        let note = database
            .create_note("Titolo", &document("uno"))
            .expect("note");
        let proposed = document("due");
        let request = database
            .queue_mcp_request(&replace(&note.id, &proposed))
            .expect("queued");
        database
            .connection()
            .expect("connection")
            .execute(
                "UPDATE notes SET updated_at = '2099-01-01T00:00:00.000Z' WHERE id = ?1",
                [&note.id],
            )
            .expect("touch");
        let pending = database.pending_mcp_requests().expect("pending");
        assert!(pending[0].note_changed);

        database.trash_note(&note.id).expect("trash");
        let pending = database.pending_mcp_requests().expect("pending");
        assert!(pending[0].note_missing);
        assert!(database.approve_mcp_request(&request.id).is_err());
        let failed = database.mcp_request(&request.id).expect("request");
        assert_eq!(failed.status, McpRequestStatus::Failed);
        assert!(failed.error.is_some());
    }

    #[test]
    fn queue_is_limited_validated_and_expires() {
        let database = Database::in_memory().expect("db");
        let note = database
            .create_note("Titolo", &document("uno"))
            .expect("note");
        let invalid_document = json!({ "type": "nope" });
        assert!(
            database
                .queue_mcp_request(&replace(&note.id, &invalid_document))
                .is_err()
        );
        let proposed = document("due");
        for _ in 0..MAX_PENDING_REQUESTS {
            database
                .queue_mcp_request(&replace(&note.id, &proposed))
                .expect("queued");
        }
        assert!(matches!(
            database.queue_mcp_request(&replace(&note.id, &proposed)),
            Err(StorageError::TooManyMcpRequests(_))
        ));
        // Richieste di oltre una settimana fa: scadute.
        database
            .connection()
            .expect("connection")
            .execute(
                "UPDATE mcp_requests SET created_at = '2020-01-01T00:00:00.000Z'",
                [],
            )
            .expect("age");
        assert!(database.pending_mcp_requests().expect("pending").is_empty());
        let request = database
            .queue_mcp_request(&replace(&note.id, &proposed))
            .expect("queued after expiry");
        assert_eq!(request.status, McpRequestStatus::Pending);

        // Eliminare definitivamente la nota toglie anche le richieste.
        database.trash_note(&note.id).expect("trash");
        database
            .delete_note_permanently(
                &note.id,
                std::path::Path::new("/nonexistent-greatnote-test"),
            )
            .expect("delete");
        assert!(database.mcp_request(&request.id).is_err());
    }
}
