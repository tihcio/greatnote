//! Cartella dei modelli scaricati: download con ripresa, verifica dell'hash e
//! rimozione. Un file compare con il suo nome definitivo solo dopo che
//! dimensione e SHA-256 corrispondono al catalogo; i download interrotti
//! restano in un file `.part` e riprendono da dove si erano fermati.

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use sha2::{Digest, Sha256};
use ureq::Agent;

use super::catalog::{CATALOG, CatalogModel, ModelFile};
use crate::{ai::AiError, backup::private_directory};

const PARTIAL_SUFFIX: &str = ".part";
const BUFFER_BYTES: usize = 1 << 20;
/// Intervallo minimo tra due notifiche di avanzamento.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone)]
pub struct ModelStore {
    directory: PathBuf,
}

impl ModelStore {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Percorso del file: i nomi vengono solo dal catalogo, mai dall'utente.
    pub fn path(&self, file: &ModelFile) -> PathBuf {
        self.directory.join(file.file_name)
    }

    fn partial_path(&self, file: &ModelFile) -> PathBuf {
        self.directory.join(format!("{}{PARTIAL_SUFFIX}", file.file_name))
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

    pub fn installed(&self) -> Vec<&'static CatalogModel> {
        CATALOG
            .iter()
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
        let mut request = agent.get(file.url);
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
    pub fn remove(&self, model: &CatalogModel, keep: &[&CatalogModel]) -> Result<(), AiError> {
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
            vision.projector.expect("projector").size_bytes
        );
        assert_eq!(store.installed().len(), 1);

        // Il file condiviso con un modello installato resta.
        store.remove(vision, &[chat]).expect("remove vision");
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
        assert!(store.partial_path(&model.model).exists(), "ripresa possibile");
    }
}
