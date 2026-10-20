//! Comandi per import/export, backup e integrità.
//!
//! I dialog nativi sono aperti dal lato Rust: il frontend non fornisce mai
//! percorsi del filesystem e non riceve permessi del plugin dialog. Le
//! operazioni pesanti girano su un thread bloccante, fuori dal thread UI.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};

use greatnote_core::{
    backup::{
        BackupInfo, BackupKind, BackupSettings, BackupVerification, DecryptedBackup,
        IntegrityReport, encrypt_backup_file, is_encrypted_backup,
    },
    storage::Database,
    transfer::{
        ExportFormat, ExportSummary, ImportDestination, ImportPlan, ImportPreview, ImportReport,
        copy_file_atomically, plan_archive_import, plan_files_import, plan_folder_import,
        sanitize_file_name, write_file_atomically,
    },
};
use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_dialog::{DialogExt, FileDialogBuilder, FilePath};

type CommandResult<T> = Result<T, String>;

/// Operazioni in attesa di conferma: un solo piano d'importazione e un solo
/// backup esterno alla volta, per limitare la memoria trattenuta.
#[derive(Default)]
pub struct PendingOperations {
    import: Mutex<Option<(String, ImportPlan)>>,
    external_backup: Mutex<Option<PendingBackup>>,
}

/// Backup esterno selezionato. Se cifrato, `decrypted` contiene la copia in
/// chiaro verificata, eliminata automaticamente quando il valore viene
/// scartato (annullamento, nuova selezione o fine del ripristino).
struct PendingBackup {
    token: String,
    path: PathBuf,
    decrypted: Option<DecryptedBackup>,
}

#[derive(Serialize)]
pub struct PreparedImport {
    token: String,
    preview: ImportPreview,
}

#[derive(Serialize)]
pub struct ExternalBackup {
    token: String,
    file_name: String,
    encrypted: bool,
    /// Assente finché un backup cifrato non viene sbloccato con la password.
    verification: Option<BackupVerification>,
}

fn pending_backup(
    pending: &PendingOperations,
) -> CommandResult<std::sync::MutexGuard<'_, Option<PendingBackup>>> {
    pending
        .external_backup
        .lock()
        .map_err(|_| "Stato del ripristino non disponibile".to_owned())
}

pub(crate) async fn blocking<T, F>(app: AppHandle, task: F) -> CommandResult<T>
where
    T: Send + 'static,
    F: FnOnce(&AppHandle) -> CommandResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(move || task(&app))
        .await
        .map_err(|error| error.to_string())?
}

fn dialog(app: &AppHandle, title: &str) -> FileDialogBuilder<tauri::Wry> {
    let mut builder = app.dialog().file().set_title(title);
    if let Some(window) = app.get_webview_window("main") {
        builder = builder.set_parent(&window);
    }
    builder
}

fn into_path(file: Option<FilePath>) -> CommandResult<Option<PathBuf>> {
    file.map(|file| file.into_path().map_err(|error| error.to_string()))
        .transpose()
}

fn with_extension(path: PathBuf, extension: &str) -> PathBuf {
    let matches = path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension));
    if matches {
        path
    } else {
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        name.push(".");
        name.push(extension);
        path.with_file_name(name)
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn token() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[tauri::command]
pub async fn export_note_file(
    app: AppHandle,
    note_id: String,
    format: ExportFormat,
    language: String,
    title: String,
) -> CommandResult<Option<String>> {
    blocking(app, move |app| {
        let database = app.state::<Database>();
        let exported = database
            .export_note(&note_id, format, &language)
            .map_err(|error| error.to_string())?;
        let (filter, extension) = match format {
            ExportFormat::Markdown => ("Markdown", "md"),
            ExportFormat::Html => ("HTML", "html"),
            ExportFormat::Text => ("Text", "txt"),
        };
        let Some(path) = into_path(
            dialog(app, &title)
                .set_file_name(&exported.file_name)
                .add_filter(filter, &[extension])
                .blocking_save_file(),
        )?
        else {
            return Ok(None);
        };
        let path = with_extension(path, extension);
        write_file_atomically(&path, exported.contents.as_bytes())
            .map_err(|error| error.to_string())?;
        Ok(Some(file_name(&path)))
    })
    .await
}

#[tauri::command]
pub async fn export_markdown_zip(
    app: AppHandle,
    notebook_id: Option<String>,
    default_name: String,
    unfiled_folder: String,
    title: String,
) -> CommandResult<Option<ExportSummary>> {
    blocking(app, move |app| {
        let Some(path) = into_path(
            dialog(app, &title)
                .set_file_name(format!("{}.zip", sanitize_file_name(&default_name, "note")))
                .add_filter("ZIP", &["zip"])
                .blocking_save_file(),
        )?
        else {
            return Ok(None);
        };
        app.state::<Database>()
            .export_markdown_zip(
                notebook_id.as_deref(),
                &unfiled_folder,
                &with_extension(path, "zip"),
            )
            .map(Some)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn export_full_archive(
    app: AppHandle,
    default_name: String,
    title: String,
) -> CommandResult<Option<ExportSummary>> {
    blocking(app, move |app| {
        let Some(path) = into_path(
            dialog(app, &title)
                .set_file_name(format!(
                    "{}.zip",
                    sanitize_file_name(&default_name, "greatnote")
                ))
                .add_filter("ZIP", &["zip"])
                .blocking_save_file(),
        )?
        else {
            return Ok(None);
        };
        app.state::<Database>()
            .export_archive(&with_extension(path, "zip"))
            .map(Some)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn prepare_import(
    app: AppHandle,
    source: String,
    title: String,
) -> CommandResult<Option<PreparedImport>> {
    blocking(app, move |app| {
        let builder = dialog(app, &title);
        let plan = match source.as_str() {
            "files" => {
                let Some(files) = builder
                    .add_filter(
                        "Markdown, HTML, testo",
                        &["md", "markdown", "html", "htm", "txt"],
                    )
                    .blocking_pick_files()
                else {
                    return Ok(None);
                };
                let paths = files
                    .into_iter()
                    .map(|file| file.into_path().map_err(|error| error.to_string()))
                    .collect::<CommandResult<Vec<_>>>()?;
                plan_files_import(&paths)
            }
            "folder" => {
                let Some(folder) = into_path(builder.blocking_pick_folder())? else {
                    return Ok(None);
                };
                plan_folder_import(&folder)
            }
            "archive" => {
                let Some(file) =
                    into_path(builder.add_filter("ZIP", &["zip"]).blocking_pick_file())?
                else {
                    return Ok(None);
                };
                plan_archive_import(&file)
            }
            "evernote" => {
                let Some(files) = builder
                    .add_filter("Evernote ENEX", &["enex"])
                    .blocking_pick_files()
                else {
                    return Ok(None);
                };
                let paths = files
                    .into_iter()
                    .map(|file| file.into_path().map_err(|error| error.to_string()))
                    .collect::<CommandResult<Vec<_>>>()?;
                greatnote_core::external_import::enex::plan_enex_import(&paths)
            }
            "joplin" => {
                let Some(file) = into_path(
                    builder
                        .add_filter("Joplin JEX", &["jex"])
                        .blocking_pick_file(),
                )?
                else {
                    return Ok(None);
                };
                greatnote_core::external_import::joplin::plan_jex_import(&file)
            }
            "obsidian" => {
                let Some(folder) = into_path(builder.blocking_pick_folder())? else {
                    return Ok(None);
                };
                greatnote_core::external_import::obsidian::plan_obsidian_import(&folder)
            }
            _ => return Err("Origine dell'importazione non valida".to_owned()),
        }
        .map_err(|error| error.to_string())?;
        let preview = app
            .state::<Database>()
            .preview_import(&plan)
            .map_err(|error| error.to_string())?;
        let token = token();
        let pending = app.state::<PendingOperations>();
        *pending
            .import
            .lock()
            .map_err(|_| "Stato dell'importazione non disponibile".to_owned())? =
            Some((token.clone(), plan));
        Ok(Some(PreparedImport { token, preview }))
    })
    .await
}

#[tauri::command]
pub async fn commit_import(
    app: AppHandle,
    token: String,
    destination: ImportDestination,
) -> CommandResult<ImportReport> {
    blocking(app, move |app| {
        let pending = app.state::<PendingOperations>();
        let plan = {
            let mut guard = pending
                .import
                .lock()
                .map_err(|_| "Stato dell'importazione non disponibile".to_owned())?;
            match guard.take() {
                Some((stored, plan)) if stored == token => plan,
                other => {
                    *guard = other;
                    return Err(
                        "L'anteprima dell'importazione è scaduta: ripeti la selezione".to_owned(),
                    );
                }
            }
        };
        app.state::<Database>()
            .commit_import(&plan, &destination)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub fn cancel_import(pending: State<'_, PendingOperations>, token: String) -> CommandResult<()> {
    let mut guard = pending
        .import
        .lock()
        .map_err(|_| "Stato dell'importazione non disponibile".to_owned())?;
    if guard.as_ref().is_some_and(|(stored, _)| *stored == token) {
        *guard = None;
    }
    Ok(())
}

#[tauri::command]
pub async fn list_backups(app: AppHandle) -> CommandResult<Vec<BackupInfo>> {
    blocking(app, |app| {
        app.state::<Database>()
            .list_backups()
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn create_backup(app: AppHandle) -> CommandResult<BackupInfo> {
    blocking(app, |app| {
        app.state::<Database>()
            .create_backup(BackupKind::Manual)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn verify_backup(app: AppHandle, file_name: String) -> CommandResult<BackupVerification> {
    blocking(app, move |app| {
        let database = app.state::<Database>();
        let path = greatnote_core::backup::backup_path(
            database
                .data_directory()
                .map_err(|error| error.to_string())?,
            &file_name,
        )
        .map_err(|error| error.to_string())?;
        database
            .verify_backup_file(&path)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn restore_backup(app: AppHandle, file_name: String) -> CommandResult<BackupInfo> {
    blocking(app, move |app| {
        app.state::<Database>()
            .restore_backup(&file_name)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn export_backup(
    app: AppHandle,
    file_name: String,
    title: String,
    passphrase: Option<String>,
) -> CommandResult<Option<String>> {
    blocking(app, move |app| {
        let database = app.state::<Database>();
        let source = greatnote_core::backup::backup_path(
            database
                .data_directory()
                .map_err(|error| error.to_string())?,
            &file_name,
        )
        .map_err(|error| error.to_string())?;
        // Copia cifrata: `.zip.age`, decifrabile anche con lo strumento age.
        let (suggested, filter, extension) = match passphrase {
            Some(_) => (format!("{file_name}.age"), "age", "age"),
            None => (file_name.clone(), "ZIP", "zip"),
        };
        let Some(path) = into_path(
            dialog(app, &title)
                .set_file_name(&suggested)
                .add_filter(filter, &[extension])
                .blocking_save_file(),
        )?
        else {
            return Ok(None);
        };
        let path = with_extension(path, extension);
        match passphrase {
            Some(passphrase) => encrypt_backup_file(&source, &path, &passphrase),
            None => copy_file_atomically(&source, &path),
        }
        .map_err(|error| error.to_string())?;
        Ok(Some(self::file_name(&path)))
    })
    .await
}

#[tauri::command]
pub async fn inspect_external_backup(
    app: AppHandle,
    title: String,
) -> CommandResult<Option<ExternalBackup>> {
    blocking(app, move |app| {
        let Some(path) = into_path(
            dialog(app, &title)
                .add_filter("ZIP, age", &["zip", "age"])
                .blocking_pick_file(),
        )?
        else {
            return Ok(None);
        };
        // Una selezione precedente (e la sua eventuale copia decifrata) viene
        // scartata subito.
        pending_backup(&app.state::<PendingOperations>())?.take();
        let encrypted = is_encrypted_backup(&path).map_err(|error| error.to_string())?;
        let verification = if encrypted {
            None
        } else {
            Some(
                app.state::<Database>()
                    .verify_backup_file(&path)
                    .map_err(|error| error.to_string())?,
            )
        };
        let token = token();
        *pending_backup(&app.state::<PendingOperations>())? = Some(PendingBackup {
            token: token.clone(),
            path: path.clone(),
            decrypted: None,
        });
        Ok(Some(ExternalBackup {
            token,
            file_name: file_name(&path),
            encrypted,
            verification,
        }))
    })
    .await
}

/// Decifra e verifica il backup esterno selezionato. La password non viene
/// conservata: resta in memoria solo la copia decifrata, nella cartella
/// privata dell'app, fino al ripristino o all'annullamento.
#[tauri::command]
pub async fn unlock_external_backup(
    app: AppHandle,
    token: String,
    passphrase: String,
) -> CommandResult<ExternalBackup> {
    blocking(app, move |app| {
        let path = match pending_backup(&app.state::<PendingOperations>())?.as_ref() {
            Some(pending) if pending.token == token => pending.path.clone(),
            _ => return Err("Seleziona di nuovo il file di backup".to_owned()),
        };
        let decrypted = app
            .state::<Database>()
            .decrypt_and_verify_backup(&path, &passphrase)
            .map_err(|error| error.to_string())?;
        let verification = decrypted.verification.clone();
        let operations = app.state::<PendingOperations>();
        let mut guard = pending_backup(&operations)?;
        match guard.as_mut() {
            Some(pending) if pending.token == token => pending.decrypted = Some(decrypted),
            // Selezione cambiata nel frattempo: la copia viene eliminata.
            _ => return Err("Seleziona di nuovo il file di backup".to_owned()),
        }
        Ok(ExternalBackup {
            token,
            file_name: file_name(&path),
            encrypted: true,
            verification: Some(verification),
        })
    })
    .await
}

#[tauri::command]
pub async fn restore_external_backup(app: AppHandle, token: String) -> CommandResult<BackupInfo> {
    blocking(app, move |app| {
        let operations = app.state::<PendingOperations>();
        let pending = {
            let mut guard = pending_backup(&operations)?;
            match guard.take() {
                Some(pending) if pending.token == token => pending,
                other => {
                    *guard = other;
                    return Err("Seleziona di nuovo il file di backup".to_owned());
                }
            }
        };
        // Per un backup cifrato si ripristina la copia decifrata e verificata;
        // `pending` la elimina quando esce dallo scope.
        let source = match &pending.decrypted {
            Some(decrypted) => decrypted.path(),
            None => pending.path.as_path(),
        };
        app.state::<Database>()
            .restore_backup_file(source)
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub fn cancel_external_backup(
    pending: State<'_, PendingOperations>,
    token: String,
) -> CommandResult<()> {
    let mut guard = pending_backup(&pending)?;
    if guard.as_ref().is_some_and(|stored| stored.token == token) {
        *guard = None;
    }
    Ok(())
}

#[tauri::command]
pub fn get_backup_settings(database: State<'_, Database>) -> CommandResult<BackupSettings> {
    database
        .backup_settings()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_backup_settings(
    database: State<'_, Database>,
    settings: BackupSettings,
) -> CommandResult<BackupSettings> {
    database
        .update_backup_settings(settings)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn check_integrity(app: AppHandle) -> CommandResult<IntegrityReport> {
    blocking(app, |app| {
        app.state::<Database>()
            .check_integrity()
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
pub async fn rebuild_search_index(app: AppHandle) -> CommandResult<u64> {
    blocking(app, |app| {
        app.state::<Database>()
            .rebuild_search_index()
            .map_err(|error| error.to_string())
    })
    .await
}

/// Apre il dialog di stampa di sistema (consente anche il salvataggio PDF).
#[tauri::command]
pub fn print_window(window: WebviewWindow) -> CommandResult<()> {
    window.print().map_err(|error| error.to_string())
}

/// Backup automatico all'avvio, fuori dal thread principale.
pub fn spawn_automatic_backup(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(
        move || match app.state::<Database>().run_automatic_backup() {
            Ok(Some(backup)) => {
                tracing::info!(file = %backup.file_name, "backup automatico creato")
            }
            Ok(None) => {}
            Err(error) => tracing::warn!("backup automatico non riuscito: {error}"),
        },
    );
}
