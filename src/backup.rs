//! Backup locali verificabili, ripristino e controllo d'integrità.
//!
//! Un backup è un archivio ZIP con `manifest.json`, una copia coerente del
//! database ottenuta con `VACUUM INTO` e i file allegati. Ogni voce è
//! verificata con dimensione e SHA-256 prima di un ripristino.
//!
//! Le copie esportate possono essere cifrate con una password nel formato
//! standard age (`age-encryption.org/v1`, scrypt + ChaCha20-Poly1305): il
//! file resta decifrabile anche con lo strumento `age`, senza dipendere
//! dall'app. I backup interni non sono cifrati e sono protetti dai permessi
//! della cartella dati, come il database stesso.

use std::{
    collections::HashSet,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

use age::secrecy::SecretString;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zip::{CompressionMethod, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::storage::{
    CURRENT_SCHEMA_VERSION, Database, StorageError, open_and_migrate, open_connection,
};

// Identificatori di formato stabili: restano `gna-*` anche dopo il cambio di
// nome dell'app, per compatibilità con i backup esistenti (ADR 0006).
const BACKUP_FORMAT: &str = "gna-backup";
const BACKUP_FORMAT_VERSION: u32 = 1;
const MANIFEST_ENTRY: &str = "manifest.json";
const DATABASE_ENTRY: &str = "database/gna.sqlite3";
const MAX_MANIFEST_BYTES: u64 = 32 * 1024 * 1024;
const MAX_DATABASE_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_BACKUP_ENTRIES: usize = 1_000_000;
const BACKUP_FILE_PREFIX: &str = "greatnote-";
const LEGACY_BACKUP_FILE_PREFIX: &str = "gna-";
const AGE_MAGIC: &[u8] = b"age-encryption.org/v1\n";
const DECRYPTED_PREFIX: &str = ".decrypted-";
pub const MIN_BACKUP_PASSPHRASE_CHARS: usize = 10;
const ATTACHMENT_EXTENSIONS: [&str; 5] = ["png", "jpg", "gif", "pdf", "txt"];

const SETTING_AUTO_ENABLED: &str = "backup.auto_enabled";
const SETTING_INTERVAL_DAYS: &str = "backup.interval_days";
const SETTING_RETENTION: &str = "backup.retention";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackupKind {
    Manual,
    Automatic,
    PreMigration,
    PreRestore,
}

impl BackupKind {
    fn slug(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Automatic => "auto",
            Self::PreMigration => "premigration",
            Self::PreRestore => "prerestore",
        }
    }

    fn from_slug(slug: &str) -> Option<Self> {
        match slug {
            "manual" => Some(Self::Manual),
            "auto" => Some(Self::Automatic),
            "premigration" => Some(Self::PreMigration),
            "prerestore" => Some(Self::PreRestore),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ManifestFile {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct BackupManifest {
    format: String,
    version: u32,
    created_at: String,
    kind: BackupKind,
    schema_version: u32,
    app_version: String,
    files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupInfo {
    pub file_name: String,
    pub kind: BackupKind,
    pub created_at: String,
    pub size_bytes: u64,
    #[serde(skip)]
    compact_timestamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BackupVerification {
    pub created_at: String,
    pub kind: BackupKind,
    pub schema_version: u32,
    pub note_count: u64,
    pub attachment_count: u64,
    pub files_checked: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupSettings {
    pub auto_enabled: bool,
    pub interval_days: u32,
    pub retention: u32,
}

impl Default for BackupSettings {
    fn default() -> Self {
        Self {
            auto_enabled: true,
            interval_days: 7,
            retention: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttachmentIssue {
    pub note_id: String,
    pub note_title: String,
    pub filename: String,
    pub problem: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct IntegrityReport {
    pub ok: bool,
    pub sqlite_result: String,
    pub foreign_key_violations: u64,
    pub search_index_ok: bool,
    pub missing_search_rows: u64,
    pub note_count: u64,
    pub attachment_count: u64,
    pub attachment_issues: Vec<AttachmentIssue>,
    pub orphan_files: u64,
}

pub(crate) fn is_stored_attachment_name(name: &str) -> bool {
    name.split_once('.').is_some_and(|(id, extension)| {
        Uuid::parse_str(id).is_ok() && ATTACHMENT_EXTENSIONS.contains(&extension)
    })
}

/// Crea una directory privata (0700 su Unix) rifiutando i collegamenti
/// simbolici.
pub(crate) fn private_directory(path: &Path) -> Result<(), StorageError> {
    fs::create_dir_all(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(StorageError::InvalidBackup(
            "la cartella di destinazione non è una directory sicura".to_owned(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

pub(crate) fn create_private_file(path: &Path) -> Result<File, StorageError> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

pub(crate) struct HashingWriter<W> {
    inner: W,
    hasher: Sha256,
    size: u64,
}

impl<W: Write> HashingWriter<W> {
    pub(crate) fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            size: 0,
        }
    }

    pub(crate) fn finish(self) -> (String, u64) {
        (format!("{:x}", self.hasher.finalize()), self.size)
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let written = self.inner.write(buffer)?;
        self.hasher.update(&buffer[..written]);
        self.size += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

pub(crate) fn zip_options() -> SimpleFileOptions {
    SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(true)
        .unix_permissions(0o600)
}

fn add_file_to_zip<W: Write + io::Seek>(
    zip: &mut ZipWriter<W>,
    entry: &str,
    source: &Path,
) -> Result<ManifestFile, StorageError> {
    zip.start_file(entry, zip_options())?;
    let mut writer = HashingWriter::new(&mut *zip);
    io::copy(&mut File::open(source)?, &mut writer)?;
    let (sha256, size) = writer.finish();
    Ok(ManifestFile {
        path: entry.to_owned(),
        sha256,
        size,
    })
}

fn backup_directory(data_directory: &Path) -> Result<PathBuf, StorageError> {
    let directory = data_directory.join("backups");
    private_directory(&directory)?;
    Ok(directory)
}

fn timestamps(connection: &Connection) -> Result<(String, String), StorageError> {
    Ok(connection.query_row(
        "SELECT strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), strftime('%Y%m%dT%H%M%SZ', 'now')",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?)
}

fn parse_backup_name(file_name: &str) -> Option<(BackupKind, String)> {
    let stem = file_name
        .strip_prefix(BACKUP_FILE_PREFIX)
        .or_else(|| file_name.strip_prefix(LEGACY_BACKUP_FILE_PREFIX))?
        .strip_suffix(".zip")?;
    let mut parts = stem.split('-');
    let kind = BackupKind::from_slug(parts.next()?)?;
    let compact = parts.next()?;
    let suffix = parts.next()?;
    let valid_compact = compact.len() == 16
        && compact.as_bytes()[8] == b'T'
        && compact.ends_with('Z')
        && compact
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 15) || byte.is_ascii_digit());
    let valid_suffix = suffix.len() == 8 && suffix.bytes().all(|byte| byte.is_ascii_hexdigit());
    (parts.next().is_none() && valid_compact && valid_suffix).then(|| (kind, compact.to_owned()))
}

fn expand_timestamp(compact: &str) -> String {
    format!(
        "{}-{}-{}T{}:{}:{}Z",
        &compact[0..4],
        &compact[4..6],
        &compact[6..8],
        &compact[9..11],
        &compact[11..13],
        &compact[13..15]
    )
}

/// Crea un backup completo usando una connessione già aperta (anche prima
/// delle migrazioni).
pub(crate) fn create_backup_with_connection(
    connection: &Connection,
    data_directory: &Path,
    kind: BackupKind,
) -> Result<BackupInfo, StorageError> {
    let directory = backup_directory(data_directory)?;
    let (created_at, compact) = timestamps(connection)?;
    let suffix = Uuid::new_v4().simple().to_string()[..8].to_owned();
    let file_name = format!("{BACKUP_FILE_PREFIX}{}-{compact}-{suffix}.zip", kind.slug());
    let snapshot = directory.join(format!(".snapshot-{suffix}.sqlite3"));
    let partial = directory.join(format!(".{file_name}.partial"));
    let final_path = directory.join(&file_name);

    let result = (|| -> Result<u64, StorageError> {
        let snapshot_text = snapshot
            .to_str()
            .ok_or_else(|| StorageError::InvalidBackup("percorso non UTF-8".to_owned()))?;
        connection.execute("VACUUM INTO ?1", [snapshot_text])?;
        let schema_version: u32 =
            connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;

        let mut zip = ZipWriter::new(create_private_file(&partial)?);
        let mut files = vec![add_file_to_zip(&mut zip, DATABASE_ENTRY, &snapshot)?];
        let attachments = data_directory.join("attachments");
        if fs::symlink_metadata(&attachments).is_ok_and(|metadata| metadata.is_dir()) {
            let mut names = Vec::new();
            for entry in fs::read_dir(&attachments)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                if is_stored_attachment_name(&name) && entry.file_type()?.is_file() {
                    names.push(name);
                }
            }
            names.sort();
            for name in names {
                files.push(add_file_to_zip(
                    &mut zip,
                    &format!("attachments/{name}"),
                    &attachments.join(&name),
                )?);
            }
        }
        let manifest = BackupManifest {
            format: BACKUP_FORMAT.to_owned(),
            version: BACKUP_FORMAT_VERSION,
            created_at: created_at.clone(),
            kind,
            schema_version,
            app_version: env!("CARGO_PKG_VERSION").to_owned(),
            files,
        };
        zip.start_file(MANIFEST_ENTRY, zip_options())?;
        zip.write_all(&serde_json::to_vec_pretty(&manifest)?)?;
        let file = zip.finish()?;
        file.sync_all()?;
        let size = file.metadata()?.len();
        drop(file);
        fs::rename(&partial, &final_path)?;
        Ok(size)
    })();

    if let Err(error) = fs::remove_file(&snapshot)
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!("snapshot temporaneo non rimosso: {error}");
    }
    match result {
        Ok(size_bytes) => Ok(BackupInfo {
            file_name,
            kind,
            created_at,
            size_bytes,
            compact_timestamp: compact,
        }),
        Err(error) => {
            let _ = fs::remove_file(&partial);
            Err(error)
        }
    }
}

pub fn list_backups(data_directory: &Path) -> Result<Vec<BackupInfo>, StorageError> {
    let directory = data_directory.join("backups");
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut backups = Vec::new();
    for entry in fs::read_dir(&directory)? {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy().into_owned();
        let Some((kind, compact)) = parse_backup_name(&file_name) else {
            continue;
        };
        if !entry.file_type()?.is_file() {
            continue;
        }
        backups.push(BackupInfo {
            created_at: expand_timestamp(&compact),
            size_bytes: entry.metadata()?.len(),
            file_name,
            kind,
            compact_timestamp: compact,
        });
    }
    backups.sort_by(|a, b| {
        b.compact_timestamp
            .cmp(&a.compact_timestamp)
            .then_with(|| b.file_name.cmp(&a.file_name))
    });
    Ok(backups)
}

/// Risolve il nome di un backup interno senza accettare percorsi.
pub fn backup_path(data_directory: &Path, file_name: &str) -> Result<PathBuf, StorageError> {
    if parse_backup_name(file_name).is_none() {
        return Err(StorageError::InvalidBackup(
            "nome del backup non valido".to_owned(),
        ));
    }
    let path = data_directory.join("backups").join(file_name);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|_| StorageError::InvalidBackup("backup non trovato".to_owned()))?;
    if !metadata.is_file() {
        return Err(StorageError::InvalidBackup("backup non trovato".to_owned()));
    }
    Ok(path)
}

/// Indica se il file è un backup cifrato nel formato age.
pub fn is_encrypted_backup(path: &Path) -> Result<bool, StorageError> {
    let mut header = Vec::with_capacity(AGE_MAGIC.len());
    File::open(path)?
        .take(AGE_MAGIC.len() as u64)
        .read_to_end(&mut header)?;
    Ok(header == AGE_MAGIC)
}

fn passphrase(value: &str) -> Result<SecretString, StorageError> {
    if value.chars().count() < MIN_BACKUP_PASSPHRASE_CHARS {
        return Err(StorageError::WeakBackupPassphrase(
            MIN_BACKUP_PASSPHRASE_CHARS,
        ));
    }
    Ok(SecretString::from(value.to_owned()))
}

/// Scrive in `destination` una copia cifrata con password del backup
/// `source`, senza lasciare file parziali in caso di errore.
pub fn encrypt_backup_file(
    source: &Path,
    destination: &Path,
    passphrase_text: &str,
) -> Result<(), StorageError> {
    let secret = passphrase(passphrase_text)?;
    if is_encrypted_backup(source)? {
        return Err(invalid("il file è già cifrato"));
    }
    let partial = crate::transfer::partial_path(destination)?;
    let result = (|| {
        #[cfg_attr(not(test), allow(unused_mut))]
        let mut recipient = age::scrypt::Recipient::new(secret);
        // Nei test un work factor basso evita secondi di CPU e centinaia di
        // MiB per ogni cifratura; in produzione resta quello calibrato.
        #[cfg(test)]
        recipient.set_work_factor(10);
        let encryptor =
            age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
                .map_err(|_| invalid("cifratura non disponibile"))?;
        let mut writer = encryptor.wrap_output(create_private_file(&partial)?)?;
        io::copy(&mut File::open(source)?, &mut writer)?;
        let file = writer.finish()?;
        file.sync_all()?;
        drop(file);
        fs::rename(&partial, destination)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

/// Decifra un backup age in un file privato temporaneo nella cartella dati
/// e ne restituisce il percorso. Il chiamante deve eliminarlo dopo l'uso;
/// gli eventuali residui vengono rimossi all'avvio da
/// [`remove_decrypted_backups`].
fn decrypt_backup_file(
    source: &Path,
    data_directory: &Path,
    passphrase_text: &str,
) -> Result<PathBuf, StorageError> {
    let identity = age::scrypt::Identity::new(SecretString::from(passphrase_text.to_owned()));
    let target = data_directory.join(format!("{DECRYPTED_PREFIX}{}.zip", Uuid::new_v4().simple()));
    let result = (|| {
        let decryptor = age::Decryptor::new_buffered(io::BufReader::new(File::open(source)?))
            .map_err(|_| invalid("file cifrato non riconosciuto"))?;
        if !decryptor.is_scrypt() {
            return Err(invalid(
                "il file è cifrato con chiavi, non con una password",
            ));
        }
        let mut reader = decryptor
            .decrypt(std::iter::once(&identity as &dyn age::Identity))
            .map_err(|error| match error {
                age::DecryptError::DecryptionFailed
                | age::DecryptError::KeyDecryptionFailed
                | age::DecryptError::NoMatchingKeys => StorageError::WrongBackupPassphrase,
                age::DecryptError::ExcessiveWork { .. } => invalid(
                    "la cifratura richiede più risorse di quelle consentite su questo computer",
                ),
                _ => invalid("file cifrato non leggibile"),
            })?;
        let mut file = create_private_file(&target)?;
        // La password è già verificata dall'header: un blocco che non supera
        // l'autenticazione indica un file alterato o troncato.
        io::copy(&mut reader, &mut file).map_err(|error| match error.kind() {
            io::ErrorKind::InvalidData | io::ErrorKind::UnexpectedEof => {
                invalid("file cifrato danneggiato o incompleto")
            }
            _ => error.into(),
        })?;
        file.sync_all()?;
        Ok(())
    })();
    match result {
        Ok(()) => Ok(target),
        Err(error) => {
            let _ = fs::remove_file(&target);
            Err(error)
        }
    }
}

/// Rimuove le copie decifrate rimaste da una sessione interrotta.
pub fn remove_decrypted_backups(data_directory: &Path) -> Result<(), StorageError> {
    for entry in fs::read_dir(data_directory)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(DECRYPTED_PREFIX) && entry.file_type()?.is_file() {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

/// Backup esterno cifrato già decifrato e verificato. La copia in chiaro
/// viene eliminata quando il valore esce dallo scope.
#[derive(Debug)]
pub struct DecryptedBackup {
    path: PathBuf,
    pub verification: BackupVerification,
}

impl DecryptedBackup {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for DecryptedBackup {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.path)
            && error.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!("copia decifrata non rimossa: {error}");
        }
    }
}

pub(crate) fn read_zip_entry<R: Read + io::Seek>(
    archive: &mut ZipArchive<R>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, StorageError> {
    let entry = archive.by_name(name)?;
    if entry.size() > limit {
        return Err(StorageError::InvalidBackup(format!(
            "la voce {name} è troppo grande"
        )));
    }
    let mut bytes = Vec::new();
    entry.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(StorageError::InvalidBackup(format!(
            "la voce {name} è troppo grande"
        )));
    }
    Ok(bytes)
}

fn invalid(message: impl Into<String>) -> StorageError {
    StorageError::InvalidBackup(message.into())
}

/// Verifica un backup ed estrae database (e allegati, se richiesto) in
/// `staging`. Nessun nome contenuto nell'archivio viene usato come percorso
/// senza essere convalidato.
fn extract_backup(
    path: &Path,
    staging: &Path,
    extract_attachments: bool,
) -> Result<BackupVerification, StorageError> {
    if is_encrypted_backup(path)? {
        return Err(StorageError::BackupPassphraseRequired);
    }
    let mut archive = ZipArchive::new(File::open(path)?)?;
    if archive.len() > MAX_BACKUP_ENTRIES {
        return Err(invalid("troppe voci nell'archivio"));
    }
    let manifest: BackupManifest = serde_json::from_slice(&read_zip_entry(
        &mut archive,
        MANIFEST_ENTRY,
        MAX_MANIFEST_BYTES,
    )?)
    .map_err(|_| invalid("manifest mancante o non leggibile"))?;
    if manifest.format != BACKUP_FORMAT || manifest.version != BACKUP_FORMAT_VERSION {
        return Err(invalid("formato di backup non riconosciuto"));
    }
    if manifest.schema_version == 0 || manifest.schema_version > CURRENT_SCHEMA_VERSION {
        return Err(invalid(
            "il backup è stato creato da una versione più recente dell'app",
        ));
    }

    let mut seen = HashSet::new();
    let mut attachment_files = HashSet::new();
    for file in &manifest.files {
        let valid = file.path == DATABASE_ENTRY
            || file
                .path
                .strip_prefix("attachments/")
                .is_some_and(is_stored_attachment_name);
        if !valid || !seen.insert(file.path.clone()) || file.sha256.len() != 64 {
            return Err(invalid(format!(
                "voce non valida nel manifest: {}",
                file.path
            )));
        }
        if let Some(name) = file.path.strip_prefix("attachments/") {
            attachment_files.insert(name.to_owned());
        }
    }
    if !seen.contains(DATABASE_ENTRY) {
        return Err(invalid("il backup non contiene il database"));
    }

    private_directory(staging)?;
    if extract_attachments {
        private_directory(&staging.join("attachments"))?;
    }
    let database_path = staging.join("gna.sqlite3");
    for file in &manifest.files {
        let limit = if file.path == DATABASE_ENTRY {
            MAX_DATABASE_BYTES
        } else {
            crate::storage::MAX_ATTACHMENT_BYTES as u64
        };
        if file.size > limit {
            return Err(invalid(format!("dimensione non valida: {}", file.path)));
        }
        let entry = archive.by_name(&file.path)?;
        let target = if file.path == DATABASE_ENTRY {
            Some(database_path.clone())
        } else if extract_attachments {
            file.path
                .strip_prefix("attachments/")
                .map(|name| staging.join("attachments").join(name))
        } else {
            None
        };
        let (sha256, size) = match &target {
            Some(target) => {
                let mut writer = HashingWriter::new(create_private_file(target)?);
                io::copy(&mut entry.take(limit + 1), &mut writer)?;
                writer.inner.sync_all()?;
                writer.finish()
            }
            None => {
                let mut writer = HashingWriter::new(io::sink());
                io::copy(&mut entry.take(limit + 1), &mut writer)?;
                writer.finish()
            }
        };
        if size != file.size || sha256 != file.sha256 {
            return Err(invalid(format!(
                "verifica d'integrità fallita per {}",
                file.path
            )));
        }
    }

    let connection = Connection::open_with_flags(&database_path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if integrity != "ok" {
        return Err(invalid(format!("database danneggiato: {integrity}")));
    }
    let schema_version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if schema_version != manifest.schema_version {
        return Err(invalid("versione dello schema incoerente con il manifest"));
    }
    let note_count = count(&connection, "SELECT COUNT(*) FROM notes")?;
    let mut attachment_count = 0;
    if schema_version >= 3 {
        let mut statement = connection
            .prepare("SELECT stored_filename FROM attachments WHERE deleted_at IS NULL")?;
        let names = statement.query_map([], |row| row.get::<_, String>(0))?;
        for name in names {
            let name = name?;
            if !attachment_files.contains(&name) {
                return Err(invalid("il backup non contiene tutti gli allegati"));
            }
            attachment_count += 1;
        }
    }
    drop(connection);
    Ok(BackupVerification {
        created_at: manifest.created_at,
        kind: manifest.kind,
        schema_version,
        note_count,
        attachment_count,
        files_checked: manifest.files.len() as u64,
    })
}

fn count(connection: &Connection, sql: &str) -> Result<u64, StorageError> {
    let value: i64 = connection.query_row(sql, [], |row| row.get(0))?;
    Ok(u64::try_from(value).unwrap_or(0))
}

fn remove_directory(path: &Path) {
    if let Err(error) = fs::remove_dir_all(path)
        && error.kind() != io::ErrorKind::NotFound
    {
        tracing::warn!("cartella temporanea non rimossa: {error}");
    }
}

fn read_setting(connection: &Connection, key: &str) -> Result<Option<String>, StorageError> {
    Ok(connection
        .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .optional()?)
}

fn write_setting(connection: &Connection, key: &str, value: &str) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET
             value = excluded.value,
             updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![key, value],
    )?;
    Ok(())
}

impl Database {
    pub fn data_directory(&self) -> Result<&Path, StorageError> {
        self.path()
            .and_then(Path::parent)
            .ok_or(StorageError::InMemoryDatabase)
    }

    pub fn backup_settings(&self) -> Result<BackupSettings, StorageError> {
        let connection = self.connection()?;
        let defaults = BackupSettings::default();
        Ok(BackupSettings {
            auto_enabled: read_setting(&connection, SETTING_AUTO_ENABLED)?
                .map(|value| value == "true")
                .unwrap_or(defaults.auto_enabled),
            interval_days: read_setting(&connection, SETTING_INTERVAL_DAYS)?
                .and_then(|value| value.parse().ok())
                .filter(|days| (1..=365).contains(days))
                .unwrap_or(defaults.interval_days),
            retention: read_setting(&connection, SETTING_RETENTION)?
                .and_then(|value| value.parse().ok())
                .filter(|count| (1..=100).contains(count))
                .unwrap_or(defaults.retention),
        })
    }

    pub fn update_backup_settings(
        &self,
        settings: BackupSettings,
    ) -> Result<BackupSettings, StorageError> {
        if !(1..=365).contains(&settings.interval_days) {
            return Err(StorageError::InvalidSetting(
                "l'intervallo deve essere tra 1 e 365 giorni",
            ));
        }
        if !(1..=100).contains(&settings.retention) {
            return Err(StorageError::InvalidSetting(
                "il numero di backup conservati deve essere tra 1 e 100",
            ));
        }
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        write_setting(
            &transaction,
            SETTING_AUTO_ENABLED,
            if settings.auto_enabled {
                "true"
            } else {
                "false"
            },
        )?;
        write_setting(
            &transaction,
            SETTING_INTERVAL_DAYS,
            &settings.interval_days.to_string(),
        )?;
        write_setting(
            &transaction,
            SETTING_RETENTION,
            &settings.retention.to_string(),
        )?;
        transaction.commit()?;
        Ok(settings)
    }

    pub fn create_backup(&self, kind: BackupKind) -> Result<BackupInfo, StorageError> {
        let data_directory = self.data_directory()?;
        let connection = self.connection()?;
        create_backup_with_connection(&connection, data_directory, kind)
    }

    pub fn list_backups(&self) -> Result<Vec<BackupInfo>, StorageError> {
        list_backups(self.data_directory()?)
    }

    /// Verifica un archivio di backup senza modificare i dati correnti.
    pub fn verify_backup_file(&self, path: &Path) -> Result<BackupVerification, StorageError> {
        let staging = self
            .data_directory()?
            .join(format!(".verify-{}", Uuid::new_v4().simple()));
        let result = extract_backup(path, &staging, false);
        remove_directory(&staging);
        result
    }

    /// Decifra con la password un backup esterno e lo verifica senza
    /// modificare i dati correnti.
    pub fn decrypt_and_verify_backup(
        &self,
        path: &Path,
        passphrase: &str,
    ) -> Result<DecryptedBackup, StorageError> {
        let path = decrypt_backup_file(path, self.data_directory()?, passphrase)?;
        // Da qui in poi la copia in chiaro è eliminata anche in caso di errore.
        let mut decrypted = DecryptedBackup {
            path,
            verification: BackupVerification {
                created_at: String::new(),
                kind: BackupKind::Manual,
                schema_version: 0,
                note_count: 0,
                attachment_count: 0,
                files_checked: 0,
            },
        };
        decrypted.verification = self.verify_backup_file(&decrypted.path)?;
        Ok(decrypted)
    }

    /// Ripristina un backup verificato. Prima salva lo stato corrente in un
    /// backup `pre-restore`; se uno dei passaggi fallisce, ripristina i file
    /// originali e riapre il database precedente.
    pub fn restore_backup_file(&self, path: &Path) -> Result<BackupInfo, StorageError> {
        let database_path = self
            .path()
            .ok_or(StorageError::InMemoryDatabase)?
            .to_path_buf();
        let data_directory = self.data_directory()?.to_path_buf();
        let staging = data_directory.join(format!(".restore-{}", Uuid::new_v4().simple()));
        let result = (|| {
            extract_backup(path, &staging, true)?;
            // Le migrazioni di un backup meno recente avvengono sulla copia.
            open_and_migrate(&staging.join("gna.sqlite3"))?
                .close()
                .map_err(|(_, error)| error)?;

            let mut connection = self.connection()?;
            let safety = create_backup_with_connection(
                &connection,
                &data_directory,
                BackupKind::PreRestore,
            )?;
            let current = std::mem::replace(&mut *connection, Connection::open_in_memory()?);
            if let Err((current, error)) = current.close() {
                *connection = current;
                return Err(error.into());
            }

            if let Err(error) = swap_in(&database_path, &data_directory, &staging) {
                *connection = open_connection(&database_path)?;
                return Err(error);
            }
            match open_and_migrate(&database_path) {
                Ok(restored) => {
                    *connection = restored;
                    Ok(safety)
                }
                Err(error) => {
                    swap_back(&database_path, &data_directory, &staging);
                    *connection = open_connection(&database_path)?;
                    Err(error)
                }
            }
        })();
        remove_directory(&staging);
        result
    }

    pub fn restore_backup(&self, file_name: &str) -> Result<BackupInfo, StorageError> {
        let path = backup_path(self.data_directory()?, file_name)?;
        self.restore_backup_file(&path)
    }

    /// Esegue il backup automatico se abilitato e scaduto, poi elimina i
    /// backup automatici oltre la soglia di conservazione.
    pub fn run_automatic_backup(&self) -> Result<Option<BackupInfo>, StorageError> {
        let settings = self.backup_settings()?;
        if !settings.auto_enabled {
            return Ok(None);
        }
        let data_directory = self.data_directory()?;
        let connection = self.connection()?;
        let threshold: String = connection.query_row(
            "SELECT strftime('%Y%m%dT%H%M%SZ', 'now', ?1)",
            [format!("-{} days", settings.interval_days)],
            |row| row.get(0),
        )?;
        let automatic: Vec<BackupInfo> = list_backups(data_directory)?
            .into_iter()
            .filter(|backup| backup.kind == BackupKind::Automatic)
            .collect();
        let due = automatic
            .first()
            .is_none_or(|latest| latest.compact_timestamp <= threshold);
        let created = if due {
            Some(create_backup_with_connection(
                &connection,
                data_directory,
                BackupKind::Automatic,
            )?)
        } else {
            None
        };
        drop(connection);
        let automatic: Vec<BackupInfo> = list_backups(data_directory)?
            .into_iter()
            .filter(|backup| backup.kind == BackupKind::Automatic)
            .collect();
        for old in automatic.iter().skip(settings.retention as usize) {
            fs::remove_file(data_directory.join("backups").join(&old.file_name))?;
        }
        Ok(created)
    }

    pub fn check_integrity(&self) -> Result<IntegrityReport, StorageError> {
        let data_directory = self.data_directory().ok();
        let (
            sqlite_result,
            foreign_key_violations,
            search_index_ok,
            missing_search_rows,
            note_count,
            attachments,
        ) = {
            let connection = self.connection()?;
            let mut statement = connection.prepare("PRAGMA integrity_check")?;
            let messages = statement
                .query_map([], |row| row.get::<_, String>(0))?
                .take(20)
                .collect::<Result<Vec<_>, _>>()?;
            let foreign_key_violations = connection
                .prepare("PRAGMA foreign_key_check")?
                .query_map([], |_| Ok(()))?
                .count() as u64;
            let search_index_ok = connection
                .execute(
                    "INSERT INTO note_search(note_search) VALUES ('integrity-check')",
                    [],
                )
                .is_ok();
            let missing_search_rows = count(
                &connection,
                "SELECT COUNT(*) FROM notes
                 WHERE NOT EXISTS (SELECT 1 FROM note_search WHERE note_search.note_id = notes.id)",
            )?;
            let note_count = count(&connection, "SELECT COUNT(*) FROM notes")?;
            let mut statement = connection.prepare(
                "SELECT notes.id, notes.title, attachments.original_filename,
                        attachments.stored_filename, attachments.sha256, attachments.size_bytes
                 FROM attachments INNER JOIN notes ON notes.id = attachments.note_id
                 WHERE attachments.deleted_at IS NULL
                 ORDER BY notes.title, attachments.original_filename",
            )?;
            let attachments = statement
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, i64>(5)?,
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            (
                messages.join("; "),
                foreign_key_violations,
                search_index_ok,
                missing_search_rows,
                note_count,
                attachments,
            )
        };

        // Il controllo dei file avviene senza trattenere il blocco del database.
        let mut attachment_issues = Vec::new();
        let mut referenced = HashSet::new();
        let mut orphan_files = 0;
        if let Some(data_directory) = data_directory {
            for (note_id, note_title, filename, stored, expected_sha, expected_size) in &attachments
            {
                referenced.insert(stored.clone());
                let path = data_directory.join("attachments").join(stored);
                let problem = match fs::symlink_metadata(&path) {
                    Err(_) => Some("missing"),
                    Ok(metadata) if !metadata.is_file() => Some("missing"),
                    Ok(metadata) if metadata.len() != *expected_size as u64 => Some("corrupted"),
                    Ok(_) => {
                        let mut writer = HashingWriter::new(io::sink());
                        io::copy(&mut File::open(&path)?, &mut writer)?;
                        (writer.finish().0 != *expected_sha).then_some("corrupted")
                    }
                };
                if let Some(problem) = problem {
                    attachment_issues.push(AttachmentIssue {
                        note_id: note_id.clone(),
                        note_title: note_title.clone(),
                        filename: filename.clone(),
                        problem: problem.to_owned(),
                    });
                }
            }
            let directory = data_directory.join("attachments");
            if directory.is_dir() {
                for entry in fs::read_dir(directory)? {
                    let name = entry?.file_name().to_string_lossy().into_owned();
                    if !referenced.contains(&name) {
                        orphan_files += 1;
                    }
                }
            }
        }
        Ok(IntegrityReport {
            ok: sqlite_result == "ok"
                && foreign_key_violations == 0
                && search_index_ok
                && missing_search_rows == 0
                && attachment_issues.is_empty(),
            sqlite_result,
            foreign_key_violations,
            search_index_ok,
            missing_search_rows,
            note_count,
            attachment_count: attachments.len() as u64,
            attachment_issues,
            orphan_files,
        })
    }

    /// Ricostruisce l'indice FTS di tutte le note.
    pub fn rebuild_search_index(&self) -> Result<u64, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM note_search", [])?;
        let count = transaction.execute(
            "INSERT INTO note_search (note_id, title, content, attachments)
             SELECT notes.id, notes.title,
                    COALESCE((
                        SELECT group_concat(node.value, ' ')
                        FROM json_tree(notes.content_json) AS node
                        WHERE node.type = 'text'
                    ), ''),
                    COALESCE((
                        SELECT group_concat(attachments.search_text, ' ')
                        FROM attachments
                        WHERE attachments.note_id = notes.id
                          AND attachments.deleted_at IS NULL
                    ), '')
             FROM notes",
            [],
        )?;
        transaction.execute(
            "INSERT INTO note_search(note_search) VALUES ('optimize')",
            [],
        )?;
        transaction.commit()?;
        Ok(count as u64)
    }
}

fn swap_in(
    database_path: &Path,
    data_directory: &Path,
    staging: &Path,
) -> Result<(), StorageError> {
    let live_attachments = data_directory.join("attachments");
    let staged_attachments = staging.join("attachments");
    let previous_database = staging.join("previous.sqlite3");
    let previous_attachments = staging.join("previous-attachments");

    fs::rename(database_path, &previous_database)?;
    if let Err(error) = fs::rename(staging.join("gna.sqlite3"), database_path) {
        let _ = fs::rename(&previous_database, database_path);
        return Err(error.into());
    }
    if live_attachments.exists()
        && let Err(error) = fs::rename(&live_attachments, &previous_attachments)
    {
        swap_back(database_path, data_directory, staging);
        return Err(error.into());
    }
    if let Err(error) = fs::rename(&staged_attachments, &live_attachments) {
        swap_back(database_path, data_directory, staging);
        return Err(error.into());
    }
    Ok(())
}

fn swap_back(database_path: &Path, data_directory: &Path, staging: &Path) {
    let live_attachments = data_directory.join("attachments");
    let previous_database = staging.join("previous.sqlite3");
    let previous_attachments = staging.join("previous-attachments");
    if previous_database.exists() {
        let _ = fs::rename(database_path, staging.join("failed.sqlite3"));
        if let Err(error) = fs::rename(&previous_database, database_path) {
            tracing::error!("ripristino del database precedente non riuscito: {error}");
        }
    }
    if previous_attachments.exists() {
        let _ = fs::rename(&live_attachments, staging.join("failed-attachments"));
        if let Err(error) = fs::rename(&previous_attachments, &live_attachments) {
            tracing::error!("ripristino degli allegati precedenti non riuscito: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, io::Write};

    use serde_json::json;

    use super::{
        BackupKind, BackupSettings, encrypt_backup_file, is_encrypted_backup, list_backups,
        parse_backup_name, remove_decrypted_backups,
    };
    use crate::{
        domain::note::empty_document,
        storage::{Database, StorageError},
    };

    fn decrypted_leftovers(data: &std::path::Path) -> usize {
        fs::read_dir(data)
            .expect("dir")
            .filter(|entry| {
                entry
                    .as_ref()
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".decrypted-")
            })
            .count()
    }

    fn document(text: &str) -> serde_json::Value {
        json!({ "type": "doc", "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }] })
    }

    #[test]
    fn backup_names_are_strictly_parsed() {
        assert!(parse_backup_name("greatnote-manual-20261014T101112Z-0a1b2c3d.zip").is_some());
        // Nomi creati prima del cambio di nome dell'app.
        assert!(parse_backup_name("gna-manual-20261014T101112Z-0a1b2c3d.zip").is_some());
        for name in [
            "greatnote-manual-20261014T101112Z-0a1b2c3d.zip.age",
            "other-manual-20261014T101112Z-0a1b2c3d.zip",
            "../gna-manual-20261014T101112Z-0a1b2c3d.zip",
            "gna-manual-20261014T101112Z-0a1b2c3d.zip.partial",
            "gna-other-20261014T101112Z-0a1b2c3d.zip",
            "gna-manual-2026101XT101112Z-0a1b2c3d.zip",
        ] {
            assert!(parse_backup_name(name).is_none(), "{name}");
        }
    }

    #[test]
    fn backup_is_verified_and_restores_notes_and_attachments() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let data = directory.path();
        let database = Database::open(data.join("gna.sqlite3")).expect("database opens");
        let kept = database
            .create_note("Da conservare", &document("contenuto originale"))
            .expect("note");
        database
            .add_attachment(&kept.id, "appunti.txt", b"testo allegato ricercabile", data)
            .expect("attachment");

        let backup = database.create_backup(BackupKind::Manual).expect("backup");
        assert_eq!(backup.kind, BackupKind::Manual);
        let verification = database
            .verify_backup_file(&data.join("backups").join(&backup.file_name))
            .expect("backup verifies");
        assert_eq!(verification.note_count, 1);
        assert_eq!(verification.attachment_count, 1);

        // Modifiche successive al backup.
        database
            .update_note(&kept.id, "Modificata", &document("nuovo"))
            .expect("update");
        database
            .create_note("Nota successiva", &empty_document())
            .expect("note");
        let attachments = database.list_attachments(&kept.id).expect("attachments");
        database
            .delete_attachment(&kept.id, &attachments[0].id, data)
            .expect("attachment removed");

        let safety = database.restore_backup(&backup.file_name).expect("restore");
        assert_eq!(safety.kind, BackupKind::PreRestore);
        let restored = database.get_note(&kept.id).expect("restored note");
        assert_eq!(restored.title, "Da conservare");
        assert_eq!(database.list_notes(10, 0).expect("notes").len(), 1);
        let attachments = database.list_attachments(&kept.id).expect("attachments");
        assert_eq!(attachments.len(), 1);
        database
            .verified_attachment_path(&kept.id, &attachments[0].id, data)
            .expect("restored attachment passes integrity check");
        let results = database
            .search_notes("ricercabile", "all", None, 10, 0)
            .expect("search");
        assert_eq!(results.len(), 1);

        // Il backup di sicurezza riporta allo stato precedente al ripristino.
        database
            .restore_backup(&safety.file_name)
            .expect("restore safety backup");
        assert_eq!(database.list_notes(10, 0).expect("notes").len(), 2);
        assert!(database.check_integrity().expect("integrity").ok);

        drop(database);
        let reopened = Database::open(data.join("gna.sqlite3")).expect("database reopens");
        assert_eq!(reopened.list_notes(10, 0).expect("notes").len(), 2);
    }

    #[test]
    fn tampered_backups_are_rejected_without_touching_live_data() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let data = directory.path();
        let database = Database::open(data.join("gna.sqlite3")).expect("database opens");
        database
            .create_note("Viva", &empty_document())
            .expect("note");
        let backup = database.create_backup(BackupKind::Manual).expect("backup");
        let path = data.join("backups").join(&backup.file_name);

        // Archivio con la stessa struttura ma database alterato.
        let source = fs::File::open(&path).expect("open backup");
        let mut archive = zip::ZipArchive::new(source).expect("zip");
        let tampered_path = data.join("tampered.zip");
        let mut writer = zip::ZipWriter::new(fs::File::create(&tampered_path).expect("create"));
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).expect("entry");
            let name = entry.name().expect("entry name").to_string();
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut bytes).expect("read");
            if name == "database/gna.sqlite3" {
                let last = bytes.len() - 1;
                bytes[last] ^= 0xff;
            }
            writer
                .start_file(name, super::zip_options())
                .expect("start");
            writer.write_all(&bytes).expect("write");
        }
        writer.finish().expect("finish");

        assert!(database.verify_backup_file(&tampered_path).is_err());
        assert!(database.restore_backup_file(&tampered_path).is_err());
        assert_eq!(database.list_notes(10, 0).expect("notes")[0].title, "Viva");
        assert!(database.restore_backup("../tampered.zip").is_err());
        fs::write(data.join("not-a-zip.zip"), b"plain").expect("write");
        assert!(
            database
                .verify_backup_file(&data.join("not-a-zip.zip"))
                .is_err()
        );
    }

    #[test]
    fn encrypted_copy_requires_password_and_restores() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let data = directory.path();
        let database = Database::open(data.join("gna.sqlite3")).expect("database opens");
        let kept = database
            .create_note("Segreta", &document("contenuto riservato"))
            .expect("note");
        database
            .add_attachment(&kept.id, "a.txt", b"allegato riservato", data)
            .expect("attachment");
        let backup = database.create_backup(BackupKind::Manual).expect("backup");
        let source = data.join("backups").join(&backup.file_name);
        let exported = directory.path().join("copia.zip.age");

        assert!(matches!(
            encrypt_backup_file(&source, &exported, "corta"),
            Err(StorageError::WeakBackupPassphrase(_))
        ));
        assert!(!exported.exists());
        encrypt_backup_file(&source, &exported, "una password lunga").expect("encrypted");
        assert!(is_encrypted_backup(&exported).expect("header"));
        assert!(!is_encrypted_backup(&source).expect("header"));
        let ciphertext = fs::read(&exported).expect("read");
        assert!(
            !ciphertext
                .windows(b"contenuto riservato".len())
                .any(|window| window == b"contenuto riservato")
        );
        assert!(
            encrypt_backup_file(&exported, &data.join("di-nuovo.age"), "una password lunga")
                .is_err()
        );

        assert!(matches!(
            database.verify_backup_file(&exported),
            Err(StorageError::BackupPassphraseRequired)
        ));
        assert!(matches!(
            database.restore_backup_file(&exported),
            Err(StorageError::BackupPassphraseRequired)
        ));
        assert!(matches!(
            database.decrypt_and_verify_backup(&exported, "password sbagliata"),
            Err(StorageError::WrongBackupPassphrase)
        ));
        assert_eq!(decrypted_leftovers(data), 0);

        database
            .update_note(&kept.id, "Modificata", &document("nuovo"))
            .expect("update");
        let decrypted = database
            .decrypt_and_verify_backup(&exported, "una password lunga")
            .expect("decrypts");
        assert_eq!(decrypted.verification.note_count, 1);
        assert_eq!(decrypted.verification.attachment_count, 1);
        assert_eq!(decrypted_leftovers(data), 1);
        database
            .restore_backup_file(decrypted.path())
            .expect("restore");
        drop(decrypted);
        assert_eq!(decrypted_leftovers(data), 0);
        assert_eq!(database.get_note(&kept.id).expect("note").title, "Segreta");
        assert!(database.check_integrity().expect("integrity").ok);
    }

    #[test]
    fn tampered_encrypted_copy_is_rejected_without_leftovers() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let data = directory.path();
        let database = Database::open(data.join("gna.sqlite3")).expect("database opens");
        database
            .create_note("Viva", &empty_document())
            .expect("note");
        let backup = database.create_backup(BackupKind::Manual).expect("backup");
        let exported = data.join("copia.age");
        encrypt_backup_file(
            &data.join("backups").join(&backup.file_name),
            &exported,
            "una password lunga",
        )
        .expect("encrypted");
        let mut bytes = fs::read(&exported).expect("read");
        let last = bytes.len() - 1;
        bytes[last] ^= 0x01;
        fs::write(&exported, &bytes).expect("tamper");
        let error = database
            .decrypt_and_verify_backup(&exported, "una password lunga")
            .expect_err("tampered file is rejected");
        assert!(matches!(error, StorageError::InvalidBackup(_)), "{error}");
        assert_eq!(decrypted_leftovers(data), 0);

        fs::write(data.join(".decrypted-residuo.zip"), b"x").expect("leftover");
        remove_decrypted_backups(data).expect("cleanup");
        assert_eq!(decrypted_leftovers(data), 0);
        assert!(data.join("gna.sqlite3").exists());
    }

    #[test]
    fn automatic_backups_respect_interval_and_retention() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let data = directory.path();
        let database = Database::open(data.join("gna.sqlite3")).expect("database opens");
        assert_eq!(
            database.backup_settings().expect("settings"),
            BackupSettings::default()
        );
        assert!(database.run_automatic_backup().expect("first").is_some());
        assert!(database.run_automatic_backup().expect("not due").is_none());

        assert!(
            database
                .update_backup_settings(BackupSettings {
                    auto_enabled: true,
                    interval_days: 0,
                    retention: 2
                })
                .is_err()
        );
        database
            .update_backup_settings(BackupSettings {
                auto_enabled: true,
                interval_days: 1,
                retention: 2,
            })
            .expect("settings saved");
        // Simula backup automatici più vecchi.
        for (index, day) in ["20200101T000000Z", "20200102T000000Z", "20200103T000000Z"]
            .iter()
            .enumerate()
        {
            fs::write(
                data.join("backups")
                    .join(format!("gna-auto-{day}-0000000{index}.zip")),
                b"old",
            )
            .expect("write");
        }
        database.run_automatic_backup().expect("retention applied");
        let automatic: Vec<_> = list_backups(data)
            .expect("list")
            .into_iter()
            .filter(|backup| backup.kind == BackupKind::Automatic)
            .collect();
        assert_eq!(automatic.len(), 2);
        assert!(
            automatic
                .iter()
                .all(|backup| !backup.created_at.starts_with("2020-01-01"))
        );

        database
            .update_backup_settings(BackupSettings {
                auto_enabled: false,
                interval_days: 1,
                retention: 2,
            })
            .expect("disabled");
        drop(database);
        let reopened = Database::open(data.join("gna.sqlite3")).expect("reopen");
        assert!(!reopened.backup_settings().expect("settings").auto_enabled);
        assert!(reopened.run_automatic_backup().expect("disabled").is_none());
    }

    #[test]
    fn integrity_check_reports_missing_and_corrupted_attachments() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let data = directory.path();
        let database = Database::open(data.join("gna.sqlite3")).expect("database opens");
        let note = database
            .create_note("Con allegati", &empty_document())
            .expect("note");
        database
            .add_attachment(&note.id, "a.txt", b"primo", data)
            .expect("first");
        database
            .add_attachment(&note.id, "b.txt", b"secondo", data)
            .expect("second");
        assert!(database.check_integrity().expect("report").ok);

        let mut stored: Vec<_> = fs::read_dir(data.join("attachments"))
            .expect("dir")
            .map(|entry| entry.expect("entry").path())
            .collect();
        stored.sort();
        fs::remove_file(&stored[0]).expect("remove");
        fs::write(&stored[1], b"altro!!").expect("corrupt");
        fs::write(data.join("attachments").join("orfano.txt"), b"x").expect("orphan");

        let report = database.check_integrity().expect("report");
        assert!(!report.ok);
        assert_eq!(report.attachment_issues.len(), 2);
        assert_eq!(report.orphan_files, 1);
        assert_eq!(report.sqlite_result, "ok");
        assert_eq!(database.rebuild_search_index().expect("rebuild"), 1);
    }
}
