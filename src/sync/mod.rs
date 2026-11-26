//! Sincronizzazione tra dispositivi tramite una cartella condivisa
//! (ADR 0017): registro delle modifiche locali con trigger, lotti immutabili
//! per dispositivo, orologi di Lamport, consegna causale e conflitti risolti
//! in modo deterministico conservando la versione perdente.

pub mod crypto;
pub mod dropbox;
pub mod folder;
pub(crate) mod tables;

use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use self::{
    folder::{
        Batch, DeviceFile, FORMAT, FolderError, LocalStore, Operation, SyncAccess, SyncFolder,
        SyncStore, Version,
    },
    tables::{Applied, SyncTable, TABLES},
};
use crate::storage::{
    Database, StorageError, attachment_path, sync_search_index, write_attachment_file,
};

const SETTINGS_KEY: &str = "sync";
/// Operazioni per lotto e dimensione indicativa massima.
const MAX_BATCH_OPS: usize = 1000;
const MAX_BATCH_BYTES: usize = 8 * 1024 * 1024;
const MAX_CONFLICTS_LISTED: usize = 200;
const MAX_CHANGED_NOTES: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SyncSettings {
    pub enabled: bool,
    /// Dove passano i dati: cartella locale sincronizzata o Dropbox.
    pub transport: SyncTransport,
    /// Cartella sincronizzata scelta dall'utente (percorso assoluto).
    pub folder: String,
    /// Account Dropbox collegato (solo per mostrarlo; l'accesso è nel
    /// portachiavi).
    pub dropbox_account: String,
    /// Nome di questo dispositivo, mostrato agli altri.
    pub device_name: String,
    /// Cifrare la libreria con una passphrase (ADR 0032): una libreria
    /// nuova nasce cifrata, una in chiaro viene convertita.
    pub encrypted: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncTransport {
    #[default]
    Folder,
    Dropbox,
}

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("{}", crate::tr!("La sincronizzazione non è attiva", "Sync is not active"))]
    NotConfigured,
    #[error("{0}")]
    Folder(#[from] FolderError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(
        "{}", crate::tr!("Nella cartella ci sono modifiche di questo dispositivo che il database non conosce: è stato ripristinato un backup meno recente oppure lo stesso database è stato copiato su un altro computer. Per sicurezza la sincronizzazione è ferma: disattivala e riattivala per dare a questo computer una nuova identità.", "The folder contains changes from this device that the database does not know about: an older backup was restored or the same database was copied to another computer. For safety, sync is paused: disable and re-enable it to give this computer a new identity.")
    )]
    DuplicateDevice,
    #[error(
        "{}", crate::tr!("La cartella contiene un'altra libreria di Greatnote: scegli di nuovo la cartella nelle impostazioni", "The folder contains another Greatnote library: choose the folder again in settings")
    )]
    OtherLibrary,
}

impl SyncError {
    /// Le credenziali del servizio remoto non valgono più.
    pub fn is_unauthorized(&self) -> bool {
        matches!(self, Self::Folder(FolderError::Unauthorized(_)))
    }
}

impl From<rusqlite::Error> for SyncError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Storage(error.into())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncReport {
    pub exported: usize,
    pub imported: usize,
    pub conflicts: usize,
    /// Operazioni remote scartate perché non valide.
    pub rejected: usize,
    /// Lotti in attesa di altri lotti non ancora arrivati nella cartella.
    pub waiting: bool,
    /// Note cambiate da altri dispositivi (per aggiornare l'interfaccia).
    pub changed_notes: Vec<String>,
    /// Avvisi su dispositivi con file non validi.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncDevice {
    pub device_id: String,
    pub name: String,
    pub applied_seq: i64,
    pub last_seen_at: Option<String>,
    pub this_device: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncConflict {
    pub id: String,
    pub entity: String,
    pub entity_key: String,
    pub title: String,
    pub from_device: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SyncStatus {
    pub settings: SyncSettings,
    pub device_id: Option<String>,
    pub last_sync_at: Option<String>,
    pub last_error: Option<String>,
    pub pending_changes: i64,
    pub missing_attachments: i64,
    /// Dispositivi della libreria, senza quelli tolti dall'elenco.
    pub devices: Vec<SyncDevice>,
    pub conflicts: Vec<SyncConflict>,
    /// Nome usato quando `settings.device_name` è vuoto (nome host).
    pub default_device_name: String,
    /// La libreria in uso è cifrata.
    pub encrypted: bool,
    /// L'ultima sincronizzazione si è fermata perché serve la passphrase.
    pub passphrase_required: bool,
}

// ---------------------------------------------------------------------------
// Stato

fn state(connection: &Connection, key: &str) -> Result<Option<String>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT value FROM sync_state WHERE key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()
}

fn set_state(connection: &Connection, key: &str, value: &str) -> Result<(), rusqlite::Error> {
    connection.execute(
        "INSERT INTO sync_state (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn state_number(connection: &Connection, key: &str) -> Result<i64, rusqlite::Error> {
    Ok(state(connection, key)?
        .and_then(|value| value.parse().ok())
        .unwrap_or(0))
}

fn version_of(
    connection: &Connection,
    entity: &str,
    key: &str,
) -> Result<Option<Version>, rusqlite::Error> {
    connection
        .query_row(
            "SELECT counter, device FROM sync_versions WHERE entity = ?1 AND entity_key = ?2",
            params![entity, key],
            |row| Ok(Version(row.get(0)?, row.get(1)?)),
        )
        .optional()
}

fn set_version(
    connection: &Connection,
    entity: &str,
    key: &str,
    version: &Version,
) -> Result<(), rusqlite::Error> {
    connection.execute(
        "INSERT INTO sync_versions (entity, entity_key, counter, device) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(entity, entity_key) DO UPDATE SET counter = excluded.counter, device = excluded.device",
        params![entity, key, version.0, version.1],
    )?;
    Ok(())
}

/// Chiave di stato con l'ultimo lotto di `device` già presente quando questo
/// dispositivo è entrato nella libreria.
fn history_key(device: &str) -> String {
    format!("history:{device}")
}

/// Chiave di stato con i dispositivi tolti dall'elenco (identificativo →
/// istante della scelta), propri e ricevuti dagli altri dispositivi.
const RETIRED_KEY: &str = "retired";

fn retired_devices(connection: &Connection) -> Result<BTreeMap<String, String>, rusqlite::Error> {
    Ok(state(connection, RETIRED_KEY)?
        .and_then(|value| serde_json::from_str(&value).ok())
        .unwrap_or_default())
}

fn set_retired_devices(
    connection: &Connection,
    retired: &BTreeMap<String, String>,
) -> Result<(), rusqlite::Error> {
    let value = serde_json::to_string(retired).unwrap_or_else(|_| "{}".to_owned());
    set_state(connection, RETIRED_KEY, &value)
}

/// Adesso, ma almeno un secondo dopo `instant`: gli istanti sono al
/// secondo e un dispositivo che torna dopo essere stato tolto dall'elenco
/// deve risultare attivo *dopo* la rimozione, anche nello stesso secondo.
fn later_than(instant: &str) -> String {
    // Confronto sugli istanti già formattati (al secondo): nello stesso
    // secondo «adesso» è più tardi solo nelle frazioni, che si perdono.
    let now = now_utc();
    if now.as_str() > instant {
        return now;
    }
    crate::time::parse_utc(instant)
        .ok()
        .and_then(|at| at.checked_add(jiff::SignedDuration::from_secs(1)).ok())
        .map(crate::time::format_utc)
        .unwrap_or(now)
}

fn now_utc() -> String {
    crate::time::format_utc(crate::time::now())
}

/// Azzera identità e storia di sincronizzazione (nuova cartella o nuova
/// identità): alla prossima esecuzione tutto viene riesportato.
fn reset_protocol_state(connection: &Connection) -> Result<(), rusqlite::Error> {
    connection.execute_batch(
        "DELETE FROM sync_state;
         DELETE FROM sync_versions;
         DELETE FROM sync_devices;
         DELETE FROM sync_aliases;
         DELETE FROM sync_missing_blobs;
         DELETE FROM sync_outbox;
         DELETE FROM sync_dirty;",
    )
}

impl Database {
    pub fn sync_settings(&self) -> Result<SyncSettings, StorageError> {
        let stored: Option<String> = self
            .connection()?
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                [SETTINGS_KEY],
                |row| row.get(0),
            )
            .optional()?;
        Ok(stored
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default())
    }

    /// Salva le impostazioni. Cambiando cartella (o disattivando) la storia
    /// locale del protocollo viene azzerata: le note restano, e alla prossima
    /// sincronizzazione vengono esportate di nuovo e unite a quelle presenti.
    pub fn set_sync_settings(&self, settings: SyncSettings) -> Result<SyncSettings, StorageError> {
        let folder = settings.folder.trim().to_owned();
        let device_name: String = settings.device_name.trim().chars().take(100).collect();
        let dropbox_account: String = settings.dropbox_account.trim().chars().take(320).collect();
        if settings.enabled
            && settings.transport == SyncTransport::Dropbox
            && dropbox_account.is_empty()
        {
            return Err(StorageError::InvalidEvent(
                "collega prima l'account Dropbox",
            ));
        }
        if settings.enabled && settings.transport == SyncTransport::Folder {
            if folder.is_empty() || !Path::new(&folder).is_absolute() {
                return Err(StorageError::InvalidEvent(crate::tr_str!(
                    "scegli la cartella da usare per la sincronizzazione",
                    "choose the folder to use for sync"
                )));
            }
            if !Path::new(&folder).is_dir() {
                return Err(StorageError::InvalidEvent(crate::tr_str!(
                    "la cartella scelta non esiste",
                    "the chosen folder does not exist"
                )));
            }
        }
        let settings = SyncSettings {
            enabled: settings.enabled,
            transport: settings.transport,
            folder,
            dropbox_account,
            device_name,
            encrypted: settings.encrypted,
        };
        let previous = self.sync_settings()?;
        let value = serde_json::to_string(&settings)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        // Un'altra destinazione è un'altra libreria: storia del protocollo
        // azzerata (le note restano e vengono riesportate e unite).
        let moved = previous.transport != settings.transport
            || match settings.transport {
                SyncTransport::Folder => previous.folder != settings.folder,
                SyncTransport::Dropbox => previous.dropbox_account != settings.dropbox_account,
            };
        if moved || !settings.enabled {
            reset_protocol_state(&transaction)?;
        }
        transaction.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value,
                 updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
            params![SETTINGS_KEY, value],
        )?;
        transaction.commit()?;
        Ok(settings)
    }

    pub fn sync_status(&self) -> Result<SyncStatus, StorageError> {
        let settings = self.sync_settings()?;
        let connection = self.connection()?;
        let device_id = state(&connection, "device_id")?;
        let retired = retired_devices(&connection)?;
        let devices = connection
            .prepare("SELECT device_id, name, applied_seq, last_seen_at FROM sync_devices ORDER BY name, device_id")?
            .query_map([], |row| {
                let id: String = row.get(0)?;
                Ok(SyncDevice {
                    this_device: device_id.as_deref() == Some(id.as_str()),
                    device_id: id,
                    name: row.get(1)?,
                    applied_seq: row.get(2)?,
                    last_seen_at: row.get(3)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|device| device.this_device || !retired.contains_key(&device.device_id))
            .collect();
        let conflicts = connection
            .prepare(&format!(
                "SELECT c.id, c.entity, c.entity_key, c.title,
                        COALESCE((SELECT name FROM sync_devices WHERE device_id = c.from_device), c.from_device),
                        c.created_at
                 FROM sync_conflicts c WHERE c.dismissed_at IS NULL
                 ORDER BY c.created_at DESC LIMIT {MAX_CONFLICTS_LISTED}"
            ))?
            .query_map([], |row| {
                Ok(SyncConflict {
                    id: row.get(0)?,
                    entity: row.get(1)?,
                    entity_key: row.get(2)?,
                    title: row.get(3)?,
                    from_device: row.get(4)?,
                    created_at: row.get(5)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(SyncStatus {
            device_id,
            last_sync_at: state(&connection, "last_sync_at")?,
            last_error: state(&connection, "last_error")?.filter(|error| !error.is_empty()),
            pending_changes: connection.query_row(
                "SELECT COUNT(*) FROM sync_dirty",
                [],
                |row| row.get(0),
            )?,
            missing_attachments: connection.query_row(
                "SELECT COUNT(*) FROM sync_missing_blobs",
                [],
                |row| row.get(0),
            )?,
            devices,
            conflicts,
            settings,
            default_device_name: default_device_name(),
            encrypted: state(&connection, "encrypted")?.as_deref() == Some("1"),
            passphrase_required: state(&connection, "passphrase_required")?.as_deref() == Some("1"),
        })
    }

    /// Toglie un altro dispositivo dall'elenco, su tutti i dispositivi della
    /// libreria (alla prossima sincronizzazione). Nessun dato viene
    /// cancellato: le sue modifiche restano nella libreria e, se torna a
    /// sincronizzare, il dispositivo ricompare.
    pub fn retire_sync_device(&self, device_id: &str) -> Result<(), StorageError> {
        let connection = self.connection()?;
        if state(&connection, "device_id")?.as_deref() == Some(device_id) {
            return Err(StorageError::InvalidEvent(crate::tr_str!(
                "non puoi togliere dall'elenco questo dispositivo",
                "you cannot remove this device from the list"
            )));
        }
        let known: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sync_devices WHERE device_id = ?1)",
            [device_id],
            |row| row.get(0),
        )?;
        if !known {
            return Err(StorageError::InvalidEvent(crate::tr_str!(
                "dispositivo sconosciuto",
                "unknown device"
            )));
        }
        let mut retired = retired_devices(&connection)?;
        retired.insert(device_id.to_owned(), now_utc());
        set_retired_devices(&connection, &retired)?;
        Ok(())
    }

    pub fn dismiss_sync_conflict(&self, id: &str) -> Result<bool, StorageError> {
        let changed = self.connection()?.execute(
            "UPDATE sync_conflicts SET dismissed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
             WHERE id = ?1 AND dismissed_at IS NULL",
            [id],
        )?;
        Ok(changed > 0)
    }

    /// Registra l'esito di una sincronizzazione non riuscita.
    pub fn record_sync_error(&self, error: &str) -> Result<(), StorageError> {
        let text: String = error.chars().take(1000).collect();
        set_state(&*self.connection()?, "last_error", &text)?;
        Ok(())
    }

    /// Sincronizza con la cartella locale delle impostazioni.
    pub fn sync_now(&self) -> Result<SyncReport, SyncError> {
        self.sync_now_with(None, &mut SyncAccess::default())
    }

    /// Esegue una sincronizzazione completa: esporta le modifiche locali e
    /// applica quelle degli altri dispositivi. `store` è l'archivio remoto
    /// (Dropbox); senza, si usa la cartella locale delle impostazioni.
    /// `access` porta chiave o passphrase di una libreria cifrata e, alla
    /// fine, la chiave in uso (da conservare nel portachiavi).
    pub fn sync_now_with(
        &self,
        store: Option<Box<dyn SyncStore>>,
        access: &mut SyncAccess,
    ) -> Result<SyncReport, SyncError> {
        let settings = self.sync_settings()?;
        if !settings.enabled {
            return Err(SyncError::NotConfigured);
        }
        let store = self.sync_store(store, &settings)?;
        access.encrypt = access.encrypt || settings.encrypted;
        let data_directory = self.data_directory()?.to_path_buf();
        let result = run(self, store, &settings, &data_directory, access);
        let connection = self.connection()?;
        let needs_passphrase = matches!(
            result,
            Err(SyncError::Folder(FolderError::PassphraseRequired))
        );
        set_state(
            &connection,
            "passphrase_required",
            if needs_passphrase { "1" } else { "" },
        )?;
        match &result {
            Ok(_) => {
                set_state(&connection, "last_sync_at", &now_utc())?;
                set_state(&connection, "last_error", "")?;
            }
            Err(error) => set_state(
                &connection,
                "last_error",
                &error.to_string().chars().take(1000).collect::<String>(),
            )?,
        }
        result
    }

    fn sync_store(
        &self,
        store: Option<Box<dyn SyncStore>>,
        settings: &SyncSettings,
    ) -> Result<Box<dyn SyncStore>, SyncError> {
        Ok(match (store, settings.transport) {
            (Some(store), _) => store,
            (None, SyncTransport::Folder) => {
                Box::new(LocalStore::new(Path::new(&settings.folder))?)
            }
            (None, SyncTransport::Dropbox) => return Err(SyncError::NotConfigured),
        })
    }

    /// Cambia la passphrase della libreria cifrata in uso. Serve la chiave
    /// (dal portachiavi) o la passphrase attuale; la chiave della libreria
    /// non cambia, quindi gli altri dispositivi continuano senza fare nulla.
    pub fn change_sync_passphrase(
        &self,
        store: Option<Box<dyn SyncStore>>,
        access: &mut SyncAccess,
        new_passphrase: &str,
    ) -> Result<(), SyncError> {
        let settings = self.sync_settings()?;
        if !settings.enabled {
            return Err(SyncError::NotConfigured);
        }
        crypto::check_passphrase(new_passphrase).map_err(FolderError::from)?;
        let store = self.sync_store(store, &settings)?;
        access.encrypt = false;
        let known = state(&*self.connection()?, "library_id")?;
        let opened = SyncFolder::open(store, || Uuid::new_v4().to_string(), access)?;
        if known.as_deref() != Some(opened.library.as_str()) {
            return Err(SyncError::OtherLibrary);
        }
        opened.folder.change_passphrase(new_passphrase)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Esecuzione

/// Lotti per dispositivo letti in anticipo a ogni giro (i successivi
/// arrivano al giro dopo).
const MAX_PREFETCH: usize = 100;

/// Una sincronizzazione in tre fasi, perché la rete (Dropbox) può essere
/// lenta: lettura dei lotti remoti senza bloccare il database, poi un breve
/// blocco per esportare le modifiche locali nella coda di uscita e applicare
/// quelle remote, infine invio di allegati e lotti senza blocco.
fn run(
    database: &Database,
    store: Box<dyn SyncStore>,
    settings: &SyncSettings,
    data_directory: &Path,
    access: &mut SyncAccess,
) -> Result<SyncReport, SyncError> {
    let known_library = state(&*database.connection()?, "library_id")?;
    let opened = SyncFolder::open(
        store,
        || {
            known_library
                .clone()
                .unwrap_or_else(|| Uuid::new_v4().to_string())
        },
        access,
    )?;
    let (folder, library) = (opened.folder, opened.library);
    match &known_library {
        // Libreria convertita in cifrata (da qui o da un altro dispositivo):
        // si ricomincia nella nuova riesportando tutto, e le note vengono
        // unite a quelle degli altri dispositivi.
        Some(known) if *known != library && opened.previous_library.as_ref() == Some(known) => {
            let mut connection = database.connection()?;
            let transaction = connection.transaction()?;
            reset_protocol_state(&transaction)?;
            set_state(&transaction, "library_id", &library)?;
            transaction.commit()?;
            tracing::info!("sincronizzazione passata alla libreria cifrata");
        }
        Some(known) if *known != library => return Err(SyncError::OtherLibrary),
        Some(_) => {}
        None => set_state(&*database.connection()?, "library_id", &library)?,
    }
    let (device, seq) = {
        let connection = database.connection()?;
        set_state(
            &connection,
            "encrypted",
            if folder.is_encrypted() { "1" } else { "" },
        )?;
        let device = match state(&connection, "device_id")? {
            Some(device) => device,
            None => {
                let device = Uuid::new_v4().to_string();
                set_state(&connection, "device_id", &device)?;
                device
            }
        };
        let seq = u64::try_from(state_number(&connection, "seq")?).unwrap_or(0);
        (device, seq)
    };

    // Ogni lotto è registrato nel database prima di essere scritto: un lotto
    // nella cartella che il database non conosce significa che un altro
    // computer usa la stessa identità (database copiato).
    if folder.last_seq(&device)? > seq {
        return Err(SyncError::DuplicateDevice);
    }
    // Lotti rimasti da un giro interrotto.
    flush_outbox(database, &folder, data_directory)?;

    let name = if settings.device_name.is_empty() {
        default_device_name()
    } else {
        settings.device_name.clone()
    };
    if folder
        .read_device(&device)
        .is_none_or(|file| file.name != name)
    {
        folder.write_device(&DeviceFile {
            device: device.clone(),
            name: name.clone(),
            updated_at: now_utc(),
        })?;
    }

    reconcile_retired(database, &folder, &device, &name)?;

    let mut report = SyncReport::default();
    // Prima attivazione: si annota fin dove arrivava la storia di ogni altro
    // dispositivo. Letto dall'archivio prima di prendere il database.
    let initialized = state(&*database.connection()?, "initialized")?.is_some();
    let mut existing_history: Vec<(String, u64)> = Vec::new();
    if !initialized {
        for remote in folder.devices()? {
            if remote != device {
                let last = folder.last_seq(&remote)?;
                existing_history.push((remote, last));
            }
        }
    }
    let prefetched = prefetch(database, &folder, &library, &device, &mut report)?;

    {
        let mut connection = database.connection()?;
        if state(&connection, "initialized")?.is_none() {
            // Prima attivazione: tutto ciò che esiste va esportato.
            let transaction = connection.transaction()?;
            for table in TABLES {
                for key in tables::all_keys(&transaction, table)? {
                    transaction.execute(
                        "INSERT OR IGNORE INTO sync_dirty (entity, entity_key) VALUES (?1, ?2)",
                        params![table.name, key],
                    )?;
                }
            }
            for (remote, last) in &existing_history {
                set_state(&transaction, &history_key(remote), &last.to_string())?;
            }
            set_state(&transaction, "initialized", "1")?;
            transaction.commit()?;
        }
        connection.execute(
            "INSERT INTO sync_devices (device_id, name, applied_seq, last_seen_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(device_id) DO UPDATE SET name = excluded.name, last_seen_at = excluded.last_seen_at",
            params![device, name, seq as i64, now_utc()],
        )?;
        // Esportazione e importazione nello stesso blocco: nessuna modifica
        // locale può inserirsi tra le due.
        export(&mut connection, &library, &device, &mut report)?;
        import(
            &mut connection,
            prefetched,
            &device,
            data_directory,
            &mut report,
        )?;
    }

    flush_outbox(database, &folder, data_directory)?;
    fetch_missing_blobs(database, &folder, data_directory)?;
    Ok(report)
}

/// Nome di questo dispositivo quando l'utente non ne sceglie uno: il nome
/// host del computer, il nome commerciale del telefono su Android.
pub fn default_device_name() -> String {
    let name = if cfg!(target_os = "android") {
        android_device_name()
    } else {
        host_name()
    };
    name.map(|name| name.trim().chars().take(100).collect::<String>())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| {
            if cfg!(target_os = "android") {
                "Android".to_owned()
            } else {
                "Computer".to_owned()
            }
        })
}

/// Windows: `COMPUTERNAME`. Linux: `HOSTNAME` della shell, poi il nome host
/// del kernel (presente anche dove `/etc/hostname` manca).
fn host_name() -> Option<String> {
    let clean = |value: String| Some(value.trim().to_owned()).filter(|value| !value.is_empty());
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .find_map(|key| std::env::var(key).ok().and_then(clean))
        .or_else(|| {
            ["/proc/sys/kernel/hostname", "/etc/hostname"]
                .iter()
                .find_map(|path| fs::read_to_string(path).ok().and_then(clean))
        })
}

/// Nome commerciale («Pixel 7 Pro»), altrimenti il modello. Letto dalle
/// proprietà di sistema con `getprop`, disponibile a ogni app.
fn android_device_name() -> Option<String> {
    if !cfg!(target_os = "android") {
        return None;
    }
    ["ro.product.marketname", "ro.product.model"]
        .iter()
        .find_map(|property| {
            std::process::Command::new("getprop")
                .arg(property)
                .output()
                .ok()
                .filter(|output| output.status.success())
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
                .filter(|value| !value.is_empty())
        })
}

/// Allinea l'elenco dei dispositivi tolti: unisce le scelte degli altri
/// dispositivi alle proprie, scarta quelle superate (il dispositivo ha
/// sincronizzato dopo essere stato tolto) e pubblica il risultato nel
/// proprio `retired.json`. Se questo dispositivo era stato tolto, ripubblica
/// il proprio file così gli altri lo vedono di nuovo.
fn reconcile_retired(
    database: &Database,
    folder: &SyncFolder,
    device: &str,
    name: &str,
) -> Result<(), SyncError> {
    let mut retired = retired_devices(&*database.connection()?)?;
    for remote in folder.devices()? {
        if remote == device {
            continue;
        }
        for (id, at) in folder.read_retired(&remote) {
            let newer = retired.get(&id).is_none_or(|known| at > *known);
            if newer {
                retired.insert(id, at);
            }
        }
    }
    if let Some(at) = retired.remove(device) {
        let stale = folder
            .read_device(device)
            .is_none_or(|file| file.updated_at <= at);
        if stale {
            folder.write_device(&DeviceFile {
                device: device.to_owned(),
                name: name.to_owned(),
                updated_at: later_than(&at),
            })?;
        }
    }
    {
        let connection = database.connection()?;
        let mut active = Vec::new();
        for (id, at) in &retired {
            let seen: Option<String> = connection
                .query_row(
                    "SELECT last_seen_at FROM sync_devices WHERE device_id = ?1",
                    [id],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            let updated = folder.read_device(id).map(|file| file.updated_at);
            if seen.is_some_and(|seen| seen > *at) || updated.is_some_and(|updated| updated > *at) {
                active.push(id.clone());
            }
        }
        for id in active {
            retired.remove(&id);
        }
        set_retired_devices(&connection, &retired)?;
    }
    if folder.read_retired(device) != retired {
        folder.write_retired(device, &retired)?;
    }
    Ok(())
}

/// Lotti remoti disponibili oltre quelli già applicati, letti senza
/// bloccare il database. Con il nome di ogni dispositivo.
struct Prefetched {
    batches: BTreeMap<String, Vec<Batch>>,
    names: BTreeMap<String, String>,
}

fn prefetch(
    database: &Database,
    folder: &SyncFolder,
    library: &str,
    device: &str,
    report: &mut SyncReport,
) -> Result<Prefetched, SyncError> {
    let applied: BTreeMap<String, i64> = database
        .connection()?
        .prepare("SELECT device_id, applied_seq FROM sync_devices")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut prefetched = Prefetched {
        batches: BTreeMap::new(),
        names: BTreeMap::new(),
    };
    for remote in folder.devices()? {
        if remote == device {
            continue;
        }
        let name = folder
            .read_device(&remote)
            .map(|file| file.name)
            .unwrap_or_default();
        let label = if name.is_empty() {
            remote.clone()
        } else {
            name.clone()
        };
        prefetched.names.insert(remote.clone(), name);
        let mut next = applied.get(&remote).copied().unwrap_or(0) as u64 + 1;
        let mut batches = Vec::new();
        while batches.len() < MAX_PREFETCH {
            match folder.read_batch(&remote, next) {
                Ok(Some(batch)) if batch.library == library && batch.format <= FORMAT => {
                    batches.push(batch)
                }
                Ok(Some(_)) => {
                    report.warnings.push(crate::tr!(
                        "{label}: lotto di un'altra libreria o versione",
                        "{label}: batch from another library or version"
                    ));
                    break;
                }
                Ok(None) => break,
                Err(error) => {
                    report.warnings.push(format!("{label}: {error}"));
                    break;
                }
            }
            next += 1;
        }
        if !batches.is_empty() {
            prefetched.batches.insert(remote, batches);
        }
    }
    Ok(prefetched)
}

fn export(
    connection: &mut Connection,
    library: &str,
    device: &str,
    report: &mut SyncReport,
) -> Result<(), SyncError> {
    let transaction = connection.transaction()?;
    let dirty: Vec<(String, String)> = transaction
        .prepare("SELECT entity, entity_key FROM sync_dirty")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    if dirty.is_empty() {
        return Ok(());
    }
    let mut lamport = state_number(&transaction, "lamport")?;
    let mut ops = Vec::new();
    // In ordine di dipendenza, così un lotto non riferisce entità successive.
    for table in TABLES {
        for (_, key) in dirty.iter().filter(|(entity, _)| entity == table.name) {
            let data = tables::snapshot(&transaction, table, key)?;
            let base = version_of(&transaction, table.name, key)?;
            if data.is_none() && base.is_none() {
                // Creata ed eliminata prima di essere mai esportata.
                continue;
            }
            lamport += 1;
            let version = Version(lamport, device.to_owned());
            set_version(&transaction, table.name, key, &version)?;
            ops.push(Operation {
                entity: table.name.to_owned(),
                key: key.clone(),
                version,
                base,
                data,
            });
        }
    }
    let deps: BTreeMap<String, u64> = transaction
        .prepare("SELECT device_id, applied_seq FROM sync_devices WHERE device_id <> ?1")?
        .query_map([device], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .filter_map(Result::ok)
        .filter(|(_, seq)| *seq > 0)
        .map(|(id, seq)| (id, seq as u64))
        .collect();
    let mut seq = u64::try_from(state_number(&transaction, "seq")?).unwrap_or(0);
    let mut chunk: Vec<Operation> = Vec::new();
    let mut chunk_bytes = 0;
    let total = ops.len();
    let flush = |chunk: &mut Vec<Operation>, seq: &mut u64| -> Result<(), SyncError> {
        if chunk.is_empty() {
            return Ok(());
        }
        *seq += 1;
        let batch = Batch {
            format: FORMAT,
            library: library.to_owned(),
            device: device.to_owned(),
            seq: *seq,
            deps: deps.clone(),
            created_at: now_utc(),
            ops: std::mem::take(chunk),
        };
        let json = serde_json::to_string(&batch).map_err(StorageError::from)?;
        transaction.execute(
            "INSERT INTO sync_outbox (seq, batch_json) VALUES (?1, ?2)",
            params![*seq as i64, json],
        )?;
        Ok(())
    };
    for op in ops {
        let size = op
            .data
            .as_ref()
            .map_or(64, |data| data.to_string().len() + 128);
        if !chunk.is_empty()
            && (chunk.len() >= MAX_BATCH_OPS || chunk_bytes + size > MAX_BATCH_BYTES)
        {
            flush(&mut chunk, &mut seq)?;
            chunk_bytes = 0;
        }
        chunk_bytes += size;
        chunk.push(op);
    }
    flush(&mut chunk, &mut seq)?;
    transaction.execute("DELETE FROM sync_dirty", [])?;
    set_state(&transaction, "lamport", &lamport.to_string())?;
    set_state(&transaction, "seq", &seq.to_string())?;
    transaction.execute(
        "UPDATE sync_devices SET applied_seq = ?2 WHERE device_id = ?1",
        params![device, seq as i64],
    )?;
    transaction.commit()?;
    report.exported = total;
    Ok(())
}

/// Invia i lotti registrati e non ancora scritti, senza bloccare il
/// database durante la rete. Prima di ogni lotto vengono caricati gli
/// allegati che riferisce: un'interruzione non lascia mai un lotto senza i
/// suoi blob.
fn flush_outbox(
    database: &Database,
    folder: &SyncFolder,
    data_directory: &Path,
) -> Result<(), SyncError> {
    let pending: Vec<(i64, String)> = database
        .connection()?
        .prepare("SELECT seq, batch_json FROM sync_outbox ORDER BY seq")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (seq, json) in pending {
        let batch: Batch = serde_json::from_str(&json).map_err(StorageError::from)?;
        for op in batch.ops.iter().filter(|op| op.entity == "attachments") {
            if let Some(row) = &op.data
                && row["deleted_at"].is_null()
            {
                upload_blob(folder, data_directory, row)?;
            }
        }
        // Già scritto prima di un'interruzione: è lo stesso contenuto.
        if folder.read_batch(&batch.device, batch.seq)?.is_none() {
            folder.write_batch(&batch)?;
        }
        database
            .connection()?
            .execute("DELETE FROM sync_outbox WHERE seq = ?1", [seq])?;
    }
    Ok(())
}

fn upload_blob(folder: &SyncFolder, data_directory: &Path, row: &Value) -> Result<(), SyncError> {
    let (Some(sha256), Some(stored)) = (row["sha256"].as_str(), row["stored_filename"].as_str())
    else {
        return Ok(());
    };
    match attachment_path(data_directory, stored) {
        Ok(path) if path.is_file() => match folder.put_blob(sha256, &path) {
            Ok(()) => Ok(()),
            // Errori di rete: il lotto resta in coda e si riprova.
            Err(FolderError::Remote(error)) => Err(SyncError::Folder(FolderError::Remote(error))),
            Err(FolderError::Io(error)) => Err(SyncError::Folder(FolderError::Io(error))),
            Err(FolderError::Unauthorized(service)) => {
                Err(SyncError::Folder(FolderError::Unauthorized(service)))
            }
            Err(error) => {
                tracing::warn!(%error, "allegato non caricato");
                Ok(())
            }
        },
        // Il file manca anche qui (es. a sua volta in attesa): lo porterà un
        // altro dispositivo.
        _ => Ok(()),
    }
}

fn import(
    connection: &mut Connection,
    mut prefetched: Prefetched,
    device: &str,
    data_directory: &Path,
    report: &mut SyncReport,
) -> Result<(), SyncError> {
    let mut changed_notes: HashSet<String> = HashSet::new();
    let applied_seq = |connection: &Connection, id: &str| -> Result<u64, rusqlite::Error> {
        Ok(connection
            .query_row(
                "SELECT applied_seq FROM sync_devices WHERE device_id = ?1",
                [id],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
            .unwrap_or(0) as u64)
    };
    loop {
        let mut progress = false;
        let remotes: Vec<String> = prefetched.batches.keys().cloned().collect();
        for remote in remotes {
            let Some(batch) = prefetched
                .batches
                .get(&remote)
                .and_then(|batches| batches.first())
            else {
                continue;
            };
            if batch.seq != applied_seq(connection, &remote)? + 1 {
                // Già applicato (o buco): si scarta dalla lista anticipata.
                if batch.seq <= applied_seq(connection, &remote)? {
                    prefetched
                        .batches
                        .get_mut(&remote)
                        .map(|batches| batches.remove(0));
                }
                continue;
            }
            // Consegna causale: servono tutti i lotti che l'autore aveva già.
            let mut ready = true;
            for (dependency, needed) in &batch.deps {
                if dependency != device && applied_seq(connection, dependency)? < *needed {
                    ready = false;
                }
            }
            if !ready {
                continue;
            }
            let Some(batch) = prefetched
                .batches
                .get_mut(&remote)
                .filter(|batches| !batches.is_empty())
                .map(|batches| batches.remove(0))
            else {
                continue;
            };
            apply_batch(
                connection,
                &batch,
                data_directory,
                report,
                &mut changed_notes,
            )?;
            let name = prefetched.names.get(&remote).cloned().unwrap_or_default();
            connection.execute(
                "INSERT INTO sync_devices (device_id, name, applied_seq, last_seen_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(device_id) DO UPDATE SET name = CASE WHEN excluded.name = '' THEN sync_devices.name
                     ELSE excluded.name END, applied_seq = excluded.applied_seq, last_seen_at = excluded.last_seen_at",
                params![remote, name, batch.seq as i64, batch.created_at],
            )?;
            progress = true;
        }
        if !progress {
            break;
        }
    }
    // Lotti letti ma non applicabili: aspettano dipendenze non ancora arrivate.
    report.waiting = prefetched
        .batches
        .values()
        .any(|batches| !batches.is_empty());
    report.changed_notes = changed_notes.into_iter().take(MAX_CHANGED_NOTES).collect();
    Ok(())
}

/// Chiave locale corrispondente (tag unificati per nome).
fn translate(connection: &Connection, entity: &str, key: &str) -> Result<String, rusqlite::Error> {
    let alias = |remote: &str| -> Result<String, rusqlite::Error> {
        Ok(connection
            .query_row(
                "SELECT local_key FROM sync_aliases WHERE entity = 'tags' AND remote_key = ?1",
                [remote],
                |row| row.get::<_, String>(0),
            )
            .optional()?
            .unwrap_or_else(|| remote.to_owned()))
    };
    Ok(match entity {
        "tags" => alias(key)?,
        "note_tags" => match key.split_once('|') {
            Some((note, tag)) => format!("{note}|{}", alias(tag)?),
            None => key.to_owned(),
        },
        "trash_relations" => {
            let parts: Vec<&str> = key.split('|').collect();
            if parts.len() == 3 && parts[0] == "tag" {
                format!("tag|{}|{}", alias(parts[1])?, parts[2])
            } else {
                key.to_owned()
            }
        }
        _ => key.to_owned(),
    })
}

fn apply_batch(
    connection: &mut Connection,
    batch: &Batch,
    data_directory: &Path,
    report: &mut SyncReport,
    changed_notes: &mut HashSet<String>,
) -> Result<(), SyncError> {
    let transaction = connection.transaction()?;
    transaction.execute("UPDATE sync_control SET applying = 1 WHERE id = 1", [])?;
    let mut lamport = state_number(&transaction, "lamport")?;
    let mut orphaned_files: Vec<String> = Vec::new();
    // Lotto scritto prima che questo dispositivo entrasse nella libreria:
    // riapplicarlo ripercorre la storia degli altri dispositivi.
    let replay = Replay {
        history: u64::try_from(state_number(&transaction, &history_key(&batch.device))?)
            .is_ok_and(|last| batch.seq <= last),
        this_device: state(&transaction, "device_id")?.unwrap_or_default(),
    };
    for op in &batch.ops {
        let Some(table) = tables::table(&op.entity) else {
            report.rejected += 1;
            continue;
        };
        if op.version.1 != batch.device
            || op.version.0 < 1
            || tables::split_key(table, &op.key).is_none()
        {
            report.rejected += 1;
            continue;
        }
        lamport = lamport.max(op.version.0);
        transaction.execute_batch("SAVEPOINT sync_op")?;
        match apply_op(
            &transaction,
            table,
            op,
            &batch.device,
            &replay,
            data_directory,
            report,
            changed_notes,
            &mut orphaned_files,
        ) {
            Ok(()) => transaction.execute_batch("RELEASE sync_op")?,
            Err(error) => {
                tracing::warn!(entity = table.name, %error, "modifica remota scartata");
                transaction.execute_batch("ROLLBACK TO sync_op; RELEASE sync_op")?;
                report.rejected += 1;
            }
        }
    }
    set_state(&transaction, "lamport", &lamport.to_string())?;
    transaction.execute("UPDATE sync_control SET applying = 0 WHERE id = 1", [])?;
    transaction.commit()?;
    // File di allegati non più usati: rimossi dopo il commit.
    for stored in orphaned_files {
        let still_used: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM attachments WHERE stored_filename = ?1 AND deleted_at IS NULL)",
            [&stored],
            |row| row.get(0),
        )?;
        if !still_used && let Ok(path) = attachment_path(data_directory, &stored) {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
}

/// Contesto del lotto in corso di applicazione.
struct Replay {
    /// Il lotto appartiene alla storia precedente all'ingresso di questo
    /// dispositivo nella libreria.
    history: bool,
    this_device: String,
}

#[allow(clippy::too_many_arguments)]
fn apply_op(
    connection: &Connection,
    table: &SyncTable,
    op: &Operation,
    from_device: &str,
    replay: &Replay,
    data_directory: &Path,
    report: &mut SyncReport,
    changed_notes: &mut HashSet<String>,
    orphaned_files: &mut Vec<String>,
) -> Result<(), StorageError> {
    let key = translate(connection, table.name, &op.key)?;
    let local = version_of(connection, table.name, &key)?;
    if local.as_ref() == Some(&op.version) {
        return Ok(());
    }
    let fast_forward = local.is_none() || op.base == local;
    let current = tables::snapshot(connection, table, &key)?;
    let remote_wins = fast_forward
        || match (&op.data, &current) {
            // Una modifica prevale sempre su un'eliminazione contemporanea.
            (Some(_), None) => true,
            (None, Some(_)) => false,
            _ => local.as_ref().is_none_or(|local| op.version > *local),
        };
    // Ripercorrendo la storia, due versioni di altri dispositivi che allora
    // erano contemporanee si incontrano di nuovo. Quel conflitto è già stato
    // registrato dai dispositivi che c'erano: qui sarebbe un falso allarme,
    // perché nessuna modifica fatta su questo dispositivo è in gioco.
    let old_remote_conflict = replay.history
        && local
            .as_ref()
            .is_some_and(|local| local.1 != replay.this_device);
    if !fast_forward && table.keep_conflicts && !old_remote_conflict {
        let (loser, loser_device) = if remote_wins {
            (current.clone(), String::new())
        } else {
            (op.data.clone(), from_device.to_owned())
        };
        let winner = if remote_wins {
            op.data.clone()
        } else {
            current.clone()
        };
        if let Some(loser) = loser
            && differs(table, &loser, winner.as_ref())
        {
            keep_conflict(connection, table, &key, &loser, &loser_device)?;
            report.conflicts += 1;
        }
    }
    if !remote_wins {
        return Ok(());
    }
    match &op.data {
        Some(data) => {
            match tables::upsert(connection, table, &key, data) {
                Ok(Applied::Stored) => {}
                Ok(Applied::Skipped) => return Ok(()),
                Err(StorageError::Sqlite(error))
                    if table.name == "tags" && error.to_string().contains("tags.name") =>
                {
                    // Tag con lo stesso nome creato su due dispositivi: unificati.
                    let name = data["name"].as_str().unwrap_or_default();
                    if let Some(existing) = connection
                        .query_row(
                            "SELECT id FROM tags WHERE name = ?1 COLLATE NOCASE",
                            [name],
                            |row| row.get::<_, String>(0),
                        )
                        .optional()?
                    {
                        connection.execute(
                            "INSERT OR REPLACE INTO sync_aliases (entity, remote_key, local_key) VALUES ('tags', ?1, ?2)",
                            params![op.key, existing],
                        )?;
                    }
                    return Ok(());
                }
                Err(error) => return Err(error),
            }
            after_upsert(
                connection,
                table,
                &key,
                data,
                current.as_ref(),
                data_directory,
                changed_notes,
                orphaned_files,
            )?;
        }
        None => {
            if table.name == "notes" {
                let files: Vec<String> = connection
                    .prepare("SELECT stored_filename FROM attachments WHERE note_id = ?1")?
                    .query_map([&key], |row| row.get(0))?
                    .collect::<Result<_, _>>()?;
                orphaned_files.extend(files);
                crate::storage::remove_from_search_index(connection, &key)?;
                connection.execute(
                    "DELETE FROM reminders WHERE entity_type = 'note' AND entity_id = ?1",
                    [&key],
                )?;
                changed_notes.insert(key.clone());
            }
            if table.name == "attachments"
                && let Some(stored) = current
                    .as_ref()
                    .and_then(|row| row["stored_filename"].as_str())
            {
                orphaned_files.push(stored.to_owned());
            }
            tables::delete(connection, table, &key)?;
        }
    }
    set_version(connection, table.name, &key, &op.version)?;
    report.imported += 1;
    Ok(())
}

/// Differenze di contenuto tra perdente e vincitore (titolo e testo).
fn differs(table: &SyncTable, loser: &Value, winner: Option<&Value>) -> bool {
    let Some(winner) = winner else { return true };
    let fields: &[&str] = match table.name {
        "notes" => &["title", "content_json"],
        "templates" => &["name", "title", "content_json"],
        _ => table.columns,
    };
    fields.iter().any(|field| loser[*field] != winner[*field])
}

/// Conserva la versione perdente: elenco dei conflitti e, per le note,
/// cronologia della nota (confronto e ripristino dal dialog esistente).
fn keep_conflict(
    connection: &Connection,
    table: &SyncTable,
    key: &str,
    loser: &Value,
    from_device: &str,
) -> Result<(), StorageError> {
    let title = loser["title"]
        .as_str()
        .or_else(|| loser["name"].as_str())
        .unwrap_or_default();
    connection.execute(
        "INSERT INTO sync_conflicts (id, entity, entity_key, title, data_json, from_device) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![Uuid::new_v4().to_string(), table.name, key, title.chars().take(512).collect::<String>(), loser.to_string(), from_device],
    )?;
    if table.name == "notes"
        && let Some(content) = loser["content_json"].as_str()
    {
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM notes WHERE id = ?1)",
            [key],
            |row| row.get(0),
        )?;
        if exists {
            connection.execute(
                "INSERT INTO note_revisions (id, note_id, title, content_json) VALUES (?1, ?2, ?3, ?4)",
                params![Uuid::new_v4().to_string(), key, title, content],
            )?;
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn after_upsert(
    connection: &Connection,
    table: &SyncTable,
    key: &str,
    data: &Value,
    previous: Option<&Value>,
    data_directory: &Path,
    changed_notes: &mut HashSet<String>,
    orphaned_files: &mut Vec<String>,
) -> Result<(), StorageError> {
    match table.name {
        "notes" => {
            let title = data["title"].as_str().unwrap_or_default();
            let content: Value = data["content_json"]
                .as_str()
                .and_then(|text| serde_json::from_str(text).ok())
                .unwrap_or(Value::Null);
            sync_search_index(connection, key, title, &content)?;
            let text_changed = previous.is_some_and(|previous| {
                previous["title"] != data["title"]
                    || previous["content_json"] != data["content_json"]
            });
            if text_changed {
                connection.execute(
                    "INSERT INTO sync_remote_edits (note_id, applied_at) VALUES (?1, ?2)
                     ON CONFLICT(note_id) DO UPDATE SET applied_at = excluded.applied_at",
                    params![key, now_utc()],
                )?;
            }
            changed_notes.insert(key.to_owned());
        }
        "attachments" => {
            let stored = data["stored_filename"].as_str().unwrap_or_default();
            let sha256 = data["sha256"].as_str().unwrap_or_default();
            // Il nome del file deve rispettare il formato atteso (niente percorsi).
            ensure_attachments_directory(data_directory)?;
            let path = attachment_path(data_directory, stored)?;
            if data["deleted_at"].is_null() {
                if !path.is_file() {
                    connection.execute(
                        "INSERT OR REPLACE INTO sync_missing_blobs (stored_filename, sha256) VALUES (?1, ?2)",
                        params![stored, sha256],
                    )?;
                }
            } else {
                orphaned_files.push(stored.to_owned());
            }
            if let Some(note) = data["note_id"].as_str() {
                changed_notes.insert(note.to_owned());
            }
        }
        _ => {}
    }
    Ok(())
}

/// Prima di un salvataggio dell'editor: se il testo della nota è stato
/// cambiato da un altro dispositivo dopo la versione `base_updated_at` su
/// cui l'editor lavorava, quel testo viene conservato (cronologia e
/// conflitti) invece di essere sovrascritto in silenzio.
pub(crate) fn keep_overwritten_remote_edit(
    connection: &Connection,
    note_id: &str,
    base_updated_at: &str,
) -> Result<(), StorageError> {
    let remote_edit: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sync_remote_edits WHERE note_id = ?1)",
        [note_id],
        |row| row.get(0),
    )?;
    if !remote_edit {
        return Ok(());
    }
    let current: Option<(String, String, String)> = connection
        .query_row(
            "SELECT title, content_json, updated_at FROM notes WHERE id = ?1",
            [note_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;
    let Some((title, content_json, updated_at)) = current else {
        return Ok(());
    };
    if updated_at == base_updated_at {
        // L'editor aveva già la versione arrivata.
        return Ok(());
    }
    let notes = tables::table("notes").ok_or(StorageError::NoteNotFound)?;
    let loser = serde_json::json!({ "title": title, "content_json": content_json });
    keep_conflict(connection, notes, note_id, &loser, "")?;
    Ok(())
}

/// Cartella degli allegati (0700) su un dispositivo che non ne ha ancora.
fn ensure_attachments_directory(data_directory: &Path) -> Result<(), StorageError> {
    let directory = data_directory.join("attachments");
    if !directory.exists() {
        fs::create_dir_all(&directory)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))?;
        }
    }
    Ok(())
}

/// Copia dai blob gli allegati ancora mancanti (verificando l'hash).
fn fetch_missing_blobs(
    database: &Database,
    folder: &SyncFolder,
    data_directory: &Path,
) -> Result<(), SyncError> {
    let missing: Vec<(String, String)> = database
        .connection()?
        .prepare("SELECT stored_filename, sha256 FROM sync_missing_blobs")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (stored, sha256) in missing {
        let still_needed: bool = database.connection()?.query_row(
            "SELECT EXISTS(SELECT 1 FROM attachments WHERE stored_filename = ?1 AND deleted_at IS NULL)",
            [&stored],
            |row| row.get(0),
        )?;
        let path: Option<PathBuf> = attachment_path(data_directory, &stored).ok();
        let present = path.as_ref().is_some_and(|path| path.is_file());
        let done = if !still_needed || present {
            true
        } else if let Some(bytes) = folder.get_blob(&sha256) {
            write_attachment_file(data_directory, &stored, &bytes)?;
            true
        } else {
            false
        };
        if done {
            database.connection()?.execute(
                "DELETE FROM sync_missing_blobs WHERE stored_filename = ?1",
                [&stored],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
