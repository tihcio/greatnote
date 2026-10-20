import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { toLocalInput } from "../lib/dates";
import { errorMessage } from "../lib/format";
import { askConfirmation } from "../lib/confirm";
import type { CalendarEvent, EventInput } from "../types";
import { calendarApi } from "./calendar/calendarApi";
import type { CalendarInfo, CalendarSubscription } from "./calendar/calendarApi";
import { providerName } from "./calendar/CalendarIntegrations";
import DateTimeField from "./DateTimeField";
import EntityReminders from "./EntityReminders";
import PlannerDialog from "./PlannerDialog";

interface EventDialogProps {
  /** Evento esistente, oppure giorno (AAAA-MM-GG) per un evento nuovo. */
  event: CalendarEvent | null;
  day: string;
  onClose: () => void;
  onChanged: () => void;
  onOpenNote: (noteId: string) => void;
  /** Calendari esterni collegati (id → calendario e account). */
  calendars?: Map<string, CalendarInfo>;
  /** Abbonamenti iCal (id → abbonamento). */
  subscriptions?: Map<string, CalendarSubscription>;
}

function initialRange(event: CalendarEvent | null, day: string) {
  if (event?.all_day) {
    return { start: event.start_date ?? day, end: event.end_date ?? day };
  }
  if (event?.start_at && event.end_at) {
    return { start: toLocalInput(event.start_at), end: toLocalInput(event.end_at) };
  }
  return { start: `${day}T09:00`, end: `${day}T10:00` };
}

export default function EventDialog({
  event,
  day,
  onClose,
  onChanged,
  onOpenNote,
  calendars = new Map(),
  subscriptions = new Map(),
}: EventDialogProps) {
  const external = event?.external_calendar_id ? calendars.get(event.external_calendar_id) ?? null : null;
  const feed = event?.subscription_id ? subscriptions.get(event.subscription_id) ?? null : null;
  const isExternal = Boolean(event?.external_calendar_id);
  const readOnly = Boolean(event?.read_only);
  /** Calendari in cui si può creare un evento (selezionati e modificabili). */
  const writable = [...calendars.values()].filter((info) => info.calendar.selected && info.calendar.can_edit);
  const [target, setTarget] = useState("");
  const [title, setTitle] = useState(event?.title ?? "");
  const [description, setDescription] = useState(event?.description ?? "");
  const [allDay, setAllDay] = useState(event?.all_day ?? false);
  const initial = initialRange(event, day);
  const [start, setStart] = useState(initial.start);
  const [end, setEnd] = useState(initial.end);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  /** Spostando l'inizio, la fine conserva la durata (o diventa +1 ora se
   * precedeva l'inizio). */
  const changeStart = (value: string) => {
    if (!allDay && start.length === 16 && end.length === 16) {
      const duration = new Date(end).getTime() - new Date(start).getTime();
      const shifted = new Date(new Date(value).getTime() + Math.max(duration, 60 * 60 * 1000));
      setEnd(toLocalInput(shifted));
    } else if (allDay && end < value) {
      setEnd(value);
    }
    setStart(value);
  };

  const toggleAllDay = (value: boolean) => {
    setAllDay(value);
    // Converte i valori fra data e data/ora mantenendo il giorno scelto.
    if (value) {
      setStart(start.slice(0, 10));
      setEnd(end.slice(0, 10));
    } else {
      setStart(`${start.slice(0, 10)}T09:00`);
      setEnd(`${end.slice(0, 10)}T10:00`);
    }
  };

  const save = async () => {
    const input: EventInput = {
      title,
      description,
      all_day: allDay,
      start,
      end,
      timezone: null,
      note_id: event?.note_id ?? null,
    };
    setBusy(true);
    try {
      if (event && isExternal) {
        await calendarApi.updateEvent(event.id, input);
      } else if (event) {
        await invoke<CalendarEvent>("update_event", { id: event.id, input });
      } else if (target) {
        await calendarApi.createEvent(target, input);
      } else {
        await invoke<CalendarEvent>("create_event", { input });
      }
      onChanged();
      onClose();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!event) return;
    const provider = event.calendar_provider ? providerName(event.calendar_provider) : null;
    const confirmed = await askConfirmation(
      provider
        ? { title: t.titleDeleteEvent, message: t.confirmDeleteExternalEvent(event.title, provider), confirmLabel: t.deleteEvent }
        : { title: t.titleDeleteEvent, message: t.confirmDeleteEvent(event.title), confirmLabel: t.actionMoveToTrash },
    );
    if (!confirmed) return;
    setBusy(true);
    try {
      if (isExternal) await calendarApi.deleteEvent(event.id);
      else await invoke("move_item_to_trash", { kind: "event", id: event.id });
      onChanged();
      onClose();
    } catch (cause) {
      setError(errorMessage(cause));
      setBusy(false);
    }
  };

  return (
    <PlannerDialog busy={busy} mark="▦" onClose={onClose} title={event ? t.editEvent : t.newEvent}>
      <form
        className="organization-dialog-form"
        onSubmit={(submit) => {
          submit.preventDefault();
          void save();
        }}
      >
        {event?.calendar_provider && (
          <div className="external-event-origin">
            <span
              className="calendar-swatch"
              aria-hidden="true"
              style={{ background: (feed?.color ?? external?.calendar.color) || undefined }}
            />
            <span>
              {t.eventFromCalendar(
                feed
                  ? feed.name
                  : external
                    ? `${external.calendar.name} · ${external.account.label}`
                    : providerName(event.calendar_provider),
                providerName(event.calendar_provider),
              )}
            </span>
            {event.web_link && (
              <button className="text-button" onClick={() => void calendarApi.openEvent(event.id)} type="button">
                {t.openInCalendar(providerName(event.calendar_provider))}
              </button>
            )}
          </div>
        )}
        {readOnly && (
          <p className="data-hint">{event?.calendar_provider === "ics" ? t.icsReadOnlyHint : t.eventReadOnlyHint}</p>
        )}
        {!event && writable.length > 0 && (
          <label className="dialog-field">
            <span>{t.eventCalendar}</span>
            <select onChange={(change) => setTarget(change.target.value)} value={target}>
              <option value="">{t.eventCalendarLocal}</option>
              {writable.map((info) => (
                <option key={info.calendar.id} value={info.calendar.id}>
                  {info.calendar.name} · {info.account.label} ({providerName(info.account.provider)})
                </option>
              ))}
            </select>
          </label>
        )}
        <fieldset className="event-fields" disabled={readOnly}>
        <label className="dialog-field">
          <span>{t.eventTitleLabel}</span>
          <input
            maxLength={512}
            onChange={(change) => setTitle(change.target.value)}
            required
            value={title}
          />
        </label>
        <label className="checkbox-field">
          <input
            checked={allDay}
            onChange={(change) => toggleAllDay(change.target.checked)}
            type="checkbox"
          />
          <span>{t.eventAllDay}</span>
        </label>
        {allDay ? (
          <div className="settings-row">
            <label className="dialog-field">
              <span>{t.eventStart}</span>
              <input
                onChange={(change) => change.target.value && changeStart(change.target.value)}
                required
                type="date"
                value={start}
              />
            </label>
            <label className="dialog-field">
              <span>{t.eventEnd}</span>
              <input
                min={start}
                onChange={(change) => change.target.value && setEnd(change.target.value)}
                required
                type="date"
                value={end}
              />
            </label>
          </div>
        ) : (
          <>
            <div className="dialog-field">
              <span>{t.eventStart}</span>
              <DateTimeField label={t.eventStart} onChange={changeStart} value={start} />
            </div>
            <div className="dialog-field">
              <span>{t.eventEnd}</span>
              <DateTimeField
                label={t.eventEnd}
                minDate={start.slice(0, 10)}
                onChange={setEnd}
                value={end}
              />
            </div>
          </>
        )}
        <label className="dialog-field">
          <span>{t.eventDescription}</span>
          <textarea
            maxLength={8192}
            onChange={(change) => setDescription(change.target.value)}
            rows={3}
            value={description}
          />
        </label>
        </fieldset>
        {event?.note_id && event.note_title && (
          <div className="dialog-field">
            <span>{t.taskNoteLabel}</span>
            <button
              className="text-button planner-note-link"
              onClick={() => {
                onClose();
                onOpenNote(event.note_id as string);
              }}
              type="button"
            >
              {event.note_title}
            </button>
          </div>
        )}
        {event && (
          <div className="dialog-field">
            <span>{t.noteRemindersTitle}</span>
            <EntityReminders entityId={event.id} entityType="event" onChanged={onChanged} />
          </div>
        )}
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
        <footer className="organization-dialog-actions">
          {event && readOnly ? (
            <button className="dialog-cancel-button" disabled={busy} onClick={onClose} type="button">
              {t.close}
            </button>
          ) : event ? (
            <button
              className="dialog-cancel-button destructive-text"
              disabled={busy}
              onClick={() => void remove()}
              type="button"
            >
              {isExternal ? t.deleteEvent : t.actionMoveToTrash}
            </button>
          ) : (
            <button className="dialog-cancel-button" disabled={busy} onClick={onClose} type="button">
              {t.cancel}
            </button>
          )}
          <button className="dialog-submit-button" disabled={busy || readOnly || !title.trim()} type="submit">
            {t.saveEvent}
          </button>
        </footer>
      </form>
    </PlannerDialog>
  );
}
