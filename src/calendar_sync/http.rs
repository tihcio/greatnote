//! Client HTTP comune per OAuth e API dei calendari: nessun
//! reindirizzamento, risposte limitate, errori senza token.

use std::time::Duration;

use serde_json::Value;
use ureq::Agent;

use super::CalendarError;

const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;
/// File grezzi (lotti e allegati della sincronizzazione).
const MAX_RAW_BYTES: u64 = 80 * 1024 * 1024;
const MAX_ERROR_MESSAGE: usize = 300;

pub(crate) struct Http {
    agent: Agent,
}

/// Risposta letta: stato, ETag e corpo JSON (`Null` se vuoto).
pub(crate) struct HttpResponse {
    pub status: u16,
    pub body: Value,
}

#[derive(Clone, Copy)]
pub(crate) enum Body<'a> {
    None,
    Json(&'a Value),
    Form(&'a [(&'a str, &'a str)]),
    /// Contenuto binario (`application/octet-stream`).
    Bytes(&'a [u8]),
}

/// Risposta grezza: stato, secondi di attesa richiesti (429) e byte.
pub(crate) struct RawResponse {
    pub status: u16,
    pub retry_after: Option<u64>,
    pub bytes: Vec<u8>,
}

impl Http {
    pub(crate) fn new(timeout: Duration) -> Self {
        let config = Agent::config_builder()
            .http_status_as_error(false)
            // Il token non deve finire su un altro host.
            .max_redirects(0)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(timeout))
            .timeout_recv_body(Some(timeout))
            .user_agent(concat!("Greatnote/", env!("CARGO_PKG_VERSION")));
        Self {
            agent: config.build().new_agent(),
        }
    }

    /// Richiesta con token opzionale e intestazioni aggiuntive; il corpo
    /// della risposta è interpretato come JSON (o testo).
    pub(crate) fn send(
        &self,
        method: &str,
        url: &str,
        bearer: Option<&str>,
        headers: &[(&str, &str)],
        body: Body<'_>,
    ) -> Result<HttpResponse, CalendarError> {
        let raw = self.send_limited(method, url, bearer, headers, body, MAX_RESPONSE_BYTES)?;
        let text = String::from_utf8_lossy(&raw.bytes);
        let body = if text.trim().is_empty() {
            Value::Null
        } else {
            serde_json::from_str(&text).unwrap_or_else(|_| Value::String(text.into_owned()))
        };
        Ok(HttpResponse {
            status: raw.status,
            body,
        })
    }

    /// Come [`Http::send`], ma restituisce i byte della risposta.
    pub(crate) fn send_raw(
        &self,
        method: &str,
        url: &str,
        bearer: Option<&str>,
        headers: &[(&str, &str)],
        body: Body<'_>,
    ) -> Result<RawResponse, CalendarError> {
        self.send_limited(method, url, bearer, headers, body, MAX_RAW_BYTES)
    }

    fn send_limited(
        &self,
        method: &str,
        url: &str,
        bearer: Option<&str>,
        headers: &[(&str, &str)],
        body: Body<'_>,
        limit: u64,
    ) -> Result<RawResponse, CalendarError> {
        let builder = ureq::http::Request::builder().method(method).uri(url);
        let mut builder = headers.iter().fold(builder, |builder, (name, value)| {
            builder.header(*name, *value)
        });
        if let Some(token) = bearer {
            builder = builder.header("Authorization", format!("Bearer {token}"));
        }
        let invalid = |error: ureq::http::Error| CalendarError::InvalidResponse(error.to_string());
        let request = match body {
            Body::None => builder.body(Vec::new()).map_err(invalid)?,
            Body::Json(value) => {
                let bytes = serde_json::to_vec(value)
                    .map_err(|error| CalendarError::InvalidResponse(error.to_string()))?;
                builder
                    .header("Content-Type", "application/json")
                    .body(bytes)
                    .map_err(invalid)?
            }
            Body::Form(fields) => {
                let encoded = fields
                    .iter()
                    .map(|(name, value)| format!("{}={}", encode(name), encode(value)))
                    .collect::<Vec<_>>()
                    .join("&");
                builder
                    .header("Content-Type", "application/x-www-form-urlencoded")
                    .body(encoded.into_bytes())
                    .map_err(invalid)?
            }
            Body::Bytes(bytes) => builder
                .header("Content-Type", "application/octet-stream")
                .body(bytes.to_vec())
                .map_err(invalid)?,
        };
        let mut response = self.agent.run(request).map_err(transport_error)?;
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse().ok());
        let bytes = response
            .body_mut()
            .with_config()
            .limit(limit)
            .read_to_vec()
            .map_err(transport_error)?;
        Ok(RawResponse {
            status,
            retry_after,
            bytes,
        })
    }
}

/// Errore tipizzato per una risposta non riuscita delle API.
pub(crate) fn api_error(response: &HttpResponse) -> CalendarError {
    let message = error_message(&response.body);
    match response.status {
        401 => CalendarError::Unauthorized,
        403 => CalendarError::Forbidden(message),
        404 | 410 => CalendarError::NotFound,
        409 | 412 => CalendarError::Conflict,
        429 => CalendarError::RateLimited,
        status => CalendarError::Http { status, message },
    }
}

/// Messaggio leggibile da `{"error": {"message": …}}`, `{"error":
/// "…", "error_description": "…"}` o testo, accorciato e senza caratteri
/// di controllo.
pub(crate) fn error_message(body: &Value) -> String {
    let message = body
        .get("error_description")
        .or_else(|| body.pointer("/error/message"))
        .or_else(|| body.get("error"))
        .or_else(|| body.get("message"))
        .and_then(Value::as_str)
        .or_else(|| body.as_str())
        .unwrap_or("nessun dettaglio");
    let cleaned: String = message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_ERROR_MESSAGE)
        .collect();
    cleaned.trim().to_owned()
}

fn transport_error(error: ureq::Error) -> CalendarError {
    match error {
        ureq::Error::Timeout(_) => CalendarError::Network("tempo scaduto".to_owned()),
        ureq::Error::HostNotFound => {
            CalendarError::Network("server non raggiungibile: sei connesso a internet?".to_owned())
        }
        ureq::Error::ConnectionFailed => {
            CalendarError::Network("connessione non riuscita".to_owned())
        }
        ureq::Error::BodyExceedsLimit(_) => {
            CalendarError::InvalidResponse("risposta troppo grande".to_owned())
        }
        other => {
            let text: String = other.to_string().chars().take(MAX_ERROR_MESSAGE).collect();
            CalendarError::Network(text)
        }
    }
}

/// Codifica percentuale (RFC 3986): restano solo i caratteri non riservati.
pub(crate) fn encode(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

/// Decodifica di un valore `application/x-www-form-urlencoded`.
pub(crate) fn decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hex = value.get(index + 1..index + 3)?;
                output.push(u8::from_str_radix(hex, 16).ok()?);
                index += 3;
            }
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(output).ok()
}

/// Query string da coppie chiave/valore.
pub(crate) fn query(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(name, value)| format!("{}={}", encode(name), encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Server HTTP finto per i test: risponde in ordine alle risposte date e
/// registra le richieste ricevute (riga iniziale e intestazioni).
#[cfg(test)]
pub(crate) mod fake {
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
        thread,
    };

    pub(crate) struct FakeServer {
        pub url: String,
        pub requests: Arc<Mutex<Vec<String>>>,
        handle: Option<thread::JoinHandle<()>>,
    }

    impl FakeServer {
        pub(crate) fn start(responses: Vec<(u16, String)>) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
            let url = format!("http://{}", listener.local_addr().expect("addr"));
            let requests = Arc::new(Mutex::new(Vec::new()));
            let log = Arc::clone(&requests);
            let base = url.clone();
            let handle = thread::spawn(move || {
                for (status, body) in responses {
                    let Ok((stream, _)) = listener.accept() else {
                        return;
                    };
                    let mut reader = BufReader::new(stream);
                    let mut head = String::new();
                    let mut length = 0;
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                            break;
                        }
                        if let Some(value) =
                            line.to_ascii_lowercase().strip_prefix("content-length:")
                        {
                            length = value.trim().parse().unwrap_or(0);
                        }
                        head.push_str(&line);
                    }
                    let mut body_in = vec![0; length];
                    let _ = reader.read_exact(&mut body_in);
                    head.push_str(&String::from_utf8_lossy(&body_in));
                    log.lock().expect("log").push(head);
                    let mut stream = reader.into_inner();
                    let body = body.replace("{BASE}", &base);
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                }
            });
            Self {
                url,
                requests,
                handle: Some(handle),
            }
        }

        pub(crate) fn finish(mut self) -> Vec<String> {
            if let Some(handle) = self.handle.take() {
                handle.join().expect("server");
            }
            self.requests.lock().expect("log").clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encoding_round_trip() {
        let value = "primary#it.italian@group v.calendar.google.com/ä";
        let encoded = encode(value);
        assert!(!encoded.contains(['#', '@', ' ', '/']));
        assert_eq!(decode(&encoded).as_deref(), Some(value));
        assert_eq!(decode("a+b%20c").as_deref(), Some("a b c"));
        assert_eq!(decode("%zz"), None);
        assert_eq!(query(&[("a", "1 2"), ("b", "x&y")]), "a=1%202&b=x%26y");
    }

    #[test]
    fn readable_errors() {
        assert_eq!(
            error_message(
                &json!({"error": "invalid_grant", "error_description": "Token\nscaduto"})
            ),
            "Token scaduto"
        );
        assert_eq!(
            error_message(&json!({"error": {"message": "Not Found"}})),
            "Not Found"
        );
        let response = HttpResponse {
            status: 412,
            body: Value::Null,
        };
        assert!(matches!(api_error(&response), CalendarError::Conflict));
    }
}
