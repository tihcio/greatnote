//! Interfaccia dei provider AI e implementazione per le API compatibili
//! OpenAI (`/models`, `/chat/completions`, `/embeddings`).
//!
//! Le chiamate sono bloccanti: la shell desktop le esegue fuori dal thread
//! dell'interfaccia. L'annullamento è controllato tra un frammento e l'altro
//! della risposta in streaming e prima di ogni richiesta.

use std::{
    fmt,
    io::{BufRead, BufReader},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ureq::Agent;

use super::{AiError, settings::Endpoint};

/// Testo massimo accettato in una risposta di chat.
pub const MAX_OUTPUT_CHARS: usize = 200_000;
/// Dimensione massima di una risposta non in streaming.
const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;
/// Testi per singola richiesta di embedding.
pub const EMBEDDING_BATCH: usize = 16;
const MAX_DIMENSIONS: usize = 8192;
const MAX_ERROR_MESSAGE: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    System,
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
}

impl ChatMessage {
    pub fn new(role: ChatRole, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
        }
    }
}

/// Origine di una risposta, mostrata all'utente accanto al risultato.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Provenance {
    pub host: String,
    pub model: String,
    pub local: bool,
}

/// Contratto comune dei provider: elenco modelli, chat in streaming ed
/// embedding. I test usano server HTTP finti, mai servizi reali.
pub trait AiProvider: Send + Sync {
    fn list_models(&self) -> Result<Vec<String>, AiError>;

    /// Restituisce il testo completo; `on_delta` riceve i frammenti man mano.
    fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        temperature: f32,
        cancel: &AtomicBool,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String, AiError>;

    fn embed(
        &self,
        model: &str,
        inputs: &[String],
        cancel: &AtomicBool,
    ) -> Result<Vec<Vec<f32>>, AiError>;

    /// Server e modello effettivi, mostrati con i risultati.
    fn provenance(&self, model: &str) -> Provenance;

    /// Legge un'immagine con un modello con visione (testo nell'immagine).
    /// `instructions` sono le istruzioni di sistema, `request` la richiesta.
    fn read_image(
        &self,
        _model: &str,
        _instructions: &str,
        _request: &str,
        _image: &[u8],
        _mime: &str,
        _cancel: &AtomicBool,
    ) -> Result<String, AiError> {
        Err(AiError::NotConfigured(
            "questo provider non può leggere immagini",
        ))
    }
}

/// Chiave API in memoria: non compare in `Debug` né nei log.
#[derive(Clone)]
pub struct ApiKey(String);

impl ApiKey {
    pub fn new(value: String) -> Option<Self> {
        let value = value.trim().to_owned();
        (!value.is_empty()).then_some(Self(value))
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ApiKey(***)")
    }
}

pub struct OpenAiCompatible {
    endpoint: Endpoint,
    api_key: Option<ApiKey>,
    agent: Agent,
}

impl OpenAiCompatible {
    pub fn new(endpoint: Endpoint, api_key: Option<ApiKey>, timeout: Duration) -> Self {
        let mut config = Agent::config_builder()
            .http_status_as_error(false)
            // Nessun reindirizzamento: la chiave non deve finire altrove.
            .max_redirects(0)
            .timeout_connect(Some(Duration::from_secs(10)))
            .timeout_recv_response(Some(timeout))
            .timeout_recv_body(Some(timeout))
            .user_agent(concat!("Greatnote/", env!("CARGO_PKG_VERSION")));
        if endpoint.is_local() {
            // Un proxy di sistema non deve intercettare il server locale.
            config = config.proxy(None);
        }
        Self {
            endpoint,
            api_key,
            agent: config.build().new_agent(),
        }
    }

    fn post(&self, path: &str, body: &Value) -> Result<ureq::http::Response<ureq::Body>, AiError> {
        let mut request = self
            .agent
            .post(self.endpoint.url(path))
            .header("Content-Type", "application/json");
        if let Some(key) = &self.api_key {
            request = request.header("Authorization", format!("Bearer {}", key.0));
        }
        let bytes = serde_json::to_vec(body).map_err(|error| {
            AiError::InvalidResponse(format!("richiesta non serializzabile: {error}"))
        })?;
        let response = request.send(&bytes[..]).map_err(transport_error)?;
        check_status(response)
    }

    fn get(&self, path: &str) -> Result<ureq::http::Response<ureq::Body>, AiError> {
        let mut request = self.agent.get(self.endpoint.url(path));
        if let Some(key) = &self.api_key {
            request = request.header("Authorization", format!("Bearer {}", key.0));
        }
        let response = request.call().map_err(transport_error)?;
        check_status(response)
    }
}

fn transport_error(error: ureq::Error) -> AiError {
    match error {
        ureq::Error::Timeout(_) => AiError::Timeout,
        ureq::Error::HostNotFound => AiError::Connection("server non trovato".to_owned()),
        ureq::Error::ConnectionFailed => AiError::Connection(
            "connessione rifiutata: il server è avviato all'indirizzo indicato?".to_owned(),
        ),
        ureq::Error::Io(error) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
            AiError::Connection(
                "connessione rifiutata: il server è avviato all'indirizzo indicato?".to_owned(),
            )
        }
        ureq::Error::Io(error) if error.kind() == std::io::ErrorKind::TimedOut => AiError::Timeout,
        ureq::Error::BodyExceedsLimit(_) => {
            AiError::InvalidResponse("risposta troppo grande".to_owned())
        }
        other => AiError::Connection(truncate(&other.to_string(), MAX_ERROR_MESSAGE)),
    }
}

fn check_status(
    mut response: ureq::http::Response<ureq::Body>,
) -> Result<ureq::http::Response<ureq::Body>, AiError> {
    let status = response.status().as_u16();
    if status < 400 {
        return Ok(response);
    }
    if status == 401 || status == 403 {
        return Err(AiError::Unauthorized(status));
    }
    if status == 429 {
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok());
        return Err(AiError::RateLimited(retry_after));
    }
    let body = response
        .body_mut()
        .with_config()
        .limit(64 * 1024)
        .lossy_utf8(true)
        .read_to_string()
        .unwrap_or_default();
    Err(AiError::Http {
        status,
        message: error_message(&body),
    })
}

/// Messaggio leggibile dal corpo di un errore (`{"error": {"message": …}}`,
/// `{"error": "…"}` o testo semplice), accorciato e senza caratteri di
/// controllo.
fn error_message(body: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let message = parsed
        .as_ref()
        .and_then(|value| {
            value
                .pointer("/error/message")
                .or_else(|| value.get("error"))
                .or_else(|| value.get("message"))
        })
        .and_then(Value::as_str)
        .map(str::to_owned)
        .unwrap_or_else(|| body.to_owned());
    let message = message.trim();
    if message.is_empty() {
        "nessun dettaglio".to_owned()
    } else {
        truncate(message, MAX_ERROR_MESSAGE)
    }
}

fn truncate(text: &str, max: usize) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if cleaned.chars().count() <= max {
        cleaned
    } else {
        let mut shortened: String = cleaned.chars().take(max).collect();
        shortened.push('…');
        shortened
    }
}

fn read_json(mut response: ureq::http::Response<ureq::Body>) -> Result<Value, AiError> {
    let text = response
        .body_mut()
        .with_config()
        .limit(MAX_RESPONSE_BYTES)
        .read_to_string()
        .map_err(transport_error)?;
    serde_json::from_str(&text).map_err(|_| {
        let trimmed = text.trim_start().to_ascii_lowercase();
        if trimmed.starts_with("<!doctype") || trimmed.starts_with("<html") {
            // Tipico di un indirizzo che punta all'interfaccia web invece che
            // all'API (es. Open WebUI senza `/api`).
            AiError::InvalidResponse(
                "il server ha risposto con una pagina web invece che con l'API: controlla l'indirizzo (per Open WebUI usa http://localhost:3000/api)"
                    .to_owned(),
            )
        } else {
            AiError::InvalidResponse("il server non ha restituito JSON".to_owned())
        }
    })
}

impl AiProvider for OpenAiCompatible {
    fn read_image(
        &self,
        model: &str,
        instructions: &str,
        request: &str,
        image: &[u8],
        mime: &str,
        cancel: &AtomicBool,
    ) -> Result<String, AiError> {
        use base64::Engine as _;
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Cancelled);
        }
        let data = base64::engine::general_purpose::STANDARD.encode(image);
        let body = json!({
            "model": model,
            "temperature": 0,
            "stream": false,
            "messages": [
                { "role": "system", "content": instructions },
                { "role": "user", "content": [
                    { "type": "text", "text": request },
                    { "type": "image_url", "image_url": { "url": format!("data:{mime};base64,{data}") } }
                ] }
            ],
        });
        let value = read_json(self.post("chat/completions", &body)?)?;
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Cancelled);
        }
        let content = value
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .ok_or_else(|| AiError::InvalidResponse("risposta senza testo".to_owned()))?;
        let mut output = OutputFilter::default();
        output.push(content, &mut |_| {})?;
        output.finish()
    }

    fn provenance(&self, model: &str) -> Provenance {
        Provenance {
            host: self.endpoint.host.clone(),
            model: model.to_owned(),
            local: self.endpoint.is_local(),
        }
    }

    fn list_models(&self) -> Result<Vec<String>, AiError> {
        let value = read_json(self.get("models")?)?;
        let data = value
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| AiError::InvalidResponse("elenco dei modelli assente".to_owned()))?;
        let mut models: Vec<String> = data
            .iter()
            .filter_map(|model| model.get("id").and_then(Value::as_str))
            .map(|id| truncate(id, 200))
            .collect();
        models.sort();
        models.dedup();
        Ok(models)
    }

    fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        temperature: f32,
        cancel: &AtomicBool,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String, AiError> {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Cancelled);
        }
        let body = json!({
            "model": model,
            "messages": messages,
            "temperature": temperature,
            "stream": true,
        });
        let mut response = self.post("chat/completions", &body)?;
        let mut output = OutputFilter::default();
        let event_stream = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/event-stream"));
        if !event_stream {
            // Alcuni server ignorano `stream` e rispondono in un colpo solo.
            let value = read_json(response)?;
            let content = value
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .ok_or_else(|| AiError::InvalidResponse("risposta senza testo".to_owned()))?;
            output.push(content, on_delta)?;
            return output.finish();
        }
        let reader = BufReader::new(response.body_mut().as_reader());
        for line in reader.lines() {
            if cancel.load(Ordering::Relaxed) {
                return Err(AiError::Cancelled);
            }
            let line = line.map_err(|error| match error.kind() {
                std::io::ErrorKind::TimedOut => AiError::Timeout,
                _ => AiError::Connection(truncate(&error.to_string(), MAX_ERROR_MESSAGE)),
            })?;
            let Some(data) = line.strip_prefix("data:") else {
                continue;
            };
            let data = data.trim();
            if data == "[DONE]" {
                break;
            }
            let event: Value = serde_json::from_str(data).map_err(|_| {
                AiError::InvalidResponse("frammento della risposta non leggibile".to_owned())
            })?;
            if event.get("error").is_some() {
                return Err(AiError::Http {
                    status: 200,
                    message: error_message(data),
                });
            }
            if let Some(delta) = event
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
            {
                output.push(delta, on_delta)?;
            }
        }
        output.finish()
    }

    fn embed(
        &self,
        model: &str,
        inputs: &[String],
        cancel: &AtomicBool,
    ) -> Result<Vec<Vec<f32>>, AiError> {
        let mut vectors = Vec::with_capacity(inputs.len());
        for batch in inputs.chunks(EMBEDDING_BATCH) {
            if cancel.load(Ordering::Relaxed) {
                return Err(AiError::Cancelled);
            }
            let value =
                read_json(self.post("embeddings", &json!({ "model": model, "input": batch }))?)?;
            vectors.extend(parse_embeddings(&value, batch.len())?);
        }
        Ok(vectors)
    }
}

fn parse_embeddings(value: &Value, expected: usize) -> Result<Vec<Vec<f32>>, AiError> {
    let invalid = |reason: &str| AiError::InvalidResponse(format!("embedding: {reason}"));
    let data = value
        .get("data")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("elenco assente"))?;
    if data.len() != expected {
        return Err(invalid("numero di vettori diverso dai testi inviati"));
    }
    let mut indexed: Vec<(usize, Vec<f32>)> = Vec::with_capacity(expected);
    for (position, item) in data.iter().enumerate() {
        let index = item
            .get("index")
            .and_then(Value::as_u64)
            .map_or(position, |index| index as usize);
        let numbers = item
            .get("embedding")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("vettore assente"))?;
        if numbers.is_empty() || numbers.len() > MAX_DIMENSIONS {
            return Err(invalid("dimensione del vettore non supportata"));
        }
        let vector = numbers
            .iter()
            .map(|number| {
                number
                    .as_f64()
                    .map(|number| number as f32)
                    .filter(|number| number.is_finite())
            })
            .collect::<Option<Vec<f32>>>()
            .ok_or_else(|| invalid("valori non numerici"))?;
        indexed.push((index, vector));
    }
    indexed.sort_by_key(|(index, _)| *index);
    let dimensions = indexed[0].1.len();
    if indexed
        .iter()
        .enumerate()
        .any(|(position, (index, vector))| *index != position || vector.len() != dimensions)
    {
        return Err(invalid("vettori incoerenti"));
    }
    Ok(indexed.into_iter().map(|(_, vector)| vector).collect())
}

/// Filtra il testo in uscita: rimuove il ragionamento `<think>…</think>` che
/// alcuni modelli locali premettono alla risposta e limita la lunghezza.
#[derive(Default)]
struct OutputFilter {
    text: String,
    pending: String,
    state: ThinkState,
}

#[derive(Default, PartialEq)]
enum ThinkState {
    #[default]
    Start,
    Thinking,
    Answer,
}

const THINK_OPEN: &str = "<think>";
const THINK_CLOSE: &str = "</think>";

impl OutputFilter {
    fn push(&mut self, delta: &str, on_delta: &mut dyn FnMut(&str)) -> Result<(), AiError> {
        self.pending.push_str(delta);
        loop {
            match self.state {
                ThinkState::Start => {
                    let trimmed = self.pending.trim_start();
                    if let Some(rest) = trimmed.strip_prefix(THINK_OPEN) {
                        self.pending = rest.to_owned();
                        self.state = ThinkState::Thinking;
                    } else if THINK_OPEN.starts_with(trimmed) {
                        // Potrebbe essere l'inizio di `<think>`: si attende.
                        return Ok(());
                    } else {
                        self.state = ThinkState::Answer;
                    }
                }
                ThinkState::Thinking => {
                    if let Some(end) = self.pending.find(THINK_CLOSE) {
                        self.pending = self.pending[end + THINK_CLOSE.len()..]
                            .trim_start()
                            .to_owned();
                        self.state = ThinkState::Answer;
                    } else {
                        // Conserva solo una coda utile a trovare la chiusura.
                        let keep = self.pending.len().saturating_sub(THINK_CLOSE.len());
                        let boundary = (keep..=self.pending.len())
                            .find(|index| self.pending.is_char_boundary(*index))
                            .unwrap_or(self.pending.len());
                        self.pending.drain(..boundary);
                        return Ok(());
                    }
                }
                ThinkState::Answer => {
                    if self.pending.is_empty() {
                        return Ok(());
                    }
                    let chunk = std::mem::take(&mut self.pending);
                    if self.text.chars().count() + chunk.chars().count() > MAX_OUTPUT_CHARS {
                        return Err(AiError::InvalidResponse("risposta troppo lunga".to_owned()));
                    }
                    self.text.push_str(&chunk);
                    on_delta(&chunk);
                    return Ok(());
                }
            }
        }
    }

    fn finish(mut self) -> Result<String, AiError> {
        if self.state == ThinkState::Start && !self.pending.is_empty() {
            self.text.push_str(&std::mem::take(&mut self.pending));
        }
        Ok(self.text.trim().to_owned())
    }
}

/// Server HTTP finto per i test: risponde alle richieste in ordine con le
/// risposte preparate e registra ciò che riceve.
#[cfg(test)]
pub(crate) mod fake_server {
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        sync::{Arc, Mutex},
        thread,
    };

    #[derive(Debug, Clone)]
    pub struct Recorded {
        pub request_line: String,
        pub headers: Vec<String>,
        pub body: String,
    }

    pub struct FakeServer {
        pub base_url: String,
        pub requests: Arc<Mutex<Vec<Recorded>>>,
    }

    pub fn respond_with(responses: Vec<(u16, &'static str, String)>) -> FakeServer {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let address = listener.local_addr().expect("address");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        thread::spawn(move || {
            for (status, content_type, body) in responses {
                let Ok((stream, _)) = listener.accept() else {
                    return;
                };
                let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                let mut request_line = String::new();
                reader.read_line(&mut request_line).expect("request line");
                let mut headers = Vec::new();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).expect("header");
                    let line = line.trim_end().to_owned();
                    if line.is_empty() {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().expect("length");
                    }
                    headers.push(line);
                }
                let mut body_bytes = vec![0; length];
                reader.read_exact(&mut body_bytes).expect("body");
                recorded.lock().expect("lock").push(Recorded {
                    request_line: request_line.trim_end().to_owned(),
                    headers,
                    body: String::from_utf8_lossy(&body_bytes).into_owned(),
                });
                let mut stream = stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nRetry-After: 7\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        FakeServer {
            base_url: format!("http://{address}/v1"),
            requests,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake_server::respond_with;
    use super::*;

    fn provider(base_url: &str, key: Option<&str>) -> OpenAiCompatible {
        OpenAiCompatible::new(
            Endpoint::parse(base_url).expect("endpoint"),
            key.and_then(|key| ApiKey::new(key.to_owned())),
            Duration::from_secs(5),
        )
    }

    #[test]
    fn lists_models_and_sends_the_key_only_as_header() {
        let server = respond_with(vec![(
            200,
            "application/json",
            r#"{"data":[{"id":"b"},{"id":"a"},{"id":"a"}]}"#.to_owned(),
        )]);
        let provider = provider(&server.base_url, Some("segreto-di-test"));
        assert_eq!(provider.list_models().expect("models"), vec!["a", "b"]);
        let requests = server.requests.lock().expect("lock");
        assert_eq!(requests[0].request_line, "GET /v1/models HTTP/1.1");
        assert!(
            requests[0]
                .headers
                .iter()
                .any(|header| header == "authorization: Bearer segreto-di-test"
                    || header == "Authorization: Bearer segreto-di-test")
        );
        assert!(!format!("{:?}", ApiKey::new("x".to_owned())).contains('x'));
    }

    #[test]
    fn streams_chat_and_strips_reasoning() {
        let stream = [
            r#"data: {"choices":[{"delta":{"content":"<thi"}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"nk>ragiono</think>\n"}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"Ciao "}}]}"#,
            r#"data: {"choices":[{"delta":{"content":"[1]"}}]}"#,
            "data: [DONE]",
        ]
        .join("\n\n");
        let server = respond_with(vec![(200, "text/event-stream", stream)]);
        let provider = provider(&server.base_url, None);
        let mut deltas = Vec::new();
        let text = provider
            .chat(
                "modello",
                &[ChatMessage::new(ChatRole::User, "ciao")],
                0.2,
                &AtomicBool::new(false),
                &mut |delta| deltas.push(delta.to_owned()),
            )
            .expect("chat");
        assert_eq!(text, "Ciao [1]");
        assert_eq!(deltas.concat(), "Ciao [1]");
        let body: Value =
            serde_json::from_str(&server.requests.lock().expect("lock")[0].body).expect("json");
        assert_eq!(body["model"], "modello");
        assert_eq!(body["stream"], true);
        assert_eq!(body["messages"][0]["role"], "user");
    }

    #[test]
    fn accepts_non_streaming_answers_and_reports_errors() {
        let server = respond_with(vec![
            (
                200,
                "application/json",
                r#"{"choices":[{"message":{"content":" Risposta "}}]}"#.to_owned(),
            ),
            (
                404,
                "application/json",
                r#"{"error":{"message":"model \"x\" not found"}}"#.to_owned(),
            ),
            (401, "application/json", "{}".to_owned()),
            (429, "application/json", "{}".to_owned()),
        ]);
        let provider = provider(&server.base_url, None);
        let cancel = AtomicBool::new(false);
        let chat = |provider: &OpenAiCompatible| provider.chat("x", &[], 0.0, &cancel, &mut |_| {});
        assert_eq!(chat(&provider).expect("answer"), "Risposta");
        assert!(matches!(
            chat(&provider),
            Err(AiError::Http { status: 404, message }) if message.contains("not found")
        ));
        assert!(matches!(chat(&provider), Err(AiError::Unauthorized(401))));
        assert!(matches!(
            chat(&provider),
            Err(AiError::RateLimited(Some(7)))
        ));
    }

    #[test]
    fn web_pages_instead_of_the_api_are_explained() {
        let server = respond_with(vec![(
            200,
            "text/html",
            "<!doctype html><html><body>App</body></html>".to_owned(),
        )]);
        let provider = provider(&server.base_url, None);
        assert!(matches!(
            provider.list_models(),
            Err(AiError::InvalidResponse(message)) if message.contains("pagina web")
        ));
    }

    #[test]
    fn images_are_sent_as_data_urls() {
        let server = respond_with(vec![(
            200,
            "application/json",
            r#"{"choices":[{"message":{"content":"Totale 37,50"}}]}"#.to_owned(),
        )]);
        let provider = provider(&server.base_url, None);
        let text = provider
            .read_image(
                "visione",
                "istruzioni",
                "trascrivi",
                b"\x89PNG",
                "image/png",
                &AtomicBool::new(false),
            )
            .expect("read");
        assert_eq!(text, "Totale 37,50");
        let body: Value =
            serde_json::from_str(&server.requests.lock().expect("lock")[0].body).expect("json");
        assert_eq!(body["model"], "visione");
        assert_eq!(body["messages"][0]["content"], "istruzioni");
        let url = body["messages"][1]["content"][1]["image_url"]["url"]
            .as_str()
            .expect("url");
        assert!(url.starts_with("data:image/png;base64,"));
    }

    #[test]
    fn embeddings_are_ordered_and_validated() {
        let server = respond_with(vec![
            (
                200,
                "application/json",
                r#"{"data":[{"index":1,"embedding":[0.0,1.0]},{"index":0,"embedding":[1.0,0.0]}]}"#
                    .to_owned(),
            ),
            (
                200,
                "application/json",
                r#"{"data":[{"index":0,"embedding":[1.0]}]}"#.to_owned(),
            ),
        ]);
        let provider = provider(&server.base_url, None);
        let cancel = AtomicBool::new(false);
        let vectors = provider
            .embed("e", &["a".to_owned(), "b".to_owned()], &cancel)
            .expect("vectors");
        assert_eq!(vectors, vec![vec![1.0, 0.0], vec![0.0, 1.0]]);
        assert!(matches!(
            provider.embed("e", &["a".to_owned(), "b".to_owned()], &cancel),
            Err(AiError::InvalidResponse(_))
        ));
    }

    #[test]
    fn cancelled_and_unreachable_requests_fail_cleanly() {
        let provider = provider("http://127.0.0.1:9/v1", None);
        assert!(matches!(
            provider.chat("x", &[], 0.0, &AtomicBool::new(true), &mut |_| {}),
            Err(AiError::Cancelled)
        ));
        assert!(matches!(
            provider.list_models(),
            Err(AiError::Connection(_) | AiError::Timeout)
        ));
    }

    #[test]
    fn output_filter_keeps_plain_answers() {
        let mut filter = OutputFilter::default();
        let mut seen = String::new();
        filter
            .push("<", &mut |delta| seen.push_str(delta))
            .expect("push");
        filter
            .push("b>grassetto", &mut |delta| seen.push_str(delta))
            .expect("push");
        assert_eq!(filter.finish().expect("finish"), "<b>grassetto");
        assert_eq!(seen, "<b>grassetto");
    }
}
