import { useState } from "react";
import { t } from "../i18n";
import type { Reminder } from "../types";
import { ReminderActions, entityLabels, reminderTimeText } from "./RemindersView";

const MAX_VISIBLE = 3;

interface ReminderAlertsProps {
  reminders: Reminder[];
  onOpen: (reminder: Reminder) => void;
  onChanged: () => void;
  onShowAll: () => void;
}

/** Riquadro persistente con i promemoria scattati: resta finché l'utente non
 * li gestisce, così un promemoria importante non sparisce da solo. */
export default function ReminderAlerts({ reminders, onOpen, onChanged, onShowAll }: ReminderAlertsProps) {
  const [error, setError] = useState<string | null>(null);
  if (reminders.length === 0) return null;
  const visible = reminders.slice(0, MAX_VISIBLE);
  return (
    <aside aria-label={t.reminderAlertTitle(reminders.length)} aria-live="assertive" className="reminder-alerts">
      <header>
        <strong>
          <span aria-hidden="true">◷ </span>
          {t.reminderAlertTitle(reminders.length)}
        </strong>
        <button className="text-button" onClick={onShowAll} type="button">
          {t.showReminders}
        </button>
      </header>
      <ul>
        {visible.map((reminder) => (
          <li key={reminder.id}>
            <div className="reminder-main">
              <span className="reminder-kind">{entityLabels[reminder.entity_type]()}</span>
              <strong>{reminder.entity_title || t.untitled}</strong>
              <span className="reminder-time overdue">{reminderTimeText(reminder, true)}</span>
            </div>
            <ReminderActions
              onChanged={onChanged}
              onError={setError}
              onOpen={onOpen}
              reminder={reminder}
            />
          </li>
        ))}
      </ul>
      {reminders.length > visible.length && (
        <p className="reminder-more">{t.moreItems(reminders.length - visible.length)}</p>
      )}
      {error && <p className="inline-error">{error}</p>}
    </aside>
  );
}
