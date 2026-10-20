//! Attività, promemoria ed eventi del calendario locale.
//!
//! Le viste che dipendono da "oggi" (oggi, in ritardo, prossime) ricevono la
//! data civile già calcolata nel fuso dell'utente; i promemoria ricevono
//! l'istante corrente. Così la logica è deterministica e testabile con fusi e
//! cambi d'ora arbitrari.

use jiff::{Timestamp, civil::Date};
use rusqlite::{Connection, ErrorCode, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::planner::{
        CalendarEvent, EventInput, Reminder, ReminderEntity, ReminderStatus, SnoozePreset, Task,
        TaskCounts, TaskInput, TaskStatus, TaskView,
    },
    storage::{Database, StorageError},
    time::{self, Zone},
};

const MAX_TITLE_CHARS: usize = 512;
const MAX_DESCRIPTION_CHARS: usize = 8192;
const MAX_SEARCH_CHARS: usize = 200;
const MAX_TASKS: u32 = 500;
const MAX_EVENT_DAYS: i64 = 366;
const MAX_RANGE_DAYS: i64 = 400;
/// Tolleranza per un promemoria impostato "adesso".
const PAST_TOLERANCE_MINUTES: i64 = 1;

const NOW_SQL: &str = "strftime('%Y-%m-%dT%H:%M:%fZ', 'now')";

const TASK_COLUMNS: &str = "tasks.id, tasks.note_id,
    (SELECT notes.title FROM notes WHERE notes.id = tasks.note_id AND notes.deleted_at IS NULL),
    tasks.title, tasks.description, tasks.status, tasks.priority, tasks.due_date,
    tasks.completed_at, tasks.created_at, tasks.updated_at,
    (SELECT MIN(reminders.remind_at) FROM reminders
     WHERE reminders.entity_type = 'task' AND reminders.entity_id = tasks.id
       AND reminders.status != 'done'),
    tasks.flagged";

const EVENT_COLUMNS: &str = "events.id, events.note_id,
    (SELECT notes.title FROM notes WHERE notes.id = events.note_id AND notes.deleted_at IS NULL),
    events.title, events.description, events.all_day, events.start_at, events.end_at,
    events.start_date, events.end_date, events.timezone, events.created_at, events.updated_at,
    events.external_calendar_id, events.calendar_provider, events.read_only, events.web_link,
    events.subscription_id";

/// Colonne di un promemoria con titolo e nota dell'entità. Usa l'alias `r`.
const REMINDER_COLUMNS: &str = "r.id, r.entity_type, r.entity_id, r.remind_at, r.timezone,
    r.local_time, r.status, r.last_triggered_at,
    COALESCE(CASE r.entity_type
        WHEN 'note' THEN (SELECT title FROM notes WHERE id = r.entity_id)
        WHEN 'task' THEN (SELECT title FROM tasks WHERE id = r.entity_id)
        WHEN 'event' THEN (SELECT title FROM events WHERE id = r.entity_id)
    END, ''),
    CASE r.entity_type
        WHEN 'note' THEN r.entity_id
        WHEN 'task' THEN (SELECT notes.id FROM tasks JOIN notes ON notes.id = tasks.note_id
                          WHERE tasks.id = r.entity_id AND notes.deleted_at IS NULL)
        WHEN 'event' THEN (SELECT notes.id FROM events JOIN notes ON notes.id = events.note_id
                           WHERE events.id = r.entity_id AND notes.deleted_at IS NULL)
    END,
    r.updated_at";

/// Vero se l'entità del promemoria `r` esiste ed è attiva. Le note nel
/// cestino conservano i promemoria, che riprendono dopo il ripristino.
const REMINDER_ENTITY_ACTIVE: &str = "CASE r.entity_type
    WHEN 'note' THEN EXISTS(SELECT 1 FROM notes WHERE id = r.entity_id AND deleted_at IS NULL)
    WHEN 'task' THEN EXISTS(SELECT 1 FROM tasks
                            WHERE id = r.entity_id AND status = 'open' AND deleted_at IS NULL)
    WHEN 'event' THEN EXISTS(SELECT 1 FROM events WHERE id = r.entity_id AND deleted_at IS NULL)
    ELSE 0 END";

fn task_from_row(row: &Row<'_>) -> rusqlite::Result<Task> {
    Ok(Task {
        id: row.get(0)?,
        note_id: row.get(1)?,
        note_title: row.get(2)?,
        title: row.get(3)?,
        description: row.get(4)?,
        status: TaskStatus::parse(&row.get::<_, String>(5)?),
        priority: row.get(6)?,
        due_date: row.get(7)?,
        completed_at: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
        next_reminder_at: row.get(11)?,
        flagged: row.get(12)?,
    })
}

fn event_from_row(row: &Row<'_>) -> rusqlite::Result<CalendarEvent> {
    Ok(CalendarEvent {
        id: row.get(0)?,
        note_id: row.get(1)?,
        note_title: row.get(2)?,
        title: row.get(3)?,
        description: row.get(4)?,
        all_day: row.get(5)?,
        start_at: row.get(6)?,
        end_at: row.get(7)?,
        start_date: row.get(8)?,
        end_date: row.get(9)?,
        timezone: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        external_calendar_id: row.get(13)?,
        calendar_provider: row.get(14)?,
        read_only: row.get(15)?,
        web_link: row.get(16)?,
        subscription_id: row.get(17)?,
    })
}

fn reminder_from_row(row: &Row<'_>) -> rusqlite::Result<Reminder> {
    let entity: String = row.get(1)?;
    Ok(Reminder {
        id: row.get(0)?,
        entity_type: ReminderEntity::parse(&entity).unwrap_or(ReminderEntity::Note),
        entity_id: row.get(2)?,
        remind_at: row.get(3)?,
        timezone: row.get(4)?,
        local_time: row.get(5)?,
        status: ReminderStatus::parse(&row.get::<_, String>(6)?),
        last_triggered_at: row.get(7)?,
        entity_title: row.get(8)?,
        note_id: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn normalize_title(title: &str, error: StorageError) -> Result<String, StorageError> {
    let title = title.trim();
    if title.is_empty() || title.chars().count() > MAX_TITLE_CHARS {
        return Err(error);
    }
    Ok(title.to_owned())
}

fn normalize_description(description: &str, error: StorageError) -> Result<String, StorageError> {
    let description = description.trim();
    if description.chars().count() > MAX_DESCRIPTION_CHARS {
        return Err(error);
    }
    Ok(description.to_owned())
}

fn ensure_active_note(connection: &Connection, note_id: Option<&str>) -> Result<(), StorageError> {
    if let Some(note_id) = note_id {
        let exists: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM notes WHERE id = ?1 AND deleted_at IS NULL)",
            [note_id],
            |row| row.get(0),
        )?;
        if !exists {
            return Err(StorageError::NoteNotFound);
        }
    }
    Ok(())
}

struct ValidTask {
    flagged: bool,
    title: String,
    description: String,
    priority: u8,
    due_date: Option<String>,
    note_id: Option<String>,
}

fn validate_task(input: &TaskInput) -> Result<ValidTask, StorageError> {
    let title = normalize_title(
        &input.title,
        StorageError::InvalidTask("il titolo deve contenere da 1 a 512 caratteri"),
    )?;
    let description = normalize_description(
        &input.description,
        StorageError::InvalidTask("descrizione troppo lunga"),
    )?;
    if input.priority > 3 {
        return Err(StorageError::InvalidTask("priorità non valida"));
    }
    let due_date = match input.due_date.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(value) => Some(time::parse_date(value)?.to_string()),
    };
    Ok(ValidTask {
        flagged: input.flagged,
        title,
        description,
        priority: input.priority,
        due_date,
        note_id: input.note_id.clone().filter(|id| !id.is_empty()),
    })
}

fn get_task(connection: &Connection, id: &str) -> Result<Task, StorageError> {
    connection
        .query_row(
            &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE tasks.id = ?1"),
            [id],
            task_from_row,
        )
        .optional()?
        .ok_or(StorageError::TaskNotFound)
}

fn get_event(connection: &Connection, id: &str) -> Result<CalendarEvent, StorageError> {
    connection
        .query_row(
            &format!("SELECT {EVENT_COLUMNS} FROM events WHERE events.id = ?1"),
            [id],
            event_from_row,
        )
        .optional()?
        .ok_or(StorageError::EventNotFound)
}

fn get_reminder(connection: &Connection, id: &str) -> Result<Reminder, StorageError> {
    connection
        .query_row(
            &format!("SELECT {REMINDER_COLUMNS} FROM reminders AS r WHERE r.id = ?1"),
            [id],
            reminder_from_row,
        )
        .optional()?
        .ok_or(StorageError::ReminderNotFound)
}

pub(crate) struct ValidEvent {
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) all_day: bool,
    pub(crate) start_at: Option<String>,
    pub(crate) end_at: Option<String>,
    pub(crate) start_date: Option<String>,
    pub(crate) end_date: Option<String>,
    pub(crate) timezone: String,
    pub(crate) note_id: Option<String>,
}

pub(crate) fn validate_event(input: &EventInput) -> Result<ValidEvent, StorageError> {
    let title = normalize_title(
        &input.title,
        StorageError::InvalidEvent("il titolo deve contenere da 1 a 512 caratteri"),
    )?;
    let description = normalize_description(
        &input.description,
        StorageError::InvalidEvent("descrizione troppo lunga"),
    )?;
    let zone = Zone::resolve(input.timezone.as_deref())?;
    let note_id = input.note_id.clone().filter(|id| !id.is_empty());
    if input.all_day {
        let start = time::parse_date(input.start.trim())?;
        let end = time::parse_date(input.end.trim())?;
        let days = (end - start).get_days() as i64;
        if days < 0 {
            return Err(StorageError::InvalidEvent("la fine precede l'inizio"));
        }
        if days >= MAX_EVENT_DAYS {
            return Err(StorageError::InvalidEvent(
                "l'evento può durare al massimo un anno",
            ));
        }
        Ok(ValidEvent {
            title,
            description,
            all_day: true,
            start_at: None,
            end_at: None,
            start_date: Some(start.to_string()),
            end_date: Some(end.to_string()),
            timezone: zone.name,
            note_id,
        })
    } else {
        let start = time::local_to_utc(input.start.trim(), &zone)?;
        let end = time::local_to_utc(input.end.trim(), &zone)?;
        if end < start {
            return Err(StorageError::InvalidEvent("la fine precede l'inizio"));
        }
        if end.as_second() - start.as_second() > MAX_EVENT_DAYS * 86_400 {
            return Err(StorageError::InvalidEvent(
                "l'evento può durare al massimo un anno",
            ));
        }
        Ok(ValidEvent {
            title,
            description,
            all_day: false,
            start_at: Some(time::format_utc(start)),
            end_at: Some(time::format_utc(end)),
            start_date: None,
            end_date: None,
            timezone: zone.name,
            note_id,
        })
    }
}

fn ensure_reminder_entity(
    connection: &Connection,
    entity: ReminderEntity,
    entity_id: &str,
) -> Result<(), StorageError> {
    let (sql, error) = match entity {
        ReminderEntity::Note => (
            "SELECT EXISTS(SELECT 1 FROM notes WHERE id = ?1 AND deleted_at IS NULL)",
            StorageError::NoteNotFound,
        ),
        ReminderEntity::Task => (
            "SELECT EXISTS(SELECT 1 FROM tasks WHERE id = ?1 AND deleted_at IS NULL)",
            StorageError::TaskNotFound,
        ),
        ReminderEntity::Event => (
            "SELECT EXISTS(SELECT 1 FROM events WHERE id = ?1 AND deleted_at IS NULL)",
            StorageError::EventNotFound,
        ),
    };
    let exists: bool = connection.query_row(sql, [entity_id], |row| row.get(0))?;
    if exists { Ok(()) } else { Err(error) }
}

impl Database {
    // -----------------------------------------------------------------------
    // Attività

    pub fn create_task(&self, input: &TaskInput) -> Result<Task, StorageError> {
        let task = validate_task(input)?;
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        ensure_active_note(&connection, task.note_id.as_deref())?;
        connection.execute(
            "INSERT INTO tasks (id, note_id, title, description, priority, due_date, flagged)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                task.note_id,
                task.title,
                task.description,
                task.priority,
                task.due_date,
                task.flagged
            ],
        )?;
        get_task(&connection, &id)
    }

    pub fn update_task(&self, id: &str, input: &TaskInput) -> Result<Task, StorageError> {
        let task = validate_task(input)?;
        let connection = self.connection()?;
        let current = get_task(&connection, id)?;
        // Una nota nel frattempo cestinata può restare collegata.
        if task.note_id != current.note_id {
            ensure_active_note(&connection, task.note_id.as_deref())?;
        }
        connection.execute(
            &format!(
                "UPDATE tasks SET note_id = ?1, title = ?2, description = ?3, priority = ?4,
                     due_date = ?5, flagged = ?6, updated_at = {NOW_SQL}
                 WHERE id = ?7"
            ),
            params![
                task.note_id,
                task.title,
                task.description,
                task.priority,
                task.due_date,
                task.flagged,
                id
            ],
        )?;
        get_task(&connection, id)
    }

    /// Completa o riapre un'attività. Completandola, i suoi promemoria
    /// vengono chiusi nella stessa transazione.
    pub fn set_task_completed(&self, id: &str, completed: bool) -> Result<Task, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let status = if completed {
            TaskStatus::Done
        } else {
            TaskStatus::Open
        };
        let changed = transaction.execute(
            &format!(
                "UPDATE tasks SET status = ?1,
                     completed_at = CASE WHEN ?1 = 'done' THEN COALESCE(completed_at, {NOW_SQL})
                                         ELSE NULL END,
                     updated_at = {NOW_SQL}
                 WHERE id = ?2"
            ),
            params![status.as_str(), id],
        )?;
        if changed == 0 {
            return Err(StorageError::TaskNotFound);
        }
        if completed {
            transaction.execute(
                &format!(
                    "UPDATE reminders SET status = 'done', updated_at = {NOW_SQL}
                     WHERE entity_type = 'task' AND entity_id = ?1 AND status != 'done'"
                ),
                [id],
            )?;
        }
        let task = get_task(&transaction, id)?;
        transaction.commit()?;
        Ok(task)
    }

    pub fn get_task(&self, id: &str) -> Result<Task, StorageError> {
        get_task(&*self.connection()?, id)
    }

    /// Elenca le attività di una vista rispetto alla data civile `today`,
    /// filtrandole facoltativamente per testo in titolo e descrizione.
    pub fn list_tasks(
        &self,
        view: TaskView,
        today: Date,
        search: Option<&str>,
    ) -> Result<Vec<Task>, StorageError> {
        let search = search.map(str::trim).filter(|value| !value.is_empty());
        if search.is_some_and(|value| value.chars().count() > MAX_SEARCH_CHARS) {
            return Err(StorageError::InvalidSearchQuery(
                "la ricerca può contenere al massimo 200 caratteri".to_owned(),
            ));
        }
        let (condition, order) = match view {
            TaskView::Today => ("status = 'open' AND due_date = ?1", "priority DESC, title"),
            TaskView::Overdue => (
                "status = 'open' AND due_date < ?1",
                "due_date, priority DESC, title",
            ),
            TaskView::Upcoming => (
                "status = 'open' AND due_date > ?1",
                "due_date, priority DESC, title",
            ),
            TaskView::NoDate => (
                "status = 'open' AND due_date IS NULL AND ?1 IS NOT NULL",
                "priority DESC, created_at DESC",
            ),
            TaskView::Open => (
                "status = 'open' AND ?1 IS NOT NULL",
                "due_date IS NULL, due_date, priority DESC, title",
            ),
            TaskView::Flagged => (
                "status = 'open' AND flagged = 1 AND ?1 IS NOT NULL",
                "due_date IS NULL, due_date, priority DESC, title",
            ),
            TaskView::Completed => ("status = 'done' AND ?1 IS NOT NULL", "completed_at DESC"),
        };
        let connection = self.connection()?;
        // Confronto senza distinzione di maiuscole tramite `lower()` sui due
        // lati: copre l'ASCII, non tutte le lingue (limite documentato).
        let mut statement = connection.prepare(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks
             WHERE {condition} AND deleted_at IS NULL
               AND (?2 IS NULL OR instr(lower(title || ' ' || description), lower(?2)) > 0)
             ORDER BY {order}
             LIMIT {MAX_TASKS}"
        ))?;
        let rows = statement.query_map(params![today.to_string(), search], task_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn task_counts(&self, today: Date) -> Result<TaskCounts, StorageError> {
        let connection = self.connection()?;
        Ok(connection.query_row(
            "SELECT
                 COALESCE(SUM(status = 'open' AND due_date = ?1), 0),
                 COALESCE(SUM(status = 'open' AND due_date < ?1), 0)
             FROM tasks WHERE deleted_at IS NULL",
            [today.to_string()],
            |row| {
                Ok(TaskCounts {
                    today: u64::try_from(row.get::<_, i64>(0)?).unwrap_or(0),
                    overdue: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
                })
            },
        )?)
    }

    pub fn list_note_tasks(&self, note_id: &str) -> Result<Vec<Task>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks
             WHERE tasks.note_id = ?1 AND tasks.deleted_at IS NULL
             ORDER BY status = 'done', due_date IS NULL, due_date, created_at
             LIMIT {MAX_TASKS}"
        ))?;
        let rows = statement.query_map([note_id], task_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Attività aperte con scadenza nell'intervallo (estremi inclusi).
    pub fn list_tasks_due_between(&self, from: Date, to: Date) -> Result<Vec<Task>, StorageError> {
        validate_range(from, to)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks
             WHERE status = 'open' AND deleted_at IS NULL AND due_date BETWEEN ?1 AND ?2
             ORDER BY due_date, priority DESC, title
             LIMIT {MAX_TASKS}"
        ))?;
        let rows = statement.query_map(params![from.to_string(), to.to_string()], task_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Eventi

    pub fn create_event(&self, input: &EventInput) -> Result<CalendarEvent, StorageError> {
        let event = validate_event(input)?;
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        ensure_active_note(&connection, event.note_id.as_deref())?;
        connection.execute(
            "INSERT INTO events (id, note_id, title, description, all_day, start_at, end_at,
                                 start_date, end_date, timezone)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                event.note_id,
                event.title,
                event.description,
                event.all_day,
                event.start_at,
                event.end_at,
                event.start_date,
                event.end_date,
                event.timezone
            ],
        )?;
        get_event(&connection, &id)
    }

    pub fn update_event(
        &self,
        id: &str,
        input: &EventInput,
    ) -> Result<CalendarEvent, StorageError> {
        let event = validate_event(input)?;
        let connection = self.connection()?;
        let current = get_event(&connection, id)?;
        if current.calendar_provider.is_some() {
            return Err(StorageError::ExternalEvent);
        }
        if event.note_id != current.note_id {
            ensure_active_note(&connection, event.note_id.as_deref())?;
        }
        connection.execute(
            &format!(
                "UPDATE events SET note_id = ?1, title = ?2, description = ?3, all_day = ?4,
                     start_at = ?5, end_at = ?6, start_date = ?7, end_date = ?8, timezone = ?9,
                     updated_at = {NOW_SQL}
                 WHERE id = ?10"
            ),
            params![
                event.note_id,
                event.title,
                event.description,
                event.all_day,
                event.start_at,
                event.end_at,
                event.start_date,
                event.end_date,
                event.timezone,
                id
            ],
        )?;
        get_event(&connection, id)
    }

    pub fn get_event(&self, id: &str) -> Result<CalendarEvent, StorageError> {
        get_event(&*self.connection()?, id)
    }

    /// Eventi che intersecano i giorni civili `from..=to` nel fuso `zone`.
    pub fn list_events(
        &self,
        from: Date,
        to: Date,
        zone: &Zone,
    ) -> Result<Vec<CalendarEvent>, StorageError> {
        validate_range(from, to)?;
        let start = time::format_utc(time::day_start(from, zone)?);
        let end = time::format_utc(time::day_start(time::next_day(to)?, zone)?);
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {EVENT_COLUMNS} FROM events
             WHERE deleted_at IS NULL
               AND ((all_day = 0 AND start_at < ?2 AND (end_at > ?1 OR start_at >= ?1))
                    OR (all_day = 1 AND start_date <= ?4 AND end_date >= ?3))
             ORDER BY all_day DESC, COALESCE(start_at, start_date), title
             LIMIT 2000"
        ))?;
        let rows = statement.query_map(
            params![start, end, from.to_string(), to.to_string()],
            event_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    // -----------------------------------------------------------------------
    // Promemoria

    /// Crea un promemoria all'orario locale `local` (`AAAA-MM-GGTHH:MM`) nel
    /// fuso indicato o in quello del sistema.
    pub fn create_reminder(
        &self,
        entity: ReminderEntity,
        entity_id: &str,
        local: &str,
        timezone: Option<&str>,
        now: Timestamp,
    ) -> Result<Reminder, StorageError> {
        let zone = Zone::resolve(timezone)?;
        let remind_at = time::local_to_utc(local, &zone)?;
        if remind_at < time::add_minutes(now, -PAST_TOLERANCE_MINUTES)? {
            return Err(StorageError::InvalidReminder("l'orario è già passato"));
        }
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        ensure_reminder_entity(&connection, entity, entity_id)?;
        connection.execute(
            "INSERT INTO reminders (id, entity_type, entity_id, remind_at, timezone, local_time)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id,
                entity.as_str(),
                entity_id,
                time::format_utc(remind_at),
                zone.name,
                time::local_wall(remind_at, &zone)
            ],
        )?;
        get_reminder(&connection, &id)
    }

    /// Promemoria da mostrare: prima quelli scattati e non gestiti, poi i
    /// programmati, esclusi quelli di note nel cestino o attività completate.
    pub fn list_reminders(&self) -> Result<Vec<Reminder>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {REMINDER_COLUMNS} FROM reminders AS r
             WHERE r.status != 'done' AND {REMINDER_ENTITY_ACTIVE}
             ORDER BY r.status = 'pending', r.remind_at
             LIMIT 1000"
        ))?;
        let rows = statement.query_map([], reminder_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Promemoria già gestiti (fatti, archiviati o chiusi completando
    /// un'attività), dal più recente.
    pub fn list_reminder_history(&self, limit: u32) -> Result<Vec<Reminder>, StorageError> {
        crate::storage::validate_page(limit)?;
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {REMINDER_COLUMNS} FROM reminders AS r
             WHERE r.status = 'done'
             ORDER BY r.updated_at DESC, r.id
             LIMIT ?1"
        ))?;
        let rows = statement.query_map([limit], reminder_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Elimina lo storico dei promemoria gestiti; restituisce quanti.
    pub fn clear_reminder_history(&self) -> Result<u64, StorageError> {
        let connection = self.connection()?;
        Ok(connection.execute("DELETE FROM reminders WHERE status = 'done'", [])? as u64)
    }

    pub fn list_entity_reminders(
        &self,
        entity: ReminderEntity,
        entity_id: &str,
    ) -> Result<Vec<Reminder>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {REMINDER_COLUMNS} FROM reminders AS r
             WHERE r.entity_type = ?1 AND r.entity_id = ?2 AND r.status != 'done'
             ORDER BY r.remind_at"
        ))?;
        let rows = statement.query_map(params![entity.as_str(), entity_id], reminder_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn snooze_reminder(
        &self,
        id: &str,
        preset: SnoozePreset,
        now: Timestamp,
    ) -> Result<Reminder, StorageError> {
        let connection = self.connection()?;
        let current = get_reminder(&connection, id)?;
        let zone = Zone::stored(&current.timezone);
        let next = match preset {
            SnoozePreset::TenMinutes => time::add_minutes(now, 10)?,
            SnoozePreset::OneHour => time::add_minutes(now, 60)?,
            SnoozePreset::Tomorrow => {
                let time_of_day = time::parse_local(&current.local_time)?.time();
                time::tomorrow_at(now, time_of_day, &zone)?
            }
        };
        connection.execute(
            &format!(
                "UPDATE reminders SET status = 'pending', remind_at = ?1, local_time = ?2,
                     updated_at = {NOW_SQL}
                 WHERE id = ?3"
            ),
            params![time::format_utc(next), time::local_wall(next, &zone), id],
        )?;
        get_reminder(&connection, id)
    }

    /// Segna un promemoria come gestito.
    pub fn dismiss_reminder(&self, id: &str) -> Result<(), StorageError> {
        let connection = self.connection()?;
        let changed = connection.execute(
            &format!("UPDATE reminders SET status = 'done', updated_at = {NOW_SQL} WHERE id = ?1"),
            [id],
        )?;
        if changed == 0 {
            return Err(StorageError::ReminderNotFound);
        }
        Ok(())
    }

    pub fn delete_reminder(&self, id: &str) -> Result<(), StorageError> {
        let connection = self.connection()?;
        if connection.execute("DELETE FROM reminders WHERE id = ?1", [id])? == 0 {
            return Err(StorageError::ReminderNotFound);
        }
        Ok(())
    }

    /// Segna come scattati i promemoria programmati fino a `now` (inclusi
    /// quelli persi mentre l'app era chiusa) e li restituisce. Ogni
    /// promemoria viene restituito una sola volta.
    pub fn take_due_reminders(&self, now: Timestamp) -> Result<Vec<Reminder>, StorageError> {
        let now_text = time::format_utc(now);
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let ids = {
            let mut statement = transaction.prepare(&format!(
                "SELECT r.id FROM reminders AS r
                 WHERE r.status = 'pending' AND r.remind_at <= ?1 AND {REMINDER_ENTITY_ACTIVE}
                 ORDER BY r.remind_at
                 LIMIT 100"
            ))?;
            let rows = statement.query_map([&now_text], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        let mut due = Vec::with_capacity(ids.len());
        for id in &ids {
            transaction.execute(
                &format!(
                    "UPDATE reminders SET status = 'fired', last_triggered_at = ?1,
                         updated_at = {NOW_SQL}
                     WHERE id = ?2"
                ),
                params![now_text, id],
            )?;
            due.push(get_reminder(&transaction, id)?);
        }
        transaction.commit()?;
        Ok(due)
    }
}

// ---------------------------------------------------------------------------
// Archivio completo (campi facoltativi di `gna-archive` v1)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ArchiveTask {
    #[serde(default)]
    flagged: bool,
    id: String,
    note_id: Option<String>,
    title: String,
    description: String,
    status: String,
    priority: i64,
    due_date: Option<String>,
    completed_at: Option<String>,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ArchiveEvent {
    id: String,
    note_id: Option<String>,
    title: String,
    description: String,
    all_day: bool,
    start_at: Option<String>,
    end_at: Option<String>,
    start_date: Option<String>,
    end_date: Option<String>,
    timezone: String,
    created_at: String,
    updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ArchiveReminder {
    id: String,
    entity_type: String,
    entity_id: String,
    remind_at: String,
    timezone: String,
    local_time: String,
    status: String,
    last_triggered_at: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct PlannerArchive {
    #[serde(default)]
    pub tasks: Vec<ArchiveTask>,
    #[serde(default)]
    pub events: Vec<ArchiveEvent>,
    #[serde(default)]
    pub reminders: Vec<ArchiveReminder>,
}

/// Legge attività, eventi e promemoria non gestiti per l'archivio completo.
/// I collegamenti a note nel cestino vengono omessi perché il cestino non è
/// esportato.
pub(crate) fn export_planner(connection: &Connection) -> Result<PlannerArchive, StorageError> {
    const ACTIVE_NOTE: &str = "(SELECT id FROM notes WHERE id = note_id AND deleted_at IS NULL)";
    let tasks = connection
        .prepare(&format!(
            "SELECT id, {ACTIVE_NOTE}, title, description, status, priority, due_date,
                    completed_at, created_at, updated_at, flagged
             FROM tasks WHERE deleted_at IS NULL ORDER BY created_at, id"
        ))?
        .query_map([], |row| {
            Ok(ArchiveTask {
                id: row.get(0)?,
                note_id: row.get(1)?,
                title: row.get(2)?,
                description: row.get(3)?,
                status: row.get(4)?,
                priority: row.get(5)?,
                due_date: row.get(6)?,
                completed_at: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
                flagged: row.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let events = connection
        .prepare(&format!(
            "SELECT id, {ACTIVE_NOTE}, title, description, all_day, start_at, end_at,
                    start_date, end_date, timezone, created_at, updated_at
             FROM events
             WHERE deleted_at IS NULL
               -- Gli eventi esterni restano nel calendario d'origine.
               AND calendar_provider IS NULL
             ORDER BY created_at, id"
        ))?
        .query_map([], |row| {
            Ok(ArchiveEvent {
                id: row.get(0)?,
                note_id: row.get(1)?,
                title: row.get(2)?,
                description: row.get(3)?,
                all_day: row.get(4)?,
                start_at: row.get(5)?,
                end_at: row.get(6)?,
                start_date: row.get(7)?,
                end_date: row.get(8)?,
                timezone: row.get(9)?,
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let reminders = connection
        .prepare(
            "SELECT id, entity_type, entity_id, remind_at, timezone, local_time, status,
                    last_triggered_at
             FROM reminders AS r
             WHERE status != 'done'
               AND CASE entity_type
                   WHEN 'note' THEN EXISTS(SELECT 1 FROM notes
                                           WHERE id = r.entity_id AND deleted_at IS NULL)
                   WHEN 'task' THEN EXISTS(SELECT 1 FROM tasks
                                           WHERE id = r.entity_id AND deleted_at IS NULL)
                   ELSE EXISTS(SELECT 1 FROM events WHERE id = r.entity_id AND deleted_at IS NULL
                                                         AND calendar_provider IS NULL)
                   END
             ORDER BY remind_at, id",
        )?
        .query_map([], |row| {
            Ok(ArchiveReminder {
                id: row.get(0)?,
                entity_type: row.get(1)?,
                entity_id: row.get(2)?,
                remind_at: row.get(3)?,
                timezone: row.get(4)?,
                local_time: row.get(5)?,
                status: row.get(6)?,
                last_triggered_at: row.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(PlannerArchive {
        tasks,
        events,
        reminders,
    })
}

/// Esegue un inserimento ignorando le righe che violano i vincoli dello
/// schema: un elemento non valido dell'archivio viene saltato, gli altri
/// importati. Restituisce se la riga è stata inserita.
fn insert_or_skip(
    connection: &Connection,
    sql: &str,
    params: impl rusqlite::Params,
) -> Result<bool, StorageError> {
    match connection.execute(sql, params) {
        Ok(changed) => Ok(changed > 0),
        Err(rusqlite::Error::SqliteFailure(error, _))
            if error.code == ErrorCode::ConstraintViolation =>
        {
            Ok(false)
        }
        Err(error) => Err(error.into()),
    }
}

/// Importa attività, eventi e promemoria di un archivio (elementi già
/// presenti con lo stesso ID vengono saltati). I promemoria programmati nel
/// passato vengono importati come già scattati, per mostrarli senza inviare
/// notifiche arretrate.
pub(crate) fn import_planner(
    connection: &Connection,
    archive: &PlannerArchive,
    now: Timestamp,
) -> Result<u64, StorageError> {
    const NOTE_IF_PRESENT: &str = "(SELECT id FROM notes WHERE id = ?2)";
    let now_text = time::format_utc(now);
    let valid_id = |id: &str| Uuid::parse_str(id).is_ok();
    let mut imported = 0;
    for task in archive.tasks.iter().filter(|task| valid_id(&task.id)) {
        let inserted = insert_or_skip(
            connection,
            &format!(
                "INSERT OR IGNORE INTO tasks (id, note_id, title, description, status, priority,
                     due_date, completed_at, created_at, updated_at, flagged)
                 VALUES (?1, {NOTE_IF_PRESENT}, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
            ),
            params![
                task.id,
                task.note_id,
                task.title.trim(),
                task.description,
                task.status,
                task.priority,
                task.due_date,
                task.completed_at,
                task.created_at,
                task.updated_at,
                task.flagged
            ],
        )?;
        imported += u64::from(inserted);
    }
    for event in archive.events.iter().filter(|event| valid_id(&event.id)) {
        if Zone::resolve(Some(&event.timezone)).is_err() {
            continue;
        }
        let inserted = insert_or_skip(
            connection,
            &format!(
                "INSERT OR IGNORE INTO events (id, note_id, title, description, all_day,
                     start_at, end_at, start_date, end_date, timezone, created_at, updated_at)
                 VALUES (?1, {NOTE_IF_PRESENT}, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"
            ),
            params![
                event.id,
                event.note_id,
                event.title.trim(),
                event.description,
                event.all_day,
                event.start_at,
                event.end_at,
                event.start_date,
                event.end_date,
                event.timezone,
                event.created_at,
                event.updated_at
            ],
        )?;
        imported += u64::from(inserted);
    }
    for reminder in archive
        .reminders
        .iter()
        .filter(|reminder| valid_id(&reminder.id))
    {
        let Some(entity) = ReminderEntity::parse(&reminder.entity_type) else {
            continue;
        };
        let valid_time = time::parse_utc(&reminder.remind_at).is_ok()
            && time::parse_local(&reminder.local_time).is_ok()
            && Zone::resolve(Some(&reminder.timezone)).is_ok();
        if !valid_time || ensure_reminder_entity(connection, entity, &reminder.entity_id).is_err() {
            continue;
        }
        let inserted = insert_or_skip(
            connection,
            "INSERT OR IGNORE INTO reminders (id, entity_type, entity_id, remind_at, timezone,
                 local_time, status, last_triggered_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6,
                     CASE WHEN ?7 = 'pending' AND ?4 <= ?9 THEN 'fired' ELSE ?7 END, ?8)",
            params![
                reminder.id,
                entity.as_str(),
                reminder.entity_id,
                reminder.remind_at,
                reminder.timezone,
                reminder.local_time,
                reminder.status,
                reminder.last_triggered_at,
                now_text
            ],
        )?;
        imported += u64::from(inserted);
    }
    Ok(imported)
}

fn validate_range(from: Date, to: Date) -> Result<(), StorageError> {
    let days = (to - from).get_days() as i64;
    if !(0..=MAX_RANGE_DAYS).contains(&days) {
        return Err(StorageError::InvalidDateTime(
            "intervallo di date non valido".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{
        domain::{
            note::empty_document,
            planner::{
                EventInput, ReminderEntity, ReminderStatus, SnoozePreset, TaskInput, TaskStatus,
                TaskView,
            },
        },
        storage::{Database, StorageError},
        time::{self, Zone},
    };

    fn task(title: &str, due: Option<&str>) -> TaskInput {
        TaskInput {
            title: title.to_owned(),
            description: String::new(),
            priority: 0,
            due_date: due.map(str::to_owned),
            note_id: None,
            flagged: false,
        }
    }

    fn date(value: &str) -> jiff::civil::Date {
        time::parse_date(value).expect("date")
    }

    fn instant(value: &str) -> jiff::Timestamp {
        time::parse_utc(value).expect("instant")
    }

    #[test]
    fn task_views_follow_the_given_day() {
        let database = Database::in_memory().expect("database");
        database
            .create_task(&task("Ieri", Some("2026-10-13")))
            .expect("task");
        database
            .create_task(&task("Oggi", Some("2026-10-14")))
            .expect("task");
        database
            .create_task(&task("Domani", Some("2026-10-15")))
            .expect("task");
        database
            .create_task(&task("Quando capita", None))
            .expect("task");
        let today = date("2026-10-14");

        let titles = |view| {
            database
                .list_tasks(view, today, None)
                .expect("tasks")
                .into_iter()
                .map(|task| task.title)
                .collect::<Vec<_>>()
        };
        assert_eq!(titles(TaskView::Overdue), ["Ieri"]);
        assert_eq!(titles(TaskView::Today), ["Oggi"]);
        assert_eq!(titles(TaskView::Upcoming), ["Domani"]);
        assert_eq!(titles(TaskView::NoDate), ["Quando capita"]);
        assert_eq!(titles(TaskView::Open).len(), 4);
        let counts = database.task_counts(today).expect("counts");
        assert_eq!((counts.today, counts.overdue), (1, 1));

        // Il giorno dopo "Oggi" diventa in ritardo, senza modificare i dati.
        let tomorrow = date("2026-10-15");
        assert_eq!(database.task_counts(tomorrow).expect("counts").overdue, 2);
    }

    #[test]
    fn tasks_validate_link_complete_and_search() {
        let database = Database::in_memory().expect("database");
        for invalid in [
            task("   ", None),
            task("x", Some("2026-02-30")),
            task("x", Some("domani")),
            TaskInput {
                priority: 4,
                ..task("x", None)
            },
            task(&"a".repeat(513), None),
        ] {
            assert!(database.create_task(&invalid).is_err(), "{invalid:?}");
        }
        assert!(matches!(
            database.create_task(&TaskInput {
                note_id: Some("missing".to_owned()),
                ..task("x", None)
            }),
            Err(StorageError::NoteNotFound)
        ));

        let note = database
            .create_note("Riunione", &empty_document())
            .expect("note");
        let linked = database
            .create_task(&TaskInput {
                description: "Preparare le slide".to_owned(),
                priority: 3,
                note_id: Some(note.id.clone()),
                ..task("Presentazione", Some("2026-10-20"))
            })
            .expect("task");
        assert_eq!(linked.note_title.as_deref(), Some("Riunione"));
        assert_eq!(database.list_note_tasks(&note.id).expect("tasks").len(), 1);

        let today = date("2026-10-14");
        let found = database
            .list_tasks(TaskView::Open, today, Some("SLIDE"))
            .expect("search");
        assert_eq!(found.len(), 1);
        // Caratteri speciali trattati come testo, mai come SQL o pattern.
        for query in ["%", "_", "' OR 1=1 --", "\"", "\\"] {
            assert!(
                database
                    .list_tasks(TaskView::Open, today, Some(query))
                    .expect("search")
                    .is_empty(),
                "{query}"
            );
        }

        let done = database.set_task_completed(&linked.id, true).expect("done");
        assert_eq!(done.status, TaskStatus::Done);
        assert!(done.completed_at.is_some());
        assert!(
            database
                .list_tasks(TaskView::Upcoming, today, None)
                .expect("tasks")
                .is_empty()
        );
        assert_eq!(
            database
                .list_tasks(TaskView::Completed, today, None)
                .expect("tasks")
                .len(),
            1
        );
        let reopened = database
            .set_task_completed(&linked.id, false)
            .expect("open");
        assert!(reopened.completed_at.is_none());

        // La nota nel cestino non viene mostrata ma il collegamento resta.
        database.trash_note(&note.id).expect("trash");
        let task = database.get_task(&linked.id).expect("task");
        assert_eq!(task.note_id.as_deref(), Some(note.id.as_str()));
        assert!(task.note_title.is_none());
        let updated = database
            .update_task(
                &linked.id,
                &TaskInput {
                    note_id: Some(note.id.clone()),
                    ..task_input_from(&task)
                },
            )
            .expect("update keeps link");
        assert_eq!(updated.title, "Presentazione");

        database
            .move_item_to_trash(crate::trash::TrashKind::Task, &linked.id)
            .expect("trash");
        database
            .delete_trash_item(crate::trash::TrashKind::Task, &linked.id)
            .expect("delete");
        assert!(matches!(
            database.get_task(&linked.id),
            Err(StorageError::TaskNotFound)
        ));
    }

    fn task_input_from(task: &crate::domain::planner::Task) -> TaskInput {
        TaskInput {
            title: task.title.clone(),
            description: task.description.clone(),
            priority: task.priority,
            due_date: task.due_date.clone(),
            note_id: task.note_id.clone(),
            flagged: false,
        }
    }

    #[test]
    fn reminders_fire_once_including_missed_ones() {
        let database = Database::in_memory().expect("database");
        let note = database
            .create_note("Chiamare", &empty_document())
            .expect("note");
        let now = instant("2026-10-14T07:00:00Z");
        assert!(matches!(
            database.create_reminder(
                ReminderEntity::Note,
                &note.id,
                "2026-10-14T08:00",
                Some("Europe/Rome"),
                now
            ),
            Err(StorageError::InvalidReminder(_))
        ));
        let reminder = database
            .create_reminder(
                ReminderEntity::Note,
                &note.id,
                "2026-10-14T10:00",
                Some("Europe/Rome"),
                now,
            )
            .expect("reminder");
        assert_eq!(reminder.remind_at, "2026-10-14T08:00:00Z");
        assert_eq!(reminder.local_time, "2026-10-14T10:00");
        assert_eq!(reminder.entity_title, "Chiamare");
        assert_eq!(reminder.note_id.as_deref(), Some(note.id.as_str()));

        assert!(
            database
                .take_due_reminders(instant("2026-10-14T07:59:59Z"))
                .expect("due")
                .is_empty()
        );
        // L'app era chiusa all'orario previsto: scatta al primo controllo.
        let fired = database
            .take_due_reminders(instant("2026-10-14T12:00:00Z"))
            .expect("due");
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].status, ReminderStatus::Fired);
        assert!(
            database
                .take_due_reminders(instant("2026-10-14T12:01:00Z"))
                .expect("due")
                .is_empty()
        );
        assert_eq!(
            database.list_reminders().expect("list")[0].status,
            ReminderStatus::Fired
        );

        database.dismiss_reminder(&reminder.id).expect("dismiss");
        assert!(database.list_reminders().expect("list").is_empty());
        // Il promemoria gestito resta nello storico finché non viene svuotato.
        let history = database.list_reminder_history(50).expect("history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, reminder.id);
        assert_eq!(history[0].status, ReminderStatus::Done);
        assert_eq!(history[0].entity_title, "Chiamare");
        assert!(database.list_reminder_history(0).is_err());
        assert!(
            database
                .list_entity_reminders(ReminderEntity::Note, &note.id)
                .expect("list")
                .is_empty()
        );
    }

    #[test]
    fn reminders_of_trashed_notes_and_completed_tasks_do_not_fire() {
        let database = Database::in_memory().expect("database");
        let now = instant("2026-10-14T07:00:00Z");
        let note = database
            .create_note("Nel cestino", &empty_document())
            .expect("note");
        database
            .create_reminder(
                ReminderEntity::Note,
                &note.id,
                "2026-10-14T10:00",
                Some("Europe/Rome"),
                now,
            )
            .expect("reminder");
        let task = database.create_task(&task("Fatta", None)).expect("task");
        let task_reminder = database
            .create_reminder(
                ReminderEntity::Task,
                &task.id,
                "2026-10-14T10:00",
                Some("Europe/Rome"),
                now,
            )
            .expect("reminder");
        assert!(
            database
                .get_task(&task.id)
                .expect("task")
                .next_reminder_at
                .is_some()
        );

        database.trash_note(&note.id).expect("trash");
        database.set_task_completed(&task.id, true).expect("done");
        let later = instant("2026-10-14T09:00:00Z");
        assert!(database.take_due_reminders(later).expect("due").is_empty());
        assert!(database.list_reminders().expect("list").is_empty());

        // Ripristinata la nota, il promemoria perso scatta.
        database.restore_note(&note.id).expect("restore");
        assert_eq!(database.take_due_reminders(later).expect("due").len(), 1);
        // Eliminando l'attività sparisce anche il suo promemoria.
        // Completare l'attività ha chiuso il promemoria: è nello storico.
        let history = database.list_reminder_history(50).expect("history");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, task_reminder.id);
        assert_eq!(database.clear_reminder_history().expect("clear"), 1);
        assert!(
            database
                .list_reminder_history(50)
                .expect("history")
                .is_empty()
        );
        database
            .move_item_to_trash(crate::trash::TrashKind::Task, &task.id)
            .expect("trash");
        database
            .delete_trash_item(crate::trash::TrashKind::Task, &task.id)
            .expect("delete");
        assert!(matches!(
            database.dismiss_reminder(&task_reminder.id),
            Err(StorageError::ReminderNotFound)
        ));
    }

    #[test]
    fn snoozing_keeps_local_time_across_daylight_saving() {
        let database = Database::in_memory().expect("database");
        let task = database.create_task(&task("Medicina", None)).expect("task");
        // 24 ottobre 2026, 09:00 a Roma (CEST, UTC+2).
        let reminder = database
            .create_reminder(
                ReminderEntity::Task,
                &task.id,
                "2026-10-24T09:00",
                Some("Europe/Rome"),
                instant("2026-10-24T06:00:00Z"),
            )
            .expect("reminder");
        assert_eq!(reminder.remind_at, "2026-10-24T07:00:00Z");
        let fired_at = instant("2026-10-24T07:00:30Z");
        database.take_due_reminders(fired_at).expect("fired");

        let tomorrow = database
            .snooze_reminder(&reminder.id, SnoozePreset::Tomorrow, fired_at)
            .expect("snooze");
        // Il 25 ottobre è tornata l'ora solare (UTC+1): sempre le 09:00 locali.
        assert_eq!(tomorrow.remind_at, "2026-10-25T08:00:00Z");
        assert_eq!(tomorrow.local_time, "2026-10-25T09:00");
        assert_eq!(tomorrow.status, ReminderStatus::Pending);

        let soon = database
            .snooze_reminder(&reminder.id, SnoozePreset::TenMinutes, fired_at)
            .expect("snooze");
        assert_eq!(soon.remind_at, "2026-10-24T07:10:30Z");
    }

    #[test]
    fn reminder_set_in_a_dst_gap_moves_forward() {
        let database = Database::in_memory().expect("database");
        let note = database
            .create_note("Gap", &empty_document())
            .expect("note");
        let reminder = database
            .create_reminder(
                ReminderEntity::Note,
                &note.id,
                "2026-03-29T02:30",
                Some("Europe/Rome"),
                instant("2026-03-28T00:00:00Z"),
            )
            .expect("reminder");
        assert_eq!(reminder.local_time, "2026-03-29T03:30");
        assert_eq!(reminder.remind_at, "2026-03-29T01:30:00Z");
    }

    #[test]
    fn events_are_listed_by_local_day_and_validated() {
        let database = Database::in_memory().expect("database");
        let rome = Zone::resolve(Some("Europe/Rome")).expect("tz");
        let timed = |title: &str, start: &str, end: &str| EventInput {
            title: title.to_owned(),
            description: String::new(),
            all_day: false,
            start: start.to_owned(),
            end: end.to_owned(),
            timezone: Some("Europe/Rome".to_owned()),
            note_id: None,
        };
        // 00:30 locali del 15 = 22:30 UTC del 14: appartiene al 15 a Roma.
        let late = database
            .create_event(&timed("Notte", "2026-10-15T00:30", "2026-10-15T01:00"))
            .expect("event");
        assert_eq!(late.start_at.as_deref(), Some("2026-10-14T22:30:00Z"));
        database
            .create_event(&EventInput {
                all_day: true,
                start: "2026-10-13".to_owned(),
                end: "2026-10-16".to_owned(),
                ..timed("Fiera", "", "")
            })
            .expect("all-day");
        database
            .create_event(&timed("Riunione", "2026-10-14T09:00", "2026-10-14T10:00"))
            .expect("event");

        let titles = |from: &str, to: &str| {
            database
                .list_events(date(from), date(to), &rome)
                .expect("events")
                .into_iter()
                .map(|event| event.title)
                .collect::<Vec<_>>()
        };
        assert_eq!(titles("2026-10-14", "2026-10-14"), ["Fiera", "Riunione"]);
        assert_eq!(titles("2026-10-15", "2026-10-15"), ["Fiera", "Notte"]);
        assert!(titles("2026-10-17", "2026-10-31").is_empty());
        // Visto da New York, "Notte" cade ancora il 14.
        let new_york = Zone::resolve(Some("America/New_York")).expect("tz");
        assert!(
            database
                .list_events(date("2026-10-14"), date("2026-10-14"), &new_york)
                .expect("events")
                .iter()
                .any(|event| event.title == "Notte")
        );

        for invalid in [
            timed("Indietro", "2026-10-14T10:00", "2026-10-14T09:00"),
            timed("Formato", "2026-10-14 10:00", "2026-10-14T11:00"),
            timed("", "2026-10-14T10:00", "2026-10-14T11:00"),
            EventInput {
                timezone: Some("Nowhere/City".to_owned()),
                ..timed("Fuso", "2026-10-14T10:00", "2026-10-14T11:00")
            },
            EventInput {
                all_day: true,
                start: "2026-10-16".to_owned(),
                end: "2026-10-13".to_owned(),
                ..timed("Date", "", "")
            },
        ] {
            assert!(database.create_event(&invalid).is_err(), "{invalid:?}");
        }
        assert!(
            database
                .list_events(date("2026-10-14"), date("2026-10-13"), &rome)
                .is_err()
        );

        let moved = database
            .update_event(
                &late.id,
                &timed("Notte", "2026-10-20T00:30", "2026-10-20T01:00"),
            )
            .expect("update");
        assert_eq!(moved.start_at.as_deref(), Some("2026-10-19T22:30:00Z"));
        database
            .create_reminder(
                ReminderEntity::Event,
                &late.id,
                "2026-10-20T00:00",
                Some("Europe/Rome"),
                instant("2026-10-14T00:00:00Z"),
            )
            .expect("event reminder");
        database
            .move_item_to_trash(crate::trash::TrashKind::Event, &late.id)
            .expect("trash");
        database
            .delete_trash_item(crate::trash::TrashKind::Event, &late.id)
            .expect("delete");
        assert!(database.list_reminders().expect("reminders").is_empty());
    }

    #[test]
    fn note_search_filters_notes_with_open_tasks() {
        let database = Database::in_memory().expect("database");
        let with_task = database
            .create_note("Con attività", &empty_document())
            .expect("note");
        database
            .create_note("Senza", &empty_document())
            .expect("note");
        let linked = database
            .create_task(&TaskInput {
                note_id: Some(with_task.id.clone()),
                ..task("Da fare", None)
            })
            .expect("task");
        let found = database
            .search_notes("is:task", "all", None, 10, 0)
            .expect("search");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].note.id, with_task.id);
        database.set_task_completed(&linked.id, true).expect("done");
        assert!(
            database
                .search_notes("has:task", "all", None, 10, 0)
                .expect("search")
                .is_empty()
        );
    }

    #[test]
    fn planner_data_survives_reopening() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("gna.sqlite3");
        let now = instant("2026-10-14T07:00:00Z");
        {
            let database = Database::open(&path).expect("database");
            let task = database
                .create_task(&task("Persistente", Some("2026-10-14")))
                .expect("task");
            database
                .create_reminder(
                    ReminderEntity::Task,
                    &task.id,
                    "2026-10-14T18:00",
                    Some("Europe/Rome"),
                    now,
                )
                .expect("reminder");
            database
                .create_event(&EventInput {
                    title: "Evento".to_owned(),
                    description: String::new(),
                    all_day: true,
                    start: "2026-10-14".to_owned(),
                    end: "2026-10-14".to_owned(),
                    timezone: Some("Europe/Rome".to_owned()),
                    note_id: None,
                })
                .expect("event");
        }
        let database = Database::open(&path).expect("reopen");
        let today = date("2026-10-14");
        let tasks = database
            .list_tasks(TaskView::Today, today, None)
            .expect("tasks");
        assert_eq!(tasks.len(), 1);
        assert_eq!(
            tasks[0].next_reminder_at.as_deref(),
            Some("2026-10-14T16:00:00Z")
        );
        assert_eq!(database.list_reminders().expect("reminders").len(), 1);
        let rome = Zone::resolve(Some("Europe/Rome")).expect("tz");
        assert_eq!(
            database
                .list_events(today, today, &rome)
                .expect("events")
                .len(),
            1
        );
        assert_eq!(
            database
                .list_tasks_due_between(today, today)
                .expect("due")
                .len(),
            1
        );
    }
}
