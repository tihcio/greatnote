//! Abbonamenti a calendari iCal in sola lettura (ADR 0016): niente login né
//! registrazioni, basta l'indirizzo segreto del calendario. L'indirizzo è
//! conservato dal chiamante nel portachiavi; qui arrivano solo i dati.

use std::time::Duration;

use jiff::{Timestamp, tz::TimeZone};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use uuid::Uuid;

use super::{
    CalendarError, NOW_SQL, RemoteEvent, clip, delete_event_reminders,
    http::{Body, Http, api_error},
    ics::parse_calendar,
    normalize, sync_window,
};
use crate::storage::{Database, StorageError};

const FETCH_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_URL_LENGTH: usize = 2048;
pub const PROVIDER_ICS: &str = "ics";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CalendarSubscription {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
    /// Eventi copiati in Greatnote.
    pub events: i64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct SubscriptionReport {
    pub events: usize,
    pub removed: usize,
}

/// Indirizzo valido per un abbonamento: `https://` (o `webcal://`, che è lo
/// stesso indirizzo in https). `http://` solo verso questo computer.
pub fn normalize_ics_url(input: &str) -> Result<String, CalendarError> {
    let url = input.trim();
    let invalid = || CalendarError::InvalidUrl;
    if url.is_empty() || url.len() > MAX_URL_LENGTH || url.chars().any(char::is_whitespace) {
        return Err(invalid());
    }
    let lower = url.to_ascii_lowercase();
    let url = if let Some(rest) = lower
        .strip_prefix("webcal://")
        .map(|_| &url["webcal://".len()..])
    {
        format!("https://{rest}")
    } else if lower.starts_with("https://")
        || lower.starts_with("http://127.0.0.1")
        || lower.starts_with("http://localhost")
    {
        url.to_owned()
    } else {
        return Err(invalid());
    };
    let host = url
        .split("://")
        .nth(1)
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .unwrap_or("");
    if host.is_empty() || host.contains('@') {
        return Err(invalid());
    }
    Ok(url)
}

/// Scarica il calendario (senza reindirizzamenti, al massimo 16 MB).
pub fn fetch_ics(url: &str) -> Result<String, CalendarError> {
    let url = normalize_ics_url(url)?;
    let response = Http::new(FETCH_TIMEOUT).send(
        "GET",
        &url,
        None,
        &[("Accept", "text/calendar, */*")],
        Body::None,
    )?;
    if (300..400).contains(&response.status) {
        return Err(CalendarError::InvalidResponse(
            "l'indirizzo reindirizza altrove: usa l'indirizzo iCal diretto".to_owned(),
        ));
    }
    if response.status >= 400 {
        return Err(match api_error(&response) {
            CalendarError::NotFound | CalendarError::Forbidden(_) | CalendarError::Unauthorized => {
                CalendarError::InvalidResponse(
                    "calendario non trovato o indirizzo non più valido (può essere stato reimpostato)".to_owned(),
                )
            }
            other => other,
        });
    }
    match response.body {
        serde_json::Value::String(text) => Ok(text),
        _ => Err(CalendarError::InvalidResponse(
            "il contenuto non è un calendario iCal".to_owned(),
        )),
    }
}

impl Database {
    pub fn add_calendar_subscription(
        &self,
        name: &str,
        color: Option<&str>,
    ) -> Result<String, StorageError> {
        let name = clip(name.trim(), 200);
        if name.is_empty() {
            return Err(StorageError::InvalidEvent("dai un nome al calendario"));
        }
        let id = Uuid::new_v4().to_string();
        self.connection()?.execute(
            "INSERT INTO calendar_subscriptions (id, name, color) VALUES (?1, ?2, ?3)",
            params![id, name, valid_color(color)],
        )?;
        Ok(id)
    }

    pub fn calendar_subscriptions(&self) -> Result<Vec<CalendarSubscription>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT s.id, s.name, s.color, s.last_sync_at, s.last_error,
                    (SELECT COUNT(*) FROM events e WHERE e.subscription_id = s.id)
             FROM calendar_subscriptions s ORDER BY s.created_at, s.id",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(CalendarSubscription {
                id: row.get(0)?,
                name: row.get(1)?,
                color: row.get(2)?,
                last_sync_at: row.get(3)?,
                last_error: row.get(4)?,
                events: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn calendar_subscription(&self, id: &str) -> Result<CalendarSubscription, StorageError> {
        self.calendar_subscriptions()?
            .into_iter()
            .find(|subscription| subscription.id == id)
            .ok_or(StorageError::CalendarNotFound)
    }

    pub fn update_calendar_subscription(
        &self,
        id: &str,
        name: &str,
        color: Option<&str>,
    ) -> Result<(), StorageError> {
        let name = clip(name.trim(), 200);
        if name.is_empty() {
            return Err(StorageError::InvalidEvent("dai un nome al calendario"));
        }
        let changed = self.connection()?.execute(
            &format!("UPDATE calendar_subscriptions SET name = ?2, color = ?3, updated_at = {NOW_SQL} WHERE id = ?1"),
            params![id, name, valid_color(color)],
        )?;
        if changed == 0 {
            return Err(StorageError::CalendarNotFound);
        }
        Ok(())
    }

    /// Rimuove l'abbonamento, i suoi eventi copiati e i loro promemoria.
    pub fn delete_calendar_subscription(&self, id: &str) -> Result<bool, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        delete_event_reminders(&transaction, "subscription_id = ?1", id)?;
        let deleted =
            transaction.execute("DELETE FROM calendar_subscriptions WHERE id = ?1", [id])?;
        transaction.commit()?;
        Ok(deleted > 0)
    }

    pub fn record_subscription_sync(
        &self,
        id: &str,
        error: Option<&str>,
    ) -> Result<(), StorageError> {
        let connection = self.connection()?;
        match error {
            None => connection.execute(
                &format!(
                    "UPDATE calendar_subscriptions SET last_sync_at = {NOW_SQL}, last_error = NULL,
                         updated_at = {NOW_SQL} WHERE id = ?1"
                ),
                [id],
            )?,
            Some(error) => connection.execute(
                &format!("UPDATE calendar_subscriptions SET last_error = ?2, updated_at = {NOW_SQL} WHERE id = ?1"),
                params![id, clip(error, 1000)],
            )?,
        };
        Ok(())
    }

    /// Sostituisce gli eventi copiati di un abbonamento con quelli letti,
    /// mantenendo nota collegata e promemoria degli eventi che restano.
    pub fn store_subscription_events(
        &self,
        id: &str,
        events: &[RemoteEvent],
    ) -> Result<SubscriptionReport, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let exists: Option<String> = transaction
            .query_row(
                "SELECT id FROM calendar_subscriptions WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        if exists.is_none() {
            return Err(StorageError::CalendarNotFound);
        }
        let mut seen = std::collections::HashSet::new();
        for event in events {
            let Some(stored) = normalize(event, false) else {
                continue;
            };
            if !seen.insert(event.remote_id.as_str()) {
                continue;
            }
            transaction.execute(
                &format!(
                    "INSERT INTO events (id, title, description, all_day, start_at, end_at, start_date,
                                         end_date, timezone, calendar_provider, external_id,
                                         subscription_id, web_link, read_only)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, 1)
                     ON CONFLICT (subscription_id, external_id) WHERE subscription_id IS NOT NULL
                     DO UPDATE SET title = excluded.title, description = excluded.description,
                         all_day = excluded.all_day, start_at = excluded.start_at,
                         end_at = excluded.end_at, start_date = excluded.start_date,
                         end_date = excluded.end_date, timezone = excluded.timezone,
                         web_link = excluded.web_link, deleted_at = NULL,
                         updated_at = CASE
                             WHEN events.title IS excluded.title AND events.description IS excluded.description
                              AND events.start_at IS excluded.start_at AND events.end_at IS excluded.end_at
                              AND events.start_date IS excluded.start_date AND events.end_date IS excluded.end_date
                             THEN events.updated_at ELSE {NOW_SQL} END"
                ),
                params![
                    Uuid::new_v4().to_string(),
                    stored.title,
                    stored.description,
                    stored.all_day,
                    stored.start_at,
                    stored.end_at,
                    stored.start_date,
                    stored.end_date,
                    stored.timezone,
                    PROVIDER_ICS,
                    event.remote_id,
                    id,
                    stored.web_link,
                ],
            )?;
        }
        let existing: Vec<(String, String)> = transaction
            .prepare("SELECT id, external_id FROM events WHERE subscription_id = ?1")?
            .query_map([id], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let mut removed = 0;
        for (event_id, remote_id) in existing {
            if !seen.contains(remote_id.as_str()) {
                delete_event_reminders(&transaction, "id = ?1", &event_id)?;
                removed += transaction.execute("DELETE FROM events WHERE id = ?1", [&event_id])?;
            }
        }
        transaction.commit()?;
        Ok(SubscriptionReport {
            events: seen.len(),
            removed,
        })
    }
}

fn valid_color(color: Option<&str>) -> Option<String> {
    color
        .map(str::trim)
        .filter(|color| {
            let hex = color.strip_prefix('#').unwrap_or("");
            (hex.len() == 3 || hex.len() == 6) && hex.chars().all(|c| c.is_ascii_hexdigit())
        })
        .map(str::to_ascii_lowercase)
}

/// Legge il testo di un calendario e aggiorna gli eventi dell'abbonamento.
pub fn apply_ics(
    database: &Database,
    id: &str,
    text: &str,
    now: Timestamp,
    zone: &TimeZone,
) -> Result<SubscriptionReport, CalendarError> {
    let (from, to) = sync_window(now)?;
    let parse = |value: &str| {
        value
            .parse::<Timestamp>()
            .map_err(|_| CalendarError::InvalidEvent)
    };
    let calendar = parse_calendar(text, parse(&from)?, parse(&to)?, zone)
        .map_err(|error| CalendarError::InvalidResponse(error.to_string()))?;
    let report = database.store_subscription_events(id, &calendar.events)?;
    database.record_subscription_sync(id, None)?;
    Ok(report)
}

/// Come [`apply_ics`], adesso e nel fuso del sistema.
pub fn apply_ics_now(
    database: &Database,
    id: &str,
    text: &str,
) -> Result<SubscriptionReport, CalendarError> {
    apply_ics(database, id, text, Timestamp::now(), &TimeZone::system())
}

/// Verifica che il testo sia un calendario iCal.
pub fn validate_ics(text: &str) -> Result<(), CalendarError> {
    let now = Timestamp::now();
    parse_calendar(text, now, now, &TimeZone::UTC)
        .map(|_| ())
        .map_err(|error| CalendarError::InvalidResponse(error.to_string()))
}

/// Nome suggerito dal calendario (`X-WR-CALNAME`), per un nuovo abbonamento.
pub fn calendar_name(text: &str) -> Option<String> {
    let now = Timestamp::now();
    parse_calendar(text, now, now, &TimeZone::UTC)
        .ok()
        .and_then(|calendar| calendar.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        calendar_sync::http::fake::FakeServer,
        domain::planner::{EventInput, ReminderEntity},
        time::Zone,
        trash::TrashKind,
    };

    const ICS: &str = "BEGIN:VCALENDAR\r\nX-WR-CALNAME:Famiglia\r\n\
        BEGIN:VEVENT\r\nUID:cena\r\nSUMMARY:Cena\r\nDTSTART:20261021T180000Z\r\nDTEND:20261021T200000Z\r\nEND:VEVENT\r\n\
        BEGIN:VEVENT\r\nUID:palestra\r\nSUMMARY:Palestra\r\nDTSTART;TZID=Europe/Rome:20261019T190000\r\nDTEND;TZID=Europe/Rome:20261019T200000\r\nRRULE:FREQ=WEEKLY;COUNT=3\r\nEND:VEVENT\r\n\
        END:VCALENDAR\r\n";

    fn rome() -> TimeZone {
        TimeZone::get("Europe/Rome").expect("zone")
    }

    #[test]
    fn urls_are_checked() {
        assert_eq!(
            normalize_ics_url(
                " webcal://calendar.google.com/calendar/ical/x%40gmail.com/private-abc/basic.ics "
            )
            .expect("url"),
            "https://calendar.google.com/calendar/ical/x%40gmail.com/private-abc/basic.ics"
        );
        assert!(
            normalize_ics_url("https://outlook.office365.com/owa/calendar/a/b/calendar.ics")
                .is_ok()
        );
        for bad in [
            "http://example.com/a.ics",
            "file:///etc/passwd",
            "https://",
            "https://user@host/a",
            "https://a b",
            "javascript:alert(1)",
        ] {
            assert!(normalize_ics_url(bad).is_err(), "{bad}");
        }
        assert!(normalize_ics_url(&format!("https://a.com/{}", "x".repeat(3000))).is_err());
    }

    #[test]
    fn subscription_sync_is_read_only_and_keeps_links() {
        let database = Database::in_memory().expect("db");
        assert_eq!(calendar_name(ICS).as_deref(), Some("Famiglia"));
        let id = database
            .add_calendar_subscription("Famiglia", Some("#33AA77"))
            .expect("add");
        assert!(database.add_calendar_subscription("  ", None).is_err());
        let now: Timestamp = "2026-10-20T10:00:00Z".parse().expect("now");
        let report = apply_ics(&database, &id, ICS, now, &rome()).expect("apply");
        assert_eq!(
            report,
            SubscriptionReport {
                events: 4,
                removed: 0
            }
        );
        let subscription = database.calendar_subscription(&id).expect("subscription");
        assert_eq!(
            (subscription.events, subscription.color.as_deref()),
            (4, Some("#33aa77"))
        );
        assert!(subscription.last_sync_at.is_some());

        let zone = Zone::resolve(Some("Europe/Rome")).expect("zone");
        let day = |value: &str| value.parse().expect("date");
        let dinner = database
            .list_events(day("2026-10-21"), day("2026-10-21"), &zone)
            .expect("events")
            .into_iter()
            .find(|event| event.title == "Cena")
            .expect("dinner");
        assert!(dinner.read_only);
        assert_eq!(dinner.calendar_provider.as_deref(), Some("ics"));
        assert_eq!(dinner.subscription_id.as_deref(), Some(id.as_str()));
        let input = EventInput {
            title: "x".to_owned(),
            description: String::new(),
            all_day: true,
            start: "2026-10-21".to_owned(),
            end: "2026-10-21".to_owned(),
            timezone: None,
            note_id: None,
        };
        assert!(matches!(
            database.update_event(&dinner.id, &input),
            Err(StorageError::ExternalEvent)
        ));
        assert!(matches!(
            database.move_item_to_trash(TrashKind::Event, &dinner.id),
            Err(StorageError::ExternalEvent)
        ));
        database
            .create_reminder(
                ReminderEntity::Event,
                &dinner.id,
                "2026-10-21T19:30",
                Some("Europe/Rome"),
                now,
            )
            .expect("reminder");

        // Nuova lettura: la cena resta (con il promemoria), la palestra sparisce.
        let only_dinner = ICS
            .replace("RRULE:FREQ=WEEKLY;COUNT=3", "")
            .replace("UID:palestra", "UID:altro");
        let report = apply_ics(&database, &id, &only_dinner, now, &rome()).expect("apply");
        assert_eq!(
            report,
            SubscriptionReport {
                events: 2,
                removed: 3
            }
        );
        assert_eq!(
            database.get_event(&dinner.id).expect("dinner").id,
            dinner.id
        );
        assert_eq!(database.list_reminders().expect("reminders").len(), 1);

        // Errore registrato senza toccare gli eventi.
        assert!(apply_ics(&database, &id, "<html>", now, &rome()).is_err());
        database
            .record_subscription_sync(&id, Some("rete"))
            .expect("record");
        assert_eq!(
            database
                .calendar_subscription(&id)
                .expect("sub")
                .last_error
                .as_deref(),
            Some("rete")
        );
        assert_eq!(database.calendar_subscription(&id).expect("sub").events, 2);

        database
            .update_calendar_subscription(&id, "Casa", Some("rosso"))
            .expect("update");
        let renamed = database.calendar_subscription(&id).expect("sub");
        assert_eq!((renamed.name.as_str(), renamed.color), ("Casa", None));

        assert!(database.delete_calendar_subscription(&id).expect("delete"));
        assert!(database.get_event(&dinner.id).is_err());
        assert!(database.list_reminders().expect("reminders").is_empty());
    }

    #[test]
    fn fetch_over_http() {
        let server = FakeServer::start(vec![
            (200, ICS.to_owned()),
            (404, String::new()),
            (302, String::new()),
        ]);
        let url = format!("{}/calendar.ics", server.url);
        assert!(fetch_ics(&url).expect("fetch").contains("BEGIN:VCALENDAR"));
        assert!(matches!(
            fetch_ics(&url),
            Err(CalendarError::InvalidResponse(_))
        ));
        assert!(fetch_ics(&url).is_err());
        let requests = server.finish();
        assert!(requests[0].starts_with("GET /calendar.ics "));
        assert!(!requests[0].to_ascii_lowercase().contains("authorization"));
    }
}
