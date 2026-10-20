//! Indice semantico locale: vettori dei frammenti delle note in SQLite,
//! ricerca per similarità (coseno) con scansione completa. Per qualche
//! decina di migliaia di frammenti la scansione resta nell'ordine delle
//! decine di millisecondi; un indice approssimato potrà sostituirla senza
//! cambiare l'interfaccia.

use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{
    AiError,
    chunking::{Chunk, char_slice, chunk_text},
    provider::AiProvider,
    settings::AiSettings,
};
use crate::{
    content::to_plain_text,
    domain::note::Note,
    storage::{Database, StorageError, get_note},
};

/// Condizione SQL per le note utilizzabili dall'AI (alias `n`): attive, non
/// archiviate, fuori dai taccuini esclusi (elenco JSON nel parametro indicato).
pub(crate) fn usable_notes_condition(excluded_parameter: usize) -> String {
    format!(
        "n.deleted_at IS NULL AND n.archived_at IS NULL
         AND (n.notebook_id IS NULL
              OR n.notebook_id NOT IN (SELECT value FROM json_each(?{excluded_parameter})))"
    )
}

/// Testo di una nota come lo vede l'AI: titolo e testo semplice.
#[derive(Debug, Clone)]
pub struct NoteText {
    pub note_id: String,
    pub notebook_id: Option<String>,
    pub title: String,
    pub text: String,
    pub updated_at: String,
}

pub(crate) fn plain_text(content_json: &str) -> String {
    serde_json::from_str(content_json)
        .map(|document| to_plain_text(&document))
        .unwrap_or_default()
}

/// Colonna SQL (alias `n` per la nota) con il testo letto dalle immagini
/// allegate, nell'ordine degli allegati: `[nome]` e testo.
pub(crate) const IMAGE_TEXT_COLUMN: &str = "(SELECT group_concat(item, char(10) || char(10)) FROM (
         SELECT '[' || a.original_filename || ']' || char(10) || a.ocr_text AS item
         FROM attachments a
         WHERE a.note_id = n.id AND a.deleted_at IS NULL AND a.ocr_status = 'done'
           AND a.ocr_text IS NOT NULL AND a.ocr_text != ''
         ORDER BY a.created_at, a.id))";

/// Testo di una nota per l'AI: contenuto e, in coda, il testo delle
/// immagini allegate. Le posizioni dei frammenti si riferiscono a questo.
pub(crate) fn ai_text(content_json: &str, image_text: Option<String>) -> String {
    let text = plain_text(content_json);
    match image_text.filter(|images| !images.trim().is_empty()) {
        Some(images) if text.trim().is_empty() => format!("Testo nelle immagini:\n{images}"),
        Some(images) => format!("{text}\n\nTesto nelle immagini:\n{images}"),
        None => text,
    }
}

/// Rilegge una nota attiva (anche archiviata: il chiamante decide).
pub(crate) fn note_text(
    connection: &Connection,
    note_id: &str,
) -> Result<Option<(NoteText, bool)>, StorageError> {
    let row = connection
        .query_row(
            &format!(
                "SELECT n.id, n.notebook_id, n.title, n.content_json, n.updated_at,
                        n.archived_at IS NOT NULL, {IMAGE_TEXT_COLUMN}
                 FROM notes n WHERE n.id = ?1 AND n.deleted_at IS NULL"
            ),
            [note_id],
            |row| {
                Ok((
                    NoteText {
                        note_id: row.get(0)?,
                        notebook_id: row.get(1)?,
                        title: row.get(2)?,
                        text: ai_text(&row.get::<_, String>(3)?, row.get(6)?),
                        updated_at: row.get(4)?,
                    },
                    row.get::<_, bool>(5)?,
                ))
            },
        )
        .optional()?;
    Ok(row)
}

pub fn source_hash(title: &str, text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(title.as_bytes());
    hasher.update([0]);
    hasher.update(text.as_bytes());
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Testo inviato al modello di embedding per un frammento: il titolo dà
/// contesto ai frammenti centrali della nota.
pub fn embedding_input(title: &str, chunk: &str) -> String {
    if title.trim().is_empty() {
        chunk.to_owned()
    } else {
        format!("{}\n\n{chunk}", title.trim())
    }
}

fn normalized(vector: &[f32]) -> Option<Vec<f32>> {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    (norm.is_finite() && norm > 0.0).then(|| vector.iter().map(|value| value / norm).collect())
}

fn to_bytes(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

fn dot(bytes: &[u8], query: &[f32]) -> f32 {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .zip(query)
        .map(|(chunk, value)| f32::from_le_bytes(*chunk) * value)
        .sum()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AiIndexStatus {
    pub model: String,
    /// Note aggiornate nell'indice con il modello attuale.
    pub indexed_notes: u64,
    /// Note da (re)indicizzare: nuove, modificate o di un altro modello.
    pub pending_notes: u64,
    pub chunks: u64,
}

#[derive(Debug, Clone)]
pub struct PendingNote {
    pub note: NoteText,
    /// Impronta già indicizzata con lo stesso modello, se esiste.
    pub previous_hash: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SemanticHit {
    pub chunk: Chunk,
    pub chunk_index: usize,
    pub score: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SemanticResult {
    pub note: Note,
    /// Frammento più simile: spiega perché la nota è stata proposta.
    pub excerpt: String,
    pub score: f32,
}

impl Database {
    /// Elimina dall'indice ciò che non deve più esserci: altri modelli, note
    /// cestinate, archiviate o in taccuini esclusi. Senza modello di
    /// embedding svuota l'indice.
    pub fn ai_purge_index(&self, settings: &AiSettings) -> Result<u64, StorageError> {
        let connection = self.connection()?;
        let removed = if settings.embedding_model.is_empty() {
            connection.execute("DELETE FROM ai_index_notes", [])?
        } else {
            connection.execute(
                "DELETE FROM ai_index_notes
                 WHERE model != ?1
                    OR note_id IN (
                        SELECT n.id FROM notes n
                        WHERE n.deleted_at IS NOT NULL OR n.archived_at IS NOT NULL
                           OR n.notebook_id IN (SELECT value FROM json_each(?2)))",
                params![settings.embedding_model, settings.excluded_json()],
            )?
        };
        Ok(removed as u64)
    }

    pub fn ai_clear_index(&self) -> Result<u64, StorageError> {
        let connection = self.connection()?;
        Ok(connection.execute("DELETE FROM ai_index_notes", [])? as u64)
    }

    pub fn ai_index_status(&self, settings: &AiSettings) -> Result<AiIndexStatus, StorageError> {
        let connection = self.connection()?;
        let usable = usable_notes_condition(2);
        let (total, indexed, chunks): (i64, i64, i64) = connection.query_row(
            &format!(
                "SELECT count(*),
                        count(ai.note_id),
                        COALESCE(sum((SELECT count(*) FROM ai_chunks c WHERE c.note_id = ai.note_id)), 0)
                 FROM notes n
                 LEFT JOIN ai_index_notes ai
                   ON ai.note_id = n.id AND ai.model = ?1 AND ai.note_updated_at = n.updated_at
                 WHERE {usable}"
            ),
            params![settings.embedding_model, settings.excluded_json()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        Ok(AiIndexStatus {
            model: settings.embedding_model.clone(),
            indexed_notes: indexed.max(0) as u64,
            pending_notes: if settings.embedding_model.is_empty() {
                0
            } else {
                (total - indexed).max(0) as u64
            },
            chunks: chunks.max(0) as u64,
        })
    }

    /// Note da indicizzare, escluse quelle già tentate in questo ciclo.
    pub fn ai_pending_notes(
        &self,
        settings: &AiSettings,
        attempted: &[String],
        limit: u32,
    ) -> Result<Vec<PendingNote>, StorageError> {
        let connection = self.connection()?;
        let usable = usable_notes_condition(2);
        let attempted = serde_json::to_string(attempted)?;
        let mut statement = connection.prepare(&format!(
            "SELECT n.id, n.notebook_id, n.title, n.content_json, n.updated_at,
                    CASE WHEN ai.model = ?1 THEN ai.source_hash END, {IMAGE_TEXT_COLUMN}
             FROM notes n
             LEFT JOIN ai_index_notes ai ON ai.note_id = n.id
             WHERE {usable}
               AND (ai.note_id IS NULL OR ai.model != ?1 OR ai.note_updated_at != n.updated_at)
               AND n.id NOT IN (SELECT value FROM json_each(?3))
             ORDER BY n.updated_at DESC, n.id
             LIMIT ?4"
        ))?;
        let rows = statement.query_map(
            params![
                settings.embedding_model,
                settings.excluded_json(),
                attempted,
                limit
            ],
            |row| {
                Ok(PendingNote {
                    note: NoteText {
                        note_id: row.get(0)?,
                        notebook_id: row.get(1)?,
                        title: row.get(2)?,
                        text: ai_text(&row.get::<_, String>(3)?, row.get(6)?),
                        updated_at: row.get(4)?,
                    },
                    previous_hash: row.get(5)?,
                })
            },
        )?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Salva i vettori di una nota. Restituisce `false` (senza modifiche) se
    /// nel frattempo la nota è cambiata o non è più attiva.
    pub fn ai_store_note_index(
        &self,
        note: &NoteText,
        model: &str,
        source_hash: &str,
        chunks: &[Chunk],
        vectors: &[Vec<f32>],
    ) -> Result<bool, StorageError> {
        if chunks.len() != vectors.len() {
            return Err(StorageError::InvalidSetting(
                "frammenti e vettori non corrispondono",
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let current: Option<String> = transaction
            .query_row(
                "SELECT updated_at FROM notes WHERE id = ?1 AND deleted_at IS NULL",
                [&note.note_id],
                |row| row.get(0),
            )
            .optional()?;
        if current.as_deref() != Some(note.updated_at.as_str()) {
            return Ok(false);
        }
        transaction.execute(
            "DELETE FROM ai_index_notes WHERE note_id = ?1",
            [&note.note_id],
        )?;
        transaction.execute(
            "INSERT INTO ai_index_notes (note_id, model, note_updated_at, source_hash)
             VALUES (?1, ?2, ?3, ?4)",
            params![note.note_id, model, note.updated_at, source_hash],
        )?;
        {
            let mut insert = transaction.prepare(
                "INSERT INTO ai_chunks (note_id, chunk_index, char_start, char_end, dimensions, vector)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (index, (chunk, vector)) in chunks.iter().zip(vectors).enumerate() {
                let Some(vector) = normalized(vector) else {
                    continue;
                };
                insert.execute(params![
                    note.note_id,
                    index as i64,
                    chunk.start as i64,
                    chunk.end as i64,
                    vector.len() as i64,
                    to_bytes(&vector)
                ])?;
            }
        }
        transaction.commit()?;
        Ok(true)
    }

    /// Aggiorna solo la data di riferimento quando titolo e testo non sono
    /// cambiati (per esempio la nota è stata fissata o spostata).
    pub fn ai_mark_index_current(
        &self,
        note: &NoteText,
        model: &str,
        source_hash: &str,
    ) -> Result<bool, StorageError> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE ai_index_notes SET note_updated_at = ?3
             WHERE note_id = ?1 AND model = ?2 AND source_hash = ?4
               AND EXISTS (SELECT 1 FROM notes
                           WHERE id = ?1 AND updated_at = ?3 AND deleted_at IS NULL)",
            params![note.note_id, model, note.updated_at, source_hash],
        )?;
        Ok(changed == 1)
    }

    /// Frammenti più simili alla domanda, solo da note aggiornate
    /// nell'indice e utilizzabili dall'AI.
    pub fn ai_semantic_hits(
        &self,
        settings: &AiSettings,
        query: &[f32],
        limit: usize,
    ) -> Result<Vec<(String, SemanticHit)>, StorageError> {
        let Some(query) = normalized(query) else {
            return Ok(Vec::new());
        };
        let connection = self.connection()?;
        let usable = usable_notes_condition(2);
        let mut statement = connection.prepare(&format!(
            "SELECT c.note_id, c.chunk_index, c.char_start, c.char_end, c.vector
             FROM ai_chunks c
             JOIN ai_index_notes ai ON ai.note_id = c.note_id
             JOIN notes n ON n.id = c.note_id
             WHERE ai.model = ?1 AND ai.note_updated_at = n.updated_at AND c.dimensions = ?3
               AND {usable}"
        ))?;
        let mut rows = statement.query(params![
            settings.embedding_model,
            settings.excluded_json(),
            query.len() as i64
        ])?;
        let mut hits: Vec<(String, SemanticHit)> = Vec::new();
        while let Some(row) = rows.next()? {
            let bytes: Vec<u8> = row.get(4)?;
            let score = dot(&bytes, &query);
            hits.push((
                row.get(0)?,
                SemanticHit {
                    chunk_index: row.get::<_, i64>(1)? as usize,
                    chunk: Chunk {
                        start: row.get::<_, i64>(2)? as usize,
                        end: row.get::<_, i64>(3)? as usize,
                    },
                    score,
                },
            ));
            if hits.len() > limit * 4 {
                hits.sort_by(|a, b| b.1.score.total_cmp(&a.1.score));
                hits.truncate(limit);
            }
        }
        hits.sort_by(|a, b| b.1.score.total_cmp(&a.1.score));
        hits.truncate(limit);
        Ok(hits)
    }

    /// Ricerca per significato: una riga per nota con il frammento migliore.
    pub fn ai_semantic_search(
        &self,
        settings: &AiSettings,
        query: &[f32],
        limit: usize,
    ) -> Result<Vec<SemanticResult>, StorageError> {
        let hits = self.ai_semantic_hits(settings, query, limit * 5)?;
        let connection = self.connection()?;
        let mut results: Vec<SemanticResult> = Vec::new();
        for (note_id, hit) in hits {
            if results.len() >= limit || results.iter().any(|result| result.note.id == note_id) {
                continue;
            }
            let Some(note) = get_note(&connection, &note_id)? else {
                continue;
            };
            let text = note_text(&connection, &note_id)?
                .map(|(note, _)| note.text)
                .unwrap_or_default();
            let mut excerpt = char_slice(&text, hit.chunk.start, hit.chunk.end);
            if excerpt.chars().count() > 280 {
                excerpt = excerpt.chars().take(280).collect::<String>() + "…";
            }
            results.push(SemanticResult {
                note,
                excerpt,
                score: hit.score,
            });
        }
        Ok(results)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct IndexReport {
    pub indexed: u64,
    pub unchanged: u64,
    pub failed: u64,
    /// Ultimo errore su una singola nota, se l'indicizzazione è proseguita.
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct IndexProgress {
    pub done: u64,
    pub total: u64,
}

/// Indicizza le note nuove o modificate. Gli errori del server (rete,
/// modello assente, embedding non supportati) fermano il ciclo; quelli su
/// una singola nota (per esempio testo rifiutato) vengono contati e il ciclo
/// prosegue.
pub fn index_pending_notes(
    database: &Database,
    provider: &dyn AiProvider,
    settings: &AiSettings,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(IndexProgress),
) -> Result<IndexReport, AiError> {
    settings.ensure_usable()?;
    let model = settings.ensure_embedding_model()?.to_owned();
    database.ai_purge_index(settings)?;
    let total = database.ai_index_status(settings)?.pending_notes;
    let mut report = IndexReport::default();
    let mut attempted: Vec<String> = Vec::new();
    let mut done = 0;
    on_progress(IndexProgress { done, total });
    loop {
        let batch = database.ai_pending_notes(settings, &attempted, 20)?;
        if batch.is_empty() {
            break;
        }
        for pending in batch {
            if cancel.load(Ordering::Relaxed) {
                return Err(AiError::Cancelled);
            }
            attempted.push(pending.note.note_id.clone());
            let note = &pending.note;
            let hash = source_hash(&note.title, &note.text);
            if pending.previous_hash.as_deref() == Some(hash.as_str())
                && database.ai_mark_index_current(note, &model, &hash)?
            {
                report.unchanged += 1;
            } else {
                let chunks = chunk_text(&note.text);
                let inputs: Vec<String> = chunks
                    .iter()
                    .map(|chunk| {
                        embedding_input(
                            &note.title,
                            &char_slice(&note.text, chunk.start, chunk.end),
                        )
                    })
                    .collect();
                let vectors = if inputs.is_empty() {
                    Ok(Vec::new())
                } else {
                    provider.embed(&model, &inputs, cancel)
                };
                match vectors {
                    Ok(vectors) => {
                        if database.ai_store_note_index(note, &model, &hash, &chunks, &vectors)? {
                            report.indexed += 1;
                        }
                    }
                    // Testo rifiutato per questa nota (troppo lungo, non
                    // valido): si prosegue con le altre. Gli altri errori
                    // (server, modello, rete) valgono per tutte le note.
                    Err(
                        error @ (AiError::Http {
                            status: 400 | 413 | 422,
                            ..
                        }
                        | AiError::InvalidResponse(_)),
                    ) => {
                        tracing::warn!(note_id = %note.note_id, "nota non indicizzata: {error}");
                        report.failed += 1;
                        report.last_error = Some(error.to_string());
                    }
                    Err(error) => return Err(error),
                }
            }
            done += 1;
            on_progress(IndexProgress {
                done,
                total: total.max(done),
            });
        }
    }
    Ok(report)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;
    use crate::ai::provider::ChatMessage;

    /// Provider finto con embedding deterministici: una dimensione per parola
    /// chiave, così la similarità è prevedibile.
    pub(crate) struct KeywordEmbedder {
        pub calls: Mutex<usize>,
    }

    pub(crate) const KEYWORDS: [&str; 4] = ["gatto", "auto", "cucina", "budget"];

    impl AiProvider for KeywordEmbedder {
        fn list_models(&self) -> Result<Vec<String>, AiError> {
            Ok(vec!["finto".to_owned()])
        }

        fn provenance(&self, model: &str) -> crate::ai::provider::Provenance {
            crate::ai::provider::Provenance {
                host: "test".to_owned(),
                model: model.to_owned(),
                local: true,
            }
        }

        fn chat(
            &self,
            _model: &str,
            _messages: &[ChatMessage],
            _temperature: f32,
            _cancel: &AtomicBool,
            _on_delta: &mut dyn FnMut(&str),
        ) -> Result<String, AiError> {
            Ok(String::new())
        }

        fn embed(
            &self,
            _model: &str,
            inputs: &[String],
            _cancel: &AtomicBool,
        ) -> Result<Vec<Vec<f32>>, AiError> {
            *self.calls.lock().expect("lock") += 1;
            Ok(inputs
                .iter()
                .map(|input| {
                    let lower = input.to_lowercase();
                    let mut vector: Vec<f32> = KEYWORDS
                        .iter()
                        .map(|word| lower.matches(word).count() as f32)
                        .collect();
                    vector.push(0.01);
                    vector
                })
                .collect())
        }
    }

    pub(crate) fn document(text: &str) -> serde_json::Value {
        json!({"type": "doc", "content": [{"type": "paragraph", "content": [{"type": "text", "text": text}]}]})
    }

    pub(crate) fn settings() -> AiSettings {
        AiSettings {
            enabled: true,
            chat_model: "chat".to_owned(),
            embedding_model: "finto".to_owned(),
            ..AiSettings::default()
        }
    }

    fn query(embedder: &KeywordEmbedder, text: &str) -> Vec<f32> {
        embedder
            .embed("finto", &[text.to_owned()], &AtomicBool::new(false))
            .expect("query")
            .remove(0)
    }

    #[test]
    fn indexes_searches_and_follows_note_changes() {
        let database = Database::in_memory().expect("database");
        let cat = database
            .create_note("Animali", &document("Il gatto dorme sul divano"))
            .expect("note");
        let car = database
            .create_note("Garage", &document("Portare l'auto dal meccanico"))
            .expect("note");
        let embedder = KeywordEmbedder {
            calls: Mutex::new(0),
        };
        let settings = settings();
        let mut progress = Vec::new();
        let report = index_pending_notes(
            &database,
            &embedder,
            &settings,
            &AtomicBool::new(false),
            &mut |step| progress.push(step),
        )
        .expect("index");
        assert_eq!(report.indexed, 2);
        assert_eq!(progress.last(), Some(&IndexProgress { done: 2, total: 2 }));
        let status = database.ai_index_status(&settings).expect("status");
        assert_eq!((status.indexed_notes, status.pending_notes), (2, 0));

        let results = database
            .ai_semantic_search(&settings, &query(&embedder, "il mio gatto"), 5)
            .expect("search");
        assert_eq!(results[0].note.id, cat.id);
        assert!(results[0].excerpt.contains("gatto"));

        // Una nota modificata esce dai risultati finché non è reindicizzata.
        database
            .update_note(&car.id, "Garage", &document("Ricetta di cucina"))
            .expect("update");
        assert_eq!(
            database
                .ai_index_status(&settings)
                .expect("status")
                .pending_notes,
            1
        );
        assert!(
            database
                .ai_semantic_search(&settings, &query(&embedder, "cucina"), 5)
                .expect("search")
                .iter()
                .all(|result| result.note.id != car.id)
        );
        // Fissare la nota non ricalcola gli embedding.
        database.set_note_pinned(&cat.id, true).expect("pin");
        let calls_before = *embedder.calls.lock().expect("lock");
        let report = index_pending_notes(
            &database,
            &embedder,
            &settings,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .expect("reindex");
        assert_eq!((report.indexed, report.unchanged), (1, 1));
        assert_eq!(*embedder.calls.lock().expect("lock"), calls_before + 1);
        let results = database
            .ai_semantic_search(&settings, &query(&embedder, "cucina"), 5)
            .expect("search");
        assert_eq!(results[0].note.id, car.id);
    }

    #[test]
    fn excluded_trashed_and_other_model_notes_are_purged() {
        let database = Database::in_memory().expect("database");
        let notebook = database
            .create_notebook(None, "Privato", "")
            .expect("notebook");
        let private = database
            .create_note_in_notebook("Diario", &document("budget segreto"), Some(&notebook.id))
            .expect("note");
        let public = database
            .create_note("Spese", &document("budget di casa"))
            .expect("note");
        let embedder = KeywordEmbedder {
            calls: Mutex::new(0),
        };
        let mut settings = settings();
        let cancel = AtomicBool::new(false);
        index_pending_notes(&database, &embedder, &settings, &cancel, &mut |_| {}).expect("index");
        assert_eq!(
            database
                .ai_index_status(&settings)
                .expect("s")
                .indexed_notes,
            2
        );

        settings.excluded_notebook_ids = vec![notebook.id.clone()];
        let budget = query(&embedder, "budget");
        let ids: Vec<String> = database
            .ai_semantic_search(&settings, &budget, 5)
            .expect("search")
            .into_iter()
            .map(|result| result.note.id)
            .collect();
        assert_eq!(ids, vec![public.id.clone()]);
        assert_eq!(database.ai_purge_index(&settings).expect("purge"), 1);
        assert!(note_indexed(&database, &public.id));
        assert!(!note_indexed(&database, &private.id));

        database.trash_note(&public.id).expect("trash");
        database.ai_purge_index(&settings).expect("purge");
        assert!(!note_indexed(&database, &public.id));

        settings.embedding_model = "altro".to_owned();
        database.ai_purge_index(&settings).expect("purge");
        assert_eq!(database.ai_clear_index().expect("clear"), 0);
    }

    fn note_indexed(database: &Database, note_id: &str) -> bool {
        database
            .connection()
            .expect("lock")
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM ai_index_notes WHERE note_id = ?1)",
                [note_id],
                |row| row.get(0),
            )
            .expect("query")
    }

    #[test]
    fn permanent_deletion_removes_vectors() {
        let directory = tempfile::tempdir().expect("directory");
        let database = Database::open(directory.path().join("gna.sqlite3")).expect("database");
        let note = database
            .create_note("Auto", &document("auto nuova"))
            .expect("note");
        let embedder = KeywordEmbedder {
            calls: Mutex::new(0),
        };
        index_pending_notes(
            &database,
            &embedder,
            &settings(),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .expect("index");
        database.trash_note(&note.id).expect("trash");
        database
            .delete_note_permanently(&note.id, directory.path())
            .expect("delete");
        let chunks: i64 = database
            .connection()
            .expect("lock")
            .query_row("SELECT count(*) FROM ai_chunks", [], |row| row.get(0))
            .expect("count");
        assert_eq!(chunks, 0);
    }

    #[test]
    fn server_errors_stop_indexing_but_rejected_notes_do_not() {
        use crate::ai::provider::{OpenAiCompatible, fake_server::respond_with};
        use crate::ai::settings::Endpoint;

        let database = Database::in_memory().expect("database");
        for title in ["Uno", "Due"] {
            database
                .create_note(title, &document("testo da indicizzare"))
                .expect("note");
        }
        let provider = |responses| {
            let server = respond_with(responses);
            OpenAiCompatible::new(
                Endpoint::parse(&server.base_url).expect("endpoint"),
                None,
                std::time::Duration::from_secs(5),
            )
        };
        let cancel = AtomicBool::new(false);
        let unsupported = provider(vec![(
            501,
            "application/json",
            r#"{"error":{"message":"embeddings not supported"}}"#.to_owned(),
        )]);
        assert!(matches!(
            index_pending_notes(&database, &unsupported, &settings(), &cancel, &mut |_| {}),
            Err(AiError::Http { status: 501, .. })
        ));
        let rejected = provider(vec![
            (
                400,
                "application/json",
                r#"{"error":"input too long"}"#.to_owned(),
            ),
            (
                200,
                "application/json",
                r#"{"data":[{"index":0,"embedding":[1.0,0.0]}]}"#.to_owned(),
            ),
        ]);
        let report = index_pending_notes(&database, &rejected, &settings(), &cancel, &mut |_| {})
            .expect("indexing continues");
        assert_eq!((report.indexed, report.failed), (1, 1));
        assert!(report.last_error.expect("error").contains("input too long"));
    }

    #[test]
    fn disabled_or_unconfigured_indexing_is_refused() {
        let database = Database::in_memory().expect("database");
        let embedder = KeywordEmbedder {
            calls: Mutex::new(0),
        };
        let cancel = AtomicBool::new(false);
        let disabled = AiSettings::default();
        assert!(matches!(
            index_pending_notes(&database, &embedder, &disabled, &cancel, &mut |_| {}),
            Err(AiError::Disabled)
        ));
        let no_model = AiSettings {
            embedding_model: String::new(),
            ..settings()
        };
        assert!(matches!(
            index_pending_notes(&database, &embedder, &no_model, &cancel, &mut |_| {}),
            Err(AiError::NotConfigured(_))
        ));
    }
}
