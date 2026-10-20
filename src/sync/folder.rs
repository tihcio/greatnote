//! Libreria di sincronizzazione: struttura, lotti, dispositivi e blob sopra
//! un archivio di file intercambiabile ([`SyncStore`]): una cartella locale
//! sincronizzata da un servizio esterno oppure Dropbox. Ogni dispositivo
//! scrive solo nei propri file; i lotti sono immutabili e i blob sono
//! indirizzati per hash.

use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

pub(crate) const ROOT: &str = "greatnote-sync";
pub const FORMAT: u32 = 1;
/// Dimensione massima di un lotto letto (protezione da file anomali).
const MAX_BATCH_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum FolderError {
    #[error("cartella di sincronizzazione non accessibile: {0}")]
    Io(#[from] std::io::Error),
    #[error(
        "la cartella contiene una libreria di formato {0}, più recente di questa versione di Greatnote"
    )]
    NewerFormat(u32),
    #[error("file di sincronizzazione non valido: {0}")]
    Invalid(String),
    #[error("il file esiste già")]
    Exists,
    #[error("{0}")]
    Remote(String),
}

/// Archivio di file della libreria. I percorsi sono relativi alla radice
/// della libreria, con `/` come separatore, e vengono solo da questo modulo.
pub trait SyncStore {
    /// Contenuto di un file; `None` se non esiste.
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, FolderError>;
    /// Scrive un file in modo atomico. Con `create_only` non sovrascrive:
    /// restituisce [`FolderError::Exists`] se il file c'è già.
    fn write(&self, path: &str, bytes: &[u8], create_only: bool) -> Result<(), FolderError>;
    fn exists(&self, path: &str) -> Result<bool, FolderError>;
    /// Voci di una cartella (nome, è cartella); vuoto se non esiste.
    fn list(&self, directory: &str) -> Result<Vec<(String, bool)>, FolderError>;
}

/// Cartella locale (sincronizzata da Nextcloud, Syncthing, Dropbox…).
pub struct LocalStore {
    root: PathBuf,
}

impl LocalStore {
    pub fn new(folder: &Path) -> Result<Self, FolderError> {
        if !folder.is_dir() {
            return Err(FolderError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "la cartella scelta non esiste",
            )));
        }
        let root = folder.join(ROOT);
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    fn path(&self, relative: &str) -> PathBuf {
        relative
            .split('/')
            .filter(|part| !part.is_empty())
            .fold(self.root.clone(), |path, part| path.join(part))
    }
}

/// Scrittura atomica: file temporaneo nascosto nella stessa cartella, poi
/// rinomina (i lettori ignorano i nomi non attesi).
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let directory = path
        .parent()
        .ok_or_else(|| std::io::Error::other("percorso senza cartella"))?;
    fs::create_dir_all(directory)?;
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let temporary = directory.join(format!(".{name}.{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut file = fs::File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

impl SyncStore for LocalStore {
    fn read(&self, path: &str) -> Result<Option<Vec<u8>>, FolderError> {
        match fs::read(self.path(path)) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn write(&self, path: &str, bytes: &[u8], create_only: bool) -> Result<(), FolderError> {
        let target = self.path(path);
        if create_only && target.exists() {
            return Err(FolderError::Exists);
        }
        write_atomic(&target, bytes)?;
        Ok(())
    }

    fn exists(&self, path: &str) -> Result<bool, FolderError> {
        Ok(self.path(path).exists())
    }

    fn list(&self, directory: &str) -> Result<Vec<(String, bool)>, FolderError> {
        let Ok(entries) = fs::read_dir(self.path(directory)) else {
            return Ok(Vec::new());
        };
        let mut items = Vec::new();
        for entry in entries {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                items.push((name.to_owned(), entry.file_type()?.is_dir()));
            }
        }
        Ok(items)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FormatFile {
    pub format: u32,
    pub library: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceFile {
    pub device: String,
    pub name: String,
    pub updated_at: String,
}

/// Versione di un'entità: (contatore di Lamport, dispositivo).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct Version(pub i64, pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Operation {
    pub entity: String,
    pub key: String,
    pub version: Version,
    pub base: Option<Version>,
    /// Istantanea della riga, `null` per un'eliminazione definitiva.
    pub data: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Batch {
    pub format: u32,
    pub library: String,
    pub device: String,
    pub seq: u64,
    /// Ultimo lotto di ogni altro dispositivo già applicato da chi scrive.
    pub deps: BTreeMap<String, u64>,
    pub created_at: String,
    pub ops: Vec<Operation>,
}

/// Libreria sopra un archivio di file.
pub(crate) struct SyncFolder {
    store: Box<dyn SyncStore>,
}

/// Id di dispositivo o libreria ammessi come nome di cartella.
pub(crate) fn valid_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok()
}

/// Usata solo se `format.json` è comparso tra la lettura e la scrittura:
/// a quel punto la libreria esiste e viene letta, non creata.
fn library_created_elsewhere() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn batch_name(seq: u64) -> String {
    format!("{seq:012}.json")
}

fn parse_batch_name(name: &str) -> Option<u64> {
    name.strip_suffix(".json")
        .filter(|digits| digits.len() == 12 && digits.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|digits| digits.parse().ok())
}

fn changes_dir(device: &str) -> String {
    format!("devices/{device}/changes")
}

fn blob_path(sha256: &str) -> Option<String> {
    (sha256.len() == 64
        && sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()))
    .then(|| format!("blobs/{}/{sha256}", &sha256[..2]))
}

impl SyncFolder {
    /// Apre (o crea) la libreria nell'archivio.
    pub(crate) fn open(
        store: Box<dyn SyncStore>,
        create_library: impl FnOnce() -> String,
    ) -> Result<(Self, String), FolderError> {
        let library = match store.read("format.json")? {
            Some(bytes) => {
                let format: FormatFile = serde_json::from_slice(&bytes)
                    .map_err(|error| FolderError::Invalid(format!("format.json: {error}")))?;
                if format.format > FORMAT {
                    return Err(FolderError::NewerFormat(format.format));
                }
                if !valid_id(&format.library) {
                    return Err(FolderError::Invalid(
                        "format.json: libreria non valida".to_owned(),
                    ));
                }
                format.library
            }
            None => {
                let library = create_library();
                let bytes = serde_json::to_vec_pretty(&FormatFile {
                    format: FORMAT,
                    library: library.clone(),
                })
                .map_err(|error| FolderError::Invalid(error.to_string()))?;
                match store.write("format.json", &bytes, true) {
                    Ok(()) => library,
                    // Creata nel frattempo da un altro dispositivo: vale quella.
                    Err(FolderError::Exists) => {
                        return Self::open(store, library_created_elsewhere);
                    }
                    Err(error) => return Err(error),
                }
            }
        };
        Ok((Self { store }, library))
    }

    pub(crate) fn write_device(&self, file: &DeviceFile) -> Result<(), FolderError> {
        let bytes = serde_json::to_vec_pretty(file)
            .map_err(|error| FolderError::Invalid(error.to_string()))?;
        self.store.write(
            &format!("devices/{}/device.json", file.device),
            &bytes,
            false,
        )
    }

    pub(crate) fn read_device(&self, device: &str) -> Option<DeviceFile> {
        let bytes = self
            .store
            .read(&format!("devices/{device}/device.json"))
            .ok()??;
        serde_json::from_slice(&bytes).ok()
    }

    /// Dispositivi presenti (solo nomi validi).
    pub(crate) fn devices(&self) -> Result<Vec<String>, FolderError> {
        let mut devices: Vec<String> = self
            .store
            .list("devices")?
            .into_iter()
            .filter(|(name, is_dir)| *is_dir && valid_id(name))
            .map(|(name, _)| name)
            .collect();
        devices.sort();
        Ok(devices)
    }

    /// Numero di sequenza più alto presente per un dispositivo.
    pub(crate) fn last_seq(&self, device: &str) -> Result<u64, FolderError> {
        Ok(self
            .store
            .list(&changes_dir(device))?
            .into_iter()
            .filter(|(_, is_dir)| !is_dir)
            .filter_map(|(name, _)| parse_batch_name(&name))
            .max()
            .unwrap_or(0))
    }

    pub(crate) fn write_batch(&self, batch: &Batch) -> Result<(), FolderError> {
        let bytes =
            serde_json::to_vec(batch).map_err(|error| FolderError::Invalid(error.to_string()))?;
        let path = format!("{}/{}", changes_dir(&batch.device), batch_name(batch.seq));
        match self.store.write(&path, &bytes, true) {
            Err(FolderError::Exists) => Err(FolderError::Invalid(format!(
                "il lotto {} esiste già",
                batch.seq
            ))),
            other => other,
        }
    }

    /// Lotto `seq` di un dispositivo: `Ok(None)` se non è (ancora) arrivato
    /// o è incompleto, errore se è presente ma non valido.
    pub(crate) fn read_batch(&self, device: &str, seq: u64) -> Result<Option<Batch>, FolderError> {
        let Some(bytes) =
            self.store
                .read(&format!("{}/{}", changes_dir(device), batch_name(seq)))?
        else {
            return Ok(None);
        };
        if bytes.len() > MAX_BATCH_BYTES {
            return Err(FolderError::Invalid(format!("lotto {seq} troppo grande")));
        }
        match serde_json::from_slice::<Batch>(&bytes) {
            Ok(batch) if batch.device == device && batch.seq == seq => Ok(Some(batch)),
            Ok(_) => Err(FolderError::Invalid(format!(
                "lotto {seq} con intestazione incoerente"
            ))),
            // Probabilmente ancora in trasferimento: si riprova più tardi.
            Err(error) if error.is_eof() => Ok(None),
            Err(error) => Err(FolderError::Invalid(format!("lotto {seq}: {error}"))),
        }
    }

    /// Copia un allegato nei blob se non c'è già.
    pub(crate) fn put_blob(&self, sha256: &str, source: &Path) -> Result<(), FolderError> {
        let path =
            blob_path(sha256).ok_or_else(|| FolderError::Invalid("hash non valido".to_owned()))?;
        if self.store.exists(&path)? {
            return Ok(());
        }
        let bytes = fs::read(source)?;
        if format!("{:x}", Sha256::digest(&bytes)) != sha256 {
            return Err(FolderError::Invalid(
                "un allegato locale non corrisponde al suo hash".to_owned(),
            ));
        }
        match self.store.write(&path, &bytes, true) {
            // Stesso hash, stesso contenuto: scritto da un altro dispositivo.
            Ok(()) | Err(FolderError::Exists) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Contenuto di un blob verificato con il suo hash; `None` se manca o è
    /// incompleto.
    pub(crate) fn get_blob(&self, sha256: &str) -> Option<Vec<u8>> {
        let bytes = self.store.read(&blob_path(sha256)?).ok()??;
        (format!("{:x}", Sha256::digest(&bytes)) == sha256).then_some(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_local(
        path: &Path,
        library: impl FnOnce() -> String,
    ) -> Result<(SyncFolder, String), FolderError> {
        SyncFolder::open(Box::new(LocalStore::new(path)?), library)
    }

    #[test]
    fn layout_batches_and_blobs() {
        let directory = tempfile::tempdir().expect("dir");
        let library = uuid::Uuid::new_v4().to_string();
        let (folder, opened) = open_local(directory.path(), || library.clone()).expect("open");
        assert_eq!(opened, library);
        // Riaperta: stessa libreria, la funzione di creazione non serve.
        let (_, again) = open_local(directory.path(), || unreachable!()).expect("reopen");
        assert_eq!(again, library);

        let device = uuid::Uuid::new_v4().to_string();
        assert_eq!(folder.last_seq(&device).expect("seq"), 0);
        let batch = Batch {
            format: FORMAT,
            library: library.clone(),
            device: device.clone(),
            seq: 1,
            deps: BTreeMap::new(),
            created_at: "2026-10-20T10:00:00Z".to_owned(),
            ops: vec![Operation {
                entity: "notes".to_owned(),
                key: "n".to_owned(),
                version: Version(1, device.clone()),
                base: None,
                data: None,
            }],
        };
        folder.write_batch(&batch).expect("write");
        assert!(
            folder.write_batch(&batch).is_err(),
            "i lotti sono immutabili"
        );
        assert_eq!(folder.last_seq(&device).expect("seq"), 1);
        assert_eq!(folder.read_batch(&device, 1).expect("read"), Some(batch));
        assert_eq!(folder.read_batch(&device, 2).expect("read"), None);
        assert_eq!(folder.devices().expect("devices"), vec![device.clone()]);

        // File troncato (ancora in trasferimento): non disponibile, non errore.
        let partial = directory
            .path()
            .join(ROOT)
            .join("devices")
            .join(&device)
            .join("changes")
            .join(batch_name(2));
        fs::write(&partial, br#"{"format":1,"library":"#).expect("partial");
        assert_eq!(folder.read_batch(&device, 2).expect("read"), None);
        fs::write(&partial, b"non json").expect("garbage");
        assert!(folder.read_batch(&device, 2).is_err());

        let source = directory.path().join("allegato.txt");
        fs::write(&source, b"ciao").expect("source");
        let hash = format!("{:x}", Sha256::digest(b"ciao"));
        folder.put_blob(&hash, &source).expect("blob");
        folder.put_blob(&hash, &source).expect("blob già presente");
        assert_eq!(folder.get_blob(&hash).as_deref(), Some(&b"ciao"[..]));
        assert!(folder.put_blob(&"0".repeat(64), &source).is_err());
        assert!(folder.get_blob("../../etc/passwd").is_none());
        // Nessun file temporaneo rimasto.
        let leftovers = fs::read_dir(directory.path().join(ROOT).join("blobs").join(&hash[..2]))
            .expect("dir")
            .filter(|entry| {
                entry
                    .as_ref()
                    .is_ok_and(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
            })
            .count();
        assert_eq!(leftovers, 0);

        fs::write(
            directory.path().join(ROOT).join("format.json"),
            br#"{"format":99,"library":"x"}"#,
        )
        .expect("format");
        assert!(matches!(
            open_local(directory.path(), String::new),
            Err(FolderError::NewerFormat(99))
        ));
        assert!(LocalStore::new(&directory.path().join("non-esiste")).is_err());
    }
}
