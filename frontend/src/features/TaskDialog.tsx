import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { askConfirmation } from "../lib/confirm";
import { addDays, suggestedReminder, toIsoDate, toLocalInput } from "../lib/dates";
import { errorMessage } from "../lib/format";
import type { Recurrence, RecurrenceFrequency, Reminder, SearchResult, Task, TaskInput } from "../types";
import DateTimeField from "./DateTimeField";
import EntityReminders from "./EntityReminders";
import PlannerDialog from "./PlannerDialog";
import { priorityLabels, weekdayNames } from "./taskLabels";

interface TaskDialogProps {
  /** Attività da modificare; `null` per crearne una nuova. */
  task: Task | null;
  /** Valori iniziali per una nuova attività. */
  defaults?: { noteId?: string | null; dueDate?: string | null };
  onClose: () => void;
  onSaved: (task: Task) => void;
  onDeleted: (id: string) => void;
  onOpenNote: (noteId: string) => void;
  onRemindersChanged?: () => void;
}

type ReminderChoice = "none" | "1h" | "4h" | "custom";

function inHours(hours: number): string {
  return toLocalInput(new Date(Date.now() + hours * 60 * 60 * 1000));
}

/** Creazione e modifica di un'attività. */
export default function TaskDialog({
  task,
  defaults,
  onClose,
  onSaved,
  onDeleted,
  onOpenNote,
  onRemindersChanged,
}: TaskDialogProps) {
  const today = toIsoDate(new Date());
  const [title, setTitle] = useState(task?.title ?? "");
  const [description, setDescription] = useState(task?.description ?? "");
  const [dueDate, setDueDate] = useState(task?.due_date ?? defaults?.dueDate ?? "");
  const [customDue, setCustomDue] = useState(
    Boolean(task?.due_date && task.due_date !== today && task.due_date !== addDays(today, 1)),
  );
  const [priority, setPriority] = useState(task?.priority ?? 0);
  const [flagged, setFlagged] = useState(task?.flagged ?? false);
  const [repeat, setRepeat] = useState<RecurrenceFrequency | null>(task?.recurrence?.frequency ?? null);
  const [repeatInterval, setRepeatInterval] = useState(task?.recurrence?.interval ?? 1);
  const [repeatDays, setRepeatDays] = useState<number[]>(task?.recurrence?.weekdays ?? []);
  const [repeatUntil, setRepeatUntil] = useState(task?.recurrence?.until ?? "");
  const [repeatNotice, setRepeatNotice] = useState(false);
  const [noteId, setNoteId] = useState<string | null>(task?.note_id ?? defaults?.noteId ?? null);
  const [noteOptions, setNoteOptions] = useState<{ id: string; title: string }[]>([]);
  const [reminderChoice, setReminderChoice] = useState<ReminderChoice>("none");
  const [customReminder, setCustomReminder] = useState(suggestedReminder);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void invoke<SearchResult[]>("search_notes", {
      query: "",
      scope: "all",
      scopeId: null,
      limit: 300,
      offset: 0,
    })
      .then((results) =>
        setNoteOptions(results.map((result) => ({ id: result.note.id, title: result.note.title || t.untitled }))),
      )
      .catch(() => setNoteOptions([]));
  }, []);

  const reminderTime = (): string | null => {
    switch (reminderChoice) {
      case "1h":
        return inHours(1);
      case "4h":
        return inHours(4);
      case "custom":
        return customReminder;
      default:
        return null;
    }
  };

  /** Regola dai campi; il giorno del mese resta quello già salvato. */
  const recurrence = (): Recurrence | null => {
    if (!repeat) return null;
    const keepDay = task?.recurrence?.frequency === repeat ? task.recurrence.month_day ?? null : null;
    return {
      frequency: repeat,
      interval: Math.min(99, Math.max(1, Math.round(repeatInterval) || 1)),
      weekdays: repeat === "weekly" ? repeatDays : [],
      month_day: repeat === "monthly" || repeat === "yearly" ? keepDay : null,
      until: repeatUntil || null,
    };
  };

  const chooseRepeat = (frequency: RecurrenceFrequency | null) => {
    setRepeat(frequency);
    // La serie parte dalla scadenza: senza, si usa oggi.
    if (frequency && !dueDate) {
      setDueDate(today);
      setRepeatNotice(true);
    }
  };

  const save = async () => {
    const input: TaskInput = {
      title,
      description,
      priority,
      due_date: dueDate || null,
      note_id: noteId,
      flagged,
      recurrence: recurrence(),
    };
    setBusy(true);
    try {
      const saved = task
        ? await invoke<Task>("update_task", { id: task.id, input })
        : await invoke<Task>("create_task", { input });
      const localTime = reminderTime();
      if (localTime) {
        await invoke<Reminder>("create_reminder", {
          entityType: "task",
          entityId: saved.id,
          localTime,
        });
        onRemindersChanged?.();
      }
      onSaved(saved);
      onClose();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!task) return;
    if (
      !(await askConfirmation({
        title: t.titleDeleteTask,
        message: t.confirmDeleteTask(task.title),
        confirmLabel: t.actionMoveToTrash,
      }))
    ) {
      return;
    }
    setBusy(true);
    try {
      await invoke("move_item_to_trash", { kind: "task", id: task.id });
      onDeleted(task.id);
      onClose();
    } catch (cause) {
      setError(errorMessage(cause));
      setBusy(false);
    }
  };

  const chip = (selected: boolean, label: string, onClick: () => void) => (
    <button
      aria-pressed={selected}
      className={`chip${selected ? " selected" : ""}`}
      key={label}
      onClick={onClick}
      type="button"
    >
      {label}
    </button>
  );

  const linkedNoteMissing = noteId && !noteOptions.some((option) => option.id === noteId);

  return (
    <PlannerDialog busy={busy} mark="✓" onClose={onClose} title={task ? t.editTask : t.newTaskAction}>
      <form
        className="organization-dialog-form task-form"
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <label className="dialog-field">
          <span>{t.taskNoteLabel}</span>
          <select onChange={(event) => setNoteId(event.target.value || null)} value={noteId ?? ""}>
            <option value="">{t.noLinkedNote}</option>
            {linkedNoteMissing && (
              <option value={noteId ?? ""}>{task?.note_title ?? t.linkedNoteTrashed}</option>
            )}
            {noteOptions.map((option) => (
              <option key={option.id} value={option.id}>
                {option.title}
              </option>
            ))}
          </select>
        </label>
        {noteId && task?.note_title && (
          <button
            className="text-button planner-note-link"
            onClick={() => {
              onClose();
              onOpenNote(noteId);
            }}
            type="button"
          >
            {t.openLinkedNote}: {task.note_title}
          </button>
        )}
        <label className="dialog-field">
          <span>{t.taskTitleLabel}</span>
          <input
            maxLength={512}
            onChange={(event) => setTitle(event.target.value)}
            placeholder={t.enterTask}
            required
            value={title}
          />
        </label>
        <label className="dialog-field">
          <span>{t.taskDescriptionLabel}</span>
          <textarea
            maxLength={8192}
            onChange={(event) => setDescription(event.target.value)}
            placeholder={t.taskDescriptionPlaceholder}
            rows={3}
            value={description}
          />
        </label>

        <div className="dialog-field">
          <span>{t.taskDueLabel}</span>
          <div className="chip-row">
            {chip(dueDate === today && !customDue, t.taskViewToday, () => {
              setCustomDue(false);
              setDueDate(today);
            })}
            {chip(dueDate === addDays(today, 1) && !customDue, t.tomorrow, () => {
              setCustomDue(false);
              setDueDate(addDays(today, 1));
            })}
            {chip(customDue, t.custom, () => {
              setCustomDue(true);
              if (!dueDate) setDueDate(addDays(today, 2));
            })}
            {dueDate !== "" &&
              chip(false, t.noDueDate, () => {
                setCustomDue(false);
                setDueDate("");
                // Senza scadenza la serie non ha un punto di partenza.
                setRepeat(null);
              })}
          </div>
          {customDue && (
            <input
              aria-label={t.taskDueLabel}
              onChange={(event) => setDueDate(event.target.value)}
              type="date"
              value={dueDate}
            />
          )}
        </div>

        <div className="dialog-field">
          <span>{t.repeatLabel}</span>
          <div className="chip-row">
            {chip(repeat === null, t.repeatNever, () => chooseRepeat(null))}
            {chip(repeat === "daily", t.repeatDaily, () => chooseRepeat("daily"))}
            {chip(repeat === "weekly", t.repeatWeekly, () => chooseRepeat("weekly"))}
            {chip(repeat === "monthly", t.repeatMonthly, () => chooseRepeat("monthly"))}
            {chip(repeat === "yearly", t.repeatYearly, () => chooseRepeat("yearly"))}
          </div>
          {repeat && (
            <div className="repeat-options">
              <label className="repeat-interval">
                <span>{t.repeatIntervalLabel}</span>
                <input
                  max={99}
                  min={1}
                  onChange={(event) => setRepeatInterval(Number(event.target.value))}
                  type="number"
                  value={repeatInterval}
                />
                <span>{t.repeatUnit(repeat, repeatInterval)}</span>
              </label>
              {repeat === "weekly" && (
                <div className="repeat-days" role="group" aria-label={t.repeatOnDays}>
                  <small>{t.repeatOnDays}</small>
                  <div className="chip-row">
                    {weekdayNames().map((name, index) =>
                      chip(repeatDays.includes(index + 1), name, () =>
                        setRepeatDays((current) =>
                          current.includes(index + 1)
                            ? current.filter((day) => day !== index + 1)
                            : [...current, index + 1].sort(),
                        ),
                      ),
                    )}
                  </div>
                </div>
              )}
              <label className="dialog-field">
                <small>{t.repeatUntil}</small>
                <input
                  min={dueDate || undefined}
                  onChange={(event) => setRepeatUntil(event.target.value)}
                  type="date"
                  value={repeatUntil}
                />
              </label>
              <small className="data-hint">{repeatNotice ? t.repeatNeedsDue : t.repeatHint}</small>
            </div>
          )}
        </div>

        <div className="dialog-field">
          <span>{t.noteRemindersTitle}</span>
          {task && <EntityReminders entityId={task.id} entityType="task" onChanged={onRemindersChanged} />}
          <div className="chip-row">
            {chip(reminderChoice === "1h", t.inOneHour, () =>
              setReminderChoice(reminderChoice === "1h" ? "none" : "1h"),
            )}
            {chip(reminderChoice === "4h", t.inFourHours, () =>
              setReminderChoice(reminderChoice === "4h" ? "none" : "4h"),
            )}
            {chip(reminderChoice === "custom", t.custom, () =>
              setReminderChoice(reminderChoice === "custom" ? "none" : "custom"),
            )}
          </div>
          {reminderChoice === "custom" && (
            <DateTimeField label={t.reminderTimeLabel} onChange={setCustomReminder} value={customReminder} />
          )}
          {reminderChoice !== "none" && <small className="data-hint">{t.reminderAddedOnSave}</small>}
        </div>

        <div className="dialog-field">
          <span>{t.taskPriorityLabel}</span>
          <div className="chip-row">
            {priorityLabels().map((label, value) => chip(priority === value, label, () => setPriority(value)))}
          </div>
        </div>

        <label className="toggle-field">
          <span>{t.flagTask}</span>
          <input
            checked={flagged}
            className="toggle"
            onChange={(event) => setFlagged(event.target.checked)}
            role="switch"
            type="checkbox"
          />
        </label>

        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
        <footer className="organization-dialog-actions">
          {task ? (
            <button
              className="dialog-cancel-button destructive-text"
              disabled={busy}
              onClick={() => void remove()}
              type="button"
            >
              {t.actionMoveToTrash}
            </button>
          ) : (
            <button className="dialog-cancel-button" disabled={busy} onClick={onClose} type="button">
              {t.cancel}
            </button>
          )}
          <button className="dialog-submit-button" disabled={busy || !title.trim()} type="submit">
            {task ? t.saveTask : t.createTaskAction}
          </button>
        </footer>
      </form>
    </PlannerDialog>
  );
}
