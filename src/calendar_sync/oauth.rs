//! OAuth 2.0 per app desktop (RFC 8252): codice di autorizzazione con PKCE
//! (S256), browser di sistema e ritorno su un indirizzo di loopback.
//! Nessuna password passa da Greatnote; il token di aggiornamento viene
//! conservato dal chiamante nel portachiavi.

use std::time::Duration;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    CalendarError, Provider,
    http::{Body, Http, decode, error_message, query},
};

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";
/// Eventi in lettura e scrittura, elenco dei calendari in sola lettura.
const GOOGLE_SCOPES: &str = "https://www.googleapis.com/auth/calendar.events https://www.googleapis.com/auth/calendar.calendarlist.readonly";
const MICROSOFT_AUTH_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/authorize";
const MICROSOFT_TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
/// `offline_access` per il token di aggiornamento, `User.Read` per
/// l'indirizzo dell'account.
const MICROSOFT_SCOPES: &str = "offline_access User.Read Calendars.ReadWrite";
const TOKEN_TIMEOUT: Duration = Duration::from_secs(30);

/// Registrazione dell'app presso il fornitore, configurata dall'utente.
#[derive(Debug, Clone)]
pub struct OAuthClient {
    pub provider: Provider,
    pub client_id: String,
    /// Google lo fornisce anche per le app desktop, dove non è davvero
    /// segreto; resta comunque nel portachiavi.
    pub client_secret: Option<String>,
    auth_url: String,
    token_url: String,
    revoke_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenSet {
    pub access_token: String,
    /// Assente quando il fornitore non lo ruota durante un aggiornamento.
    pub refresh_token: Option<String>,
    pub expires_in: u64,
}

/// Verificatore e sfida PKCE.
#[derive(Debug, Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Self {
        // 3 UUID v4 = 48 byte da un generatore crittografico del sistema
        // (122 bit casuali ciascuno): 64 caratteri base64url.
        let bytes: Vec<u8> = (0..3)
            .flat_map(|_| *uuid::Uuid::new_v4().as_bytes())
            .collect();
        let verifier = URL_SAFE_NO_PAD.encode(bytes);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        Self {
            verifier,
            challenge,
        }
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

/// Valore casuale `state` contro le richieste forgiate (CSRF).
pub fn random_state() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Indirizzo di ritorno: Google accetta qualunque porta su 127.0.0.1;
/// Microsoft accetta `http://localhost` registrato senza porta, che viene
/// ignorata nel confronto.
pub fn redirect_uri(provider: Provider, port: u16) -> String {
    match provider {
        Provider::Google => format!("http://127.0.0.1:{port}"),
        Provider::Microsoft => format!("http://localhost:{port}"),
    }
}

impl OAuthClient {
    pub fn new(provider: Provider, client_id: &str, client_secret: Option<&str>) -> Self {
        let (auth_url, token_url, revoke_url) = match provider {
            Provider::Google => (GOOGLE_AUTH_URL, GOOGLE_TOKEN_URL, Some(GOOGLE_REVOKE_URL)),
            Provider::Microsoft => (MICROSOFT_AUTH_URL, MICROSOFT_TOKEN_URL, None),
        };
        Self {
            provider,
            client_id: client_id.trim().to_owned(),
            client_secret: client_secret
                .map(str::trim)
                .filter(|secret| !secret.is_empty())
                .map(str::to_owned),
            auth_url: auth_url.to_owned(),
            token_url: token_url.to_owned(),
            revoke_url: revoke_url.map(str::to_owned),
        }
    }

    /// Client con indirizzi diversi (server finti nei test).
    #[cfg(test)]
    pub(crate) fn with_urls(mut self, token_url: &str, revoke_url: Option<&str>) -> Self {
        self.token_url = token_url.to_owned();
        self.revoke_url = revoke_url.map(str::to_owned);
        self
    }

    /// Indirizzo da aprire nel browser.
    pub fn authorization_url(&self, redirect_uri: &str, state: &str, pkce: &Pkce) -> String {
        let mut pairs = vec![
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", redirect_uri),
            ("response_type", "code"),
            ("state", state),
            ("code_challenge", pkce.challenge.as_str()),
            ("code_challenge_method", "S256"),
        ];
        match self.provider {
            Provider::Google => pairs.extend([
                ("scope", GOOGLE_SCOPES),
                // Token di aggiornamento anche se l'app era già autorizzata.
                ("access_type", "offline"),
                ("prompt", "consent"),
            ]),
            Provider::Microsoft => pairs.extend([
                ("scope", MICROSOFT_SCOPES),
                ("response_mode", "query"),
                ("prompt", "select_account"),
            ]),
        }
        format!("{}?{}", self.auth_url, query(&pairs))
    }

    pub fn exchange_code(
        &self,
        code: &str,
        verifier: &str,
        redirect_uri: &str,
    ) -> Result<TokenSet, CalendarError> {
        let mut fields = vec![
            ("client_id", self.client_id.as_str()),
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect_uri),
        ];
        if let Some(secret) = &self.client_secret {
            fields.push(("client_secret", secret));
        }
        self.token_request(&fields)
    }

    pub fn refresh(&self, refresh_token: &str) -> Result<TokenSet, CalendarError> {
        let mut fields = vec![
            ("client_id", self.client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ];
        if self.provider == Provider::Microsoft {
            fields.push(("scope", MICROSOFT_SCOPES));
        }
        if let Some(secret) = &self.client_secret {
            fields.push(("client_secret", secret));
        }
        self.token_request(&fields)
    }

    /// Revoca presso il fornitore, se disponibile (Google). Microsoft non
    /// offre la revoca di un singolo token: l'accesso si toglie dal proprio
    /// account Microsoft.
    pub fn revoke(&self, token: &str) -> Result<bool, CalendarError> {
        let Some(url) = &self.revoke_url else {
            return Ok(false);
        };
        let response = Http::new(TOKEN_TIMEOUT).send(
            "POST",
            url,
            None,
            &[],
            Body::Form(&[("token", token)]),
        )?;
        // 400: token già scaduto o revocato.
        Ok(response.status < 300)
    }

    fn token_request(&self, fields: &[(&str, &str)]) -> Result<TokenSet, CalendarError> {
        let response = Http::new(TOKEN_TIMEOUT).send(
            "POST",
            &self.token_url,
            None,
            &[("Accept", "application/json")],
            Body::Form(fields),
        )?;
        if response.status >= 400 {
            let code = response.body.get("error").and_then(Value::as_str);
            return Err(if code == Some("invalid_grant") {
                CalendarError::Unauthorized
            } else {
                CalendarError::OAuth(error_message(&response.body))
            });
        }
        parse_token_response(&response.body)
    }
}

fn parse_token_response(body: &Value) -> Result<TokenSet, CalendarError> {
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
            .filter(|token| !token.is_empty())
            .map(str::to_owned),
        expires_in: body
            .get("expires_in")
            .and_then(|value| value.as_u64().or_else(|| value.as_str()?.parse().ok()))
            .unwrap_or(3600),
    })
}

/// Esito del ritorno dal browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callback {
    Code {
        code: String,
        state: String,
    },
    /// L'utente ha negato l'accesso o il fornitore ha segnalato un errore.
    Denied {
        error: String,
        state: Option<String>,
    },
    /// Richiesta estranea (es. `/favicon.ico`): da ignorare.
    Other,
}

/// Interpreta la prima riga della richiesta HTTP ricevuta sul loopback
/// (`GET /?code=…&state=… HTTP/1.1`).
pub fn parse_callback(request_line: &str) -> Callback {
    let mut parts = request_line.split_whitespace();
    let (Some("GET"), Some(target)) = (parts.next(), parts.next()) else {
        return Callback::Other;
    };
    let Some((path, query)) = target.split_once('?') else {
        return Callback::Other;
    };
    if path != "/" {
        return Callback::Other;
    }
    let mut code = None;
    let mut state = None;
    let mut error = None;
    let mut description = None;
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = decode(value);
        match name {
            "code" => code = value,
            "state" => state = value,
            "error" => error = value,
            "error_description" => description = value,
            _ => {}
        }
    }
    if let Some(error) = error {
        let text: String = description
            .unwrap_or(error)
            .chars()
            .filter(|c| !c.is_control())
            .take(300)
            .collect();
        return Callback::Denied { error: text, state };
    }
    match (code, state) {
        (Some(code), Some(state)) if !code.is_empty() => Callback::Code { code, state },
        _ => Callback::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        thread,
    };

    #[test]
    fn pkce_and_authorization_url() {
        let pkce = Pkce::new();
        assert_eq!(pkce.verifier.len(), 64);
        assert_ne!(Pkce::new().verifier, pkce.verifier);
        assert_eq!(
            pkce.challenge,
            URL_SAFE_NO_PAD.encode(Sha256::digest(pkce.verifier.as_bytes()))
        );
        let google = OAuthClient::new(Provider::Google, " id.apps ", Some("  "));
        assert!(google.client_secret.is_none());
        let url = google.authorization_url(&redirect_uri(Provider::Google, 4567), "abc", &pkce);
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?client_id=id.apps&"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A4567"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("access_type=offline"));
        assert!(url.contains(&format!("code_challenge={}", pkce.challenge)));
        let microsoft = OAuthClient::new(Provider::Microsoft, "guid", None);
        let url = microsoft.authorization_url(&redirect_uri(Provider::Microsoft, 80), "s", &pkce);
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A80"));
        assert!(url.contains("offline_access%20User.Read%20Calendars.ReadWrite"));
    }

    #[test]
    fn callback_parsing() {
        assert_eq!(
            parse_callback("GET /?state=xyz&code=4%2F0Ab-c&scope=a HTTP/1.1"),
            Callback::Code {
                code: "4/0Ab-c".to_owned(),
                state: "xyz".to_owned()
            }
        );
        assert_eq!(
            parse_callback("GET /?error=access_denied&state=xyz HTTP/1.1"),
            Callback::Denied {
                error: "access_denied".to_owned(),
                state: Some("xyz".to_owned())
            }
        );
        assert_eq!(parse_callback("GET /favicon.ico HTTP/1.1"), Callback::Other);
        assert_eq!(
            parse_callback("POST /?code=a&state=b HTTP/1.1"),
            Callback::Other
        );
        assert_eq!(
            parse_callback("GET /?code=&state=b HTTP/1.1"),
            Callback::Other
        );
    }

    /// Server finto che risponde una volta e restituisce il corpo ricevuto.
    fn fake_token_server(status: u16, body: Value) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}/token", listener.local_addr().expect("addr"));
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(stream);
            let mut length = 0;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("line");
                if line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    length = value.trim().parse().expect("length");
                }
            }
            let mut received = vec![0; length];
            reader.read_exact(&mut received).expect("body");
            let text = body.to_string();
            let mut stream = reader.into_inner();
            write!(
                stream,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{text}",
                text.len()
            )
            .expect("write");
            String::from_utf8(received).expect("utf8")
        });
        (url, handle)
    }

    #[test]
    fn code_exchange_and_refresh() {
        let (url, server) = fake_token_server(
            200,
            json!({"access_token": "at", "refresh_token": "rt", "expires_in": 3599, "token_type": "Bearer"}),
        );
        let client =
            OAuthClient::new(Provider::Google, "cid", Some("secret")).with_urls(&url, None);
        let tokens = client
            .exchange_code("code/1", "verifier", "http://127.0.0.1:9")
            .expect("tokens");
        assert_eq!(
            tokens,
            TokenSet {
                access_token: "at".to_owned(),
                refresh_token: Some("rt".to_owned()),
                expires_in: 3599
            }
        );
        let sent = server.join().expect("server");
        assert!(sent.contains("grant_type=authorization_code"));
        assert!(sent.contains("code=code%2F1"));
        assert!(sent.contains("code_verifier=verifier"));
        assert!(sent.contains("client_secret=secret"));

        let (url, server) = fake_token_server(
            400,
            json!({"error": "invalid_grant", "error_description": "revocato"}),
        );
        let client = OAuthClient::new(Provider::Microsoft, "cid", None).with_urls(&url, None);
        assert!(matches!(
            client.refresh("old"),
            Err(CalendarError::Unauthorized)
        ));
        let sent = server.join().expect("server");
        assert!(sent.contains("grant_type=refresh_token") && sent.contains("scope=offline_access"));
        assert!(!sent.contains("client_secret"));

        let (url, server) = fake_token_server(
            400,
            json!({"error": "invalid_client", "error_description": "client sconosciuto"}),
        );
        let client = OAuthClient::new(Provider::Google, "cid", None).with_urls(&url, None);
        match client.exchange_code("c", "v", "r") {
            Err(CalendarError::OAuth(message)) => assert_eq!(message, "client sconosciuto"),
            other => panic!("atteso errore OAuth: {other:?}"),
        }
        server.join().expect("server");
    }
}
