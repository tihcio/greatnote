//! Promemoria consegnati dal sistema operativo.
//!
//! Sul desktop i promemoria scattano dallo scheduler dell'app
//! (`planner_commands`), quindi solo mentre è aperta. Android può terminare
//! l'app in qualsiasi momento: lì ogni promemoria programmato viene affidato
//! al sistema come notifica a orario, che arriva anche ad app chiusa e viene
//! ripristinata dopo un riavvio del dispositivo (lo fa il plugin delle
//! notifiche).
//!
//! Il database resta l'unica fonte di verità: [`sync`] confronta i promemoria
//! programmati con quelli affidati al sistema e riallinea i secondi. Viene
//! chiamata dopo ogni modifica e a ogni giro dello scheduler, così copre
//! anche le modifiche indirette (attività completate, elementi nel cestino,
//! ripristino di un backup).

use tauri::AppHandle;

/// Stato del permesso di mostrare notifiche: `granted`, `denied` o `prompt`
/// (non ancora chiesto).
pub type Permission = &'static str;

#[cfg(not(target_os = "android"))]
mod platform {
    use super::*;

    /// Sul desktop le notifiche partono dallo scheduler dell'app.
    pub const DELIVERED_BY_SYSTEM: bool = false;

    pub fn setup(_app: &AppHandle) {}

    pub fn sync(_app: &AppHandle) {}

    pub fn sync_soon(_app: &AppHandle) {}

    pub fn permission(_app: &AppHandle) -> Permission {
        "granted"
    }

    pub fn request_permission(_app: &AppHandle) -> Permission {
        "granted"
    }
}

#[cfg(target_os = "android")]
mod platform {
    use std::sync::Mutex;

    use greatnote_core::{
        domain::planner::{Reminder, ReminderStatus},
        storage::Database,
        time,
    };
    use tauri::Manager;
    use tauri_plugin_notification::{
        Channel, Importance, NotificationExt, PermissionState, Schedule,
    };

    use super::*;
    use crate::planner_commands::UiLanguage;

    pub const DELIVERED_BY_SYSTEM: bool = true;

    const CHANNEL: &str = "reminders";
    /// Nome della risorsa drawable usata come icona piccola delle notifiche.
    const NOTIFICATION_ICON: &str = "ic_stat_notify";
    /// Verde dell'app (`--accent` in styles.css).
    const NOTIFICATION_ICON_COLOR: &str = "#128A55";
    /// Android limita gli allarmi per app: si affidano al sistema i
    /// promemoria più vicini, gli altri alla sincronizzazione successiva.
    const MAX_SCHEDULED: usize = 100;

    /// Notifica affidata al sistema: identificatore, istante (secondi Unix),
    /// titolo e testo.
    type Planned = (i32, i64, String, String);

    /// Ultimo insieme affidato al sistema, per non riprogrammare tutto
    /// quando nulla è cambiato.
    static LAST: Mutex<Option<Vec<Planned>>> = Mutex::new(None);

    /// Identificatore numerico stabile della notifica di un promemoria,
    /// ricavato dal suo UUID.
    pub fn notification_id(reminder_id: &str) -> i32 {
        let hex: String = reminder_id
            .chars()
            .filter(char::is_ascii_hexdigit)
            .take(8)
            .collect();
        let value = u32::from_str_radix(&hex, 16).unwrap_or(0);
        i32::try_from(value & 0x7fff_ffff).unwrap_or(0)
    }

    pub fn setup(app: &AppHandle) {
        let channel = Channel::builder(CHANNEL, "Promemoria")
            .description(greatnote_core::tr_str!(
                "Promemoria di note, attività ed eventi",
                "Reminders for notes, tasks, and events"
            ))
            .importance(Importance::High)
            .vibration(true)
            .build();
        if let Err(error) = app.notification().create_channel(channel) {
            tracing::warn!("canale delle notifiche non creato: {error}");
        }
    }

    fn text(reminder: &Reminder, english: bool) -> (String, String) {
        let title = if reminder.entity_title.is_empty() {
            if english { "Reminder" } else { "Promemoria" }.to_owned()
        } else {
            reminder.entity_title.clone()
        };
        let clock = reminder.local_time.get(11..16).unwrap_or_default();
        let body = if english {
            format!("Reminder for {clock}")
        } else {
            greatnote_core::tr!("Promemoria delle {clock}", "Reminders at {clock}")
        };
        (title, body)
    }

    fn planned(app: &AppHandle) -> Result<Vec<Planned>, String> {
        let english = app.state::<UiLanguage>().is_english();
        let now = time::now().as_second();
        let mut planned: Vec<Planned> = app
            .state::<Database>()
            .list_reminders()
            .map_err(|error| error.to_string())?
            .iter()
            .filter(|reminder| reminder.status == ReminderStatus::Pending)
            .filter_map(|reminder| {
                let at = time::parse_utc(&reminder.remind_at).ok()?.as_second();
                let (title, body) = text(reminder, english);
                (at > now).then(|| (notification_id(&reminder.id), at, title, body))
            })
            .collect();
        planned.sort_by_key(|item| (item.1, item.0));
        planned.truncate(MAX_SCHEDULED);
        Ok(planned)
    }

    pub fn sync(app: &AppHandle) {
        let planned = match planned(app) {
            Ok(planned) => planned,
            Err(error) => {
                tracing::warn!("promemoria da programmare non letti: {error}");
                return;
            }
        };
        let mut last = LAST
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if last.as_ref() == Some(&planned) {
            return;
        }
        let notifications = app.notification();
        // `cancel_all` del plugin 2.6.0 fallisce su Android (argomenti nulli):
        // si annullano una per una le notifiche ancora in attesa.
        let waiting: Vec<i32> = match notifications.pending() {
            Ok(pending) => pending
                .iter()
                .map(|notification| notification.id())
                .collect(),
            Err(error) => {
                tracing::warn!("notifiche programmate non lette: {error}");
                return;
            }
        };
        if !waiting.is_empty()
            && let Err(error) = notifications.cancel(waiting)
        {
            tracing::warn!("notifiche programmate non annullate: {error}");
            return;
        }
        let mut scheduled = 0_usize;
        for (id, at, title, body) in &planned {
            let Ok(date) = ::time::OffsetDateTime::from_unix_timestamp(*at) else {
                continue;
            };
            let result = notifications
                .builder()
                .id(*id)
                .channel_id(CHANNEL)
                // Icona piccola dell'app (res/drawable/ic_stat_notify.xml,
                // protetta dalla riduzione delle risorse in res/raw/keep.xml).
                // Il plugin 2.6.0 non accetta un'icona predefinita nella
                // configurazione: va indicata su ogni notifica.
                .icon(NOTIFICATION_ICON)
                .icon_color(NOTIFICATION_ICON_COLOR)
                .title(title)
                .body(body)
                .schedule(Schedule::At {
                    date,
                    repeating: false,
                    allow_while_idle: true,
                })
                .auto_cancel()
                .show();
            match result {
                Ok(()) => scheduled += 1,
                Err(error) => tracing::warn!("promemoria non programmato: {error}"),
            }
        }
        tracing::info!(scheduled, "promemoria affidati al sistema");
        *last = Some(planned);
    }

    /// Riallinea su un altro thread: i comandi non restano in attesa del
    /// sistema.
    pub fn sync_soon(app: &AppHandle) {
        let app = app.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("reminder-alarms".to_owned())
            .spawn(move || sync(&app))
        {
            tracing::warn!("promemoria non riallineati: {error}");
        }
    }

    fn describe(state: tauri_plugin_notification::Result<PermissionState>) -> Permission {
        match state {
            Ok(PermissionState::Granted) => "granted",
            Ok(PermissionState::Denied) => "denied",
            Ok(_) => "prompt",
            Err(error) => {
                tracing::warn!("permesso delle notifiche non letto: {error}");
                "prompt"
            }
        }
    }

    pub fn permission(app: &AppHandle) -> Permission {
        describe(app.notification().permission_state())
    }

    /// Mostra la richiesta di sistema (solo se non è già stata decisa).
    pub fn request_permission(app: &AppHandle) -> Permission {
        describe(app.notification().request_permission())
    }
}

pub use platform::{DELIVERED_BY_SYSTEM, permission, request_permission, setup, sync, sync_soon};
