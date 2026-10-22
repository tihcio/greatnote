import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { toIsoDate } from "../lib/dates";
import { errorMessage } from "../lib/format";
import type { Task, TaskInput } from "../types";
import EntityReminders from "./EntityReminders";
import TaskDialog from "./TaskDialog";
import TaskRow from "./TaskRow";

interface NotePlannerProps {
  noteId: string;
  onChanged: () => void;
  onOpenNote: (noteId: string) => void;
  refreshKey: number;
}

/** Attività collegate e promemoria della nota aperta nell'editor. */
export default function NotePlanner({ noteId, onChanged, onOpenNote, refreshKey }: NotePlannerProps) {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [title, setTitle] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<Task | null>(null);

  const load = useCallback(async () => {
    try {
      setTasks(await invoke<Task[]>("list_note_tasks", { noteId }));
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }, [noteId]);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  const changed = () => {
    void load();
    onChanged();
  };

  const add = async () => {
    if (!title.trim()) return;
    const input: TaskInput = {
      title,
      description: "",
      priority: 0,
      due_date: null,
      note_id: noteId,
      flagged: false,
      recurrence: null,
    };
    try {
      await invoke<Task>("create_task", { input });
      setTitle("");
      changed();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const toggle = async (task: Task) => {
    try {
      await invoke<Task>("set_task_completed", { id: task.id, completed: task.status !== "done" });
      changed();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  return (
    <>
      <section className="note-detail-panel" aria-label={t.noteTasksTitle}>
        <div className="note-detail-heading">
          <h2>{t.noteTasksTitle}</h2>
        </div>
        {tasks.length === 0 ? (
          <p className="detail-empty">{t.noNoteTasks}</p>
        ) : (
          <ul className="task-list compact-list">
            {tasks.map((task) => (
              <TaskRow
                compact
                key={task.id}
                onDeleted={() => {
                  void load();
                  onChanged();
                }}
                onEdit={setEditing}
                onError={setError}
                onToggle={(item) => void toggle(item)}
                task={task}
                today={toIsoDate(new Date())}
              />
            ))}
          </ul>
        )}
        <form
          className="inline-form"
          onSubmit={(event) => {
            event.preventDefault();
            void add();
          }}
        >
          <input
            aria-label={t.quickTaskLabel}
            maxLength={512}
            onChange={(event) => setTitle(event.target.value)}
            placeholder={t.newTaskPlaceholder}
            value={title}
          />
          <button className="subtle-action" disabled={!title.trim()} type="submit">
            {t.addTask}
          </button>
        </form>
        {error && (
          <p className="inline-error" role="alert">
            {error}
          </p>
        )}
      </section>
      <section className="note-detail-panel note-reminders-panel" aria-label={t.noteRemindersTitle}>
        <div className="note-detail-heading">
          <h2>{t.noteRemindersTitle}</h2>
        </div>
        <EntityReminders entityId={noteId} entityType="note" onChanged={onChanged} refreshKey={refreshKey} />
      </section>
      {editing && (
        <TaskDialog
          onClose={() => setEditing(null)}
          onDeleted={changed}
          onOpenNote={onOpenNote}
          onRemindersChanged={changed}
          onSaved={changed}
          task={editing}
        />
      )}
    </>
  );
}
