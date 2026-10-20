//! Provider basato sulla CLI di Claude Code installata sul computer
//! (`claude -p`), che usa l'account con cui l'utente ha già effettuato
//! l'accesso. I dati vanno ad Anthropic: per Greatnote è un server remoto e
//! richiede il consenso come gli altri.
//!
//! Claude Code è un agente con strumenti (file, comandi, web): qui viene
//! avviato senza alcuno strumento (`--tools ""`), senza server MCP, in
//! modalità sicura (niente hook, plugin, CLAUDE.md), senza salvare la
//! sessione e in una cartella temporanea vuota. Il testo delle note passa
//! dallo standard input, non dalla riga di comando. Non fornisce embedding.

use std::{
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;
use serde_json::Value;

use super::{
    AiError,
    provider::{AiProvider, ChatMessage, ChatRole, MAX_OUTPUT_CHARS, Provenance},
    settings::CLAUDE_CODE_HOST,
};

/// Alias accettati da `--model`; "default" usa il modello dell'account.
pub const MODEL_ALIASES: [&str; 5] = ["default", "sonnet", "opus", "haiku", "fable"];
const MAX_STDERR_BYTES: usize = 4096;
const POLL: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClaudeCodeStatus {
    pub executable: String,
    pub version: String,
    pub logged_in: bool,
    /// Metodo di accesso (es. account claude.ai o chiave API), senza dati
    /// personali.
    pub auth_method: Option<String>,
}

pub struct ClaudeCode {
    executable: PathBuf,
    timeout: Duration,
}

/// Cerca l'eseguibile: percorso configurato, poi `PATH`, poi le cartelle
/// d'installazione abituali (le app avviate dal desktop possono avere un
/// `PATH` ridotto).
pub fn locate(configured: &str) -> Result<PathBuf, AiError> {
    let not_found = || {
        AiError::ClaudeCode(
            "eseguibile `claude` non trovato: installa Claude Code o indica il percorso nelle impostazioni"
                .to_owned(),
        )
    };
    if !configured.trim().is_empty() {
        let path = PathBuf::from(configured.trim());
        return path.is_file().then_some(path).ok_or_else(not_found);
    }
    let names: &[&str] = if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &["claude"]
    };
    let mut candidates: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect())
        .unwrap_or_default();
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        candidates.push(home.join(".local/bin"));
        candidates.push(home.join(".claude/local"));
        candidates.push(home.join(".npm-global/bin"));
    }
    candidates.push(PathBuf::from("/usr/local/bin"));
    candidates.push(PathBuf::from("/opt/homebrew/bin"));
    candidates
        .iter()
        .flat_map(|directory| names.iter().map(move |name| directory.join(name)))
        .find(|path| path.is_file())
        .ok_or_else(not_found)
}

/// Cartella di lavoro vuota e privata, rimossa alla fine.
struct Workspace(PathBuf);

impl Workspace {
    fn create() -> Result<Self, AiError> {
        let path = std::env::temp_dir().join(format!("greatnote-claude-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).map_err(|error| AiError::ClaudeCode(error.to_string()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700));
        }
        Ok(Self(path))
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Contenuto inviato sullo standard input.
enum Input {
    Text(String),
    /// Un messaggio utente in formato `stream-json` (testo e immagini).
    Message(Value),
}

/// Istruzioni di sistema e testo da inviare: la cronologia della chat
/// diventa una trascrizione prima dell'ultimo messaggio.
fn compose(messages: &[ChatMessage]) -> (String, String) {
    let system = messages
        .iter()
        .filter(|message| message.role == ChatRole::System)
        .map(|message| message.content.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    let turns: Vec<&ChatMessage> = messages
        .iter()
        .filter(|message| message.role != ChatRole::System)
        .collect();
    let Some((last, previous)) = turns.split_last() else {
        return (system, String::new());
    };
    if previous.is_empty() {
        return (system, last.content.clone());
    }
    let mut prompt = String::from("Conversazione precedente:\n\n");
    for message in previous {
        let speaker = if message.role == ChatRole::User {
            "Utente"
        } else {
            "Assistente"
        };
        prompt.push_str(&format!("{speaker}: {}\n\n", message.content));
    }
    prompt.push_str("---\n\n");
    prompt.push_str(&last.content);
    (system, prompt)
}

fn short(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.chars().count() > 300 {
        trimmed.chars().take(299).collect::<String>() + "…"
    } else {
        trimmed.to_owned()
    }
}

/// Messaggio comprensibile per gli errori più comuni.
fn explain(message: &str) -> AiError {
    let lower = message.to_lowercase();
    if lower.contains("login") || lower.contains("not logged") || lower.contains("api key") {
        AiError::ClaudeCode(format!(
            "accesso non effettuato: apri un terminale, esegui `claude` e accedi ({})",
            short(message)
        ))
    } else if lower.contains("rate limit")
        || lower.contains("usage limit")
        || lower.contains("limit reached")
    {
        AiError::RateLimited(None)
    } else {
        AiError::ClaudeCode(short(message))
    }
}

impl ClaudeCode {
    pub fn new(executable: PathBuf, timeout: Duration) -> Self {
        Self {
            executable,
            timeout,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.executable);
        command.stdin(Stdio::null());
        command
    }

    fn output(&self, args: &[&str]) -> Result<String, AiError> {
        let mut command = self.command();
        command
            .args(args)
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .stdin(Stdio::null());
        let output = spawn_with_retry(&mut command)
            .and_then(std::process::Child::wait_with_output)
            .map_err(|error| AiError::ClaudeCode(format!("avvio non riuscito: {error}")))?;
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        if !output.status.success() && stdout.trim().is_empty() {
            return Err(explain(&String::from_utf8_lossy(&output.stderr)));
        }
        Ok(stdout)
    }

    /// Versione e stato dell'accesso, senza fare richieste al modello.
    pub fn status(&self) -> Result<ClaudeCodeStatus, AiError> {
        let version = self.output(&["--version"])?.trim().to_owned();
        let auth: Value =
            serde_json::from_str(&self.output(&["auth", "status"])?).unwrap_or(Value::Null);
        Ok(ClaudeCodeStatus {
            executable: self.executable.display().to_string(),
            version: short(&version),
            logged_in: auth
                .get("loggedIn")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            auth_method: auth.get("authMethod").and_then(Value::as_str).map(short),
        })
    }

    fn run(
        &self,
        model: &str,
        messages: &[ChatMessage],
        cancel: &AtomicBool,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String, AiError> {
        let (system, prompt) = compose(messages);
        self.execute(model, &system, Input::Text(prompt), cancel, on_delta)
    }

    fn execute(
        &self,
        model: &str,
        system: &str,
        input: Input,
        cancel: &AtomicBool,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String, AiError> {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Cancelled);
        }
        let (prompt, json_input) = match input {
            Input::Text(text) => (text, false),
            Input::Message(message) => (message.to_string() + "\n", true),
        };
        let workspace = Workspace::create()?;
        let mut command = Command::new(&self.executable);
        command
            .args([
                "-p",
                "--output-format",
                "stream-json",
                "--include-partial-messages",
                "--verbose",
                "--tools",
                "",
                "--safe-mode",
                "--strict-mcp-config",
                "--no-session-persistence",
                "--disable-slash-commands",
                "--permission-prompts",
                "none",
                "--system-prompt",
                system,
            ])
            .current_dir(&workspace.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if !model.is_empty() && model != "default" {
            command.args(["--model", model]);
        }
        if json_input {
            // Messaggio strutturato (per esempio con un'immagine).
            command.args(["--input-format", "stream-json"]);
        }
        let mut child = spawn_with_retry(&mut command)
            .map_err(|error| AiError::ClaudeCode(format!("avvio non riuscito: {error}")))?;
        let mut stdin = child.stdin.take();
        let writer = thread::spawn(move || {
            if let Some(stdin) = stdin.as_mut() {
                let _ = stdin.write_all(prompt.as_bytes());
            }
        });
        let (sender, receiver) = mpsc::channel::<String>();
        let stdout = child.stdout.take();
        let reader = thread::spawn(move || {
            if let Some(stdout) = stdout {
                for line in BufReader::new(stdout).lines() {
                    let Ok(line) = line else { break };
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            }
        });
        let stderr = child.stderr.take();
        let errors = thread::spawn(move || {
            let mut collected = Vec::new();
            if let Some(stderr) = stderr {
                let _ = stderr
                    .take(MAX_STDERR_BYTES as u64)
                    .read_to_end(&mut collected);
            }
            String::from_utf8_lossy(&collected).into_owned()
        });

        let started = Instant::now();
        let mut text = String::new();
        let mut final_text: Option<String> = None;
        let mut failure: Option<AiError> = None;
        loop {
            match receiver.recv_timeout(POLL) {
                Ok(line) => {
                    let Ok(event) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    match event.get("type").and_then(Value::as_str) {
                        Some("stream_event") => {
                            let delta = event
                                .pointer("/event/delta")
                                .filter(|delta| {
                                    delta.get("type").and_then(Value::as_str) == Some("text_delta")
                                })
                                .and_then(|delta| delta.get("text"))
                                .and_then(Value::as_str);
                            if let Some(delta) = delta {
                                if text.chars().count() + delta.chars().count() > MAX_OUTPUT_CHARS {
                                    failure = Some(AiError::InvalidResponse(
                                        "risposta troppo lunga".to_owned(),
                                    ));
                                    break;
                                }
                                text.push_str(delta);
                                on_delta(delta);
                            }
                        }
                        Some("result") => {
                            let result = event
                                .get("result")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned();
                            let is_error = event.get("is_error").and_then(Value::as_bool)
                                == Some(true)
                                || event
                                    .get("subtype")
                                    .and_then(Value::as_str)
                                    .is_some_and(|subtype| subtype != "success");
                            if is_error {
                                failure = Some(explain(if result.is_empty() {
                                    "richiesta non riuscita"
                                } else {
                                    &result
                                }));
                            } else {
                                final_text = Some(result);
                            }
                        }
                        _ => {}
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if cancel.load(Ordering::Relaxed) {
                        failure = Some(AiError::Cancelled);
                        break;
                    }
                    if started.elapsed() > self.timeout {
                        failure = Some(AiError::Timeout);
                        break;
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        if let Some(error) = failure {
            // I thread di lettura terminano da soli quando le pipe si chiudono:
            // non si attendono, così l'annullamento è immediato.
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        let status = child.wait();
        let _ = writer.join();
        let _ = reader.join();
        let stderr = errors.join().unwrap_or_default();
        let text = final_text
            .filter(|result| !result.trim().is_empty())
            .unwrap_or(text);
        if text.trim().is_empty() {
            let succeeded = status.as_ref().is_ok_and(|status| status.success());
            return Err(if succeeded {
                AiError::InvalidResponse("Claude Code non ha restituito testo".to_owned())
            } else {
                explain(if stderr.trim().is_empty() {
                    "Claude Code si è chiuso con un errore"
                } else {
                    &stderr
                })
            });
        }
        Ok(text.trim().to_owned())
    }
}

impl AiProvider for ClaudeCode {
    fn list_models(&self) -> Result<Vec<String>, AiError> {
        Ok(MODEL_ALIASES
            .iter()
            .map(|alias| (*alias).to_owned())
            .collect())
    }

    fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        _temperature: f32,
        cancel: &AtomicBool,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<String, AiError> {
        self.run(model, messages, cancel, on_delta)
    }

    fn embed(
        &self,
        _model: &str,
        _inputs: &[String],
        _cancel: &AtomicBool,
    ) -> Result<Vec<Vec<f32>>, AiError> {
        Err(AiError::NotConfigured(
            "Claude Code non fornisce embedding: la ricerca per significato non è disponibile",
        ))
    }

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
        let message = serde_json::json!({
            "type": "user",
            "message": {
                "role": "user",
                "content": [
                    { "type": "text", "text": request },
                    {
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": mime,
                            "data": base64::engine::general_purpose::STANDARD.encode(image),
                        }
                    }
                ]
            }
        });
        self.execute(
            model,
            instructions,
            Input::Message(message),
            cancel,
            &mut |_| {},
        )
    }

    fn provenance(&self, model: &str) -> Provenance {
        Provenance {
            host: CLAUDE_CODE_HOST.to_owned(),
            model: if model.is_empty() || model == "default" {
                "Claude Code".to_owned()
            } else {
                format!("Claude Code · {model}")
            },
            local: false,
        }
    }
}

/// Avvia il processo riprovando brevemente se l'eseguibile risulta «in uso»
/// (ETXTBSY): succede su Linux mentre il file viene scritto, per esempio
/// durante un aggiornamento di Claude Code o, nei test, se un altro thread
/// avvia un processo mentre lo script finto è ancora aperto in scrittura.
fn spawn_with_retry(command: &mut Command) -> std::io::Result<std::process::Child> {
    let mut attempt = 0;
    loop {
        match command.spawn() {
            Err(error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy && attempt < 5 => {
                attempt += 1;
                thread::sleep(Duration::from_millis(20 * attempt));
            }
            other => return other,
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use std::{fs, os::unix::fs::PermissionsExt, path::Path};

    use super::*;

    /// Finto `claude`: registra argomenti e input, poi stampa l'output dato.
    fn fake_claude(directory: &Path, output: &str, extra: &str) -> PathBuf {
        let path = directory.join("claude");
        let log = directory.join("log");
        fs::write(
            &path,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{log}.args'\npwd > '{log}.cwd'\ncat > '{log}.stdin'\n{extra}\ncat <<'EOF'\n{output}\nEOF\n",
                log = log.display()
            ),
        )
        .expect("script");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    fn messages() -> Vec<ChatMessage> {
        vec![
            ChatMessage::new(ChatRole::System, "Istruzioni"),
            ChatMessage::new(ChatRole::User, "Prima domanda"),
            ChatMessage::new(ChatRole::Assistant, "Prima risposta"),
            ChatMessage::new(ChatRole::User, "Contenuto della nota riservato"),
        ]
    }

    #[test]
    fn streams_text_without_tools_and_with_input_on_stdin() {
        let directory = tempfile::tempdir().expect("directory");
        let output = [
            r#"{"type":"system","subtype":"init","tools":[]}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Ciao "}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"[1]"}}}"#,
            r#"{"type":"result","subtype":"success","is_error":false,"result":"Ciao [1]"}"#,
        ]
        .join("\n");
        let executable = fake_claude(directory.path(), &output, "");
        let provider = ClaudeCode::new(executable, Duration::from_secs(10));
        let mut deltas = String::new();
        let text = provider
            .chat(
                "sonnet",
                &messages(),
                0.2,
                &AtomicBool::new(false),
                &mut |delta| deltas.push_str(delta),
            )
            .expect("chat");
        assert_eq!(text, "Ciao [1]");
        assert_eq!(deltas, "Ciao [1]");

        let log = directory.path().join("log");
        let args = fs::read_to_string(log.with_extension("args")).expect("args");
        let args: Vec<&str> = args.lines().collect();
        let tools = args
            .iter()
            .position(|arg| *arg == "--tools")
            .expect("--tools");
        assert_eq!(args[tools + 1], "", "nessuno strumento");
        for flag in [
            "-p",
            "--safe-mode",
            "--strict-mcp-config",
            "--no-session-persistence",
        ] {
            assert!(args.contains(&flag), "{flag}");
        }
        assert!(args.windows(2).any(|pair| pair == ["--model", "sonnet"]));
        assert!(
            !args.iter().any(|arg| arg.contains("riservato")),
            "nota sulla riga di comando"
        );
        let stdin = fs::read_to_string(log.with_extension("stdin")).expect("stdin");
        assert!(stdin.contains("Utente: Prima domanda"));
        assert!(stdin.ends_with("Contenuto della nota riservato"));
        let cwd = fs::read_to_string(log.with_extension("cwd")).expect("cwd");
        assert!(cwd.contains("greatnote-claude-"));
        assert!(
            !Path::new(cwd.trim()).exists(),
            "cartella temporanea rimossa"
        );
    }

    #[test]
    fn images_are_sent_as_structured_input() {
        let directory = tempfile::tempdir().expect("directory");
        let executable = fake_claude(
            directory.path(),
            r#"{"type":"result","subtype":"success","is_error":false,"result":"Totale 37,50"}"#,
            "",
        );
        let provider = ClaudeCode::new(executable, Duration::from_secs(10));
        let text = provider
            .read_image(
                "default",
                "istruzioni",
                "trascrivi",
                b"\x89PNG",
                "image/png",
                &AtomicBool::new(false),
            )
            .expect("read");
        assert_eq!(text, "Totale 37,50");
        let log = directory.path().join("log");
        let args = fs::read_to_string(log.with_extension("args")).expect("args");
        assert!(
            args.lines()
                .collect::<Vec<_>>()
                .windows(2)
                .any(|pair| pair == ["--input-format", "stream-json"])
        );
        let stdin = fs::read_to_string(log.with_extension("stdin")).expect("stdin");
        let message: Value = serde_json::from_str(stdin.trim()).expect("json");
        assert_eq!(message["message"]["content"][1]["type"], "image");
        assert_eq!(
            message["message"]["content"][1]["source"]["media_type"],
            "image/png"
        );
    }

    #[test]
    fn errors_are_explained() {
        let directory = tempfile::tempdir().expect("directory");
        let executable = fake_claude(
            directory.path(),
            r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login"}"#,
            "",
        );
        let provider = ClaudeCode::new(executable, Duration::from_secs(10));
        let error = provider
            .chat(
                "default",
                &messages(),
                0.2,
                &AtomicBool::new(false),
                &mut |_| {},
            )
            .expect_err("error");
        assert!(error.to_string().contains("accedi"), "{error}");
        let args = fs::read_to_string(directory.path().join("log.args")).expect("args");
        assert!(
            !args.lines().any(|arg| arg == "--model"),
            "modello predefinito"
        );

        assert!(locate("/percorso/inesistente/claude").is_err());
        assert!(matches!(
            provider.embed("x", &[], &AtomicBool::new(false)),
            Err(AiError::NotConfigured(_))
        ));
        assert!(!provider.provenance("default").local);
    }

    #[test]
    fn slow_runs_can_be_cancelled() {
        let directory = tempfile::tempdir().expect("directory");
        let executable = fake_claude(directory.path(), "", "sleep 30");
        let provider = ClaudeCode::new(executable, Duration::from_secs(60));
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let flag = std::sync::Arc::clone(&cancel);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(400));
            flag.store(true, Ordering::Relaxed);
        });
        let started = Instant::now();
        let result = provider.chat("default", &messages(), 0.2, &cancel, &mut |_| {});
        assert!(matches!(result, Err(AiError::Cancelled)));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn status_reports_login_without_personal_data() {
        let directory = tempfile::tempdir().expect("directory");
        let path = directory.path().join("claude");
        fs::write(
            &path,
            "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then echo '2.1.0 (Claude Code)'; else echo '{\"loggedIn\":true,\"authMethod\":\"claude.ai\",\"email\":\"x@example.com\"}'; fi\n",
        )
        .expect("script");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("chmod");
        let status = ClaudeCode::new(path, Duration::from_secs(5))
            .status()
            .expect("status");
        assert!(status.logged_in);
        assert_eq!(status.auth_method.as_deref(), Some("claude.ai"));
        assert_eq!(status.version, "2.1.0 (Claude Code)");
        assert!(!format!("{status:?}").contains("example.com"));
    }
}
