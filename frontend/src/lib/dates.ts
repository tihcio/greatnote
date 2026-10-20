/**
 * Date civili (AAAA-MM-GG) e orari locali (AAAA-MM-GGTHH:MM) nel fuso del
 * sistema. Gli istanti arrivano da Rust in UTC e vengono solo visualizzati.
 */

const pad = (value: number) => String(value).padStart(2, "0");

export function toIsoDate(date: Date): string {
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** Interpreta AAAA-MM-GG come mezzogiorno locale, lontano dai cambi d'ora. */
export function parseIsoDate(value: string): Date {
  const [year, month, day] = value.split("-").map(Number);
  return new Date(year, month - 1, day, 12);
}

export function addDays(value: string, days: number): string {
  const date = parseIsoDate(value);
  date.setDate(date.getDate() + days);
  return toIsoDate(date);
}

export function addMonths(value: string, months: number): string {
  const date = parseIsoDate(value);
  date.setDate(1);
  date.setMonth(date.getMonth() + months);
  return toIsoDate(date);
}

/** Lunedì della settimana che contiene la data. */
export function startOfWeek(value: string): string {
  const date = parseIsoDate(value);
  const offset = (date.getDay() + 6) % 7;
  return addDays(value, -offset);
}

export function startOfMonth(value: string): string {
  return `${value.slice(0, 7)}-01`;
}

/** 42 giorni (6 settimane) che coprono il mese, a partire da lunedì. */
export function monthGrid(value: string): string[] {
  const first = startOfWeek(startOfMonth(value));
  return Array.from({ length: 42 }, (_, index) => addDays(first, index));
}

export function weekDays(value: string): string[] {
  const first = startOfWeek(value);
  return Array.from({ length: 7 }, (_, index) => addDays(first, index));
}

/** Orario locale AAAA-MM-GGTHH:MM di un istante, per `datetime-local`. */
export function toLocalInput(instant: string | Date): string {
  const date = typeof instant === "string" ? new Date(instant) : instant;
  return `${toIsoDate(date)}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** Giorno civile locale di un istante UTC. */
export function localDay(instant: string): string {
  return toIsoDate(new Date(instant));
}

/** Prossima ora piena fra almeno 30 minuti, come proposta predefinita. */
export function suggestedReminder(): string {
  const date = new Date(Date.now() + 30 * 60 * 1000);
  date.setMinutes(0, 0, 0);
  date.setHours(date.getHours() + 1);
  return toLocalInput(date);
}

export function formatTime(instant: string): string {
  return new Intl.DateTimeFormat(undefined, { hour: "2-digit", minute: "2-digit" }).format(
    new Date(instant),
  );
}

export function formatDay(value: string, options: Intl.DateTimeFormatOptions = {}): string {
  return new Intl.DateTimeFormat(undefined, {
    weekday: "short",
    day: "numeric",
    month: "short",
    ...options,
  }).format(parseIsoDate(value));
}

export function formatMonth(value: string): string {
  return new Intl.DateTimeFormat(undefined, { month: "long", year: "numeric" }).format(
    parseIsoDate(value),
  );
}

/** Nomi brevi dei giorni da lunedì a domenica nella lingua del sistema. */
export function weekdayNames(): string[] {
  const monday = parseIsoDate("2024-01-01");
  return Array.from({ length: 7 }, (_, index) => {
    const date = new Date(monday);
    date.setDate(monday.getDate() + index);
    return new Intl.DateTimeFormat(undefined, { weekday: "short" }).format(date);
  });
}
