//! Cronologia locale delle richieste AI: domanda o azione, risposta,
//! modello e server usati, fonti citate (solo riferimento e titolo). Si attiva o
//! disattiva dalle impostazioni e si può cancellare in ogni momento; resta
//! sul dispositivo come il resto del database.

use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::{
    provider::Provenance,
    retrieval::{ContextSource, SourceKind, SourceTarget},
};
use crate::storage::{Database, StorageError};

/// Voci conservate: oltre questo limite le più vecchie vengono eliminate.
pub const MAX_HISTORY_ENTRIES: u32 = 1_000;
const MAX_PROMPT_CHARS: usize = 4_000;
const MAX_RESPONSE_CHARS: usize = 100_000;
const MAX_ERROR_CHARS: usize = 1_000;
const MAX_PAGE: u32 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HistoryKind {
    Chat,
    Transform,
    Tags,
    Tasks,
    Search,
}

impl HistoryKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Transform => "transform",
            Self::Tags => "tags",
            Self::Tasks => "tasks",
            Self::Search => "search",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "transform" => Self::Transform,
            "tags" => Self::Tags,
            "tasks" => Self::Tasks,
            "search" => Self::Search,
            _ => Self::Chat,
        }
    }
}

/// Fonte citata in una risposta: senza il testo inviato.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistorySource {
    pub number: usize,
    /// Le voci salvate prima delle fonti del planner sono note.
    #[serde(default)]
    pub kind: SourceKind,
    pub note_id: String,
    #[serde(default)]
    pub item_id: String,
    pub title: String,
    /// Elemento da aprire; assente nelle voci meno recenti (la nota).
    #[serde(default)]
    pub open: Option<SourceTarget>,
}

impl From<&ContextSource> for HistorySource {
    fn from(source: &ContextSource) -> Self {
        Self {
            number: source.number,
            kind: source.kind,
            note_id: source.note_id.clone(),
            item_id: source.item_id.clone(),
            title: source.title.clone(),
            open: Some(source.open.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HistoryEntry {
    pub id: String,
    pub kind: HistoryKind,
    pub conversation_id: Option<String>,
    pub note_id: Option<String>,
    pub prompt: String,
    pub response: String,
    pub sources: Vec<HistorySource>,
    pub model: String,
    pub host: String,
    pub local: bool,
    pub error: Option<String>,
    pub created_at: String,
}

/// Dati di una nuova voce.
#[derive(Debug, Clone)]
pub struct NewHistoryEntry<'a> {
    pub kind: HistoryKind,
    pub conversation_id: Option<&'a str>,
    pub note_id: Option<&'a str>,
    pub prompt: &'a str,
    pub response: &'a str,
    pub sources: Vec<HistorySource>,
    pub provenance: &'a Provenance,
    pub error: Option<&'a str>,
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        text.chars().take(max.saturating_sub(1)).collect::<String>() + "…"
    }
}

fn valid_conversation(id: Option<&str>) -> Option<String> {
    id.filter(|id| {
        !id.is_empty()
            && id.len() <= 64
            && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
    .map(str::to_owned)
}

fn entry_from_row(row: &Row<'_>) -> rusqlite::Result<HistoryEntry> {
    let sources: String = row.get(6)?;
    Ok(HistoryEntry {
        id: row.get(0)?,
        kind: HistoryKind::parse(&row.get::<_, String>(1)?),
        conversation_id: row.get(2)?,
        note_id: row.get(3)?,
        prompt: row.get(4)?,
        response: row.get(5)?,
        sources: serde_json::from_str(&sources).unwrap_or_default(),
        model: row.get(7)?,
        host: row.get(8)?,
        local: row.get(9)?,
        error: row.get(10)?,
        created_at: row.get(11)?,
    })
}

const COLUMNS: &str =
    "id, kind, conversation_id, note_id, prompt, response, sources_json, model, host,
                       local, error, created_at";

/// `%` e `_` nel testo cercato non sono caratteri jolly.
fn like_pattern(query: &str) -> String {
    let escaped = query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

impl Database {
    /// Registra una richiesta e mantiene al più [`MAX_HISTORY_ENTRIES`] voci.
    pub fn ai_history_add(&self, entry: NewHistoryEntry<'_>) -> Result<String, StorageError> {
        let id = Uuid::new_v4().to_string();
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        // La nota può non esistere più (eliminata nel frattempo).
        let note_id = match entry.note_id {
            Some(note_id) => transaction
                .query_row("SELECT id FROM notes WHERE id = ?1", [note_id], |row| {
                    row.get::<_, String>(0)
                })
                .optional()?,
            None => None,
        };
        transaction.execute(
            "INSERT INTO ai_history (id, kind, conversation_id, note_id, prompt, response,
                                     sources_json, model, host, local, error)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                id,
                entry.kind.as_str(),
                valid_conversation(entry.conversation_id),
                note_id,
                clip(entry.prompt.trim(), MAX_PROMPT_CHARS),
                clip(entry.response, MAX_RESPONSE_CHARS),
                serde_json::to_string(&entry.sources)?,
                clip(&entry.provenance.model, 200),
                clip(&entry.provenance.host, 300),
                entry.provenance.local,
                entry.error.map(|error| clip(error, MAX_ERROR_CHARS)),
            ],
        )?;
        transaction.execute(
            "DELETE FROM ai_history WHERE id NOT IN (
                 SELECT id FROM ai_history ORDER BY created_at DESC, rowid DESC LIMIT ?1)",
            [MAX_HISTORY_ENTRIES],
        )?;
        transaction.commit()?;
        Ok(id)
    }

    /// Voci dalla più recente, filtrate per testo di domanda o risposta.
    pub fn ai_history_list(
        &self,
        query: Option<&str>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<HistoryEntry>, StorageError> {
        if limit == 0 || limit > MAX_PAGE {
            return Err(StorageError::InvalidPage);
        }
        let connection = self.connection()?;
        let query = query.map(str::trim).filter(|query| !query.is_empty());
        let mut statement = connection.prepare(&format!(
            "SELECT {COLUMNS} FROM ai_history
             WHERE ?1 IS NULL
                OR prompt LIKE ?1 ESCAPE '\\' OR response LIKE ?1 ESCAPE '\\'
             ORDER BY created_at DESC, rowid DESC
             LIMIT ?2 OFFSET ?3"
        ))?;
        let rows = statement.query_map(
            params![query.map(like_pattern), limit, offset],
            entry_from_row,
        )?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Tutti gli scambi di una conversazione, in ordine cronologico.
    pub fn ai_history_conversation(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<HistoryEntry>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {COLUMNS} FROM ai_history
             WHERE conversation_id = ?1 AND kind = 'chat'
             ORDER BY created_at, rowid
             LIMIT 200"
        ))?;
        let rows = statement.query_map([conversation_id], entry_from_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn ai_history_delete(&self, id: &str) -> Result<bool, StorageError> {
        let connection = self.connection()?;
        Ok(connection.execute("DELETE FROM ai_history WHERE id = ?1", [id])? > 0)
    }

    pub fn ai_history_clear(&self) -> Result<u64, StorageError> {
        let connection = self.connection()?;
        Ok(connection.execute("DELETE FROM ai_history", [])? as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provenance() -> Provenance {
        Provenance {
            host: "localhost".to_owned(),
            model: "modello".to_owned(),
            local: true,
        }
    }

    fn entry<'a>(prompt: &'a str, provenance: &'a Provenance) -> NewHistoryEntry<'a> {
        NewHistoryEntry {
            kind: HistoryKind::Chat,
            conversation_id: Some("conv-1"),
            note_id: None,
            prompt,
            response: "risposta [1]",
            sources: vec![HistorySource {
                number: 1,
                kind: SourceKind::Note,
                note_id: "n".to_owned(),
                item_id: String::new(),
                title: "Nota".to_owned(),
                open: Some(SourceTarget::note("n")),
            }],
            provenance,
            error: None,
        }
    }

    #[test]
    fn entries_are_listed_searched_grouped_and_deleted() {
        let database = Database::in_memory().expect("database");
        let provenance = provenance();
        let first = database
            .ai_history_add(entry("Prima domanda al 100%", &provenance))
            .expect("first");
        database
            .ai_history_add(entry("Seconda domanda", &provenance))
            .expect("second");
        database
            .ai_history_add(NewHistoryEntry {
                kind: HistoryKind::Transform,
                conversation_id: Some("conv-1"),
                error: Some("server non raggiungibile"),
                ..entry("Riassumi", &provenance)
            })
            .expect("transform");

        let all = database.ai_history_list(None, 50, 0).expect("list");
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].kind, HistoryKind::Transform);
        assert_eq!(all[2].sources[0].title, "Nota");
        assert_eq!(
            database
                .ai_history_list(Some("100%"), 50, 0)
                .expect("search")
                .len(),
            1
        );
        assert!(
            database
                .ai_history_list(Some("_"), 50, 0)
                .expect("literal")
                .is_empty()
        );
        let conversation = database
            .ai_history_conversation("conv-1")
            .expect("conversation");
        assert_eq!(conversation.len(), 2, "solo gli scambi di chat");
        assert_eq!(conversation[0].prompt, "Prima domanda al 100%");

        assert!(database.ai_history_delete(&first).expect("delete"));
        assert_eq!(database.ai_history_clear().expect("clear"), 2);
        assert!(
            database
                .ai_history_list(None, 50, 0)
                .expect("empty")
                .is_empty()
        );
        assert!(database.ai_history_list(None, 0, 0).is_err());
    }

    #[test]
    fn long_texts_are_clipped_and_old_entries_pruned() {
        let database = Database::in_memory().expect("database");
        let provenance = provenance();
        let long = "x".repeat(MAX_PROMPT_CHARS + 50);
        database
            .ai_history_add(NewHistoryEntry {
                note_id: Some("nota-inesistente"),
                conversation_id: Some("non valido!"),
                ..entry(&long, &provenance)
            })
            .expect("add");
        let stored = &database.ai_history_list(None, 1, 0).expect("list")[0];
        assert_eq!(stored.prompt.chars().count(), MAX_PROMPT_CHARS);
        assert!(stored.note_id.is_none());
        assert!(stored.conversation_id.is_none());

        for index in 0..(MAX_HISTORY_ENTRIES + 5) {
            database
                .ai_history_add(entry(&format!("domanda {index}"), &provenance))
                .expect("add");
        }
        let connection = database.connection().expect("lock");
        let count: i64 = connection
            .query_row("SELECT count(*) FROM ai_history", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, i64::from(MAX_HISTORY_ENTRIES));
    }

    #[test]
    fn deleting_a_note_keeps_its_history_without_the_link() {
        let directory = tempfile::tempdir().expect("directory");
        let database = Database::open(directory.path().join("gna.sqlite3")).expect("database");
        let note = database
            .create_note(
                "Nota",
                &serde_json::json!({"type": "doc", "content": [{"type": "paragraph"}]}),
            )
            .expect("note");
        let provenance = provenance();
        database
            .ai_history_add(NewHistoryEntry {
                note_id: Some(&note.id),
                ..entry("Riassumi", &provenance)
            })
            .expect("add");
        database.trash_note(&note.id).expect("trash");
        database
            .delete_note_permanently(&note.id, directory.path())
            .expect("delete");
        let stored = database.ai_history_list(None, 10, 0).expect("list");
        assert_eq!(stored.len(), 1);
        assert!(stored[0].note_id.is_none());
    }
}
