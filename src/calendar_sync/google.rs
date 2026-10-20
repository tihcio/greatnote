//! Google Calendar API v3.

use std::time::Duration;

use serde_json::{Value, json};

use super::{
    CalendarError, CalendarService, EventDraft, RemoteCalendar, RemoteEvent,
    http::{Body, Http, api_error, encode, query},
    parse_remote_instant, previous_day,
};

const BASE_URL: &str = "https://www.googleapis.com/calendar/v3";
const MAX_PAGES: usize = 40;

pub struct GoogleCalendar {
    http: Http,
    token: String,
    base_url: String,
}

impl GoogleCalendar {
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

    fn call(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: Body<'_>,
    ) -> Result<Value, CalendarError> {
        let url = format!("{}{path}", self.base_url);
        let response = self
            .http
            .send(method, &url, Some(&self.token), headers, body)?;
        if response.status >= 400 {
            return Err(api_error(&response));
        }
        Ok(response.body)
    }

    fn event_path(calendar: &str, event: Option<&str>) -> String {
        match event {
            Some(event) => format!("/calendars/{}/events/{}", encode(calendar), encode(event)),
            None => format!("/calendars/{}/events", encode(calendar)),
        }
    }
}

/// Corpo di un evento per creazione e modifica. Per i giorni interi Google
/// usa una fine esclusiva.
fn event_body(draft: &EventDraft) -> Result<Value, CalendarError> {
    let (start, end) = if draft.all_day {
        let start = draft
            .start_date
            .as_deref()
            .ok_or(CalendarError::InvalidEvent)?;
        let end = draft
            .end_date
            .as_deref()
            .ok_or(CalendarError::InvalidEvent)?;
        let exclusive = super::next_day_string(end)?;
        (json!({ "date": start }), json!({ "date": exclusive }))
    } else {
        let start = draft
            .start_at
            .as_deref()
            .ok_or(CalendarError::InvalidEvent)?;
        let end = draft.end_at.as_deref().ok_or(CalendarError::InvalidEvent)?;
        (
            json!({ "dateTime": start, "timeZone": draft.timezone }),
            json!({ "dateTime": end, "timeZone": draft.timezone }),
        )
    };
    Ok(json!({
        "summary": draft.title,
        "description": draft.description,
        "start": start,
        "end": end,
    }))
}

/// Evento di Google in forma comune; `None` per gli eventi annullati.
pub(crate) fn parse_event(value: &Value) -> Result<Option<RemoteEvent>, CalendarError> {
    if value.get("status").and_then(Value::as_str) == Some("cancelled") {
        return Ok(None);
    }
    let remote_id = string(value, "id").ok_or_else(|| invalid("evento senza id"))?;
    let start = value
        .get("start")
        .ok_or_else(|| invalid("evento senza inizio"))?;
    let end = value.get("end").unwrap_or(start);
    let mut event = RemoteEvent {
        remote_id,
        etag: string(value, "etag"),
        title: string(value, "summary").unwrap_or_default(),
        description: string(value, "description").unwrap_or_default(),
        all_day: false,
        start_at: None,
        end_at: None,
        start_date: None,
        end_date: None,
        timezone: start
            .get("timeZone")
            .and_then(Value::as_str)
            .map(str::to_owned),
        recurring: value.get("recurringEventId").is_some(),
        web_link: string(value, "htmlLink"),
    };
    if let Some(date) = start.get("date").and_then(Value::as_str) {
        let end_date = end.get("date").and_then(Value::as_str).unwrap_or(date);
        event.all_day = true;
        event.start_date = Some(date.to_owned());
        event.end_date = Some(previous_day(end_date, date)?);
    } else {
        let start_at = start
            .get("dateTime")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("inizio senza orario"))?;
        let end_at = end
            .get("dateTime")
            .and_then(Value::as_str)
            .unwrap_or(start_at);
        event.start_at = Some(parse_remote_instant(start_at)?);
        event.end_at = Some(parse_remote_instant(end_at)?);
    }
    Ok(Some(event))
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn invalid(message: &str) -> CalendarError {
    CalendarError::InvalidResponse(message.to_owned())
}

fn single(value: &Value) -> Result<RemoteEvent, CalendarError> {
    parse_event(value)?.ok_or_else(|| invalid("evento annullato"))
}

impl CalendarService for GoogleCalendar {
    fn account_label(&self) -> Result<String, CalendarError> {
        // L'id del calendario principale è l'indirizzo dell'account.
        let value = self.call("GET", "/users/me/calendarList/primary", &[], Body::None)?;
        string(&value, "id").ok_or_else(|| invalid("calendario principale senza id"))
    }

    fn list_calendars(&self) -> Result<Vec<RemoteCalendar>, CalendarError> {
        let mut calendars = Vec::new();
        let mut page_token: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let mut pairs = vec![("maxResults", "250")];
            if let Some(token) = &page_token {
                pairs.push(("pageToken", token));
            }
            let value = self.call(
                "GET",
                &format!("/users/me/calendarList?{}", query(&pairs)),
                &[],
                Body::None,
            )?;
            for item in value
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(remote_id) = string(item, "id") else {
                    continue;
                };
                let role = item.get("accessRole").and_then(Value::as_str).unwrap_or("");
                calendars.push(RemoteCalendar {
                    name: string(item, "summaryOverride")
                        .or_else(|| string(item, "summary"))
                        .unwrap_or_else(|| remote_id.clone()),
                    remote_id,
                    color: string(item, "backgroundColor"),
                    can_edit: matches!(role, "owner" | "writer"),
                    is_primary: item
                        .get("primary")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                });
            }
            page_token = string(&value, "nextPageToken");
            if page_token.is_none() {
                return Ok(calendars);
            }
        }
        Ok(calendars)
    }

    fn list_events(
        &self,
        calendar: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<RemoteEvent>, CalendarError> {
        let mut events = Vec::new();
        let mut page_token: Option<String> = None;
        for _ in 0..MAX_PAGES {
            let mut pairs = vec![
                // Le ricorrenze arrivano come singole occorrenze.
                ("singleEvents", "true"),
                ("timeMin", from),
                ("timeMax", to),
                ("maxResults", "2500"),
                ("showDeleted", "false"),
            ];
            if let Some(token) = &page_token {
                pairs.push(("pageToken", token));
            }
            let path = format!("{}?{}", Self::event_path(calendar, None), query(&pairs));
            let value = self.call("GET", &path, &[], Body::None)?;
            for item in value
                .get("items")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(event) = parse_event(item)? {
                    events.push(event);
                }
            }
            page_token = string(&value, "nextPageToken");
            if page_token.is_none() {
                break;
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
            &Self::event_path(calendar, None),
            &[],
            Body::Json(&body),
        )?)
    }

    fn update_event(
        &self,
        calendar: &str,
        remote_id: &str,
        etag: Option<&str>,
        draft: &EventDraft,
    ) -> Result<RemoteEvent, CalendarError> {
        let body = event_body(draft)?;
        let headers: Vec<(&str, &str)> = etag.map(|etag| ("If-Match", etag)).into_iter().collect();
        single(&self.call(
            "PATCH",
            &Self::event_path(calendar, Some(remote_id)),
            &headers,
            Body::Json(&body),
        )?)
    }

    fn delete_event(
        &self,
        calendar: &str,
        remote_id: &str,
        etag: Option<&str>,
    ) -> Result<(), CalendarError> {
        let headers: Vec<(&str, &str)> = etag.map(|etag| ("If-Match", etag)).into_iter().collect();
        match self.call(
            "DELETE",
            &Self::event_path(calendar, Some(remote_id)),
            &headers,
            Body::None,
        ) {
            // Già eliminato altrove: il risultato è quello voluto.
            Ok(_) | Err(CalendarError::NotFound) => Ok(()),
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timed_all_day_recurring_and_cancelled_events() {
        let timed = parse_event(&json!({
            "id": "e1", "etag": "\"3\"", "status": "confirmed", "summary": "Riunione",
            "start": {"dateTime": "2026-03-29T09:30:00+02:00", "timeZone": "Europe/Rome"},
            "end": {"dateTime": "2026-03-29T10:30:00+02:00"},
            "htmlLink": "https://www.google.com/calendar/event?eid=x"
        }))
        .expect("parse")
        .expect("event");
        assert_eq!(timed.start_at.as_deref(), Some("2026-03-29T07:30:00Z"));
        assert_eq!(timed.end_at.as_deref(), Some("2026-03-29T08:30:00Z"));
        assert_eq!(timed.timezone.as_deref(), Some("Europe/Rome"));
        assert!(!timed.recurring && !timed.all_day);

        let all_day = parse_event(&json!({
            "id": "e2", "start": {"date": "2026-10-20"}, "end": {"date": "2026-10-22"},
            "recurringEventId": "serie"
        }))
        .expect("parse")
        .expect("event");
        assert!(all_day.all_day && all_day.recurring);
        assert_eq!(all_day.start_date.as_deref(), Some("2026-10-20"));
        // Fine esclusiva di Google → fine inclusa di Greatnote.
        assert_eq!(all_day.end_date.as_deref(), Some("2026-10-21"));
        assert_eq!(all_day.title, "");

        assert!(
            parse_event(&json!({"id": "e3", "status": "cancelled"}))
                .expect("parse")
                .is_none()
        );
        assert!(parse_event(&json!({"start": {"date": "2026-10-20"}})).is_err());
    }

    #[test]
    fn body_uses_exclusive_end_for_all_day() {
        let draft = EventDraft {
            title: "Ferie".to_owned(),
            description: String::new(),
            all_day: true,
            start_at: None,
            end_at: None,
            start_date: Some("2026-12-31".to_owned()),
            end_date: Some("2026-12-31".to_owned()),
            timezone: "Europe/Rome".to_owned(),
        };
        let body = event_body(&draft).expect("body");
        assert_eq!(body["start"]["date"], "2026-12-31");
        assert_eq!(body["end"]["date"], "2027-01-01");
    }

    #[test]
    fn http_paging_and_conditional_requests() {
        use crate::calendar_sync::http::fake::FakeServer;
        let server = FakeServer::start(vec![
            (200, json!({"items": [{"id": "a", "etag": "\"1\"", "summary": "Uno", "start": {"date": "2026-10-20"}, "end": {"date": "2026-10-21"}}], "nextPageToken": "p2"}).to_string()),
            (200, json!({"items": [{"id": "b", "status": "cancelled"}, {"id": "c", "summary": "Tre", "start": {"dateTime": "2026-10-20T10:00:00Z"}, "end": {"dateTime": "2026-10-20T11:00:00Z"}}]}).to_string()),
            (412, json!({"error": {"code": 412, "message": "Precondition Failed"}}).to_string()),
            (410, String::new()),
            (401, json!({"error": {"message": "Invalid Credentials"}}).to_string()),
        ]);
        let api = GoogleCalendar::new("tok", Duration::from_secs(5)).with_base_url(&server.url);
        let events = api
            .list_events(
                "anna@example.com",
                "2026-10-01T00:00:00Z",
                "2026-11-01T00:00:00Z",
            )
            .expect("events");
        assert_eq!(
            events
                .iter()
                .map(|event| event.remote_id.as_str())
                .collect::<Vec<_>>(),
            ["a", "c"]
        );
        let draft = EventDraft {
            title: "Uno".to_owned(),
            description: String::new(),
            all_day: true,
            start_at: None,
            end_at: None,
            start_date: Some("2026-10-20".to_owned()),
            end_date: Some("2026-10-20".to_owned()),
            timezone: "Europe/Rome".to_owned(),
        };
        assert!(matches!(
            api.update_event("anna@example.com", "a", Some("\"1\""), &draft),
            Err(CalendarError::Conflict)
        ));
        api.delete_event("anna@example.com", "a", None)
            .expect("già eliminato");
        assert!(matches!(
            api.list_calendars(),
            Err(CalendarError::Unauthorized)
        ));
        let requests = server.finish();
        assert!(requests[0].starts_with("GET /calendars/anna%40example.com/events?singleEvents=true&timeMin=2026-10-01T00%3A00%3A00Z"));
        assert!(
            requests[0]
                .to_ascii_lowercase()
                .contains("authorization: bearer tok")
        );
        assert!(requests[1].contains("pageToken=p2"));
        assert!(requests[2].starts_with("PATCH /calendars/anna%40example.com/events/a "));
        assert!(requests[2].to_ascii_lowercase().contains("if-match: \"1\""));
        assert!(requests[2].contains("\"date\":\"2026-10-21\""));
        assert!(requests[3].starts_with("DELETE "));
    }
}
