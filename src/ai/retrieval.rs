//! Recupero delle note pertinenti a una domanda. Con un modello di embedding
//! usa l'indice semantico; altrimenti (o se l'indice è vuoto) la ricerca
//! testuale FTS5. Le fonti hanno sempre un riferimento verificabile (nota e
//! posizione nel testo) e il loro testo viene riletto dalla nota corrente
//! prima dell'invio: il frontend passa solo riferimenti.

use std::collections::HashSet;

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::{
    AiError,
    chunking::{Chunk, MAX_CHUNK_CHARS, char_slice, chunk_text},
    index::{IMAGE_TEXT_COLUMN, NoteText, ai_text, note_text, usable_notes_condition},
    planner_context::MAX_PLANNER_SOURCES,
    prompt::Language,
    settings::AiSettings,
};
use crate::{
    storage::{Database, StorageError},
    time::Zone,
};

/// Frammenti per nota nel contesto di una risposta.
const MAX_CHUNKS_PER_SOURCE_NOTE: usize = 2;
/// Fonti al massimo in una risposta.
pub const MAX_SOURCES: usize = 8;
const MAX_KEYWORD_TERMS: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RetrievalMethod {
    /// Similarità con l'indice semantico.
    Semantic,
    /// Parole in comune (ricerca testuale).
    Keyword,
    /// Nota scelta dall'utente.
    Selected,
    /// Attività, evento o promemoria nel periodo nominato dalla domanda.
    Date,
}

/// Che cosa è una fonte.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    #[default]
    Note,
    Task,
    Event,
    /// Promemoria di una nota (quelli di attività ed eventi compaiono nella
    /// fonte dell'attività o dell'evento).
    Reminder,
}

/// Elemento che l'interfaccia apre da una citazione.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceTarget {
    pub kind: SourceKind,
    pub id: String,
}

impl SourceTarget {
    pub fn note(id: &str) -> Self {
        Self {
            kind: SourceKind::Note,
            id: id.to_owned(),
        }
    }

    pub fn task(id: &str) -> Self {
        Self {
            kind: SourceKind::Task,
            id: id.to_owned(),
        }
    }

    pub fn event(id: &str) -> Self {
        Self {
            kind: SourceKind::Event,
            id: id.to_owned(),
        }
    }
}

/// Riferimento a un frammento di nota o a un elemento del planner: è ciò
/// che il frontend restituisce quando l'utente conferma l'invio.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRef {
    #[serde(default)]
    pub kind: SourceKind,
    #[serde(default)]
    pub note_id: String,
    /// Id dell'attività, dell'evento o del promemoria.
    #[serde(default)]
    pub item_id: String,
    #[serde(default)]
    pub char_start: usize,
    #[serde(default)]
    pub char_end: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ContextSource {
    /// Numero usato nelle citazioni `[n]`.
    pub number: usize,
    pub kind: SourceKind,
    /// Nota del frammento, o nota collegata all'elemento del planner (vuoto
    /// se non c'è).
    pub note_id: String,
    /// Id dell'attività, dell'evento o del promemoria; vuoto per le note.
    pub item_id: String,
    pub title: String,
    pub char_start: usize,
    pub char_end: usize,
    pub text: String,
    pub method: RetrievalMethod,
    pub score: Option<f32>,
    pub open: SourceTarget,
}

impl ContextSource {
    pub fn reference(&self) -> SourceRef {
        SourceRef {
            kind: self.kind,
            note_id: self.note_id.clone(),
            item_id: self.item_id.clone(),
            char_start: self.char_start,
            char_end: self.char_end,
        }
    }
}

/// Parole significative della domanda per la ricerca testuale.
pub fn keyword_terms(question: &str) -> Vec<String> {
    const STOPWORDS: &[&str] = &[
        "che", "chi", "con", "come", "cosa", "del", "della", "delle", "dei", "degli", "dove",
        "per", "sono", "una", "uno", "nel", "nella", "nelle", "sul", "sulla", "tra", "fra",
        "quale", "quali", "quando", "quanto", "mio", "mia", "miei", "mie", "hai", "ho", "abbiamo",
        "the", "and", "for", "with", "what", "which", "when", "where", "who", "how", "are", "was",
        "were", "this", "that", "from", "about", "have", "has", "did", "does", "my", "our", "your",
        "note", "notes",
    ];
    let mut seen = HashSet::new();
    question
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|word| word.chars().count() >= 3 && !STOPWORDS.contains(&word.as_str()))
        .filter(|word| seen.insert(word.clone()))
        .take(MAX_KEYWORD_TERMS)
        .collect()
}

/// Espressione FTS5 sicura: ogni termine è alfanumerico, tra virgolette e
/// con ricerca per prefisso; i termini sono in OR. Viene passata come
/// parametro, mai concatenata nell'SQL.
fn fts_expression(terms: &[String]) -> Option<String> {
    let parts: Vec<String> = terms
        .iter()
        .filter(|term| term.chars().all(char::is_alphanumeric))
        .map(|term| format!("\"{term}\"*"))
        .collect();
    (!parts.is_empty()).then(|| parts.join(" OR "))
}

fn term_score(text: &str, terms: &[String]) -> usize {
    let lower = text.to_lowercase();
    terms
        .iter()
        .map(|term| lower.matches(term.as_str()).count())
        .sum()
}

pub(crate) struct Budget {
    remaining: usize,
}

impl Budget {
    pub(crate) fn new(remaining: usize) -> Self {
        Self { remaining }
    }

    /// Accorcia il testo al budget rimasto; `None` se il budget è esaurito.
    pub(crate) fn take(&mut self, text: String) -> Option<String> {
        if self.remaining < 200 {
            return None;
        }
        let length = text.chars().count();
        let text = if length > self.remaining {
            text.chars().take(self.remaining).collect::<String>() + "…"
        } else {
            text
        };
        self.remaining = self.remaining.saturating_sub(length);
        Some(text)
    }
}

impl Database {
    /// Note candidate per parole chiave (FTS5, ordinate per pertinenza).
    fn ai_keyword_notes(
        &self,
        settings: &AiSettings,
        terms: &[String],
        limit: u32,
    ) -> Result<Vec<NoteText>, StorageError> {
        let Some(expression) = fts_expression(terms) else {
            return Ok(Vec::new());
        };
        let connection = self.connection()?;
        let usable = usable_notes_condition(2);
        let mut statement = connection.prepare(&format!(
            "SELECT n.id, n.notebook_id, n.title, n.content_json, n.updated_at, {IMAGE_TEXT_COLUMN}
             FROM note_search JOIN notes n ON n.id = note_search.note_id
             WHERE note_search MATCH ?1 AND {usable}
             ORDER BY bm25(note_search), n.updated_at DESC
             LIMIT ?3"
        ))?;
        let rows = statement.query_map(
            params![expression, settings.excluded_json(), limit],
            |row| {
                Ok(NoteText {
                    note_id: row.get(0)?,
                    notebook_id: row.get(1)?,
                    title: row.get(2)?,
                    text: ai_text(&row.get::<_, String>(3)?, row.get(5)?),
                    updated_at: row.get(4)?,
                })
            },
        )?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Fonti per una domanda. `query_vector` è l'embedding della domanda,
    /// se la ricerca semantica è configurata.
    pub fn ai_retrieve(
        &self,
        settings: &AiSettings,
        question: &str,
        query_vector: Option<&[f32]>,
    ) -> Result<Vec<ContextSource>, StorageError> {
        let mut budget = Budget {
            remaining: settings.max_context_chars as usize,
        };
        let mut sources: Vec<ContextSource> = Vec::new();
        if let Some(vector) = query_vector {
            let hits = self.ai_semantic_hits(settings, vector, MAX_SOURCES * 3)?;
            let connection = self.connection()?;
            for (note_id, hit) in hits {
                if sources.len() >= MAX_SOURCES {
                    break;
                }
                let per_note = sources.iter().filter(|s| s.note_id == note_id).count();
                if per_note >= MAX_CHUNKS_PER_SOURCE_NOTE {
                    continue;
                }
                let Some((note, archived)) = note_text(&connection, &note_id)? else {
                    continue;
                };
                if archived || settings.is_excluded(note.notebook_id.as_deref()) {
                    continue;
                }
                let Some(text) =
                    budget.take(char_slice(&note.text, hit.chunk.start, hit.chunk.end))
                else {
                    break;
                };
                sources.push(source(
                    &note,
                    hit.chunk,
                    text,
                    RetrievalMethod::Semantic,
                    Some(hit.score),
                ));
            }
        }
        if sources.is_empty() {
            let terms = keyword_terms(question);
            for note in self.ai_keyword_notes(settings, &terms, MAX_SOURCES as u32)? {
                let mut chunks = chunk_text(&note.text);
                if chunks.is_empty() {
                    continue;
                }
                chunks.sort_by_key(|chunk| {
                    std::cmp::Reverse(term_score(
                        &char_slice(&note.text, chunk.start, chunk.end),
                        &terms,
                    ))
                });
                for chunk in chunks.into_iter().take(MAX_CHUNKS_PER_SOURCE_NOTE) {
                    let text = char_slice(&note.text, chunk.start, chunk.end);
                    if sources.len() >= MAX_SOURCES
                        || (term_score(&text, &terms) == 0
                            && sources.iter().any(|s| s.note_id == note.note_id))
                    {
                        break;
                    }
                    let Some(text) = budget.take(text) else {
                        break;
                    };
                    sources.push(source(&note, chunk, text, RetrievalMethod::Keyword, None));
                }
            }
        }
        renumber(&mut sources);
        Ok(sources)
    }

    /// Una nota scelta dall'utente come contesto (es. "chiedi su questa
    /// nota"), accorciata al budget.
    pub fn ai_note_context(
        &self,
        settings: &AiSettings,
        note_id: &str,
    ) -> Result<Vec<ContextSource>, AiError> {
        let connection = self.connection()?;
        let (note, _) = note_text(&connection, note_id)?.ok_or(StorageError::NoteNotFound)?;
        if settings.is_excluded(note.notebook_id.as_deref()) {
            return Err(AiError::ExcludedNote);
        }
        let mut budget = Budget {
            remaining: settings.max_context_chars as usize,
        };
        let length = note.text.chars().count();
        let text = budget.take(note.text.clone()).unwrap_or_default();
        Ok(vec![ContextSource {
            number: 1,
            kind: SourceKind::Note,
            note_id: note.note_id.clone(),
            item_id: String::new(),
            title: note.title.clone(),
            char_start: 0,
            char_end: length.max(1),
            text,
            method: RetrievalMethod::Selected,
            score: None,
            open: SourceTarget::note(&note.note_id),
        }])
    }

    /// Rilegge dal database le fonti confermate dall'utente. I riferimenti
    /// a note o elementi cestinati, esclusi o inesistenti sono ignorati; il
    /// testo segue la versione corrente. Gli elementi del planner sono
    /// descritti nel fuso `zone` e nella lingua `language`.
    pub fn ai_resolve_sources(
        &self,
        settings: &AiSettings,
        references: &[SourceRef],
        zone: &Zone,
        language: Language,
    ) -> Result<Vec<ContextSource>, StorageError> {
        let connection = self.connection()?;
        let mut budget = Budget {
            remaining: settings.max_context_chars as usize,
        };
        let mut sources = Vec::new();
        let (mut notes, mut planner) = (0, 0);
        for reference in references {
            if reference.kind != SourceKind::Note {
                if planner >= MAX_PLANNER_SOURCES {
                    continue;
                }
                planner += 1;
                let Some(mut source) = Database::ai_resolve_planner_source(
                    &connection,
                    settings,
                    reference,
                    zone,
                    language,
                )?
                else {
                    continue;
                };
                let Some(text) = budget.take(source.text) else {
                    break;
                };
                source.text = text;
                sources.push(source);
                continue;
            }
            if notes >= MAX_SOURCES {
                continue;
            }
            notes += 1;
            let Some((note, _)) = note_text(&connection, &reference.note_id)? else {
                continue;
            };
            if settings.is_excluded(note.notebook_id.as_deref())
                || reference.char_end <= reference.char_start
            {
                continue;
            }
            // I frammenti recuperati sono al massimo lunghi un chunk; una nota
            // scelta per intero può arrivare al budget.
            let end = reference
                .char_end
                .min(reference.char_start + settings.max_context_chars as usize + MAX_CHUNK_CHARS);
            let text = char_slice(&note.text, reference.char_start, end);
            if text.trim().is_empty() {
                continue;
            }
            let Some(text) = budget.take(text) else {
                break;
            };
            sources.push(ContextSource {
                number: 0,
                kind: SourceKind::Note,
                note_id: note.note_id.clone(),
                item_id: String::new(),
                title: note.title.clone(),
                char_start: reference.char_start,
                char_end: end,
                text,
                method: RetrievalMethod::Selected,
                score: None,
                open: SourceTarget::note(&note.note_id),
            });
        }
        renumber(&mut sources);
        Ok(sources)
    }
}

fn source(
    note: &NoteText,
    chunk: Chunk,
    text: String,
    method: RetrievalMethod,
    score: Option<f32>,
) -> ContextSource {
    ContextSource {
        number: 0,
        kind: SourceKind::Note,
        note_id: note.note_id.clone(),
        item_id: String::new(),
        title: note.title.clone(),
        char_start: chunk.start,
        char_end: chunk.end,
        text,
        method,
        score,
        open: SourceTarget::note(&note.note_id),
    }
}

/// Numera le fonti nell'ordine dato, a partire da 1.
pub fn renumber(sources: &mut [ContextSource]) {
    for (index, source) in sources.iter_mut().enumerate() {
        source.number = index + 1;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, atomic::AtomicBool};

    use super::*;
    use crate::ai::{
        index::{
            index_pending_notes,
            tests::{KeywordEmbedder, document, settings},
        },
        provider::AiProvider,
    };

    #[test]
    fn keyword_terms_skip_stopwords_and_symbols() {
        assert_eq!(
            keyword_terms("Cosa ho scritto sul \"budget\" 2026 per l'AUTO? OR NOT*"),
            vec!["scritto", "budget", "2026", "auto", "not"]
        );
        assert_eq!(
            fts_expression(&keyword_terms("budget auto")).as_deref(),
            Some("\"budget\"* OR \"auto\"*")
        );
        assert!(fts_expression(&[]).is_none());
    }

    #[test]
    fn keyword_retrieval_finds_relevant_notes_and_respects_exclusions() {
        let database = Database::in_memory().expect("database");
        let notebook = database.create_notebook(None, "Privato", "").expect("nb");
        let public = database
            .create_note(
                "Bilancio",
                &document("Il budget per la cucina è di 500 euro"),
            )
            .expect("note");
        database
            .create_note_in_notebook("Diario", &document("budget personale"), Some(&notebook.id))
            .expect("note");
        let archived = database
            .create_note("Vecchio", &document("budget del 2020"))
            .expect("note");
        database
            .set_note_archived(&archived.id, true)
            .expect("archive");
        let mut settings = settings();
        settings.embedding_model.clear();
        settings.excluded_notebook_ids = vec![notebook.id];
        let sources = database
            .ai_retrieve(&settings, "Qual è il budget della cucina?", None)
            .expect("retrieve");
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].note_id, public.id);
        assert_eq!(sources[0].number, 1);
        assert_eq!(sources[0].method, RetrievalMethod::Keyword);
        assert!(sources[0].text.contains("500 euro"));
        // Domande senza parole utili non producono fonti né errori FTS.
        assert!(
            database
                .ai_retrieve(&settings, "\"*(", None)
                .expect("ok")
                .is_empty()
        );
    }

    #[test]
    fn semantic_retrieval_and_resolution_reread_current_text() {
        let database = Database::in_memory().expect("database");
        let note = database
            .create_note("Gatti", &document("Il gatto mangia alle otto"))
            .expect("note");
        database
            .create_note("Auto", &document("Tagliando auto a maggio"))
            .expect("note");
        let embedder = KeywordEmbedder {
            calls: Mutex::new(0),
        };
        let settings = settings();
        let cancel = AtomicBool::new(false);
        index_pending_notes(&database, &embedder, &settings, &cancel, &mut |_| {}).expect("index");
        let vector = embedder
            .embed("finto", &["quando mangia il gatto".to_owned()], &cancel)
            .expect("vector")
            .remove(0);
        let sources = database
            .ai_retrieve(&settings, "quando mangia il gatto", Some(&vector))
            .expect("retrieve");
        assert_eq!(sources[0].note_id, note.id);
        assert_eq!(sources[0].method, RetrievalMethod::Semantic);

        // Il testo inviato è quello attuale della nota, non quello mostrato.
        let reference = sources[0].reference();
        database
            .update_note(&note.id, "Gatti", &document("Il gatto mangia alle nove"))
            .expect("update");
        let resolved = database
            .ai_resolve_sources(
                &settings,
                std::slice::from_ref(&reference),
                &Zone::system(),
                Language::It,
            )
            .expect("resolve");
        assert!(resolved[0].text.contains("nove"));

        database.trash_note(&note.id).expect("trash");
        assert!(
            database
                .ai_resolve_sources(&settings, &[reference], &Zone::system(), Language::It)
                .expect("resolve")
                .is_empty()
        );
    }

    #[test]
    fn context_respects_the_character_budget() {
        let database = Database::in_memory().expect("database");
        let long = "parola ".repeat(3_000);
        let note = database
            .create_note("Lunga", &document(&long))
            .expect("note");
        let mut settings = settings();
        settings.max_context_chars = 2_000;
        let context = database
            .ai_note_context(&settings, &note.id)
            .expect("context");
        assert!(context[0].text.chars().count() <= 2_001);
        let excluded = AiSettings {
            excluded_notebook_ids: vec!["nb".to_owned()],
            ..settings
        };
        let notebook_note = database
            .create_note_in_notebook("X", &document("y"), None)
            .expect("note");
        assert!(
            database
                .ai_note_context(&excluded, &notebook_note.id)
                .is_ok()
        );
    }
}
