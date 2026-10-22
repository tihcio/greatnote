//! Comandi per attività, promemoria e calendario, e scheduler dei promemoria.
//!
//! "Oggi" e i giorni del calendario sono calcolati nel fuso del sistema. Lo
//! scheduler controlla periodicamente i promemoria scaduti: i promemoria persi
//! mentre l'app era chiusa scattano al primo controllo dopo l'avvio. Non c'è
//! un servizio in background quando l'app è chiusa.

use std::{
    sync::{Mutex, OnceLock},
    thread,
    time::Duration,
};

use greatnote_core::{
    domain::planner::{
        CalendarEvent, EventInput, Reminder, ReminderEntity, SnoozePreset, Task, TaskCounts,
        TaskInput, TaskView,
    },
    storage::Database,
    time::{self, Zone},
};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
#[cfg(not(all(unix, not(target_os = "macos"))))]
use tauri_plugin_notification::NotificationExt;

type CommandResult<T> = Result<T, String>;

const CHECK_INTERVAL: Duration = Duration::from_secs(15);
const STARTUP_DELAY: Duration = Duration::from_secs(3);
/// Oltre questo ritardo il promemoria è scattato ad app chiusa o sospesa: con
/// l'app aperta il controllo avviene ogni 15 secondi.
const MISSED_AFTER_SECONDS: i64 = 60;
pub const REMINDERS_FIRED_EVENT: &str = "reminders-fired";

/// Lingua dell'interfaccia, usata per il testo delle notifiche di sistema.
#[derive(Default)]
pub struct UiLanguage(Mutex<Option<String>>);

impl UiLanguage {
    pub(crate) fn is_english(&self) -> bool {
        self.0
            .lock()
            .ok()
            .and_then(|language| language.clone())
            .is_some_and(|language| language == "en")
    }
}

fn to_message(error: impl ToString) -> String {
    error.to_string()
}

fn parse_date(value: &str) -> CommandResult<time::CivilDate> {
    time::parse_date(value).map_err(to_message)
}

fn today() -> time::CivilDate {
    time::today(time::now(), &Zone::system())
}

#[derive(Serialize)]
pub struct TimeContext {
    timezone: String,
    pub(crate) today: String,
}

#[tauri::command]
pub fn get_time_context() -> TimeContext {
    let zone = Zone::system();
    TimeContext {
        today: time::today(time::now(), &zone).to_string(),
        timezone: zone.name,
    }
}

#[tauri::command]
pub fn set_ui_language(language: State<'_, UiLanguage>, value: String) -> CommandResult<()> {
    let value = if value == "en" { "en" } else { "it" };
    *language
        .0
        .lock()
        .map_err(|_| "Lingua non disponibile".to_owned())? = Some(value.to_owned());
    Ok(())
}

// ---------------------------------------------------------------------------
// Attività

#[tauri::command]
pub fn list_tasks(
    database: State<'_, Database>,
    view: TaskView,
    search: Option<String>,
) -> CommandResult<Vec<Task>> {
    database
        .list_tasks(view, today(), search.as_deref())
        .map_err(to_message)
}

#[tauri::command]
pub fn task_counts(database: State<'_, Database>) -> CommandResult<TaskCounts> {
    database.task_counts(today()).map_err(to_message)
}

#[tauri::command]
pub fn list_note_tasks(database: State<'_, Database>, note_id: String) -> CommandResult<Vec<Task>> {
    database.list_note_tasks(&note_id).map_err(to_message)
}

#[tauri::command]
pub fn create_task(database: State<'_, Database>, input: TaskInput) -> CommandResult<Task> {
    database.create_task(&input).map_err(to_message)
}

#[tauri::command]
pub fn update_task(
    database: State<'_, Database>,
    id: String,
    input: TaskInput,
) -> CommandResult<Task> {
    database.update_task(&id, &input).map_err(to_message)
}

#[tauri::command]
pub fn set_task_completed(
    database: State<'_, Database>,
    id: String,
    completed: bool,
) -> CommandResult<Task> {
    database
        .set_task_completed(&id, completed, time::now(), &Zone::system())
        .map_err(to_message)
}

#[tauri::command]
pub fn get_task(database: State<'_, Database>, id: String) -> CommandResult<Task> {
    database.get_task(&id).map_err(to_message)
}

// ---------------------------------------------------------------------------
// Calendario

#[tauri::command]
pub fn list_events(
    database: State<'_, Database>,
    from: String,
    to: String,
) -> CommandResult<Vec<CalendarEvent>> {
    database
        .list_events(parse_date(&from)?, parse_date(&to)?, &Zone::system())
        .map_err(to_message)
}

#[tauri::command]
pub fn list_tasks_due_between(
    database: State<'_, Database>,
    from: String,
    to: String,
) -> CommandResult<Vec<Task>> {
    database
        .list_tasks_due_between(parse_date(&from)?, parse_date(&to)?)
        .map_err(to_message)
}

#[tauri::command]
pub fn create_event(
    database: State<'_, Database>,
    input: EventInput,
) -> CommandResult<CalendarEvent> {
    database.create_event(&input).map_err(to_message)
}

#[tauri::command]
pub fn update_event(
    database: State<'_, Database>,
    id: String,
    input: EventInput,
) -> CommandResult<CalendarEvent> {
    database.update_event(&id, &input).map_err(to_message)
}

#[tauri::command]
pub fn get_event(database: State<'_, Database>, id: String) -> CommandResult<CalendarEvent> {
    database.get_event(&id).map_err(to_message)
}

// ---------------------------------------------------------------------------
// Promemoria

#[tauri::command]
pub fn create_reminder(
    database: State<'_, Database>,
    entity_type: ReminderEntity,
    entity_id: String,
    local_time: String,
) -> CommandResult<Reminder> {
    database
        .create_reminder(entity_type, &entity_id, &local_time, None, time::now())
        .map_err(to_message)
}

#[tauri::command]
pub fn list_reminders(database: State<'_, Database>) -> CommandResult<Vec<Reminder>> {
    database.list_reminders().map_err(to_message)
}

#[tauri::command]
pub fn list_reminder_history(database: State<'_, Database>) -> CommandResult<Vec<Reminder>> {
    database.list_reminder_history(200).map_err(to_message)
}

#[tauri::command]
pub fn clear_reminder_history(database: State<'_, Database>) -> CommandResult<u64> {
    database.clear_reminder_history().map_err(to_message)
}

#[tauri::command]
pub fn list_entity_reminders(
    database: State<'_, Database>,
    entity_type: ReminderEntity,
    entity_id: String,
) -> CommandResult<Vec<Reminder>> {
    database
        .list_entity_reminders(entity_type, &entity_id)
        .map_err(to_message)
}

#[tauri::command]
pub fn snooze_reminder(
    database: State<'_, Database>,
    id: String,
    preset: SnoozePreset,
) -> CommandResult<Reminder> {
    database
        .snooze_reminder(&id, preset, time::now())
        .map_err(to_message)
}

#[tauri::command]
pub fn dismiss_reminder(database: State<'_, Database>, id: String) -> CommandResult<()> {
    database.dismiss_reminder(&id).map_err(to_message)
}

#[tauri::command]
pub fn delete_reminder(database: State<'_, Database>, id: String) -> CommandResult<()> {
    database.delete_reminder(&id).map_err(to_message)
}

// ---------------------------------------------------------------------------
// Scheduler

fn notification_body(reminder: &Reminder, english: bool, now: time::Instant) -> String {
    let zone = Zone::system();
    let local = time::parse_utc(&reminder.remind_at)
        .map(|instant| time::local_wall(instant, &zone))
        .unwrap_or_else(|_| reminder.local_time.clone());
    let clock = local.get(11..16).unwrap_or_default();
    let missed = time::parse_utc(&reminder.remind_at)
        .is_ok_and(|instant| now.as_second() - instant.as_second() > MISSED_AFTER_SECONDS);
    match (english, missed) {
        (false, false) => format!("Promemoria delle {clock}"),
        (false, true) => format!("Scaduto alle {clock}: perso mentre l'app era chiusa"),
        (true, false) => format!("Reminder for {clock}"),
        (true, true) => format!("Was due at {clock}: missed while the app was closed"),
    }
}

/// Invia una notifica di sistema. Il promemoria resta comunque visibile
/// nell'app come "da gestire" anche se la consegna fallisce.
fn send_notification(app: &AppHandle, title: &str, body: &str) -> Result<(), String> {
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = app;
        // Consegna sincrona via D-Bus: l'esito è noto e registrato. Senza
        // scadenza la notifica resta finché l'utente non la chiude.
        notify_rust::Notification::new()
            .appname("Greatnote")
            // Associa la notifica alla voce .desktop dell'app (icona e nome).
            .hint(notify_rust::Hint::DesktopEntry(
                "dev.greatnote.notes".to_owned(),
            ))
            .summary(title)
            .body(body)
            .icon("appointment-soon")
            .timeout(notify_rust::Timeout::Never)
            .show()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
    #[cfg(not(all(unix, not(target_os = "macos"))))]
    {
        app.notification()
            .builder()
            .title(title)
            .body(body)
            .show()
            .map_err(|error| error.to_string())
    }
}

fn notify(app: &AppHandle, reminders: &[Reminder], now: time::Instant) {
    let english = app.state::<UiLanguage>().is_english();
    for reminder in reminders {
        let title = if reminder.entity_title.is_empty() {
            if english { "Reminder" } else { "Promemoria" }.to_owned()
        } else {
            reminder.entity_title.clone()
        };
        let body = notification_body(reminder, english, now);
        match send_notification(app, &title, &body) {
            Ok(()) => tracing::info!(reminder = %reminder.id, "notifica di sistema inviata"),
            Err(error) => {
                tracing::warn!(reminder = %reminder.id, "notifica di sistema non inviata: {error}")
            }
        }
    }
}

fn pending_summary(count: usize, english: bool) -> (String, String) {
    if english {
        (
            "Greatnote".to_owned(),
            if count == 1 {
                "You have 1 reminder that needs attention".to_owned()
            } else {
                format!("You have {count} reminders that need attention")
            },
        )
    } else {
        (
            "Greatnote".to_owned(),
            if count == 1 {
                "Hai 1 promemoria da gestire".to_owned()
            } else {
                format!("Hai {count} promemoria da gestire")
            },
        )
    }
}

/// All'avvio ricorda i promemoria scattati in sessioni precedenti e non
/// ancora gestiti (quelli appena scattati hanno già una notifica propria).
fn notify_unhandled_at_startup(app: &AppHandle, just_fired: &[Reminder]) {
    let unhandled = match app.state::<Database>().list_reminders() {
        Ok(reminders) => reminders
            .iter()
            .filter(|reminder| {
                reminder.status == greatnote_core::domain::planner::ReminderStatus::Fired
                    && !just_fired.iter().any(|fired| fired.id == reminder.id)
            })
            .count(),
        Err(error) => {
            tracing::warn!("promemoria da gestire non letti: {error}");
            return;
        }
    };
    if unhandled == 0 {
        return;
    }
    let english = app.state::<UiLanguage>().is_english();
    let (title, body) = pending_summary(unhandled, english);
    if let Err(error) = send_notification(app, &title, &body) {
        tracing::warn!("riepilogo dei promemoria non inviato: {error}");
    }
}

/// Avvia il controllo periodico dei promemoria su un thread dedicato.
pub fn spawn_reminder_scheduler(app: &AppHandle) {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    let app = app.clone();
    let spawned = thread::Builder::new()
        .name("reminders".to_owned())
        .spawn(move || {
            // Lascia alla UI il tempo di comunicare la lingua delle notifiche.
            thread::sleep(STARTUP_DELAY);
            let mut first = true;
            loop {
                let now = time::now();
                let due = match app.state::<Database>().take_due_reminders(now) {
                    Ok(due) => due,
                    Err(error) => {
                        tracing::warn!("controllo dei promemoria non riuscito: {error}");
                        Vec::new()
                    }
                };
                if !due.is_empty() {
                    tracing::info!(count = due.len(), "promemoria scattati");
                    notify(&app, &due, now);
                    if let Err(error) = app.emit(REMINDERS_FIRED_EVENT, &due) {
                        tracing::warn!("evento promemoria non inviato alla UI: {error}");
                    }
                }
                if first {
                    notify_unhandled_at_startup(&app, &due);
                    first = false;
                }
                thread::sleep(CHECK_INTERVAL);
            }
        });
    if let Err(error) = spawned {
        tracing::error!("scheduler dei promemoria non avviato: {error}");
    }
}

#[cfg(test)]
mod tests {
    use greatnote_core::domain::planner::{Reminder, ReminderEntity, ReminderStatus};

    fn reminder(remind_at: &str) -> Reminder {
        Reminder {
            id: "r".to_owned(),
            entity_type: ReminderEntity::Note,
            entity_id: "n".to_owned(),
            remind_at: remind_at.to_owned(),
            timezone: "UTC".to_owned(),
            local_time: "2026-10-14T09:00".to_owned(),
            status: ReminderStatus::Fired,
            last_triggered_at: None,
            entity_title: "Nota".to_owned(),
            note_id: None,
            updated_at: "2026-10-14T09:00:00.000Z".to_owned(),
        }
    }

    #[test]
    fn notification_text_marks_missed_reminders() {
        let now = greatnote_core::time::parse_utc("2026-10-14T09:30:00Z").expect("now");
        let on_time = super::notification_body(&reminder("2026-10-14T09:29:00Z"), false, now);
        assert!(on_time.starts_with("Promemoria delle "), "{on_time}");
        assert!(!on_time.contains("perso"));
        let missed = super::notification_body(&reminder("2026-10-14T08:00:00Z"), true, now);
        assert!(missed.contains("missed"), "{missed}");
        // Un minuto e mezzo di ritardo: l'app era chiusa all'orario previsto.
        let late = super::notification_body(&reminder("2026-10-14T09:28:30Z"), false, now);
        assert!(late.starts_with("Scaduto alle "), "{late}");
        assert_eq!(
            super::pending_summary(1, false).1,
            "Hai 1 promemoria da gestire"
        );
        assert_eq!(
            super::pending_summary(3, true).1,
            "You have 3 reminders that need attention"
        );
    }
}
