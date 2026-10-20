import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { addDays, toIsoDate } from "../lib/dates";
import { errorMessage } from "../lib/format";
import type { Task, TaskCounts, TaskInput, TaskView } from "../types";
import TaskDialog from "./TaskDialog";
import TaskRow from "./TaskRow";
import { priorityLabels } from "./taskLabels";

interface TasksViewProps {
  counts: TaskCounts | null;
  onOpenNote: (noteId: string) => void;
  onChanged: () => void;
  /** Attività da aprire subito (es. da un promemoria). */
  initialTask?: Task | null;
  /** Cambia per aprire il dialog di creazione (menu File › Nuova attività). */
  createRequest?: number;
  refreshKey: number;
}

const views: { id: TaskView; label: () => string }[] = [
  { id: "today", label: () => t.taskViewToday },
  { id: "overdue", label: () => t.taskViewOverdue },
  { id: "upcoming", label: () => t.taskViewUpcoming },
  { id: "no-date", label: () => t.taskViewNoDate },
  { id: "open", label: () => t.taskViewOpen },
  { id: "flagged", label: () => t.taskViewFlagged },
  { id: "completed", label: () => t.taskViewCompleted },
];

function defaultDue(view: TaskView, today: string): string {
  if (view === "today" || view === "overdue") return today;
  if (view === "upcoming") return addDays(today, 1);
  return "";
}

export default function TasksView({
  counts,
  onOpenNote,
  onChanged,
  initialTask,
  createRequest = 0,
  refreshKey,
}: TasksViewProps) {
  const [view, setView] = useState<TaskView>("today");
  const [tasks, setTasks] = useState<Task[]>([]);
  const [search, setSearch] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [title, setTitle] = useState("");
  const [today, setToday] = useState(() => toIsoDate(new Date()));
  const [dueDate, setDueDate] = useState(() => defaultDue("today", toIsoDate(new Date())));
  const [priority, setPriority] = useState(0);
  const [editing, setEditing] = useState<Task | null>(initialTask ?? null);
  const [creating, setCreating] = useState(createRequest > 0);
  const titleInput = useRef<HTMLInputElement>(null);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const trimmed = search.trim();
      setTasks(await invoke<Task[]>("list_tasks", { view, search: trimmed || null }));
      setToday(toIsoDate(new Date()));
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setLoading(false);
    }
  }, [search, view]);

  useEffect(() => {
    const timer = window.setTimeout(() => void load(), search ? 200 : 0);
    return () => window.clearTimeout(timer);
  }, [load, refreshKey, search]);

  useEffect(() => {
    if (initialTask) setEditing(initialTask);
  }, [initialTask]);

  useEffect(() => {
    if (createRequest > 0) setCreating(true);
  }, [createRequest]);

  const changeView = (next: TaskView) => {
    setView(next);
    setDueDate(defaultDue(next, today));
  };

  const addTask = async () => {
    if (!title.trim()) return;
    const input: TaskInput = {
      title,
      description: "",
      priority,
      due_date: dueDate || null,
      note_id: null,
      flagged: false,
    };
    try {
      await invoke<Task>("create_task", { input });
      setTitle("");
      setPriority(0);
      setError(null);
      await load();
      onChanged();
      titleInput.current?.focus();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const toggle = async (task: Task) => {
    try {
      await invoke<Task>("set_task_completed", { id: task.id, completed: task.status !== "done" });
      await load();
      onChanged();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const countFor = (id: TaskView) =>
    id === "today" ? counts?.today : id === "overdue" ? counts?.overdue : undefined;

  return (
    <section className="workspace-panel" aria-labelledby="tasks-title">
      <header className="workspace-header">
        <div>
          <h1 id="tasks-title">{t.tasksNav}</h1>
          <p className="workspace-subtitle">{t.tasksSubtitle}</p>
        </div>
        <button className="subtle-action primary-action" onClick={() => setCreating(true)} type="button">
          ＋ {t.newTaskAction}
        </button>
      </header>

      <div className="workspace-toolbar">
        <div className="segmented" role="tablist" aria-label={t.tasksNav}>
          {views.map((item, index) => {
            const count = countFor(item.id);
            return (
              <button
                aria-selected={view === item.id}
                className={`segment${view === item.id ? " selected" : ""}`}
                id={`task-view-${item.id}`}
                key={item.id}
                onClick={() => changeView(item.id)}
                onKeyDown={(event) => {
                  if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") return;
                  event.preventDefault();
                  const offset = event.key === "ArrowRight" ? 1 : views.length - 1;
                  const next = views[(index + offset) % views.length];
                  changeView(next.id);
                  document.getElementById(`task-view-${next.id}`)?.focus();
                }}
                role="tab"
                tabIndex={view === item.id ? 0 : -1}
                type="button"
              >
                {item.label()}
                {count ? <span className="segment-count">{count}</span> : null}
              </button>
            );
          })}
        </div>
        <label className="search-field workspace-search">
          <span aria-hidden="true" className="search-icon">
            ⌕
          </span>
          <input
            aria-label={t.searchTasks}
            maxLength={200}
            onChange={(event) => setSearch(event.target.value)}
            placeholder={t.searchTasks}
            type="search"
            value={search}
          />
        </label>
      </div>

      <div className="workspace-scroll" role="tabpanel" aria-labelledby={`task-view-${view}`}>
        {view !== "completed" && (
          <form
            className="quick-task-form"
            onSubmit={(event) => {
              event.preventDefault();
              void addTask();
            }}
          >
            <input
              aria-label={t.newTaskPlaceholder}
              className="quick-task-title"
              maxLength={512}
              onChange={(event) => setTitle(event.target.value)}
              placeholder={t.newTaskPlaceholder}
              ref={titleInput}
              value={title}
            />
            <input
              aria-label={t.taskDueLabel}
              onChange={(event) => setDueDate(event.target.value)}
              type="date"
              value={dueDate}
            />
            <select
              aria-label={t.taskPriorityLabel}
              onChange={(event) => setPriority(Number(event.target.value))}
              value={priority}
            >
              {priorityLabels().map((label, value) => (
                <option key={value} value={value}>
                  {value === 0 ? `${t.taskPriorityLabel}: ${label}` : label}
                </option>
              ))}
            </select>
            <button className="subtle-action" disabled={!title.trim()} type="submit">
              {t.addTask}
            </button>
          </form>
        )}

        {error && (
          <div className="inline-error" role="alert">
            {error}
          </div>
        )}

        {loading && tasks.length === 0 ? (
          <div className="list-message" role="status">
            <span className="loading-spinner" />
          </div>
        ) : tasks.length === 0 ? (
          <p className="workspace-empty">{t.noTasks}</p>
        ) : (
          <ul className="task-list" aria-label={views.find((item) => item.id === view)?.label()}>
            {tasks.map((task) => (
              <TaskRow
                key={task.id}
                onEdit={setEditing}
                onOpenNote={onOpenNote}
                onToggle={(item) => void toggle(item)}
                task={task}
                today={today}
              />
            ))}
          </ul>
        )}
      </div>

      {creating && (
        <TaskDialog
          defaults={{ dueDate: defaultDue(view, today) || null }}
          onClose={() => setCreating(false)}
          onDeleted={() => undefined}
          onOpenNote={onOpenNote}
          onRemindersChanged={onChanged}
          onSaved={() => {
            void load();
            onChanged();
          }}
          task={null}
        />
      )}
      {editing && (
        <TaskDialog
          onClose={() => setEditing(null)}
          onDeleted={() => {
            void load();
            onChanged();
          }}
          onOpenNote={onOpenNote}
          onRemindersChanged={() => {
            void load();
            onChanged();
          }}
          onSaved={() => {
            void load();
            onChanged();
          }}
          task={editing}
        />
      )}
    </section>
  );
}
