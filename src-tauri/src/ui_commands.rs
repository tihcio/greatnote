//! Comandi per menu e interfaccia: revisioni, modelli, copia di note, file,
//! contatori, informazioni, preferenze, finestra e appunti.
//!
//! Le operazioni di finestra e appunti passano dal processo Rust: il
//! frontend non riceve i permessi dei rispettivi plugin.

use greatnote_core::{
    domain::note::Note,
    library::{FileEntry, NoteInfo, NoteRevision, NoteRevisionDetail, SidebarCounts, Template},
    storage::{CURRENT_SCHEMA_VERSION, Database},
    transfer::ExportFormat,
};
use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Manager, State, Theme, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_plugin_opener::OpenerExt;

type CommandResult<T> = Result<T, String>;

const MIN_ZOOM: f64 = 0.5;
const MAX_ZOOM: f64 = 2.0;
/// Testo massimo letto dagli appunti per "Incolla" dal menu.
const MAX_CLIPBOARD_CHARS: usize = 1_000_000;

fn to_message(error: impl ToString) -> String {
    error.to_string()
}

fn data_directory(database: &Database) -> CommandResult<std::path::PathBuf> {
    database
        .data_directory()
        .map(std::path::Path::to_path_buf)
        .map_err(to_message)
}

// ---------------------------------------------------------------------------
// Collegamenti

/// Apre nel browser (o nel client di posta) un collegamento della nota, dal
/// menu contestuale dell'editor. Solo gli schemi ammessi nei documenti
/// (`http`, `https`, `mailto`): mai `file:` o altri gestori del sistema.
#[tauri::command]
pub fn open_note_link(app: AppHandle, href: String) -> CommandResult<()> {
    let href = href.trim();
    if !greatnote_core::content::is_safe_link(href) {
        return Err("Collegamento non valido o non sicuro".to_owned());
    }
    app.opener()
        .open_url(href, None::<&str>)
        .map_err(to_message)
}

// ---------------------------------------------------------------------------
// Revisioni e modelli

#[tauri::command]
pub fn list_note_revisions(
    database: State<'_, Database>,
    note_id: String,
) -> CommandResult<Vec<NoteRevision>> {
    database.list_note_revisions(&note_id).map_err(to_message)
}

/// Elimina la cronologia di una nota (offerta dopo aver cifrato un testo che
/// le revisioni conservano in chiaro).
#[tauri::command]
pub fn delete_note_revisions(
    database: State<'_, Database>,
    note_id: String,
) -> CommandResult<usize> {
    database.delete_note_revisions(&note_id).map_err(to_message)
}

#[tauri::command]
pub fn get_note_revision(
    database: State<'_, Database>,
    revision_id: String,
) -> CommandResult<NoteRevisionDetail> {
    database.get_note_revision(&revision_id).map_err(to_message)
}

#[tauri::command]
pub fn restore_note_revision(
    database: State<'_, Database>,
    revision_id: String,
) -> CommandResult<Note> {
    database
        .restore_note_revision(&revision_id)
        .map_err(to_message)
}

#[tauri::command]
pub fn list_templates(
    database: State<'_, Database>,
    language: String,
) -> CommandResult<Vec<Template>> {
    database
        .list_templates(language == "en")
        .map_err(to_message)
}

#[tauri::command]
pub fn create_template_from_note(
    database: State<'_, Database>,
    note_id: String,
    name: String,
) -> CommandResult<Template> {
    database
        .create_template_from_note(&note_id, &name)
        .map_err(to_message)
}

#[tauri::command]
pub fn rename_template(
    database: State<'_, Database>,
    id: String,
    name: String,
) -> CommandResult<Template> {
    database.rename_template(&id, &name).map_err(to_message)
}

#[tauri::command]
pub fn create_note_from_template(
    database: State<'_, Database>,
    template_id: String,
    notebook_id: Option<String>,
    language: String,
) -> CommandResult<Note> {
    database
        .create_note_from_template(&template_id, notebook_id.as_deref(), language == "en")
        .map_err(to_message)
}

// ---------------------------------------------------------------------------
// Copia, file, contatori, informazioni

/// `target`: `"same"` (duplica nello stesso taccuino), `"unfiled"` oppure
/// l'ID di un taccuino.
#[tauri::command]
pub fn copy_note(
    database: State<'_, Database>,
    note_id: String,
    target: String,
    title_suffix: Option<String>,
) -> CommandResult<Note> {
    let directory = data_directory(&database)?;
    let notebook = match target.as_str() {
        "same" => None,
        "unfiled" => Some(None),
        id => Some(Some(id)),
    };
    database
        .copy_note(&note_id, notebook, title_suffix.as_deref(), &directory)
        .map_err(to_message)
}

#[tauri::command]
pub fn list_files(
    database: State<'_, Database>,
    search: Option<String>,
    limit: u32,
    offset: u32,
) -> CommandResult<Vec<FileEntry>> {
    database
        .list_files(search.as_deref(), limit, offset)
        .map_err(to_message)
}

#[tauri::command]
pub fn sidebar_counts(database: State<'_, Database>) -> CommandResult<SidebarCounts> {
    database.sidebar_counts().map_err(to_message)
}

#[tauri::command]
pub fn note_info(database: State<'_, Database>, note_id: String) -> CommandResult<NoteInfo> {
    database.note_info(&note_id).map_err(to_message)
}

#[tauri::command]
pub fn get_ui_preferences(database: State<'_, Database>) -> CommandResult<Value> {
    database.ui_preferences().map_err(to_message)
}

#[tauri::command]
pub fn set_ui_preferences(
    database: State<'_, Database>,
    preferences: Value,
) -> CommandResult<Value> {
    database
        .set_ui_preferences(&preferences)
        .map_err(to_message)
}

// ---------------------------------------------------------------------------
// Finestra

#[tauri::command]
pub fn set_zoom(window: WebviewWindow, level: f64) -> CommandResult<f64> {
    if !level.is_finite() {
        return Err("Livello di zoom non valido".to_owned());
    }
    let level = level.clamp(MIN_ZOOM, MAX_ZOOM);
    window.set_zoom(level).map_err(to_message)?;
    Ok(level)
}

#[tauri::command]
pub fn toggle_fullscreen(window: WebviewWindow) -> CommandResult<bool> {
    let next = !window.is_fullscreen().map_err(to_message)?;
    window.set_fullscreen(next).map_err(to_message)?;
    Ok(next)
}

#[tauri::command]
pub fn minimize_window(window: WebviewWindow) -> CommandResult<()> {
    window.minimize().map_err(to_message)
}

/// `"light"`, `"dark"` oppure `"system"`: tema della barra del titolo e dei
/// controlli nativi, in accordo con quello dell'interfaccia.
#[tauri::command]
pub fn set_window_theme(window: WebviewWindow, theme: String) -> CommandResult<()> {
    let theme = match theme.as_str() {
        "light" => Some(Theme::Light),
        "dark" => Some(Theme::Dark),
        "system" => None,
        _ => return Err("Tema non valido".to_owned()),
    };
    window.set_theme(theme).map_err(to_message)
}

/// Chiude l'app. Il frontend salva le modifiche prima di chiamarlo.
#[tauri::command]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
}

// ---------------------------------------------------------------------------
// Appunti

#[tauri::command]
pub fn read_clipboard_text(app: AppHandle) -> CommandResult<String> {
    let text = app.clipboard().read_text().map_err(to_message)?;
    Ok(text.chars().take(MAX_CLIPBOARD_CHARS).collect())
}

#[tauri::command]
pub fn write_clipboard_text(app: AppHandle, text: String) -> CommandResult<()> {
    if text.chars().count() > MAX_CLIPBOARD_CHARS {
        return Err("Testo troppo lungo per gli appunti".to_owned());
    }
    app.clipboard().write_text(text).map_err(to_message)
}

/// Copia negli appunti il contenuto di una nota come Markdown o testo
/// ("Condividi" locale: nessun servizio esterno).
#[tauri::command]
pub fn copy_note_as_text(
    app: AppHandle,
    database: State<'_, Database>,
    note_id: String,
    format: ExportFormat,
    language: String,
) -> CommandResult<()> {
    let exported = database
        .export_note(&note_id, format, &language)
        .map_err(to_message)?;
    app.clipboard()
        .write_text(exported.contents)
        .map_err(to_message)
}

#[derive(Serialize)]
pub struct AppInfo {
    version: &'static str,
    schema_version: u32,
    data_directory: String,
    database_bytes: u64,
}

#[tauri::command]
pub fn app_info(database: State<'_, Database>) -> CommandResult<AppInfo> {
    let directory = data_directory(&database)?;
    let database_bytes = database
        .path()
        .and_then(|path| std::fs::metadata(path).ok())
        .map_or(0, |metadata| metadata.len());
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        schema_version: CURRENT_SCHEMA_VERSION,
        data_directory: directory.display().to_string(),
        database_bytes,
    })
}

/// Apre (o porta in primo piano) una finestra dedicata a una nota.
#[tauri::command]
pub fn open_note_window(app: AppHandle, note_id: String, title: String) -> CommandResult<()> {
    // L'ID finisce nell'etichetta e nell'URL della finestra: solo UUID.
    let id = uuid::Uuid::parse_str(&note_id).map_err(|_| "Nota non valida".to_owned())?;
    let label = format!("note-{id}");
    if let Some(window) = app.get_webview_window(&label) {
        return window.set_focus().map_err(to_message);
    }
    let title: String = title
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect();
    WebviewWindowBuilder::new(
        &app,
        label,
        WebviewUrl::App(format!("index.html#note={id}").into()),
    )
    .title(if title.trim().is_empty() {
        "Greatnote".to_owned()
    } else {
        title
    })
    .inner_size(760.0, 720.0)
    .min_inner_size(420.0, 360.0)
    .build()
    .map(|_| ())
    .map_err(to_message)
}
