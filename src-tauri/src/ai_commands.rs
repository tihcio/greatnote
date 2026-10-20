//! Comandi delle funzioni AI (ADR 0010). Le chiamate di rete girano fuori
//! dal thread principale; ogni richiesta ha un identificativo per
//! l'annullamento. La chiave API resta nel portachiavi di sistema e non viene
//! mai restituita al frontend.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use greatnote_core::{
    ai::{
        AiError,
        builtin::BuiltinProvider,
        claude_code::{self, ClaudeCode, ClaudeCodeStatus},
        history::{HistoryEntry, HistoryKind, HistorySource, NewHistoryEntry},
        index::{AiIndexStatus, IndexProgress, IndexReport, SemanticResult, index_pending_notes},
        prompt::{
            self, Language, MAX_QUESTION_CHARS, SuggestedTask, TransformAction, parse_tags,
            parse_tasks,
        },
        provider::{AiProvider, ApiKey, ChatMessage, OpenAiCompatible, Provenance},
        retrieval::{ContextSource, SourceRef},
        settings::{AiSettings, Endpoint, OcrEngine, ProviderKind},
    },
    content::{markdown_to_document, to_plain_text},
    ocr::{
        MAX_VISION_IMAGE_BYTES, TesseractInfo, is_context_overflow, prepare_for_vision,
        run_tesseract, tesseract_info, vision_prompt, vision_sizes,
    },
    storage::Database,
};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State, ipc::Channel};

use crate::{
    builtin_ai_commands,
    data_commands::blocking,
    planner_commands::{UiLanguage, get_time_context},
};

type CommandResult<T> = Result<T, String>;

const KEYRING_SERVICE: &str = "dev.greatnote.notes";
const KEYRING_USER: &str = "ai-provider-api-key";
const MAX_API_KEY_LENGTH: usize = 4096;
const MAX_HISTORY_MESSAGES: usize = 50;
const MAX_MARKDOWN_CHARS: usize = 400_000;
pub const INDEX_PROGRESS_EVENT: &str = "ai-index-progress";
pub const INDEX_FINISHED_EVENT: &str = "ai-index-finished";
/// Controllo dell'indicizzazione automatica.
const AUTO_INDEX_INTERVAL: Duration = Duration::from_secs(90);

fn message(error: impl ToString) -> String {
    error.to_string()
}

/// Richieste in corso (per l'annullamento) e indicizzazione attiva.
#[derive(Default)]
pub struct AiJobs {
    requests: Mutex<HashMap<String, Arc<AtomicBool>>>,
    indexing: Mutex<Option<Arc<AtomicBool>>>,
    /// Lettura del testo di tutte le immagini in corso.
    image_text: Mutex<Option<Arc<AtomicBool>>>,
}

struct RequestGuard<'a> {
    jobs: &'a AiJobs,
    id: String,
    cancel: Arc<AtomicBool>,
}

impl Drop for RequestGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut requests) = self.jobs.requests.lock() {
            requests.remove(&self.id);
        }
    }
}

impl AiJobs {
    fn begin(&self, id: &str) -> CommandResult<RequestGuard<'_>> {
        if id.is_empty()
            || id.len() > 64
            || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        {
            return Err("Identificativo di richiesta non valido".to_owned());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.requests
            .lock()
            .map_err(|_| "Stato delle richieste AI non disponibile".to_owned())?
            .insert(id.to_owned(), Arc::clone(&cancel));
        Ok(RequestGuard {
            jobs: self,
            id: id.to_owned(),
            cancel,
        })
    }

    fn cancel(&self, id: &str) {
        if let Ok(requests) = self.requests.lock()
            && let Some(flag) = requests.get(id)
        {
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn begin_indexing(&self) -> Option<Arc<AtomicBool>> {
        let mut indexing = self.indexing.lock().ok()?;
        if indexing.is_some() {
            return None;
        }
        let flag = Arc::new(AtomicBool::new(false));
        *indexing = Some(Arc::clone(&flag));
        Some(flag)
    }

    fn end_indexing(&self) {
        if let Ok(mut indexing) = self.indexing.lock() {
            *indexing = None;
        }
    }

    fn cancel_indexing(&self) {
        if let Ok(indexing) = self.indexing.lock()
            && let Some(flag) = indexing.as_ref()
        {
            flag.store(true, Ordering::Relaxed);
        }
    }

    fn is_indexing(&self) -> bool {
        self.indexing
            .lock()
            .map(|indexing| indexing.is_some())
            .unwrap_or(false)
    }
}

// ---------------------------------------------------------------------------
// Portachiavi

fn keyring_entry() -> CommandResult<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|error| message(AiError::Keyring(error.to_string())))
}

fn read_api_key() -> CommandResult<Option<ApiKey>> {
    match keyring_entry()?.get_password() {
        Ok(value) => Ok(ApiKey::new(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(message(AiError::Keyring(error.to_string()))),
    }
}

fn provider_for(settings: &AiSettings, endpoint: Endpoint) -> CommandResult<Box<dyn AiProvider>> {
    provider_with_key(settings, endpoint, None)
}

/// Come [`provider_for`], ma una chiave appena digitata (non ancora salvata)
/// ha la precedenza su quella del portachiavi.
fn provider_with_key(
    settings: &AiSettings,
    endpoint: Endpoint,
    typed_key: Option<ApiKey>,
) -> CommandResult<Box<dyn AiProvider>> {
    let timeout = Duration::from_secs(settings.timeout_seconds.into());
    if settings.provider == ProviderKind::ClaudeCode {
        let executable = claude_code::locate(&settings.claude_path).map_err(message)?;
        return Ok(Box::new(ClaudeCode::new(executable, timeout)));
    }
    if settings.provider == ProviderKind::Builtin {
        return Ok(Box::new(BuiltinProvider::new(
            builtin_ai_commands::runtime()?,
            settings,
            timeout,
        )));
    }
    let key = if typed_key.is_some() {
        typed_key
    } else if settings.use_api_key {
        Some(read_api_key()?.ok_or_else(|| {
            "Chiave API non trovata nel portachiavi: inseriscila di nuovo nelle impostazioni AI"
                .to_owned()
        })?)
    } else {
        None
    };
    Ok(Box::new(OpenAiCompatible::new(endpoint, key, timeout)))
}

/// Impostazioni e provider per una richiesta che invia dati: AI attiva,
/// consenso per i server remoti, modello di chat scelto.
fn chat_provider(database: &Database) -> CommandResult<(AiSettings, Box<dyn AiProvider>)> {
    let settings = database.ai_settings().map_err(message)?;
    let endpoint = settings.ensure_usable().map_err(message)?;
    settings.ensure_chat_model().map_err(message)?;
    let provider = provider_for(&settings, endpoint)?;
    Ok((settings, provider))
}

fn language(app: &AppHandle) -> Language {
    if app.state::<UiLanguage>().is_english() {
        Language::En
    } else {
        Language::It
    }
}

/// Registra la richiesta nella cronologia, se attiva. Le richieste annullate
/// non vengono registrate; un errore di salvataggio non blocca la risposta.
#[allow(clippy::too_many_arguments)]
fn remember(
    database: &Database,
    settings: &AiSettings,
    provenance: &Provenance,
    kind: HistoryKind,
    conversation_id: Option<&str>,
    note_id: Option<&str>,
    prompt: &str,
    sources: Vec<HistorySource>,
    outcome: Result<&str, &AiError>,
) {
    if !settings.keep_history || matches!(outcome, Err(AiError::Cancelled)) {
        return;
    }
    let error = outcome.err().map(ToString::to_string);
    let entry = NewHistoryEntry {
        kind,
        conversation_id,
        note_id,
        prompt,
        response: outcome.unwrap_or_default(),
        sources,
        provenance,
        error: error.as_deref(),
    };
    if let Err(error) = database.ai_history_add(entry) {
        tracing::warn!("cronologia AI non salvata: {error}");
    }
}

// ---------------------------------------------------------------------------
// Impostazioni e stato

#[derive(Debug, Serialize)]
pub struct AiStatus {
    pub settings: AiSettings,
    /// Il server configurato è su questo computer.
    pub local: bool,
    pub host: String,
    pub index: AiIndexStatus,
    pub indexing: bool,
}

fn status(database: &Database, jobs: &AiJobs) -> CommandResult<AiStatus> {
    let settings = database.ai_settings().map_err(message)?;
    let endpoint = settings.endpoint().map_err(message)?;
    let index = database.ai_index_status(&settings).map_err(message)?;
    Ok(AiStatus {
        local: endpoint.is_local(),
        host: endpoint.host,
        index,
        indexing: jobs.is_indexing(),
        settings,
    })
}

#[tauri::command]
pub async fn get_ai_status(app: AppHandle) -> CommandResult<AiStatus> {
    blocking(app, |app| {
        status(&app.state::<Database>(), &app.state::<AiJobs>())
    })
    .await
}

#[tauri::command]
pub async fn update_ai_settings(app: AppHandle, settings: AiSettings) -> CommandResult<AiStatus> {
    blocking(app, move |app| {
        let database = app.state::<Database>();
        let jobs = app.state::<AiJobs>();
        let previous = database.ai_settings().map_err(message)?;
        // La presenza della chiave cambia solo con i comandi del portachiavi.
        let settings = AiSettings {
            use_api_key: previous.use_api_key,
            ..settings
        };
        let saved = database.set_ai_settings(settings).map_err(message)?;
        if saved != previous {
            // L'indicizzazione in corso usa le impostazioni precedenti.
            jobs.cancel_indexing();
            // I server integrati non più usati liberano subito la memoria.
            if let Ok(runtime) = builtin_ai_commands::runtime() {
                runtime.stop_idle(Duration::ZERO);
            }
        }
        database.ai_purge_index(&saved).map_err(message)?;
        status(&database, &jobs)
    })
    .await
}

#[tauri::command]
pub async fn set_ai_api_key(app: AppHandle, key: String) -> CommandResult<AiStatus> {
    blocking(app, move |app| {
        let key = key.trim();
        if key.is_empty() || key.len() > MAX_API_KEY_LENGTH || key.chars().any(char::is_control) {
            return Err("Chiave API non valida".to_owned());
        }
        keyring_entry()?
            .set_password(key)
            .map_err(|error| message(AiError::Keyring(error.to_string())))?;
        let database = app.state::<Database>();
        let settings = database.ai_settings().map_err(message)?;
        database
            .set_ai_settings(AiSettings {
                use_api_key: true,
                ..settings
            })
            .map_err(message)?;
        status(&database, &app.state::<AiJobs>())
    })
    .await
}

#[tauri::command]
pub async fn delete_ai_api_key(app: AppHandle) -> CommandResult<AiStatus> {
    blocking(app, move |app| {
        match keyring_entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(error) => return Err(message(AiError::Keyring(error.to_string()))),
        }
        let database = app.state::<Database>();
        let settings = database.ai_settings().map_err(message)?;
        database
            .set_ai_settings(AiSettings {
                use_api_key: false,
                ..settings
            })
            .map_err(message)?;
        status(&database, &app.state::<AiJobs>())
    })
    .await
}

#[derive(Debug, Serialize)]
pub struct ConnectionTest {
    pub models: Vec<String>,
    pub local: bool,
    pub host: String,
    /// Solo per Claude Code: eseguibile, versione e stato dell'accesso.
    pub claude_code: Option<ClaudeCodeStatus>,
}

/// Prova la connessione con le impostazioni del modulo (anche non salvate) e
/// l'eventuale chiave digitata ma non ancora salvata: chiede solo l'elenco
/// dei modelli, senza inviare note. La chiave non viene conservata.
#[tauri::command]
pub async fn test_ai_connection(
    app: AppHandle,
    settings: AiSettings,
    api_key: Option<String>,
) -> CommandResult<ConnectionTest> {
    blocking(app, move |app| {
        let stored = app.state::<Database>().ai_settings().map_err(message)?;
        let settings = AiSettings {
            use_api_key: stored.use_api_key,
            ..settings
        }
        .validated()
        .map_err(message)?;
        let endpoint = settings.endpoint().map_err(message)?;
        let (local, host) = (endpoint.is_local(), endpoint.host.clone());
        if settings.provider == ProviderKind::ClaudeCode {
            // Nessuna richiesta al modello: versione e stato dell'accesso.
            let executable = claude_code::locate(&settings.claude_path).map_err(message)?;
            let status = ClaudeCode::new(executable, Duration::from_secs(30))
                .status()
                .map_err(message)?;
            if !status.logged_in {
                return Err(
                    "Claude Code è installato ma non hai effettuato l'accesso: apri un terminale, esegui `claude` e accedi"
                        .to_owned(),
                );
            }
            return Ok(ConnectionTest {
                models: claude_code::MODEL_ALIASES
                    .iter()
                    .map(|alias| (*alias).to_owned())
                    .collect(),
                local,
                host,
                claude_code: Some(status),
            });
        }
        if settings.provider == ProviderKind::Builtin {
            // Nessun processo avviato: motore presente e modelli scaricati.
            let runtime = builtin_ai_commands::runtime()?;
            if runtime.executable().is_none() {
                return Err(message(AiError::NotConfigured(
                    "il motore AI integrato non è incluso in questa installazione",
                )));
            }
            let models = runtime
                .store()
                .installed()
                .into_iter()
                .map(|model| model.id.to_owned())
                .collect();
            return Ok(ConnectionTest {
                models,
                local,
                host,
                claude_code: None,
            });
        }
        let typed_key = api_key.and_then(ApiKey::new);
        let models = provider_with_key(&settings, endpoint, typed_key)?
            .list_models()
            .map_err(|error| match error {
                AiError::Unauthorized(_) => format!(
                    "{error}. Se il server richiede una chiave (per esempio Open WebUI: Impostazioni › Account › Chiavi API), inseriscila nel campo «Chiave API»"
                ),
                other => message(other),
            })?;
        Ok(ConnectionTest {
            models,
            local,
            host,
            claude_code: None,
        })
    })
    .await
}

#[tauri::command]
pub fn cancel_ai_request(jobs: State<'_, AiJobs>, request_id: String) {
    jobs.cancel(&request_id);
}

// ---------------------------------------------------------------------------
// Chat con le note

#[derive(Debug, Serialize)]
pub struct PreparedContext {
    pub sources: Vec<ContextSource>,
    pub local: bool,
    pub host: String,
    /// Avviso se la ricerca semantica non era disponibile.
    pub warning: Option<String>,
}

/// Primo passo della chat: trova le fonti da inviare. Il frontend le mostra
/// (e per un server remoto chiede conferma) prima di [`ai_chat`].
#[tauri::command]
pub async fn ai_prepare_chat(
    app: AppHandle,
    request_id: String,
    question: String,
    note_id: Option<String>,
) -> CommandResult<PreparedContext> {
    blocking(app, move |app| {
        let jobs = app.state::<AiJobs>();
        let guard = jobs.begin(&request_id)?;
        let database = app.state::<Database>();
        let (settings, provider) = chat_provider(&database)?;
        let question = question.trim();
        if question.is_empty() {
            return Err("Scrivi una domanda".to_owned());
        }
        if question.chars().count() > MAX_QUESTION_CHARS {
            return Err(message(AiError::InputTooLarge(MAX_QUESTION_CHARS)));
        }
        let endpoint = settings.endpoint().map_err(message)?;
        let mut warning = None;
        let sources = if let Some(note_id) = note_id {
            database
                .ai_note_context(&settings, &note_id)
                .map_err(message)?
        } else {
            let semantic = !settings.embedding_model.is_empty()
                && database.ai_index_status(&settings).map_err(message)?.chunks > 0;
            let vector = if semantic {
                match provider.embed(
                    &settings.embedding_model,
                    &[question.to_owned()],
                    &guard.cancel,
                ) {
                    Ok(mut vectors) => vectors.pop(),
                    Err(AiError::Cancelled) => return Err(message(AiError::Cancelled)),
                    Err(error) => {
                        warning = Some(format!(
                            "Ricerca semantica non disponibile, uso la ricerca testuale: {error}"
                        ));
                        None
                    }
                }
            } else {
                None
            };
            database
                .ai_retrieve(&settings, question, vector.as_deref())
                .map_err(message)?
        };
        Ok(PreparedContext {
            sources,
            local: endpoint.is_local(),
            host: endpoint.host,
            warning,
        })
    })
    .await
}

#[derive(Debug, Serialize)]
pub struct AiAnswer {
    pub text: String,
    /// Fonti effettivamente inviate, numerate come nelle citazioni.
    pub sources: Vec<ContextSource>,
    pub provenance: Provenance,
}

#[tauri::command]
pub async fn ai_chat(
    app: AppHandle,
    request_id: String,
    question: String,
    history: Vec<ChatMessage>,
    sources: Vec<SourceRef>,
    conversation_id: Option<String>,
    on_delta: Channel<String>,
) -> CommandResult<AiAnswer> {
    blocking(app, move |app| {
        if history.len() > MAX_HISTORY_MESSAGES {
            return Err("Conversazione troppo lunga: iniziane una nuova".to_owned());
        }
        let jobs = app.state::<AiJobs>();
        let guard = jobs.begin(&request_id)?;
        let database = app.state::<Database>();
        let (settings, provider) = chat_provider(&database)?;
        let sources = database
            .ai_resolve_sources(&settings, &sources)
            .map_err(message)?;
        let messages =
            prompt::chat_messages(language(app), &history, &question, &sources).map_err(message)?;
        let provenance = provider.provenance(&settings.chat_model);
        let result = provider.chat(
            &settings.chat_model,
            &messages,
            0.3,
            &guard.cancel,
            &mut |delta| {
                let _ = on_delta.send(delta.to_owned());
            },
        );
        remember(
            &database,
            &settings,
            &provenance,
            HistoryKind::Chat,
            conversation_id.as_deref(),
            None,
            &question,
            sources.iter().map(HistorySource::from).collect(),
            result.as_deref(),
        );
        Ok(AiAnswer {
            text: result.map_err(message)?,
            sources,
            provenance,
        })
    })
    .await
}

// ---------------------------------------------------------------------------
// Modifiche e suggerimenti sulla nota

#[derive(Debug, Serialize)]
pub struct AiTextResult {
    pub text: String,
    pub provenance: Provenance,
}

/// Verifica che la nota esista e non sia in un taccuino escluso.
fn ensure_note_allowed(
    database: &Database,
    settings: &AiSettings,
    note_id: &str,
) -> CommandResult<greatnote_core::domain::note::Note> {
    let note = database.get_note(note_id).map_err(message)?;
    if settings.is_excluded(note.notebook_id.as_deref()) {
        return Err(message(AiError::ExcludedNote));
    }
    Ok(note)
}

#[tauri::command]
pub async fn ai_transform(
    app: AppHandle,
    request_id: String,
    note_id: Option<String>,
    action: TransformAction,
    text: String,
    on_delta: Channel<String>,
) -> CommandResult<AiTextResult> {
    blocking(app, move |app| {
        let jobs = app.state::<AiJobs>();
        let guard = jobs.begin(&request_id)?;
        let database = app.state::<Database>();
        let (settings, provider) = chat_provider(&database)?;
        if let Some(note_id) = &note_id {
            ensure_note_allowed(&database, &settings, note_id)?;
        }
        let language = language(app);
        let messages = prompt::transform_messages(language, action, &text).map_err(message)?;
        let provenance = provider.provenance(&settings.chat_model);
        let result = provider.chat(
            &settings.chat_model,
            &messages,
            0.3,
            &guard.cancel,
            &mut |delta| {
                let _ = on_delta.send(delta.to_owned());
            },
        );
        let excerpt: String = text.trim().chars().take(300).collect();
        remember(
            &database,
            &settings,
            &provenance,
            HistoryKind::Transform,
            None,
            note_id.as_deref(),
            &format!("{}: {excerpt}", action.label(language)),
            Vec::new(),
            result.as_deref(),
        );
        let text = result.map_err(message)?;
        if text.is_empty() {
            return Err(message(AiError::InvalidResponse(
                "il modello ha restituito un testo vuoto".to_owned(),
            )));
        }
        Ok(AiTextResult { text, provenance })
    })
    .await
}

/// Spiega le differenze tra una revisione e la versione attuale della nota.
/// I testi vengono letti qui dal database: dal frontend arriva solo l'id.
#[tauri::command]
pub async fn ai_compare_revision(
    app: AppHandle,
    request_id: String,
    revision_id: String,
    on_delta: Channel<String>,
) -> CommandResult<AiTextResult> {
    blocking(app, move |app| {
        let jobs = app.state::<AiJobs>();
        let guard = jobs.begin(&request_id)?;
        let database = app.state::<Database>();
        let (settings, provider) = chat_provider(&database)?;
        let revision = database.get_note_revision(&revision_id).map_err(message)?;
        let note = ensure_note_allowed(&database, &settings, &revision.revision.note_id)?;
        let language = language(app);
        let messages = prompt::compare_messages(
            language,
            &revision.revision.title,
            &to_plain_text(&revision.content),
            &note.title,
            &to_plain_text(&note.content),
        )
        .map_err(message)?;
        let provenance = provider.provenance(&settings.chat_model);
        let result = provider.chat(
            &settings.chat_model,
            &messages,
            0.2,
            &guard.cancel,
            &mut |delta| {
                let _ = on_delta.send(delta.to_owned());
            },
        );
        let label = match language {
            Language::It => "Confronta versioni",
            Language::En => "Compare versions",
        };
        remember(
            &database,
            &settings,
            &provenance,
            HistoryKind::Transform,
            None,
            Some(&note.id),
            &format!("{label}: {} ({})", note.title, revision.revision.created_at),
            Vec::new(),
            result.as_deref(),
        );
        let text = result.map_err(message)?;
        if text.is_empty() {
            return Err(message(AiError::InvalidResponse(
                "il modello ha restituito un testo vuoto".to_owned(),
            )));
        }
        Ok(AiTextResult { text, provenance })
    })
    .await
}

#[derive(Debug, Serialize)]
pub struct TagSuggestions {
    pub tags: Vec<String>,
    pub provenance: Provenance,
}

#[tauri::command]
pub async fn ai_suggest_tags(
    app: AppHandle,
    request_id: String,
    note_id: String,
) -> CommandResult<TagSuggestions> {
    blocking(app, move |app| {
        let jobs = app.state::<AiJobs>();
        let guard = jobs.begin(&request_id)?;
        let database = app.state::<Database>();
        let (settings, provider) = chat_provider(&database)?;
        let note = ensure_note_allowed(&database, &settings, &note_id)?;
        let existing: Vec<String> = database
            .list_tags()
            .map_err(message)?
            .into_iter()
            .map(|tag| tag.name)
            .collect();
        let messages = prompt::tag_messages(
            language(app),
            &note.title,
            &to_plain_text(&note.content),
            &existing,
        )
        .map_err(message)?;
        let provenance = provider.provenance(&settings.chat_model);
        let result = provider
            .chat(
                &settings.chat_model,
                &messages,
                0.1,
                &guard.cancel,
                &mut |_| {},
            )
            .and_then(|answer| parse_tags(&answer));
        let joined = result.as_ref().map(|tags| tags.join(", "));
        remember(
            &database,
            &settings,
            &provenance,
            HistoryKind::Tags,
            None,
            Some(&note_id),
            &note.title,
            Vec::new(),
            joined.as_deref().map_err(|error| *error),
        );
        Ok(TagSuggestions {
            tags: result.map_err(message)?,
            provenance,
        })
    })
    .await
}

#[derive(Debug, Serialize)]
pub struct TaskSuggestions {
    pub tasks: Vec<SuggestedTask>,
    pub provenance: Provenance,
}

#[tauri::command]
pub async fn ai_extract_tasks(
    app: AppHandle,
    request_id: String,
    note_id: String,
) -> CommandResult<TaskSuggestions> {
    blocking(app, move |app| {
        let jobs = app.state::<AiJobs>();
        let guard = jobs.begin(&request_id)?;
        let database = app.state::<Database>();
        let (settings, provider) = chat_provider(&database)?;
        let note = ensure_note_allowed(&database, &settings, &note_id)?;
        let messages = prompt::task_messages(
            language(app),
            &note.title,
            &to_plain_text(&note.content),
            &get_time_context().today,
        )
        .map_err(message)?;
        let provenance = provider.provenance(&settings.chat_model);
        let result = provider
            .chat(
                &settings.chat_model,
                &messages,
                0.1,
                &guard.cancel,
                &mut |_| {},
            )
            .and_then(|answer| parse_tasks(&answer));
        let listed = result.as_ref().map(|tasks| {
            tasks
                .iter()
                .map(|task| match &task.due_date {
                    Some(date) => format!("- {} ({date})", task.title),
                    None => format!("- {}", task.title),
                })
                .collect::<Vec<_>>()
                .join("\n")
        });
        remember(
            &database,
            &settings,
            &provenance,
            HistoryKind::Tasks,
            None,
            Some(&note_id),
            &note.title,
            Vec::new(),
            listed.as_deref().map_err(|error| *error),
        );
        Ok(TaskSuggestions {
            tasks: result.map_err(message)?,
            provenance,
        })
    })
    .await
}

/// Converte il Markdown prodotto dall'AI nel formato dei documenti, per
/// inserirlo nell'editor quando l'utente applica una proposta.
#[tauri::command]
pub fn ai_markdown_to_document(markdown: String) -> CommandResult<Value> {
    if markdown.chars().count() > MAX_MARKDOWN_CHARS {
        return Err(message(AiError::InputTooLarge(MAX_MARKDOWN_CHARS)));
    }
    Ok(markdown_to_document(&markdown).document)
}

// ---------------------------------------------------------------------------
// Indice semantico

#[derive(Debug, Clone, Serialize)]
pub struct IndexFinished {
    pub report: Option<IndexReport>,
    pub error: Option<String>,
    pub cancelled: bool,
}

/// Esegue un ciclo di indicizzazione nel thread corrente. `None` se ne era
/// già in corso un altro.
fn run_indexing(app: &AppHandle) -> Option<IndexFinished> {
    let jobs = app.state::<AiJobs>();
    let cancel = jobs.begin_indexing()?;
    let database = app.state::<Database>();
    let outcome = (|| -> CommandResult<IndexReport> {
        let settings = database.ai_settings().map_err(message)?;
        let endpoint = settings.ensure_usable().map_err(message)?;
        settings.ensure_embedding_model().map_err(message)?;
        let provider = provider_for(&settings, endpoint)?;
        index_pending_notes(
            &database,
            provider.as_ref(),
            &settings,
            &cancel,
            &mut |progress: IndexProgress| {
                let _ = app.emit(INDEX_PROGRESS_EVENT, progress);
            },
        )
        .map_err(|error| match error {
            AiError::Cancelled => String::new(),
            other => message(other),
        })
    })();
    jobs.end_indexing();
    let finished = match outcome {
        Ok(report) => IndexFinished {
            report: Some(report),
            error: None,
            cancelled: false,
        },
        Err(error) if error.is_empty() => IndexFinished {
            report: None,
            error: None,
            cancelled: true,
        },
        Err(error) => IndexFinished {
            report: None,
            error: Some(error),
            cancelled: false,
        },
    };
    let _ = app.emit(INDEX_FINISHED_EVENT, finished.clone());
    Some(finished)
}

/// Avvia l'indicizzazione in background; l'avanzamento arriva con gli eventi
/// `ai-index-progress` e `ai-index-finished`.
#[tauri::command]
pub fn start_ai_indexing(app: AppHandle, jobs: State<'_, AiJobs>) -> CommandResult<()> {
    if jobs.is_indexing() {
        return Err("Indicizzazione già in corso".to_owned());
    }
    thread::Builder::new()
        .name("ai-index".to_owned())
        .spawn(move || {
            run_indexing(&app);
        })
        .map_err(message)?;
    Ok(())
}

#[tauri::command]
pub fn cancel_ai_indexing(jobs: State<'_, AiJobs>) {
    jobs.cancel_indexing();
}

#[tauri::command]
pub async fn clear_ai_index(app: AppHandle) -> CommandResult<AiStatus> {
    blocking(app, |app| {
        let jobs = app.state::<AiJobs>();
        jobs.cancel_indexing();
        let database = app.state::<Database>();
        database.ai_clear_index().map_err(message)?;
        status(&database, &jobs)
    })
    .await
}

#[tauri::command]
pub async fn ai_semantic_search(
    app: AppHandle,
    request_id: String,
    query: String,
    limit: u32,
) -> CommandResult<Vec<SemanticResult>> {
    blocking(app, move |app| {
        let jobs = app.state::<AiJobs>();
        let guard = jobs.begin(&request_id)?;
        let database = app.state::<Database>();
        let settings = database.ai_settings().map_err(message)?;
        let endpoint = settings.ensure_usable().map_err(message)?;
        let model = settings
            .ensure_embedding_model()
            .map_err(message)?
            .to_owned();
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        if query.chars().count() > MAX_QUESTION_CHARS {
            return Err(message(AiError::InputTooLarge(MAX_QUESTION_CHARS)));
        }
        let provider = provider_for(&settings, endpoint)?;
        let provenance = provider.provenance(&model);
        let result = provider
            .embed(&model, &[query.to_owned()], &guard.cancel)
            .and_then(|mut vectors| {
                vectors.pop().ok_or_else(|| {
                    AiError::InvalidResponse("il server non ha restituito l'embedding".to_owned())
                })
            })
            .and_then(|vector| {
                database
                    .ai_semantic_search(&settings, &vector, limit.clamp(1, 50) as usize)
                    .map_err(AiError::from)
            });
        let summary = result.as_ref().map(|results| {
            results
                .iter()
                .map(|result| format!("- {}", result.note.title))
                .collect::<Vec<_>>()
                .join("\n")
        });
        let sources = result
            .as_ref()
            .map(|results| {
                results
                    .iter()
                    .enumerate()
                    .map(|(index, result)| HistorySource {
                        number: index + 1,
                        note_id: result.note.id.clone(),
                        title: result.note.title.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        remember(
            &database,
            &settings,
            &provenance,
            HistoryKind::Search,
            None,
            None,
            query,
            sources,
            summary.as_deref().map_err(|error| *error),
        );
        result.map_err(message)
    })
    .await
}

// ---------------------------------------------------------------------------
// Immagini: visualizzazione e lettura del testo

pub const IMAGE_TEXT_PROGRESS_EVENT: &str = "image-text-progress";
pub const IMAGE_TEXT_FINISHED_EVENT: &str = "image-text-finished";

fn data_directory(database: &Database) -> CommandResult<std::path::PathBuf> {
    database
        .data_directory()
        .map(std::path::Path::to_path_buf)
        .map_err(message)
}

/// Byte di un'immagine allegata (verificata con l'hash), per mostrarla nel
/// testo della nota. Solo immagini, mai altri file.
#[tauri::command]
pub async fn read_attachment_image(
    app: AppHandle,
    attachment_id: String,
) -> CommandResult<tauri::ipc::Response> {
    blocking(app, move |app| {
        let database = app.state::<Database>();
        let directory = data_directory(&database)?;
        let image = database
            .image_for_ocr(&attachment_id, &directory)
            .map_err(message)?;
        Ok(tauri::ipc::Response::new(image.bytes))
    })
    .await
}

#[derive(Debug, Serialize)]
pub struct ImageTextCapabilities {
    pub tesseract: Option<TesseractInfo>,
    pub pending_images: u64,
}

#[tauri::command]
pub async fn image_text_capabilities(app: AppHandle) -> CommandResult<ImageTextCapabilities> {
    blocking(app, |app| {
        Ok(ImageTextCapabilities {
            tesseract: tesseract_info(),
            pending_images: app
                .state::<Database>()
                .count_images_without_text()
                .map_err(message)?,
        })
    })
    .await
}

/// Legge il testo di un'immagine con il motore scelto. Restituisce l'esito
/// da salvare (o l'annullamento, che non viene salvato) e il nome del motore.
fn read_text_of(
    app: &AppHandle,
    database: &Database,
    settings: &AiSettings,
    attachment_id: &str,
    cancel: &AtomicBool,
) -> CommandResult<(Result<String, AiError>, String)> {
    let directory = data_directory(database)?;
    let image = database
        .image_for_ocr(attachment_id, &directory)
        .map_err(message)?;
    let timeout = Duration::from_secs(settings.timeout_seconds.into());
    match settings.ocr_engine {
        OcrEngine::Tesseract => {
            let info = tesseract_info().ok_or_else(|| {
                "Tesseract non è installato: su Fedora `sudo dnf install tesseract tesseract-langpack-ita`, oppure scegli il modello AI".to_owned()
            })?;
            Ok((
                run_tesseract(&info, &image.bytes, timeout, cancel),
                "Tesseract".to_owned(),
            ))
        }
        OcrEngine::Ai => {
            let endpoint = settings.ensure_usable().map_err(message)?;
            if settings.is_excluded(image.notebook_id.as_deref()) {
                return Err(message(AiError::ExcludedNote));
            }
            let model = if settings.provider == ProviderKind::ClaudeCode {
                settings.chat_model.clone()
            } else if settings.provider == ProviderKind::Builtin {
                // I modelli integrati per la chat non leggono le immagini.
                if settings.vision_model.is_empty() {
                    return Err(message(AiError::NotConfigured(
                        "scegli e scarica un modello per le immagini nelle impostazioni AI",
                    )));
                }
                settings.vision_model.clone()
            } else if !settings.vision_model.is_empty() {
                settings.vision_model.clone()
            } else {
                settings.ensure_chat_model().map_err(message)?.to_owned()
            };
            let provider = provider_for(settings, endpoint)?;
            let (instructions, request) = vision_prompt(language(app));
            // Immagine ridotta (meno token); se il server segnala contesto
            // insufficiente si riprova più piccola.
            let mut outcome = Err(AiError::Ocr("nessun tentativo".to_owned()));
            for side in vision_sizes(settings.vision_max_side) {
                let prepared =
                    match prepare_for_vision(&image.bytes, &image.attachment.mime_type, side) {
                        Ok(prepared) => prepared,
                        Err(error) => {
                            outcome = Err(error);
                            break;
                        }
                    };
                if prepared.bytes.len() > MAX_VISION_IMAGE_BYTES {
                    outcome = Err(AiError::Ocr(
                        "immagine oltre 5 MB anche dopo la riduzione".to_owned(),
                    ));
                    continue;
                }
                outcome = provider.read_image(
                    &model,
                    instructions,
                    request,
                    &prepared.bytes,
                    prepared.mime,
                    cancel,
                );
                match &outcome {
                    Err(error) if is_context_overflow(error) => continue,
                    _ => break,
                }
            }
            if let Err(error) = &outcome
                && is_context_overflow(error)
            {
                outcome = Err(AiError::Ocr(format!(
                    "{error}. Anche ridotta l'immagine non entra nel contesto del modello: aumenta il contesto del server (llama.cpp: -c 8192; Ollama: num_ctx 8192) o abbassa la risoluzione nelle impostazioni AI"
                )));
            }
            Ok((outcome, provider.provenance(&model).model))
        }
    }
}

#[tauri::command]
pub async fn read_image_text(
    app: AppHandle,
    request_id: String,
    attachment_id: String,
) -> CommandResult<greatnote_core::domain::note::Attachment> {
    blocking(app, move |app| {
        let jobs = app.state::<AiJobs>();
        let guard = jobs.begin(&request_id)?;
        let database = app.state::<Database>();
        let settings = database.ai_settings().map_err(message)?;
        let (outcome, engine) =
            read_text_of(app, &database, &settings, &attachment_id, &guard.cancel)?;
        match outcome {
            Err(AiError::Cancelled) => Err(message(AiError::Cancelled)),
            outcome => {
                let error = outcome.as_ref().err().map(ToString::to_string);
                database
                    .store_image_text(
                        &attachment_id,
                        outcome
                            .as_deref()
                            .map_err(|_| error.as_deref().unwrap_or_default()),
                        &engine,
                    )
                    .map_err(message)
            }
        }
    })
    .await
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageTextFinished {
    pub read: u64,
    pub failed: u64,
    pub skipped: u64,
    pub error: Option<String>,
    pub cancelled: bool,
}

/// Legge il testo di tutte le immagini non ancora lette, in background.
#[tauri::command]
pub fn start_image_text_all(
    app: AppHandle,
    jobs: State<'_, AiJobs>,
    include_failed: bool,
) -> CommandResult<()> {
    let cancel = {
        let mut slot = jobs
            .image_text
            .lock()
            .map_err(|_| "Stato della lettura non disponibile".to_owned())?;
        if slot.is_some() {
            return Err("Lettura delle immagini già in corso".to_owned());
        }
        let flag = Arc::new(AtomicBool::new(false));
        *slot = Some(Arc::clone(&flag));
        flag
    };
    thread::Builder::new()
        .name("image-text".to_owned())
        .spawn(move || {
            let database = app.state::<Database>();
            let mut finished = ImageTextFinished {
                read: 0,
                failed: 0,
                skipped: 0,
                error: None,
                cancelled: false,
            };
            let outcome = (|| -> CommandResult<()> {
                let settings = database.ai_settings().map_err(message)?;
                let pending = database
                    .images_without_text(include_failed, 5_000)
                    .map_err(message)?;
                let total = pending.len() as u64;
                for (index, attachment_id) in pending.iter().enumerate() {
                    if cancel.load(Ordering::Relaxed) {
                        finished.cancelled = true;
                        break;
                    }
                    let _ = app.emit(
                        IMAGE_TEXT_PROGRESS_EVENT,
                        IndexProgress {
                            done: index as u64,
                            total,
                        },
                    );
                    match read_text_of(&app, &database, &settings, attachment_id, &cancel) {
                        Ok((Err(AiError::Cancelled), _)) => {
                            finished.cancelled = true;
                            break;
                        }
                        Ok((outcome, engine)) => {
                            let error = outcome.as_ref().err().map(ToString::to_string);
                            if error.is_some() {
                                finished.failed += 1;
                            } else {
                                finished.read += 1;
                            }
                            database
                                .store_image_text(
                                    attachment_id,
                                    outcome
                                        .as_deref()
                                        .map_err(|_| error.as_deref().unwrap_or_default()),
                                    &engine,
                                )
                                .map_err(message)?;
                        }
                        // Note escluse, immagini troppo grandi, file mancanti.
                        Err(_) => finished.skipped += 1,
                    }
                }
                Ok(())
            })();
            if let Err(error) = outcome {
                finished.error = Some(error);
            }
            if let Ok(mut slot) = app.state::<AiJobs>().image_text.lock() {
                *slot = None;
            }
            let _ = app.emit(IMAGE_TEXT_FINISHED_EVENT, finished);
        })
        .map_err(message)?;
    Ok(())
}

#[tauri::command]
pub fn cancel_image_text_all(jobs: State<'_, AiJobs>) {
    if let Ok(slot) = jobs.image_text.lock()
        && let Some(flag) = slot.as_ref()
    {
        flag.store(true, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Cronologia

#[tauri::command]
pub async fn list_ai_history(
    app: AppHandle,
    query: Option<String>,
    limit: u32,
    offset: u32,
) -> CommandResult<Vec<HistoryEntry>> {
    blocking(app, move |app| {
        app.state::<Database>()
            .ai_history_list(query.as_deref(), limit, offset)
            .map_err(message)
    })
    .await
}

#[tauri::command]
pub async fn get_ai_conversation(
    app: AppHandle,
    conversation_id: String,
) -> CommandResult<Vec<HistoryEntry>> {
    blocking(app, move |app| {
        app.state::<Database>()
            .ai_history_conversation(&conversation_id)
            .map_err(message)
    })
    .await
}

#[tauri::command]
pub async fn delete_ai_history_entry(app: AppHandle, id: String) -> CommandResult<bool> {
    blocking(app, move |app| {
        app.state::<Database>()
            .ai_history_delete(&id)
            .map_err(message)
    })
    .await
}

#[tauri::command]
pub async fn clear_ai_history(app: AppHandle) -> CommandResult<u64> {
    blocking(app, |app| {
        app.state::<Database>().ai_history_clear().map_err(message)
    })
    .await
}

/// Indicizzazione automatica: solo se attivata dall'utente, con AI attiva e
/// modello di embedding scelto. Controlla periodicamente le note modificate.
pub fn spawn_auto_indexer(app: &AppHandle) {
    let app = app.clone();
    let spawned = thread::Builder::new()
        .name("ai-auto-index".to_owned())
        .spawn(move || {
            loop {
                thread::sleep(AUTO_INDEX_INTERVAL);
                let database = app.state::<Database>();
                let Ok(settings) = database.ai_settings() else {
                    continue;
                };
                if !settings.auto_index
                    || settings.ensure_usable().is_err()
                    || settings.embedding_model.is_empty()
                {
                    continue;
                }
                let pending = database
                    .ai_index_status(&settings)
                    .map(|status| status.pending_notes)
                    .unwrap_or(0);
                if pending > 0
                    && let Some(IndexFinished {
                        error: Some(error), ..
                    }) = run_indexing(&app)
                {
                    tracing::warn!("indicizzazione automatica non riuscita: {error}");
                }
            }
        });
    if let Err(error) = spawned {
        tracing::error!("indicizzazione automatica non avviata: {error}");
    }
}

#[cfg(test)]
mod tests {
    /// Prova reale del portachiavi di sistema (Secret Service: GNOME Keyring
    /// o KWallet) con un valore fittizio e un servizio di prova, poi rimosso:
    /// `cargo test -p greatnote-desktop keyring -- --ignored`
    #[test]
    #[ignore = "richiede il portachiavi della sessione desktop"]
    fn system_keyring_round_trip() {
        let entry = keyring::Entry::new("greatnote-test", "prova-portachiavi").expect("entry");
        entry.set_password("valore-fittizio-123").expect("set");
        assert_eq!(entry.get_password().expect("get"), "valore-fittizio-123");
        entry.delete_credential().expect("delete");
        assert!(matches!(entry.get_password(), Err(keyring::Error::NoEntry)));
    }
}
