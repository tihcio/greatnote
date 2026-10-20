//! Dropbox come trasporto della sincronizzazione (ADR 0018): accesso con
//! OAuth 2 + PKCE nel browser (nessun segreto dell'app), permesso limitato
//! alla cartella dell'app (`Applicazioni/<nome app>`), file della libreria
//! letti e scritti con le API v2. Il protocollo resta quello di ADR 0017.

use std::{thread, time::Duration};

use serde_json::{Value, json};

use super::folder::{FolderError, ROOT, SyncStore};
use crate::calendar_sync::{
    CalendarError,
    http::{Body, Http, RawResponse, error_message, query},
    oauth::{Pkce, TokenSet},
};

const AUTH_URL: &str = "https://www.dropbox.com/oauth2/authorize";
const API_URL: &str = "https://api.dropboxapi.com";
const CONTENT_URL: &str = "https://content.dropboxapi.com";
/// Porta fissa: Dropbox confronta l'indirizzo di ritorno esattamente con
/// quello registrato nell'app (`http://localhost:41927/`).
pub const REDIRECT_PORT: u16 = 41927;
const TIMEOUT: Duration = Duration::from_secs(120);
const MAX_ATTEMPTS: u32 = 4;
const MAX_WAIT_SECONDS: u64 = 30;
const LIST_PAGES: usize = 200;

pub fn redirect_uri() -> String {
    format!("http://localhost:{REDIRECT_PORT}/")
}

/// Accesso all'account Dropbox per un'app registrata (solo App key).
pub struct DropboxAuth {
    app_key: String,
    api_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropboxAccount {
    pub account_id: String,
    /// Indirizzo email o nome mostrato all'utente.
    pub label: String,
}

fn oauth_error(body: &Value) -> CalendarError {
    if body.get("error").and_then(Value::as_str) == Some("invalid_grant") {
        CalendarError::Unauthorized
    } else {
        CalendarError::OAuth(error_message(body))
    }
}

fn token_set(body: &Value) -> Result<TokenSet, CalendarError> {
    let access_token = body
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| CalendarError::InvalidResponse("token di accesso assente".to_owned()))?;
    Ok(TokenSet {
        access_token: access_token.to_owned(),
        refresh_token: body
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::to_owned),
        expires_in: body
            .get("expires_in")
            .and_then(Value::as_u64)
            .unwrap_or(14_400),
    })
}

impl DropboxAuth {
    pub fn new(app_key: &str) -> Self {
        Self {
            app_key: app_key.trim().to_owned(),
            api_url: API_URL.to_owned(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_api_url(mut self, url: &str) -> Self {
        self.api_url = url.to_owned();
        self
    }

    /// Indirizzo da aprire nel browser. `token_access_type=offline` dà un
    /// token di aggiornamento che non scade finché l'accesso non viene
    /// revocato.
    pub fn authorization_url(&self, state: &str, pkce: &Pkce) -> String {
        format!(
            "{AUTH_URL}?{}",
            query(&[
                ("client_id", &self.app_key),
                ("response_type", "code"),
                ("redirect_uri", &redirect_uri()),
                ("state", state),
                ("code_challenge", &pkce.challenge),
                ("code_challenge_method", "S256"),
                ("token_access_type", "offline"),
            ])
        )
    }

    fn token(&self, fields: &[(&str, &str)]) -> Result<TokenSet, CalendarError> {
        let response = Http::new(Duration::from_secs(30)).send(
            "POST",
            &format!("{}/oauth2/token", self.api_url),
            None,
            &[],
            Body::Form(fields),
        )?;
        if response.status >= 400 {
            return Err(oauth_error(&response.body));
        }
        token_set(&response.body)
    }

    pub fn exchange_code(&self, code: &str, verifier: &str) -> Result<TokenSet, CalendarError> {
        let redirect = redirect_uri();
        self.token(&[
            ("code", code),
            ("grant_type", "authorization_code"),
            ("code_verifier", verifier),
            ("client_id", &self.app_key),
            ("redirect_uri", &redirect),
        ])
    }

    pub fn refresh(&self, refresh_token: &str) -> Result<TokenSet, CalendarError> {
        self.token(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &self.app_key),
        ])
    }

    /// Revoca l'accesso di questo token (e del collegamento) presso Dropbox.
    pub fn revoke(&self, access_token: &str) -> Result<(), CalendarError> {
        let response = Http::new(Duration::from_secs(30)).send(
            "POST",
            &format!("{}/2/auth/token/revoke", self.api_url),
            Some(access_token),
            &[],
            Body::None,
        )?;
        // 401: già revocato.
        if response.status >= 400 && response.status != 401 {
            return Err(CalendarError::OAuth(error_message(&response.body)));
        }
        Ok(())
    }

    pub fn current_account(&self, access_token: &str) -> Result<DropboxAccount, CalendarError> {
        let response = Http::new(Duration::from_secs(30)).send(
            "POST",
            &format!("{}/2/users/get_current_account", self.api_url),
            Some(access_token),
            &[],
            Body::Json(&Value::Null),
        )?;
        if response.status == 401 {
            return Err(CalendarError::Unauthorized);
        }
        if response.status >= 400 {
            return Err(CalendarError::OAuth(error_message(&response.body)));
        }
        let body = response.body;
        let account_id = body
            .get("account_id")
            .and_then(Value::as_str)
            .ok_or_else(|| CalendarError::InvalidResponse("account senza id".to_owned()))?;
        let label = body
            .get("email")
            .and_then(Value::as_str)
            .or_else(|| body.pointer("/name/display_name").and_then(Value::as_str))
            .unwrap_or(account_id);
        Ok(DropboxAccount {
            account_id: account_id.to_owned(),
            label: label.to_owned(),
        })
    }
}

/// File della libreria nella cartella dell'app su Dropbox.
pub struct DropboxStore {
    http: Http,
    token: String,
    api_url: String,
    content_url: String,
}

impl DropboxStore {
    pub fn new(access_token: &str) -> Self {
        Self {
            http: Http::new(TIMEOUT),
            token: access_token.to_owned(),
            api_url: API_URL.to_owned(),
            content_url: CONTENT_URL.to_owned(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_urls(mut self, api_url: &str, content_url: &str) -> Self {
        self.api_url = api_url.to_owned();
        self.content_url = content_url.to_owned();
        self
    }

    /// Percorso assoluto nella cartella dell'app. I percorsi arrivano solo
    /// dal modulo della libreria (id, numeri, hash): solo ASCII sicuro.
    fn full_path(relative: &str) -> Result<String, FolderError> {
        let safe = relative
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-' | b'.' | b'_'))
            && !relative
                .split('/')
                .any(|part| part.is_empty() || part == "..");
        if !safe && !relative.is_empty() {
            return Err(FolderError::Invalid(format!(
                "percorso non valido: {relative}"
            )));
        }
        Ok(if relative.is_empty() {
            format!("/{ROOT}")
        } else {
            format!("/{ROOT}/{relative}")
        })
    }

    /// Richiesta con nuovi tentativi per limiti di frequenza (429) ed errori
    /// temporanei del server (5xx).
    fn call(
        &self,
        url: &str,
        headers: &[(&str, &str)],
        body: Body<'_>,
    ) -> Result<RawResponse, FolderError> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let response = self
                .http
                .send_raw("POST", url, Some(&self.token), headers, body)
                .map_err(|error| FolderError::Remote(format!("Dropbox: {error}")))?;
            let retry = response.status == 429 || response.status >= 500;
            if !retry || attempt >= MAX_ATTEMPTS {
                return match response.status {
                    401 => Err(FolderError::Remote(
                        "Accesso a Dropbox non più valido: ricollega l'account".to_owned(),
                    )),
                    _ => Ok(response),
                };
            }
            let wait = response
                .retry_after
                .unwrap_or(u64::from(attempt) * 2)
                .min(MAX_WAIT_SECONDS);
            thread::sleep(Duration::from_secs(wait));
        }
    }

    fn rpc(&self, endpoint: &str, argument: &Value) -> Result<(u16, Value), FolderError> {
        let response = self.call(
            &format!("{}/2/{endpoint}", self.api_url),
            &[],
            Body::Json(argument),
        )?;
        let body = serde_json::from_slice(&response.bytes).unwrap_or(Value::Null);
        Ok((response.status, body))
    }

    fn failure(status: u16, body: &Value) -> FolderError {
        let summary = body
            .get("error_summary")
            .and_then(Value::as_str)
            .map(str::to_owned);
        FolderError::Remote(format!(
            "Dropbox ({status}): {}",
            summary.unwrap_or_else(|| error_message(body))
        ))
    }

    fn summary(body: &Value) -> &str {
        body.get("error_summary")
            .and_then(Value::as_str)
            .unwrap_or("")
    }
}

impl SyncStore for DropboxStore {
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, FolderError> {
        let argument = json!({ "path": Self::full_path(path)? }).to_string();
        let response = self.call(
            &format!("{}/2/files/download", self.content_url),
            &[("Dropbox-API-Arg", &argument)],
            Body::None,
        )?;
        match response.status {
            200 => Ok(Some(response.bytes)),
            409 => {
                let body: Value = serde_json::from_slice(&response.bytes).unwrap_or(Value::Null);
                if Self::summary(&body).starts_with("path/not_found") {
                    Ok(None)
                } else {
                    Err(Self::failure(409, &body))
                }
            }
            status => Err(Self::failure(
                status,
                &serde_json::from_slice(&response.bytes).unwrap_or(Value::Null),
            )),
        }
    }

    fn write(&self, path: &str, bytes: &[u8], create_only: bool) -> Result<(), FolderError> {
        let argument = json!({
            "path": Self::full_path(path)?,
            "mode": if create_only { "add" } else { "overwrite" },
            "autorename": false,
            "mute": true,
            "strict_conflict": create_only,
        })
        .to_string();
        let response = self.call(
            &format!("{}/2/files/upload", self.content_url),
            &[("Dropbox-API-Arg", &argument)],
            Body::Bytes(bytes),
        )?;
        let body: Value = serde_json::from_slice(&response.bytes).unwrap_or(Value::Null);
        match response.status {
            200 => Ok(()),
            409 if Self::summary(&body).contains("conflict") => Err(FolderError::Exists),
            status => Err(Self::failure(status, &body)),
        }
    }

    fn exists(&self, path: &str) -> Result<bool, FolderError> {
        let (status, body) = self.rpc(
            "files/get_metadata",
            &json!({ "path": Self::full_path(path)? }),
        )?;
        match status {
            200 => Ok(true),
            409 if Self::summary(&body).starts_with("path/not_found") => Ok(false),
            status => Err(Self::failure(status, &body)),
        }
    }

    fn list(&self, directory: &str) -> Result<Vec<(String, bool)>, FolderError> {
        let mut items = Vec::new();
        let (mut status, mut body) = self.rpc(
            "files/list_folder",
            &json!({ "path": Self::full_path(directory)?, "recursive": false, "limit": 2000 }),
        )?;
        for _ in 0..LIST_PAGES {
            match status {
                200 => {}
                409 if Self::summary(&body).starts_with("path/not_found") => return Ok(Vec::new()),
                status => return Err(Self::failure(status, &body)),
            }
            for entry in body
                .get("entries")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(name) = entry.get("name").and_then(Value::as_str) {
                    let is_dir = entry.get(".tag").and_then(Value::as_str) == Some("folder");
                    items.push((name.to_owned(), is_dir));
                }
            }
            let more = body
                .get("has_more")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let cursor = body
                .get("cursor")
                .and_then(Value::as_str)
                .map(str::to_owned);
            match (more, cursor) {
                (true, Some(cursor)) => {
                    (status, body) =
                        self.rpc("files/list_folder/continue", &json!({ "cursor": cursor }))?;
                }
                _ => break,
            }
        }
        Ok(items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::calendar_sync::http::fake::FakeServer;

    #[test]
    fn authorization_and_tokens() {
        let pkce = Pkce::new();
        let auth = DropboxAuth::new(" chiaveapp ");
        let url = auth.authorization_url("stato", &pkce);
        assert!(url.starts_with("https://www.dropbox.com/oauth2/authorize?client_id=chiaveapp&"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A41927%2F"));
        assert!(url.contains("token_access_type=offline"));
        assert!(url.contains("code_challenge_method=S256"));

        let server = FakeServer::start(vec![
            (200, json!({"access_token": "at", "refresh_token": "rt", "expires_in": 14400, "token_type": "bearer"}).to_string()),
            (400, json!({"error": "invalid_grant", "error_description": "refresh token is invalid"}).to_string()),
            (200, json!({"account_id": "dbid:abc", "email": "anna@example.com", "name": {"display_name": "Anna"}}).to_string()),
        ]);
        let auth = DropboxAuth::new("chiaveapp").with_api_url(&server.url);
        let tokens = auth
            .exchange_code("codice", &pkce.verifier)
            .expect("exchange");
        assert_eq!(tokens.refresh_token.as_deref(), Some("rt"));
        assert!(matches!(
            auth.refresh("vecchio"),
            Err(CalendarError::Unauthorized)
        ));
        let account = auth.current_account("at").expect("account");
        assert_eq!(
            account,
            DropboxAccount {
                account_id: "dbid:abc".to_owned(),
                label: "anna@example.com".to_owned()
            }
        );
        let requests = server.finish();
        assert!(requests[0].starts_with("POST /oauth2/token "));
        assert!(
            requests[0].contains("code_verifier=") && requests[0].contains("client_id=chiaveapp")
        );
        assert!(
            !requests[0].contains("client_secret"),
            "nessun segreto dell'app"
        );
        assert!(
            requests[2]
                .to_ascii_lowercase()
                .contains("authorization: bearer at")
        );
    }

    #[test]
    fn files_through_the_api() {
        let binary: Vec<u8> = (0..=255).collect();
        let server = FakeServer::start(vec![
            // read: non trovato, poi contenuto binario
            (409, json!({"error_summary": "path/not_found/..", "error": {}}).to_string()),
            (200, String::from_utf8_lossy(b"{\"format\":1}").into_owned()),
            // write: creato, poi già esistente
            (200, json!({"name": "000000000001.json"}).to_string()),
            (409, json!({"error_summary": "path/conflict/file/..", "error": {}}).to_string()),
            // exists
            (200, json!({".tag": "file"}).to_string()),
            (409, json!({"error_summary": "path/not_found/.."}).to_string()),
            // list con due pagine, poi cartella assente
            (200, json!({"entries": [{".tag": "folder", "name": "a"}], "has_more": true, "cursor": "c1"}).to_string()),
            (200, json!({"entries": [{".tag": "file", "name": "b.json"}], "has_more": false, "cursor": "c2"}).to_string()),
            (409, json!({"error_summary": "path/not_found/.."}).to_string()),
            // limite di frequenza, poi successo
            (429, json!({"error_summary": "too_many_requests/"}).to_string()),
            (200, json!({"name": "x"}).to_string()),
            (401, json!({"error_summary": "expired_access_token/"}).to_string()),
        ]);
        let store = DropboxStore::new("tok").with_urls(&server.url, &server.url);
        assert_eq!(store.read("format.json").expect("read"), None);
        assert_eq!(
            store.read("format.json").expect("read").as_deref(),
            Some(&b"{\"format\":1}"[..])
        );
        store
            .write("devices/d/changes/000000000001.json", &binary, true)
            .expect("write");
        assert!(matches!(
            store.write("devices/d/changes/000000000001.json", b"x", true),
            Err(FolderError::Exists)
        ));
        assert!(store.exists("blobs/ab/abc").expect("exists"));
        assert!(!store.exists("blobs/ab/abd").expect("exists"));
        assert_eq!(
            store.list("devices").expect("list"),
            vec![("a".to_owned(), true), ("b.json".to_owned(), false)]
        );
        assert!(store.list("assente").expect("list").is_empty());
        store
            .write("devices/d/device.json", b"{}", false)
            .expect("retry after 429");
        assert!(matches!(
            store.read("format.json"),
            Err(FolderError::Remote(_))
        ));
        assert!(store.read("../fuori").is_err());

        let requests = server.finish();
        assert!(requests[0].starts_with("POST /2/files/download "));
        assert!(
            requests[0].contains(r#"Dropbox-API-Arg: {"path":"/greatnote-sync/format.json"}"#)
                || requests[0]
                    .contains(r#"dropbox-api-arg: {"path":"/greatnote-sync/format.json"}"#)
        );
        assert!(requests[2].starts_with("POST /2/files/upload "));
        assert!(requests[2].contains(r#""mode":"add""#));
        assert!(
            requests[2]
                .to_ascii_lowercase()
                .contains("content-type: application/octet-stream")
        );
        assert!(requests[7].starts_with("POST /2/files/list_folder/continue "));
        assert!(requests[10].contains(r#""mode":"overwrite""#));
    }
}
