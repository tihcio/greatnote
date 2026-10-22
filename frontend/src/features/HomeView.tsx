import type { MouseEvent as ReactMouseEvent } from "react";
import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { addDays, formatDay, formatTime, localDay, toIsoDate } from "../lib/dates";
import { errorMessage, formatDate } from "../lib/format";
import type { CalendarEvent, Note, Reminder, SearchResult, Task, TaskCounts } from "../types";

interface HomeViewProps {
  counts: TaskCounts | null;
  reminders: Reminder[];
  refreshKey: number;
  onCreateNote: () => void;
  onCreateTask: () => void;
  onCreateEvent: () => void;
  onOpenNote: (note: Note) => void;
  /** Menu contestuale di una nota (apri, nuova scheda, nuova finestra). */
  onNoteMenu?: (event: ReactMouseEvent, note: Note) => void;
  onOpenTasks: () => void;
  onOpenCalendar: () => void;
  onOpenReminders: () => void;
  onOpenShortcuts: () => void;
}

function plainPreview(note: Note): string {
  const parts: string[] = [];
  const walk = (node: { text?: string; content?: unknown[] }) => {
    if (node.text) parts.push(node.text);
    node.content?.forEach((child) => walk(child as { text?: string; content?: unknown[] }));
  };
  walk(note.content as { content?: unknown[] });
  return parts.join(" ").slice(0, 140);
}

/** Pagina iniziale: note recenti, scorciatoie, attività, agenda e promemoria. */
export default function HomeView(props: HomeViewProps) {
  const { counts, reminders, refreshKey } = props;
  const [recent, setRecent] = useState<Note[]>([]);
  const [shortcuts, setShortcuts] = useState<Note[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [events, setEvents] = useState<CalendarEvent[]>([]);
  const [error, setError] = useState<string | null>(null);
  const today = toIsoDate(new Date());

  useEffect(() => {
    const search = (scope: string, limit: number) =>
      invoke<SearchResult[]>("search_notes", { query: "", scope, scopeId: null, limit, offset: 0 }).then(
        (results) => results.map((result) => result.note),
      );
    void Promise.all([
      search("recent", 8),
      search("pinned", 8),
      invoke<Task[]>("list_tasks", { view: "today", search: null }),
      invoke<Task[]>("list_tasks", { view: "overdue", search: null }),
      invoke<CalendarEvent[]>("list_events", { from: today, to: addDays(today, 6) }),
    ])
      .then(([recentNotes, pinned, todayTasks, overdueTasks, upcoming]) => {
        setRecent(recentNotes);
        setShortcuts(pinned);
        setTasks([...overdueTasks, ...todayTasks].slice(0, 8));
        setEvents(upcoming.slice(0, 8));
        setError(null);
      })
      .catch((cause: unknown) => setError(errorMessage(cause)));
  }, [refreshKey, today]);

  const fired = reminders.filter((reminder) => reminder.status === "fired");
  const hour = new Date().getHours();
  const greeting = hour < 12 ? t.goodMorning : hour < 18 ? t.goodAfternoon : t.goodEvening;

  return (
    <section className="workspace-panel home-panel" aria-labelledby="home-title">
      <header className="workspace-header">
        <div>
          <div className="eyebrow">{formatDay(today, { year: "numeric", weekday: "long" })}</div>
          <h1 id="home-title">{greeting}</h1>
        </div>
        <div className="calendar-controls">
          <button className="subtle-action primary-action" onClick={props.onCreateNote} type="button">
            ＋ {t.newNote}
          </button>
          <button className="subtle-action" onClick={props.onCreateTask} type="button">
            ✓ {t.newTaskAction}
          </button>
          <button className="subtle-action" onClick={props.onCreateEvent} type="button">
            ▦ {t.newEvent}
          </button>
        </div>
      </header>
      <div className="workspace-scroll home-grid">
        {error && (
          <div className="inline-error" role="alert">
            {error}
          </div>
        )}
        <section className="home-card home-card-wide" aria-labelledby="home-recent">
          <h2 id="home-recent">{t.recentNotes}</h2>
          {recent.length === 0 ? (
            <p className="workspace-empty">{t.homeNoRecent}</p>
          ) : (
            <ul className="home-note-grid">
              {recent.map((note) => (
                <li key={note.id}>
                  <button
                    className="home-note"
                    onClick={() => props.onOpenNote(note)}
                    onContextMenu={(event) => props.onNoteMenu?.(event, note)}
                    type="button"
                  >
                    <strong>{note.title || t.untitled}</strong>
                    <span>{plainPreview(note) || "—"}</span>
                    <small>{formatDate(note.updated_at)}</small>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="home-card" aria-labelledby="home-shortcuts">
          <div className="home-card-heading">
            <h2 id="home-shortcuts">{t.pinnedNotes}</h2>
            <button className="text-button" onClick={props.onOpenShortcuts} type="button">
              {t.showAll}
            </button>
          </div>
          {shortcuts.length === 0 ? (
            <p className="workspace-empty">{t.homeNoShortcuts}</p>
          ) : (
            <ul className="home-list">
              {shortcuts.map((note) => (
                <li key={note.id}>
                  <button
                    className="text-button home-link"
                    onClick={() => props.onOpenNote(note)}
                    onContextMenu={(event) => props.onNoteMenu?.(event, note)}
                    type="button"
                  >
                    ★ {note.title || t.untitled}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="home-card" aria-labelledby="home-tasks">
          <div className="home-card-heading">
            <h2 id="home-tasks">{t.tasksNav}</h2>
            <button className="text-button" onClick={props.onOpenTasks} type="button">
              {t.showAll}
            </button>
          </div>
          {counts && (counts.overdue > 0 || counts.today > 0) && (
            <p className="data-hint">{t.taskCountsLabel(counts.today, counts.overdue)}</p>
          )}
          {tasks.length === 0 ? (
            <p className="workspace-empty">{t.homeNoTasks}</p>
          ) : (
            <ul className="home-list">
              {tasks.map((task) => (
                <li key={task.id}>
                  <span className={`due-badge ${task.due_date && task.due_date < today ? "overdue" : "today"}`}>
                    {task.due_date && task.due_date < today ? t.taskViewOverdue : t.taskViewToday}
                  </span>{" "}
                  {task.title}
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="home-card" aria-labelledby="home-agenda">
          <div className="home-card-heading">
            <h2 id="home-agenda">{t.homeAgenda}</h2>
            <button className="text-button" onClick={props.onOpenCalendar} type="button">
              {t.calendarNav}
            </button>
          </div>
          {events.length === 0 ? (
            <p className="workspace-empty">{t.homeNoEvents}</p>
          ) : (
            <ul className="home-list">
              {events.map((event) => (
                <li key={event.id}>
                  <small>
                    {event.all_day
                      ? formatDay(event.start_date ?? today)
                      : event.start_at
                        ? `${formatDay(localDay(event.start_at))} ${formatTime(event.start_at)}`
                        : ""}
                  </small>{" "}
                  {event.title}
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="home-card" aria-labelledby="home-reminders">
          <div className="home-card-heading">
            <h2 id="home-reminders">{t.remindersNav}</h2>
            <button className="text-button" onClick={props.onOpenReminders} type="button">
              {t.showAll}
            </button>
          </div>
          {fired.length === 0 ? (
            <p className="workspace-empty">{t.homeNoReminders}</p>
          ) : (
            <ul className="home-list">
              {fired.slice(0, 6).map((reminder) => (
                <li key={reminder.id}>
                  <span className="reminder-time overdue">{formatTime(reminder.remind_at)}</span>{" "}
                  {reminder.entity_title || t.untitled}
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </section>
  );
}
