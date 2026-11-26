//! Registrazione delle riunioni (ADR 0035): microfono della webview, audio
//! salvato man mano dal core (`greatnote_core::meeting`), esportazione in
//! WAV ed eliminazione. La trascrizione è in `ai_commands`.
//!
//! Il microfono si apre solo dopo un'azione esplicita: l'interfaccia chiama
//! [`allow_microphone`] subito prima di `getUserMedia`, e il gestore dei
//! permessi della webview concede il microfono solo in quel momento. La
//! fotocamera non viene mai concessa.

use std::sync::atomic::{AtomicBool, Ordering};

use base64::Engine as _;
use greatnote_core::{
    meeting::{self, Recording},
    storage::Database,
};
use tauri::{
    AppHandle, Manager, Runtime, State,
    webview::{PermissionKind, PermissionResponse, Webview},
};

use crate::file_access;

type CommandResult<T> = Result<T, String>;

/// Il microfono è stato chiesto dall'utente (pannello della registrazione
/// aperto).
static MICROPHONE_ALLOWED: AtomicBool = AtomicBool::new(false);

fn message(error: impl ToString) -> String {
    error.to_string()
}

fn data_directory(database: &Database) -> CommandResult<std::path::PathBuf> {
    database
        .data_directory()
        .map(std::path::Path::to_path_buf)
        .map_err(message)
}

/// Gestore dei permessi della webview: microfono solo se chiesto
/// dall'utente nella finestra principale, mai la fotocamera; il resto come
/// prima.
pub fn permission<R: Runtime>(webview: Webview<R>, kind: PermissionKind) -> PermissionResponse {
    match kind {
        PermissionKind::Microphone
            if webview.label() == "main" && MICROPHONE_ALLOWED.load(Ordering::SeqCst) =>
        {
            PermissionResponse::Allow
        }
        PermissionKind::Microphone | PermissionKind::Camera | PermissionKind::DisplayCapture => {
            PermissionResponse::Deny
        }
        _ => PermissionResponse::Default,
    }
}

/// Linux: WebKitGTK espone `getUserMedia` solo con l'impostazione attiva.
/// L'accesso resta comunque deciso da [`permission`].
pub fn enable_media_stream(app: &AppHandle) {
    #[cfg(target_os = "linux")]
    if let Some(window) = app.get_webview_window("main") {
        let result = window.with_webview(|webview| {
            use webkit2gtk::{SettingsExt, WebViewExt};
            if let Some(settings) = webview.inner().settings() {
                settings.set_enable_media_stream(true);
            }
        });
        if let Err(error) = result {
            tracing::warn!("microfono della webview non attivato: {error}");
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = app;
}

/// Consente il microfono finché il pannello della registrazione è aperto.
#[tauri::command]
pub fn allow_microphone(allowed: bool) {
    MICROPHONE_ALLOWED.store(allowed, Ordering::SeqCst);
    tracing::info!(allowed, "microfono per la registrazione");
}

#[tauri::command]
pub fn start_recording(
    database: State<'_, Database>,
    note_id: Option<String>,
) -> CommandResult<Recording> {
    let directory = data_directory(&database)?;
    let recording = meeting::start(&directory, note_id.as_deref()).map_err(message)?;
    tracing::info!("registrazione iniziata");
    Ok(recording)
}

/// Blocco di audio (PCM 16 bit, 16 kHz, mono, in base64). Restituisce la
/// durata registrata.
#[tauri::command]
pub fn append_recording(
    database: State<'_, Database>,
    id: String,
    data: String,
) -> CommandResult<f64> {
    if data.len() > meeting::MAX_CHUNK_BYTES * 4 / 3 + 4 {
        return Err(message(meeting::MeetingError::InvalidAudio));
    }
    let pcm = base64::engine::general_purpose::STANDARD
        .decode(data.as_bytes())
        .map_err(|_| message(meeting::MeetingError::InvalidAudio))?;
    let directory = data_directory(&database)?;
    meeting::append(&directory, &id, &pcm).map_err(message)
}

#[tauri::command]
pub fn list_recordings(database: State<'_, Database>) -> CommandResult<Vec<Recording>> {
    meeting::list(&data_directory(&database)?).map_err(message)
}

#[tauri::command]
pub fn delete_recording(database: State<'_, Database>, id: String) -> CommandResult<()> {
    meeting::delete(&data_directory(&database)?, &id).map_err(message)?;
    tracing::info!("registrazione eliminata");
    Ok(())
}

/// Salva l'audio in un file WAV scelto dall'utente.
#[tauri::command]
pub async fn export_recording(
    app: AppHandle,
    id: String,
    title: String,
    file_name: String,
) -> CommandResult<Option<String>> {
    tauri::async_runtime::spawn_blocking(move || {
        let directory = data_directory(&app.state::<Database>())?;
        let name = format!(
            "{}.wav",
            greatnote_core::transfer::sanitize_file_name(&file_name, "riunione")
        );
        let builder = crate::data_commands::dialog(&app, &title)
            .set_file_name(&name)
            .add_filter("WAV", &["wav"]);
        let Some(output) = file_access::pick_output(&app, builder, "wav")? else {
            return Ok(None);
        };
        meeting::wav_file(&directory, &id, output.path()).map_err(message)?;
        output.finish(&app).map(Some)
    })
    .await
    .map_err(message)?
}
