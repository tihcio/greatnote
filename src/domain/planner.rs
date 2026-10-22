use serde::{Deserialize, Serialize};

use super::recurrence::Recurrence;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Open,
    Done,
}

impl TaskStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Done => "done",
        }
    }

    pub(crate) fn parse(value: &str) -> Self {
        if value == "done" {
            Self::Done
        } else {
            Self::Open
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub note_id: Option<String>,
    /// Titolo della nota collegata, se non è nel cestino.
    pub note_title: Option<String>,
    pub title: String,
    pub description: String,
    pub status: TaskStatus,
    /// 0 nessuna, 1 bassa, 2 media, 3 alta.
    pub priority: u8,
    /// Data civile `AAAA-MM-GG`.
    pub due_date: Option<String>,
    pub completed_at: Option<String>,
    /// Attività contrassegnata come importante.
    pub flagged: bool,
    pub created_at: String,
    pub updated_at: String,
    /// Prossimo promemoria attivo (istante UTC).
    pub next_reminder_at: Option<String>,
    /// Ripetizione: completando l'attività nasce la successiva.
    pub recurrence: Option<Recurrence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskInput {
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub priority: u8,
    pub due_date: Option<String>,
    pub note_id: Option<String>,
    #[serde(default)]
    pub flagged: bool,
    /// Richiede una scadenza, da cui parte la serie.
    #[serde(default)]
    pub recurrence: Option<Recurrence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TaskView {
    Today,
    Overdue,
    Upcoming,
    NoDate,
    Open,
    Flagged,
    Completed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskCounts {
    pub today: u64,
    pub overdue: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReminderEntity {
    Note,
    Task,
    Event,
}

impl ReminderEntity {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Note => "note",
            Self::Task => "task",
            Self::Event => "event",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "note" => Some(Self::Note),
            "task" => Some(Self::Task),
            "event" => Some(Self::Event),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReminderStatus {
    /// Programmato.
    Pending,
    /// Scattato, in attesa di un'azione dell'utente.
    Fired,
    /// Gestito o annullato.
    Done,
}

impl ReminderStatus {
    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "fired" => Self::Fired,
            "done" => Self::Done,
            _ => Self::Pending,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reminder {
    pub id: String,
    pub entity_type: ReminderEntity,
    pub entity_id: String,
    /// Istante UTC.
    pub remind_at: String,
    pub timezone: String,
    /// Orario locale effettivo nel fuso `timezone` (`AAAA-MM-GGTHH:MM`).
    pub local_time: String,
    pub status: ReminderStatus,
    pub last_triggered_at: Option<String>,
    pub entity_title: String,
    /// Nota da aprire: la nota stessa o quella collegata all'attività/evento.
    pub note_id: Option<String>,
    /// Ultima modifica; per un promemoria gestito è il momento della gestione.
    pub updated_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SnoozePreset {
    TenMinutes,
    OneHour,
    /// Domani alla stessa ora locale del promemoria.
    Tomorrow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CalendarEvent {
    pub id: String,
    pub note_id: Option<String>,
    pub note_title: Option<String>,
    pub title: String,
    pub description: String,
    pub all_day: bool,
    /// Istanti UTC per gli eventi con orario.
    pub start_at: Option<String>,
    pub end_at: Option<String>,
    /// Date civili (fine inclusa) per gli eventi di un giorno intero.
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub timezone: String,
    pub created_at: String,
    pub updated_at: String,
    /// Calendario esterno d'origine (`None` per gli eventi di Greatnote).
    pub external_calendar_id: Option<String>,
    /// `google` o `microsoft` per gli eventi esterni.
    pub calendar_provider: Option<String>,
    /// Non modificabile da Greatnote (occorrenze di eventi ricorrenti,
    /// calendari in sola lettura).
    pub read_only: bool,
    /// Indirizzo dell'evento nel calendario d'origine.
    pub web_link: Option<String>,
    /// Abbonamento iCal d'origine (eventi in sola lettura).
    pub subscription_id: Option<String>,
}

/// Dati inseriti dall'utente: per un evento con orario `start`/`end` sono
/// orari locali `AAAA-MM-GGTHH:MM`, per un giorno intero date `AAAA-MM-GG`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventInput {
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub all_day: bool,
    pub start: String,
    pub end: String,
    pub timezone: Option<String>,
    pub note_id: Option<String>,
}
