//! Modalità in background del desktop (ADR 0031).
//!
//! Con la modalità attiva Greatnote resta in esecuzione con un'icona
//! nell'area di notifica: chiudere la finestra la nasconde (dopo che il
//! frontend ha salvato la bozza) e lo scheduler dei promemoria continua a
//! inviare le notifiche. Con l'avvio all'accesso l'app parte nascosta.
//! Una sola istanza alla volta (plugin single-instance): aprire di nuovo
//! Greatnote mostra la finestra di quella già attiva.

use greatnote_core::{background::BackgroundSettings, storage::Database};
use serde::Serialize;
#[cfg(desktop)]
use tauri::Emitter;
use tauri::{AppHandle, Manager};

type CommandResult<T> = Result<T, String>;

#[cfg(desktop)]
/// Argomento dell'avvio all'accesso: la finestra parte nascosta.
pub const BACKGROUND_FLAG: &str = "--background";

#[cfg(desktop)]
/// Evento per il frontend: salvare la bozza e chiudere l'app.
const QUIT_REQUESTED_EVENT: &str = "quit-requested";

#[derive(Debug, Clone, Serialize)]
pub struct BackgroundStatus {
    #[serde(flatten)]
    settings: BackgroundSettings,
    /// L'avvio all'accesso si può impostare da Greatnote (non nel pacchetto
    /// del Microsoft Store, dove lo gestisce Windows).
    autostart_supported: bool,
}

#[cfg(desktop)]
/// Vero se l'app è stata avviata all'accesso per restare in background.
pub fn started_in_background() -> bool {
    std::env::args().skip(1).any(|arg| arg == BACKGROUND_FLAG)
}

#[cfg(desktop)]
/// Mostra e porta in primo piano la finestra principale.
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        if let Err(error) = window.show() {
            tracing::warn!("finestra principale non mostrata: {error}");
        }
        let _ = window.set_focus();
    }
}

#[cfg(desktop)]
/// Chiede al frontend di salvare e chiudere; se non risponde entro pochi
/// secondi l'app si chiude comunque (le bozze restano nel database).
fn request_quit(app: &AppHandle) {
    if app.emit_to("main", QUIT_REQUESTED_EVENT, ()).is_err() {
        app.exit(0);
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(5));
        app.exit(0);
    });
}

#[cfg(desktop)]
mod tray {
    use tauri::{
        AppHandle,
        menu::{Menu, MenuItem},
        tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    };

    const TRAY_ID: &str = "greatnote";

    pub fn show(app: &AppHandle) -> Result<(), String> {
        if app.tray_by_id(TRAY_ID).is_some() {
            return Ok(());
        }
        let open = MenuItem::with_id(
            app,
            "tray-open",
            greatnote_core::tr!("Apri Greatnote", "Open Greatnote"),
            true,
            None::<&str>,
        )
        .map_err(|error| error.to_string())?;
        let quit = MenuItem::with_id(
            app,
            "tray-quit",
            greatnote_core::tr!("Esci da Greatnote", "Quit Greatnote"),
            true,
            None::<&str>,
        )
        .map_err(|error| error.to_string())?;
        let menu = Menu::with_items(app, &[&open, &quit]).map_err(|error| error.to_string())?;
        let mut builder = TrayIconBuilder::with_id(TRAY_ID)
            .tooltip("Greatnote")
            .menu(&menu)
            .show_menu_on_left_click(false)
            .on_menu_event(|app, event| match event.id().as_ref() {
                "tray-open" => super::show_main(app),
                "tray-quit" => super::request_quit(app),
                _ => {}
            })
            .on_tray_icon_event(|tray, event| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    super::show_main(tray.app_handle());
                }
            });
        if let Some(icon) = app.default_window_icon() {
            builder = builder.icon(icon.clone());
        }
        builder.build(app).map(|_| ()).map_err(|error| {
            tracing::warn!("icona nell'area di notifica non disponibile: {error}");
            greatnote_core::tr!(
                "L'area di notifica non è disponibile su questo sistema ({}). Su Linux serve la libreria libayatana-appindicator.",
                "The system tray is not available on this system ({}). On Linux the libayatana-appindicator library is required.",
                error
            )
        })
    }

    pub fn hide(app: &AppHandle) {
        let _ = app.remove_tray_by_id(TRAY_ID);
    }
}

#[cfg(target_os = "linux")]
mod autostart {
    use std::path::PathBuf;

    pub fn supported() -> bool {
        entry_path().is_some()
    }

    fn entry_path() -> Option<PathBuf> {
        dirs::config_dir().map(|dir| dir.join("autostart").join("dev.greatnote.notes.desktop"))
    }

    /// Programma da avviare: l'AppImage stessa (non il punto di montaggio
    /// temporaneo) o l'eseguibile installato.
    fn executable() -> Result<String, String> {
        if let Some(appimage) = std::env::var_os("APPIMAGE") {
            return Ok(appimage.to_string_lossy().into_owned());
        }
        std::env::current_exe()
            .map(|path| path.to_string_lossy().into_owned())
            .map_err(|error| error.to_string())
    }

    /// Argomento tra virgolette secondo la specifica dei file `.desktop`.
    fn quote(argument: &str) -> String {
        let mut quoted = String::from("\"");
        for character in argument.chars() {
            if matches!(character, '"' | '`' | '$' | '\\') {
                quoted.push('\\');
            }
            quoted.push(character);
        }
        quoted.push('"');
        quoted
    }

    pub fn entry(executable: &str) -> String {
        format!(
            "[Desktop Entry]\nType=Application\nName=Greatnote\nComment=Promemoria di Greatnote\n\
             Exec={} {}\nIcon=dev.greatnote.notes\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
            quote(executable).replace('%', "%%"),
            super::BACKGROUND_FLAG
        )
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        let path = entry_path().ok_or("cartella di configurazione non trovata")?;
        if !enabled {
            return match std::fs::remove_file(&path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    Err(error.to_string())
                }
                _ => Ok(()),
            };
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::write(&path, entry(&executable()?)).map_err(|error| error.to_string())
    }
}

#[cfg(windows)]
mod autostart {
    use windows_sys::Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS},
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW,
            RegOpenKeyExW, RegSetValueExW,
        },
    };

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "Greatnote";

    /// Nel pacchetto MSIX la voce del registro non vale e lo StartupTask del
    /// manifest non passa argomenti (la finestra si aprirebbe): non disponibile.
    pub fn supported() -> bool {
        !crate::msix::is_packaged()
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        let key_name = wide(RUN_KEY);
        let value_name = wide(VALUE_NAME);
        let mut key: HKEY = std::ptr::null_mut();
        // SAFETY: stringhe terminate da zero, chiave chiusa sotto.
        let opened = unsafe {
            RegOpenKeyExW(
                HKEY_CURRENT_USER,
                key_name.as_ptr(),
                0,
                KEY_SET_VALUE,
                &mut key,
            )
        };
        if opened != ERROR_SUCCESS {
            return Err(format!("registro non accessibile (errore {opened})"));
        }
        let result = if enabled {
            let executable = std::env::current_exe().map_err(|error| error.to_string())?;
            let command = wide(&format!(
                "\"{}\" {}",
                executable.display(),
                super::BACKGROUND_FLAG
            ));
            // SAFETY: `command` resta valido per la durata della chiamata e
            // la dimensione è in byte, terminatore compreso.
            unsafe {
                RegSetValueExW(
                    key,
                    value_name.as_ptr(),
                    0,
                    REG_SZ,
                    command.as_ptr().cast(),
                    (command.len() * 2) as u32,
                )
            }
        } else {
            // SAFETY: chiave aperta e nome terminato da zero.
            match unsafe { RegDeleteValueW(key, value_name.as_ptr()) } {
                ERROR_FILE_NOT_FOUND => ERROR_SUCCESS,
                other => other,
            }
        };
        // SAFETY: chiave aperta sopra.
        unsafe { RegCloseKey(key) };
        if result == ERROR_SUCCESS {
            Ok(())
        } else {
            Err(format!("registro non aggiornato (errore {result})"))
        }
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod autostart {
    pub fn supported() -> bool {
        false
    }

    pub fn set(enabled: bool) -> Result<(), String> {
        if enabled {
            Err("avvio all'accesso non disponibile su questo sistema".to_owned())
        } else {
            Ok(())
        }
    }
}

/// All'avvio: icona nell'area di notifica se la modalità è attiva, finestra
/// nascosta se l'app è partita all'accesso, voce di avvio aggiornata (per
/// esempio se l'AppImage è stata spostata).
#[cfg(desktop)]
pub fn setup(app: &AppHandle) {
    let settings = match app.state::<Database>().background_settings() {
        Ok(settings) => settings,
        Err(error) => {
            tracing::warn!("preferenze della modalità in background non lette: {error}");
            return;
        }
    };
    if !settings.enabled {
        return;
    }
    if tray::show(app).is_err() {
        // Senza icona la finestra nascosta non si potrebbe riaprire dall'area
        // di notifica: resta visibile (la si riapre comunque rilanciando l'app).
        return;
    }
    if settings.autostart
        && autostart::supported()
        && let Err(error) = autostart::set(true)
    {
        tracing::warn!("voce di avvio all'accesso non aggiornata: {error}");
    }
    if started_in_background()
        && let Some(window) = app.get_webview_window("main")
    {
        let _ = window.hide();
    }
}

#[tauri::command]
pub fn get_background_settings(app: AppHandle) -> CommandResult<BackgroundStatus> {
    let settings = app
        .state::<Database>()
        .background_settings()
        .map_err(|error| error.to_string())?;
    Ok(BackgroundStatus {
        settings,
        autostart_supported: autostart::supported(),
    })
}

/// Applica e salva le preferenze. L'icona viene creata prima di salvare:
/// se l'area di notifica manca, la modalità resta spenta e l'errore lo dice.
#[tauri::command]
pub fn set_background_settings(
    app: AppHandle,
    settings: BackgroundSettings,
) -> CommandResult<BackgroundStatus> {
    // Su Android e iOS la modalità non esiste: il sistema consegna già i
    // promemoria ad app chiusa (reminder_alarms.rs).
    #[cfg(mobile)]
    let mut settings = {
        let _ = settings;
        BackgroundSettings::default()
    };
    #[cfg(desktop)]
    let mut settings = settings.normalized();
    #[cfg(desktop)]
    {
        if settings.enabled {
            tray::show(&app)?;
        } else {
            tray::hide(&app);
        }
    }
    if !autostart::supported() {
        settings.autostart = false;
    }
    autostart::set(settings.autostart)?;
    let settings = app
        .state::<Database>()
        .set_background_settings(&settings)
        .map_err(|error| error.to_string())?;
    Ok(BackgroundStatus {
        settings,
        autostart_supported: autostart::supported(),
    })
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn autostart_entry_quotes_the_path_and_starts_hidden() {
        let entry = super::autostart::entry("/home/a b/Greatnote \"x\" $HOME.AppImage");
        assert!(
            entry
                .contains("Exec=\"/home/a b/Greatnote \\\"x\\\" \\$HOME.AppImage\" --background\n"),
            "{entry}"
        );
        assert!(entry.starts_with("[Desktop Entry]\nType=Application\n"));
    }
}
