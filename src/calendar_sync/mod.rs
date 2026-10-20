//! Calendari esterni: Google Calendar e Microsoft Outlook/365 (ADR 0015).
//!
//! Il calendario d'origine resta la fonte di verità. Greatnote ne tiene una
//! copia locale degli eventi in una finestra di date (90 giorni indietro,
//! 400 avanti) per mostrarli offline nel calendario, e modifica o crea
//! eventi chiamando le API del fornitore, con controllo delle versioni
//! (`If-Match`) per non sovrascrivere modifiche fatte altrove. Le
//! occorrenze di eventi ricorrenti sono in sola lettura.

pub mod google;
pub(crate) mod http;
pub mod ics;
pub mod microsoft;
pub mod oauth;
pub mod subscriptions;

use jiff::{Timestamp, ToSpan, civil::Date};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::planner::{CalendarEvent, EventInput},
    planner::validate_event,
    storage::{Database, StorageError},
    time::{self, Zone},
};

/// Giorni sincronizzati prima e dopo oggi.
pub const SYNC_PAST_DAYS: i64 = 90;
pub const SYNC_FUTURE_DAYS: i64 = 400;
const MAX_TITLE_CHARS: usize = 512;
const MAX_DESCRIPTION_CHARS: usize = 8192;
const NOW_SQL: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Google,
    Microsoft,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Google => "google",
            Self::Microsoft => "microsoft",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "google" => Some(Self::Google),
            "microsoft" => Some(Self::Microsoft),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CalendarError {
    #[error("Accesso non più valido: ricollega l'account")]
    Unauthorized,
    #[error("Operazione non consentita dal fornitore: {0}")]
    Forbidden(String),
    #[error("Evento o calendario non trovato")]
    NotFound,
    #[error("L'evento è stato modificato altrove nel frattempo: sincronizza e riprova")]
    Conflict,
    #[error("Troppe richieste al fornitore: riprova tra qualche minuto")]
    RateLimited,
    #[error("Errore del fornitore ({status}): {message}")]
    Http { status: u16, message: String },
    #[error("Rete: {0}")]
    Network(String),
    #[error("Risposta non valida: {0}")]
    InvalidResponse(String),
    #[error("Accesso non riuscito: {0}")]
    OAuth(String),
    #[error("Evento non valido")]
    InvalidEvent,
    #[error("Indirizzo non valido: usa l'indirizzo iCal (https:// o webcal://) del calendario")]
    InvalidUrl,
    #[error("Questo evento non si può modificare da Greatnote: aprilo nel calendario d'origine")]
    ReadOnly,
    #[error(transparent)]
    Storage(#[from] StorageError),
}

/// Calendario come lo restituisce il fornitore.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCalendar {
    pub remote_id: String,
    pub name: String,
    pub color: Option<String>,
    pub can_edit: bool,
    pub is_primary: bool,
}

/// Evento del fornitore in forma comune: istanti UTC o date con fine
/// inclusa, come gli eventi di Greatnote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteEvent {
    pub remote_id: String,
    pub etag: Option<String>,
    pub title: String,
    pub description: String,
    pub all_day: bool,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub timezone: Option<String>,
    /// Occorrenza di un evento ricorrente.
    pub recurring: bool,
    pub web_link: Option<String>,
}

/// Evento da inviare al fornitore, già validato.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventDraft {
    pub title: String,
    pub description: String,
    pub all_day: bool,
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub timezone: String,
}

impl EventDraft {
    /// Stesse regole degli eventi locali (titolo, durata, fuso).
    pub fn from_input(input: &EventInput) -> Result<Self, StorageError> {
        let valid = validate_event(input)?;
        Ok(Self {
            title: valid.title,
            description: valid.description,
            all_day: valid.all_day,
            start_at: valid.start_at,
            end_at: valid.end_at,
            start_date: valid.start_date,
            end_date: valid.end_date,
            timezone: valid.timezone,
        })
    }
}

/// Operazioni sul calendario di un account (implementate per Google e
/// Microsoft; nei test da un servizio finto).
pub trait CalendarService {
    fn account_label(&self) -> Result<String, CalendarError>;
    fn list_calendars(&self) -> Result<Vec<RemoteCalendar>, CalendarError>;
    /// Eventi che intersecano `from..to` (istanti RFC 3339 UTC).
    fn list_events(
        &self,
        calendar: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<RemoteEvent>, CalendarError>;
    fn create_event(
        &self,
        calendar: &str,
        draft: &EventDraft,
    ) -> Result<RemoteEvent, CalendarError>;
    fn update_event(
        &self,
        calendar: &str,
        remote_id: &str,
        etag: Option<&str>,
        draft: &EventDraft,
    ) -> Result<RemoteEvent, CalendarError>;
    fn delete_event(
        &self,
        calendar: &str,
        remote_id: &str,
        etag: Option<&str>,
    ) -> Result<(), CalendarError>;
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExternalCalendar {
    pub id: String,
    pub account_id: String,
    pub remote_id: String,
    pub name: String,
    pub color: Option<String>,
    pub can_edit: bool,
    pub is_primary: bool,
    pub selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CalendarAccount {
    pub id: String,
    pub provider: Provider,
    pub label: String,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
    pub calendars: Vec<ExternalCalendar>,
}

/// Riferimenti remoti di un evento esterno, per modificarlo o eliminarlo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalEventRef {
    pub account_id: String,
    pub calendar_id: String,
    pub calendar_remote_id: String,
    pub remote_id: String,
    pub etag: Option<String>,
    pub read_only: bool,
    pub note_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct SyncReport {
    pub calendars: usize,
    pub events: usize,
    pub removed: usize,
}

// ---------------------------------------------------------------------------
// Date e orari

/// Istante RFC 3339 con fuso qualsiasi → `AAAA-MM-GGTHH:MM:SSZ`.
pub(crate) fn parse_remote_instant(value: &str) -> Result<String, CalendarError> {
    value
        .parse::<Timestamp>()
        .map(time::format_utc)
        .map_err(|_| CalendarError::InvalidResponse(format!("orario non valido: {value}")))
}

/// Fine esclusiva → fine inclusa, mai prima dell'inizio.
pub(crate) fn previous_day(end_exclusive: &str, start: &str) -> Result<String, CalendarError> {
    let parse = |value: &str| {
        value
            .parse::<Date>()
            .map_err(|_| CalendarError::InvalidResponse(format!("data non valida: {value}")))
    };
    let start = parse(start)?;
    let end = parse(end_exclusive)?
        .yesterday()
        .map_err(|_| CalendarError::InvalidResponse("data fuori intervallo".to_owned()))?;
    Ok(end.max(start).to_string())
}

/// Fine inclusa → fine esclusiva.
pub(crate) fn next_day_string(date: &str) -> Result<String, CalendarError> {
    let date = date
        .parse::<Date>()
        .map_err(|_| CalendarError::InvalidEvent)?;
    Ok(time::next_day(date)?.to_string())
}

/// Finestra sincronizzata attorno a `now`.
pub fn sync_window(now: Timestamp) -> Result<(String, String), CalendarError> {
    let from = now
        .checked_sub((SYNC_PAST_DAYS * 24).hours())
        .map_err(|_| CalendarError::InvalidEvent)?;
    let to = now
        .checked_add((SYNC_FUTURE_DAYS * 24).hours())
        .map_err(|_| CalendarError::InvalidEvent)?;
    Ok((time::format_utc(from), time::format_utc(to)))
}

fn clip(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// Evento remoto adattato ai vincoli della tabella `events`.
struct StoredEvent {
    title: String,
    description: String,
    all_day: bool,
    start_at: Option<String>,
    end_at: Option<String>,
    start_date: Option<String>,
    end_date: Option<String>,
    timezone: String,
    etag: Option<String>,
    web_link: Option<String>,
    read_only: bool,
}

fn normalize(event: &RemoteEvent, can_edit: bool) -> Option<StoredEvent> {
    if event.remote_id.is_empty() || event.remote_id.len() > 512 {
        return None;
    }
    let title = event.title.trim();
    let title = if title.is_empty() {
        "(senza titolo)"
    } else {
        title
    };
    let timezone = event
        .timezone
        .as_deref()
        .and_then(|name| Zone::resolve(Some(name)).ok())
        .unwrap_or_else(Zone::system)
        .name;
    let (start_at, end_at, start_date, end_date) = if event.all_day {
        let start = event.start_date.clone()?;
        let end = event
            .end_date
            .clone()
            .filter(|end| *end >= start)
            .unwrap_or_else(|| start.clone());
        (None, None, Some(start), Some(end))
    } else {
        let start = event.start_at.clone()?;
        let end = event
            .end_at
            .clone()
            .filter(|end| *end >= start)
            .unwrap_or_else(|| start.clone());
        (Some(start), Some(end), None, None)
    };
    Some(StoredEvent {
        title: clip(title, MAX_TITLE_CHARS),
        description: clip(&event.description, MAX_DESCRIPTION_CHARS),
        all_day: event.all_day,
        start_at,
        end_at,
        start_date,
        end_date,
        timezone,
        etag: event.etag.clone().filter(|etag| etag.len() <= 512),
        // Solo collegamenti web sicuri, mai altri schemi.
        web_link: event
            .web_link
            .clone()
            .filter(|link| link.starts_with("https://") && link.len() <= 2048),
        read_only: event.recurring || !can_edit,
    })
}

// ---------------------------------------------------------------------------
// Database

const CALENDAR_COLUMNS: &str =
    "id, account_id, remote_id, name, color, can_edit, is_primary, selected";

fn calendar_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ExternalCalendar> {
    Ok(ExternalCalendar {
        id: row.get(0)?,
        account_id: row.get(1)?,
        remote_id: row.get(2)?,
        name: row.get(3)?,
        color: row.get(4)?,
        can_edit: row.get(5)?,
        is_primary: row.get(6)?,
        selected: row.get(7)?,
    })
}

fn account_calendars(
    connection: &Connection,
    account_id: &str,
) -> Result<Vec<ExternalCalendar>, StorageError> {
    let mut statement = connection.prepare(&format!(
        "SELECT {CALENDAR_COLUMNS} FROM external_calendars WHERE account_id = ?1
         ORDER BY is_primary DESC, name COLLATE NOCASE, id"
    ))?;
    let rows = statement.query_map([account_id], calendar_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
}

/// Promemoria degli eventi esterni che stanno per essere eliminati.
fn delete_event_reminders(
    transaction: &Transaction<'_>,
    where_events: &str,
    value: &str,
) -> Result<(), StorageError> {
    transaction.execute(
        &format!(
            "DELETE FROM reminders WHERE entity_type = 'event'
               AND entity_id IN (SELECT id FROM events WHERE {where_events})"
        ),
        [value],
    )?;
    Ok(())
}

fn upsert_event(
    transaction: &Transaction<'_>,
    calendar_id: &str,
    provider: Provider,
    remote_id: &str,
    event: &StoredEvent,
) -> Result<String, StorageError> {
    let id: String = transaction.query_row(
        &format!(
            "INSERT INTO events (id, title, description, all_day, start_at, end_at, start_date,
                                 end_date, timezone, calendar_provider, external_id,
                                 external_calendar_id, etag, web_link, read_only)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT (external_calendar_id, external_id) WHERE external_calendar_id IS NOT NULL
             DO UPDATE SET title = excluded.title, description = excluded.description,
                 all_day = excluded.all_day, start_at = excluded.start_at,
                 end_at = excluded.end_at, start_date = excluded.start_date,
                 end_date = excluded.end_date, timezone = excluded.timezone,
                 etag = excluded.etag, web_link = excluded.web_link,
                 read_only = excluded.read_only, deleted_at = NULL,
                 updated_at = CASE WHEN events.etag IS excluded.etag
                                   THEN events.updated_at ELSE {NOW_SQL} END
             RETURNING id"
        ),
        params![
            Uuid::new_v4().to_string(),
            event.title,
            event.description,
            event.all_day,
            event.start_at,
            event.end_at,
            event.start_date,
            event.end_date,
            event.timezone,
            provider.as_str(),
            remote_id,
            calendar_id,
            event.etag,
            event.web_link,
            event.read_only,
        ],
        |row| row.get(0),
    )?;
    Ok(id)
}

impl Database {
    /// Registra un account collegato (o ritrova quello già collegato con lo
    /// stesso indirizzo) e ne restituisce l'id.
    pub fn save_calendar_account(
        &self,
        provider: Provider,
        label: &str,
    ) -> Result<String, StorageError> {
        let label = clip(label.trim(), 320);
        if label.is_empty() {
            return Err(StorageError::InvalidEvent("account senza nome"));
        }
        let connection = self.connection()?;
        let id = connection.query_row(
            &format!(
                "INSERT INTO calendar_accounts (id, provider, label) VALUES (?1, ?2, ?3)
                 ON CONFLICT (provider, label) DO UPDATE SET updated_at = {NOW_SQL},
                     last_error = NULL
                 RETURNING id"
            ),
            params![Uuid::new_v4().to_string(), provider.as_str(), label],
            |row| row.get(0),
        )?;
        Ok(id)
    }

    pub fn calendar_accounts(&self) -> Result<Vec<CalendarAccount>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, provider, label, last_sync_at, last_error FROM calendar_accounts
             ORDER BY created_at, id",
        )?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let mut accounts = Vec::with_capacity(rows.len());
        for (id, provider, label, last_sync_at, last_error) in rows {
            let Some(provider) = Provider::parse(&provider) else {
                continue;
            };
            accounts.push(CalendarAccount {
                calendars: account_calendars(&connection, &id)?,
                id,
                provider,
                label,
                last_sync_at,
                last_error,
            });
        }
        Ok(accounts)
    }

    pub fn calendar_account(&self, id: &str) -> Result<CalendarAccount, StorageError> {
        self.calendar_accounts()?
            .into_iter()
            .find(|account| account.id == id)
            .ok_or(StorageError::CalendarNotFound)
    }

    /// Scollega un account: elimina calendari ed eventi copiati (non quelli
    /// del fornitore) e i promemoria su quegli eventi.
    pub fn delete_calendar_account(&self, id: &str) -> Result<bool, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        delete_event_reminders(
            &transaction,
            "external_calendar_id IN (SELECT id FROM external_calendars WHERE account_id = ?1)",
            id,
        )?;
        let deleted = transaction.execute("DELETE FROM calendar_accounts WHERE id = ?1", [id])?;
        transaction.commit()?;
        Ok(deleted > 0)
    }

    pub fn external_calendar(&self, id: &str) -> Result<ExternalCalendar, StorageError> {
        self.connection()?
            .query_row(
                &format!("SELECT {CALENDAR_COLUMNS} FROM external_calendars WHERE id = ?1"),
                [id],
                calendar_from_row,
            )
            .optional()?
            .ok_or(StorageError::CalendarNotFound)
    }

    /// Sceglie se sincronizzare un calendario; deselezionato, i suoi eventi
    /// locali vengono rimossi.
    pub fn set_calendar_selected(
        &self,
        id: &str,
        selected: bool,
    ) -> Result<ExternalCalendar, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE external_calendars SET selected = ?2 WHERE id = ?1",
            params![id, selected],
        )?;
        if changed == 0 {
            return Err(StorageError::CalendarNotFound);
        }
        if !selected {
            delete_event_reminders(&transaction, "external_calendar_id = ?1", id)?;
            transaction.execute("DELETE FROM events WHERE external_calendar_id = ?1", [id])?;
        }
        transaction.commit()?;
        drop(connection);
        self.external_calendar(id)
    }

    pub fn record_calendar_sync(
        &self,
        account_id: &str,
        error: Option<&str>,
    ) -> Result<(), StorageError> {
        let connection = self.connection()?;
        match error {
            None => connection.execute(
                &format!(
                    "UPDATE calendar_accounts SET last_sync_at = {NOW_SQL}, last_error = NULL,
                         updated_at = {NOW_SQL} WHERE id = ?1"
                ),
                [account_id],
            )?,
            Some(error) => connection.execute(
                &format!(
                    "UPDATE calendar_accounts SET last_error = ?2, updated_at = {NOW_SQL}
                     WHERE id = ?1"
                ),
                params![account_id, clip(error, 1000)],
            )?,
        };
        Ok(())
    }

    /// Aggiorna l'elenco dei calendari di un account. I nuovi vengono
    /// sincronizzati solo se principali; quelli scomparsi dal fornitore
    /// vengono rimossi con i loro eventi.
    pub fn store_calendar_list(
        &self,
        account_id: &str,
        remotes: &[RemoteCalendar],
    ) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        for remote in remotes {
            if remote.remote_id.is_empty() || remote.remote_id.len() > 1024 {
                continue;
            }
            let name = clip(remote.name.trim(), 512);
            let name = if name.is_empty() {
                "Calendario".to_owned()
            } else {
                name
            };
            let color = remote.color.clone().filter(|color| color.len() <= 16);
            transaction.execute(
                "INSERT INTO external_calendars
                     (id, account_id, remote_id, name, color, can_edit, is_primary, selected)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
                 ON CONFLICT (account_id, remote_id) DO UPDATE SET name = excluded.name,
                     color = excluded.color, can_edit = excluded.can_edit,
                     is_primary = excluded.is_primary",
                params![
                    Uuid::new_v4().to_string(),
                    account_id,
                    remote.remote_id,
                    name,
                    color,
                    remote.can_edit,
                    remote.is_primary
                ],
            )?;
        }
        let known: Vec<(String, String)> = transaction
            .prepare("SELECT id, remote_id FROM external_calendars WHERE account_id = ?1")?
            .query_map([account_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        for (id, remote_id) in known {
            if !remotes.iter().any(|remote| remote.remote_id == remote_id) {
                delete_event_reminders(&transaction, "external_calendar_id = ?1", &id)?;
                transaction.execute("DELETE FROM external_calendars WHERE id = ?1", [&id])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Sostituisce la copia locale degli eventi di un calendario con quelli
    /// ricevuti: aggiorna i presenti (mantenendo la nota collegata) e
    /// rimuove quelli spariti. Restituisce (eventi salvati, rimossi).
    pub fn store_calendar_events(
        &self,
        calendar_id: &str,
        events: &[RemoteEvent],
    ) -> Result<(usize, usize), StorageError> {
        let calendar = self.external_calendar(calendar_id)?;
        let provider = self.calendar_account(&calendar.account_id)?.provider;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let mut seen = std::collections::HashSet::new();
        for event in events {
            let Some(stored) = normalize(event, calendar.can_edit) else {
                continue;
            };
            upsert_event(
                &transaction,
                calendar_id,
                provider,
                &event.remote_id,
                &stored,
            )?;
            seen.insert(event.remote_id.as_str());
        }
        let existing: Vec<(String, String)> = transaction
            .prepare("SELECT id, external_id FROM events WHERE external_calendar_id = ?1")?
            .query_map([calendar_id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let mut removed = 0;
        for (id, remote_id) in existing {
            if !seen.contains(remote_id.as_str()) {
                delete_event_reminders(&transaction, "id = ?1", &id)?;
                removed += transaction.execute("DELETE FROM events WHERE id = ?1", [&id])?;
            }
        }
        transaction.commit()?;
        Ok((seen.len(), removed))
    }

    /// Salva un evento appena creato o modificato presso il fornitore,
    /// collegandolo alla nota indicata.
    pub fn store_external_event(
        &self,
        calendar_id: &str,
        event: &RemoteEvent,
        note_id: Option<&str>,
    ) -> Result<CalendarEvent, StorageError> {
        let calendar = self.external_calendar(calendar_id)?;
        let provider = self.calendar_account(&calendar.account_id)?.provider;
        let stored = normalize(event, calendar.can_edit)
            .ok_or(StorageError::InvalidEvent("evento esterno non valido"))?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let id = upsert_event(
            &transaction,
            calendar_id,
            provider,
            &event.remote_id,
            &stored,
        )?;
        let note_id = note_id.filter(|id| !id.is_empty());
        if let Some(note_id) = note_id {
            let active: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM notes WHERE id = ?1 AND deleted_at IS NULL)",
                [note_id],
                |row| row.get(0),
            )?;
            if !active {
                return Err(StorageError::NoteNotFound);
            }
        }
        transaction.execute(
            "UPDATE events SET note_id = ?2 WHERE id = ?1",
            params![id, note_id],
        )?;
        transaction.commit()?;
        drop(connection);
        self.get_event(&id)
    }

    /// Rimuove la copia locale di un evento esterno (dopo l'eliminazione
    /// presso il fornitore).
    pub fn remove_external_event(&self, event_id: &str) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        delete_event_reminders(
            &transaction,
            "id = ?1 AND external_calendar_id IS NOT NULL",
            event_id,
        )?;
        transaction.execute(
            "DELETE FROM events WHERE id = ?1 AND external_calendar_id IS NOT NULL",
            [event_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// Riferimenti remoti di un evento; `None` per gli eventi di Greatnote.
    pub fn external_event_ref(
        &self,
        event_id: &str,
    ) -> Result<Option<ExternalEventRef>, StorageError> {
        let connection = self.connection()?;
        let row = connection
            .query_row(
                "SELECT c.account_id, c.id, c.remote_id, e.external_id, e.etag, e.read_only,
                        e.note_id
                 FROM events e JOIN external_calendars c ON c.id = e.external_calendar_id
                 WHERE e.id = ?1",
                [event_id],
                |row| {
                    Ok(ExternalEventRef {
                        account_id: row.get(0)?,
                        calendar_id: row.get(1)?,
                        calendar_remote_id: row.get(2)?,
                        remote_id: row.get(3)?,
                        etag: row.get(4)?,
                        read_only: row.get(5)?,
                        note_id: row.get(6)?,
                    })
                },
            )
            .optional()?;
        drop(connection);
        if row.is_none() {
            // Distingue "evento locale" da "evento inesistente".
            self.get_event(event_id)?;
        }
        Ok(row)
    }
}

// ---------------------------------------------------------------------------
// Registrazioni dell'app presso i fornitori

const CLIENTS_KEY: &str = "calendar_clients";

/// Identificativi client registrati dall'utente presso Google e Microsoft
/// (non segreti). Il segreto client di Google resta nel portachiavi.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CalendarClients {
    pub google_client_id: String,
    pub microsoft_client_id: String,
}

impl CalendarClients {
    pub fn validated(self) -> Result<Self, StorageError> {
        let google_client_id = self.google_client_id.trim().to_owned();
        let microsoft_client_id = self.microsoft_client_id.trim().to_owned();
        let plain = |value: &str| {
            value.len() <= 200
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        };
        if !plain(&google_client_id) {
            return Err(StorageError::InvalidEvent(
                "ID client di Google non valido (es. 1234-abc.apps.googleusercontent.com)",
            ));
        }
        if !microsoft_client_id.is_empty() && Uuid::parse_str(&microsoft_client_id).is_err() {
            return Err(StorageError::InvalidEvent(
                "ID applicazione Microsoft non valido: è un GUID come 00000000-0000-0000-0000-000000000000",
            ));
        }
        Ok(Self {
            google_client_id,
            microsoft_client_id,
        })
    }

    pub fn client_id(&self, provider: Provider) -> Option<&str> {
        let id = match provider {
            Provider::Google => &self.google_client_id,
            Provider::Microsoft => &self.microsoft_client_id,
        };
        (!id.is_empty()).then_some(id.as_str())
    }
}

impl Database {
    pub fn calendar_clients(&self) -> Result<CalendarClients, StorageError> {
        let stored: Option<String> = self
            .connection()?
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [CLIENTS_KEY],
                |row| row.get(0),
            )
            .optional()?;
        Ok(stored
            .and_then(|value| serde_json::from_str::<CalendarClients>(&value).ok())
            .and_then(|clients| clients.validated().ok())
            .unwrap_or_default())
    }

    pub fn set_calendar_clients(
        &self,
        clients: CalendarClients,
    ) -> Result<CalendarClients, StorageError> {
        let clients = clients.validated()?;
        let value = serde_json::to_string(&clients)?;
        self.connection()?.execute(
            &format!(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = {NOW_SQL}"
            ),
            params![CLIENTS_KEY, value],
        )?;
        Ok(clients)
    }
}

// ---------------------------------------------------------------------------
// Sincronizzazione

/// Aggiorna calendari ed eventi di un account. Gli errori vanno registrati
/// dal chiamante con [`Database::record_calendar_sync`].
pub fn sync_account(
    database: &Database,
    service: &dyn CalendarService,
    account_id: &str,
    now: Timestamp,
) -> Result<SyncReport, CalendarError> {
    let remotes = service.list_calendars()?;
    database.store_calendar_list(account_id, &remotes)?;
    let account = database.calendar_account(account_id)?;
    let (from, to) = sync_window(now)?;
    let mut report = SyncReport::default();
    for calendar in account
        .calendars
        .iter()
        .filter(|calendar| calendar.selected)
    {
        let events = service.list_events(&calendar.remote_id, &from, &to)?;
        let (stored, removed) = database.store_calendar_events(&calendar.id, &events)?;
        report.calendars += 1;
        report.events += stored;
        report.removed += removed;
    }
    database.record_calendar_sync(account_id, None)?;
    Ok(report)
}

/// Crea un evento in un calendario esterno modificabile.
pub fn create_external_event(
    database: &Database,
    service: &dyn CalendarService,
    calendar_id: &str,
    input: &EventInput,
) -> Result<CalendarEvent, CalendarError> {
    let calendar = database.external_calendar(calendar_id)?;
    if !calendar.can_edit {
        return Err(CalendarError::ReadOnly);
    }
    let draft = EventDraft::from_input(input)?;
    let created = service.create_event(&calendar.remote_id, &draft)?;
    Ok(database.store_external_event(calendar_id, &created, input.note_id.as_deref())?)
}

/// Modifica un evento esterno presso il fornitore, solo se nessuno l'ha
/// cambiato nel frattempo (ETag).
pub fn update_external_event(
    database: &Database,
    service: &dyn CalendarService,
    reference: &ExternalEventRef,
    input: &EventInput,
) -> Result<CalendarEvent, CalendarError> {
    if reference.read_only {
        return Err(CalendarError::ReadOnly);
    }
    let draft = EventDraft::from_input(input)?;
    let updated = service.update_event(
        &reference.calendar_remote_id,
        &reference.remote_id,
        reference.etag.as_deref(),
        &draft,
    )?;
    Ok(
        database.store_external_event(
            &reference.calendar_id,
            &updated,
            input.note_id.as_deref(),
        )?,
    )
}

pub fn delete_external_event(
    database: &Database,
    service: &dyn CalendarService,
    event_id: &str,
    reference: &ExternalEventRef,
) -> Result<(), CalendarError> {
    if reference.read_only {
        return Err(CalendarError::ReadOnly);
    }
    service.delete_event(
        &reference.calendar_remote_id,
        &reference.remote_id,
        reference.etag.as_deref(),
    )?;
    database.remove_external_event(event_id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, collections::HashMap};

    use super::*;
    use crate::domain::planner::ReminderEntity;

    /// Fornitore in memoria con controllo degli ETag.
    #[derive(Default)]
    struct FakeService {
        calendars: Vec<RemoteCalendar>,
        events: RefCell<HashMap<String, Vec<RemoteEvent>>>,
        counter: RefCell<u32>,
    }

    impl FakeService {
        fn with(calendars: Vec<RemoteCalendar>) -> Self {
            Self {
                calendars,
                ..Self::default()
            }
        }

        fn add(&self, calendar: &str, event: RemoteEvent) {
            self.events
                .borrow_mut()
                .entry(calendar.to_owned())
                .or_default()
                .push(event);
        }

        fn event_from_draft(&self, remote_id: &str, draft: &EventDraft) -> RemoteEvent {
            *self.counter.borrow_mut() += 1;
            RemoteEvent {
                remote_id: remote_id.to_owned(),
                etag: Some(format!("v{}", self.counter.borrow())),
                title: draft.title.clone(),
                description: draft.description.clone(),
                all_day: draft.all_day,
                start_at: draft.start_at.clone(),
                end_at: draft.end_at.clone(),
                start_date: draft.start_date.clone(),
                end_date: draft.end_date.clone(),
                timezone: Some(draft.timezone.clone()),
                recurring: false,
                web_link: Some(format!("https://calendar.example/{remote_id}")),
            }
        }
    }

    impl CalendarService for FakeService {
        fn account_label(&self) -> Result<String, CalendarError> {
            Ok("anna@example.com".to_owned())
        }
        fn list_calendars(&self) -> Result<Vec<RemoteCalendar>, CalendarError> {
            Ok(self.calendars.clone())
        }
        fn list_events(
            &self,
            calendar: &str,
            _from: &str,
            _to: &str,
        ) -> Result<Vec<RemoteEvent>, CalendarError> {
            Ok(self
                .events
                .borrow()
                .get(calendar)
                .cloned()
                .unwrap_or_default())
        }
        fn create_event(
            &self,
            calendar: &str,
            draft: &EventDraft,
        ) -> Result<RemoteEvent, CalendarError> {
            let event = self.event_from_draft(&format!("nuovo-{}", self.counter.borrow()), draft);
            self.add(calendar, event.clone());
            Ok(event)
        }
        fn update_event(
            &self,
            calendar: &str,
            remote_id: &str,
            etag: Option<&str>,
            draft: &EventDraft,
        ) -> Result<RemoteEvent, CalendarError> {
            let mut events = self.events.borrow_mut();
            let list = events.get_mut(calendar).ok_or(CalendarError::NotFound)?;
            let position = list
                .iter()
                .position(|event| event.remote_id == remote_id)
                .ok_or(CalendarError::NotFound)?;
            if list[position].etag.as_deref() != etag {
                return Err(CalendarError::Conflict);
            }
            drop(events);
            let updated = self.event_from_draft(remote_id, draft);
            self.events
                .borrow_mut()
                .get_mut(calendar)
                .expect("calendar")[position] = updated.clone();
            Ok(updated)
        }
        fn delete_event(
            &self,
            calendar: &str,
            remote_id: &str,
            _etag: Option<&str>,
        ) -> Result<(), CalendarError> {
            if let Some(list) = self.events.borrow_mut().get_mut(calendar) {
                list.retain(|event| event.remote_id != remote_id);
            }
            Ok(())
        }
    }

    fn calendar(remote_id: &str, primary: bool, can_edit: bool) -> RemoteCalendar {
        RemoteCalendar {
            remote_id: remote_id.to_owned(),
            name: remote_id.to_owned(),
            color: Some("#33aa77".to_owned()),
            can_edit,
            is_primary: primary,
        }
    }

    fn timed(remote_id: &str, title: &str, start: &str, end: &str) -> RemoteEvent {
        RemoteEvent {
            remote_id: remote_id.to_owned(),
            etag: Some("e1".to_owned()),
            title: title.to_owned(),
            description: String::new(),
            all_day: false,
            start_at: Some(start.to_owned()),
            end_at: Some(end.to_owned()),
            start_date: None,
            end_date: None,
            timezone: Some("Europe/Rome".to_owned()),
            recurring: false,
            web_link: Some("javascript:alert(1)".to_owned()),
        }
    }

    fn input(title: &str, note_id: Option<String>) -> EventInput {
        EventInput {
            title: title.to_owned(),
            description: String::new(),
            all_day: false,
            start: "2026-10-21T09:00".to_owned(),
            end: "2026-10-21T10:00".to_owned(),
            timezone: Some("Europe/Rome".to_owned()),
            note_id,
        }
    }

    fn rome() -> Zone {
        Zone::resolve(Some("Europe/Rome")).expect("zone")
    }

    fn day(value: &str) -> Date {
        value.parse().expect("date")
    }

    #[test]
    fn sync_mirrors_selected_calendars_and_keeps_note_links() {
        let database = Database::in_memory().expect("db");
        let service = FakeService::with(vec![
            calendar("principale", true, true),
            calendar("festività", false, false),
        ]);
        service.add(
            "principale",
            timed(
                "a",
                "Riunione",
                "2026-10-21T07:00:00Z",
                "2026-10-21T08:00:00Z",
            ),
        );
        service.add(
            "principale",
            timed("b", "  ", "2026-10-22T07:00:00Z", "2026-10-22T06:00:00Z"),
        );
        service.add(
            "festività",
            timed("h", "Festa", "2026-11-01T00:00:00Z", "2026-11-01T01:00:00Z"),
        );
        let account = database
            .save_calendar_account(Provider::Google, "anna@example.com")
            .expect("account");
        assert_eq!(
            database
                .save_calendar_account(Provider::Google, "anna@example.com")
                .expect("again"),
            account
        );

        let now: Timestamp = "2026-10-20T10:00:00Z".parse().expect("now");
        let report = sync_account(&database, &service, &account, now).expect("sync");
        // Solo il calendario principale è selezionato all'inizio.
        assert_eq!(
            report,
            SyncReport {
                calendars: 1,
                events: 2,
                removed: 0
            }
        );
        let events = database
            .list_events(day("2026-10-21"), day("2026-10-22"), &rome())
            .expect("events");
        assert_eq!(events.len(), 2);
        let meeting = events
            .iter()
            .find(|event| event.title == "Riunione")
            .expect("meeting");
        assert_eq!(meeting.calendar_provider.as_deref(), Some("google"));
        assert!(meeting.web_link.is_none(), "schema non sicuro accettato");
        let untitled = events
            .iter()
            .find(|event| event.title == "(senza titolo)")
            .expect("untitled");
        assert_eq!(untitled.end_at, untitled.start_at);

        // Le modifiche locali passano dal fornitore.
        assert!(matches!(
            database.update_event(&meeting.id, &input("x", None)),
            Err(StorageError::ExternalEvent)
        ));
        assert!(matches!(
            database.move_item_to_trash(crate::trash::TrashKind::Event, &meeting.id),
            Err(StorageError::ExternalEvent)
        ));

        // Collegamento a una nota e promemoria sopravvivono alla sincronizzazione.
        let note = database
            .create_note(
                "Verbale",
                &serde_json::json!({"type": "doc", "content": []}),
            )
            .expect("note");
        let reference = database
            .external_event_ref(&meeting.id)
            .expect("ref")
            .expect("external");
        let mut edited = input("Riunione spostata", Some(note.id.clone()));
        edited.start = "2026-10-21T11:00".to_owned();
        edited.end = "2026-10-21T12:00".to_owned();
        let updated =
            update_external_event(&database, &service, &reference, &edited).expect("update");
        assert_eq!(updated.id, meeting.id);
        assert_eq!(updated.start_at.as_deref(), Some("2026-10-21T09:00:00Z"));
        assert_eq!(updated.note_id.as_deref(), Some(note.id.as_str()));
        database
            .create_reminder(
                ReminderEntity::Event,
                &meeting.id,
                "2026-10-21T08:30",
                Some("Europe/Rome"),
                now,
            )
            .expect("reminder");
        sync_account(&database, &service, &account, now).expect("resync");
        let again = database.get_event(&meeting.id).expect("event");
        assert_eq!(again.note_id.as_deref(), Some(note.id.as_str()));
        assert_eq!(again.title, "Riunione spostata");
        assert_eq!(database.list_reminders().expect("reminders").len(), 1);

        // ETag superato: nessuna sovrascrittura.
        let stale = ExternalEventRef {
            etag: Some("vecchio".to_owned()),
            ..reference.clone()
        };
        assert!(matches!(
            update_external_event(&database, &service, &stale, &edited),
            Err(CalendarError::Conflict)
        ));

        // Eliminato presso il fornitore → rimosso anche qui, con i promemoria.
        service
            .events
            .borrow_mut()
            .get_mut("principale")
            .expect("cal")
            .retain(|event| event.remote_id != "a");
        let report = sync_account(&database, &service, &account, now).expect("sync");
        assert_eq!(report.removed, 1);
        assert!(database.get_event(&meeting.id).is_err());
        assert!(database.list_reminders().expect("reminders").is_empty());

        // Calendario aggiunto alla selezione e poi tolto.
        let holidays = database
            .calendar_account(&account)
            .expect("account")
            .calendars
            .into_iter()
            .find(|calendar| calendar.remote_id == "festività")
            .expect("holidays");
        assert!(!holidays.selected && !holidays.can_edit);
        database
            .set_calendar_selected(&holidays.id, true)
            .expect("select");
        sync_account(&database, &service, &account, now).expect("sync");
        let festa = database
            .list_events(day("2026-11-01"), day("2026-11-01"), &rome())
            .expect("events")
            .into_iter()
            .find(|event| event.title == "Festa")
            .expect("festa");
        assert!(festa.read_only);
        let festa_ref = database
            .external_event_ref(&festa.id)
            .expect("ref")
            .expect("external");
        assert!(matches!(
            delete_external_event(&database, &service, &festa.id, &festa_ref),
            Err(CalendarError::ReadOnly)
        ));
        database
            .set_calendar_selected(&holidays.id, false)
            .expect("deselect");
        assert!(database.get_event(&festa.id).is_err());

        // Gli eventi esterni non entrano negli archivi dell'app.
        let local = database
            .create_event(&input("Locale", None))
            .expect("local");
        assert!(
            database
                .external_event_ref(&local.id)
                .expect("ref")
                .is_none()
        );
    }

    #[test]
    fn create_delete_and_disconnect() {
        let database = Database::in_memory().expect("db");
        let service = FakeService::with(vec![
            calendar("lavoro", true, true),
            calendar("altro", false, false),
        ]);
        let account = database
            .save_calendar_account(Provider::Microsoft, "anna@contoso.com")
            .expect("account");
        let now: Timestamp = "2026-10-20T10:00:00Z".parse().expect("now");
        sync_account(&database, &service, &account, now).expect("sync");
        let calendars = database
            .calendar_account(&account)
            .expect("account")
            .calendars;
        let work = calendars
            .iter()
            .find(|calendar| calendar.remote_id == "lavoro")
            .expect("work");
        let other = calendars
            .iter()
            .find(|calendar| calendar.remote_id == "altro")
            .expect("other");
        assert!(matches!(
            create_external_event(&database, &service, &other.id, &input("No", None)),
            Err(CalendarError::ReadOnly)
        ));
        let created = create_external_event(&database, &service, &work.id, &input("Call", None))
            .expect("create");
        assert_eq!(created.calendar_provider.as_deref(), Some("microsoft"));
        assert_eq!(
            created.external_calendar_id.as_deref(),
            Some(work.id.as_str())
        );
        assert!(
            created
                .web_link
                .as_deref()
                .is_some_and(|link| link.starts_with("https://"))
        );
        // Una nuova sincronizzazione non lo duplica.
        sync_account(&database, &service, &account, now).expect("sync");
        assert_eq!(
            database
                .list_events(day("2026-10-21"), day("2026-10-21"), &rome())
                .expect("events")
                .len(),
            1
        );

        let reference = database
            .external_event_ref(&created.id)
            .expect("ref")
            .expect("external");
        delete_external_event(&database, &service, &created.id, &reference).expect("delete");
        assert!(database.get_event(&created.id).is_err());

        create_external_event(&database, &service, &work.id, &input("Altra", None))
            .expect("create");
        // Calendario sparito dal fornitore: rimosso con i suoi eventi.
        let service = FakeService::with(vec![calendar("altro", false, false)]);
        sync_account(&database, &service, &account, now).expect("sync");
        assert!(database.external_calendar(&work.id).is_err());
        assert!(
            database
                .list_events(day("2026-10-21"), day("2026-10-21"), &rome())
                .expect("events")
                .is_empty()
        );

        assert!(database.delete_calendar_account(&account).expect("delete"));
        assert!(database.calendar_accounts().expect("accounts").is_empty());
        assert!(database.external_calendar(&other.id).is_err());
    }

    #[test]
    fn date_helpers() {
        assert_eq!(
            previous_day("2026-01-01", "2025-12-31").expect("day"),
            "2025-12-31"
        );
        assert_eq!(
            previous_day("2026-01-01", "2026-01-01").expect("day"),
            "2026-01-01"
        );
        assert_eq!(next_day_string("2028-02-28").expect("day"), "2028-02-29");
        assert_eq!(
            parse_remote_instant("2026-10-25T02:30:00+01:00").expect("instant"),
            "2026-10-25T01:30:00Z"
        );
        let (from, to) = sync_window("2026-10-20T10:00:00Z".parse().expect("now")).expect("window");
        assert_eq!(from, "2026-07-22T10:00:00Z");
        assert_eq!(to, "2027-11-24T10:00:00Z");
    }

    #[test]
    fn client_ids_are_validated() {
        let database = Database::in_memory().expect("db");
        assert_eq!(
            database.calendar_clients().expect("clients"),
            CalendarClients::default()
        );
        let saved = database
            .set_calendar_clients(CalendarClients {
                google_client_id: " 123-abc.apps.googleusercontent.com ".to_owned(),
                microsoft_client_id: "6731de76-14a6-49ae-97bc-6eba6914391e".to_owned(),
            })
            .expect("save");
        assert_eq!(saved.google_client_id, "123-abc.apps.googleusercontent.com");
        assert_eq!(
            saved.client_id(Provider::Microsoft),
            Some("6731de76-14a6-49ae-97bc-6eba6914391e")
        );
        assert_eq!(database.calendar_clients().expect("clients"), saved);
        for bad in [
            CalendarClients {
                google_client_id: "a b".to_owned(),
                ..CalendarClients::default()
            },
            CalendarClients {
                microsoft_client_id: "non-guid".to_owned(),
                ..CalendarClients::default()
            },
        ] {
            assert!(database.set_calendar_clients(bad).is_err());
        }
        assert_eq!(CalendarClients::default().client_id(Provider::Google), None);
    }
}
