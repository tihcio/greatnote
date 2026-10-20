//! Calendari esterni (Google, Microsoft): collegamento con OAuth nel
//! browser di sistema, token nel portachiavi, sincronizzazione periodica e
//! modifiche agli eventi tramite le API del fornitore (ADR 0015).

use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use greatnote_core::{
    calendar_sync::{
        self, CalendarAccount, CalendarClients, CalendarError, CalendarService, Provider,
        google::GoogleCalendar,
        microsoft::MicrosoftCalendar,
        oauth::{
            Callback, OAuthClient, Pkce, TokenSet, parse_callback, random_state, redirect_uri,
        },
        subscriptions::{
            CalendarSubscription, apply_ics_now, calendar_name, fetch_ics, normalize_ics_url,
            validate_ics,
        },
    },
    domain::planner::{CalendarEvent, EventInput},
    storage::Database,
    time,
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

type CommandResult<T> = Result<T, String>;

const KEYRING_SERVICE: &str = "greatnote-calendar";
const GOOGLE_SECRET_USER: &str = "google-client-secret";
const API_TIMEOUT: Duration = Duration::from_secs(60);
/// Tempo concesso per completare l'accesso nel browser.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const SYNC_INTERVAL: Duration = Duration::from_secs(15 * 60);
const FIRST_SYNC_DELAY: Duration = Duration::from_secs(20);
pub const SYNC_FINISHED_EVENT: &str = "calendar-sync-finished";

/// Token di accesso in memoria (mai su disco) e stato delle operazioni.
#[derive(Default)]
pub struct CalendarState {
    access: Mutex<HashMap<String, (String, Instant)>>,
    connecting: Mutex<Option<Arc<AtomicBool>>>,
    syncing: AtomicBool,
}

fn message(error: impl std::fmt::Display) -> String {
    error.to_string()
}

// ---------------------------------------------------------------------------
// Portachiavi

fn keyring_entry(user: &str) -> CommandResult<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, user).map_err(|error| format!("Portachiavi: {error}"))
}

fn read_secret(user: &str) -> CommandResult<Option<String>> {
    match keyring_entry(user)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(format!("Portachiavi: {error}")),
    }
}

fn write_secret(user: &str, value: &str) -> CommandResult<()> {
    keyring_entry(user)?
        .set_password(value)
        .map_err(|error| format!("Portachiavi: {error}"))
}

fn delete_secret(user: &str) -> CommandResult<()> {
    match keyring_entry(user)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(error) => Err(format!("Portachiavi: {error}")),
    }
}

fn refresh_user(account_id: &str) -> String {
    format!("account-{account_id}")
}

// ---------------------------------------------------------------------------
// Client OAuth e servizi

/// Registrazioni dell'editore incorporate nella build (variabili d'ambiente
/// al momento della compilazione, mai nel repository). Con queste l'utente
/// deve solo accedere e consentire.
const BUILTIN_GOOGLE_ID: Option<&str> = option_env!("GREATNOTE_GOOGLE_CLIENT_ID");
const BUILTIN_GOOGLE_SECRET: Option<&str> = option_env!("GREATNOTE_GOOGLE_CLIENT_SECRET");
const BUILTIN_MICROSOFT_ID: Option<&str> = option_env!("GREATNOTE_MICROSOFT_CLIENT_ID");

/// ID (e segreto Google) incorporati per un fornitore, se presenti e validi.
fn builtin_client(
    provider: Provider,
    google_id: Option<&'static str>,
    google_secret: Option<&'static str>,
    microsoft_id: Option<&'static str>,
) -> Option<(&'static str, Option<&'static str>)> {
    let clean =
        |value: Option<&'static str>| value.map(str::trim).filter(|value| !value.is_empty());
    let (id, secret) = match provider {
        Provider::Google => (clean(google_id)?, clean(google_secret)),
        Provider::Microsoft => (clean(microsoft_id)?, None),
    };
    let clients = match provider {
        Provider::Google => CalendarClients {
            google_client_id: id.to_owned(),
            ..CalendarClients::default()
        },
        Provider::Microsoft => CalendarClients {
            microsoft_client_id: id.to_owned(),
            ..CalendarClients::default()
        },
    };
    clients.validated().ok().map(|_| (id, secret))
}

fn builtin(provider: Provider) -> Option<(&'static str, Option<&'static str>)> {
    builtin_client(
        provider,
        BUILTIN_GOOGLE_ID,
        BUILTIN_GOOGLE_SECRET,
        BUILTIN_MICROSOFT_ID,
    )
}

/// Client OAuth da usare: la registrazione personale inserita nelle
/// impostazioni avanzate, se presente, altrimenti quella incorporata.
fn oauth_client(database: &Database, provider: Provider) -> CommandResult<OAuthClient> {
    let clients = database.calendar_clients().map_err(message)?;
    if let Some(client_id) = clients.client_id(provider) {
        let secret = match provider {
            Provider::Google => read_secret(GOOGLE_SECRET_USER)?,
            Provider::Microsoft => None,
        };
        return Ok(OAuthClient::new(provider, client_id, secret.as_deref()));
    }
    let (client_id, secret) = builtin(provider).ok_or_else(|| {
        "Questa versione di Greatnote non include il collegamento a questo fornitore: inserisci una tua registrazione in Impostazioni › Integrazioni › Avanzate"
            .to_owned()
    })?;
    Ok(OAuthClient::new(provider, client_id, secret))
}

fn service(provider: Provider, token: &str) -> Box<dyn CalendarService> {
    match provider {
        Provider::Google => Box::new(GoogleCalendar::new(token, API_TIMEOUT)),
        Provider::Microsoft => Box::new(MicrosoftCalendar::new(token, API_TIMEOUT)),
    }
}

fn remember_token(state: &CalendarState, account_id: &str, tokens: &TokenSet) {
    // Margine di un minuto prima della scadenza dichiarata.
    let lifetime = Duration::from_secs(tokens.expires_in.saturating_sub(60).max(30));
    if let Ok(mut access) = state.access.lock() {
        access.insert(
            account_id.to_owned(),
            (tokens.access_token.clone(), Instant::now() + lifetime),
        );
    }
}

/// Servizio con un token valido: quello in memoria o uno nuovo ottenuto con
/// il token di aggiornamento (salvando quello ruotato dal fornitore).
fn account_service(
    app: &AppHandle,
    account: &CalendarAccount,
) -> CommandResult<Box<dyn CalendarService>> {
    let state = app.state::<CalendarState>();
    if let Ok(access) = state.access.lock()
        && let Some((token, expires)) = access.get(&account.id)
        && *expires > Instant::now()
    {
        return Ok(service(account.provider, token));
    }
    let database = app.state::<Database>();
    let refresh_token = read_secret(&refresh_user(&account.id))?
        .ok_or_else(|| "Accesso non trovato nel portachiavi: ricollega l'account".to_owned())?;
    let client = oauth_client(&database, account.provider)?;
    let tokens = client.refresh(&refresh_token).map_err(message)?;
    if let Some(rotated) = &tokens.refresh_token
        && rotated != &refresh_token
    {
        write_secret(&refresh_user(&account.id), rotated)?;
    }
    remember_token(&state, &account.id, &tokens);
    Ok(service(account.provider, &tokens.access_token))
}

fn forget_token(app: &AppHandle, account_id: &str) {
    if let Ok(mut access) = app.state::<CalendarState>().access.lock() {
        access.remove(account_id);
    }
}

// ---------------------------------------------------------------------------
// Stato per le impostazioni

#[derive(Debug, Serialize)]
pub struct CalendarStatus {
    pub clients: CalendarClients,
    pub google_has_secret: bool,
    /// Registrazioni incorporate nella build: basta accedere e consentire.
    pub builtin_google: bool,
    pub builtin_microsoft: bool,
    pub accounts: Vec<CalendarAccount>,
    /// Abbonamenti iCal in sola lettura.
    pub subscriptions: Vec<CalendarSubscription>,
    pub syncing: bool,
    pub connecting: bool,
}

fn status(app: &AppHandle) -> CommandResult<CalendarStatus> {
    let database = app.state::<Database>();
    let state = app.state::<CalendarState>();
    Ok(CalendarStatus {
        clients: database.calendar_clients().map_err(message)?,
        google_has_secret: read_secret(GOOGLE_SECRET_USER).ok().flatten().is_some(),
        builtin_google: builtin(Provider::Google).is_some(),
        builtin_microsoft: builtin(Provider::Microsoft).is_some(),
        accounts: database.calendar_accounts().map_err(message)?,
        subscriptions: database.calendar_subscriptions().map_err(message)?,
        syncing: state.syncing.load(Ordering::SeqCst),
        connecting: state
            .connecting
            .lock()
            .map(|slot| slot.is_some())
            .unwrap_or(false),
    })
}

async fn blocking<T: Send + 'static>(
    app: AppHandle,
    work: impl FnOnce(&AppHandle) -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(move || work(&app))
        .await
        .map_err(|error| format!("Operazione interrotta: {error}"))?
}

#[tauri::command]
pub async fn get_calendar_status(app: AppHandle) -> CommandResult<CalendarStatus> {
    blocking(app, status).await
}

/// `google_client_secret`: `None` lascia quello salvato, vuoto lo elimina.
#[tauri::command]
pub async fn set_calendar_clients(
    app: AppHandle,
    clients: CalendarClients,
    google_client_secret: Option<String>,
) -> CommandResult<CalendarStatus> {
    blocking(app, move |app| {
        app.state::<Database>()
            .set_calendar_clients(clients)
            .map_err(message)?;
        match google_client_secret.as_deref().map(str::trim) {
            None => {}
            Some("") => delete_secret(GOOGLE_SECRET_USER)?,
            Some(secret) if secret.len() <= 200 && !secret.contains(char::is_whitespace) => {
                write_secret(GOOGLE_SECRET_USER, secret)?
            }
            Some(_) => return Err("Segreto client di Google non valido".to_owned()),
        }
        status(app)
    })
    .await
}

// ---------------------------------------------------------------------------
// Collegamento di un account

const PAGE_DONE: &str = "<!doctype html><meta charset=utf-8><title>Greatnote</title><body style=\"font-family:sans-serif;padding:3em\"><h2>Account collegato</h2><p>Puoi chiudere questa scheda e tornare a Greatnote.</p>";
const PAGE_FAILED: &str = "<!doctype html><meta charset=utf-8><title>Greatnote</title><body style=\"font-family:sans-serif;padding:3em\"><h2>Collegamento non completato</h2><p>Torna a Greatnote per i dettagli.</p>";

fn respond(mut stream: TcpStream, status: &str, body: &str) {
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
}

/// Attende il ritorno dal browser sul loopback. Ignora richieste estranee e
/// quelle con `state` diverso.
pub(crate) fn wait_for_code(
    listeners: &[TcpListener],
    state: &str,
    cancel: &AtomicBool,
) -> CommandResult<String> {
    let deadline = Instant::now() + LOGIN_TIMEOUT;
    for listener in listeners {
        listener.set_nonblocking(true).map_err(message)?;
    }
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("Collegamento annullato".to_owned());
        }
        if Instant::now() > deadline {
            return Err("Tempo scaduto: l'accesso nel browser non è stato completato".to_owned());
        }
        let mut accepted = None;
        for listener in listeners {
            if let Ok((stream, _)) = listener.accept() {
                accepted = Some(stream);
                break;
            }
        }
        let Some(stream) = accepted else {
            thread::sleep(Duration::from_millis(100));
            continue;
        };
        let _ = stream.set_nonblocking(false);
        let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
        let mut line = String::new();
        let mut reader = BufReader::new(stream.try_clone().map_err(message)?);
        if reader.read_line(&mut line).is_err() || line.len() > 8192 {
            continue;
        }
        match parse_callback(&line) {
            Callback::Code {
                code,
                state: received,
            } if received == state => {
                respond(stream, "200 OK", PAGE_DONE);
                return Ok(code);
            }
            Callback::Denied {
                error,
                state: Some(received),
            } if received == state => {
                respond(stream, "200 OK", PAGE_FAILED);
                return Err(format!("Accesso negato o non riuscito: {error}"));
            }
            _ => respond(stream, "404 Not Found", ""),
        }
    }
}

/// Loopback IPv4 su una porta libera; per Microsoft (`localhost`) anche
/// IPv6 sulla stessa porta, se disponibile, perché il browser può
/// risolvere `localhost` in `::1`.
fn bind_loopback(provider: Provider) -> CommandResult<(Vec<TcpListener>, u16)> {
    let v4 = TcpListener::bind("127.0.0.1:0")
        .map_err(|error| format!("Porta locale non disponibile: {error}"))?;
    let port = v4.local_addr().map_err(message)?.port();
    let mut listeners = vec![v4];
    if provider == Provider::Microsoft
        && let Ok(v6) = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port)))
    {
        listeners.push(v6);
    }
    Ok((listeners, port))
}

#[tauri::command]
pub async fn connect_calendar_account(
    app: AppHandle,
    provider: Provider,
) -> CommandResult<CalendarStatus> {
    blocking(app, move |app| {
        let database = app.state::<Database>();
        let state = app.state::<CalendarState>();
        let client = oauth_client(&database, provider)?;
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut slot = state.connecting.lock().map_err(message)?;
            if slot.is_some() {
                return Err("Un collegamento è già in corso".to_owned());
            }
            *slot = Some(Arc::clone(&cancel));
        }
        let result: CommandResult<()> = (|| {
            let (listeners, port) = bind_loopback(provider)?;
            let redirect = redirect_uri(provider, port);
            let pkce = Pkce::new();
            let csrf = random_state();
            app.opener()
                .open_url(
                    client.authorization_url(&redirect, &csrf, &pkce),
                    None::<&str>,
                )
                .map_err(|error| format!("Impossibile aprire il browser: {error}"))?;
            let code = wait_for_code(&listeners, &csrf, &cancel)?;
            drop(listeners);
            let tokens = client
                .exchange_code(&code, &pkce.verifier, &redirect)
                .map_err(message)?;
            let refresh_token = tokens.refresh_token.clone().ok_or_else(|| {
                "Il fornitore non ha concesso l'accesso continuativo: riprova il collegamento"
                    .to_owned()
            })?;
            let api = service(provider, &tokens.access_token);
            let label = api.account_label().map_err(message)?;
            let account_id = database
                .save_calendar_account(provider, &label)
                .map_err(message)?;
            write_secret(&refresh_user(&account_id), &refresh_token)?;
            remember_token(&state, &account_id, &tokens);
            tracing::info!(provider = provider.as_str(), "account calendario collegato");
            let result =
                calendar_sync::sync_account(&database, api.as_ref(), &account_id, time::now());
            if let Err(error) = &result {
                let _ = database.record_calendar_sync(&account_id, Some(&error.to_string()));
            }
            Ok(())
        })();
        if let Ok(mut slot) = state.connecting.lock() {
            *slot = None;
        }
        result?;
        let _ = app.emit(SYNC_FINISHED_EVENT, ());
        status(app)
    })
    .await
}

#[tauri::command]
pub fn cancel_calendar_connect(state: State<'_, CalendarState>) {
    if let Ok(slot) = state.connecting.lock()
        && let Some(cancel) = slot.as_ref()
    {
        cancel.store(true, Ordering::SeqCst);
    }
}

/// Scollega un account: revoca l'accesso presso Google (Microsoft non lo
/// consente via API), rimuove il token dal portachiavi e la copia locale
/// degli eventi. Gli eventi nel calendario d'origine non vengono toccati.
#[tauri::command]
pub async fn disconnect_calendar_account(
    app: AppHandle,
    account_id: String,
) -> CommandResult<CalendarStatus> {
    blocking(app, move |app| {
        let database = app.state::<Database>();
        let account = database.calendar_account(&account_id).map_err(message)?;
        let user = refresh_user(&account.id);
        if let Some(token) = read_secret(&user)?
            && let Ok(client) = oauth_client(&database, account.provider)
            && let Err(error) = client.revoke(&token)
        {
            tracing::warn!(%error, "revoca presso il fornitore non riuscita");
        }
        delete_secret(&user)?;
        forget_token(app, &account.id);
        database
            .delete_calendar_account(&account.id)
            .map_err(message)?;
        let _ = app.emit(SYNC_FINISHED_EVENT, ());
        status(app)
    })
    .await
}

#[tauri::command]
pub async fn set_external_calendar_selected(
    app: AppHandle,
    calendar_id: String,
    selected: bool,
) -> CommandResult<CalendarStatus> {
    blocking(app, move |app| {
        let calendar = app
            .state::<Database>()
            .set_calendar_selected(&calendar_id, selected)
            .map_err(message)?;
        if selected {
            sync_one(app, &calendar.account_id);
        }
        let _ = app.emit(SYNC_FINISHED_EVENT, ());
        status(app)
    })
    .await
}

// ---------------------------------------------------------------------------
// Sincronizzazione

/// Sincronizza un account registrando l'esito (anche l'errore).
fn sync_one(app: &AppHandle, account_id: &str) {
    let database = app.state::<Database>();
    let Ok(account) = database.calendar_account(account_id) else {
        return;
    };
    let result = account_service(app, &account).and_then(|api| {
        calendar_sync::sync_account(&database, api.as_ref(), &account.id, time::now())
            .map_err(message)
    });
    if let Err(error) = result {
        tracing::warn!(
            provider = account.provider.as_str(),
            "sincronizzazione calendario non riuscita"
        );
        if error == CalendarError::Unauthorized.to_string() {
            forget_token(app, &account.id);
        }
        let _ = database.record_calendar_sync(&account.id, Some(&error));
    }
}

fn sync_all(app: &AppHandle) {
    let state = app.state::<CalendarState>();
    if state.syncing.swap(true, Ordering::SeqCst) {
        return;
    }
    let database = app.state::<Database>();
    let accounts = database.calendar_accounts().unwrap_or_default();
    for account in &accounts {
        sync_one(app, &account.id);
    }
    let subscriptions = database.calendar_subscriptions().unwrap_or_default();
    for subscription in &subscriptions {
        sync_subscription(app, &subscription.id);
    }
    state.syncing.store(false, Ordering::SeqCst);
    if !accounts.is_empty() || !subscriptions.is_empty() {
        let _ = app.emit(SYNC_FINISHED_EVENT, ());
    }
}

// ---------------------------------------------------------------------------
// Abbonamenti iCal (sola lettura, nessun accesso all'account)

fn subscription_user(id: &str) -> String {
    format!("ics-{id}")
}

/// Aggiorna un abbonamento registrando l'esito.
fn sync_subscription(app: &AppHandle, id: &str) {
    let database = app.state::<Database>();
    let result = read_secret(&subscription_user(id))
        .and_then(|url| {
            url.ok_or_else(|| {
                "Indirizzo non trovato nel portachiavi: aggiungi di nuovo il calendario".to_owned()
            })
        })
        .and_then(|url| fetch_ics(&url).map_err(message))
        .and_then(|text| apply_ics_now(&database, id, &text).map_err(message));
    if let Err(error) = result {
        tracing::warn!("aggiornamento di un calendario iCal non riuscito");
        let _ = database.record_subscription_sync(id, Some(&error));
    }
}

/// Aggiunge un abbonamento: scarica subito il calendario (un indirizzo
/// sbagliato non viene salvato) e conserva l'indirizzo nel portachiavi.
#[tauri::command]
pub async fn add_calendar_subscription(
    app: AppHandle,
    url: String,
    name: Option<String>,
    color: Option<String>,
) -> CommandResult<CalendarStatus> {
    blocking(app, move |app| {
        let url = normalize_ics_url(&url).map_err(message)?;
        let text = fetch_ics(&url).map_err(message)?;
        let name = name
            .map(|name| name.trim().to_owned())
            .filter(|name| !name.is_empty())
            .or_else(|| calendar_name(&text))
            .unwrap_or_else(|| "Calendario iCal".to_owned());
        let database = app.state::<Database>();
        // Il contenuto deve essere un calendario prima di salvare qualcosa.
        validate_ics(&text).map_err(message)?;
        let id = database
            .add_calendar_subscription(&name, color.as_deref())
            .map_err(message)?;
        if let Err(error) = write_secret(&subscription_user(&id), &url) {
            let _ = database.delete_calendar_subscription(&id);
            return Err(error);
        }
        if let Err(error) = apply_ics_now(&database, &id, &text) {
            let _ = database.record_subscription_sync(&id, Some(&error.to_string()));
        }
        tracing::info!("calendario iCal aggiunto");
        let _ = app.emit(SYNC_FINISHED_EVENT, ());
        status(app)
    })
    .await
}

#[tauri::command]
pub async fn update_calendar_subscription(
    app: AppHandle,
    id: String,
    name: String,
    color: Option<String>,
) -> CommandResult<CalendarStatus> {
    blocking(app, move |app| {
        app.state::<Database>()
            .update_calendar_subscription(&id, &name, color.as_deref())
            .map_err(message)?;
        let _ = app.emit(SYNC_FINISHED_EVENT, ());
        status(app)
    })
    .await
}

#[tauri::command]
pub async fn remove_calendar_subscription(
    app: AppHandle,
    id: String,
) -> CommandResult<CalendarStatus> {
    blocking(app, move |app| {
        delete_secret(&subscription_user(&id))?;
        app.state::<Database>()
            .delete_calendar_subscription(&id)
            .map_err(message)?;
        let _ = app.emit(SYNC_FINISHED_EVENT, ());
        status(app)
    })
    .await
}

#[tauri::command]
pub async fn sync_calendars_now(app: AppHandle) -> CommandResult<CalendarStatus> {
    blocking(app, |app| {
        sync_all(app);
        status(app)
    })
    .await
}

/// Sincronizzazione periodica in background (solo se ci sono account).
pub fn spawn_calendar_sync(app: &AppHandle) {
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(FIRST_SYNC_DELAY);
        loop {
            sync_all(&app);
            thread::sleep(SYNC_INTERVAL);
        }
    });
}

// ---------------------------------------------------------------------------
// Eventi esterni

fn account_for_calendar(app: &AppHandle, calendar_id: &str) -> CommandResult<CalendarAccount> {
    let database = app.state::<Database>();
    let calendar = database.external_calendar(calendar_id).map_err(message)?;
    database
        .calendar_account(&calendar.account_id)
        .map_err(message)
}

#[tauri::command]
pub async fn create_external_event(
    app: AppHandle,
    calendar_id: String,
    input: EventInput,
) -> CommandResult<CalendarEvent> {
    blocking(app, move |app| {
        let account = account_for_calendar(app, &calendar_id)?;
        let api = account_service(app, &account)?;
        calendar_sync::create_external_event(
            &app.state::<Database>(),
            api.as_ref(),
            &calendar_id,
            &input,
        )
        .map_err(message)
    })
    .await
}

#[tauri::command]
pub async fn update_external_event(
    app: AppHandle,
    id: String,
    input: EventInput,
) -> CommandResult<CalendarEvent> {
    blocking(app, move |app| {
        let database = app.state::<Database>();
        let reference = database
            .external_event_ref(&id)
            .map_err(message)?
            .ok_or_else(|| "Non è un evento di un calendario esterno".to_owned())?;
        let account = database
            .calendar_account(&reference.account_id)
            .map_err(message)?;
        let api = account_service(app, &account)?;
        calendar_sync::update_external_event(&database, api.as_ref(), &reference, &input)
            .map_err(message)
    })
    .await
}

#[tauri::command]
pub async fn delete_external_event(app: AppHandle, id: String) -> CommandResult<()> {
    blocking(app, move |app| {
        let database = app.state::<Database>();
        let reference = database
            .external_event_ref(&id)
            .map_err(message)?
            .ok_or_else(|| "Non è un evento di un calendario esterno".to_owned())?;
        let account = database
            .calendar_account(&reference.account_id)
            .map_err(message)?;
        let api = account_service(app, &account)?;
        calendar_sync::delete_external_event(&database, api.as_ref(), &id, &reference)
            .map_err(message)
    })
    .await
}

/// Apre l'evento nel calendario d'origine (solo indirizzi https salvati
/// dalla sincronizzazione).
#[tauri::command]
pub fn open_external_event(
    app: AppHandle,
    database: State<'_, Database>,
    id: String,
) -> CommandResult<()> {
    let event = database.get_event(&id).map_err(message)?;
    let link = event
        .web_link
        .filter(|link| link.starts_with("https://"))
        .ok_or_else(|| "Collegamento all'evento non disponibile".to_owned())?;
    app.opener().open_url(link, None::<&str>).map_err(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_callback_with_state_check() {
        let (listeners, port) = bind_loopback(Provider::Google).expect("bind");
        let cancel = AtomicBool::new(false);
        let client = thread::spawn(move || {
            let send = |path: &str| {
                let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
                write!(stream, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").expect("write");
                let mut response = String::new();
                let _ = std::io::Read::read_to_string(&mut stream, &mut response);
                response
            };
            assert!(send("/favicon.ico").starts_with("HTTP/1.1 404"));
            // `state` diverso: richiesta forgiata, ignorata.
            assert!(send("/?code=rubato&state=altro").starts_with("HTTP/1.1 404"));
            assert!(send("/?code=buono&state=atteso").contains("Account collegato"));
        });
        assert_eq!(
            wait_for_code(&listeners, "atteso", &cancel).expect("code"),
            "buono"
        );
        client.join().expect("client");

        let (listeners, _) = bind_loopback(Provider::Microsoft).expect("bind");
        let cancel = AtomicBool::new(true);
        assert!(wait_for_code(&listeners, "x", &cancel).is_err());
    }

    #[test]
    fn builtin_clients_are_optional_and_validated() {
        let google = Some(" 123-abc.apps.googleusercontent.com ");
        let microsoft = Some("6731de76-14a6-49ae-97bc-6eba6914391e");
        assert_eq!(
            builtin_client(Provider::Google, google, Some("GOCSPX-x"), microsoft),
            Some(("123-abc.apps.googleusercontent.com", Some("GOCSPX-x")))
        );
        assert_eq!(
            builtin_client(Provider::Microsoft, None, None, microsoft),
            Some(("6731de76-14a6-49ae-97bc-6eba6914391e", None))
        );
        assert_eq!(
            builtin_client(Provider::Google, Some("  "), None, None),
            None
        );
        assert_eq!(
            builtin_client(Provider::Google, None, None, microsoft),
            None
        );
        assert_eq!(
            builtin_client(Provider::Microsoft, None, None, Some("non-guid")),
            None
        );
        assert_eq!(
            builtin_client(Provider::Google, Some("id con spazi"), None, None),
            None
        );
    }
}
