import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { formatTime } from "../lib/dates";
import { errorMessage, formatDateTime } from "../lib/format";
import { askConfirmation } from "../lib/confirm";
import type { Reminder, ReminderEntity, SnoozePreset } from "../types";

export const entityLabels: Record<ReminderEntity, () => string> = {
  note: () => t.reminderEntityNote,
  task: () => t.reminderEntityTask,
  event: () => t.reminderEntityEvent,
};

/** Oltre questo ritardo un promemoria è scattato ad app chiusa o sospesa:
 * con l'app aperta lo scheduler controlla ogni 15 secondi. */
const MISSED_AFTER_MS = 60 * 1000;

export function isMissed(reminder: Reminder): boolean {
  if (!reminder.last_triggered_at) return false;
  return (
    new Date(reminder.last_triggered_at).getTime() - new Date(reminder.remind_at).getTime() >
    MISSED_AFTER_MS
  );
}

/** "Scaduto alle 12:29 · perso mentre l'app era chiusa" oppure data e ora. */
export function reminderTimeText(reminder: Reminder, short = false): string {
  if (reminder.status !== "fired") return formatDateTime(reminder.remind_at);
  const time = short ? formatTime(reminder.remind_at) : formatDateTime(reminder.remind_at);
  const due = t.reminderDueAt(time);
  return isMissed(reminder) ? `${due} · ${t.reminderMissed}` : due;
}

interface ReminderActionsProps {
  reminder: Reminder;
  onOpen: (reminder: Reminder) => void;
  onChanged: () => void;
  onError: (message: string) => void;
}

/** Azioni su un promemoria: apri, rinvia, segna come gestito, elimina. */
export function ReminderActions({ reminder, onOpen, onChanged, onError }: ReminderActionsProps) {
  const [busy, setBusy] = useState(false);
  const act = async (task: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await task();
      onChanged();
    } catch (cause) {
      onError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  };
  const snooze = (preset: SnoozePreset) =>
    act(() => invoke("snooze_reminder", { id: reminder.id, preset }));
  const done = () =>
    act(() =>
      reminder.entity_type === "task"
        ? invoke("set_task_completed", { id: reminder.entity_id, completed: true })
        : invoke("dismiss_reminder", { id: reminder.id }),
    );

  return (
    <div className="reminder-actions">
      <button className="text-button" disabled={busy} onClick={() => onOpen(reminder)} type="button">
        {t.openItem}
      </button>
      <button className="text-button" disabled={busy} onClick={() => void snooze("ten-minutes")} type="button">
        {t.snoozeTenMinutes}
      </button>
      <button className="text-button" disabled={busy} onClick={() => void snooze("one-hour")} type="button">
        {t.snoozeOneHour}
      </button>
      <button className="text-button" disabled={busy} onClick={() => void snooze("tomorrow")} type="button">
        {t.snoozeTomorrow}
      </button>
      {reminder.status === "fired" && (
        <button className="text-button strong-text" disabled={busy} onClick={() => void done()} type="button">
          {t.reminderDone}
        </button>
      )}
      <button
        aria-label={`${t.deleteReminder}: ${reminder.entity_title}`}
        className="text-button destructive-text"
        disabled={busy}
        onClick={() => void act(() => invoke("delete_reminder", { id: reminder.id }))}
        title={t.deleteReminder}
        type="button"
      >
        ×
      </button>
    </div>
  );
}

interface RemindersViewProps {
  reminders: Reminder[];
  onOpen: (reminder: Reminder) => void;
  onChanged: () => void;
  refreshKey: number;
}

interface HistoryItemProps {
  reminder: Reminder;
  onOpen: (reminder: Reminder) => void;
  onDelete: (reminder: Reminder) => void;
}

/** Promemoria gestito: orario previsto, momento della gestione, apri o elimina. */
function HistoryItem({ reminder, onOpen, onDelete }: HistoryItemProps) {
  return (
    <li className="reminder-row done">
      <div className="reminder-main">
        <span className="reminder-kind">{entityLabels[reminder.entity_type]()}</span>
        <strong>{reminder.entity_title || t.untitled}</strong>
        <span className="reminder-time">
          {t.scheduledFor(formatDateTime(reminder.remind_at))} ·{" "}
          {t.handledOn(formatDateTime(reminder.updated_at))}
        </span>
      </div>
      <div className="reminder-actions">
        <button className="text-button" onClick={() => onOpen(reminder)} type="button">
          {t.openItem}
        </button>
        <button
          aria-label={`${t.deleteReminder}: ${reminder.entity_title}`}
          className="text-button destructive-text"
          onClick={() => onDelete(reminder)}
          title={t.deleteReminder}
          type="button"
        >
          ×
        </button>
      </div>
    </li>
  );
}

function ReminderItem({
  reminder,
  onOpen,
  onChanged,
  onError,
}: ReminderActionsProps) {
  return (
    <li className={`reminder-row ${reminder.status}`}>
      <div className="reminder-main">
        <span className="reminder-kind">{entityLabels[reminder.entity_type]()}</span>
        <strong>{reminder.entity_title || t.untitled}</strong>
        <span className={`reminder-time${reminder.status === "fired" ? " overdue" : ""}`}>
          {reminderTimeText(reminder)}
        </span>
      </div>
      <ReminderActions onChanged={onChanged} onError={onError} onOpen={onOpen} reminder={reminder} />
    </li>
  );
}

export default function RemindersView({ reminders, onOpen, onChanged, refreshKey }: RemindersViewProps) {
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState<Reminder[]>([]);

  const loadHistory = useCallback(async () => {
    try {
      setHistory(await invoke<Reminder[]>("list_reminder_history"));
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }, []);

  useEffect(() => {
    void loadHistory();
  }, [loadHistory, refreshKey]);

  const deleteHandled = async (reminder: Reminder) => {
    try {
      await invoke("delete_reminder", { id: reminder.id });
      await loadHistory();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const clearHistory = async () => {
    if (!(await askConfirmation({ title: t.clearHistory, message: t.confirmClearHistory(history.length), confirmLabel: t.clearHistory, destructive: true }))) return;
    try {
      await invoke<number>("clear_reminder_history");
      await loadHistory();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };
  const fired = reminders.filter((reminder) => reminder.status === "fired");
  const pending = reminders.filter((reminder) => reminder.status === "pending");

  return (
    <section className="workspace-panel" aria-labelledby="reminders-title">
      <header className="workspace-header">
        <div>
          <h1 id="reminders-title">{t.remindersNav}</h1>
          <p className="workspace-subtitle">{t.remindersIntro}</p>
        </div>
      </header>
      <div className="workspace-scroll">
        {error && (
          <div className="inline-error" role="alert">
            {error}
          </div>
        )}
        {fired.length > 0 && (
          <section className="reminder-section" aria-labelledby="fired-reminders">
            <h2 id="fired-reminders">
              {t.firedReminders} <span className="segment-count">{fired.length}</span>
            </h2>
            <ul className="reminder-list">
              {fired.map((reminder) => (
                <ReminderItem
                  key={reminder.id}
                  onChanged={onChanged}
                  onError={setError}
                  onOpen={onOpen}
                  reminder={reminder}
                />
              ))}
            </ul>
          </section>
        )}
        <section className="reminder-section" aria-labelledby="scheduled-reminders">
          <h2 id="scheduled-reminders">{t.scheduledReminders}</h2>
          {pending.length === 0 ? (
            <p className="workspace-empty">{t.noReminders}</p>
          ) : (
            <ul className="reminder-list">
              {pending.map((reminder) => (
                <ReminderItem
                  key={reminder.id}
                  onChanged={onChanged}
                  onError={setError}
                  onOpen={onOpen}
                  reminder={reminder}
                />
              ))}
            </ul>
          )}
        </section>
        <section className="reminder-section" aria-labelledby="handled-reminders">
          <div className="reminder-section-heading">
            <h2 id="handled-reminders">{t.handledReminders}</h2>
            {history.length > 0 && (
              <button className="text-button destructive-text" onClick={() => void clearHistory()} type="button">
                {t.clearHistory}
              </button>
            )}
          </div>
          {history.length === 0 ? (
            <p className="workspace-empty">{t.noHandledReminders}</p>
          ) : (
            <ul className="reminder-list">
              {history.map((reminder) => (
                <HistoryItem
                  key={reminder.id}
                  onDelete={(item) => void deleteHandled(item)}
                  onOpen={onOpen}
                  reminder={reminder}
                />
              ))}
            </ul>
          )}
        </section>
      </div>
    </section>
  );
}
