import { language, t } from "../i18n";
import { formatDay } from "../lib/dates";
import { formatDate } from "../lib/format";
import type { Recurrence, Task } from "../types";

/** Nomi brevi dei giorni ISO 1–7 nella lingua dell'interfaccia. */
export function weekdayNames(format: "short" | "long" = "short"): string[] {
  const formatter = new Intl.DateTimeFormat(language, { weekday: format, timeZone: "UTC" });
  // 5 gennaio 2026 è un lunedì.
  return Array.from({ length: 7 }, (_, index) => formatter.format(new Date(Date.UTC(2026, 0, 5 + index))));
}

/** Ripetizione in parole, per l'elenco e il dialog. */
export function recurrenceLabel(rule: Recurrence): string {
  let text = t.repeatEvery(rule.frequency, rule.interval);
  if (rule.frequency === "weekly" && rule.weekdays && rule.weekdays.length > 0) {
    const names = weekdayNames();
    text += ` (${rule.weekdays.map((day) => names[day - 1]).join(", ")})`;
  }
  if (rule.until) text += `, ${t.repeatUntilShort(formatDay(rule.until))}`;
  return text;
}

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
