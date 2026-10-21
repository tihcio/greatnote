//! Motore AI integrato (ADR 0020, ADR 0021): catalogo dei modelli, ricerca
//! su Hugging Face, download con avanzamento e annullamento, eliminazione e
//! ciclo di vita dei processi `llama-server`. I modelli stanno nella cartella
//! dati locale dell'utente (su Windows `%LOCALAPPDATA%`, non nel profilo
//! roaming) e non entrano nei backup.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use greatnote_core::{
    ai::{
        AiError,
        builtin::{
            catalog::{CatalogModel, ModelRole, ModelSource},
            hardware::{self, HardwareInfo, ModelFit, model_fit},
            hub::{HubClient, HubRepo, HubRepoDetails},
            server::{BuiltinRuntime, IDLE_TIMEOUT, SERVER_EXECUTABLE},
            store::ModelStore,
        },
        settings::ProviderKind,
    },
    storage::Database,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::data_commands::blocking;

type CommandResult<T> = Result<T, String>;

pub const DOWNLOAD_PROGRESS_EVENT: &str = "ai-model-download-progress";
pub const DOWNLOAD_FINISHED_EVENT: &str = "ai-model-download-finished";
/// Percorso alternativo di `llama-server` (sviluppo o build personalizzate).
const SERVER_OVERRIDE_ENV: &str = "GREATNOTE_LLAMA_SERVER";
const IDLE_CHECK_INTERVAL: Duration = Duration::from_secs(60);

static RUNTIME: OnceLock<Arc<BuiltinRuntime>> = OnceLock::new();
/// Memoria e schede video, rilevate una volta per avvio.
static HARDWARE: OnceLock<HardwareInfo> = OnceLock::new();

fn message(error: impl ToString) -> String {
    error.to_string()
}

/// Motore integrato, disponibile dopo [`setup`].
pub(crate) fn runtime() -> CommandResult<Arc<BuiltinRuntime>> {
    RUNTIME
        .get()
        .cloned()
        .ok_or_else(|| "Motore AI integrato non inizializzato".to_owned())
}

/// Hardware del computer; la prima chiamata interroga il motore.
fn hardware() -> &'static HardwareInfo {
    HARDWARE.get_or_init(|| {
        let engine = RUNTIME.get().and_then(|runtime| runtime.executable());
        let detected = hardware::detect(engine);
        tracing::info!(
            "hardware per i modelli AI: RAM {:?} byte, {} schede video",
            detected.ram_bytes,
            detected.gpus.len()
        );
        detected
    })
}

/// Prepara il motore all'avvio e ferma i server inattivi in background.
pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let directory = app.path().app_local_data_dir()?.join("ai-models");
    let executable = locate_server(app);
    match &executable {
        Some(path) => tracing::info!("motore AI integrato: {}", path.display()),
        None => tracing::info!("motore AI integrato non incluso in questa installazione"),
    }
    let _ = RUNTIME.set(Arc::new(BuiltinRuntime::new(
        executable,
        ModelStore::new(directory),
    )));
    let spawned = thread::Builder::new()
        .name("ai-builtin-idle".to_owned())
        .spawn(|| {
            loop {
                thread::sleep(IDLE_CHECK_INTERVAL);
                if let Some(runtime) = RUNTIME.get() {
                    runtime.stop_idle(IDLE_TIMEOUT);
                }
            }
        });
    if let Err(error) = spawned {
        tracing::error!("controllo dei server AI inattivi non avviato: {error}");
    }
    // Rilevamento in anticipo: le impostazioni AI si aprono senza attesa.
    if let Err(error) = thread::Builder::new()
        .name("ai-builtin-hardware".to_owned())
        .spawn(|| {
            hardware();
        })
    {
        tracing::warn!("rilevamento dell'hardware non avviato: {error}");
    }
    Ok(())
}

/// Ferma tutti i server all'uscita dall'app.
pub fn shutdown() {
    if let Some(runtime) = RUNTIME.get() {
        runtime.stop_all();
    }
}

/// `llama-server` distribuito con l'app (risorse), indicato nell'ambiente
/// oppure installato nel sistema (`PATH`, utile su Linux).
fn locate_server(app: &AppHandle) -> Option<PathBuf> {
    let bundled = app
        .path()
        .resource_dir()
        .ok()
        .map(|directory| directory.join("llama").join(SERVER_EXECUTABLE));
    let configured = std::env::var_os(SERVER_OVERRIDE_ENV)
        .map(PathBuf::from)
        .filter(|path| path.is_absolute());
    let in_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|directory| directory.join(SERVER_EXECUTABLE))
            .find(|path| path.is_file())
    });
    bundled
        .into_iter()
        .chain(configured)
        .find(|path| path.is_file())
        .or(in_path)
}

/// Download in corso, per l'annullamento.
#[derive(Default)]
pub struct ModelDownloads {
    active: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl ModelDownloads {
    fn is_active(&self, id: &str) -> bool {
        self.active
            .lock()
            .map(|active| active.contains_key(id))
            .unwrap_or(false)
    }
}

#[derive(Debug, Serialize)]
pub struct BuiltinModelInfo {
    pub id: String,
    pub role: ModelRole,
    pub name: String,
    pub license: String,
    pub memory_gb: f32,
    pub recommended: bool,
    pub size_bytes: u64,
    /// Byte ancora da scaricare (0 se installato; meno del totale se un
    /// file è condiviso con un modello già scaricato).
    pub missing_bytes: u64,
    pub installed: bool,
    pub downloading: bool,
    /// Quanto è adatto a questo computer.
    pub fit: ModelFit,
    /// Del catalogo curato, provato con il motore distribuito.
    pub curated: bool,
    /// Repository di Hugging Face, per i modelli scelti dalla ricerca.
    pub repo: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BuiltinCatalog {
    /// `llama-server` è incluso o installato.
    pub engine_available: bool,
    pub models: Vec<BuiltinModelInfo>,
    pub models_directory: String,
    pub used_bytes: u64,
    pub running_servers: usize,
    pub hardware: HardwareInfo,
}

fn catalog_status(runtime: &BuiltinRuntime, downloads: &ModelDownloads) -> BuiltinCatalog {
    let store = runtime.store();
    let hardware = hardware();
    BuiltinCatalog {
        engine_available: runtime.executable().is_some(),
        models: store
            .models()
            .into_iter()
            .map(|model| {
                let size_bytes = model.size_bytes();
                BuiltinModelInfo {
                    role: model.role,
                    license: model.license.clone(),
                    memory_gb: model.memory_gb,
                    recommended: model.recommended,
                    size_bytes,
                    missing_bytes: store.missing_bytes(&model),
                    installed: store.is_installed(&model),
                    downloading: downloads.is_active(&model.id),
                    fit: model_fit(size_bytes, hardware),
                    curated: model.is_curated(),
                    repo: match &model.source {
                        ModelSource::Curated => None,
                        ModelSource::HuggingFace { repo, .. } => Some(repo.clone()),
                    },
                    name: model.name,
                    id: model.id,
                }
            })
            .collect(),
        models_directory: store.directory().display().to_string(),
        used_bytes: store.used_bytes(),
        running_servers: runtime.running(),
        hardware: hardware.clone(),
    }
}

#[tauri::command]
pub async fn get_builtin_ai_catalog(app: AppHandle) -> CommandResult<BuiltinCatalog> {
    blocking(app, |app| {
        let runtime = runtime()?;
        Ok(catalog_status(&runtime, &app.state::<ModelDownloads>()))
    })
    .await
}

/// Cerca su Hugging Face i repository GGUF adatti al ruolo. Invia a Hugging
/// Face solo il testo della ricerca, mai il contenuto delle note.
#[tauri::command]
pub async fn search_hub_models(
    app: AppHandle,
    query: String,
    role: ModelRole,
) -> CommandResult<Vec<HubRepo>> {
    blocking(app, move |_| {
        HubClient::new().search(&query, role).map_err(message)
    })
    .await
}

/// Varianti scaricabili di un repository, con quanto sono adatte.
#[tauri::command]
pub async fn get_hub_model_details(
    app: AppHandle,
    repo: String,
    role: ModelRole,
) -> CommandResult<HubRepoDetails> {
    blocking(app, move |_| {
        HubClient::new()
            .details(&repo, role, hardware())
            .map_err(message)
    })
    .await
}

/// Registra e scarica una variante scelta su Hugging Face. URL, dimensioni
/// e hash vengono riletti da Hugging Face, non presi dall'interfaccia.
#[tauri::command]
pub async fn start_hub_model_download(
    app: AppHandle,
    repo: String,
    role: ModelRole,
    model_id: String,
) -> CommandResult<()> {
    blocking(app, move |app| {
        let details = HubClient::new()
            .details(&repo, role, hardware())
            .map_err(message)?;
        let model = details
            .variants
            .into_iter()
            .map(|variant| variant.model)
            .find(|model| model.id == model_id)
            .ok_or("Variante non più disponibile su Hugging Face: aggiorna l'elenco")?;
        let runtime = runtime()?;
        // Un modello già registrato a un'altra revisione viene aggiornato:
        // i file della revisione precedente, se non condivisi, si eliminano.
        if let Some(previous) = runtime.store().find(&model.id)
            && previous != model
        {
            if app.state::<ModelDownloads>().is_active(&model.id) {
                return Err("Download già in corso".to_owned());
            }
            let others: Vec<CatalogModel> = runtime
                .store()
                .models()
                .into_iter()
                .filter(|other| other.id != previous.id)
                .collect();
            runtime.stop_all();
            runtime
                .store()
                .remove(&previous, &others)
                .map_err(message)?;
        }
        runtime.store().register(&model).map_err(message)?;
        begin_download(app, model)
    })
    .await
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadProgress {
    pub model_id: String,
    pub done: u64,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct DownloadFinished {
    pub model_id: String,
    pub error: Option<String>,
    pub cancelled: bool,
}

/// Avvia il download in background; avanzamento ed esito arrivano con gli
/// eventi `ai-model-download-progress` e `ai-model-download-finished`.
#[tauri::command]
pub fn start_builtin_model_download(app: AppHandle, model_id: String) -> CommandResult<()> {
    let model = runtime()?
        .store()
        .find(&model_id)
        .ok_or("Modello sconosciuto")?;
    begin_download(&app, model)
}

fn begin_download(app: &AppHandle, model: CatalogModel) -> CommandResult<()> {
    let runtime = runtime()?;
    let downloads = app.state::<ModelDownloads>();
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut active = downloads
            .active
            .lock()
            .map_err(|_| "Stato dei download non disponibile".to_owned())?;
        if active.contains_key(&model.id) {
            return Err("Download già in corso".to_owned());
        }
        // Due download non scrivono mai sullo stesso file.
        let shares_file = active
            .keys()
            .filter_map(|id| runtime.store().find(id))
            .any(|other| {
                other
                    .files()
                    .any(|file| model.files().any(|own| own.file_name == file.file_name))
            });
        if shares_file {
            return Err(
                "Un altro download in corso usa lo stesso file: attendi che finisca".to_owned(),
            );
        }
        active.insert(model.id.clone(), Arc::clone(&cancel));
    }
    let worker_app = app.clone();
    let model_id = model.id.clone();
    let spawned = thread::Builder::new()
        .name("ai-model-download".to_owned())
        .spawn(move || {
            let app = worker_app;
            let result = runtime
                .store()
                .download(&model, &cancel, &mut |done, total| {
                    let _ = app.emit(
                        DOWNLOAD_PROGRESS_EVENT,
                        DownloadProgress {
                            model_id: model.id.clone(),
                            done,
                            total,
                        },
                    );
                });
            if let Ok(mut active) = app.state::<ModelDownloads>().active.lock() {
                active.remove(&model.id);
            }
            if let Err(error) = &result {
                tracing::warn!("download del modello {} non riuscito: {error}", model.id);
            }
            let _ = app.emit(
                DOWNLOAD_FINISHED_EVENT,
                DownloadFinished {
                    model_id: model.id.clone(),
                    cancelled: matches!(result, Err(AiError::Cancelled)),
                    error: match result {
                        Ok(()) | Err(AiError::Cancelled) => None,
                        Err(error) => Some(message(error)),
                    },
                },
            );
        });
    if let Err(error) = spawned {
        if let Ok(mut active) = downloads.active.lock() {
            active.remove(&model_id);
        }
        return Err(format!("Download non avviato: {error}"));
    }
    Ok(())
}

/// Interrompe il download: il file parziale resta e un nuovo download
/// riprende da dove si era fermato.
#[tauri::command]
pub fn cancel_builtin_model_download(downloads: State<'_, ModelDownloads>, model_id: String) {
    if let Ok(active) = downloads.active.lock()
        && let Some(flag) = active.get(&model_id)
    {
        flag.store(true, Ordering::Relaxed);
    }
}

/// Elimina i file del modello (tranne quelli condivisi con altri modelli
/// scaricati) e, per i modelli di Hugging Face, lo toglie dall'elenco. Un
/// modello scelto nelle impostazioni salvate non si elimina.
#[tauri::command]
pub async fn delete_builtin_model(
    app: AppHandle,
    model_id: String,
) -> CommandResult<BuiltinCatalog> {
    blocking(app, move |app| {
        let runtime = runtime()?;
        let model = runtime
            .store()
            .find(&model_id)
            .ok_or("Modello sconosciuto")?;
        let downloads = app.state::<ModelDownloads>();
        if downloads.is_active(&model.id) {
            return Err("Interrompi il download prima di eliminare il modello".to_owned());
        }
        let settings = app.state::<Database>().ai_settings().map_err(message)?;
        let in_use = settings.provider == ProviderKind::Builtin
            && [
                &settings.chat_model,
                &settings.embedding_model,
                &settings.vision_model,
            ]
            .iter()
            .any(|selected| selected.as_str() == model.id);
        if in_use {
            return Err(
                "Il modello è in uso: scegline un altro nelle impostazioni AI e salva prima di eliminarlo"
                    .to_owned(),
            );
        }
        // Su Windows un file aperto da un server non si può eliminare.
        runtime.stop_all();
        let installed = runtime.store().installed();
        runtime.store().remove(&model, &installed).map_err(message)?;
        if !model.is_curated() {
            runtime.store().unregister(&model.id).map_err(message)?;
        }
        Ok(catalog_status(&runtime, &downloads))
    })
    .await
}
