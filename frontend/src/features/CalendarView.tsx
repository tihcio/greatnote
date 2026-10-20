import { useCallback, useEffect, useMemo, useState } from "react";
import type { CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { t } from "../i18n";
import {
  addDays,
  addMonths,
  formatDay,
  formatMonth,
  formatTime,
  localDay,
  monthGrid,
  toIsoDate,
  weekDays,
  weekdayNames,
} from "../lib/dates";
import { errorMessage } from "../lib/format";
import type { CalendarEvent, Task } from "../types";
import { CALENDAR_SYNC_EVENT, calendarApi, calendarIndex, safeColor } from "./calendar/calendarApi";
import type { CalendarAccount, CalendarSubscription } from "./calendar/calendarApi";
import { providerName } from "./calendar/CalendarIntegrations";
import EventDialog from "./EventDialog";
import TaskDialog from "./TaskDialog";

type Mode = "month" | "week";
type DayItem =
  | { kind: "event"; event: CalendarEvent; sort: string }
  | { kind: "task"; task: Task; sort: string };

const MAX_MONTH_ITEMS = 3;

interface CalendarViewProps {
  onOpenNote: (noteId: string) => void;
  onChanged: () => void;
  /** Evento da aprire (es. da un promemoria). */
  initialEvent?: CalendarEvent | null;
  /** Cambia per aprire il dialog di un nuovo evento (Ctrl+E). */
  createRequest?: number;
  refreshKey: number;
}

/** Giorni civili locali coperti da un evento (fine inclusa per i giorni
 * interi; per gli eventi con orario la fine esatta a mezzanotte non conta). */
function eventDays(event: CalendarEvent): [string, string] {
  if (event.all_day) {
    return [event.start_date ?? "", event.end_date ?? event.start_date ?? ""];
  }
  const start = localDay(event.start_at as string);
  const endInstant = new Date(event.end_at as string);
  const lastMoment = new Date(Math.max(endInstant.getTime() - 1, new Date(event.start_at as string).getTime()));
  return [start, toIsoDate(lastMoment)];
}

export default function CalendarView({
  onOpenNote,
  onChanged,
  initialEvent,
  createRequest = 0,
  refreshKey,
}: CalendarViewProps) {
  const today = toIsoDate(new Date());
  const [mode, setMode] = useState<Mode>("month");
  const [anchor, setAnchor] = useState(() =>
    initialEvent ? eventDays(initialEvent)[0] : today,
  );
  const [events, setEvents] = useState<CalendarEvent[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [timezone, setTimezone] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [eventDialog, setEventDialog] = useState<{ event: CalendarEvent | null; day: string } | null>(
    initialEvent ? { event: initialEvent, day: eventDays(initialEvent)[0] } : null,
  );
  const [editingTask, setEditingTask] = useState<Task | null>(null);
  const [accounts, setAccounts] = useState<CalendarAccount[]>([]);
  const [subscriptions, setSubscriptions] = useState<CalendarSubscription[]>([]);
  const subscriptionIndex = useMemo(
    () => new Map(subscriptions.map((subscription) => [subscription.id, subscription])),
    [subscriptions],
  );
  const [syncing, setSyncing] = useState(false);
  const calendars = useMemo(() => calendarIndex(accounts), [accounts]);

  const days = useMemo(() => (mode === "month" ? monthGrid(anchor) : weekDays(anchor)), [anchor, mode]);
  const from = days[0];
  const to = days[days.length - 1];

  const load = useCallback(async () => {
    try {
      const [loadedEvents, loadedTasks, context, calendarStatus] = await Promise.all([
        invoke<CalendarEvent[]>("list_events", { from, to }),
        invoke<Task[]>("list_tasks_due_between", { from, to }),
        invoke<{ timezone: string }>("get_time_context"),
        // I calendari esterni sono facoltativi: un errore qui non blocca.
        calendarApi.status().catch(() => null),
      ]);
      setEvents(loadedEvents);
      if (calendarStatus) {
        setAccounts(calendarStatus.accounts);
        setSubscriptions(calendarStatus.subscriptions);
        setSyncing(calendarStatus.syncing);
      }
      setTasks(loadedTasks);
      setTimezone(context.timezone);
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }, [from, to]);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  // Ricarica quando la sincronizzazione in background porta novità.
  useEffect(() => {
    let stop: (() => void) | null = null;
    let cancelled = false;
    void listen(CALENDAR_SYNC_EVENT, () => void load()).then((unlisten) =>
      cancelled ? unlisten() : (stop = unlisten),
    );
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [load]);

  const syncNow = async () => {
    setSyncing(true);
    try {
      const next = await calendarApi.syncNow();
      setAccounts(next.accounts);
      setSubscriptions(next.subscriptions);
      const failed = next.accounts.find((account) => account.last_error);
      const failedFeed = next.subscriptions.find((subscription) => subscription.last_error);
      setError(
        failed
          ? `${providerName(failed.provider)} (${failed.label}): ${failed.last_error}`
          : failedFeed
            ? `${failedFeed.name}: ${failedFeed.last_error}`
            : null,
      );
      await load();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setSyncing(false);
    }
  };

  useEffect(() => {
    if (createRequest > 0) setEventDialog({ event: null, day: toIsoDate(new Date()) });
  }, [createRequest]);

  useEffect(() => {
    if (initialEvent) {
      setAnchor(eventDays(initialEvent)[0]);
      setEventDialog({ event: initialEvent, day: eventDays(initialEvent)[0] });
    }
  }, [initialEvent]);

  const itemsByDay = useMemo(() => {
    const map = new Map<string, DayItem[]>();
    const push = (day: string, item: DayItem) => {
      const list = map.get(day) ?? [];
      list.push(item);
      map.set(day, list);
    };
    for (const event of events) {
      const [first, last] = eventDays(event);
      for (let day = first < from ? from : first; day <= last && day <= to; day = addDays(day, 1)) {
        push(day, {
          kind: "event",
          event,
          sort: event.all_day ? "0" : `1${event.start_at}`,
        });
      }
    }
    for (const task of tasks) {
      if (task.due_date) push(task.due_date, { kind: "task", task, sort: "2" });
    }
    for (const list of map.values()) list.sort((a, b) => a.sort.localeCompare(b.sort));
    return map;
  }, [events, from, tasks, to]);

  const move = (direction: number) => {
    setAnchor(mode === "month" ? addMonths(anchor, direction) : addDays(anchor, 7 * direction));
  };

  const changed = () => {
    void load();
    onChanged();
  };

  const toggleTask = async (task: Task) => {
    try {
      await invoke("set_task_completed", { id: task.id, completed: true });
      changed();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const calendarLabel = (event: CalendarEvent) => {
    const feed = event.subscription_id ? subscriptionIndex.get(event.subscription_id) : undefined;
    if (feed) return `${feed.name}, ${t.icsProvider}`;
    const info = event.external_calendar_id ? calendars.get(event.external_calendar_id) : undefined;
    const provider = event.calendar_provider ? providerName(event.calendar_provider) : "";
    return info ? `${info.calendar.name}, ${provider}` : provider;
  };

  const eventStyle = (event: CalendarEvent): CSSProperties | undefined => {
    const color = event.subscription_id
      ? safeColor(subscriptionIndex.get(event.subscription_id)?.color)
      : event.external_calendar_id
        ? safeColor(calendars.get(event.external_calendar_id)?.calendar.color)
        : undefined;
    return color ? ({ "--event-color": color } as CSSProperties) : undefined;
  };

  const renderItem = (item: DayItem) =>
    item.kind === "event" ? (
      <li key={`e-${item.event.id}`}>
        <button
          className={`calendar-item event${item.event.all_day ? " all-day" : ""}${item.event.calendar_provider ? " external" : ""}`}
          onClick={() => setEventDialog({ event: item.event, day: anchor })}
          style={eventStyle(item.event)}
          title={
            item.event.calendar_provider
              ? `${item.event.title} — ${calendarLabel(item.event)}`
              : undefined
          }
          type="button"
        >
          {!item.event.all_day && item.event.start_at && (
            <span className="calendar-time">{formatTime(item.event.start_at)}</span>
          )}
          {item.event.title}
          {item.event.calendar_provider && <span className="visually-hidden"> ({calendarLabel(item.event)})</span>}
        </button>
      </li>
    ) : (
      <li key={`t-${item.task.id}`} className="calendar-task-entry">
        <button
          aria-label={t.completeTask(item.task.title)}
          className="calendar-task-check"
          onClick={() => void toggleTask(item.task)}
          type="button"
        >
          <span aria-hidden="true">☐</span>
        </button>
        <button className="calendar-item task" onClick={() => setEditingTask(item.task)} type="button">
          <span className="visually-hidden">{t.taskDueBadge}: </span>
          {item.task.title}
        </button>
      </li>
    );

  const anchorMonth = anchor.slice(0, 7);
  const periodTitle =
    mode === "month"
      ? formatMonth(anchor)
      : `${formatDay(from, { weekday: undefined })} – ${formatDay(to, { weekday: undefined, year: "numeric" })}`;

  return (
    <section className="workspace-panel calendar-panel" aria-labelledby="calendar-title">
      <header className="workspace-header calendar-header">
        <div>
          <div className="eyebrow">{t.calendarNav}</div>
          <h1 id="calendar-title" aria-live="polite">
            {periodTitle}
          </h1>
          {timezone && <p className="workspace-subtitle">{t.calendarTimezone(timezone)}</p>}
        </div>
        <div className="calendar-controls">
          <div className="segmented" role="group" aria-label={t.calendarNav}>
            <button
              aria-pressed={mode === "month"}
              className={`segment${mode === "month" ? " selected" : ""}`}
              onClick={() => setMode("month")}
              type="button"
            >
              {t.monthView}
            </button>
            <button
              aria-pressed={mode === "week"}
              className={`segment${mode === "week" ? " selected" : ""}`}
              onClick={() => setMode("week")}
              type="button"
            >
              {t.weekView}
            </button>
          </div>
          <button aria-label={t.previousPeriod} className="subtle-action" onClick={() => move(-1)} title={t.previousPeriod} type="button">
            ‹
          </button>
          <button className="subtle-action" onClick={() => setAnchor(today)} type="button">
            {t.todayButton}
          </button>
          <button aria-label={t.nextPeriod} className="subtle-action" onClick={() => move(1)} title={t.nextPeriod} type="button">
            ›
          </button>
          {(accounts.length > 0 || subscriptions.length > 0) && (
            <button
              aria-label={t.calendarSyncNow}
              className="subtle-action"
              disabled={syncing}
              onClick={() => void syncNow()}
              title={t.calendarSyncNow}
              type="button"
            >
              {syncing ? t.calendarSyncing : `⟳ ${t.calendarSyncShort}`}
            </button>
          )}
          <button
            className="subtle-action primary-action"
            onClick={() => setEventDialog({ event: null, day: anchor < from || anchor > to ? from : anchor })}
            type="button"
          >
            ＋ {t.newEvent}
          </button>
        </div>
      </header>

      {error && (
        <div className="inline-error calendar-error" role="alert">
          {error}
        </div>
      )}

      <div className={`calendar-grid ${mode}`} role="grid" aria-labelledby="calendar-title">
        <div className="calendar-weekdays" role="row">
          {weekdayNames().map((name) => (
            <div className="calendar-weekday" key={name} role="columnheader">
              {name}
            </div>
          ))}
        </div>
        {Array.from({ length: days.length / 7 }, (_, week) => (
          <div className="calendar-week" key={days[week * 7]} role="row">
            {days.slice(week * 7, week * 7 + 7).map((day) => {
              const items = itemsByDay.get(day) ?? [];
              const visible = mode === "month" ? items.slice(0, MAX_MONTH_ITEMS) : items;
              const outside = mode === "month" && day.slice(0, 7) !== anchorMonth;
              const label = t.calendarDayLabel(formatDay(day, { year: "numeric" }), items.length);
              return (
                <div
                  aria-label={label}
                  className={`calendar-day${outside ? " outside" : ""}${day === today ? " today" : ""}`}
                  key={day}
                  role="gridcell"
                >
                  <div className="calendar-day-header">
                    <span className="calendar-day-number" aria-current={day === today ? "date" : undefined}>
                      {mode === "week" ? formatDay(day) : Number(day.slice(8))}
                    </span>
                    <button
                      aria-label={t.newEventOn(formatDay(day, { year: "numeric" }))}
                      className="calendar-add"
                      onClick={() => setEventDialog({ event: null, day })}
                      title={t.newEvent}
                      type="button"
                    >
                      +
                    </button>
                  </div>
                  {visible.length > 0 && <ul className="calendar-items">{visible.map(renderItem)}</ul>}
                  {items.length > visible.length && (
                    <button
                      className="calendar-more"
                      onClick={() => {
                        setAnchor(day);
                        setMode("week");
                      }}
                      type="button"
                    >
                      {t.moreItems(items.length - visible.length)}
                    </button>
                  )}
                </div>
              );
            })}
          </div>
        ))}
      </div>

      {eventDialog && (
        <EventDialog
          calendars={calendars}
          subscriptions={subscriptionIndex}
          day={eventDialog.day}
          event={eventDialog.event}
          onChanged={changed}
          onClose={() => setEventDialog(null)}
          onOpenNote={onOpenNote}
        />
      )}
      {editingTask && (
        <TaskDialog
          onClose={() => setEditingTask(null)}
          onDeleted={changed}
          onOpenNote={onOpenNote}
          onRemindersChanged={changed}
          onSaved={changed}
          task={editingTask}
        />
      )}
    </section>
  );
}
