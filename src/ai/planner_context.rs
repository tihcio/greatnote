//! Attività, eventi del calendario e promemoria come contesto della chat.
//!
//! La domanda viene analizzata in modo deterministico: espressioni di tempo
//! ("oggi", "prossima settimana", "28 ottobre", "2026-11-03"…) scelgono un
//! intervallo di giorni nel fuso dell'utente, le parole del planner
//! ("attività", "calendario", "promemoria"…) attivano il riepilogo dei
//! prossimi giorni e le altre parole cercano in titoli e descrizioni. Le
//! fonti hanno gli stessi riferimenti verificabili delle note: il frontend
//! restituisce solo tipo e id, e il testo viene ricomposto dal database al
//! momento dell'invio. Gli elementi collegati a note di taccuini esclusi non
//! vengono mai inviati.

use std::collections::{HashMap, HashSet};

use jiff::{Timestamp, ToSpan, civil::Date};
use rusqlite::{Connection, OptionalExtension, params};

use super::{
    prompt::Language,
    retrieval::{
        Budget, ContextSource, RetrievalMethod, SourceKind, SourceRef, SourceTarget, keyword_terms,
    },
    settings::AiSettings,
};
use crate::{
    domain::planner::{CalendarEvent, Reminder, ReminderEntity, ReminderStatus, Task, TaskStatus},
    planner::{
        EVENT_COLUMNS, REMINDER_COLUMNS, REMINDER_ENTITY_ACTIVE, TASK_COLUMNS, event_from_row,
        reminder_from_row, task_from_row,
    },
    storage::{Database, StorageError},
    time::{self, Zone},
};

/// Elementi del planner al massimo in una risposta.
pub const MAX_PLANNER_SOURCES: usize = 12;
/// Elementi trovati per parola chiave, per tipo.
const MAX_KEYWORD_ITEMS: usize = 4;
/// Elementi di un intervallo di date, per tipo.
const MAX_WINDOW_ITEMS: usize = 15;
/// Giorni considerati quando la domanda parla del planner senza date.
const DEFAULT_WINDOW_DAYS: i64 = 14;
/// Intervallo massimo ricavato da una domanda.
const MAX_WINDOW_DAYS: i64 = 92;
const MAX_DESCRIPTION_CHARS: usize = 400;

/// Parole che indicano una domanda su attività, calendario o promemoria
/// (confrontate come prefisso delle parole della domanda).
const INTENT_PREFIXES: &[&str] = &[
    "calendar",
    "agenda",
    "impegn",
    "appuntament",
    "evento",
    "eventi",
    "event",
    "riunion",
    "incontr",
    "meeting",
    "attivit",
    "task",
    "compit",
    "scadenz",
    "scadut",
    "deadline",
    "promemori",
    "remind",
    "todo",
    "schedul",
    "appointment",
    "overdue",
    "ritardo",
];
/// "due" da solo manca di proposito: in italiano è un numero.
const INTENT_PHRASES: &[&str] = &[" da fare ", " devo fare ", " to do ", " due date "];

const WEEKDAYS_IT: [&str; 7] = [
    "lunedì",
    "martedì",
    "mercoledì",
    "giovedì",
    "venerdì",
    "sabato",
    "domenica",
];
const WEEKDAYS_EN: [&str; 7] = [
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "saturday",
    "sunday",
];
const MONTHS_IT: [&str; 12] = [
    "gennaio",
    "febbraio",
    "marzo",
    "aprile",
    "maggio",
    "giugno",
    "luglio",
    "agosto",
    "settembre",
    "ottobre",
    "novembre",
    "dicembre",
];
/// "may" manca di proposito: in inglese è soprattutto un verbo.
const MONTHS_EN: [&str; 12] = [
    "january",
    "february",
    "march",
    "april",
    "",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];
/// Parole di tempo semplici, escluse dalla ricerca per parola chiave.
const TIME_WORDS: &[&str] = &[
    "oggi",
    "domani",
    "dopodomani",
    "ieri",
    "stamattina",
    "stasera",
    "settimana",
    "settimane",
    "prossima",
    "prossimo",
    "prossimi",
    "prossime",
    "scorsa",
    "scorso",
    "questa",
    "questo",
    "mese",
    "mesi",
    "giorni",
    "giorno",
    "fine",
    "weekend",
    "today",
    "tomorrow",
    "yesterday",
    "tonight",
    "week",
    "next",
    "last",
    "this",
    "month",
    "days",
    "day",
    "after",
    "devo",
    "fare",
    "cosa",
    "ancora",
];

/// Intervallo di giorni civili, estremi inclusi.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DayWindow {
    pub from: Date,
    pub to: Date,
}

/// Esito dell'analisi della domanda.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerQuery {
    /// Giorni nominati nella domanda.
    pub window: Option<DayWindow>,
    /// La domanda parla di attività, calendario o promemoria.
    pub mentions_planner: bool,
    /// Parole da cercare in titoli e descrizioni.
    pub terms: Vec<String>,
}

fn add_days(date: Date, days: i64) -> Option<Date> {
    date.checked_add(days.days()).ok()
}

fn monday_of(date: Date) -> Option<Date> {
    add_days(date, -i64::from(date.weekday().to_monday_zero_offset()))
}

fn week(date: Date) -> Option<DayWindow> {
    let from = monday_of(date)?;
    Some(DayWindow {
        from,
        to: add_days(from, 6)?,
    })
}

fn month_of(date: Date) -> DayWindow {
    DayWindow {
        from: date.first_of_month(),
        to: date.last_of_month(),
    }
}

/// Mese nominato senza anno: quello più vicino a oggi (entro sei mesi).
fn named_month(month: i8, year: Option<i16>, today: Date) -> Option<DayWindow> {
    let year = year.unwrap_or_else(|| {
        let difference = i32::from(month) - i32::from(today.month());
        if difference < -6 {
            today.year() + 1
        } else if difference > 6 {
            today.year() - 1
        } else {
            today.year()
        }
    });
    Date::new(year, month, 1).ok().map(month_of)
}

fn month_index(word: &str) -> Option<i8> {
    MONTHS_IT
        .iter()
        .position(|name| *name == word)
        .or_else(|| {
            MONTHS_EN
                .iter()
                .position(|name| !name.is_empty() && *name == word)
        })
        .and_then(|index| i8::try_from(index + 1).ok())
}

fn weekday_index(word: &str) -> Option<i64> {
    let unaccented = word.replace('ì', "i");
    WEEKDAYS_IT
        .iter()
        .position(|name| name.replace('ì', "i") == unaccented)
        .or_else(|| WEEKDAYS_EN.iter().position(|name| *name == word))
        .and_then(|index| i64::try_from(index).ok())
}

fn year_of(word: &str) -> Option<i16> {
    (word.len() == 4)
        .then(|| word.parse::<i16>().ok())
        .flatten()
        .filter(|year| (2000..=2100).contains(year))
}

fn day_number(word: &str) -> Option<i8> {
    (word.len() <= 2)
        .then(|| word.parse::<i8>().ok())
        .flatten()
        .filter(|day| (1..=31).contains(day))
}

/// Data scritta come `AAAA-MM-GG`, `GG/MM` o `GG/MM/AAAA`.
fn numeric_date(token: &str, today: Date) -> Option<Date> {
    if let Ok(date) = time::parse_date(token) {
        return Some(date);
    }
    let parts: Vec<&str> = token.split('/').collect();
    let (day, month, year) = match parts.as_slice() {
        [day, month] => (day, month, today.year()),
        [day, month, year] => (day, month, year_of(year)?),
        _ => return None,
    };
    let day = day_number(day)?;
    let month = month
        .parse::<i8>()
        .ok()
        .filter(|month| (1..=12).contains(month))?;
    Date::new(year, month, day).ok()
}

fn words_of(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Analizza la domanda rispetto alla data civile `today` dell'utente.
pub fn analyze_question(question: &str, today: Date) -> PlannerQuery {
    let words = words_of(question);
    let text = format!(" {} ", words.join(" "));
    let has = |phrase: &str| text.contains(&format!(" {phrase} "));
    let mut windows: Vec<DayWindow> = Vec::new();
    let push_day = |date: Option<Date>, windows: &mut Vec<DayWindow>| {
        if let Some(date) = date {
            windows.push(DayWindow {
                from: date,
                to: date,
            });
        }
    };

    if ["oggi", "today", "stamattina", "stasera", "tonight"]
        .iter()
        .any(|word| has(word))
    {
        push_day(Some(today), &mut windows);
    }
    if has("domani") || has("tomorrow") {
        push_day(add_days(today, 1), &mut windows);
    }
    if has("dopodomani") || has("day after tomorrow") {
        push_day(add_days(today, 2), &mut windows);
    }
    if has("ieri") || has("yesterday") {
        push_day(add_days(today, -1), &mut windows);
    }
    let this_week = ["questa settimana", "this week", "settimana corrente"];
    let next_week = ["prossima settimana", "settimana prossima", "next week"];
    let last_week = ["settimana scorsa", "scorsa settimana", "last week"];
    if this_week.iter().any(|phrase| has(phrase)) {
        windows.extend(week(today));
    }
    if next_week.iter().any(|phrase| has(phrase)) {
        windows.extend(add_days(today, 7).and_then(week));
    }
    if last_week.iter().any(|phrase| has(phrase)) {
        windows.extend(add_days(today, -7).and_then(week));
    }
    if has("fine settimana") || has("weekend") {
        let offset = (5 - i64::from(today.weekday().to_monday_zero_offset())).max(-1);
        let saturday = if offset < 0 {
            add_days(today, -1)
        } else {
            add_days(today, offset)
        };
        if let Some(saturday) = saturday {
            windows.extend(add_days(saturday, 1).map(|sunday| DayWindow {
                from: saturday.max(today),
                to: sunday,
            }));
        }
    }
    if has("questo mese") || has("this month") {
        windows.push(month_of(today));
    }
    if has("prossimo mese") || has("mese prossimo") || has("next month") {
        windows.extend(add_days(today.last_of_month(), 1).map(month_of));
    }
    if has("mese scorso") || has("scorso mese") || has("last month") {
        windows.extend(add_days(today.first_of_month(), -1).map(month_of));
    }

    for (index, word) in words.iter().enumerate() {
        let next = words.get(index + 1).map(String::as_str);
        let previous = index
            .checked_sub(1)
            .and_then(|i| words.get(i))
            .map(String::as_str);
        // "prossimi 10 giorni", "next 10 days".
        if matches!(previous, Some("prossimi" | "next"))
            && matches!(next, Some("giorni" | "days"))
            && let Ok(days) = word.parse::<i64>()
        {
            windows.extend(
                add_days(today, days.clamp(1, MAX_WINDOW_DAYS))
                    .map(|to| DayWindow { from: today, to }),
            );
            continue;
        }
        if let Some(weekday) = weekday_index(word) {
            let current = i64::from(today.weekday().to_monday_zero_offset());
            push_day(
                add_days(today, (weekday - current).rem_euclid(7)),
                &mut windows,
            );
            continue;
        }
        let Some(month) = month_index(word) else {
            continue;
        };
        // "28 ottobre [2026]", "october 28 [2026]", "novembre [2026]".
        let day_before = previous.and_then(day_number);
        let day_after = next.and_then(day_number);
        let year = words
            .get(index + 1 + usize::from(day_after.is_some()))
            .and_then(|word| year_of(word));
        match day_before.or(day_after) {
            Some(day) => {
                let year = year.unwrap_or_else(|| today.year());
                push_day(Date::new(year, month, day).ok(), &mut windows);
            }
            None => windows.extend(named_month(month, year, today)),
        }
    }
    for token in question.split_whitespace() {
        let token = token.trim_matches(|c: char| !c.is_alphanumeric());
        if token.contains(['-', '/']) {
            push_day(numeric_date(token, today), &mut windows);
        }
    }

    let window = windows
        .iter()
        .copied()
        .reduce(|a, b| DayWindow {
            from: a.from.min(b.from),
            to: a.to.max(b.to),
        })
        .map(|window| DayWindow {
            from: window.from,
            to: window
                .to
                .min(add_days(window.from, MAX_WINDOW_DAYS).unwrap_or(window.to)),
        });
    let mentions_planner = window.is_some()
        || INTENT_PHRASES.iter().any(|phrase| text.contains(phrase))
        || words.iter().any(|word| {
            INTENT_PREFIXES
                .iter()
                .any(|prefix| word.starts_with(prefix))
        });
    let terms = keyword_terms(question)
        .into_iter()
        .filter(|term| {
            !TIME_WORDS.contains(&term.as_str())
                && !INTENT_PREFIXES
                    .iter()
                    .any(|prefix| term.starts_with(prefix))
                && month_index(term).is_none()
                && weekday_index(term).is_none()
                && year_of(term).is_none()
                && !term.chars().all(|c| c.is_ascii_digit())
        })
        .collect();
    PlannerQuery {
        window,
        mentions_planner,
        terms,
    }
}

// ---------------------------------------------------------------------------
// Testo delle fonti

fn weekday_name(date: Date, language: Language) -> &'static str {
    let index = usize::try_from(date.weekday().to_monday_zero_offset()).unwrap_or(0);
    match language {
        Language::It => WEEKDAYS_IT[index],
        Language::En => WEEKDAYS_EN[index],
    }
}

fn describe_day(date: Date, language: Language) -> String {
    format!("{} {date}", weekday_name(date, language))
}

/// Istante UTC salvato, mostrato nel fuso dell'utente.
fn describe_instant(value: &str, zone: &Zone, language: Language) -> String {
    let Ok(instant) = time::parse_utc(value) else {
        return value.to_owned();
    };
    let zoned = instant.to_zoned(zone.tz.clone());
    format!(
        "{} {}",
        describe_day(zoned.date(), language),
        zoned.strftime("%H:%M")
    )
}

/// Data e ora correnti per il prompt, così "domani" ha un riferimento.
pub fn describe_now(now: Timestamp, zone: &Zone, language: Language) -> String {
    let zoned = now.to_zoned(zone.tz.clone());
    format!(
        "{} {} ({})",
        describe_day(zoned.date(), language),
        zoned.strftime("%H:%M"),
        zone.name
    )
}

fn clip(text: &str, max: usize) -> String {
    let text = text.trim();
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        text.chars().take(max).collect::<String>() + "…"
    }
}

struct Labels {
    task: &'static str,
    event: &'static str,
    open: &'static str,
    done: &'static str,
    priority: &'static str,
    priorities: [&'static str; 4],
    flagged: &'static str,
    due: &'static str,
    when: &'static str,
    all_day: &'static str,
    calendar: &'static str,
    local_calendar: &'static str,
    subscription: &'static str,
    reminder: &'static str,
    reminder_for_note: &'static str,
    fired: &'static str,
    linked_note: &'static str,
    description: &'static str,
    repeats: &'static str,
}

/// Regola di ripetizione in parole, per il contesto della chat.
fn describe_recurrence(rule: &crate::domain::recurrence::Recurrence, language: Language) -> String {
    use crate::domain::recurrence::Frequency;
    let italian = language == Language::It;
    let n = rule.interval;
    let unit = match (rule.frequency, italian, n == 1) {
        (Frequency::Daily, true, true) => "ogni giorno".to_owned(),
        (Frequency::Daily, true, false) => format!("ogni {n} giorni"),
        (Frequency::Weekly, true, true) => "ogni settimana".to_owned(),
        (Frequency::Weekly, true, false) => format!("ogni {n} settimane"),
        (Frequency::Monthly, true, true) => "ogni mese".to_owned(),
        (Frequency::Monthly, true, false) => format!("ogni {n} mesi"),
        (Frequency::Yearly, true, true) => "ogni anno".to_owned(),
        (Frequency::Yearly, true, false) => format!("ogni {n} anni"),
        (Frequency::Daily, false, true) => "every day".to_owned(),
        (Frequency::Daily, false, false) => format!("every {n} days"),
        (Frequency::Weekly, false, true) => "every week".to_owned(),
        (Frequency::Weekly, false, false) => format!("every {n} weeks"),
        (Frequency::Monthly, false, true) => "every month".to_owned(),
        (Frequency::Monthly, false, false) => format!("every {n} months"),
        (Frequency::Yearly, false, true) => "every year".to_owned(),
        (Frequency::Yearly, false, false) => format!("every {n} years"),
    };
    let mut text = unit;
    if !rule.weekdays.is_empty() {
        let names: [&str; 7] = if italian {
            [
                "lunedì",
                "martedì",
                "mercoledì",
                "giovedì",
                "venerdì",
                "sabato",
                "domenica",
            ]
        } else {
            [
                "Monday",
                "Tuesday",
                "Wednesday",
                "Thursday",
                "Friday",
                "Saturday",
                "Sunday",
            ]
        };
        let days: Vec<&str> = rule
            .weekdays
            .iter()
            .filter_map(|day| names.get(usize::from(day.saturating_sub(1))).copied())
            .collect();
        text.push_str(&format!(" ({})", days.join(", ")));
    }
    if let Some(until) = &rule.until {
        text.push_str(&if italian {
            format!(", fino al {until}")
        } else {
            format!(", until {until}")
        });
    }
    text
}

fn labels(language: Language) -> Labels {
    match language {
        Language::It => Labels {
            task: "Attività",
            event: "Evento",
            open: "da fare",
            done: "completata",
            priority: "Priorità",
            priorities: ["nessuna", "bassa", "media", "alta"],
            flagged: "contrassegnata",
            due: "Scadenza",
            when: "Quando",
            all_day: "tutto il giorno",
            calendar: "Calendario",
            local_calendar: "locale",
            subscription: "abbonamento iCal",
            reminder: "Promemoria",
            reminder_for_note: "Promemoria per la nota",
            fired: "scattato, in attesa",
            linked_note: "Nota collegata",
            description: "Descrizione",
            repeats: "Si ripete",
        },
        Language::En => Labels {
            task: "Task",
            event: "Event",
            open: "to do",
            done: "completed",
            priority: "Priority",
            priorities: ["none", "low", "medium", "high"],
            flagged: "flagged",
            due: "Due",
            when: "When",
            all_day: "all day",
            calendar: "Calendar",
            local_calendar: "local",
            subscription: "iCal subscription",
            reminder: "Reminder",
            reminder_for_note: "Reminder for the note",
            fired: "fired, waiting",
            linked_note: "Linked note",
            description: "Description",
            repeats: "Repeats",
        },
    }
}

fn reminder_line(reminder: &Reminder, zone: &Zone, language: Language) -> String {
    let mut line = describe_instant(&reminder.remind_at, zone, language);
    if reminder.status == ReminderStatus::Fired {
        line.push_str(&format!(" ({})", labels(language).fired));
    }
    line
}

fn task_text(task: &Task, reminders: &[Reminder], zone: &Zone, language: Language) -> String {
    let l = labels(language);
    let status = match (task.status, task.completed_at.as_deref()) {
        (TaskStatus::Done, Some(at)) => {
            format!("{} ({})", l.done, describe_instant(at, zone, language))
        }
        (TaskStatus::Done, None) => l.done.to_owned(),
        (TaskStatus::Open, _) => l.open.to_owned(),
    };
    let mut lines = vec![format!("{}: {}", l.task, task.title)];
    let mut state = format!(
        "{status} · {}: {}",
        l.priority,
        l.priorities[usize::from(task.priority.min(3))]
    );
    if task.flagged {
        state.push_str(&format!(" · {}", l.flagged));
    }
    lines.push(state);
    if let Some(due) = task.due_date.as_deref() {
        let due = time::parse_date(due)
            .map(|date| describe_day(date, language))
            .unwrap_or_else(|_| due.to_owned());
        lines.push(format!("{}: {due}", l.due));
    }
    if let Some(rule) = &task.recurrence {
        lines.push(format!(
            "{}: {}",
            l.repeats,
            describe_recurrence(rule, language)
        ));
    }
    for reminder in reminders {
        lines.push(format!(
            "{}: {}",
            l.reminder,
            reminder_line(reminder, zone, language)
        ));
    }
    if let Some(title) = task.note_title.as_deref() {
        lines.push(format!("{}: «{title}»", l.linked_note));
    }
    if !task.description.trim().is_empty() {
        lines.push(format!(
            "{}: {}",
            l.description,
            clip(&task.description, MAX_DESCRIPTION_CHARS)
        ));
    }
    lines.join("\n")
}

fn event_text(
    event: &CalendarEvent,
    reminders: &[Reminder],
    zone: &Zone,
    language: Language,
) -> String {
    let l = labels(language);
    let when = if event.all_day {
        let day = |value: &Option<String>| {
            value
                .as_deref()
                .and_then(|value| time::parse_date(value).ok())
                .map(|date| describe_day(date, language))
                .unwrap_or_default()
        };
        let (start, end) = (day(&event.start_date), day(&event.end_date));
        if start == end {
            format!("{start}, {}", l.all_day)
        } else {
            format!("{start} – {end}, {}", l.all_day)
        }
    } else {
        let start = event
            .start_at
            .as_deref()
            .map(|value| describe_instant(value, zone, language))
            .unwrap_or_default();
        let end = event
            .end_at
            .as_deref()
            .map(|value| describe_instant(value, zone, language))
            .unwrap_or_default();
        // Stesso giorno: la fine mostra solo l'ora.
        let end = match (start.rsplit_once(' '), end.rsplit_once(' ')) {
            (Some((start_day, _)), Some((end_day, end_time))) if start_day == end_day => {
                end_time.to_owned()
            }
            _ => end,
        };
        format!("{start} – {end}")
    };
    let calendar = match event.calendar_provider.as_deref() {
        None => l.local_calendar,
        Some("google") => "Google",
        Some("microsoft") => "Microsoft",
        Some(_) => l.subscription,
    };
    let mut lines = vec![
        format!("{}: {}", l.event, event.title),
        format!("{}: {when}", l.when),
        format!("{}: {calendar}", l.calendar),
    ];
    for reminder in reminders {
        lines.push(format!(
            "{}: {}",
            l.reminder,
            reminder_line(reminder, zone, language)
        ));
    }
    if let Some(title) = event.note_title.as_deref() {
        lines.push(format!("{}: «{title}»", l.linked_note));
    }
    if !event.description.trim().is_empty() {
        lines.push(format!(
            "{}: {}",
            l.description,
            clip(&event.description, MAX_DESCRIPTION_CHARS)
        ));
    }
    lines.join("\n")
}

fn note_reminder_text(reminder: &Reminder, zone: &Zone, language: Language) -> String {
    format!(
        "{} «{}»: {}",
        labels(language).reminder_for_note,
        reminder.entity_title,
        reminder_line(reminder, zone, language)
    )
}

// ---------------------------------------------------------------------------
// Letture dal database

/// Vero se la nota collegata (se presente) non è in un taccuino escluso.
/// Una nota nel cestino non blocca l'elemento: semplicemente non viene
/// mostrata (le colonne del planner riportano solo note attive).
fn linked_note_allowed(
    connection: &Connection,
    settings: &AiSettings,
    note_id: Option<&str>,
) -> Result<bool, StorageError> {
    let Some(note_id) = note_id else {
        return Ok(true);
    };
    let notebook: Option<Option<String>> = connection
        .query_row(
            "SELECT notebook_id FROM notes WHERE id = ?1",
            [note_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(!settings.is_excluded(notebook.flatten().as_deref()))
}

fn entity_reminders(
    connection: &Connection,
    entity: ReminderEntity,
    id: &str,
) -> Result<Vec<Reminder>, StorageError> {
    let entity = match entity {
        ReminderEntity::Task => "task",
        ReminderEntity::Event => "event",
        ReminderEntity::Note => "note",
    };
    let mut statement = connection.prepare(&format!(
        "SELECT {REMINDER_COLUMNS} FROM reminders AS r
         WHERE r.entity_type = ?1 AND r.entity_id = ?2 AND r.status != 'done'
         ORDER BY r.remind_at LIMIT 3"
    ))?;
    let rows = statement.query_map(params![entity, id], reminder_from_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn active_task(connection: &Connection, id: &str) -> Result<Option<Task>, StorageError> {
    Ok(connection
        .query_row(
            &format!(
                "SELECT {TASK_COLUMNS} FROM tasks WHERE tasks.id = ?1 AND tasks.deleted_at IS NULL"
            ),
            [id],
            task_from_row,
        )
        .optional()?)
}

fn active_event(connection: &Connection, id: &str) -> Result<Option<CalendarEvent>, StorageError> {
    Ok(connection
        .query_row(
            &format!(
                "SELECT {EVENT_COLUMNS} FROM events WHERE events.id = ?1 AND events.deleted_at IS NULL"
            ),
            [id],
            event_from_row,
        )
        .optional()?)
}

fn active_reminder(connection: &Connection, id: &str) -> Result<Option<Reminder>, StorageError> {
    Ok(connection
        .query_row(
            &format!(
                "SELECT {REMINDER_COLUMNS} FROM reminders AS r
                 WHERE r.id = ?1 AND {REMINDER_ENTITY_ACTIVE}"
            ),
            [id],
            reminder_from_row,
        )
        .optional()?)
}

/// Compone la fonte di un elemento, oppure `None` se non esiste più o è
/// collegato a una nota esclusa.
fn build_source(
    connection: &Connection,
    settings: &AiSettings,
    kind: SourceKind,
    id: &str,
    method: RetrievalMethod,
    zone: &Zone,
    language: Language,
) -> Result<Option<ContextSource>, StorageError> {
    let source = |kind, title: &str, note_id: Option<&str>, text: String, open| ContextSource {
        number: 0,
        kind,
        note_id: note_id.unwrap_or_default().to_owned(),
        item_id: id.to_owned(),
        title: title.to_owned(),
        char_start: 0,
        char_end: 0,
        text,
        method,
        score: None,
        open,
    };
    match kind {
        SourceKind::Note => Ok(None),
        SourceKind::Task => {
            let Some(task) = active_task(connection, id)? else {
                return Ok(None);
            };
            if !linked_note_allowed(connection, settings, task.note_id.as_deref())? {
                return Ok(None);
            }
            let reminders = entity_reminders(connection, ReminderEntity::Task, id)?;
            let text = task_text(&task, &reminders, zone, language);
            Ok(Some(source(
                SourceKind::Task,
                &task.title,
                task.note_id.as_deref(),
                text,
                SourceTarget::task(id),
            )))
        }
        SourceKind::Event => {
            let Some(event) = active_event(connection, id)? else {
                return Ok(None);
            };
            if !linked_note_allowed(connection, settings, event.note_id.as_deref())? {
                return Ok(None);
            }
            let reminders = entity_reminders(connection, ReminderEntity::Event, id)?;
            let text = event_text(&event, &reminders, zone, language);
            Ok(Some(source(
                SourceKind::Event,
                &event.title,
                event.note_id.as_deref(),
                text,
                SourceTarget::event(id),
            )))
        }
        SourceKind::Reminder => {
            let Some(reminder) = active_reminder(connection, id)? else {
                return Ok(None);
            };
            // I promemoria di attività ed eventi compaiono nella loro fonte.
            match reminder.entity_type {
                ReminderEntity::Task => build_source(
                    connection,
                    settings,
                    SourceKind::Task,
                    &reminder.entity_id,
                    method,
                    zone,
                    language,
                ),
                ReminderEntity::Event => build_source(
                    connection,
                    settings,
                    SourceKind::Event,
                    &reminder.entity_id,
                    method,
                    zone,
                    language,
                ),
                ReminderEntity::Note => {
                    if !linked_note_allowed(connection, settings, Some(&reminder.entity_id))? {
                        return Ok(None);
                    }
                    let text = note_reminder_text(&reminder, zone, language);
                    Ok(Some(source(
                        SourceKind::Reminder,
                        &reminder.entity_title,
                        Some(&reminder.entity_id),
                        text,
                        SourceTarget::note(&reminder.entity_id),
                    )))
                }
            }
        }
    }
}

fn ids(
    connection: &Connection,
    sql: &str,
    values: &[&dyn rusqlite::ToSql],
) -> Result<Vec<String>, StorageError> {
    let mut statement = connection.prepare(sql)?;
    let rows = statement.query_map(values, |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Candidati in ordine di priorità: (tipo, id, metodo).
type Candidate = (SourceKind, String, RetrievalMethod);

fn keyword_candidates(
    connection: &Connection,
    terms: &[String],
    now: &str,
) -> Result<Vec<Candidate>, StorageError> {
    let mut scores: HashMap<(SourceKind, String), usize> = HashMap::new();
    let mut order: Vec<(SourceKind, String)> = Vec::new();
    for term in terms {
        // Confronto senza maiuscole con `lower()` (solo ASCII, come la
        // ricerca delle attività).
        let tasks = ids(
            connection,
            "SELECT id FROM tasks
             WHERE deleted_at IS NULL
               AND instr(lower(title || ' ' || description), lower(?1)) > 0
             ORDER BY status = 'done', due_date IS NULL, due_date
             LIMIT 20",
            &[term],
        )?;
        let events = ids(
            connection,
            "SELECT id FROM events
             WHERE deleted_at IS NULL
               AND instr(lower(title || ' ' || description), lower(?1)) > 0
             ORDER BY COALESCE(end_at, end_date) < substr(?2, 1, length(COALESCE(end_at, end_date))),
                      COALESCE(start_at, start_date)
             LIMIT 20",
            &[term, &now],
        )?;
        for key in tasks
            .into_iter()
            .map(|id| (SourceKind::Task, id))
            .chain(events.into_iter().map(|id| (SourceKind::Event, id)))
        {
            let score = scores.entry(key.clone()).or_insert(0);
            if *score == 0 {
                order.push(key);
            }
            *score += 1;
        }
    }
    // Più parole in comune prima; a parità resta l'ordine della query.
    order.sort_by_key(|key| std::cmp::Reverse(scores.get(key).copied().unwrap_or(0)));
    let mut per_kind: HashMap<SourceKind, usize> = HashMap::new();
    Ok(order
        .into_iter()
        .filter(|(kind, _)| {
            let count = per_kind.entry(*kind).or_insert(0);
            *count += 1;
            *count <= MAX_KEYWORD_ITEMS
        })
        .map(|(kind, id)| (kind, id, RetrievalMethod::Keyword))
        .collect())
}

fn window_candidates(
    database: &Database,
    window: DayWindow,
    today: Date,
    include_undated: bool,
    zone: &Zone,
) -> Result<Vec<Candidate>, StorageError> {
    let events = database.list_events(window.from, window.to, zone)?;
    let start = time::format_utc(time::day_start(window.from, zone)?);
    let end = time::format_utc(time::day_start(time::next_day(window.to)?, zone)?);
    let connection = database.connection()?;
    let limit = MAX_WINDOW_ITEMS as i64;
    let mut tasks = ids(
        &connection,
        "SELECT id FROM tasks
         WHERE deleted_at IS NULL AND due_date BETWEEN ?1 AND ?2
         ORDER BY due_date, status = 'done', priority DESC, title
         LIMIT ?3",
        &[&window.from.to_string(), &window.to.to_string(), &limit],
    )?;
    if window.from <= today && today <= window.to {
        // Le attività in ritardo contano per "oggi" e "questa settimana".
        tasks.extend(ids(
            &connection,
            "SELECT id FROM tasks
             WHERE deleted_at IS NULL AND status = 'open' AND due_date < ?1
             ORDER BY due_date DESC LIMIT 5",
            &[&window.from.to_string()],
        )?);
    }
    if include_undated {
        tasks.extend(ids(
            &connection,
            "SELECT id FROM tasks
             WHERE deleted_at IS NULL AND status = 'open' AND due_date IS NULL
             ORDER BY flagged DESC, priority DESC, created_at DESC LIMIT 5",
            &[],
        )?);
    }
    let reminders = ids(
        &connection,
        &format!(
            "SELECT r.id FROM reminders AS r
             WHERE r.status != 'done' AND {REMINDER_ENTITY_ACTIVE}
               AND ((r.remind_at >= ?1 AND r.remind_at < ?2) OR r.status = 'fired')
             ORDER BY r.status = 'pending', r.remind_at
             LIMIT ?3"
        ),
        &[&start, &end, &limit],
    )?;
    drop(connection);

    // Ordine cronologico approssimato: eventi, attività, promemoria del
    // periodo; i doppioni vengono scartati dopo.
    let mut candidates: Vec<Candidate> = events
        .into_iter()
        .take(MAX_WINDOW_ITEMS)
        .map(|event| (SourceKind::Event, event.id, RetrievalMethod::Date))
        .collect();
    candidates.extend(
        tasks
            .into_iter()
            .map(|id| (SourceKind::Task, id, RetrievalMethod::Date)),
    );
    candidates.extend(
        reminders
            .into_iter()
            .map(|id| (SourceKind::Reminder, id, RetrievalMethod::Date)),
    );
    Ok(candidates)
}

impl Database {
    /// Attività, eventi e promemoria pertinenti alla domanda, entro
    /// `max_chars` caratteri. Numerazione assegnata dal chiamante.
    pub fn ai_planner_context(
        &self,
        settings: &AiSettings,
        question: &str,
        now: Timestamp,
        zone: &Zone,
        language: Language,
        max_chars: usize,
    ) -> Result<Vec<ContextSource>, StorageError> {
        if !settings.planner_context {
            return Ok(Vec::new());
        }
        let today = time::today(now, zone);
        let query = analyze_question(question, today);
        let mut candidates: Vec<Candidate> = Vec::new();
        if !query.terms.is_empty() {
            let connection = self.connection()?;
            candidates.extend(keyword_candidates(
                &connection,
                &query.terms,
                &time::format_utc(now),
            )?);
        }
        let window = match query.window {
            Some(window) => Some((window, false)),
            None if query.mentions_planner => {
                add_days(today, DEFAULT_WINDOW_DAYS).map(|to| (DayWindow { from: today, to }, true))
            }
            None => None,
        };
        if let Some((window, undated)) = window {
            candidates.extend(window_candidates(self, window, today, undated, zone)?);
        }

        let connection = self.connection()?;
        let mut budget = Budget::new(max_chars);
        let mut seen: HashSet<(SourceKind, String)> = HashSet::new();
        let mut sources = Vec::new();
        for (kind, id, method) in candidates {
            if sources.len() >= MAX_PLANNER_SOURCES {
                break;
            }
            let Some(mut source) =
                build_source(&connection, settings, kind, &id, method, zone, language)?
            else {
                continue;
            };
            // Un promemoria di un'attività diventa la fonte dell'attività.
            if !seen.insert((source.kind, source.item_id.clone())) {
                continue;
            }
            let Some(text) = budget.take(source.text) else {
                break;
            };
            source.text = text;
            sources.push(source);
        }
        Ok(sources)
    }

    /// Rilegge un elemento del planner confermato dall'utente.
    pub(crate) fn ai_resolve_planner_source(
        connection: &Connection,
        settings: &AiSettings,
        reference: &SourceRef,
        zone: &Zone,
        language: Language,
    ) -> Result<Option<ContextSource>, StorageError> {
        if !settings.planner_context || reference.item_id.is_empty() {
            return Ok(None);
        }
        build_source(
            connection,
            settings,
            reference.kind,
            &reference.item_id,
            RetrievalMethod::Selected,
            zone,
            language,
        )
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn recurrence_is_described_in_both_languages() {
        let rule: crate::domain::recurrence::Recurrence = serde_json::from_str(
            r#"{"frequency":"weekly","interval":2,"weekdays":[1,4],"until":"2026-12-31"}"#,
        )
        .expect("rule");
        assert_eq!(
            describe_recurrence(&rule, Language::It),
            "ogni 2 settimane (lunedì, giovedì), fino al 2026-12-31"
        );
        let monthly: crate::domain::recurrence::Recurrence =
            serde_json::from_str(r#"{"frequency":"monthly"}"#).expect("rule");
        assert_eq!(describe_recurrence(&monthly, Language::En), "every month");
    }
    use super::*;
    use crate::{
        domain::planner::{EventInput, TaskInput},
        storage::Database,
    };

    fn date(value: &str) -> Date {
        value.parse().expect("date")
    }

    fn rome() -> Zone {
        Zone::resolve(Some("Europe/Rome")).expect("tzdata")
    }

    fn window(question: &str, today: &str) -> Option<(String, String)> {
        analyze_question(question, date(today))
            .window
            .map(|window| (window.from.to_string(), window.to.to_string()))
    }

    fn pair(from: &str, to: &str) -> Option<(String, String)> {
        Some((from.to_owned(), to.to_owned()))
    }

    #[test]
    fn time_expressions_become_day_windows() {
        // 2026-10-21 è un mercoledì.
        let today = "2026-10-21";
        assert_eq!(window("Cosa ho oggi?", today), pair(today, today));
        assert_eq!(
            window("Impegni di domani", today),
            pair("2026-10-22", "2026-10-22")
        );
        assert_eq!(
            window("e dopodomani?", today),
            pair("2026-10-23", "2026-10-23")
        );
        assert_eq!(
            window("What did I do yesterday?", today),
            pair("2026-10-20", "2026-10-20")
        );
        assert_eq!(
            window("Riunioni della prossima settimana", today),
            pair("2026-10-26", "2026-11-01")
        );
        assert_eq!(window("this week", today), pair("2026-10-19", "2026-10-25"));
        assert_eq!(
            window("nel fine settimana", today),
            pair("2026-10-24", "2026-10-25")
        );
        assert_eq!(
            window("questo mese", today),
            pair("2026-10-01", "2026-10-31")
        );
        assert_eq!(
            window("il mese prossimo", today),
            pair("2026-11-01", "2026-11-30")
        );
        assert_eq!(window("venerdì", today), pair("2026-10-23", "2026-10-23"));
        assert_eq!(window("lunedi", today), pair("2026-10-26", "2026-10-26"));
        assert_eq!(
            window("il 28 ottobre", today),
            pair("2026-10-28", "2026-10-28")
        );
        assert_eq!(
            window("on November 3 2027", today),
            pair("2027-11-03", "2027-11-03")
        );
        assert_eq!(
            window("a dicembre", today),
            pair("2026-12-01", "2026-12-31")
        );
        // Un mese lontano più di sei mesi è quello dell'anno precedente.
        assert_eq!(window("a giugno", today), pair("2026-06-01", "2026-06-30"));
        assert_eq!(
            window("in august", "2026-01-10"),
            pair("2025-08-01", "2025-08-31")
        );
        assert_eq!(
            window("scadenze del 2026-11-05?", today),
            pair("2026-11-05", "2026-11-05")
        );
        assert_eq!(window("il 5/11", today), pair("2026-11-05", "2026-11-05"));
        assert_eq!(
            window("prossimi 10 giorni", today),
            pair(today, "2026-10-31")
        );
        assert_eq!(window("next 500 days", today), pair(today, "2027-01-21"));
        // Più espressioni: l'intervallo le comprende tutte.
        assert_eq!(window("oggi e domani", today), pair(today, "2026-10-22"));
        assert_eq!(window("Riassumi la nota sul budget", today), None);
        assert_eq!(window("May I see the 31/02?", today), None);
    }

    #[test]
    fn planner_intent_and_terms() {
        let today = date("2026-10-21");
        let query = analyze_question("Quali attività ho per il progetto Atlante?", today);
        assert!(query.mentions_planner);
        assert!(query.window.is_none());
        assert_eq!(query.terms, ["progetto", "atlante"]);
        let query = analyze_question("Quando è la riunione con Marco domani?", today);
        assert!(query.mentions_planner);
        assert_eq!(query.terms, ["marco"]);
        let query = analyze_question("Riassumi il verbale", today);
        assert!(!query.mentions_planner);
        assert_eq!(query.terms, ["riassumi", "verbale"]);
    }

    fn now() -> Timestamp {
        "2026-10-21T08:00:00Z".parse().expect("now")
    }

    fn task(database: &Database, title: &str, due: Option<&str>, note: Option<&str>) -> Task {
        database
            .create_task(&TaskInput {
                title: title.to_owned(),
                description: String::new(),
                priority: 2,
                due_date: due.map(str::to_owned),
                note_id: note.map(str::to_owned),
                flagged: false,
                recurrence: None,
            })
            .expect("task")
    }

    fn event(database: &Database, title: &str, start: &str, end: &str) -> CalendarEvent {
        database
            .create_event(&EventInput {
                title: title.to_owned(),
                description: "Sala Verde, portare il bilancio".to_owned(),
                all_day: false,
                start: start.to_owned(),
                end: end.to_owned(),
                timezone: Some("Europe/Rome".to_owned()),
                note_id: None,
            })
            .expect("event")
    }

    fn titles(sources: &[ContextSource]) -> Vec<&str> {
        sources.iter().map(|source| source.title.as_str()).collect()
    }

    #[test]
    fn retrieves_planner_items_by_date_and_keyword() {
        let database = Database::in_memory().expect("database");
        let settings = AiSettings::default();
        let zone = rome();
        task(&database, "Pagare l'affitto", Some("2026-10-22"), None);
        task(
            &database,
            "Rinnovare il passaporto",
            Some("2026-12-15"),
            None,
        );
        task(&database, "Bolletta scaduta", Some("2026-10-10"), None);
        let meeting = event(
            &database,
            "Riunione Atlante",
            "2026-10-22T10:00",
            "2026-10-22T11:30",
        );
        event(&database, "Cena", "2026-11-20T20:00", "2026-11-20T22:00");
        database
            .create_reminder(
                ReminderEntity::Event,
                &meeting.id,
                "2026-10-22T09:30",
                Some("Europe/Rome"),
                now(),
            )
            .expect("reminder");

        let sources = database
            .ai_planner_context(
                &settings,
                "Cosa ho domani?",
                now(),
                &zone,
                Language::It,
                10_000,
            )
            .expect("context");
        assert_eq!(titles(&sources), ["Riunione Atlante", "Pagare l'affitto"]);
        let meeting_text = &sources[0].text;
        assert!(
            meeting_text.contains("giovedì 2026-10-22 10:00 – 11:30"),
            "{meeting_text}"
        );
        assert!(
            meeting_text.contains("Promemoria: giovedì 2026-10-22 09:30"),
            "{meeting_text}"
        );
        assert!(meeting_text.contains("Calendario: locale"));
        assert_eq!(sources[0].kind, SourceKind::Event);
        assert_eq!(sources[0].method, RetrievalMethod::Date);
        assert_eq!(sources[0].open, SourceTarget::event(&meeting.id));

        // "Oggi" include le attività in ritardo.
        let sources = database
            .ai_planner_context(&settings, "today", now(), &zone, Language::En, 10_000)
            .expect("context");
        assert_eq!(titles(&sources), ["Bolletta scaduta"]);
        assert!(sources[0].text.starts_with("Task: Bolletta scaduta\nto do"));

        // Parola chiave, a qualunque data.
        let sources = database
            .ai_planner_context(
                &settings,
                "Quando devo rinnovare il passaporto?",
                now(),
                &zone,
                Language::It,
                10_000,
            )
            .expect("context");
        assert_eq!(titles(&sources), ["Rinnovare il passaporto"]);
        assert_eq!(sources[0].method, RetrievalMethod::Keyword);
        assert!(sources[0].text.contains("Scadenza: martedì 2026-12-15"));

        // Domanda sulle note: nessun elemento del planner.
        assert!(
            database
                .ai_planner_context(
                    &settings,
                    "Riassumi il verbale",
                    now(),
                    &zone,
                    Language::It,
                    10_000
                )
                .expect("context")
                .is_empty()
        );
        // Disattivato nelle impostazioni.
        let disabled = AiSettings {
            planner_context: false,
            ..AiSettings::default()
        };
        assert!(
            database
                .ai_planner_context(
                    &disabled,
                    "Cosa ho domani?",
                    now(),
                    &zone,
                    Language::It,
                    10_000
                )
                .expect("context")
                .is_empty()
        );
    }

    #[test]
    fn excluded_notebooks_trash_and_budget_are_respected() {
        let database = Database::in_memory().expect("database");
        let zone = rome();
        let private = database.create_notebook(None, "Privato", "").expect("nb");
        let secret = database
            .create_note_in_notebook(
                "Diario",
                &crate::domain::note::empty_document(),
                Some(&private.id),
            )
            .expect("note");
        let visible = database
            .create_note("Spesa", &crate::domain::note::empty_document())
            .expect("note");
        task(
            &database,
            "Visita privata",
            Some("2026-10-22"),
            Some(&secret.id),
        );
        let removed = task(&database, "Attività cestinata", Some("2026-10-22"), None);
        database
            .move_item_to_trash(crate::trash::TrashKind::Task, &removed.id)
            .expect("trash");
        database
            .create_reminder(
                ReminderEntity::Note,
                &visible.id,
                "2026-10-22T18:00",
                Some("Europe/Rome"),
                now(),
            )
            .expect("reminder");
        database
            .create_reminder(
                ReminderEntity::Note,
                &secret.id,
                "2026-10-22T19:00",
                Some("Europe/Rome"),
                now(),
            )
            .expect("reminder");
        let settings = AiSettings {
            excluded_notebook_ids: vec![private.id.clone()],
            ..AiSettings::default()
        };
        let sources = database
            .ai_planner_context(
                &settings,
                "promemoria di domani",
                now(),
                &zone,
                Language::It,
                10_000,
            )
            .expect("context");
        assert_eq!(titles(&sources), ["Spesa"]);
        assert_eq!(sources[0].kind, SourceKind::Reminder);
        assert_eq!(sources[0].open, SourceTarget::note(&visible.id));
        assert_eq!(
            sources[0].text,
            "Promemoria per la nota «Spesa»: giovedì 2026-10-22 18:00"
        );

        // La rilettura applica di nuovo esclusioni e cestino.
        let connection = database.connection().expect("connection");
        let reference = sources[0].reference();
        assert!(
            Database::ai_resolve_planner_source(
                &connection,
                &settings,
                &reference,
                &zone,
                Language::It
            )
            .expect("resolve")
            .is_some()
        );
        let trashed = SourceRef {
            kind: SourceKind::Task,
            item_id: removed.id.clone(),
            ..reference
        };
        assert!(
            Database::ai_resolve_planner_source(
                &connection,
                &settings,
                &trashed,
                &zone,
                Language::It
            )
            .expect("resolve")
            .is_none()
        );
        drop(connection);

        // Il budget limita il testo inviato.
        for index in 0..20 {
            task(
                &database,
                &format!("Compito {index}"),
                Some("2026-10-22"),
                None,
            );
        }
        let sources = database
            .ai_planner_context(
                &AiSettings::default(),
                "domani",
                now(),
                &zone,
                Language::It,
                500,
            )
            .expect("context");
        let total: usize = sources
            .iter()
            .map(|source| source.text.chars().count())
            .sum();
        assert!(total <= 501, "{total}");
        assert!(!sources.is_empty());
    }
}
