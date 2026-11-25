import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { addDays, toLocalInput } from "../lib/dates";
import { errorMessage } from "../lib/format";
import { askConfirmation } from "../lib/confirm";
import type { CalendarEvent, EventInput, Recurrence } from "../types";
import { calendarApi } from "./calendar/calendarApi";
import type { CalendarInfo, CalendarSubscription } from "./calendar/calendarApi";
import { providerName } from "./calendar/CalendarIntegrations";
import DateTimeField from "./DateTimeField";
import EntityReminders from "./EntityReminders";
import PlannerDialog from "./PlannerDialog";
import RecurrenceFields from "./RecurrenceFields";

interface EventDialogProps {
  /** Evento esistente, oppure giorno (AAAA-MM-GG) per un evento nuovo. */
  event: CalendarEvent | null;
  day: string;
  /** Orario d'inizio `HH:MM` di un evento nuovo (clic sulla griglia oraria):
   * l'evento proposto dura mezz'ora. */
  startTime?: string;
  onClose: () => void;
  onChanged: () => void;
  onOpenNote: (noteId: string) => void;
  /** Calendari esterni collegati (id → calendario e account). */
  calendars?: Map<string, CalendarInfo>;
  /** Abbonamenti iCal (id → abbonamento). */
  subscriptions?: Map<string, CalendarSubscription>;
}

/** Giorni da `from` a `to` (date AAAA-MM-GG). */
function daysBetween(from: string, to: string): number {
  return Math.round((Date.parse(to.slice(0, 10)) - Date.parse(from.slice(0, 10))) / 86_400_000);
}

/**
 * Dati della serie quando si modifica tutta la serie partendo da
 * un'occorrenza: lo spostamento in giorni e l'orario scelti valgono dal
 * primo giorno della serie, non da quello dell'occorrenza.
 */
function seriesInput(series: CalendarEvent, occurrenceDate: string, input: EventInput): EventInput {
  const shift = daysBetween(occurrenceDate, input.start);
  if (input.all_day) {
    const first = addDays(series.start_date ?? occurrenceDate, shift);
    return { ...input, start: first, end: addDays(first, daysBetween(input.start, input.end)) };
  }
  const seriesDay = toLocalInput(series.start_at ?? input.start).slice(0, 10);
  const start = `${addDays(seriesDay, shift)}${input.start.slice(10)}`;
  const duration = new Date(input.end).getTime() - new Date(input.start).getTime();
  return { ...input, start, end: toLocalInput(new Date(new Date(start).getTime() + duration)) };
}

/** Durata proposta per un evento creato con un clic sulla griglia oraria. */
const SLOT_DURATION_MS = 30 * 60 * 1000;

function initialRange(event: CalendarEvent | null, day: string, startTime?: string) {
  if (event?.all_day) {
    return { start: event.start_date ?? day, end: event.end_date ?? day };
  }
  if (event?.start_at && event.end_at) {
    return { start: toLocalInput(event.start_at), end: toLocalInput(event.end_at) };
  }
  if (startTime && /^\d{2}:\d{2}$/.test(startTime)) {
    const start = `${day}T${startTime}`;
    return { start, end: toLocalInput(new Date(new Date(start).getTime() + SLOT_DURATION_MS)) };
  }
  return { start: `${day}T09:00`, end: `${day}T10:00` };
}

export default function EventDialog({
  event,
  day,
  startTime,
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
  const initial = initialRange(event, day, startTime);
  const [start, setStart] = useState(initial.start);
  const [end, setEnd] = useState(initial.end);
  const [repeat, setRepeat] = useState<Recurrence | null>(event?.recurrence ?? null);
  /** Occorrenza aperta dal calendario: data d'inizio nella serie. */
  const occurrence = event?.recurrence ? event.occurrence_date : null;
  /** Scelta in attesa per un'occorrenza: a cosa applicare salva/elimina. */
  const [scope, setScope] = useState<"save" | "delete" | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** La ripetizione vale solo per gli eventi di Greatnote. */
  const canRepeat = !isExternal && !readOnly && !target;

  /** Spostando l'inizio, la fine conserva la durata (o diventa +1 ora se
   * precedeva l'inizio). */
  const changeStart = (value: string) => {
    if (!allDay && start.length === 16 && end.length === 16) {
      const duration = new Date(end).getTime() - new Date(start).getTime();
      const shifted = new Date(new Date(value).getTime() + (duration > 0 ? duration : 60 * 60 * 1000));
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

  const currentInput = (): EventInput => ({
    title,
    description,
    all_day: allDay,
    start,
    end,
    timezone: null,
    note_id: event?.note_id ?? null,
    recurrence: canRepeat ? repeat : null,
  });

  const run = async (task: () => Promise<unknown>) => {
    setBusy(true);
    try {
      await task();
      onChanged();
      onClose();
    } catch (cause) {
      setError(errorMessage(cause));
      setScope(null);
    } finally {
      setBusy(false);
    }
  };

  /** Salva tutta la serie; da un'occorrenza, rispetto al suo primo giorno. */
  const saveSeries = async (input: EventInput) => {
    if (!event) return;
    const data = occurrence
      ? seriesInput(await invoke<CalendarEvent>("get_event", { id: event.id }), occurrence, input)
      : input;
    await invoke<CalendarEvent>("update_event", { id: event.id, input: data });
  };

  const saveScoped = (which: "this" | "all") =>
    void run(async () => {
      if (!event || !occurrence) return;
      const input = currentInput();
      if (which === "this") {
        await invoke<CalendarEvent>("detach_event_occurrence", {
          id: event.id,
          date: occurrence,
          input: { ...input, recurrence: null },
        });
      } else {
        await saveSeries(input);
      }
    });

  const removeScoped = (which: "this" | "following" | "all") =>
    void run(async () => {
      if (!event || !occurrence) return;
      if (which === "this") {
        await invoke("skip_event_occurrence", { id: event.id, date: occurrence });
      } else if (which === "following") {
        const rest = await invoke<CalendarEvent | null>("end_event_series_before", {
          id: event.id,
          date: occurrence,
        });
        // Dalla prima occorrenza non resta niente: tutta la serie nel cestino.
        if (!rest) await invoke("move_item_to_trash", { kind: "event", id: event.id });
      } else {
        await invoke("move_item_to_trash", { kind: "event", id: event.id });
      }
    });

  const save = async () => {
    const input = currentInput();
    // Un'occorrenza: si chiede se cambiare solo questa o tutta la serie (se
    // cambia la ripetizione, riguarda per forza tutta la serie).
    if (occurrence && JSON.stringify(repeat) === JSON.stringify(event?.recurrence ?? null)) {
      setScope("save");
      return;
    }
    setBusy(true);
    try {
      if (event && occurrence) {
        await saveSeries(input);
      } else if (event && isExternal) {
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
    if (occurrence) {
      setScope("delete");
      return;
    }
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
        {canRepeat && (
          <RecurrenceFields
            daysLabel={t.eventRepeatDays}
            hint={t.eventRepeatHint}
            onChange={setRepeat}
            startDate={start.slice(0, 10)}
            value={repeat}
          />
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
        {scope && (
          <div className="occurrence-scope" role="group" aria-label={t.occurrenceScopeLabel}>
            <p>{scope === "save" ? t.occurrenceSaveQuestion : t.occurrenceDeleteQuestion}</p>
            <div className="occurrence-scope-actions">
              {scope === "save" ? (
                <>
                  <button className="subtle-action" disabled={busy} onClick={() => saveScoped("this")} type="button">
                    {t.occurrenceOnlyThis}
                  </button>
                  <button className="subtle-action" disabled={busy} onClick={() => saveScoped("all")} type="button">
                    {t.occurrenceWholeSeries}
                  </button>
                </>
              ) : (
                <>
                  <button className="subtle-action" disabled={busy} onClick={() => removeScoped("this")} type="button">
                    {t.occurrenceOnlyThis}
                  </button>
                  <button
                    className="subtle-action"
                    disabled={busy}
                    onClick={() => removeScoped("following")}
                    type="button"
                  >
                    {t.occurrenceThisAndFollowing}
                  </button>
                  <button
                    className="subtle-action destructive-text"
                    disabled={busy}
                    onClick={() => removeScoped("all")}
                    type="button"
                  >
                    {t.occurrenceWholeSeries}
                  </button>
                </>
              )}
              <button className="text-button" disabled={busy} onClick={() => setScope(null)} type="button">
                {t.cancel}
              </button>
            </div>
          </div>
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
