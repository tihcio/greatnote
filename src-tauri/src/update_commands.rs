//! Aggiornamenti dell'app dall'interno (ADR 0023).
//!
//! Il plugin updater di Tauri legge `latest.json` dalle release pubbliche
//! del progetto, scarica il pacchetto e ne verifica la firma minisign con la
//! chiave pubblica inclusa nell'app (e la versione firmata, contro i
//! ritorni a versioni vecchie). Il pacchetto scaricato resta in memoria e
//! viene installato quando l'utente sceglie «Riavvia e aggiorna» oppure alla
//! chiusura dell'app. Nelle build di sviluppo gli aggiornamenti sono
//! disattivati.
//!
//! Su Linux il plugin aggiorna AppImage, `.deb` e `.rpm` (voci
//! `linux-x86_64-<formato>` di `latest.json`). I pacchetti di sistema si
//! installano con `pkexec` e chiedono la password di amministratore: solo
//! con «Riavvia e aggiorna», mai alla chiusura, quando una richiesta di
//! password comparirebbe a finestra già chiusa.

use std::{
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use greatnote_core::{storage::Database, time, updates::UpdateSettings};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};

type CommandResult<T> = Result<T, String>;

/// Evento inviato al frontend a ogni cambio di stato.
pub const STATUS_EVENT: &str = "update-status";
/// Primo controllo dopo l'avvio, per non rallentarlo.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(60);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// Avanzamento del download inviato al più ogni 256 KiB.
const PROGRESS_STEP: u64 = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum UpdatePhase {
    /// Nessun controllo ancora eseguito.
    Idle,
    Checking,
    UpToDate,
    Downloading {
        version: String,
        downloaded: u64,
        total: Option<u64>,
    },
    /// Scaricato e verificato: si installa alla chiusura o su richiesta.
    Ready {
        version: String,
        notes: Option<String>,
    },
    Installing,
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub current_version: String,
    /// Falso nelle build di sviluppo.
    pub supported: bool,
    pub auto_update: bool,
    /// Il pacchetto pronto si installa da solo alla chiusura (falso per
    /// `.deb` e `.rpm`, che chiedono la password).
    pub installs_on_exit: bool,
    pub last_check_at: Option<String>,
    pub phase: UpdatePhase,
}

struct Pending {
    update: Update,
    bytes: Vec<u8>,
}

#[derive(Default)]
struct Inner {
    phase: Option<UpdatePhase>,
    last_check_at: Option<String>,
    pending: Option<Pending>,
}

#[derive(Default)]
pub struct UpdateState {
    inner: Mutex<Inner>,
    /// Un solo controllo o download alla volta.
    busy: AtomicBool,
}

impl UpdateState {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        // Lo stato resta utilizzabile anche dopo un panic in un altro thread.
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Gli aggiornamenti funzionano solo nelle build di rilascio: una build di
/// sviluppo verrebbe sostituita dall'ultima versione pubblicata.
fn supported() -> bool {
    !cfg!(debug_assertions)
}

/// Installazione alla chiusura senza interazione: ovunque tranne i pacchetti
/// di sistema Linux.
fn installs_on_exit() -> bool {
    #[cfg(target_os = "linux")]
    {
        use tauri::utils::{config::BundleType, platform::bundle_type};
        !matches!(bundle_type(), Some(BundleType::Deb | BundleType::Rpm))
    }
    #[cfg(not(target_os = "linux"))]
    {
        true
    }
}

fn auto_update(app: &AppHandle) -> bool {
    app.state::<Database>()
        .update_settings()
        .map(|settings| settings.auto_update)
        .unwrap_or(true)
}

fn status(app: &AppHandle) -> UpdateStatus {
    let state = app.state::<UpdateState>();
    let inner = state.lock();
    UpdateStatus {
        current_version: app.package_info().version.to_string(),
        supported: supported(),
        auto_update: auto_update(app),
        installs_on_exit: installs_on_exit(),
        last_check_at: inner.last_check_at.clone(),
        phase: inner.phase.clone().unwrap_or(UpdatePhase::Idle),
    }
}

fn set_phase(app: &AppHandle, phase: UpdatePhase) {
    app.state::<UpdateState>().lock().phase = Some(phase);
    if let Err(error) = app.emit(STATUS_EVENT, status(app)) {
        tracing::warn!("stato dell'aggiornamento non inviato: {error}");
    }
}

/// Messaggio comprensibile per gli errori del plugin.
fn describe(error: &tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error;
    match error {
        Error::TargetNotFound(_) | Error::TargetsNotFound(_) => {
            "Non ci sono aggiornamenti automatici per questo tipo di installazione".to_owned()
        }
        Error::ReleaseNotFound => {
            "Nessuna versione pubblicata trovata: riprova più tardi".to_owned()
        }
        Error::PackageInstallFailed => {
            "Installazione annullata o non riuscita: serve la password di amministratore".to_owned()
        }
        Error::Reqwest(_) | Error::Network(_) => {
            "Server degli aggiornamenti non raggiungibile: controlla la connessione".to_owned()
        }
        Error::Minisign(_)
        | Error::SignatureUtf8(_)
        | Error::Base64(_)
        | Error::SignedVersionMismatch { .. }
        | Error::MissingSignedVersion => {
            "La firma dell'aggiornamento non è valida: l'aggiornamento è stato scartato".to_owned()
        }
        other => format!("Aggiornamento non riuscito: {other}"),
    }
}

/// Controlla se c'è una versione nuova e, se c'è, la scarica e la verifica.
/// Restituisce subito se un altro controllo è in corso.
async fn check_and_download(app: &AppHandle) {
    let state = app.state::<UpdateState>();
    if state.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    // Un pacchetto già pronto non va riscaricato.
    let ready = state.lock().pending.is_some();
    if !ready {
        run_check(app).await;
    }
    state.busy.store(false, Ordering::SeqCst);
}

async fn run_check(app: &AppHandle) {
    set_phase(app, UpdatePhase::Checking);
    let checked = match app.updater() {
        Ok(updater) => updater.check().await,
        Err(error) => Err(error),
    };
    app.state::<UpdateState>().lock().last_check_at = Some(time::format_utc(time::now()));
    let update = match checked {
        Ok(Some(update)) => update,
        Ok(None) => {
            set_phase(app, UpdatePhase::UpToDate);
            return;
        }
        Err(error) => {
            tracing::warn!("controllo degli aggiornamenti non riuscito: {error}");
            set_phase(
                app,
                UpdatePhase::Error {
                    message: describe(&error),
                },
            );
            return;
        }
    };

    let version = update.version.clone();
    tracing::info!(version = %version, "nuova versione disponibile, download in corso");
    set_phase(
        app,
        UpdatePhase::Downloading {
            version: version.clone(),
            downloaded: 0,
            total: None,
        },
    );
    let mut downloaded = 0u64;
    let mut reported = 0u64;
    // `download` verifica la firma prima di restituire il pacchetto.
    let result = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                if downloaded - reported >= PROGRESS_STEP {
                    reported = downloaded;
                    set_phase(
                        app,
                        UpdatePhase::Downloading {
                            version: version.clone(),
                            downloaded,
                            total,
                        },
                    );
                }
            },
            || {},
        )
        .await;
    match result {
        Ok(bytes) => {
            let notes = update.body.clone().filter(|body| !body.trim().is_empty());
            app.state::<UpdateState>().lock().pending = Some(Pending { update, bytes });
            tracing::info!(version = %version, "aggiornamento scaricato e verificato");
            set_phase(app, UpdatePhase::Ready { version, notes });
        }
        Err(error) => {
            tracing::warn!("download dell'aggiornamento non riuscito: {error}");
            set_phase(
                app,
                UpdatePhase::Error {
                    message: describe(&error),
                },
            );
        }
    }
}

/// Controllo periodico in background, se gli aggiornamenti automatici sono
/// attivi.
pub fn spawn_update_checker(app: &AppHandle) {
    if !supported() {
        return;
    }
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(FIRST_CHECK_DELAY);
        loop {
            if auto_update(&app) {
                tauri::async_runtime::block_on(check_and_download(&app));
            }
            thread::sleep(CHECK_INTERVAL);
        }
    });
}

/// Alla chiusura dell'app installa il pacchetto pronto, senza riaprirla.
/// Su Windows l'installer parte in modalità passiva e il processo termina.
pub fn install_on_exit(app: &AppHandle) {
    if !installs_on_exit() {
        // `.deb`/`.rpm`: resta per «Riavvia e aggiorna» al prossimo avvio
        // (verrà riscaricato), senza una richiesta di password a sorpresa.
        return;
    }
    let Some(state) = app.try_state::<UpdateState>() else {
        return;
    };
    let Some(Pending { update, bytes }) = state.lock().pending.take() else {
        return;
    };
    tracing::info!(version = %update.version, "installazione dell'aggiornamento alla chiusura");
    if let Err(error) = update.restart_after_install(false).install(bytes) {
        tracing::error!("installazione dell'aggiornamento non riuscita: {error}");
    }
}

#[tauri::command]
pub fn get_update_status(app: AppHandle) -> UpdateStatus {
    status(&app)
}

/// Controllo richiesto dall'utente (anche con gli aggiornamenti automatici
/// disattivati): se c'è una versione nuova la scarica.
#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> CommandResult<UpdateStatus> {
    if !supported() {
        return Err("Gli aggiornamenti non sono disponibili nella versione di sviluppo".to_owned());
    }
    check_and_download(&app).await;
    Ok(status(&app))
}

#[tauri::command]
pub fn set_auto_update(app: AppHandle, enabled: bool) -> CommandResult<UpdateStatus> {
    app.state::<Database>()
        .set_update_settings(&UpdateSettings {
            auto_update: enabled,
        })
        .map_err(|error| error.to_string())?;
    let status = status(&app);
    if let Err(error) = app.emit(STATUS_EVENT, status.clone()) {
        tracing::warn!("stato dell'aggiornamento non inviato: {error}");
    }
    Ok(status)
}

/// Installa subito il pacchetto pronto e riavvia l'app. Il frontend salva
/// le bozze prima di chiamarlo.
#[tauri::command]
pub fn install_update_now(app: AppHandle) -> CommandResult<()> {
    let pending = app.state::<UpdateState>().lock().pending.take();
    let Some(Pending { update, bytes }) = pending else {
        return Err("Nessun aggiornamento pronto da installare".to_owned());
    };
    set_phase(&app, UpdatePhase::Installing);
    tracing::info!(version = %update.version, "installazione dell'aggiornamento richiesta");
    // Il motore AI integrato va fermato prima che il processo termini.
    crate::builtin_ai_commands::shutdown();
    // Su Windows `install` avvia l'installer, che riapre l'app, e termina il
    // processo; altrove sostituisce i file e l'app va riavviata.
    if let Err(error) = update.install(&bytes) {
        tracing::error!("installazione dell'aggiornamento non riuscita: {error}");
        let message = describe(&error);
        // Password annullata o errore: il pacchetto resta pronto per
        // riprovare, senza scaricarlo di nuovo.
        let version = update.version.clone();
        let notes = update.body.clone().filter(|body| !body.trim().is_empty());
        app.state::<UpdateState>().lock().pending = Some(Pending { update, bytes });
        set_phase(&app, UpdatePhase::Ready { version, notes });
        return Err(message);
    }
    app.restart();
}
