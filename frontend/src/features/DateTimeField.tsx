import { t } from "../i18n";

const HOURS = Array.from({ length: 24 }, (_, hour) => String(hour).padStart(2, "0"));
const MINUTE_STEP = 5;

interface DateTimeFieldProps {
  /** Orario locale `AAAA-MM-GGTHH:MM`. */
  value: string;
  onChange: (value: string) => void;
  /** Nome accessibile del gruppo (es. "Inizio"). */
  label: string;
  minDate?: string;
  onEnter?: () => void;
}

/**
 * Data e ora con controlli separati. WebKitGTK (Tauri su Linux) non offre un
 * selettore per `<input type="datetime-local">`: l'ora non è sceglibile. La
 * data usa il selettore nativo, ore e minuti due elenchi.
 */
export default function DateTimeField({ value, onChange, label, minDate, onEnter }: DateTimeFieldProps) {
  const date = value.slice(0, 10);
  const hour = value.slice(11, 13) || "09";
  const minute = value.slice(14, 16) || "00";
  const minutes = Array.from({ length: 60 / MINUTE_STEP }, (_, index) =>
    String(index * MINUTE_STEP).padStart(2, "0"),
  );
  // Un valore esistente non multiplo di 5 resta selezionabile.
  if (!minutes.includes(minute)) {
    minutes.push(minute);
    minutes.sort();
  }
  const update = (next: { date?: string; hour?: string; minute?: string }) =>
    onChange(`${next.date ?? date}T${next.hour ?? hour}:${next.minute ?? minute}`);

  return (
    <div
      aria-label={label}
      className="datetime-field"
      onKeyDown={(event) => {
        if (event.key === "Enter" && onEnter) {
          event.preventDefault();
          onEnter();
        }
      }}
      role="group"
    >
      <input
        aria-label={`${label}: ${t.dateLabel}`}
        min={minDate}
        onChange={(event) => {
          if (event.target.value) update({ date: event.target.value });
        }}
        required
        type="date"
        value={date}
      />
      <select
        aria-label={`${label}: ${t.hourLabel}`}
        onChange={(event) => update({ hour: event.target.value })}
        value={hour}
      >
        {HOURS.map((item) => (
          <option key={item} value={item}>
            {item}
          </option>
        ))}
      </select>
      <span aria-hidden="true" className="datetime-separator">
        :
      </span>
      <select
        aria-label={`${label}: ${t.minuteLabel}`}
        onChange={(event) => update({ minute: event.target.value })}
        value={minute}
      >
        {minutes.map((item) => (
          <option key={item} value={item}>
            {item}
          </option>
        ))}
      </select>
    </div>
  );
}
