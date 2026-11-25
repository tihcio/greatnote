//! Attività, promemoria ed eventi del calendario locale.
//!
//! Le viste che dipendono da "oggi" (oggi, in ritardo, prossime) ricevono la
//! data civile già calcolata nel fuso dell'utente; i promemoria ricevono
//! l'istante corrente. Così la logica è deterministica e testabile con fusi e
//! cambi d'ora arbitrari.

use jiff::{Timestamp, civil::Date};
use rusqlite::{Connection, ErrorCode, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    domain::planner::{
        CalendarEvent, EventInput, Reminder, ReminderEntity, ReminderStatus, SnoozePreset, Task,
        TaskCounts, TaskInput, TaskStatus, TaskView,
    },
    domain::recurrence::Recurrence,
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

pub(crate) const TASK_COLUMNS: &str = "tasks.id, tasks.note_id,
    (SELECT notes.title FROM notes WHERE notes.id = tasks.note_id AND notes.deleted_at IS NULL),
    tasks.title, tasks.description, tasks.status, tasks.priority, tasks.due_date,
    tasks.completed_at, tasks.created_at, tasks.updated_at,
    (SELECT MIN(reminders.remind_at) FROM reminders
     WHERE reminders.entity_type = 'task' AND reminders.entity_id = tasks.id
       AND reminders.status != 'done'),
    tasks.flagged, tasks.recurrence";

pub(crate) const EVENT_COLUMNS: &str = "events.id, events.note_id,
    (SELECT notes.title FROM notes WHERE notes.id = events.note_id AND notes.deleted_at IS NULL),
    events.title, events.description, events.all_day, events.start_at, events.end_at,
    events.start_date, events.end_date, events.timezone, events.created_at, events.updated_at,
    events.external_calendar_id, events.calendar_provider, events.read_only, events.web_link,
    events.subscription_id, events.recurrence, events.recurrence_exceptions";

/// Colonne di un promemoria con titolo e nota dell'entità. Usa l'alias `r`.
pub(crate) const REMINDER_COLUMNS: &str =
    "r.id, r.entity_type, r.entity_id, r.remind_at, r.timezone,
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
    r.updated_at, r.recurrence,
    r.entity_type = 'event' AND EXISTS(SELECT 1 FROM events
        WHERE events.id = r.entity_id AND events.recurrence IS NOT NULL)";

/// Vero se l'entità del promemoria `r` esiste ed è attiva. Le note nel
/// cestino conservano i promemoria, che riprendono dopo il ripristino.
pub(crate) const REMINDER_ENTITY_ACTIVE: &str = "CASE r.entity_type
    WHEN 'note' THEN EXISTS(SELECT 1 FROM notes WHERE id = r.entity_id AND deleted_at IS NULL)
    WHEN 'task' THEN EXISTS(SELECT 1 FROM tasks
                            WHERE id = r.entity_id AND status = 'open' AND deleted_at IS NULL)
    WHEN 'event' THEN EXISTS(SELECT 1 FROM events WHERE id = r.entity_id AND deleted_at IS NULL)
    ELSE 0 END";

pub(crate) fn task_from_row(row: &Row<'_>) -> rusqlite::Result<Task> {
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
        recurrence: parse_recurrence(row.get::<_, Option<String>>(13)?.as_deref()),
    })
}

/// Regola salvata; una regola illeggibile (dati alterati o di una versione
/// futura) equivale a nessuna ripetizione.
fn parse_recurrence(value: Option<&str>) -> Option<Recurrence> {
    serde_json::from_str(value?).ok()
}

/// Date tolte da una serie di eventi; valori non validi vengono ignorati.
fn parse_exceptions(value: Option<&str>) -> Vec<String> {
    let dates: Vec<String> = value
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default();
    dates
        .into_iter()
        .filter(|date| time::parse_date(date).is_ok())
        .collect()
}

/// Occorrenze tolte al massimo da una serie (limite della colonna).
const MAX_EXCEPTIONS: usize = 2000;

/// Id della prossima occorrenza, derivato dall'attività completata e dalla
/// nuova scadenza: due dispositivi che completano la stessa attività creano
/// la stessa riga, che la sincronizzazione unisce invece di duplicare.
fn occurrence_id(previous: &str, due: Date) -> String {
    let digest = Sha256::digest(format!("greatnote-occurrence/{previous}/{due}"));
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Uuid::new_v8(bytes).to_string()
}

pub(crate) fn event_from_row(row: &Row<'_>) -> rusqlite::Result<CalendarEvent> {
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
        recurrence: parse_recurrence(row.get::<_, Option<String>>(18)?.as_deref()),
        recurrence_exceptions: parse_exceptions(row.get::<_, Option<String>>(19)?.as_deref()),
        occurrence_date: None,
    })
}

pub(crate) fn reminder_from_row(row: &Row<'_>) -> rusqlite::Result<Reminder> {
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
        recurrence: parse_recurrence(row.get::<_, Option<String>>(11)?.as_deref()),
        repeats_with_event: row.get(12)?,
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
    /// Regola validata in JSON.
    recurrence: Option<String>,
}

fn validate_task(input: &TaskInput) -> Result<ValidTask, StorageError> {
    let title = normalize_title(
        &input.title,
        StorageError::InvalidTask(crate::tr_str!(
            "il titolo deve contenere da 1 a 512 caratteri",
            "title must contain between 1 and 512 characters"
        )),
    )?;
    let description = normalize_description(
        &input.description,
        StorageError::InvalidTask(crate::tr_str!(
            "descrizione troppo lunga",
            "description too long"
        )),
    )?;
    if input.priority > 3 {
        return Err(StorageError::InvalidTask(crate::tr_str!(
            "priorità non valida",
            "invalid priority"
        )));
    }
    let due_date = match input.due_date.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(value) => Some(time::parse_date(value)?.to_string()),
    };
    let recurrence = match &input.recurrence {
        None => None,
        Some(rule) => {
            let due = due_date
                .as_deref()
                .ok_or(StorageError::InvalidTask(crate::tr_str!(
                    "per ripetere un'attività serve una scadenza",
                    "a due date is required to repeat a task"
                )))?;
            let rule = rule
                .clone()
                .validated(time::parse_date(due)?)
                .map_err(StorageError::InvalidTask)?;
            Some(serde_json::to_string(&rule).map_err(|_| {
                StorageError::InvalidTask(crate::tr_str!(
                    "ripetizione non valida",
                    "invalid repetition"
                ))
            })?)
        }
    };
    Ok(ValidTask {
        flagged: input.flagged,
        title,
        description,
        priority: input.priority,
        due_date,
        note_id: input.note_id.clone().filter(|id| !id.is_empty()),
        recurrence,
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
    /// Regola in JSON, già validata per la data d'inizio.
    pub(crate) recurrence: Option<String>,
}

impl ValidEvent {
    /// Data d'inizio nel fuso dell'evento: il punto di partenza della serie.
    fn anchor(&self) -> Result<Date, StorageError> {
        match (&self.start_date, &self.start_at) {
            (Some(date), _) => time::parse_date(date),
            (None, Some(start)) => Ok(time::parse_utc(start)?
                .to_zoned(Zone::stored(&self.timezone).tz)
                .date()),
            (None, None) => Err(StorageError::InvalidEvent(crate::tr_str!(
                "inizio mancante",
                "missing start"
            ))),
        }
    }
}

/// Valida la regola di un evento rispetto alla sua data d'inizio.
fn validated_event_recurrence(
    event: &ValidEvent,
    rule: Option<&Recurrence>,
) -> Result<Option<String>, StorageError> {
    let Some(rule) = rule else {
        return Ok(None);
    };
    let anchor = event.anchor()?;
    let rule = rule
        .clone()
        .validated(anchor)
        .map_err(StorageError::InvalidEvent)?;
    Ok(Some(serde_json::to_string(&rule)?))
}

pub(crate) fn validate_event(input: &EventInput) -> Result<ValidEvent, StorageError> {
    let mut event = validate_event_times(input)?;
    event.recurrence = validated_event_recurrence(&event, input.recurrence.as_ref())?;
    Ok(event)
}

fn validate_event_times(input: &EventInput) -> Result<ValidEvent, StorageError> {
    let title = normalize_title(
        &input.title,
        StorageError::InvalidEvent(crate::tr_str!(
            "il titolo deve contenere da 1 a 512 caratteri",
            "title must contain between 1 and 512 characters"
        )),
    )?;
    let description = normalize_description(
        &input.description,
        StorageError::InvalidEvent(crate::tr_str!(
            "descrizione troppo lunga",
            "description too long"
        )),
    )?;
    let zone = Zone::resolve(input.timezone.as_deref())?;
    let note_id = input.note_id.clone().filter(|id| !id.is_empty());
    if input.all_day {
        let start = time::parse_date(input.start.trim())?;
        let end = time::parse_date(input.end.trim())?;
        let days = (end - start).get_days() as i64;
        if days < 0 {
            return Err(StorageError::InvalidEvent(crate::tr_str!(
                "la fine precede l'inizio",
                "end precedes start"
            )));
        }
        if days >= MAX_EVENT_DAYS {
            return Err(StorageError::InvalidEvent(crate::tr_str!(
                "l'evento può durare al massimo un anno",
                "event can last at most one year"
            )));
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
            recurrence: None,
        })
    } else {
        let start = time::local_to_utc(input.start.trim(), &zone)?;
        let end = time::local_to_utc(input.end.trim(), &zone)?;
        if end < start {
            return Err(StorageError::InvalidEvent(crate::tr_str!(
                "la fine precede l'inizio",
                "end precedes start"
            )));
        }
        if end.as_second() - start.as_second() > MAX_EVENT_DAYS * 86_400 {
            return Err(StorageError::InvalidEvent(crate::tr_str!(
                "l'evento può durare al massimo un anno",
                "event can last at most one year"
            )));
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
            recurrence: None,
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
            "INSERT INTO tasks (id, note_id, title, description, priority, due_date, flagged,
                 recurrence)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                task.note_id,
                task.title,
                task.description,
                task.priority,
                task.due_date,
                task.flagged,
                task.recurrence
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
                     due_date = ?5, flagged = ?6, recurrence = ?7, updated_at = {NOW_SQL}
                 WHERE id = ?8"
            ),
            params![
                task.note_id,
                task.title,
                task.description,
                task.priority,
                task.due_date,
                task.flagged,
                task.recurrence,
                id
            ],
        )?;
        get_task(&connection, id)
    }

    /// Completa o riapre un'attività. Completandola, i suoi promemoria
    /// vengono chiusi nella stessa transazione; se si ripete nasce la
    /// successiva (vedi [`schedule_next_occurrence`]). `now` e `zone`
    /// stabiliscono "oggi" e gli orari dei promemoria copiati.
    pub fn set_task_completed(
        &self,
        id: &str,
        completed: bool,
        now: Timestamp,
        zone: &Zone,
    ) -> Result<Task, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let previous = get_task(&transaction, id)?;
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
        if completed && previous.status == TaskStatus::Open {
            schedule_next_occurrence(&transaction, &previous, now, zone)?;
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
            return Err(StorageError::InvalidSearchQuery(crate::tr!(
                "la ricerca può contenere al massimo 200 caratteri",
                "search can contain at most 200 characters"
            )));
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
        // Confronto senza maiuscole né accenti (`gn_fold`, registrata su
        // ogni connessione): «citta» trova «Città».
        let mut statement = connection.prepare(&format!(
            "SELECT {TASK_COLUMNS} FROM tasks
             WHERE {condition} AND deleted_at IS NULL
               AND (?2 IS NULL OR instr(gn_fold(title || ' ' || description), gn_fold(?2)) > 0)
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
        insert_event(&connection, &id, &event)?;
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
                     recurrence = ?10,
                     -- Senza ripetizione non restano occorrenze da escludere.
                     recurrence_exceptions = CASE WHEN ?10 IS NULL THEN NULL
                                                  ELSE recurrence_exceptions END,
                     updated_at = {NOW_SQL}
                 WHERE id = ?11"
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
                event.recurrence,
                id
            ],
        )?;
        get_event(&connection, id)
    }

    /// Toglie dalla serie l'occorrenza che inizia il giorno `date` (per
    /// esempio «Elimina solo questa»).
    pub fn skip_event_occurrence(
        &self,
        id: &str,
        date: &str,
    ) -> Result<CalendarEvent, StorageError> {
        let connection = self.connection()?;
        let event = editable_series(&connection, id)?;
        add_exception(&connection, &event, date)?;
        get_event(&connection, id)
    }

    /// Modifica una sola occorrenza: viene tolta dalla serie e diventa un
    /// evento a sé con i dati indicati. Restituisce il nuovo evento.
    pub fn detach_event_occurrence(
        &self,
        id: &str,
        date: &str,
        input: &EventInput,
    ) -> Result<CalendarEvent, StorageError> {
        let mut detached = input.clone();
        detached.recurrence = None;
        let event = validate_event(&detached)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let series = editable_series(&transaction, id)?;
        ensure_active_note(&transaction, event.note_id.as_deref())?;
        add_exception(&transaction, &series, date)?;
        let new_id = occurrence_id(id, time::parse_date(date)?);
        insert_event(&transaction, &new_id, &event)?;
        let created = get_event(&transaction, &new_id)?;
        transaction.commit()?;
        Ok(created)
    }

    /// Termina la serie prima dell'occorrenza del giorno `date` («Elimina
    /// questa e le successive»). Se era la prima, l'evento va nel cestino
    /// come gli altri: lo fa il chiamante, qui restituisce `None`.
    pub fn end_event_series_before(
        &self,
        id: &str,
        date: &str,
    ) -> Result<Option<CalendarEvent>, StorageError> {
        let connection = self.connection()?;
        let event = editable_series(&connection, id)?;
        let date = time::parse_date(date)?;
        let (Some(rule), Some(anchor)) = (&event.recurrence, series_anchor(&event)) else {
            return Err(not_recurring());
        };
        if date <= anchor {
            return Ok(None);
        }
        let mut rule = rule.clone();
        rule.until = Some(
            date.checked_sub(jiff::Span::new().days(1))
                .map_err(|_| not_recurring())?
                .to_string(),
        );
        let rule = rule.validated(anchor).map_err(StorageError::InvalidEvent)?;
        connection.execute(
            &format!("UPDATE events SET recurrence = ?1, updated_at = {NOW_SQL} WHERE id = ?2"),
            params![serde_json::to_string(&rule)?, id],
        )?;
        get_event(&connection, id).map(Some)
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
               AND recurrence IS NULL
               AND ((all_day = 0 AND start_at < ?2 AND (end_at > ?1 OR start_at >= ?1))
                    OR (all_day = 1 AND start_date <= ?4 AND end_date >= ?3))
             ORDER BY all_day DESC, COALESCE(start_at, start_date), title
             LIMIT 2000"
        ))?;
        let rows = statement.query_map(
            params![start, end, from.to_string(), to.to_string()],
            event_from_row,
        )?;
        let mut events = rows.collect::<Result<Vec<_>, _>>()?;
        // Le serie ricorrenti diventano le loro occorrenze nell'intervallo.
        let mut series = connection.prepare(&format!(
            "SELECT {EVENT_COLUMNS} FROM events
             WHERE deleted_at IS NULL AND recurrence IS NOT NULL
               AND COALESCE(start_date, substr(start_at, 1, 10)) <= ?1"
        ))?;
        let series = series
            .query_map([time::next_day(to)?.to_string()], event_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        let range_start = time::parse_utc(&start)?;
        let range_end = time::parse_utc(&end)?;
        for event in &series {
            events.extend(expand_occurrences(event, from, to, range_start, range_end));
        }
        events.sort_by(|a, b| {
            b.all_day
                .cmp(&a.all_day)
                .then_with(|| {
                    a.start_at
                        .as_deref()
                        .or(a.start_date.as_deref())
                        .cmp(&b.start_at.as_deref().or(b.start_date.as_deref()))
                })
                .then_with(|| a.title.cmp(&b.title))
        });
        events.truncate(2000);
        Ok(events)
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
        self.create_repeating_reminder(entity, entity_id, local, timezone, now, None)
    }

    /// Come [`Database::create_reminder`], con una ripetizione propria
    /// (solo per le note: i promemoria di attività ed eventi seguono la
    /// ripetizione del loro elemento).
    pub fn create_repeating_reminder(
        &self,
        entity: ReminderEntity,
        entity_id: &str,
        local: &str,
        timezone: Option<&str>,
        now: Timestamp,
        recurrence: Option<&Recurrence>,
    ) -> Result<Reminder, StorageError> {
        if recurrence.is_some() && entity != ReminderEntity::Note {
            return Err(StorageError::InvalidReminder(crate::tr_str!(
                "solo i promemoria delle note hanno una ripetizione propria",
                "only note reminders have their own repetition"
            )));
        }
        let zone = Zone::resolve(timezone)?;
        let remind_at = time::local_to_utc(local, &zone)?;
        if remind_at < time::add_minutes(now, -PAST_TOLERANCE_MINUTES)? {
            return Err(StorageError::InvalidReminder(crate::tr_str!(
                "l'orario è già passato",
                "time has already passed"
            )));
        }
        let recurrence = match recurrence {
            Some(rule) => {
                let anchor = remind_at.to_zoned(zone.tz.clone()).date();
                let rule = rule
                    .clone()
                    .validated(anchor)
                    .map_err(StorageError::InvalidReminder)?;
                Some(serde_json::to_string(&rule)?)
            }
            None => None,
        };
        let id = Uuid::new_v4().to_string();
        let connection = self.connection()?;
        ensure_reminder_entity(&connection, entity, entity_id)?;
        connection.execute(
            "INSERT INTO reminders (id, entity_type, entity_id, remind_at, timezone, local_time,
                 recurrence)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                id,
                entity.as_str(),
                entity_id,
                time::format_utc(remind_at),
                zone.name,
                time::local_wall(remind_at, &zone),
                recurrence
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
            let reminder = get_reminder(&transaction, id)?;
            schedule_next_reminder(&transaction, &reminder, now)?;
            due.push(reminder);
        }
        transaction.commit()?;
        Ok(due)
    }
}

/// Crea l'occorrenza successiva di un'attività ricorrente appena completata:
/// stessi dati e stessa regola, scadenza alla prossima data utile da oggi in
/// poi, e una copia dei promemoria spostati degli stessi giorni (solo quelli
/// che cadono nel futuro). Idempotente: l'id dipende dall'attività e dalla
/// data, quindi completarla di nuovo dopo averla riaperta non duplica nulla.
fn schedule_next_occurrence(
    connection: &Connection,
    task: &Task,
    now: Timestamp,
    zone: &Zone,
) -> Result<Option<String>, StorageError> {
    let (Some(rule), Some(due)) = (&task.recurrence, task.due_date.as_deref()) else {
        return Ok(None);
    };
    let due = time::parse_date(due)?;
    let Some(next) = rule.next_due(due, time::today(now, zone)) else {
        return Ok(None);
    };
    let next_id = occurrence_id(&task.id, next);
    let inserted = connection.execute(
        "INSERT OR IGNORE INTO tasks (id, note_id, title, description, priority, due_date,
             flagged, recurrence)
         SELECT ?1, note_id, title, description, priority, ?2, flagged, recurrence
         FROM tasks WHERE id = ?3",
        params![next_id, next.to_string(), task.id],
    )?;
    if inserted == 0 {
        return Ok(Some(next_id));
    }
    let shift = due
        .until(next)
        .map_err(|_| {
            StorageError::InvalidTask(crate::tr_str!(
                "ripetizione non valida",
                "invalid repetition"
            ))
        })?
        .get_days();
    let reminders: Vec<(String, String, String)> = connection
        .prepare(
            "SELECT id, timezone, local_time FROM reminders
             WHERE entity_type = 'task' AND entity_id = ?1 ORDER BY remind_at",
        )?
        .query_map([&task.id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<Result<_, _>>()?;
    for (reminder_id, timezone, local_time) in reminders {
        let reminder_zone = Zone::stored(&timezone);
        let Ok(local) = time::parse_local(&local_time) else {
            continue;
        };
        let Ok(moved) = local.checked_add(jiff::Span::new().days(i64::from(shift))) else {
            continue;
        };
        let wall = moved.strftime("%Y-%m-%dT%H:%M").to_string();
        let Ok(remind_at) = time::local_to_utc(&wall, &reminder_zone) else {
            continue;
        };
        if remind_at <= now {
            continue;
        }
        connection.execute(
            "INSERT OR IGNORE INTO reminders (id, entity_type, entity_id, remind_at, timezone,
                 local_time)
             VALUES (?1, 'task', ?2, ?3, ?4, ?5)",
            params![
                occurrence_id(&reminder_id, next),
                next_id,
                time::format_utc(remind_at),
                reminder_zone.name,
                time::local_wall(remind_at, &reminder_zone)
            ],
        )?;
    }
    Ok(Some(next_id))
}

// ---------------------------------------------------------------------------
// Archivio completo (campi facoltativi di `gna-archive` v1)

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ArchiveTask {
    #[serde(default)]
    flagged: bool,
    /// Regola di ripetizione in JSON (assente negli archivi precedenti).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) recurrence: Option<String>,
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
    /// Ripetizione ed eccezioni in JSON (assenti negli archivi precedenti).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recurrence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recurrence_exceptions: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recurrence: Option<String>,
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
                    completed_at, created_at, updated_at, flagged, recurrence
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
                recurrence: row.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let events = connection
        .prepare(&format!(
            "SELECT id, {ACTIVE_NOTE}, title, description, all_day, start_at, end_at,
                    start_date, end_date, timezone, created_at, updated_at, recurrence,
                    recurrence_exceptions
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
                recurrence: row.get(12)?,
                recurrence_exceptions: row.get(13)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let reminders = connection
        .prepare(
            "SELECT id, entity_type, entity_id, remind_at, timezone, local_time, status,
                    last_triggered_at, recurrence
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
                recurrence: row.get(8)?,
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
/// Regola di un evento importato, solo se valida per la sua data d'inizio.
fn archived_event_recurrence(event: &ArchiveEvent) -> Option<String> {
    let rule = parse_recurrence(event.recurrence.as_deref())?;
    let anchor = match (&event.start_date, &event.start_at) {
        (Some(date), _) if event.all_day => time::parse_date(date).ok()?,
        (_, Some(start)) => time::parse_utc(start)
            .ok()?
            .to_zoned(Zone::stored(&event.timezone).tz)
            .date(),
        _ => return None,
    };
    serde_json::to_string(&rule.validated(anchor).ok()?).ok()
}

/// Regola di un'attività importata, solo se valida per la sua scadenza.
fn archived_recurrence(task: &ArchiveTask) -> Option<String> {
    let rule = parse_recurrence(task.recurrence.as_deref())?;
    let due = time::parse_date(task.due_date.as_deref()?).ok()?;
    serde_json::to_string(&rule.validated(due).ok()?).ok()
}

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
                     due_date, completed_at, created_at, updated_at, flagged, recurrence)
                 VALUES (?1, {NOTE_IF_PRESENT}, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)"
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
                task.flagged,
                archived_recurrence(task)
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
                     start_at, end_at, start_date, end_date, timezone, created_at, updated_at,
                     recurrence, recurrence_exceptions)
                 VALUES (?1, {NOTE_IF_PRESENT}, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12,
                         ?13, ?14)"
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
                event.updated_at,
                archived_event_recurrence(event),
                archived_event_recurrence(event).and(
                    event
                        .recurrence_exceptions
                        .as_deref()
                        .map(|value| serde_json::to_string(&parse_exceptions(Some(value))))
                        .transpose()?
                )
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
                 local_time, status, last_triggered_at, recurrence)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6,
                     CASE WHEN ?7 = 'pending' AND ?4 <= ?9 THEN 'fired' ELSE ?7 END, ?8, ?10)",
            params![
                reminder.id,
                entity.as_str(),
                reminder.entity_id,
                reminder.remind_at,
                reminder.timezone,
                reminder.local_time,
                reminder.status,
                reminder.last_triggered_at,
                now_text,
                // Solo le note hanno una ripetizione propria.
                (entity == ReminderEntity::Note)
                    .then(|| parse_recurrence(reminder.recurrence.as_deref()))
                    .flatten()
                    .and_then(|rule| serde_json::to_string(&rule).ok())
            ],
        )?;
        imported += u64::from(inserted);
    }
    Ok(imported)
}

fn insert_event(connection: &Connection, id: &str, event: &ValidEvent) -> Result<(), StorageError> {
    connection.execute(
        "INSERT INTO events (id, note_id, title, description, all_day, start_at, end_at,
                             start_date, end_date, timezone, recurrence)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
            event.timezone,
            event.recurrence
        ],
    )?;
    Ok(())
}

fn not_recurring() -> StorageError {
    StorageError::InvalidEvent(crate::tr_str!(
        "l'evento non si ripete",
        "the event does not repeat"
    ))
}

/// Evento ricorrente di Greatnote, modificabile.
fn editable_series(connection: &Connection, id: &str) -> Result<CalendarEvent, StorageError> {
    let event = get_event(connection, id)?;
    if event.calendar_provider.is_some() || event.read_only {
        return Err(StorageError::ExternalEvent);
    }
    if event.recurrence.is_none() {
        return Err(not_recurring());
    }
    Ok(event)
}

/// Data d'inizio della serie nel fuso dell'evento.
fn series_anchor(event: &CalendarEvent) -> Option<Date> {
    match (&event.start_date, &event.start_at) {
        (Some(date), _) => time::parse_date(date).ok(),
        (None, Some(start)) => Some(
            time::parse_utc(start)
                .ok()?
                .to_zoned(Zone::stored(&event.timezone).tz)
                .date(),
        ),
        _ => None,
    }
}

fn add_exception(
    connection: &Connection,
    event: &CalendarEvent,
    date: &str,
) -> Result<(), StorageError> {
    let date = time::parse_date(date)?;
    let (Some(rule), Some(anchor)) = (&event.recurrence, series_anchor(event)) else {
        return Err(not_recurring());
    };
    if !rule.occurrences(anchor, date, date, 1).contains(&date) {
        return Err(StorageError::InvalidEvent(crate::tr_str!(
            "in quel giorno l'evento non si ripete",
            "the event does not repeat on that day"
        )));
    }
    let mut exceptions = event.recurrence_exceptions.clone();
    let date = date.to_string();
    if !exceptions.contains(&date) {
        if exceptions.len() >= MAX_EXCEPTIONS {
            return Err(StorageError::InvalidEvent(crate::tr_str!(
                "troppe occorrenze tolte dalla serie",
                "too many occurrences removed from the series"
            )));
        }
        exceptions.push(date);
        exceptions.sort();
    }
    connection.execute(
        &format!(
            "UPDATE events SET recurrence_exceptions = ?1, updated_at = {NOW_SQL} WHERE id = ?2"
        ),
        params![serde_json::to_string(&exceptions)?, event.id],
    )?;
    Ok(())
}

/// Inizio e fine di un'occorrenza (il giorno `date`) di un evento con
/// orario: stessa ora locale del primo, nel fuso dell'evento, e stessa
/// durata. Così l'ora legale non sposta l'evento.
fn timed_occurrence(event: &CalendarEvent, date: Date) -> Option<(Timestamp, Timestamp)> {
    let zone = Zone::stored(&event.timezone);
    let start = time::parse_utc(event.start_at.as_deref()?).ok()?;
    let end = time::parse_utc(event.end_at.as_deref()?).ok()?;
    let local = start.to_zoned(zone.tz.clone()).datetime();
    let wall = date.at(local.hour(), local.minute(), local.second(), 0);
    let occurrence = time::local_to_zoned(wall, &zone).ok()?.timestamp();
    let duration = end.as_second() - start.as_second();
    let occurrence_end = Timestamp::from_second(occurrence.as_second() + duration).ok()?;
    Some((occurrence, occurrence_end))
}

/// Occorrenze di una serie che intersecano i giorni `from..=to` (con
/// gli istanti `range_start..range_end` per gli eventi con orario).
fn expand_occurrences(
    event: &CalendarEvent,
    from: Date,
    to: Date,
    range_start: Timestamp,
    range_end: Timestamp,
) -> Vec<CalendarEvent> {
    let (Some(rule), Some(anchor)) = (&event.recurrence, series_anchor(event)) else {
        return Vec::new();
    };
    // Giorni coperti da un'occorrenza: le occorrenze iniziate prima
    // dell'intervallo possono ancora intersecarlo.
    let span_days = match (
        &event.start_date,
        &event.end_date,
        &event.start_at,
        &event.end_at,
    ) {
        (Some(start), Some(end), _, _) => match (time::parse_date(start), time::parse_date(end)) {
            (Ok(start), Ok(end)) => (end - start).get_days().max(0),
            _ => 0,
        },
        (_, _, Some(start), Some(end)) => match (time::parse_utc(start), time::parse_utc(end)) {
            (Ok(start), Ok(end)) => ((end.as_second() - start.as_second()) / 86_400 + 1) as i32,
            _ => 0,
        },
        _ => 0,
    };
    let Ok(earliest) = from.checked_sub(jiff::Span::new().days(i64::from(span_days) + 1)) else {
        return Vec::new();
    };
    let Ok(latest) = to.checked_add(jiff::Span::new().days(1)) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for date in rule.occurrences(anchor, earliest, latest, 1000) {
        let date_text = date.to_string();
        if event.recurrence_exceptions.contains(&date_text) {
            continue;
        }
        let mut occurrence = event.clone();
        occurrence.occurrence_date = Some(date_text.clone());
        if event.all_day {
            let Ok(end) = date.checked_add(jiff::Span::new().days(i64::from(span_days))) else {
                continue;
            };
            if date > to || end < from {
                continue;
            }
            occurrence.start_date = Some(date_text);
            occurrence.end_date = Some(end.to_string());
        } else {
            let Some((start, end)) = timed_occurrence(event, date) else {
                continue;
            };
            if start >= range_end || (end <= range_start && start < range_start) {
                continue;
            }
            occurrence.start_at = Some(time::format_utc(start));
            occurrence.end_at = Some(time::format_utc(end));
        }
        found.push(occurrence);
    }
    found
}

/// Inizio della prima occorrenza di un evento ricorrente che comincia da
/// `not_before` in poi, saltando le date escluse.
fn next_occurrence_start(event: &CalendarEvent, not_before: Timestamp) -> Option<Timestamp> {
    let rule = event.recurrence.as_ref()?;
    let anchor = series_anchor(event)?;
    let zone = Zone::stored(&event.timezone);
    // Un giorno di margine: il giorno locale può precedere quello UTC.
    let from = not_before
        .to_zoned(zone.tz.clone())
        .date()
        .checked_sub(jiff::Span::new().days(1))
        .ok()?;
    let to = from.checked_add(jiff::Span::new().days(800)).ok()?;
    rule.occurrences(anchor, from, to, 400)
        .into_iter()
        .filter(|date| !event.recurrence_exceptions.contains(&date.to_string()))
        .find_map(|date| {
            let start = if event.all_day {
                time::day_start(date, &zone).ok()?
            } else {
                timed_occurrence(event, date)?.0
            };
            (start >= not_before).then_some(start)
        })
}

/// Dopo che un promemoria è scattato, crea il successivo se si ripete: per
/// una nota con la sua regola, alla stessa ora locale; per un evento
/// ricorrente, con lo stesso anticipo sull'occorrenza seguente. L'id deriva
/// da quello del promemoria, quindi due dispositivi non creano doppioni.
fn schedule_next_reminder(
    connection: &Connection,
    reminder: &Reminder,
    now: Timestamp,
) -> Result<(), StorageError> {
    let zone = Zone::stored(&reminder.timezone);
    let current = time::parse_utc(&reminder.remind_at)?;
    let next = match reminder.entity_type {
        ReminderEntity::Note => {
            let Some(rule) = &reminder.recurrence else {
                return Ok(());
            };
            let Ok(local) = time::parse_local(&reminder.local_time) else {
                return Ok(());
            };
            let anchor = local.date();
            let today = time::today(now, &zone);
            // Prima data utile da oggi in poi (le occorrenze perse non si
            // ricreano), poi la prima con l'orario ancora da venire.
            let mut date = rule.next_due(anchor, today);
            let mut found = None;
            for _ in 0..3 {
                let Some(day) = date else { break };
                let wall = day.at(local.hour(), local.minute(), 0, 0);
                let at = time::local_to_zoned(wall, &zone)?.timestamp();
                if at > now {
                    found = Some((day, at));
                    break;
                }
                date = rule.after(anchor, day);
            }
            let Some((day, at)) = found else {
                return Ok(());
            };
            Some((occurrence_id(&reminder.id, day), at))
        }
        ReminderEntity::Event => {
            let event = match get_event(connection, &reminder.entity_id) {
                Ok(event) if event.recurrence.is_some() && event.calendar_provider.is_none() => {
                    event
                }
                _ => return Ok(()),
            };
            // Occorrenza di questo promemoria: la prima che inizia dal suo
            // orario in poi; l'anticipo resta lo stesso per la successiva.
            let Some(this_start) = next_occurrence_start(&event, current) else {
                return Ok(());
            };
            let lead = this_start.as_second() - current.as_second();
            let mut after =
                Timestamp::from_second(this_start.as_second() + 1).map_err(|_| not_recurring())?;
            let mut found = None;
            for _ in 0..400 {
                let Some(start) = next_occurrence_start(&event, after) else {
                    break;
                };
                let at = Timestamp::from_second(start.as_second() - lead)
                    .map_err(|_| not_recurring())?;
                if at > now {
                    found = Some((start, at));
                    break;
                }
                after =
                    Timestamp::from_second(start.as_second() + 1).map_err(|_| not_recurring())?;
            }
            let Some((start, at)) = found else {
                return Ok(());
            };
            let day = start.to_zoned(Zone::stored(&event.timezone).tz).date();
            Some((occurrence_id(&reminder.id, day), at))
        }
        ReminderEntity::Task => None,
    };
    let Some((id, at)) = next else {
        return Ok(());
    };
    connection.execute(
        "INSERT OR IGNORE INTO reminders (id, entity_type, entity_id, remind_at, timezone,
             local_time, recurrence)
         SELECT ?1, entity_type, entity_id, ?2, timezone, ?3, recurrence
         FROM reminders WHERE id = ?4",
        params![
            id,
            time::format_utc(at),
            time::local_wall(at, &zone),
            reminder.id
        ],
    )?;
    Ok(())
}

fn validate_range(from: Date, to: Date) -> Result<(), StorageError> {
    let days = (to - from).get_days() as i64;
    if !(0..=MAX_RANGE_DAYS).contains(&days) {
        return Err(StorageError::InvalidDateTime(crate::tr!(
            "intervallo di date non valido",
            "invalid date range"
        )));
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
            recurrence: None,
        }
    }

    fn date(value: &str) -> jiff::civil::Date {
        time::parse_date(value).expect("date")
    }

    fn instant(value: &str) -> jiff::Timestamp {
        time::parse_utc(value).expect("instant")
    }

    fn weekly() -> crate::domain::recurrence::Recurrence {
        serde_json::from_str(r#"{"frequency":"weekly"}"#).expect("rule")
    }

    #[test]
    fn completing_a_recurring_task_schedules_the_next_one_once() {
        let database = Database::in_memory().expect("database");
        let rome = Zone::stored("Europe/Rome");
        let mut input = task("Bollette", Some("2026-10-19"));
        input.recurrence = Some(weekly());
        input.priority = 2;
        let first = database.create_task(&input).expect("task");
        // Promemoria alle 9:00 di Roma (ora legale: 07:00 UTC).
        database
            .create_reminder(
                ReminderEntity::Task,
                &first.id,
                "2026-10-19T09:00",
                Some("Europe/Rome"),
                instant("2026-10-18T10:00:00Z"),
            )
            .expect("reminder");

        let now = instant("2026-10-19T12:00:00Z");
        let done = database
            .set_task_completed(&first.id, true, now, &rome)
            .expect("done");
        assert_eq!(done.status, TaskStatus::Done);
        let open = database
            .list_tasks(TaskView::Open, date("2026-10-19"), None)
            .expect("open");
        assert_eq!(open.len(), 1);
        let next = &open[0];
        assert_ne!(next.id, first.id);
        assert_eq!(next.due_date.as_deref(), Some("2026-10-26"));
        assert_eq!((next.title.as_str(), next.priority), ("Bollette", 2));
        assert_eq!(next.recurrence, first.recurrence);
        // Il promemoria segue la scadenza alla stessa ora locale: dopo la fine
        // dell'ora legale (25 ottobre) le 9:00 di Roma sono le 08:00 UTC.
        assert_eq!(
            next.next_reminder_at.as_deref(),
            Some("2026-10-26T08:00:00Z")
        );

        // Riaprirla e completarla di nuovo non crea un doppione.
        database
            .set_task_completed(&first.id, false, now, &rome)
            .expect("reopen");
        database
            .set_task_completed(&first.id, true, now, &rome)
            .expect("done again");
        let open = database
            .list_tasks(TaskView::Open, date("2026-10-19"), None)
            .expect("open");
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].id, next.id);
    }

    #[test]
    fn overdue_series_skip_missed_dates_and_stop_at_the_end() {
        let database = Database::in_memory().expect("database");
        let rome = Zone::stored("Europe/Rome");
        let mut input = task("Report", Some("2026-10-05"));
        let mut rule = weekly();
        rule.until = Some("2026-10-31".to_owned());
        input.recurrence = Some(rule);
        let late = database.create_task(&input).expect("task");
        // Completata giovedì 22: la prossima è lunedì 26, non il 12 o il 19.
        database
            .set_task_completed(&late.id, true, instant("2026-10-22T10:00:00Z"), &rome)
            .expect("done");
        let open = database
            .list_tasks(TaskView::Open, date("2026-10-22"), None)
            .expect("open");
        assert_eq!(open[0].due_date.as_deref(), Some("2026-10-26"));
        // Oltre la data di fine la serie termina.
        database
            .set_task_completed(&open[0].id, true, instant("2026-10-26T10:00:00Z"), &rome)
            .expect("done");
        assert!(
            database
                .list_tasks(TaskView::Open, date("2026-10-26"), None)
                .expect("open")
                .is_empty()
        );
    }

    #[test]
    fn task_search_ignores_case_and_accents() {
        let database = Database::in_memory().expect("database");
        database
            .create_task(&task("Visita in Città", None))
            .expect("task");
        database
            .create_task(&task("ÉTÉ à Paris", None))
            .expect("task");
        let found = |search: &str| {
            database
                .list_tasks(TaskView::NoDate, date("2026-11-25"), Some(search))
                .expect("search")
                .into_iter()
                .map(|task| task.title)
                .collect::<Vec<_>>()
        };
        assert_eq!(found("città"), ["Visita in Città"]);
        assert_eq!(found("CITTA"), ["Visita in Città"]);
        assert_eq!(found("été"), ["ÉTÉ à Paris"]);
        assert!(found("roma").is_empty());
    }

    fn recurring_event(title: &str, start: &str, end: &str, rule: &str) -> EventInput {
        EventInput {
            title: title.to_owned(),
            description: String::new(),
            all_day: !start.contains('T'),
            start: start.to_owned(),
            end: end.to_owned(),
            timezone: Some("Europe/Rome".to_owned()),
            note_id: None,
            recurrence: (!rule.is_empty()).then(|| serde_json::from_str(rule).expect("rule")),
        }
    }

    fn occurrence_starts(
        database: &Database,
        from: &str,
        to: &str,
    ) -> Vec<(Option<String>, Option<String>)> {
        database
            .list_events(date(from), date(to), &Zone::stored("Europe/Rome"))
            .expect("events")
            .into_iter()
            .map(|event| (event.occurrence_date, event.start_at.or(event.start_date)))
            .collect()
    }

    #[test]
    fn recurring_events_keep_their_local_time_across_daylight_saving() {
        let database = Database::in_memory().expect("database");
        let series = database
            .create_event(&recurring_event(
                "Riunione",
                "2026-03-23T09:00",
                "2026-03-23T10:00",
                r#"{"frequency":"weekly"}"#,
            ))
            .expect("event");
        assert!(series.recurrence.is_some());
        // Il 29 marzo inizia l'ora legale: sempre alle 9 locali.
        assert_eq!(
            occurrence_starts(&database, "2026-03-20", "2026-04-07"),
            [
                (
                    Some("2026-03-23".to_owned()),
                    Some("2026-03-23T08:00:00Z".to_owned())
                ),
                (
                    Some("2026-03-30".to_owned()),
                    Some("2026-03-30T07:00:00Z".to_owned())
                ),
                (
                    Some("2026-04-06".to_owned()),
                    Some("2026-04-06T07:00:00Z".to_owned())
                ),
            ]
        );
        // Prima dell'inizio della serie non c'è niente.
        assert!(occurrence_starts(&database, "2026-03-01", "2026-03-22").is_empty());
    }

    #[test]
    fn single_occurrences_can_be_skipped_detached_or_cut() {
        let database = Database::in_memory().expect("database");
        let series = database
            .create_event(&recurring_event(
                "Corso",
                "2026-05-04T18:00",
                "2026-05-04T19:30",
                r#"{"frequency":"weekly"}"#,
            ))
            .expect("event");
        // Un giorno che non fa parte della serie viene rifiutato.
        assert!(matches!(
            database.skip_event_occurrence(&series.id, "2026-05-05"),
            Err(StorageError::InvalidEvent(_))
        ));
        database
            .skip_event_occurrence(&series.id, "2026-05-11")
            .expect("skip");
        let mut changed =
            recurring_event("Corso (aula 2)", "2026-05-18T19:00", "2026-05-18T20:00", "");
        changed.recurrence = None;
        let detached = database
            .detach_event_occurrence(&series.id, "2026-05-18", &changed)
            .expect("detach");
        assert!(detached.recurrence.is_none());
        // Ripetere lo stacco non crea un secondo evento.
        assert!(
            database
                .detach_event_occurrence(&series.id, "2026-05-18", &changed)
                .is_err()
        );
        let titles: Vec<_> = database
            .list_events(
                date("2026-05-01"),
                date("2026-05-31"),
                &Zone::stored("Europe/Rome"),
            )
            .expect("events")
            .into_iter()
            .map(|event| (event.title, event.occurrence_date))
            .collect();
        assert_eq!(
            titles,
            [
                ("Corso".to_owned(), Some("2026-05-04".to_owned())),
                ("Corso (aula 2)".to_owned(), None),
                ("Corso".to_owned(), Some("2026-05-25".to_owned())),
            ]
        );
        // «Questa e le successive»: la serie finisce il giorno prima.
        let cut = database
            .end_event_series_before(&series.id, "2026-05-25")
            .expect("cut")
            .expect("still exists");
        assert_eq!(
            cut.recurrence.and_then(|rule| rule.until).as_deref(),
            Some("2026-05-24")
        );
        assert_eq!(
            occurrence_starts(&database, "2026-05-20", "2026-06-30").len(),
            0
        );
        // Dal primo giorno l'intera serie va nel cestino (lo fa il chiamante).
        assert!(
            database
                .end_event_series_before(&series.id, "2026-05-04")
                .expect("first")
                .is_none()
        );
        // Tolta la ripetizione, spariscono anche le eccezioni.
        let mut plain = recurring_event("Corso", "2026-05-04T18:00", "2026-05-04T19:30", "");
        plain.recurrence = None;
        let updated = database.update_event(&series.id, &plain).expect("update");
        assert!(updated.recurrence_exceptions.is_empty());
    }

    #[test]
    fn multi_day_recurring_events_overlap_the_range_start() {
        let database = Database::in_memory().expect("database");
        database
            .create_event(&recurring_event(
                "Fiera",
                "2026-01-30",
                "2026-02-02",
                r#"{"frequency":"monthly"}"#,
            ))
            .expect("event");
        // L'occorrenza di marzo (30/3–2/4) interseca aprile.
        assert_eq!(
            occurrence_starts(&database, "2026-04-01", "2026-04-29"),
            [(Some("2026-03-30".to_owned()), Some("2026-03-30".to_owned()))]
        );
    }

    #[test]
    fn repeating_note_reminders_schedule_the_next_one_once() {
        let database = Database::in_memory().expect("database");
        let note = database
            .create_note("Bilancio", &empty_document())
            .expect("note");
        let created_at = instant("2026-03-20T10:00:00Z");
        let reminder = database
            .create_repeating_reminder(
                ReminderEntity::Note,
                &note.id,
                "2026-03-27T09:00",
                Some("Europe/Rome"),
                created_at,
                Some(&weekly()),
            )
            .expect("reminder");
        assert!(reminder.recurrence.is_some());
        // Le attività hanno già la loro ripetizione.
        let task = database
            .create_task(&task("Pagare", Some("2026-03-27")))
            .expect("task");
        assert!(
            database
                .create_repeating_reminder(
                    ReminderEntity::Task,
                    &task.id,
                    "2026-03-27T09:00",
                    Some("Europe/Rome"),
                    created_at,
                    Some(&weekly()),
                )
                .is_err()
        );
        let fired = database
            .take_due_reminders(instant("2026-03-27T08:00:00Z"))
            .expect("take");
        assert_eq!(fired.len(), 1);
        let pending: Vec<_> = database
            .list_entity_reminders(ReminderEntity::Note, &note.id)
            .expect("list")
            .into_iter()
            .filter(|item| item.status == ReminderStatus::Pending)
            .collect();
        // Una settimana dopo, alle 9 locali anche con l'ora legale.
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].remind_at, "2026-04-03T07:00:00Z");
        assert_eq!(pending[0].local_time, "2026-04-03T09:00");
        assert!(pending[0].recurrence.is_some());
        // Ripristinato e riscattato, non crea un doppione.
        database
            .connection()
            .expect("connection")
            .execute(
                "UPDATE reminders SET status = 'pending' WHERE id = ?1",
                [&reminder.id],
            )
            .expect("reset");
        database
            .take_due_reminders(instant("2026-03-27T08:01:00Z"))
            .expect("take again");
        let count = database
            .list_entity_reminders(ReminderEntity::Note, &note.id)
            .expect("list")
            .len();
        assert_eq!(count, 2);
    }

    #[test]
    fn reminders_of_recurring_events_follow_the_next_occurrence() {
        let database = Database::in_memory().expect("database");
        let series = database
            .create_event(&recurring_event(
                "Allenamento",
                "2026-06-01T19:00",
                "2026-06-01T20:00",
                r#"{"frequency":"weekly"}"#,
            ))
            .expect("event");
        database
            .skip_event_occurrence(&series.id, "2026-06-08")
            .expect("skip");
        let reminder = database
            .create_reminder(
                ReminderEntity::Event,
                &series.id,
                "2026-06-01T18:45",
                Some("Europe/Rome"),
                instant("2026-05-30T10:00:00Z"),
            )
            .expect("reminder");
        assert!(reminder.repeats_with_event);
        database
            .take_due_reminders(instant("2026-06-01T16:46:00Z"))
            .expect("take");
        let next: Vec<_> = database
            .list_entity_reminders(ReminderEntity::Event, &series.id)
            .expect("list")
            .into_iter()
            .filter(|item| item.status == ReminderStatus::Pending)
            .map(|item| item.local_time)
            .collect();
        // L'8 giugno è stato tolto: 15 minuti prima di quella del 15.
        assert_eq!(next, ["2026-06-15T18:45"]);
    }

    #[test]
    fn recurrence_survives_the_full_archive() {
        let source = Database::in_memory().expect("database");
        let mut input = task("Affitto", Some("2026-11-01"));
        input.recurrence = Some(serde_json::from_str(r#"{"frequency":"monthly"}"#).expect("rule"));
        let created = source.create_task(&input).expect("task");
        let series = source
            .create_event(&recurring_event(
                "Coro",
                "2026-11-03T21:00",
                "2026-11-03T22:30",
                r#"{"frequency":"weekly","until":"2026-12-29"}"#,
            ))
            .expect("event");
        source
            .skip_event_occurrence(&series.id, "2026-11-10")
            .expect("skip");
        let note = source
            .create_note("Diario", &empty_document())
            .expect("note");
        source
            .create_repeating_reminder(
                ReminderEntity::Note,
                &note.id,
                "2030-01-01T08:00",
                Some("Europe/Rome"),
                time::now(),
                Some(&weekly()),
            )
            .expect("reminder");
        let archive =
            super::export_planner(&source.connection().expect("connection")).expect("export");
        // Una regola alterata nell'archivio viene scartata, l'attività no.
        let mut tampered = archive.clone();
        tampered.tasks[0].recurrence = Some(r#"{"frequency":"hourly"}"#.to_owned());
        let target = Database::in_memory().expect("database");
        super::import_planner(
            &target.connection().expect("connection"),
            &archive,
            time::now(),
        )
        .expect("import");
        let imported = target.get_task(&created.id).expect("task");
        assert_eq!(imported.recurrence, created.recurrence);
        let event = target.get_event(&series.id).expect("event");
        assert_eq!(event.recurrence, series.recurrence);
        assert_eq!(event.recurrence_exceptions, ["2026-11-10"]);
        // Il promemoria della nota non è importato senza la nota: qui si
        // controlla solo che la regola viaggi nell'archivio.
        assert!(
            archive
                .reminders
                .iter()
                .any(|item| item.recurrence.is_some())
        );
        let other = Database::in_memory().expect("database");
        super::import_planner(
            &other.connection().expect("connection"),
            &tampered,
            time::now(),
        )
        .expect("import");
        assert!(
            other
                .get_task(&created.id)
                .expect("task")
                .recurrence
                .is_none()
        );
    }

    #[test]
    fn recurrence_needs_a_due_date_and_can_be_removed() {
        let database = Database::in_memory().expect("database");
        let mut input = task("Senza data", None);
        input.recurrence = Some(weekly());
        assert!(matches!(
            database.create_task(&input),
            Err(StorageError::InvalidTask(_))
        ));
        input.due_date = Some("2026-10-22".to_owned());
        let created = database.create_task(&input).expect("task");
        assert!(created.recurrence.is_some());
        input.recurrence = None;
        let plain = database.update_task(&created.id, &input).expect("update");
        assert!(plain.recurrence.is_none());
        database
            .set_task_completed(
                &plain.id,
                true,
                instant("2026-10-22T10:00:00Z"),
                &Zone::stored("Europe/Rome"),
            )
            .expect("done");
        assert!(
            database
                .list_tasks(TaskView::Open, date("2026-10-22"), None)
                .expect("open")
                .is_empty()
        );
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

        let done = database
            .set_task_completed(
                &linked.id,
                true,
                crate::time::now(),
                &crate::time::Zone::system(),
            )
            .expect("done");
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
            .set_task_completed(
                &linked.id,
                false,
                crate::time::now(),
                &crate::time::Zone::system(),
            )
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
            recurrence: None,
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
        database
            .set_task_completed(
                &task.id,
                true,
                crate::time::now(),
                &crate::time::Zone::system(),
            )
            .expect("done");
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
            recurrence: None,
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
        database
            .set_task_completed(
                &linked.id,
                true,
                crate::time::now(),
                &crate::time::Zone::system(),
            )
            .expect("done");
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
                    recurrence: None,
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
