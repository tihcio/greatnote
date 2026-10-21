//! Cartella dei modelli scaricati: download con ripresa, verifica dell'hash e
//! rimozione. Un file compare con il suo nome definitivo solo dopo che
//! dimensione e SHA-256 corrispondono al catalogo; i download interrotti
//! restano in un file `.part` e riprendono da dove si erano fermati.
//!
//! I modelli scelti su Hugging Face (ADR 0021) sono registrati in
//! `hub-models.json` nella stessa cartella, con URL, dimensioni e hash fissati
//! al momento della scelta. Le voci lette dal disco vengono ricontrollate.

use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use ureq::Agent;

use super::catalog::{self, CatalogModel, ModelFile, ModelRole, curated};
use crate::{
    ai::AiError,
    backup::{create_private_file, private_directory},
};

const PARTIAL_SUFFIX: &str = ".part";
const REGISTRY_FILE: &str = "hub-models.json";
const MAX_REGISTRY_BYTES: u64 = 4 * 1024 * 1024;
const MAX_REGISTERED_MODELS: usize = 200;
const BUFFER_BYTES: usize = 1 << 20;
/// Intervallo minimo tra due notifiche di avanzamento.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone)]
pub struct ModelStore {
    directory: PathBuf,
    /// Una sola scrittura alla volta del registro.
    registry_lock: Arc<Mutex<()>>,
}

impl ModelStore {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            registry_lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Percorso del file: i nomi vengono dal catalogo o dal registro, già
    /// controllati, mai direttamente dall'utente.
    pub fn path(&self, file: &ModelFile) -> PathBuf {
        self.directory.join(&file.file_name)
    }

    fn partial_path(&self, file: &ModelFile) -> PathBuf {
        self.directory
            .join(format!("{}{PARTIAL_SUFFIX}", file.file_name))
    }

    fn registry_path(&self) -> PathBuf {
        self.directory.join(REGISTRY_FILE)
    }

    /// Modelli di Hugging Face registrati. Le voci non valide (file
    /// modificato a mano o danneggiato) sono ignorate.
    pub fn hub_models(&self) -> Vec<CatalogModel> {
        let path = self.registry_path();
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            return Vec::new();
        };
        if !metadata.is_file() || metadata.len() > MAX_REGISTRY_BYTES {
            tracing::warn!("registro dei modelli ignorato: non è un file valido");
            return Vec::new();
        }
        let parsed = fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<CatalogModel>>(&text).ok());
        let Some(models) = parsed else {
            tracing::warn!("registro dei modelli illeggibile: ignorato");
            return Vec::new();
        };
        let mut seen = HashSet::new();
        models
            .into_iter()
            .filter(|model| {
                !model.is_curated()
                    && model.is_valid()
                    && catalog::find(&model.id).is_none()
                    && seen.insert(model.id.clone())
            })
            .collect()
    }

    /// Catalogo curato più modelli registrati, nell'ordine in cui mostrarli.
    pub fn models(&self) -> Vec<CatalogModel> {
        let mut models = curated().to_vec();
        models.extend(self.hub_models());
        models
    }

    pub fn find(&self, id: &str) -> Option<CatalogModel> {
        catalog::find(id)
            .cloned()
            .or_else(|| self.hub_models().into_iter().find(|model| model.id == id))
    }

    pub fn find_with_role(&self, id: &str, role: ModelRole) -> Option<CatalogModel> {
        self.find(id).filter(|model| model.role == role)
    }

    /// Aggiunge (o aggiorna) un modello di Hugging Face nel registro.
    pub fn register(&self, model: &CatalogModel) -> Result<(), AiError> {
        if model.is_curated() || !model.is_valid() || catalog::find(&model.id).is_some() {
            return Err(AiError::Hub("modello non valido".to_owned()));
        }
        let _guard = self.lock_registry()?;
        let mut models = self.hub_models();
        models.retain(|existing| existing.id != model.id);
        if models.len() >= MAX_REGISTERED_MODELS {
            return Err(AiError::Hub(
                "troppi modelli registrati: eliminane qualcuno".to_owned(),
            ));
        }
        models.push(model.clone());
        self.write_registry(&models)
    }

    /// Toglie un modello dal registro (i file si eliminano con [`Self::remove`]).
    pub fn unregister(&self, id: &str) -> Result<(), AiError> {
        let _guard = self.lock_registry()?;
        let mut models = self.hub_models();
        let before = models.len();
        models.retain(|model| model.id != id);
        if models.len() == before {
            return Ok(());
        }
        self.write_registry(&models)
    }

    fn lock_registry(&self) -> Result<std::sync::MutexGuard<'_, ()>, AiError> {
        self.registry_lock
            .lock()
            .map_err(|_| AiError::Builtin("registro dei modelli non disponibile".to_owned()))
    }

    /// Scrittura atomica: il registro non resta mai a metà.
    fn write_registry(&self, models: &[CatalogModel]) -> Result<(), AiError> {
        private_directory(&self.directory)?;
        let json = serde_json::to_vec_pretty(models)
            .map_err(|error| AiError::Builtin(format!("registro dei modelli: {error}")))?;
        let temporary = self.directory.join(format!(
            "{REGISTRY_FILE}.{}.tmp",
            uuid::Uuid::new_v4().simple()
        ));
        let written = create_private_file(&temporary)
            .map_err(AiError::from)
            .and_then(|mut file| {
                file.write_all(&json)
                    .and_then(|()| file.sync_all())
                    .map_err(io_error)
            })
            .and_then(|()| fs::rename(&temporary, self.registry_path()).map_err(io_error));
        if written.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        written
    }

    /// Il file definitivo esiste con la dimensione attesa (l'hash è già
    /// stato verificato prima di dargli il nome definitivo).
    pub fn is_present(&self, file: &ModelFile) -> bool {
        fs::symlink_metadata(self.path(file))
            .is_ok_and(|metadata| metadata.is_file() && metadata.len() == file.size_bytes)
    }

    pub fn is_installed(&self, model: &CatalogModel) -> bool {
        model.files().all(|file| self.is_present(file))
    }

    /// Byte ancora da scaricare per completare il modello.
    pub fn missing_bytes(&self, model: &CatalogModel) -> u64 {
        model
            .files()
            .filter(|file| !self.is_present(file))
            .map(|file| file.size_bytes)
            .sum()
    }

    pub fn installed(&self) -> Vec<CatalogModel> {
        self.models()
            .into_iter()
            .filter(|model| self.is_installed(model))
            .collect()
    }

    /// Scarica i file mancanti del modello. `on_progress` riceve i byte già
    /// presenti o scaricati e il totale del modello.
    pub fn download(
        &self,
        model: &CatalogModel,
        cancel: &AtomicBool,
        on_progress: &mut dyn FnMut(u64, u64),
    ) -> Result<(), AiError> {
        private_directory(&self.directory)?;
        let agent = download_agent();
        let total = model.size_bytes();
        let mut completed = 0;
        for file in model.files() {
            if !self.is_present(file) {
                self.download_file(&agent, file, completed, total, cancel, on_progress)?;
            }
            completed += file.size_bytes;
            on_progress(completed, total);
        }
        Ok(())
    }

    fn download_file(
        &self,
        agent: &Agent,
        file: &ModelFile,
        completed: u64,
        total: u64,
        cancel: &AtomicBool,
        on_progress: &mut dyn FnMut(u64, u64),
    ) -> Result<(), AiError> {
        let partial = self.partial_path(file);
        let mut hasher = Sha256::new();
        let mut existing = match fs::symlink_metadata(&partial) {
            Ok(metadata) if metadata.is_file() && metadata.len() < file.size_bytes => {
                hash_file(&partial, &mut hasher, cancel)?;
                metadata.len()
            }
            Ok(_) => {
                remove_if_exists(&partial)?;
                0
            }
            Err(_) => 0,
        };
        let mut request = agent.get(&file.url);
        if existing > 0 {
            request = request.header("Range", format!("bytes={existing}-"));
        }
        let mut response = request.call().map_err(|error| match error {
            ureq::Error::Timeout(_) => download_error("il server non risponde, riprova più tardi"),
            ureq::Error::HostNotFound | ureq::Error::ConnectionFailed => {
                download_error("connessione non riuscita: controlla la rete")
            }
            other => AiError::ModelDownload(other.to_string()),
        })?;
        let status = response.status().as_u16();
        let mut output = match status {
            206 if existing > 0 => OpenOptions::new()
                .append(true)
                .open(&partial)
                .map_err(io_error)?,
            200 => {
                // Il server non ha accettato la ripresa: si riparte da zero.
                hasher = Sha256::new();
                existing = 0;
                File::create(&partial).map_err(io_error)?
            }
            416 => {
                remove_if_exists(&partial)?;
                return Err(download_error("download da riprendere da capo: riprova"));
            }
            other => {
                return Err(AiError::ModelDownload(format!(
                    "il server ha risposto con il codice {other}"
                )));
            }
        };
        let mut reader = response.body_mut().as_reader();
        let mut buffer = vec![0; BUFFER_BYTES];
        let mut written = existing;
        let mut last_progress = Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                // Il file parziale resta per riprendere il download.
                return Err(AiError::Cancelled);
            }
            let read = reader.read(&mut buffer).map_err(|error| {
                AiError::ModelDownload(format!(
                    "connessione interrotta ({error}): riprova per riprendere"
                ))
            })?;
            if read == 0 {
                break;
            }
            written += read as u64;
            if written > file.size_bytes {
                drop(output);
                remove_if_exists(&partial)?;
                return Err(download_error("il file è più grande del previsto"));
            }
            hasher.update(&buffer[..read]);
            output.write_all(&buffer[..read]).map_err(io_error)?;
            if last_progress.elapsed() >= PROGRESS_INTERVAL {
                last_progress = Instant::now();
                on_progress(completed + written, total);
            }
        }
        output.sync_all().map_err(io_error)?;
        drop(output);
        if written != file.size_bytes {
            return Err(download_error(
                "download incompleto: riprova per riprendere da dove si è fermato",
            ));
        }
        if format!("{:x}", hasher.finalize()) != file.sha256 {
            remove_if_exists(&partial)?;
            return Err(download_error(
                "il file scaricato non corrisponde a quello previsto (hash diverso) ed è stato eliminato",
            ));
        }
        fs::rename(&partial, self.path(file)).map_err(io_error)
    }

    /// Elimina i file del modello non usati da `keep` (altri modelli
    /// installati che condividono lo stesso file), e il download parziale.
    pub fn remove(&self, model: &CatalogModel, keep: &[CatalogModel]) -> Result<(), AiError> {
        for file in model.files() {
            let shared = keep
                .iter()
                .filter(|other| other.id != model.id)
                .any(|other| other.files().any(|kept| kept.file_name == file.file_name));
            remove_if_exists(&self.partial_path(file))?;
            if !shared {
                remove_if_exists(&self.path(file))?;
            }
        }
        Ok(())
    }

    /// Spazio occupato dai modelli e dai download parziali.
    pub fn used_bytes(&self) -> u64 {
        let Ok(entries) = fs::read_dir(&self.directory) else {
            return 0;
        };
        entries
            .filter_map(Result::ok)
            .filter_map(|entry| entry.metadata().ok())
            .filter(fs::Metadata::is_file)
            .map(|metadata| metadata.len())
            .sum()
    }
}

/// Client per i download: solo https, reindirizzamenti consentiti (Hugging
/// Face rimanda alla sua CDN), nessun limite di dimensione oltre a quello del
/// catalogo, controllato durante la lettura.
fn download_agent() -> Agent {
    Agent::config_builder()
        .http_status_as_error(false)
        .https_only(true)
        .max_redirects(5)
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        // Tempo massimo complessivo per un file: anche le connessioni lente
        // finiscono, quelle bloccate prima o poi si interrompono.
        .timeout_recv_body(Some(Duration::from_secs(6 * 60 * 60)))
        .user_agent(concat!("Greatnote/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

fn hash_file(path: &Path, hasher: &mut Sha256, cancel: &AtomicBool) -> Result<(), AiError> {
    let mut file = File::open(path).map_err(io_error)?;
    let mut buffer = vec![0; BUFFER_BYTES];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(AiError::Cancelled);
        }
        let read = file.read(&mut buffer).map_err(io_error)?;
        if read == 0 {
            return Ok(());
        }
        hasher.update(&buffer[..read]);
    }
}

fn remove_if_exists(path: &Path) -> Result<(), AiError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(io_error(error)),
    }
}

fn download_error(message: &str) -> AiError {
    AiError::ModelDownload(message.to_owned())
}

fn io_error(error: std::io::Error) -> AiError {
    if error.kind() == std::io::ErrorKind::StorageFull {
        download_error("spazio su disco insufficiente")
    } else {
        AiError::ModelDownload(format!("errore del disco: {error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::builtin::catalog::{ModelRole, find};

    #[test]
    fn presence_requires_exact_size_and_removal_keeps_shared_files() {
        let directory = tempfile::tempdir().expect("directory");
        let store = ModelStore::new(directory.path().join("models"));
        let chat = find("gemma-4-e2b").expect("chat");
        let vision = find("gemma-4-e2b-vision").expect("vision");
        assert_eq!(vision.role, ModelRole::Vision);
        assert!(!store.is_installed(chat));
        assert_eq!(store.missing_bytes(vision), vision.size_bytes());
        assert_eq!(store.used_bytes(), 0);

        private_directory(store.directory()).expect("private");
        // Un file con dimensione diversa non conta come scaricato.
        fs::write(store.path(&chat.model), b"troncato").expect("write");
        assert!(!store.is_present(&chat.model));
        File::create(store.path(&chat.model))
            .and_then(|file| file.set_len(chat.model.size_bytes))
            .expect("sparse file");
        assert!(store.is_installed(chat));
        assert_eq!(
            store.missing_bytes(vision),
            vision.projector.as_ref().expect("projector").size_bytes
        );
        assert_eq!(store.installed().len(), 1);

        // Il file condiviso con un modello installato resta.
        store
            .remove(vision, std::slice::from_ref(chat))
            .expect("remove vision");
        assert!(store.is_installed(chat));
        store.remove(chat, &[]).expect("remove chat");
        assert!(!store.path(&chat.model).exists());
    }

    #[test]
    fn cancelled_download_stops_before_any_request() {
        let directory = tempfile::tempdir().expect("directory");
        let store = ModelStore::new(directory.path().to_path_buf());
        let model = find("bge-m3").expect("model");
        // Un file parziale già presente viene letto per l'hash: annullando
        // prima, non parte nessuna richiesta di rete.
        fs::write(store.partial_path(&model.model), b"inizio").expect("partial");
        let cancel = AtomicBool::new(true);
        let result = store.download(model, &cancel, &mut |_, _| {});
        assert!(matches!(result, Err(AiError::Cancelled)));
        assert!(
            store.partial_path(&model.model).exists(),
            "ripresa possibile"
        );
    }

    fn hub_model(variant: &str) -> CatalogModel {
        let revision = "6938c58e698502037401a5f19f6905f2fb2f7dee";
        CatalogModel {
            id: catalog::hub_model_id(ModelRole::Chat, "autore/repo-GGUF", variant),
            role: ModelRole::Chat,
            name: format!("repo · {variant}"),
            license: "apache-2.0".to_owned(),
            memory_gb: 2.0,
            recommended: false,
            model: ModelFile {
                file_name: format!("hf-000000000000-{variant}.gguf"),
                url: format!(
                    "https://huggingface.co/autore/repo-GGUF/resolve/{revision}/{variant}.gguf"
                ),
                size_bytes: 1024,
                sha256: "ab".repeat(32),
            },
            parts: Vec::new(),
            projector: None,
            source: catalog::ModelSource::HuggingFace {
                repo: "autore/repo-GGUF".to_owned(),
                revision: revision.to_owned(),
            },
        }
    }

    #[test]
    fn registry_keeps_valid_hub_models_and_ignores_tampered_entries() {
        let directory = tempfile::tempdir().expect("directory");
        let store = ModelStore::new(directory.path().join("models"));
        assert!(store.hub_models().is_empty());
        let first = hub_model("Q4_K_M");
        store.register(&first).expect("register");
        store.register(&first).expect("aggiornare non duplica");
        store.register(&hub_model("Q8_0")).expect("register");
        assert_eq!(store.hub_models().len(), 2);
        assert_eq!(store.models().len(), curated().len() + 2);
        assert_eq!(store.find(&first.id), Some(first.clone()));
        assert!(
            store
                .find_with_role(&first.id, ModelRole::Embedding)
                .is_none()
        );
        assert!(store.find("bge-m3").is_some(), "il catalogo curato resta");
        // Né modelli curati né modelli incoerenti nel registro.
        assert!(store.register(find("bge-m3").expect("curated")).is_err());
        let mut unsafe_name = hub_model("Q5_K_M");
        unsafe_name.model.file_name = "../fuori.gguf".to_owned();
        assert!(store.register(&unsafe_name).is_err());

        // Una voce modificata a mano con un percorso pericoloso è ignorata.
        let mut entries: Vec<serde_json::Value> =
            serde_json::from_str(&fs::read_to_string(store.registry_path()).expect("read"))
                .expect("json");
        entries[0]["model"]["file_name"] = serde_json::json!("../../segreto.gguf");
        fs::write(
            store.registry_path(),
            serde_json::to_string(&entries).expect("json"),
        )
        .expect("write");
        assert_eq!(store.hub_models().len(), 1);

        store.unregister(&hub_model("Q8_0").id).expect("unregister");
        assert!(store.hub_models().is_empty());
        fs::write(store.registry_path(), b"{non json").expect("write");
        assert!(
            store.hub_models().is_empty(),
            "registro danneggiato ignorato"
        );
        store
            .register(&first)
            .expect("si riparte da un registro valido");
        assert_eq!(store.hub_models().len(), 1);
    }
}
