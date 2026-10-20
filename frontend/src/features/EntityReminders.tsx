import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { suggestedReminder } from "../lib/dates";
import { errorMessage, formatDateTime } from "../lib/format";
import type { Reminder, ReminderEntity } from "../types";
import DateTimeField from "./DateTimeField";

interface EntityRemindersProps {
  entityType: ReminderEntity;
  entityId: string;
  /** Chiamato dopo ogni modifica, per aggiornare contatori e liste. */
  onChanged?: () => void;
  /** Cambia per forzare il ricaricamento (es. promemoria scattato). */
  refreshKey?: number;
}

/** Elenco e aggiunta dei promemoria di una nota, attività o evento. */
export default function EntityReminders({
  entityType,
  entityId,
  onChanged,
  refreshKey,
}: EntityRemindersProps) {
  const [reminders, setReminders] = useState<Reminder[]>([]);
  const [localTime, setLocalTime] = useState(suggestedReminder);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    try {
      setReminders(
        await invoke<Reminder[]>("list_entity_reminders", { entityType, entityId }),
      );
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }, [entityId, entityType]);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  const add = async () => {
    if (!localTime) return;
    setBusy(true);
    try {
      await invoke<Reminder>("create_reminder", { entityType, entityId, localTime });
      setError(null);
      setLocalTime(suggestedReminder());
      await load();
      onChanged?.();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (reminder: Reminder) => {
    try {
      await invoke("delete_reminder", { id: reminder.id });
      await load();
      onChanged?.();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  return (
    <div className="entity-reminders">
      {reminders.length === 0 ? (
        <p className="detail-empty">{t.noNoteReminders}</p>
      ) : (
        <ul className="planner-mini-list">
          {reminders.map((reminder) => (
            <li key={reminder.id}>
              <span className={`reminder-state ${reminder.status}`} aria-hidden="true">
                {reminder.status === "fired" ? "!" : "◷"}
              </span>
              <span className="planner-mini-text">
                {formatDateTime(reminder.remind_at)}
                {reminder.status === "fired" && <small> · {t.firedReminders}</small>}
              </span>
              <button
                aria-label={`${t.deleteReminder}: ${formatDateTime(reminder.remind_at)}`}
                className="text-button destructive-text"
                onClick={() => void remove(reminder)}
                title={t.deleteReminder}
                type="button"
              >
                ×
              </button>
            </li>
          ))}
        </ul>
      )}
      {/* Non è un <form>: questo componente sta anche dentro i dialog di
          attività ed eventi, e un form annidato invierebbe anche quello esterno. */}
      <div className="inline-form reminder-add">
        <DateTimeField
          label={t.reminderTimeLabel}
          onChange={setLocalTime}
          onEnter={() => void add()}
          value={localTime}
        />
        <button
          className="subtle-action"
          disabled={busy || localTime.length !== 16}
          onClick={() => void add()}
          type="button"
        >
          {t.addReminder}
        </button>
      </div>
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
