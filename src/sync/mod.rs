//! Sincronizzazione tra dispositivi tramite una cartella condivisa
//! (ADR 0017): registro delle modifiche locali con trigger, lotti immutabili
//! per dispositivo, orologi di Lamport, consegna causale e conflitti risolti
//! in modo deterministico conservando la versione perdente.

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
        Batch, DeviceFile, FORMAT, FolderError, LocalStore, Operation, SyncFolder, SyncStore,
        Version,
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
    #[error("La sincronizzazione non è attiva")]
    NotConfigured,
    #[error("{0}")]
    Folder(#[from] FolderError),
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(
        "Nella cartella ci sono modifiche di questo dispositivo che il database non conosce: probabilmente lo stesso database è stato copiato su un altro computer. Per sicurezza la sincronizzazione è ferma: disattivala e riattivala per dare a questo computer una nuova identità."
    )]
    DuplicateDevice,
    #[error(
        "La cartella contiene un'altra libreria di Greatnote: scegli di nuovo la cartella nelle impostazioni"
    )]
    OtherLibrary,
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
    pub devices: Vec<SyncDevice>,
    pub conflicts: Vec<SyncConflict>,
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
                return Err(StorageError::InvalidEvent(
                    "scegli la cartella da usare per la sincronizzazione",
                ));
            }
            if !Path::new(&folder).is_dir() {
                return Err(StorageError::InvalidEvent("la cartella scelta non esiste"));
            }
        }
        let settings = SyncSettings {
            enabled: settings.enabled,
            transport: settings.transport,
            folder,
            dropbox_account,
            device_name,
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
            .collect::<Result<Vec<_>, _>>()?;
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
        })
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
        self.sync_now_with(None)
    }

    /// Esegue una sincronizzazione completa: esporta le modifiche locali e
    /// applica quelle degli altri dispositivi. `store` è l'archivio remoto
    /// (Dropbox); senza, si usa la cartella locale delle impostazioni.
    pub fn sync_now_with(
        &self,
        store: Option<Box<dyn SyncStore>>,
    ) -> Result<SyncReport, SyncError> {
        let settings = self.sync_settings()?;
        if !settings.enabled {
            return Err(SyncError::NotConfigured);
        }
        let store: Box<dyn SyncStore> = match (store, settings.transport) {
            (Some(store), _) => store,
            (None, SyncTransport::Folder) => {
                Box::new(LocalStore::new(Path::new(&settings.folder))?)
            }
            (None, SyncTransport::Dropbox) => return Err(SyncError::NotConfigured),
        };
        let data_directory = self.data_directory()?.to_path_buf();
        let mut connection = self.connection()?;
        let result = run(&mut connection, store, &settings, &data_directory);
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
}

// ---------------------------------------------------------------------------
// Esecuzione

fn run(
    connection: &mut Connection,
    store: Box<dyn SyncStore>,
    settings: &SyncSettings,
    data_directory: &Path,
) -> Result<SyncReport, SyncError> {
    let device = match state(connection, "device_id")? {
        Some(device) => device,
        None => {
            let device = Uuid::new_v4().to_string();
            set_state(connection, "device_id", &device)?;
            device
        }
    };
    let known_library = state(connection, "library_id")?;
    let (folder, library) = SyncFolder::open(store, || {
        known_library
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string())
    })?;
    match &known_library {
        Some(known) if *known != library => return Err(SyncError::OtherLibrary),
        Some(_) => {}
        None => set_state(connection, "library_id", &library)?,
    }

    // Ogni lotto è registrato nel database prima di essere scritto: un lotto
    // nella cartella che il database non conosce significa che un altro
    // computer usa la stessa identità (database copiato).
    let seq = u64::try_from(state_number(connection, "seq")?).unwrap_or(0);
    if folder.last_seq(&device)? > seq {
        return Err(SyncError::DuplicateDevice);
    }
    flush_outbox(connection, &folder)?;

    if state(connection, "initialized")?.is_none() {
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
        set_state(&transaction, "initialized", "1")?;
        transaction.commit()?;
    }

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
    connection.execute(
        "INSERT INTO sync_devices (device_id, name, applied_seq, last_seen_at) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(device_id) DO UPDATE SET name = excluded.name, applied_seq = excluded.applied_seq,
             last_seen_at = excluded.last_seen_at",
        params![device, name, seq as i64, now_utc()],
    )?;

    let mut report = SyncReport::default();
    export(
        connection,
        &folder,
        &library,
        &device,
        data_directory,
        &mut report,
    )?;
    import(
        connection,
        &folder,
        &library,
        &device,
        data_directory,
        &mut report,
    )?;
    fetch_missing_blobs(connection, &folder, data_directory)?;
    Ok(report)
}

fn default_device_name() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .or_else(|| fs::read_to_string("/etc/hostname").ok())
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Computer".to_owned())
}

fn export(
    connection: &mut Connection,
    folder: &SyncFolder,
    library: &str,
    device: &str,
    data_directory: &Path,
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
            if table.name == "attachments"
                && let Some(row) = &data
                && row["deleted_at"].is_null()
            {
                copy_blob(folder, data_directory, row);
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
    flush_outbox(connection, folder)?;
    report.exported = total;
    Ok(())
}

/// Scrive nella cartella i lotti registrati e non ancora scritti.
fn flush_outbox(connection: &Connection, folder: &SyncFolder) -> Result<(), SyncError> {
    let pending: Vec<(i64, String)> = connection
        .prepare("SELECT seq, batch_json FROM sync_outbox ORDER BY seq")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (seq, json) in pending {
        let batch: Batch = serde_json::from_str(&json).map_err(StorageError::from)?;
        // Già scritto prima di un'interruzione: è lo stesso contenuto.
        if folder.read_batch(&batch.device, batch.seq)?.is_none() {
            folder.write_batch(&batch)?;
        }
        connection.execute("DELETE FROM sync_outbox WHERE seq = ?1", [seq])?;
    }
    Ok(())
}

fn copy_blob(folder: &SyncFolder, data_directory: &Path, row: &Value) {
    let (Some(sha256), Some(stored)) = (row["sha256"].as_str(), row["stored_filename"].as_str())
    else {
        return;
    };
    match attachment_path(data_directory, stored) {
        Ok(path) if path.is_file() => {
            if let Err(error) = folder.put_blob(sha256, &path) {
                tracing::warn!(%error, "allegato non copiato nella cartella di sincronizzazione");
            }
        }
        // Il file manca anche qui (es. a sua volta in attesa): lo porterà un
        // altro dispositivo.
        _ => {}
    }
}

fn import(
    connection: &mut Connection,
    folder: &SyncFolder,
    library: &str,
    device: &str,
    data_directory: &Path,
    report: &mut SyncReport,
) -> Result<(), SyncError> {
    let mut changed_notes: HashSet<String> = HashSet::new();
    let mut broken: HashSet<String> = HashSet::new();
    loop {
        let mut progress = false;
        // Ricalcolato a ogni passata: conta solo l'attesa che resta alla fine.
        report.waiting = false;
        for remote in folder.devices()? {
            if remote == device || broken.contains(&remote) {
                continue;
            }
            let applied: i64 = connection
                .query_row(
                    "SELECT applied_seq FROM sync_devices WHERE device_id = ?1",
                    [&remote],
                    |row| row.get(0),
                )
                .optional()?
                .unwrap_or(0);
            let next = applied as u64 + 1;
            let batch = match folder.read_batch(&remote, next) {
                Ok(Some(batch)) => batch,
                Ok(None) => continue,
                Err(error) => {
                    report
                        .warnings
                        .push(format!("{}: {error}", device_label(folder, &remote)));
                    broken.insert(remote);
                    continue;
                }
            };
            if batch.library != library || batch.format > FORMAT {
                report.warnings.push(format!(
                    "{}: lotto di un'altra libreria o versione",
                    device_label(folder, &remote)
                ));
                broken.insert(remote);
                continue;
            }
            // Consegna causale: servono tutti i lotti che l'autore aveva già.
            let mut ready = true;
            for (dependency, needed) in &batch.deps {
                if dependency == device {
                    continue;
                }
                let have: i64 = connection
                    .query_row(
                        "SELECT applied_seq FROM sync_devices WHERE device_id = ?1",
                        [dependency],
                        |row| row.get(0),
                    )
                    .optional()?
                    .unwrap_or(0);
                if (have as u64) < *needed {
                    ready = false;
                }
            }
            if !ready {
                report.waiting = true;
                continue;
            }
            apply_batch(
                connection,
                &batch,
                data_directory,
                report,
                &mut changed_notes,
            )?;
            let name = folder
                .read_device(&remote)
                .map(|file| file.name)
                .unwrap_or_default();
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
    // I lotti in attesa restano tali solo se c'è davvero qualcosa da leggere.
    report.changed_notes = changed_notes.into_iter().take(MAX_CHANGED_NOTES).collect();
    Ok(())
}

fn device_label(folder: &SyncFolder, device: &str) -> String {
    folder
        .read_device(device)
        .map(|file| file.name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| device.to_owned())
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

#[allow(clippy::too_many_arguments)]
fn apply_op(
    connection: &Connection,
    table: &SyncTable,
    op: &Operation,
    from_device: &str,
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
    if !fast_forward && table.keep_conflicts {
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
                connection.execute("DELETE FROM note_search WHERE note_id = ?1", [&key])?;
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
    connection: &Connection,
    folder: &SyncFolder,
    data_directory: &Path,
) -> Result<(), SyncError> {
    let missing: Vec<(String, String)> = connection
        .prepare("SELECT stored_filename, sha256 FROM sync_missing_blobs")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    for (stored, sha256) in missing {
        let still_needed: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM attachments WHERE stored_filename = ?1 AND deleted_at IS NULL)",
            [&stored],
            |row| row.get(0),
        )?;
        let path: Option<PathBuf> = attachment_path(data_directory, &stored).ok();
        let present = path.as_ref().is_some_and(|path| path.is_file());
        if !still_needed || present {
            connection.execute(
                "DELETE FROM sync_missing_blobs WHERE stored_filename = ?1",
                [&stored],
            )?;
            continue;
        }
        if let Some(bytes) = folder.get_blob(&sha256) {
            write_attachment_file(data_directory, &stored, &bytes)?;
            connection.execute(
                "DELETE FROM sync_missing_blobs WHERE stored_filename = ?1",
                [&stored],
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
