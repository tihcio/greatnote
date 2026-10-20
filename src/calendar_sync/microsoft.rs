//! Microsoft Graph v1.0 (calendari di Outlook.com e Microsoft 365).

use std::time::Duration;

use serde_json::{Value, json};

use super::{
    CalendarError, CalendarService, EventDraft, RemoteCalendar, RemoteEvent,
    http::{Body, Http, api_error, encode, query},
    next_day_string, previous_day,
};

const BASE_URL: &str = "https://graph.microsoft.com/v1.0";
const MAX_PAGES: usize = 40;
/// Orari in UTC e descrizione in testo semplice (non HTML).
const PREFER: &str = "outlook.timezone=\"UTC\", outlook.body-content-type=\"text\"";
const EVENT_FIELDS: &str =
    "id,subject,body,start,end,isAllDay,type,webLink,isCancelled,originalStartTimeZone";

pub struct MicrosoftCalendar {
    http: Http,
    token: String,
    base_url: String,
}

impl MicrosoftCalendar {
    pub fn new(access_token: &str, timeout: Duration) -> Self {
        Self {
            http: Http::new(timeout),
            token: access_token.to_owned(),
            base_url: BASE_URL.to_owned(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_base_url(mut self, base_url: &str) -> Self {
        self.base_url = base_url.to_owned();
        self
    }

    fn call_url(
        &self,
        method: &str,
        url: &str,
        headers: &[(&str, &str)],
        body: Body<'_>,
    ) -> Result<Value, CalendarError> {
        let mut all_headers = vec![("Prefer", PREFER)];
        all_headers.extend_from_slice(headers);
        let response = self
            .http
            .send(method, url, Some(&self.token), &all_headers, body)?;
        if response.status >= 400 {
            return Err(api_error(&response));
        }
        Ok(response.body)
    }

    fn call(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Body<'_>,
    ) -> Result<Value, CalendarError> {
        self.call_url(method, &format!("{}{path}", self.base_url), headers, body)
    }

    /// Elementi di tutte le pagine (`@odata.nextLink`), solo sullo stesso
    /// server delle API.
    fn collect(&self, first_path: &str) -> Result<Vec<Value>, CalendarError> {
        let mut items = Vec::new();
        let mut url = format!("{}{first_path}", self.base_url);
        for _ in 0..MAX_PAGES {
            let value = self.call_url("GET", &url, &[], Body::None)?;
            items.extend(
                value
                    .get("value")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
            match value.get("@odata.nextLink").and_then(Value::as_str) {
                Some(next) if next.starts_with(&format!("{}/", self.base_url)) => {
                    url = next.to_owned()
                }
                Some(_) => return Err(invalid("pagina successiva su un altro server")),
                None => break,
            }
        }
        Ok(items)
    }
}

/// Graph restituisce `2026-10-20T09:00:00.0000000` (con `Prefer` in UTC).
fn utc_instant(value: &Value) -> Result<String, CalendarError> {
    let text = value
        .get("dateTime")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("orario mancante"))?;
    let zone = value
        .get("timeZone")
        .and_then(Value::as_str)
        .unwrap_or("UTC");
    if !zone.eq_ignore_ascii_case("UTC") {
        return Err(invalid("orario non in UTC"));
    }
    super::parse_remote_instant(&format!("{}Z", text.trim_end_matches('Z')))
}

fn date_part(value: &Value) -> Result<String, CalendarError> {
    value
        .get("dateTime")
        .and_then(Value::as_str)
        .and_then(|text| text.get(..10))
        .map(str::to_owned)
        .ok_or_else(|| invalid("data mancante"))
}

pub(crate) fn parse_event(value: &Value) -> Result<Option<RemoteEvent>, CalendarError> {
    if value.get("isCancelled").and_then(Value::as_bool) == Some(true) {
        return Ok(None);
    }
    let remote_id = string(value, "id").ok_or_else(|| invalid("evento senza id"))?;
    let start = value
        .get("start")
        .ok_or_else(|| invalid("evento senza inizio"))?;
    let end = value.get("end").unwrap_or(start);
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("singleInstance");
    let mut event = RemoteEvent {
        remote_id,
        etag: string(value, "@odata.etag"),
        title: string(value, "subject").unwrap_or_default(),
        description: value
            .pointer("/body/content")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim()
            .to_owned(),
        all_day: value
            .get("isAllDay")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        start_at: None,
        end_at: None,
        start_date: None,
        end_date: None,
        // Spesso è un nome Windows ("W. Europe Standard Time"): viene usato
        // solo se è un fuso IANA valido.
        timezone: string(value, "originalStartTimeZone"),
        recurring: matches!(kind, "occurrence" | "exception" | "seriesMaster"),
        web_link: string(value, "webLink"),
    };
    if event.all_day {
        let start_date = date_part(start)?;
        let end_date = date_part(end)?;
        event.end_date = Some(previous_day(&end_date, &start_date)?);
        event.start_date = Some(start_date);
    } else {
        event.start_at = Some(utc_instant(start)?);
        event.end_at = Some(utc_instant(end)?);
    }
    Ok(Some(event))
}

fn event_body(draft: &EventDraft) -> Result<Value, CalendarError> {
    let (start, end) = if draft.all_day {
        let start = draft
            .start_date
            .as_deref()
            .ok_or(CalendarError::InvalidEvent)?;
        let end = next_day_string(
            draft
                .end_date
                .as_deref()
                .ok_or(CalendarError::InvalidEvent)?,
        )?;
        (format!("{start}T00:00:00"), format!("{end}T00:00:00"))
    } else {
        let start = draft
            .start_at
            .as_deref()
            .ok_or(CalendarError::InvalidEvent)?;
        let end = draft.end_at.as_deref().ok_or(CalendarError::InvalidEvent)?;
        (
            start.trim_end_matches('Z').to_owned(),
            end.trim_end_matches('Z').to_owned(),
        )
    };
    Ok(json!({
        "subject": draft.title,
        "body": { "contentType": "text", "content": draft.description },
        "isAllDay": draft.all_day,
        "start": { "dateTime": start, "timeZone": "UTC" },
        "end": { "dateTime": end, "timeZone": "UTC" },
    }))
}

fn string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn invalid(message: &str) -> CalendarError {
    CalendarError::InvalidResponse(message.to_owned())
}

fn single(value: &Value) -> Result<RemoteEvent, CalendarError> {
    parse_event(value)?.ok_or_else(|| invalid("evento annullato"))
}

impl CalendarService for MicrosoftCalendar {
    fn account_label(&self) -> Result<String, CalendarError> {
        let value = self.call(
            "GET",
            "/me?$select=mail,userPrincipalName,displayName",
            &[],
            Body::None,
        )?;
        string(&value, "mail")
            .or_else(|| string(&value, "userPrincipalName"))
            .or_else(|| string(&value, "displayName"))
            .ok_or_else(|| invalid("account senza indirizzo"))
    }

    fn list_calendars(&self) -> Result<Vec<RemoteCalendar>, CalendarError> {
        let items = self
            .collect("/me/calendars?$top=100&$select=id,name,hexColor,canEdit,isDefaultCalendar")?;
        Ok(items
            .iter()
            .filter_map(|item| {
                let remote_id = string(item, "id")?;
                Some(RemoteCalendar {
                    name: string(item, "name").unwrap_or_else(|| "Calendario".to_owned()),
                    remote_id,
                    color: string(item, "hexColor"),
                    can_edit: item
                        .get("canEdit")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    is_primary: item
                        .get("isDefaultCalendar")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                })
            })
            .collect())
    }

    fn list_events(
        &self,
        calendar: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<RemoteEvent>, CalendarError> {
        // calendarView espande le ricorrenze nelle singole occorrenze.
        let path = format!(
            "/me/calendars/{}/calendarView?{}",
            encode(calendar),
            query(&[
                ("startDateTime", from),
                ("endDateTime", to),
                ("$top", "500"),
                ("$select", EVENT_FIELDS),
            ])
        );
        let mut events = Vec::new();
        for item in self.collect(&path)? {
            if let Some(event) = parse_event(&item)? {
                events.push(event);
            }
        }
        Ok(events)
    }

    fn create_event(
        &self,
        calendar: &str,
        draft: &EventDraft,
    ) -> Result<RemoteEvent, CalendarError> {
        let body = event_body(draft)?;
        single(&self.call(
            "POST",
            &format!("/me/calendars/{}/events", encode(calendar)),
            &[],
            Body::Json(&body),
        )?)
    }

    fn update_event(
        &self,
        _calendar: &str,
        remote_id: &str,
        etag: Option<&str>,
        draft: &EventDraft,
    ) -> Result<RemoteEvent, CalendarError> {
        let body = event_body(draft)?;
        let headers: Vec<(&str, &str)> = etag.map(|etag| ("If-Match", etag)).into_iter().collect();
        single(&self.call(
            "PATCH",
            &format!("/me/events/{}", encode(remote_id)),
            &headers,
            Body::Json(&body),
        )?)
    }

    fn delete_event(
        &self,
        _calendar: &str,
        remote_id: &str,
        etag: Option<&str>,
    ) -> Result<(), CalendarError> {
        let headers: Vec<(&str, &str)> = etag.map(|etag| ("If-Match", etag)).into_iter().collect();
        match self.call(
            "DELETE",
            &format!("/me/events/{}", encode(remote_id)),
            &headers,
            Body::None,
        ) {
            Ok(_) | Err(CalendarError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_graph_events() {
        let timed = parse_event(&json!({
            "@odata.etag": "W/\"abc\"", "id": "AAMk=", "subject": "Revisione",
            "body": {"contentType": "text", "content": " Ordine del giorno \r\n"},
            "isAllDay": false, "type": "occurrence", "isCancelled": false,
            "start": {"dateTime": "2026-10-25T00:30:00.0000000", "timeZone": "UTC"},
            "end": {"dateTime": "2026-10-25T02:30:00.0000000", "timeZone": "UTC"},
            "originalStartTimeZone": "W. Europe Standard Time",
            "webLink": "https://outlook.live.com/calendar/item/x"
        }))
        .expect("parse")
        .expect("event");
        assert_eq!(timed.start_at.as_deref(), Some("2026-10-25T00:30:00Z"));
        assert_eq!(timed.end_at.as_deref(), Some("2026-10-25T02:30:00Z"));
        assert_eq!(timed.description, "Ordine del giorno");
        assert_eq!(timed.etag.as_deref(), Some("W/\"abc\""));
        assert!(timed.recurring);

        let all_day = parse_event(&json!({
            "id": "B", "subject": "Ferie", "isAllDay": true, "type": "singleInstance",
            "start": {"dateTime": "2026-08-10T00:00:00.0000000", "timeZone": "UTC"},
            "end": {"dateTime": "2026-08-15T00:00:00.0000000", "timeZone": "UTC"}
        }))
        .expect("parse")
        .expect("event");
        assert_eq!(all_day.start_date.as_deref(), Some("2026-08-10"));
        assert_eq!(all_day.end_date.as_deref(), Some("2026-08-14"));
        assert!(!all_day.recurring);

        assert!(
            parse_event(&json!({"id": "C", "isCancelled": true}))
                .expect("parse")
                .is_none()
        );
        assert!(
            parse_event(&json!({
                "id": "D", "isAllDay": false,
                "start": {"dateTime": "2026-08-10T09:00:00", "timeZone": "Pacific Standard Time"}
            }))
            .is_err()
        );
    }

    #[test]
    fn body_in_utc() {
        let draft = EventDraft {
            title: "Call".to_owned(),
            description: "note".to_owned(),
            all_day: false,
            start_at: Some("2026-10-20T07:00:00Z".to_owned()),
            end_at: Some("2026-10-20T08:00:00Z".to_owned()),
            start_date: None,
            end_date: None,
            timezone: "Europe/Rome".to_owned(),
        };
        let body = event_body(&draft).expect("body");
        assert_eq!(
            body["start"],
            json!({"dateTime": "2026-10-20T07:00:00", "timeZone": "UTC"})
        );
        assert_eq!(body["body"]["contentType"], "text");
    }

    #[test]
    fn next_link_must_stay_on_the_api_server() {
        use crate::calendar_sync::http::fake::FakeServer;
        let server = FakeServer::start(vec![
            (200, json!({"value": [{"id": "c1", "name": "Calendario", "canEdit": true, "isDefaultCalendar": true, "hexColor": ""}], "@odata.nextLink": "{BASE}/me/calendars?$skip=1"}).to_string()),
            (200, json!({"value": [{"id": "c2", "name": "Compleanni", "canEdit": false}]}).to_string()),
            (200, json!({"value": [], "@odata.nextLink": "https://evil.example/v1.0/me/calendars"}).to_string()),
            (200, json!({"mail": null, "userPrincipalName": "anna@contoso.com"}).to_string()),
            (404, json!({"error": {"code": "ErrorItemNotFound", "message": "Not found"}}).to_string()),
        ]);
        let api = MicrosoftCalendar::new("tok", Duration::from_secs(5)).with_base_url(&server.url);
        let calendars = api.list_calendars().expect("calendars");
        assert_eq!(calendars.len(), 2);
        assert!(calendars[0].is_primary && calendars[0].can_edit && calendars[0].color.is_none());
        assert!(!calendars[1].can_edit);
        assert!(matches!(
            api.list_calendars(),
            Err(CalendarError::InvalidResponse(_))
        ));
        assert_eq!(api.account_label().expect("label"), "anna@contoso.com");
        api.delete_event("cal", "AAMk/=", Some("W/\"x\""))
            .expect("già eliminato");
        let requests = server.finish();
        assert!(requests[1].starts_with("GET /me/calendars?$skip=1 "));
        assert!(requests[0].contains("outlook.timezone=\"UTC\""));
        assert!(requests[4].starts_with("DELETE /me/events/AAMk%2F%3D "));
        assert!(
            requests[4]
                .to_ascii_lowercase()
                .contains("if-match: w/\"x\"")
        );
    }
}
