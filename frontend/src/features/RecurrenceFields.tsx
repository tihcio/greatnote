import type { ReactNode } from "react";
import { t } from "../i18n";
import type { Recurrence, RecurrenceFrequency } from "../types";
import { weekdayNames } from "./taskLabels";

interface RecurrenceFieldsProps {
  value: Recurrence | null;
  onChange: (value: Recurrence | null) => void;
  /** Prima data della serie: la data di fine non può precederla. */
  startDate?: string;
  /** Testo sotto le opzioni (cosa succede a ogni ripetizione). */
  hint?: ReactNode;
  /** Etichetta dei giorni della settimana (cosa vale se nessuno è scelto). */
  daysLabel?: string;
}

/** Campi «Ripeti» di attività ed eventi: frequenza, intervallo, giorni della
 * settimana e data di fine. Il giorno del mese resta quello già salvato se
 * la frequenza non cambia. */
export default function RecurrenceFields({ value, onChange, startDate, hint, daysLabel }: RecurrenceFieldsProps) {
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
  const choose = (frequency: RecurrenceFrequency | null) => {
    if (!frequency) {
      onChange(null);
      return;
    }
    const same = value?.frequency === frequency;
    onChange({
      frequency,
      interval: value?.interval ?? 1,
      weekdays: frequency === "weekly" ? (same ? value?.weekdays ?? [] : []) : [],
      month_day: same ? value?.month_day ?? null : null,
      until: value?.until ?? null,
    });
  };
  const update = (patch: Partial<Recurrence>) => {
    if (value) onChange({ ...value, ...patch });
  };
  const days = value?.weekdays ?? [];

  return (
    <div className="dialog-field">
      <span>{t.repeatLabel}</span>
      <div className="chip-row">
        {chip(value === null, t.repeatNever, () => choose(null))}
        {chip(value?.frequency === "daily", t.repeatDaily, () => choose("daily"))}
        {chip(value?.frequency === "weekly", t.repeatWeekly, () => choose("weekly"))}
        {chip(value?.frequency === "monthly", t.repeatMonthly, () => choose("monthly"))}
        {chip(value?.frequency === "yearly", t.repeatYearly, () => choose("yearly"))}
      </div>
      {value && (
        <div className="repeat-options">
          <label className="repeat-interval">
            <span>{t.repeatIntervalLabel}</span>
            <input
              max={99}
              min={1}
              onChange={(event) =>
                update({ interval: Math.min(99, Math.max(1, Math.round(Number(event.target.value)) || 1)) })
              }
              type="number"
              value={value.interval}
            />
            <span>{t.repeatUnit(value.frequency, value.interval)}</span>
          </label>
          {value.frequency === "weekly" && (
            <div className="repeat-days" role="group" aria-label={daysLabel ?? t.repeatOnDays}>
              <small>{daysLabel ?? t.repeatOnDays}</small>
              <div className="chip-row">
                {weekdayNames().map((name, index) =>
                  chip(days.includes(index + 1), name, () =>
                    update({
                      weekdays: days.includes(index + 1)
                        ? days.filter((day) => day !== index + 1)
                        : [...days, index + 1].sort(),
                    }),
                  ),
                )}
              </div>
            </div>
          )}
          <label className="dialog-field">
            <small>{t.repeatUntil}</small>
            <input
              min={startDate || undefined}
              onChange={(event) => update({ until: event.target.value || null })}
              type="date"
              value={value.until ?? ""}
            />
          </label>
          {hint && <small className="data-hint">{hint}</small>}
        </div>
      )}
    </div>
  );
}
