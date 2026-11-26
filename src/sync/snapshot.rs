//! Istantanee della libreria per i dispositivi che entrano (ADR 0033).
//!
//! I lotti non vengono mai eliminati, quindi un dispositivo nuovo dovrebbe
//! rileggere tutta la storia: con migliaia di lotti, a 100 per dispositivo
//! a ogni giro, servirebbero ore. Ogni tanto un dispositivo scrive nella
//! **propria** cartella un'istantanea: per ogni entità sincronizzata la
//! riga attuale (o il tombstone) con la sua versione, e fin dove arrivava
//! la storia di ogni dispositivo (`covers`). Chi entra la applica come un
//! grande lotto e legge solo i lotti successivi.
//!
//! L'istantanea è applicata con le stesse regole dei lotti: versione per
//! versione, con i conflitti decisi allo stesso modo. Un dispositivo che la
//! usa arriva allo stato in cui arriverebbe rileggendo tutti i lotti
//! coperti. Se manca, è incompleta o non valida si rileggono i lotti.

use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
};

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

use super::{
    Replay, SyncError, SyncReport, apply_op,
    folder::{Operation, SyncFolder, Version, valid_id},
    now_utc, remove_orphaned_files, set_state, state, state_number,
    tables::{self, TABLES},
};
use crate::{
    storage::{Database, StorageError},
    time,
};

const FORMAT: u32 = 1;
/// Lotti non coperti da un'istantanea oltre i quali conviene scriverne una.
const UNCOVERED_THRESHOLD: u64 = if cfg!(test) { 3 } else { 500 };
/// Controllo al massimo una volta al giorno: elencare i lotti di ogni
/// dispositivo costa una chiamata per dispositivo su Dropbox.
const CHECK_INTERVAL_SECONDS: i64 = if cfg!(test) { 0 } else { 24 * 60 * 60 };
/// Istantanee conservate per dispositivo: la precedente resta mentre un
/// altro dispositivo potrebbe ancora leggerla.
const KEPT: usize = 2;
const PART_OPS: usize = 2000;
const PART_BYTES: usize = 8 * 1024 * 1024;
const MAX_PARTS: u32 = 10_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Manifest {
    format: u32,
    library: String,
    device: String,
    id: String,
    created_at: String,
    /// Ultimo lotto incluso di ogni dispositivo (autore compreso).
    covers: BTreeMap<String, u64>,
    lamport: i64,
    parts: u32,
    ops: u64,
    /// Tag unificati per nome dall'autore (chiave remota → locale).
    aliases: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Part {
    format: u32,
    library: String,
    snapshot: String,
    index: u32,
    ops: Vec<Operation>,
}

/// Istantanea letta e verificata, pronta da applicare.
pub(super) struct Loaded {
    manifest: Manifest,
    ops: Vec<Operation>,
}

impl Loaded {
    /// Ultimo lotto coperto di ogni dispositivo.
    pub(super) fn covers(&self) -> &BTreeMap<String, u64> {
        &self.manifest.covers
    }
}

fn directory(device: &str) -> String {
    format!("devices/{device}/snapshots")
}

fn manifest_path(device: &str, id: &str) -> String {
    format!("{}/{id}.json", directory(device))
}

fn part_path(device: &str, id: &str, index: u32) -> String {
    format!("{}/{id}/{index:06}.json", directory(device))
}

/// Id ordinabili: contatore di Lamport dell'autore, a 12 cifre.
fn valid_snapshot_id(id: &str) -> bool {
    id.len() == 12 && id.bytes().all(|b| b.is_ascii_digit())
}

/// Istantanee di un dispositivo, dalla più recente.
fn manifests(folder: &SyncFolder, library: &str, device: &str) -> Vec<Manifest> {
    let mut found: Vec<Manifest> = folder
        .list_named(&directory(device))
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, is_dir)| !is_dir)
        .filter_map(|(name, _)| name.strip_suffix(".json").map(str::to_owned))
        .filter(|id| valid_snapshot_id(id))
        .filter_map(|id| {
            let bytes = folder.read_named(&manifest_path(device, &id)).ok()??;
            let manifest: Manifest = serde_json::from_slice(&bytes).ok()?;
            (manifest.format == FORMAT
                && manifest.library == library
                && manifest.device == device
                && manifest.id == id
                && manifest.parts <= MAX_PARTS
                && manifest.covers.keys().all(|id| valid_id(id)))
            .then_some(manifest)
        })
        .collect();
    found.sort_by(|a, b| b.id.cmp(&a.id));
    found
}

fn coverage(manifest: &Manifest) -> u64 {
    manifest.covers.values().sum()
}

/// La migliore istantanea degli altri dispositivi (quella che copre più
/// lotti), letta per intero. `None` se non ce n'è una utilizzabile.
pub(super) fn find(
    folder: &SyncFolder,
    library: &str,
    own_device: &str,
    report: &mut SyncReport,
) -> Result<Option<Loaded>, SyncError> {
    let mut candidates: Vec<Manifest> = folder
        .devices()?
        .into_iter()
        .filter(|device| device != own_device)
        .flat_map(|device| manifests(folder, library, &device))
        .filter(|manifest| !manifest.covers.contains_key(own_device))
        .collect();
    candidates.sort_by(|a, b| {
        coverage(b)
            .cmp(&coverage(a))
            .then_with(|| b.created_at.cmp(&a.created_at))
    });
    // Se la migliore non si legge (eliminata nel frattempo, ancora in
    // arrivo) si prova la successiva.
    for manifest in candidates.into_iter().take(3) {
        match read_parts(folder, &manifest) {
            Ok(ops) => return Ok(Some(Loaded { manifest, ops })),
            Err(error) => {
                tracing::info!(%error, "istantanea non utilizzabile");
                report.warnings.push(crate::tr!(
                    "istantanea non utilizzabile, si rileggono i lotti: {error}",
                    "snapshot not usable, rereading the batches: {error}"
                ));
            }
        }
    }
    Ok(None)
}

fn read_parts(folder: &SyncFolder, manifest: &Manifest) -> Result<Vec<Operation>, SyncError> {
    let mut ops = Vec::new();
    for index in 1..=manifest.parts {
        let bytes = folder
            .read_named(&part_path(&manifest.device, &manifest.id, index))?
            .ok_or_else(|| {
                super::folder::FolderError::Invalid(crate::tr!(
                    "parte {index} mancante",
                    "part {index} missing"
                ))
            })?;
        let part: Part = serde_json::from_slice(&bytes).map_err(|error| {
            super::folder::FolderError::Invalid(format!("parte {index}: {error}"))
        })?;
        if part.format != FORMAT
            || part.library != manifest.library
            || part.snapshot != manifest.id
            || part.index != index
        {
            return Err(super::folder::FolderError::Invalid(crate::tr!(
                "parte {index} incoerente",
                "part {index} is inconsistent"
            ))
            .into());
        }
        ops.extend(part.ops);
    }
    if ops.len() as u64 != manifest.ops {
        return Err(super::folder::FolderError::Invalid(crate::tr!(
            "istantanea incompleta",
            "incomplete snapshot"
        ))
        .into());
    }
    Ok(ops)
}

/// Applica l'istantanea (con il database bloccato, dopo l'esportazione
/// delle modifiche locali e prima dei lotti). I dispositivi coperti
/// risultano applicati fin dove arriva l'istantanea.
pub(super) fn apply(
    connection: &mut Connection,
    loaded: &Loaded,
    own_device: &str,
    names: &BTreeMap<String, String>,
    data_directory: &Path,
    report: &mut SyncReport,
    changed_notes: &mut HashSet<String>,
) -> Result<(), SyncError> {
    let transaction = connection.transaction()?;
    transaction.execute("UPDATE sync_control SET applying = 1 WHERE id = 1", [])?;
    let mut lamport = state_number(&transaction, "lamport")?.max(loaded.manifest.lamport);
    let mut orphaned_files: Vec<String> = Vec::new();
    // Come per i lotti della storia precedente: le versioni contemporanee
    // tra altri dispositivi sono già state risolte da chi c'era.
    let replay = Replay {
        history: true,
        this_device: own_device.to_owned(),
    };
    for (remote, local) in &loaded.manifest.aliases {
        transaction.execute(
            "INSERT OR IGNORE INTO sync_aliases (entity, remote_key, local_key) VALUES ('tags', ?1, ?2)",
            params![remote, local],
        )?;
    }
    for op in &loaded.ops {
        let Some(table) = tables::table(&op.entity) else {
            report.rejected += 1;
            continue;
        };
        if !valid_id(&op.version.1)
            || op.version.0 < 1
            || tables::split_key(table, &op.key).is_none()
        {
            report.rejected += 1;
            continue;
        }
        lamport = lamport.max(op.version.0);
        transaction.execute_batch("SAVEPOINT sync_op")?;
        let author = op.version.1.clone();
        match apply_op(
            &transaction,
            table,
            op,
            &author,
            &replay,
            data_directory,
            report,
            changed_notes,
            &mut orphaned_files,
        ) {
            Ok(()) => transaction.execute_batch("RELEASE sync_op")?,
            Err(error) => {
                tracing::warn!(entity = table.name, %error, "riga dell'istantanea scartata");
                transaction.execute_batch("ROLLBACK TO sync_op; RELEASE sync_op")?;
                report.rejected += 1;
            }
        }
    }
    for (device, seq) in &loaded.manifest.covers {
        if device == own_device {
            continue;
        }
        let name = names.get(device).cloned().unwrap_or_default();
        transaction.execute(
            "INSERT INTO sync_devices (device_id, name, applied_seq, last_seen_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(device_id) DO UPDATE SET applied_seq = MAX(applied_seq, excluded.applied_seq),
                 name = CASE WHEN excluded.name = '' THEN sync_devices.name ELSE excluded.name END",
            params![device, name, *seq as i64, loaded.manifest.created_at],
        )?;
    }
    set_state(&transaction, "lamport", &lamport.to_string())?;
    transaction.execute("UPDATE sync_control SET applying = 0 WHERE id = 1", [])?;
    transaction.commit()?;
    remove_orphaned_files(connection, data_directory, orphaned_files)?;
    tracing::info!(
        ops = loaded.ops.len(),
        author = %loaded.manifest.device,
        "istantanea della libreria applicata"
    );
    Ok(())
}

/// Ultimo lotto coperto da un'istantanea, per dispositivo (la migliore per
/// ognuno tra quelle presenti).
fn best_covers(folder: &SyncFolder, library: &str, devices: &[String]) -> BTreeMap<String, u64> {
    let mut covers: BTreeMap<String, u64> = BTreeMap::new();
    for device in devices {
        if let Some(manifest) = manifests(folder, library, device).into_iter().next() {
            for (id, seq) in manifest.covers {
                let entry = covers.entry(id).or_default();
                *entry = (*entry).max(seq);
            }
        }
    }
    covers
}

/// Dopo una sincronizzazione riuscita: scrive un'istantanea se i lotti non
/// ancora coperti sono molti. Gli errori non fermano la sincronizzazione.
pub(super) fn maybe_write(
    database: &Database,
    folder: &SyncFolder,
    library: &str,
    device: &str,
) -> Result<bool, SyncError> {
    let now = time::now();
    {
        let connection = database.connection()?;
        if let Some(checked) = state(&connection, "snapshot_checked_at")?
            && let Ok(checked) = time::parse_utc(&checked)
            && now.as_second() - checked.as_second() < CHECK_INTERVAL_SECONDS
        {
            return Ok(false);
        }
        set_state(&connection, "snapshot_checked_at", &time::format_utc(now))?;
    }
    let devices = folder.devices()?;
    let covered = best_covers(folder, library, &devices);
    let mut uncovered = 0;
    for remote in &devices {
        let last = folder.last_seq(remote)?;
        uncovered += last.saturating_sub(covered.get(remote).copied().unwrap_or(0));
    }
    if uncovered < UNCOVERED_THRESHOLD {
        return Ok(false);
    }
    // Stato coerente: letto in una sola transazione e solo senza modifiche
    // locali non ancora esportate (altrimenti si riprova al giro dopo).
    let (manifest, parts) = {
        let mut connection = database.connection()?;
        let transaction = connection.transaction()?;
        let dirty: i64 =
            transaction.query_row("SELECT COUNT(*) FROM sync_dirty", [], |row| row.get(0))?;
        if dirty > 0 {
            transaction.execute(
                "DELETE FROM sync_state WHERE key = 'snapshot_checked_at'",
                [],
            )?;
            transaction.commit()?;
            return Ok(false);
        }
        build(&transaction, library, device)?
    };
    for (index, part) in parts.iter().enumerate() {
        let bytes = serde_json::to_vec(part).map_err(StorageError::from)?;
        folder.write_named(
            &part_path(device, &manifest.id, index as u32 + 1),
            &bytes,
            false,
        )?;
    }
    // Il manifesto per ultimo: chi lo trova trova anche tutte le parti.
    let bytes = serde_json::to_vec_pretty(&manifest).map_err(StorageError::from)?;
    folder.write_named(&manifest_path(device, &manifest.id), &bytes, false)?;
    tracing::info!(
        ops = manifest.ops,
        parts = manifest.parts,
        "istantanea della libreria scritta"
    );
    for old in manifests(folder, library, device).into_iter().skip(KEPT) {
        folder.remove_named(&format!("{}/{}", directory(device), old.id))?;
        folder.remove_named(&manifest_path(device, &old.id))?;
    }
    Ok(true)
}

fn build(
    connection: &Connection,
    library: &str,
    device: &str,
) -> Result<(Manifest, Vec<Part>), SyncError> {
    let lamport = state_number(connection, "lamport")?;
    let id = format!("{lamport:012}");
    let mut covers: BTreeMap<String, u64> = connection
        .prepare("SELECT device_id, applied_seq FROM sync_devices WHERE applied_seq > 0")?
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        })?
        .collect::<Result<_, _>>()?;
    let own_seq = u64::try_from(state_number(connection, "seq")?).unwrap_or(0);
    covers.insert(device.to_owned(), own_seq);
    let aliases: Vec<(String, String)> = connection
        .prepare("SELECT remote_key, local_key FROM sync_aliases WHERE entity = 'tags'")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?;
    let mut parts: Vec<Part> = Vec::new();
    let mut current: Vec<Operation> = Vec::new();
    let mut bytes = 0;
    let mut total = 0u64;
    let push_part = |ops: Vec<Operation>, parts: &mut Vec<Part>| {
        parts.push(Part {
            format: FORMAT,
            library: library.to_owned(),
            snapshot: id.clone(),
            index: parts.len() as u32 + 1,
            ops,
        });
    };
    // In ordine di dipendenza, come i lotti.
    for table in TABLES {
        let versions: Vec<(String, Version)> = connection
            .prepare("SELECT entity_key, counter, device FROM sync_versions WHERE entity = ?1 ORDER BY entity_key")?
            .query_map([table.name], |row| {
                Ok((row.get(0)?, Version(row.get(1)?, row.get(2)?)))
            })?
            .collect::<Result<_, _>>()?;
        for (key, version) in versions {
            let data = tables::snapshot(connection, table, &key)?;
            let size = data
                .as_ref()
                .map_or(64, |data| data.to_string().len() + 128);
            if !current.is_empty() && (current.len() >= PART_OPS || bytes + size > PART_BYTES) {
                push_part(std::mem::take(&mut current), &mut parts);
                bytes = 0;
            }
            bytes += size;
            total += 1;
            current.push(Operation {
                entity: table.name.to_owned(),
                key,
                version,
                base: None,
                data,
            });
        }
    }
    if !current.is_empty() {
        push_part(current, &mut parts);
    }
    let manifest = Manifest {
        format: FORMAT,
        library: library.to_owned(),
        device: device.to_owned(),
        id,
        created_at: now_utc(),
        covers,
        lamport,
        parts: parts.len() as u32,
        ops: total,
        aliases,
    };
    Ok((manifest, parts))
}
