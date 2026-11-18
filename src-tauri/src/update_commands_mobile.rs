//! Aggiornamenti su Android (stesso contratto con l'interfaccia del
//! desktop, `update_commands.rs`).
//!
//! Il plugin updater di Tauri non esiste per Android. Il controllo legge lo
//! stesso `latest.json` delle release (indirizzo in `plugins.updater` di
//! `tauri.conf.json`) e, se la versione è più recente, scarica
//! `Greatnote.arm64.apk` dalla stessa release nella cache dell'app
//! (`greatnote_core::updates::package`). «Aggiorna» apre l'installatore di
//! Android: l'utente conferma, e Android accetta il pacchetto solo se è
//! firmato con la stessa chiave dell'app installata.
//!
//! Le build di debug non si aggiornano (sarebbero sostituite da una build
//! firmata con un'altra chiave, che Android rifiuta comunque). Su iOS gli
//! aggiornamenti passano dall'App Store: i comandi rispondono «non
//! disponibile».

use std::{
    fs,
    path::PathBuf,
    sync::{
        Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::Duration,
};

use greatnote_core::{
    storage::Database,
    time,
    updates::{
        UpdateSettings,
        package::{self, PackageUpdateError},
    },
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

type CommandResult<T> = Result<T, String>;

/// Evento inviato al frontend a ogni cambio di stato.
pub const STATUS_EVENT: &str = "update-status";
/// Sul telefono l'app resta aperta poco: il primo controllo è presto.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(15);
const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// Avanzamento del download inviato al più ogni 256 KiB.
const PROGRESS_STEP: u64 = 256 * 1024;
/// Cartella dei pacchetti scaricati, dentro la cache dell'app.
const CACHE_DIRECTORY: &str = "update";
/// Nome del pacchetto allegato alle release (job `android` di
/// `.github/workflows/installers.yml`).
const RELEASE_PACKAGE: &str = "Greatnote.arm64.apk";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum UpdatePhase {
    Idle,
    Checking,
    UpToDate,
    Downloading {
        version: String,
        downloaded: u64,
        total: Option<u64>,
    },
    /// Scaricato: «Aggiorna» apre l'installatore di Android.
    Ready {
        version: String,
        notes: Option<String>,
    },
    // Nessuna fase «installing»: l'installazione la fa l'installatore di
    // Android, fuori dall'app.
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub current_version: String,
    pub supported: bool,
    pub managed_by_store: bool,
    pub auto_update: bool,
    /// Su Android l'installazione chiede sempre conferma: mai alla chiusura.
    pub installs_on_exit: bool,
    pub last_check_at: Option<String>,
    pub phase: UpdatePhase,
}

#[derive(Default)]
struct Inner {
    phase: Option<UpdatePhase>,
    last_check_at: Option<String>,
    /// Pacchetto pronto da installare.
    ready: Option<PathBuf>,
}

#[derive(Default)]
pub struct UpdateState {
    inner: Mutex<Inner>,
    /// Un solo controllo o download alla volta.
    busy: AtomicBool,
}

impl UpdateState {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn supported() -> bool {
    cfg!(target_os = "android") && !cfg!(debug_assertions)
}

fn not_available() -> String {
    if cfg!(target_os = "android") {
        greatnote_core::tr!(
            "Gli aggiornamenti non sono disponibili nella versione di sviluppo",
            "Updates are not available in the development version"
        )
    } else {
        greatnote_core::tr!(
            "Gli aggiornamenti dall'app non sono disponibili su questo dispositivo",
            "In-app updates are not available on this device"
        )
    }
}

fn auto_update(app: &AppHandle) -> bool {
    app.state::<Database>()
        .update_settings()
        .map(|settings| settings.auto_update)
        .unwrap_or(true)
}

fn current_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

fn status(app: &AppHandle) -> UpdateStatus {
    let state = app.state::<UpdateState>();
    let inner = state.lock();
    UpdateStatus {
        current_version: current_version(app),
        supported: supported(),
        // Su Android l'app si aggiorna da sé con l'APK della release; con una
        // versione distribuita da Google Play sarebbe lo store a farlo.
        managed_by_store: false,
        auto_update: auto_update(app),
        installs_on_exit: false,
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

/// Indirizzo di `latest.json`, lo stesso del desktop.
fn manifest_url(app: &AppHandle) -> Option<String> {
    app.config()
        .plugins
        .0
        .get("updater")?
        .get("endpoints")?
        .get(0)?
        .as_str()
        .map(str::to_owned)
}

fn cache_directory(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_cache_dir()
        .ok()
        .map(|cache| cache.join(CACHE_DIRECTORY))
}

fn cached_package(directory: &std::path::Path, version: &str) -> PathBuf {
    directory.join(format!("Greatnote-{version}.apk"))
}

/// Versione di un pacchetto conservato, dal nome `Greatnote-<versione>.apk`.
fn cached_version(file_name: &str) -> Option<&str> {
    file_name.strip_prefix("Greatnote-")?.strip_suffix(".apk")
}

/// All'avvio: un pacchetto più recente già scaricato è subito pronto; quelli
/// della versione installata (o precedenti) vengono eliminati.
fn restore_cached(app: &AppHandle) {
    let Some(directory) = cache_directory(app) else {
        return;
    };
    let Ok(entries) = fs::read_dir(&directory) else {
        return;
    };
    let current = current_version(app);
    let mut best: Option<(String, PathBuf)> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        match cached_version(&name) {
            Some(version) if package::is_newer(version, &current) => {
                let newer = best
                    .as_ref()
                    .is_none_or(|(known, _)| package::is_newer(version, known));
                if newer {
                    if let Some((_, old)) = best.take() {
                        let _ = fs::remove_file(old);
                    }
                    best = Some((version.to_owned(), path));
                } else {
                    let _ = fs::remove_file(path);
                }
            }
            _ => {
                let _ = fs::remove_file(path);
            }
        }
    }
    if let Some((version, path)) = best {
        app.state::<UpdateState>().lock().ready = Some(path);
        set_phase(
            app,
            UpdatePhase::Ready {
                version,
                notes: None,
            },
        );
    }
}

fn describe(error: &PackageUpdateError) -> String {
    error.to_string()
}

/// Controlla e, se c'è una versione nuova, la scarica. Bloccante: va
/// chiamata fuori dal thread dell'interfaccia.
fn check_and_download(app: &AppHandle) {
    let state = app.state::<UpdateState>();
    if state.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    run_check(app);
    state.busy.store(false, Ordering::SeqCst);
}

fn run_check(app: &AppHandle) {
    let Some(url) = manifest_url(app) else {
        set_phase(
            app,
            UpdatePhase::Error {
                message: greatnote_core::tr!(
                    "Indirizzo degli aggiornamenti non configurato",
                    "Update address not configured"
                ),
            },
        );
        return;
    };
    set_phase(app, UpdatePhase::Checking);
    let result = package::fetch_latest(&url);
    app.state::<UpdateState>().lock().last_check_at = Some(time::format_utc(time::now()));
    let release = match result {
        Ok(release) => release,
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
    if !package::is_newer(&release.version, &current_version(app)) {
        set_phase(app, UpdatePhase::UpToDate);
        return;
    }
    let Some(directory) = cache_directory(app) else {
        return;
    };
    let destination = cached_package(&directory, &release.version);
    if destination.exists() {
        app.state::<UpdateState>().lock().ready = Some(destination);
        set_phase(
            app,
            UpdatePhase::Ready {
                version: release.version,
                notes: release.notes,
            },
        );
        return;
    }
    let Some(package_url) = package::package_url(&url, &release.version, RELEASE_PACKAGE) else {
        set_phase(
            app,
            UpdatePhase::Error {
                message: greatnote_core::tr!(
                    "Indirizzo degli aggiornamenti non valido",
                    "Invalid update address"
                ),
            },
        );
        return;
    };
    set_phase(
        app,
        UpdatePhase::Downloading {
            version: release.version.clone(),
            downloaded: 0,
            total: None,
        },
    );
    let mut reported = 0_u64;
    let downloaded = package::download_package(
        &package_url,
        &release.version,
        &destination,
        |downloaded, total| {
            if downloaded - reported >= PROGRESS_STEP || Some(downloaded) == total {
                reported = downloaded;
                set_phase(
                    app,
                    UpdatePhase::Downloading {
                        version: release.version.clone(),
                        downloaded,
                        total,
                    },
                );
            }
        },
    );
    match downloaded {
        Ok(size) => {
            tracing::info!(version = %release.version, size, "aggiornamento scaricato");
            // Restano solo il pacchetto nuovo e nessuno dei precedenti.
            if let Ok(entries) = fs::read_dir(&directory) {
                for entry in entries.flatten() {
                    if entry.path() != destination {
                        let _ = fs::remove_file(entry.path());
                    }
                }
            }
            app.state::<UpdateState>().lock().ready = Some(destination);
            set_phase(
                app,
                UpdatePhase::Ready {
                    version: release.version,
                    notes: release.notes,
                },
            );
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

pub fn spawn_update_checker(app: &AppHandle) {
    if !supported() {
        return;
    }
    let app = app.clone();
    thread::spawn(move || {
        restore_cached(&app);
        thread::sleep(FIRST_CHECK_DELAY);
        loop {
            if auto_update(&app) {
                check_and_download(&app);
            }
            thread::sleep(CHECK_INTERVAL);
        }
    });
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
        return Err(not_available());
    }
    let worker = app.clone();
    tauri::async_runtime::spawn_blocking(move || check_and_download(&worker))
        .await
        .map_err(|error| error.to_string())?;
    Ok(status(&app))
}

#[tauri::command]
pub fn set_auto_update(app: AppHandle, enabled: bool) -> CommandResult<UpdateStatus> {
    if !supported() {
        return Err(not_available());
    }
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

/// Apre l'installatore di Android con il pacchetto pronto. Il frontend salva
/// le bozze prima di chiamarlo: installando, Android chiude l'app.
#[tauri::command]
pub fn install_update_now(app: AppHandle) -> CommandResult<()> {
    if !supported() {
        return Err(not_available());
    }
    let ready = app.state::<UpdateState>().lock().ready.clone();
    let Some(path) = ready.filter(|path| path.exists()) else {
        return Err(greatnote_core::tr!(
            "Nessun aggiornamento pronto da installare",
            "No update ready to install"
        ));
    };
    // La fase resta «pronto»: se l'utente annulla l'installatore può
    // riprovare senza scaricare di nuovo.
    crate::file_access::install_package(&app, &path)
}

#[cfg(test)]
mod tests {
    use super::cached_version;

    #[test]
    fn cached_package_names_carry_the_version() {
        assert_eq!(cached_version("Greatnote-0.2.8.apk"), Some("0.2.8"));
        assert_eq!(cached_version("Greatnote-0.2.8.part"), None);
        assert_eq!(cached_version("altro.apk"), None);
    }
}
