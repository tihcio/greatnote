//! Aggiornamenti su Android e iOS: l'app si aggiorna dallo store o
//! installando un pacchetto nuovo, mai da sola (ADR 0023 vale solo per il
//! desktop). I comandi esistono per mantenere lo stesso contratto con
//! l'interfaccia, che legge `supported: false` e non mostra la sezione.

use serde::Serialize;
use tauri::AppHandle;

type CommandResult<T> = Result<T, String>;

fn not_available() -> String {
    greatnote_core::tr!(
        "Gli aggiornamenti dall'app non sono disponibili su questo dispositivo",
        "In-app updates are not available on this device"
    )
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum UpdatePhase {
    Idle,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub current_version: String,
    pub supported: bool,
    pub auto_update: bool,
    pub installs_on_exit: bool,
    pub last_check_at: Option<String>,
    pub phase: UpdatePhase,
}

#[tauri::command]
pub fn get_update_status(app: AppHandle) -> UpdateStatus {
    UpdateStatus {
        current_version: app.package_info().version.to_string(),
        supported: false,
        auto_update: false,
        installs_on_exit: false,
        last_check_at: None,
        phase: UpdatePhase::Idle,
    }
}

#[tauri::command]
pub fn check_for_updates() -> CommandResult<UpdateStatus> {
    Err(not_available())
}

#[tauri::command]
pub fn set_auto_update(enabled: bool) -> CommandResult<UpdateStatus> {
    let _ = enabled;
    Err(not_available())
}

#[tauri::command]
pub fn install_update_now() -> CommandResult<()> {
    Err(not_available())
}
