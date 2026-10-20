//! Processi `llama-server` del provider integrato.
//!
//! Ogni configurazione (modello, proiettore, embedding) ha il suo processo,
//! avviato alla prima richiesta e fermato dopo un periodo di inattività per
//! liberare memoria. Il server ascolta solo su 127.0.0.1, su una porta libera
//! scelta all'avvio, e accetta solo richieste con una chiave casuale generata
//! a ogni avvio e passata nell'ambiente (non visibile nella riga di comando).

use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use ureq::Agent;
use uuid::Uuid;

use super::{
    catalog::{CatalogModel, ModelRole},
    store::ModelStore,
};
use crate::ai::{AiError, provider::ApiKey, settings::Endpoint};

/// Nome dell'eseguibile distribuito con l'app.
#[cfg(windows)]
pub const SERVER_EXECUTABLE: &str = "llama-server.exe";
#[cfg(not(windows))]
pub const SERVER_EXECUTABLE: &str = "llama-server";

/// Inattività dopo cui un server viene fermato.
pub const IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Tempo massimo per caricare un modello (dischi lenti, modelli grandi).
const STARTUP_TIMEOUT: Duration = Duration::from_secs(240);
const HEALTH_POLL: Duration = Duration::from_millis(300);
const LOG_TAIL_BYTES: u64 = 4096;
const EMBEDDING_CONTEXT: u32 = 8192;
const EMBEDDING_BATCH: u32 = 2048;
const VISION_CONTEXT: u32 = 8192;

/// Configurazione di un processo: due ruoli con la stessa configurazione
/// usano lo stesso processo.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServerSpec {
    pub model_file: &'static str,
    pub projector_file: Option<&'static str>,
    pub embedding: bool,
    pub context_tokens: u32,
    /// Nome con cui il server espone il modello (id del catalogo).
    pub alias: &'static str,
}

impl ServerSpec {
    /// Configurazione per un modello del catalogo. `chat_context_tokens` vale
    /// per la chat; embedding e visione usano valori fissi.
    pub fn for_model(model: &CatalogModel, chat_context_tokens: u32) -> Self {
        Self {
            model_file: model.model.file_name,
            projector_file: model.projector.map(|file| file.file_name),
            embedding: model.role == ModelRole::Embedding,
            context_tokens: match model.role {
                ModelRole::Chat => chat_context_tokens,
                ModelRole::Embedding => EMBEDDING_CONTEXT,
                ModelRole::Vision => VISION_CONTEXT.max(chat_context_tokens),
            },
            alias: model.id,
        }
    }

    fn arguments(&self, store: &ModelStore, port: u16, gpu: bool) -> Vec<String> {
        let mut arguments = vec![
            "--host".to_owned(),
            Ipv4Addr::LOCALHOST.to_string(),
            "--port".to_owned(),
            port.to_string(),
            "--model".to_owned(),
            path_argument(&store.directory().join(self.model_file)),
            "--alias".to_owned(),
            self.alias.to_owned(),
            "--ctx-size".to_owned(),
            self.context_tokens.to_string(),
            "--parallel".to_owned(),
            "1".to_owned(),
            "--no-webui".to_owned(),
            "--offline".to_owned(),
        ];
        if let Some(projector) = self.projector_file {
            arguments.push("--mmproj".to_owned());
            arguments.push(path_argument(&store.directory().join(projector)));
        }
        if self.embedding {
            let batch = EMBEDDING_BATCH.to_string();
            arguments.extend([
                "--embedding".to_owned(),
                "--batch-size".to_owned(),
                batch.clone(),
                "--ubatch-size".to_owned(),
                batch,
            ]);
        } else {
            // Risposte dirette: il ragionamento dei modelli piccoli su CPU
            // allunga molto i tempi senza migliorare abbastanza i risultati.
            arguments.extend(["--reasoning-budget", "0"].map(str::to_owned));
        }
        if !gpu {
            arguments.extend(["--device", "none"].map(str::to_owned));
        }
        arguments
    }
}

/// Contesto della chat in token dal limite di caratteri delle impostazioni:
/// circa 3 caratteri per token, più spazio per istruzioni e risposta.
pub fn chat_context_tokens(max_context_chars: u32) -> u32 {
    let needed = max_context_chars / 3 + 4096;
    needed
        .div_ceil(4096)
        .saturating_mul(4096)
        .clamp(8192, 32_768)
}

fn path_argument(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

struct RunningServer {
    child: Mutex<Child>,
    port: u16,
    api_key: String,
    in_use: AtomicUsize,
    last_used: Mutex<Instant>,
}

impl RunningServer {
    fn has_exited(&self) -> bool {
        self.child
            .lock()
            .map(|mut child| !matches!(child.try_wait(), Ok(None)))
            .unwrap_or(true)
    }

    fn stop(&self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    fn idle_for(&self) -> Duration {
        self.last_used
            .lock()
            .map(|instant| instant.elapsed())
            .unwrap_or_default()
    }
}

/// Server in uso: finché esiste, il server non viene fermato per inattività.
pub struct ServerLease {
    server: Arc<RunningServer>,
    pub endpoint: Endpoint,
    pub api_key: ApiKey,
}

impl Drop for ServerLease {
    fn drop(&mut self) {
        if let Ok(mut last_used) = self.server.last_used.lock() {
            *last_used = Instant::now();
        }
        self.server.in_use.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Eseguibile del server e processi attivi.
pub struct BuiltinRuntime {
    executable: Option<PathBuf>,
    store: ModelStore,
    servers: Mutex<HashMap<ServerSpec, Arc<RunningServer>>>,
}

impl BuiltinRuntime {
    /// `executable`: `llama-server` distribuito con l'app, se presente.
    pub fn new(executable: Option<PathBuf>, store: ModelStore) -> Self {
        Self {
            executable,
            store,
            servers: Mutex::new(HashMap::new()),
        }
    }

    pub fn store(&self) -> &ModelStore {
        &self.store
    }

    pub fn executable(&self) -> Option<&Path> {
        self.executable.as_deref()
    }

    /// Server avviato e pronto per la configurazione richiesta.
    pub fn lease(&self, spec: &ServerSpec, cancel: &AtomicBool) -> Result<ServerLease, AiError> {
        let executable = self.executable.as_deref().ok_or(AiError::NotConfigured(
            "il motore AI integrato non è incluso in questa installazione",
        ))?;
        for file in std::iter::once(spec.model_file).chain(spec.projector_file) {
            if !self.store.directory().join(file).is_file() {
                return Err(AiError::NotConfigured(
                    "il modello scelto non è scaricato: scaricalo nelle impostazioni AI",
                ));
            }
        }
        // Un avvio alla volta: due richieste contemporanee non devono
        // caricare due volte lo stesso modello.
        let mut servers = self
            .servers
            .lock()
            .map_err(|_| AiError::Builtin("stato dei server non disponibile".to_owned()))?;
        let reusable = servers
            .get(spec)
            .filter(|server| !server.has_exited())
            .cloned();
        let server = match reusable {
            Some(server) => server,
            None => {
                if let Some(dead) = servers.remove(spec) {
                    dead.stop();
                }
                let server = Arc::new(self.start(executable, spec, cancel)?);
                servers.insert(spec.clone(), Arc::clone(&server));
                server
            }
        };
        server.in_use.fetch_add(1, Ordering::SeqCst);
        let endpoint = Endpoint::parse(&format!("http://127.0.0.1:{}/v1", server.port))?;
        let api_key = ApiKey::new(server.api_key.clone())
            .ok_or_else(|| AiError::Builtin("chiave del server vuota".to_owned()))?;
        Ok(ServerLease {
            server,
            endpoint,
            api_key,
        })
    }

    fn start(
        &self,
        executable: &Path,
        spec: &ServerSpec,
        cancel: &AtomicBool,
    ) -> Result<RunningServer, AiError> {
        // Prima con l'accelerazione grafica (llama.cpp sceglie da solo quanti
        // strati spostare sulla GPU); se l'avvio fallisce, solo CPU.
        match self.start_once(executable, spec, true, cancel) {
            Ok(server) => Ok(server),
            Err(AiError::Cancelled) => Err(AiError::Cancelled),
            Err(error) => {
                tracing::warn!(
                    "avvio del motore AI con GPU non riuscito, riprovo solo CPU: {error}"
                );
                self.start_once(executable, spec, false, cancel)
            }
        }
    }

    fn start_once(
        &self,
        executable: &Path,
        spec: &ServerSpec,
        gpu: bool,
        cancel: &AtomicBool,
    ) -> Result<RunningServer, AiError> {
        let port = free_port()?;
        let api_key = Uuid::new_v4().simple().to_string();
        let log_path = self
            .store
            .directory()
            .join(format!("server-{}.log", spec.alias));
        let log = File::create(&log_path).map_err(|error| {
            AiError::Builtin(format!("registro del server non creato: {error}"))
        })?;
        let mut command = Command::new(executable);
        command
            .args(spec.arguments(&self.store, port, gpu))
            .env("LLAMA_API_KEY", &api_key)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log);
        if let Some(directory) = executable.parent() {
            // Le librerie del motore sono accanto all'eseguibile.
            command.current_dir(directory);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }
        let child = command.spawn().map_err(|error| {
            AiError::Builtin(format!("avvio del motore AI non riuscito: {error}"))
        })?;
        let server = RunningServer {
            child: Mutex::new(child),
            port,
            api_key,
            in_use: AtomicUsize::new(0),
            last_used: Mutex::new(Instant::now()),
        };
        match wait_until_ready(&server, cancel) {
            Ok(()) => Ok(server),
            Err(error) => {
                server.stop();
                Err(match error {
                    AiError::Builtin(message) => AiError::Builtin(format!(
                        "{message}{}",
                        log_tail(&log_path)
                            .map(|tail| format!(". Ultime righe del registro: {tail}"))
                            .unwrap_or_default()
                    )),
                    other => other,
                })
            }
        }
    }

    /// Ferma i server inutilizzati da più di `idle`.
    pub fn stop_idle(&self, idle: Duration) {
        let Ok(mut servers) = self.servers.lock() else {
            return;
        };
        servers.retain(|_, server| {
            let unused = server.in_use.load(Ordering::SeqCst) == 0 && server.idle_for() >= idle;
            if unused || server.has_exited() {
                server.stop();
                false
            } else {
                true
            }
        });
    }

    /// Ferma tutti i server (uscita dall'app, eliminazione di un modello).
    pub fn stop_all(&self) {
        if let Ok(mut servers) = self.servers.lock() {
            for (_, server) in servers.drain() {
                server.stop();
            }
        }
    }

    pub fn running(&self) -> usize {
        self.servers
            .lock()
            .map(|servers| servers.len())
            .unwrap_or(0)
    }
}

impl Drop for BuiltinRuntime {
    fn drop(&mut self) {
        self.stop_all();
    }
}

fn free_port() -> Result<u16, AiError> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .and_then(|listener| listener.local_addr())
        .map(|address| address.port())
        .map_err(|error| AiError::Builtin(format!("nessuna porta locale libera: {error}")))
}

/// Attende che `/health` risponda 200 (modello caricato).
fn wait_until_ready(server: &RunningServer, cancel: &AtomicBool) -> Result<(), AiError> {
    let agent: Agent = Agent::config_builder()
        .http_status_as_error(false)
        .max_redirects(0)
        .proxy(None)
        .timeout_global(Some(Duration::from_secs(5)))
        .build()
        .new_agent();
    let url = format!("http://127.0.0.1:{}/health", server.port);
    let started = Instant::now();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Cancelled);
        }
        if server.has_exited() {
            return Err(AiError::Builtin(
                "il motore AI si è chiuso durante il caricamento del modello (memoria insufficiente?)"
                    .to_owned(),
            ));
        }
        if let Ok(response) = agent.get(&url).call()
            && response.status().as_u16() == 200
        {
            return Ok(());
        }
        if started.elapsed() > STARTUP_TIMEOUT {
            return Err(AiError::Builtin(
                "il caricamento del modello richiede troppo tempo".to_owned(),
            ));
        }
        thread::sleep(HEALTH_POLL);
    }
}

/// Ultime righe del registro del server, senza caratteri di controllo.
fn log_tail(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let length = fs::metadata(path).ok()?.len();
    file.seek(SeekFrom::Start(length.saturating_sub(LOG_TAIL_BYTES)))
        .ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let text = String::from_utf8_lossy(&bytes);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let tail = lines[lines.len().saturating_sub(4)..].join(" | ");
    let tail: String = tail
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(600)
        .collect();
    (!tail.is_empty()).then_some(tail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::builtin::catalog::find;

    #[test]
    fn specs_share_a_process_only_with_the_same_configuration() {
        let chat = find("gemma-4-e2b").expect("chat");
        let vision = find("gemma-4-e2b-vision").expect("vision");
        let embedding = find("bge-m3").expect("embedding");
        let chat_spec = ServerSpec::for_model(chat, 8192);
        let vision_spec = ServerSpec::for_model(vision, 8192);
        assert_eq!(chat_spec.model_file, vision_spec.model_file);
        assert_ne!(chat_spec, vision_spec);
        let embedding_spec = ServerSpec::for_model(embedding, 8192);
        assert!(embedding_spec.embedding);
        assert_eq!(embedding_spec.context_tokens, EMBEDDING_CONTEXT);

        let store = ModelStore::new(PathBuf::from("models"));
        let arguments = vision_spec.arguments(&store, 4321, false);
        let joined = arguments.join(" ");
        assert!(joined.contains("--host 127.0.0.1 --port 4321"));
        assert!(joined.contains("--mmproj"));
        assert!(joined.contains("--device none"));
        assert!(!joined.contains("--embedding"));
        // La chiave non compare mai tra gli argomenti.
        assert!(!joined.contains("api-key"));
        let embedding_arguments = embedding_spec.arguments(&store, 4321, true).join(" ");
        assert!(embedding_arguments.contains("--embedding"));
        assert!(!embedding_arguments.contains("--device"));
    }

    #[test]
    fn chat_context_follows_the_character_limit() {
        assert_eq!(chat_context_tokens(2_000), 8192);
        assert_eq!(chat_context_tokens(12_000), 8192);
        assert_eq!(chat_context_tokens(30_000), 16_384);
        assert_eq!(chat_context_tokens(60_000), 24_576);
    }

    #[test]
    fn missing_engine_or_model_is_reported_without_starting_anything() {
        let directory = tempfile::tempdir().expect("directory");
        let store = ModelStore::new(directory.path().to_path_buf());
        let spec = ServerSpec::for_model(find("bge-m3").expect("model"), 8192);
        let cancel = AtomicBool::new(false);
        let without_engine = BuiltinRuntime::new(None, store.clone());
        assert!(matches!(
            without_engine.lease(&spec, &cancel),
            Err(AiError::NotConfigured(_))
        ));
        let runtime = BuiltinRuntime::new(Some(directory.path().join("llama-server")), store);
        assert!(matches!(
            runtime.lease(&spec, &cancel),
            Err(AiError::NotConfigured(_))
        ));
        assert_eq!(runtime.running(), 0);
    }
}
