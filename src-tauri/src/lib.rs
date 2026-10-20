mod ai_commands;
mod calendar_commands;
mod data_commands;
mod mcp_commands;
mod planner_commands;
mod sync_commands;
mod ui_commands;

use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use greatnote_core::{
    data_dir::{DATABASE_FILE, LegacyMigration, migrate_legacy_data_directory},
    domain::{
        note::{Attachment, Note, NoteDraft, NoteLink},
        organization::{Notebook, Space, Tag},
    },
    storage::{Database, SearchResult},
    trash::{EmptyTrashSummary, TrashItem, TrashKind},
};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

fn initialize_logging() {
    static LOGGING: OnceLock<()> = OnceLock::new();
    LOGGING.get_or_init(|| {
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn"));
        tracing_subscriber::fmt().with_env_filter(filter).init();
    });
}

/// Nome della cartella dati usata prima del cambio di nome in Greatnote.
const LEGACY_IDENTIFIER: &str = "dev.gna.notes";

/// Porta i dati dalla cartella del vecchio identificatore. Se lo spostamento
/// non riesce continua a usare la cartella precedente, così le note restano
/// raggiungibili.
fn resolve_data_directory(data_directory: &Path) -> PathBuf {
    let legacy = data_directory.with_file_name(LEGACY_IDENTIFIER);
    match migrate_legacy_data_directory(&legacy, data_directory) {
        Ok(LegacyMigration::Moved { set_aside }) => {
            tracing::info!(
                empty_database_set_aside = set_aside.is_some(),
                "dati spostati dalla cartella del vecchio identificatore"
            );
            data_directory.to_path_buf()
        }
        Ok(LegacyMigration::BothContainData) => {
            tracing::warn!(
                "anche la cartella del vecchio identificatore contiene note: non è stata modificata"
            );
            data_directory.to_path_buf()
        }
        Ok(LegacyMigration::NotNeeded) => data_directory.to_path_buf(),
        Err(error) => {
            tracing::error!(
                "dati precedenti non spostati, resta in uso la vecchia cartella: {error}"
            );
            legacy
        }
    }
}

fn create_data_directory(path: &Path) -> Result<(), std::io::Error> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

#[tauri::command]
fn list_notes(database: State<'_, Database>, limit: u32, offset: u32) -> Result<Vec<Note>, String> {
    database
        .list_notes(limit, offset)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn search_notes(
    database: State<'_, Database>,
    query: String,
    scope: String,
    scope_id: Option<String>,
    limit: u32,
    offset: u32,
) -> Result<Vec<SearchResult>, String> {
    database
        .search_notes(&query, &scope, scope_id.as_deref(), limit, offset)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_note(database: State<'_, Database>, note_id: String) -> Result<Note, String> {
    database
        .get_note(&note_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn add_note_attachment(
    database: State<'_, Database>,
    note_id: String,
    filename: String,
    data_base64: String,
) -> Result<Attachment, String> {
    if data_base64.len() > 35_000_000 {
        return Err("Il file codificato supera il limite consentito".to_owned());
    }
    let bytes = BASE64
        .decode(data_base64)
        .map_err(|error| format!("Codifica allegato non valida: {error}"))?;
    let data_directory = database
        .data_directory()
        .map_err(|error| error.to_string())?
        .to_path_buf();
    database
        .add_attachment(&note_id, &filename, &bytes, &data_directory)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_note_attachments(
    database: State<'_, Database>,
    note_id: String,
) -> Result<Vec<Attachment>, String> {
    database
        .list_attachments(&note_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_note_attachment(
    database: State<'_, Database>,
    note_id: String,
    attachment_id: String,
) -> Result<(), String> {
    let data_directory = database
        .data_directory()
        .map_err(|error| error.to_string())?
        .to_path_buf();
    database
        .delete_attachment(&note_id, &attachment_id, &data_directory)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn open_note_attachment(
    app: AppHandle,
    database: State<'_, Database>,
    note_id: String,
    attachment_id: String,
) -> Result<(), String> {
    let data_directory = database
        .data_directory()
        .map_err(|error| error.to_string())?
        .to_path_buf();
    let path = database
        .verified_attachment_path(&note_id, &attachment_id, &data_directory)
        .map_err(|error| error.to_string())?;
    app.opener()
        .open_path(path.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn create_note_link(
    database: State<'_, Database>,
    source_note_id: String,
    target_note_id: String,
    label: Option<String>,
) -> Result<(), String> {
    database
        .create_note_link(&source_note_id, &target_note_id, label.as_deref())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_note_links(
    database: State<'_, Database>,
    source_note_id: String,
) -> Result<Vec<NoteLink>, String> {
    database
        .list_note_links(&source_note_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_note_link(
    database: State<'_, Database>,
    source_note_id: String,
    target_note_id: String,
) -> Result<(), String> {
    database
        .delete_note_link(&source_note_id, &target_note_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn record_note_visit(database: State<'_, Database>, note_id: String) -> Result<(), String> {
    database
        .record_note_visit(&note_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_note_pinned(
    database: State<'_, Database>,
    note_id: String,
    pinned: bool,
) -> Result<Note, String> {
    database
        .set_note_pinned(&note_id, pinned)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_note_archived(
    database: State<'_, Database>,
    note_id: String,
    archived: bool,
) -> Result<Note, String> {
    database
        .set_note_archived(&note_id, archived)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn create_note(
    database: State<'_, Database>,
    title: String,
    content: serde_json::Value,
    notebook_id: Option<String>,
) -> Result<Note, String> {
    database
        .create_note_in_notebook(&title, &content, notebook_id.as_deref())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn update_note(
    database: State<'_, Database>,
    id: String,
    title: String,
    content: serde_json::Value,
    base_updated_at: Option<String>,
) -> Result<Note, String> {
    // `base_updated_at`: versione su cui lavorava l'editor (ADR 0017).
    database
        .update_note_from(&id, &title, &content, base_updated_at.as_deref())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn save_note_draft(
    database: State<'_, Database>,
    note_id: String,
    title: String,
    content: serde_json::Value,
) -> Result<NoteDraft, String> {
    database
        .save_note_draft(&note_id, &title, &content)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_note_drafts(database: State<'_, Database>) -> Result<Vec<NoteDraft>, String> {
    database
        .list_note_drafts()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn discard_note_draft(database: State<'_, Database>, note_id: String) -> Result<(), String> {
    database
        .discard_note_draft(&note_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn move_note(
    database: State<'_, Database>,
    note_id: String,
    notebook_id: Option<String>,
) -> Result<Note, String> {
    database
        .move_note(&note_id, notebook_id.as_deref())
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn trash_note(database: State<'_, Database>, note_id: String) -> Result<(), String> {
    database
        .trash_note(&note_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_note_permanently(database: State<'_, Database>, note_id: String) -> Result<(), String> {
    let data_directory = database
        .data_directory()
        .map_err(|error| error.to_string())?
        .to_path_buf();
    database
        .delete_note_permanently(&note_id, &data_directory)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn move_item_to_trash(
    database: State<'_, Database>,
    kind: TrashKind,
    id: String,
) -> Result<(), String> {
    database
        .move_item_to_trash(kind, &id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_trash_items(database: State<'_, Database>) -> Result<Vec<TrashItem>, String> {
    database
        .list_trash_items()
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn restore_trash_item(
    database: State<'_, Database>,
    kind: TrashKind,
    id: String,
) -> Result<(), String> {
    database
        .restore_trash_item(kind, &id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn delete_trash_item(
    database: State<'_, Database>,
    kind: TrashKind,
    id: String,
) -> Result<(), String> {
    database
        .delete_trash_item(kind, &id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn empty_trash(database: State<'_, Database>) -> Result<EmptyTrashSummary, String> {
    let data_directory = database
        .data_directory()
        .map_err(|error| error.to_string())?
        .to_path_buf();
    database
        .empty_trash(&data_directory)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn restore_note(database: State<'_, Database>, note_id: String) -> Result<Note, String> {
    database
        .restore_note(&note_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_deleted_notes(
    database: State<'_, Database>,
    limit: u32,
    offset: u32,
) -> Result<Vec<Note>, String> {
    database
        .list_deleted_notes(limit, offset)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_notes_by_tag(
    database: State<'_, Database>,
    tag_id: String,
    limit: u32,
    offset: u32,
) -> Result<Vec<Note>, String> {
    database
        .list_notes_by_tag(&tag_id, limit, offset)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_spaces(database: State<'_, Database>) -> Result<Vec<Space>, String> {
    database.list_spaces().map_err(|error| error.to_string())
}

#[tauri::command]
fn create_space(
    database: State<'_, Database>,
    name: String,
    description: String,
) -> Result<Space, String> {
    database
        .create_space(&name, &description)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn update_space(
    database: State<'_, Database>,
    id: String,
    name: String,
    description: String,
) -> Result<Space, String> {
    database
        .update_space(&id, &name, &description)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_notebooks(database: State<'_, Database>) -> Result<Vec<Notebook>, String> {
    database.list_notebooks().map_err(|error| error.to_string())
}

#[tauri::command]
fn create_notebook(
    database: State<'_, Database>,
    space_id: Option<String>,
    name: String,
    description: String,
) -> Result<Notebook, String> {
    database
        .create_notebook(space_id.as_deref(), &name, &description)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn update_notebook(
    database: State<'_, Database>,
    id: String,
    space_id: Option<String>,
    name: String,
    description: String,
) -> Result<Notebook, String> {
    database
        .update_notebook(id.as_str(), space_id.as_deref(), &name, &description)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_tags(database: State<'_, Database>) -> Result<Vec<Tag>, String> {
    database.list_tags().map_err(|error| error.to_string())
}

#[tauri::command]
fn create_tag(database: State<'_, Database>, name: String) -> Result<Tag, String> {
    database
        .create_tag(&name, None)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn update_tag(database: State<'_, Database>, id: String, name: String) -> Result<Tag, String> {
    database
        .update_tag(&id, &name, None)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_note_tags(database: State<'_, Database>, note_id: String) -> Result<Vec<Tag>, String> {
    database
        .list_note_tags(&note_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn set_note_tags(
    database: State<'_, Database>,
    note_id: String,
    tag_ids: Vec<String>,
) -> Result<(), String> {
    database
        .set_note_tags(&note_id, &tag_ids)
        .map_err(|error| error.to_string())
}

pub fn run() -> Result<(), Box<dyn Error>> {
    // `greatnote --mcp`: server MCP sullo standard input/output, senza
    // finestra (avviato dal client MCP).
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == mcp_commands::MCP_FLAG) {
        return mcp_commands::run_mcp(&args);
    }
    initialize_logging();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let data_directory = resolve_data_directory(&app.path().app_data_dir()?);
            create_data_directory(&data_directory)?;
            if let Err(error) = greatnote_core::backup::remove_decrypted_backups(&data_directory) {
                tracing::warn!("copie decifrate temporanee non rimosse: {error}");
            }
            let database = Database::open(data_directory.join(DATABASE_FILE))?;
            app.manage(database);
            app.manage(data_commands::PendingOperations::default());
            app.manage(planner_commands::UiLanguage::default());
            app.manage(ai_commands::AiJobs::default());
            app.manage(calendar_commands::CalendarState::default());
            app.manage(sync_commands::SyncRunState::default());
            data_commands::spawn_automatic_backup(app.handle());
            planner_commands::spawn_reminder_scheduler(app.handle());
            ai_commands::spawn_auto_indexer(app.handle());
            calendar_commands::spawn_calendar_sync(app.handle());
            sync_commands::spawn_sync_loop(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_notes,
            search_notes,
            get_note,
            record_note_visit,
            set_note_pinned,
            set_note_archived,
            add_note_attachment,
            list_note_attachments,
            delete_note_attachment,
            open_note_attachment,
            create_note_link,
            list_note_links,
            delete_note_link,
            create_note,
            update_note,
            save_note_draft,
            list_note_drafts,
            discard_note_draft,
            move_note,
            trash_note,
            restore_note,
            delete_note_permanently,
            empty_trash,
            move_item_to_trash,
            list_trash_items,
            restore_trash_item,
            delete_trash_item,
            list_deleted_notes,
            list_notes_by_tag,
            list_spaces,
            create_space,
            update_space,
            list_notebooks,
            create_notebook,
            update_notebook,
            list_tags,
            create_tag,
            update_tag,
            list_note_tags,
            set_note_tags,
            data_commands::export_note_file,
            data_commands::export_markdown_zip,
            data_commands::export_full_archive,
            data_commands::prepare_import,
            data_commands::commit_import,
            data_commands::cancel_import,
            data_commands::list_backups,
            data_commands::create_backup,
            data_commands::verify_backup,
            data_commands::restore_backup,
            data_commands::export_backup,
            data_commands::inspect_external_backup,
            data_commands::unlock_external_backup,
            data_commands::restore_external_backup,
            data_commands::cancel_external_backup,
            data_commands::get_backup_settings,
            data_commands::update_backup_settings,
            data_commands::check_integrity,
            data_commands::rebuild_search_index,
            data_commands::print_window,
            planner_commands::get_time_context,
            planner_commands::set_ui_language,
            planner_commands::list_tasks,
            planner_commands::task_counts,
            planner_commands::list_note_tasks,
            planner_commands::create_task,
            planner_commands::update_task,
            planner_commands::set_task_completed,
            planner_commands::get_task,
            planner_commands::list_events,
            planner_commands::list_tasks_due_between,
            planner_commands::create_event,
            planner_commands::update_event,
            planner_commands::get_event,
            planner_commands::create_reminder,
            planner_commands::list_reminders,
            planner_commands::list_reminder_history,
            planner_commands::clear_reminder_history,
            planner_commands::list_entity_reminders,
            planner_commands::snooze_reminder,
            planner_commands::dismiss_reminder,
            planner_commands::delete_reminder,
            ui_commands::list_note_revisions,
            ui_commands::get_note_revision,
            ui_commands::restore_note_revision,
            ui_commands::list_templates,
            ui_commands::create_template_from_note,
            ui_commands::rename_template,
            ui_commands::create_note_from_template,
            ui_commands::copy_note,
            ui_commands::list_files,
            ui_commands::sidebar_counts,
            ui_commands::note_info,
            ui_commands::get_ui_preferences,
            ui_commands::set_ui_preferences,
            ui_commands::set_zoom,
            ui_commands::toggle_fullscreen,
            ui_commands::minimize_window,
            ui_commands::set_window_theme,
            ui_commands::quit_app,
            ui_commands::read_clipboard_text,
            ui_commands::write_clipboard_text,
            ui_commands::copy_note_as_text,
            ui_commands::app_info,
            ui_commands::open_note_window,
            ai_commands::get_ai_status,
            ai_commands::update_ai_settings,
            ai_commands::set_ai_api_key,
            ai_commands::delete_ai_api_key,
            ai_commands::test_ai_connection,
            ai_commands::cancel_ai_request,
            ai_commands::ai_prepare_chat,
            ai_commands::ai_chat,
            ai_commands::ai_transform,
            ai_commands::ai_compare_revision,
            mcp_commands::get_mcp_status,
            calendar_commands::get_calendar_status,
            calendar_commands::set_calendar_clients,
            calendar_commands::connect_calendar_account,
            calendar_commands::cancel_calendar_connect,
            calendar_commands::disconnect_calendar_account,
            calendar_commands::set_external_calendar_selected,
            calendar_commands::sync_calendars_now,
            calendar_commands::create_external_event,
            calendar_commands::update_external_event,
            calendar_commands::delete_external_event,
            calendar_commands::open_external_event,
            calendar_commands::add_calendar_subscription,
            sync_commands::get_sync_status,
            sync_commands::set_sync_settings,
            sync_commands::sync_now,
            sync_commands::dismiss_sync_conflict,
            sync_commands::pick_sync_folder,
            sync_commands::connect_dropbox,
            sync_commands::cancel_dropbox_connect,
            sync_commands::disconnect_dropbox,
            calendar_commands::update_calendar_subscription,
            calendar_commands::remove_calendar_subscription,
            mcp_commands::set_mcp_enabled,
            ai_commands::ai_suggest_tags,
            ai_commands::ai_extract_tasks,
            ai_commands::ai_markdown_to_document,
            ai_commands::start_ai_indexing,
            ai_commands::cancel_ai_indexing,
            ai_commands::clear_ai_index,
            ai_commands::ai_semantic_search,
            ai_commands::list_ai_history,
            ai_commands::get_ai_conversation,
            ai_commands::delete_ai_history_entry,
            ai_commands::clear_ai_history,
            ai_commands::read_attachment_image,
            ai_commands::image_text_capabilities,
            ai_commands::read_image_text,
            ai_commands::start_image_text_all,
            ai_commands::cancel_image_text_all
        ])
        .run(tauri::generate_context!())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn falls_back_to_legacy_directory_when_migration_fails() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let legacy = directory.path().join(super::LEGACY_IDENTIFIER);
        let current = directory.path().join("dev.greatnote.notes");
        std::fs::create_dir(&legacy).expect("legacy directory");
        greatnote_core::storage::Database::open(legacy.join("gna.sqlite3"))
            .expect("legacy database");
        // Un file al posto della cartella nuova impedisce lo spostamento.
        std::fs::write(&current, b"non una cartella").expect("blocking file");
        assert_eq!(super::resolve_data_directory(&current), legacy);
        assert!(legacy.join("gna.sqlite3").exists());
    }

    #[test]
    fn data_directory_permissions_are_restricted_on_unix() {
        #[cfg(unix)]
        {
            use std::fs;
            use std::os::unix::fs::PermissionsExt;

            let directory = tempfile::tempdir().expect("temporary directory is created");
            let data_directory = directory.path().join("private-data");
            super::create_data_directory(&data_directory).expect("directory is created");
            let permissions = fs::metadata(&data_directory)
                .expect("directory metadata is available")
                .permissions();
            assert_eq!(permissions.mode() & 0o777, 0o700);
        }
    }
}
