import { t } from "../i18n";
import { formatDateTime } from "../lib/format";
import type { Task } from "../types";
import { dueLabel, dueState, priorityLabels } from "./taskLabels";

interface TaskRowProps {
  task: Task;
  today: string;
  onToggle: (task: Task) => void;
  onEdit: (task: Task) => void;
  onOpenNote?: (noteId: string) => void;
  compact?: boolean;
}

/** Riga di un'attività: casella di completamento, titolo e metadati. */
export default function TaskRow({ task, today, onToggle, onEdit, onOpenNote, compact }: TaskRowProps) {
  const done = task.status === "done";
  const due = dueLabel(task, today);
  return (
    <li className={`task-row${done ? " done" : ""}${compact ? " compact" : ""}`}>
      <button
        aria-checked={done}
        aria-label={done ? t.reopenTask(task.title) : t.completeTask(task.title)}
        className="task-check"
        onClick={() => onToggle(task)}
        role="checkbox"
        type="button"
      >
        <span aria-hidden="true">{done ? "✓" : ""}</span>
      </button>
      <div className="task-body">
        <button className="task-title" onClick={() => onEdit(task)} type="button">
          {task.title}
        </button>
        {!compact && task.description && <p className="task-description">{task.description}</p>}
        <div className="task-meta">
          {task.flagged && (
            <span className="flag-badge">
              <span aria-hidden="true">⚑ </span>
              {t.flaggedLabel}
            </span>
          )}
          {due && <span className={`due-badge ${dueState(task, today)}`}>{due}</span>}
          {task.priority > 0 && (
            <span className={`priority-badge p${task.priority}`}>
              {t.taskPriorityLabel}: {priorityLabels()[task.priority]}
            </span>
          )}
          {task.next_reminder_at && (
            <span className="task-reminder">
              <span aria-hidden="true">◷ </span>
              {t.reminderAtLabel(formatDateTime(task.next_reminder_at))}
            </span>
          )}
          {!compact && task.note_id && task.note_title && onOpenNote && (
            <button
              className="text-button task-note"
              onClick={() => onOpenNote(task.note_id as string)}
              type="button"
            >
              <span aria-hidden="true">▤ </span>
              {task.note_title}
            </button>
          )}
        </div>
      </div>
    </li>
  );
}
