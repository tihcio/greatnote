import { t } from "../i18n";
import { formatDay } from "../lib/dates";
import { formatDate } from "../lib/format";
import type { Task } from "../types";

export function priorityLabels(): string[] {
  return [t.priorityNone, t.priorityLow, t.priorityMedium, t.priorityHigh];
}

export type DueState = "none" | "overdue" | "today" | "future";

export function dueState(task: Task, today: string): DueState {
  if (!task.due_date || task.status === "done") return "none";
  if (task.due_date < today) return "overdue";
  if (task.due_date === today) return "today";
  return "future";
}

/** Testo della scadenza: il significato non dipende solo dal colore. */
export function dueLabel(task: Task, today: string): string | null {
  if (task.status === "done" && task.completed_at) {
    return t.completedOn(formatDate(task.completed_at));
  }
  if (!task.due_date) return null;
  switch (dueState(task, today)) {
    case "overdue":
      return t.dueOverdue(formatDay(task.due_date));
    case "today":
      return t.dueToday;
    default:
      return t.dueOn(formatDay(task.due_date));
  }
}
