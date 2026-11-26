//! Sincronizzazione tra dispositivi (ADR 0017): cartella condivisa oppure
//! Dropbox (ADR 0018). Impostazioni, collegamento a Dropbox nel browser,
//! esecuzione manuale e periodica.

use std::{
    net::{SocketAddr, TcpListener},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use greatnote_core::{
    calendar_sync::oauth::{Pkce, TokenSet, random_state},
    storage::Database,
    sync::{
        SyncReport, SyncSettings, SyncStatus, SyncTransport,
        crypto::LibraryKey,
        dropbox::{DropboxAuth, DropboxStore, REDIRECT_PORT},
        folder::{SyncAccess, SyncStore},
    },
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

type CommandResult<T> = Result<T, String>;

pub const SYNC_FINISHED_EVENT: &str = "sync-finished";
const INTERVAL: Duration = Duration::from_secs(120);
const FIRST_DELAY: Duration = Duration::from_secs(15);

/// App key Dropbox dell'editore, incorporata in fase di compilazione
/// (`oauth.env`, mai nel repository). Non è un segreto: con PKCE non serve
/// il segreto dell'app.
const DROPBOX_APP_KEY: Option<&str> = option_env!("GREATNOTE_DROPBOX_APP_KEY");
const KEYRING_SERVICE: &str = "greatnote-sync";
const DROPBOX_REFRESH_USER: &str = "dropbox-refresh";
/// Chiave della libreria cifrata (ADR 0032), mai la passphrase.
const LIBRARY_KEY_USER: &str = "library-key";

#[derive(Default)]
pub struct SyncRunState {
    running: AtomicBool,
    /// Token di accesso Dropbox in memoria (mai su disco) e scadenza.
    dropbox_token: Mutex<Option<(String, Instant)>>,
    connecting: Mutex<Option<Arc<AtomicBool>>>,
    /// Chiave della libreria cifrata, letta dal portachiavi alla prima
    /// sincronizzazione.
    library_key: Mutex<Option<LibraryKey>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncView {
    #[serde(flatten)]
    pub status: SyncStatus,
    pub running: bool,
    /// Questa versione include il collegamento a Dropbox.
    pub dropbox_available: bool,
    pub connecting: bool,
}

fn dropbox_app_key() -> Option<&'static str> {
    DROPBOX_APP_KEY.map(str::trim).filter(|key| {
        !key.is_empty() && key.len() <= 64 && key.chars().all(|c| c.is_ascii_alphanumeric())
    })
}

fn keyring_error(error: String) -> String {
    format!("Portachiavi: {error}")
}

fn read_refresh_token() -> CommandResult<Option<String>> {
    crate::secrets::read(KEYRING_SERVICE, DROPBOX_REFRESH_USER).map_err(keyring_error)
}

fn remember_token(state: &SyncRunState, tokens: &TokenSet) {
    let lifetime = Duration::from_secs(tokens.expires_in.saturating_sub(120).max(60));
    if let Ok(mut slot) = state.dropbox_token.lock() {
        *slot = Some((tokens.access_token.clone(), Instant::now() + lifetime));
    }
}

fn forget_token(state: &SyncRunState) {
    if let Ok(mut slot) = state.dropbox_token.lock() {
        *slot = None;
    }
}

/// Token di accesso valido: quello in memoria o uno nuovo dal token di
/// aggiornamento nel portachiavi.
fn dropbox_access_token(app: &AppHandle) -> CommandResult<String> {
    let state = app.state::<SyncRunState>();
    if let Ok(slot) = state.dropbox_token.lock()
        && let Some((token, expires)) = slot.as_ref()
        && *expires > Instant::now()
    {
        return Ok(token.clone());
    }
    let key = dropbox_app_key().ok_or_else(|| {
        greatnote_core::tr!(
            "Questa versione di Greatnote non include il collegamento a Dropbox",
            "This version of Greatnote does not include the Dropbox integration"
        )
    })?;
    let refresh = read_refresh_token()?.ok_or_else(|| {
        greatnote_core::tr!(
            "Accesso a Dropbox non trovato nel portachiavi: ricollega l'account",
            "Dropbox access not found in system keyring: reconnect the account"
        )
    })?;
    let tokens = DropboxAuth::new(key)
        .refresh(&refresh)
        .map_err(|error| match error {
            greatnote_core::calendar_sync::CalendarError::Unauthorized => {
                "Accesso a Dropbox revocato o scaduto: ricollega l'account".to_owned()
            }
            other => format!("Dropbox: {other}"),
        })?;
    remember_token(&state, &tokens);
    Ok(tokens.access_token)
}

/// Chiave della libreria cifrata: in memoria o dal portachiavi.
fn saved_library_key(state: &SyncRunState) -> Option<LibraryKey> {
    if let Ok(slot) = state.library_key.lock()
        && slot.is_some()
    {
        return slot.clone();
    }
    match crate::secrets::read(KEYRING_SERVICE, LIBRARY_KEY_USER) {
        Ok(Some(text)) => {
            let key = LibraryKey::from_hex(&text);
            if let Ok(mut slot) = state.library_key.lock() {
                slot.clone_from(&key);
            }
            key
        }
        Ok(None) => None,
        Err(error) => {
            tracing::warn!("chiave della sincronizzazione non letta dal portachiavi: {error}");
            None
        }
    }
}

/// Conserva (o dimentica) la chiave della libreria nel portachiavi, solo se
/// è cambiata.
fn store_library_key(state: &SyncRunState, key: Option<&LibraryKey>) -> CommandResult<()> {
    let current = state.library_key.lock().ok().and_then(|slot| slot.clone());
    if current.as_ref() == key {
        return Ok(());
    }
    match key {
        Some(key) => crate::secrets::write(KEYRING_SERVICE, LIBRARY_KEY_USER, &key.to_hex())
            .map_err(keyring_error)?,
        None => crate::secrets::delete(KEYRING_SERVICE, LIBRARY_KEY_USER).map_err(keyring_error)?,
    }
    if let Ok(mut slot) = state.library_key.lock() {
        *slot = key.cloned();
    }
    Ok(())
}

/// Evento per l'interfaccia: note da ricaricare ed eventuale errore.
#[derive(Debug, Clone, Serialize)]
struct SyncFinished {
    report: Option<SyncReport>,
    error: Option<String>,
}

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn view(app: &AppHandle) -> CommandResult<SyncView> {
    let state = app.state::<SyncRunState>();
    Ok(SyncView {
        status: app.state::<Database>().sync_status().map_err(message)?,
        running: state.running.load(Ordering::SeqCst),
        dropbox_available: dropbox_app_key().is_some(),
        connecting: state
            .connecting
            .lock()
            .map(|slot| slot.is_some())
            .unwrap_or(false),
    })
}

/// Archivio remoto della destinazione scelta (`None` per la cartella).
fn transport_store(app: &AppHandle) -> Result<Option<Box<dyn SyncStore>>, String> {
    let database = app.state::<Database>();
    let settings = database.sync_settings().map_err(message)?;
    Ok(match settings.transport {
        SyncTransport::Folder => None,
        SyncTransport::Dropbox => match dropbox_access_token(app) {
            Ok(token) => Some(Box::new(DropboxStore::new(&token))),
            Err(error) => {
                let _ = database.record_sync_error(&error);
                return Err(error);
            }
        },
    })
}

/// Sincronizzazione con la destinazione scelta. `passphrase` è quella
/// appena inserita dall'utente; altrimenti vale la chiave salvata.
fn sync_with_transport(app: &AppHandle, passphrase: Option<String>) -> Result<SyncReport, String> {
    let database = app.state::<Database>();
    let state = app.state::<SyncRunState>();
    let store = transport_store(app)?;
    let mut access = SyncAccess {
        key: saved_library_key(&state),
        passphrase,
        encrypt: false,
    };
    let result = database.sync_now_with(store, &mut access);
    match &result {
        // Libreria aperta: la chiave in uso (o nessuna) è quella da tenere.
        Ok(_) => store_library_key(&state, access.key.as_ref())?,
        // Sbloccata ma interrotta più avanti: la chiave resta valida.
        Err(_) if access.key.is_some() => store_library_key(&state, access.key.as_ref())?,
        Err(error) if error.is_unauthorized() => forget_token(&state),
        Err(_) => {}
    }
    result.map_err(message)
}

async fn blocking<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(&AppHandle) -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(move || work(&app))
        .await
        .map_err(|error| format!("Operazione interrotta: {error}"))?
}

/// Una sincronizzazione alla volta; l'esito va all'interfaccia.
fn run_once(app: &AppHandle, passphrase: Option<String>) -> Option<Result<SyncReport, String>> {
    let state = app.state::<SyncRunState>();
    if state.running.swap(true, Ordering::SeqCst) {
        return None;
    }
    let result = sync_with_transport(app, passphrase);
    state.running.store(false, Ordering::SeqCst);
    match &result {
        Ok(report) => {
            if report.imported + report.exported > 0
                || report.conflicts > 0
                || !report.warnings.is_empty()
            {
                tracing::info!(
                    exported = report.exported,
                    imported = report.imported,
                    conflicts = report.conflicts,
                    "sincronizzazione completata"
                );
            }
            let _ = app.emit(
                SYNC_FINISHED_EVENT,
                SyncFinished {
                    report: Some(report.clone()),
                    error: None,
                },
            );
        }
        Err(error) => {
            tracing::warn!("sincronizzazione non riuscita");
            let _ = app.emit(
                SYNC_FINISHED_EVENT,
                SyncFinished {
                    report: None,
                    error: Some(error.clone()),
                },
            );
        }
    }
    Some(result)
}

#[tauri::command]
pub async fn get_sync_status(app: AppHandle) -> CommandResult<SyncView> {
    blocking(app, view).await
}

/// Attiva, modifica o disattiva la sincronizzazione; attivandola parte
/// subito un primo giro, con la passphrase se l'utente l'ha inserita
/// (libreria nuova cifrata, conversione o accesso a una libreria cifrata).
/// Disattivandola la chiave della libreria cifrata viene tolta dal
/// portachiavi.
#[tauri::command]
pub async fn set_sync_settings(
    app: AppHandle,
    settings: SyncSettings,
    passphrase: Option<String>,
) -> CommandResult<SyncView> {
    blocking(app, move |app| {
        let saved = app
            .state::<Database>()
            .set_sync_settings(settings)
            .map_err(message)?;
        if !saved.enabled {
            store_library_key(&app.state::<SyncRunState>(), None)?;
        }
        if saved.enabled
            && let Some(Err(error)) = run_once(app, passphrase.filter(|p| !p.is_empty()))
        {
            return Err(error);
        }
        view(app)
    })
    .await
}

/// Sblocca una libreria cifrata con la passphrase (su questo dispositivo la
/// chiave resta poi nel portachiavi).
#[tauri::command]
pub async fn unlock_sync(app: AppHandle, passphrase: String) -> CommandResult<SyncView> {
    blocking(app, move |app| {
        if let Some(Err(error)) = run_once(app, Some(passphrase)) {
            return Err(error);
        }
        view(app)
    })
    .await
}

/// Cambia la passphrase della libreria cifrata. La chiave non cambia: gli
/// altri dispositivi continuano senza fare nulla, quelli nuovi useranno la
/// passphrase nuova.
#[tauri::command]
pub async fn change_sync_passphrase(
    app: AppHandle,
    current: Option<String>,
    passphrase: String,
) -> CommandResult<SyncView> {
    blocking(app, move |app| {
        let state = app.state::<SyncRunState>();
        let mut access = SyncAccess {
            key: saved_library_key(&state),
            passphrase: current.filter(|p| !p.is_empty()),
            encrypt: false,
        };
        let store = transport_store(app)?;
        app.state::<Database>()
            .change_sync_passphrase(store, &mut access, &passphrase)
            .map_err(message)?;
        store_library_key(&state, access.key.as_ref())?;
        tracing::info!("passphrase della sincronizzazione cambiata");
        view(app)
    })
    .await
}

#[tauri::command]
pub async fn sync_now(app: AppHandle) -> CommandResult<SyncView> {
    blocking(app, |app| {
        match run_once(app, None) {
            Some(Err(error)) => return Err(error),
            Some(Ok(_)) => {}
            // Già in corso in background: lo stato lo mostra.
            None => {}
        }
        view(app)
    })
    .await
}

#[tauri::command]
pub fn dismiss_sync_conflict(database: State<'_, Database>, id: String) -> CommandResult<bool> {
    database.dismiss_sync_conflict(&id).map_err(message)
}

/// Toglie un altro dispositivo dall'elenco (su tutti i dispositivi, alla
/// prossima sincronizzazione). Le sue modifiche restano nella libreria.
#[tauri::command]
pub fn retire_sync_device(app: AppHandle, device_id: String) -> CommandResult<SyncView> {
    if uuid::Uuid::parse_str(&device_id).is_err() {
        return Err(greatnote_core::tr!(
            "Dispositivo non valido",
            "Invalid device"
        ));
    }
    app.state::<Database>()
        .retire_sync_device(&device_id)
        .map_err(message)?;
    view(&app)
}

/// Dialog nativo per scegliere la cartella sincronizzata.
#[tauri::command]
pub async fn pick_sync_folder(app: AppHandle, title: String) -> CommandResult<Option<String>> {
    blocking(app, move |app| {
        let folder = crate::data_commands::pick_folder(crate::data_commands::dialog(app, &title))?;
        Ok(folder.map(|path| path.display().to_string()))
    })
    .await
}

// ---------------------------------------------------------------------------
// Dropbox

/// Ascolto sulla porta registrata nell'app Dropbox, su IPv4 e (se c'è) IPv6,
/// perché il browser può risolvere `localhost` in entrambi.
fn bind_redirect() -> CommandResult<Vec<TcpListener>> {
    let v4 = TcpListener::bind(("127.0.0.1", REDIRECT_PORT)).map_err(|error| {
        greatnote_core::tr!("La porta {REDIRECT_PORT} è occupata da un altro programma ({error}): chiudilo e riprova", "Port {REDIRECT_PORT} is occupied by another program ({error}): close it and try again")
    })?;
    let mut listeners = vec![v4];
    if let Ok(v6) = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], REDIRECT_PORT))) {
        listeners.push(v6);
    }
    Ok(listeners)
}

/// Collega l'account Dropbox nel browser e attiva la sincronizzazione su
/// Dropbox (le note di questo computer vengono unite a quelle già presenti).
#[tauri::command]
pub async fn connect_dropbox(app: AppHandle, device_name: String) -> CommandResult<SyncView> {
    blocking(app, move |app| {
        let key = dropbox_app_key().ok_or_else(|| {
            greatnote_core::tr!(
                "Questa versione di Greatnote non include il collegamento a Dropbox",
                "This version of Greatnote does not include the Dropbox integration"
            )
        })?;
        let state = app.state::<SyncRunState>();
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut slot = state.connecting.lock().map_err(message)?;
            if slot.is_some() {
                return Err(greatnote_core::tr!(
                    "Un collegamento è già in corso",
                    "A connection is already in progress"
                ));
            }
            *slot = Some(Arc::clone(&cancel));
        }
        let result: CommandResult<String> = (|| {
            let listeners = bind_redirect()?;
            let auth = DropboxAuth::new(key);
            let pkce = Pkce::new();
            let csrf = random_state();
            app.opener()
                .open_url(auth.authorization_url(&csrf, &pkce), None::<&str>)
                .map_err(|error| {
                    greatnote_core::tr!(
                        "Impossibile aprire il browser: {error}",
                        "Unable to open the browser: {error}"
                    )
                })?;
            let code = crate::calendar_commands::wait_for_code(&listeners, &csrf, &cancel)?;
            drop(listeners);
            // Dopo il login nel browser si torna a Greatnote.
            if let Some(window) = app.get_webview_window("main") {
                #[cfg(desktop)]
                let _ = window.unminimize();
                let _ = window.set_focus();
            }
            let tokens = auth.exchange_code(&code, &pkce.verifier).map_err(message)?;
            let refresh = tokens.refresh_token.clone().ok_or_else(|| {
                greatnote_core::tr!(
                    "Dropbox non ha concesso l'accesso continuativo: riprova il collegamento",
                    "Dropbox did not grant continuous access: retry the connection"
                )
            })?;
            let account = auth
                .current_account(&tokens.access_token)
                .map_err(message)?;
            crate::secrets::write(KEYRING_SERVICE, DROPBOX_REFRESH_USER, &refresh)
                .map_err(keyring_error)?;
            remember_token(&state, &tokens);
            Ok(account.label)
        })();
        if let Ok(mut slot) = state.connecting.lock() {
            *slot = None;
        }
        let label = result?;
        let previous = app.state::<Database>().sync_settings().map_err(message)?;
        app.state::<Database>()
            .set_sync_settings(SyncSettings {
                enabled: true,
                transport: SyncTransport::Dropbox,
                folder: previous.folder,
                dropbox_account: label,
                device_name,
                encrypted: previous.encrypted,
            })
            .map_err(message)?;
        tracing::info!("account Dropbox collegato");
        // La prima sincronizzazione può durare a lungo (tutte le note e gli
        // allegati): parte in background, l'esito arriva con l'evento
        // `sync-finished` e l'interfaccia mostra subito l'account collegato.
        let background = app.clone();
        let spawned = thread::Builder::new()
            .name("sync-first-dropbox".to_owned())
            .spawn(move || {
                run_once(&background, None);
            });
        if let Err(error) = spawned {
            tracing::warn!("prima sincronizzazione non avviata: {error}");
        }
        view(app)
    })
    .await
}

#[tauri::command]
pub fn cancel_dropbox_connect(state: State<'_, SyncRunState>) {
    if let Ok(slot) = state.connecting.lock()
        && let Some(cancel) = slot.as_ref()
    {
        cancel.store(true, Ordering::SeqCst);
    }
}

/// Scollega Dropbox: revoca l'accesso presso Dropbox, lo cancella dal
/// portachiavi e disattiva la sincronizzazione. Note e file su Dropbox
/// restano dove sono.
#[tauri::command]
pub async fn disconnect_dropbox(app: AppHandle) -> CommandResult<SyncView> {
    blocking(app, |app| {
        if let (Some(key), Ok(token)) = (dropbox_app_key(), dropbox_access_token(app))
            && let Err(error) = DropboxAuth::new(key).revoke(&token)
        {
            tracing::warn!(%error, "revoca dell'accesso Dropbox non riuscita");
        }
        crate::secrets::delete(KEYRING_SERVICE, DROPBOX_REFRESH_USER).map_err(keyring_error)?;
        forget_token(&app.state::<SyncRunState>());
        let database = app.state::<Database>();
        let previous = database.sync_settings().map_err(message)?;
        database
            .set_sync_settings(SyncSettings {
                enabled: false,
                dropbox_account: String::new(),
                ..previous
            })
            .map_err(message)?;
        store_library_key(&app.state::<SyncRunState>(), None)?;
        view(app)
    })
    .await
}

/// Giri automatici in background, solo con la sincronizzazione attiva.
pub fn spawn_sync_loop(app: &AppHandle) {
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(FIRST_DELAY);
        loop {
            let enabled = app
                .state::<Database>()
                .sync_settings()
                .map(|settings| settings.enabled)
                .unwrap_or(false);
            if enabled {
                run_once(&app, None);
            }
            thread::sleep(INTERVAL);
        }
    });
}
