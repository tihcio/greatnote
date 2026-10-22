import { invoke } from "@tauri-apps/api/core";
import { separator } from "../app/menu";
import { t } from "../i18n";
import { askConfirmation } from "../lib/confirm";
import { openContextMenu } from "../lib/contextMenu";
import { errorMessage, formatDateTime } from "../lib/format";
import type { Task } from "../types";
import { dueLabel, dueState, priorityLabels, recurrenceLabel } from "./taskLabels";

interface TaskRowProps {
  task: Task;
  today: string;
  onToggle: (task: Task) => void;
  onEdit: (task: Task) => void;
  onOpenNote?: (noteId: string) => void;
  /** Dopo lo spostamento nel cestino dal menu contestuale. */
  onDeleted?: () => void;
  onError?: (message: string) => void;
  compact?: boolean;
}

/** Sposta un'attività nel cestino dopo la conferma. */
async function trashTask(task: Task): Promise<boolean> {
  const confirmed = await askConfirmation({
    title: t.titleDeleteTask,
    message: t.confirmDeleteTask(task.title),
    confirmLabel: t.actionMoveToTrash,
  });
  if (!confirmed) return false;
  await invoke("move_item_to_trash", { kind: "task", id: task.id });
  return true;
}

/** Riga di un'attività: casella di completamento, titolo e metadati. */
export default function TaskRow({
  task,
  today,
  onToggle,
  onEdit,
  onOpenNote,
  onDeleted,
  onError,
  compact,
}: TaskRowProps) {
  const done = task.status === "done";
  const due = dueLabel(task, today);
  return (
    <li
      className={`task-row${done ? " done" : ""}${compact ? " compact" : ""}`}
      onContextMenu={(event) =>
        openContextMenu(event, task.title, [
          { id: "ctx-task-toggle", label: done ? t.taskMenuReopen : t.taskMenuComplete, run: () => onToggle(task) },
          { id: "ctx-task-edit", label: t.editTask, run: () => onEdit(task) },
          {
            id: "ctx-task-note",
            label: t.openLinkedNote,
            enabled: Boolean(task.note_id && onOpenNote),
            run: () => {
              if (task.note_id) onOpenNote?.(task.note_id);
            },
          },
          ...(onDeleted
            ? [
                separator,
                {
                  id: "ctx-task-delete",
                  label: t.actionMoveToTrash,
                  destructive: true,
                  run: async () => {
                    try {
                      if (await trashTask(task)) onDeleted();
                    } catch (cause) {
                      onError?.(errorMessage(cause));
                    }
                  },
                },
              ]
            : []),
        ])
      }
    >
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
          {task.recurrence && task.status === "open" && (
            <span className="repeat-badge">
              <span aria-hidden="true">↻ </span>
              {recurrenceLabel(task.recurrence)}
            </span>
          )}
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
