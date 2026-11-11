import { useLayoutEffect, useRef } from "react";
import type { CSSProperties, MouseEvent as ReactMouseEvent, ReactNode } from "react";
import { t } from "../../i18n";
import { formatDay, formatTime, localDay } from "../../lib/dates";
import type { CalendarEvent } from "../../types";

/** Altezza di un'ora nella griglia, in pixel (uguale a `--hour-height`). */
export const HOUR_HEIGHT = 44;
/** Un clic crea un evento di questa durata, allineato alla mezz'ora. */
export const SLOT_MINUTES = 30;
const DAY_MINUTES = 24 * 60;
/** Durata minima disegnata, così un evento breve resta cliccabile. */
const MIN_DRAWN_MINUTES = 30;
/** Ora mostrata in alto all'apertura. */
const FIRST_VISIBLE_HOUR = 7;

export interface PlacedEvent {
  event: CalendarEvent;
  /** Minuti dalla mezzanotte locale del giorno. */
  start: number;
  end: number;
  /** Colonna (da 0) e numero di colonne del gruppo di eventi sovrapposti. */
  column: number;
  columns: number;
}

const pad = (value: number) => String(value).padStart(2, "0");

function minutesOfDay(instant: string): number {
  const date = new Date(instant);
  return date.getHours() * 60 + date.getMinutes();
}

/** Orario `HH:MM` dello spazio di mezz'ora che contiene il punto cliccato. */
export function slotTime(offsetY: number): string {
  const minutes = Math.floor((offsetY / HOUR_HEIGHT) * 60);
  const slot = Math.min(
    DAY_MINUTES - SLOT_MINUTES,
    Math.max(0, Math.floor(minutes / SLOT_MINUTES) * SLOT_MINUTES),
  );
  return `${pad(Math.floor(slot / 60))}:${pad(slot % 60)}`;
}

/**
 * Parte di ogni evento con orario che cade nel giorno, con gli eventi
 * sovrapposti affiancati in colonne. Un evento che attraversa la mezzanotte
 * viene tagliato ai limiti del giorno.
 */
export function placeEvents(events: CalendarEvent[], day: string): PlacedEvent[] {
  const segments = events
    .filter((event) => !event.all_day && event.start_at && event.end_at)
    .flatMap((event) => {
      const startDay = localDay(event.start_at as string);
      const endDay = localDay(event.end_at as string);
      if (startDay > day || endDay < day) return [];
      const start = startDay < day ? 0 : minutesOfDay(event.start_at as string);
      const end = endDay > day ? DAY_MINUTES : minutesOfDay(event.end_at as string);
      // Finisce esattamente alla mezzanotte di questo giorno: è del giorno prima.
      if (startDay < day && end === 0) return [];
      return [{ event, start, end: Math.max(end, start) }];
    })
    .sort((a, b) => a.start - b.start || b.end - a.end || a.event.id.localeCompare(b.event.id));

  const placed: PlacedEvent[] = [];
  let group: PlacedEvent[] = [];
  let columnEnds: number[] = [];
  let groupEnd = -1;
  const flush = () => {
    for (const item of group) item.columns = columnEnds.length;
    placed.push(...group);
    group = [];
    columnEnds = [];
  };
  for (const segment of segments) {
    const drawnEnd = Math.max(segment.end, segment.start + MIN_DRAWN_MINUTES);
    if (group.length > 0 && segment.start >= groupEnd) flush();
    let column = columnEnds.findIndex((end) => end <= segment.start);
    if (column < 0) column = columnEnds.length;
    columnEnds[column] = drawnEnd;
    groupEnd = Math.max(groupEnd, drawnEnd);
    group.push({ ...segment, column, columns: 1 });
  }
  flush();
  return placed;
}

interface TimeGridProps {
  days: string[];
  today: string;
  events: CalendarEvent[];
  /** Eventi di tutto il giorno e attività in scadenza del giorno. */
  renderAllDay: (day: string) => ReactNode;
  onCreate: (day: string, time?: string) => void;
  onOpenEvent: (event: CalendarEvent) => void;
  onEventMenu: (pointer: ReactMouseEvent, event: CalendarEvent) => void;
  onDayMenu: (pointer: ReactMouseEvent, day: string) => void;
  /** Apre la vista del singolo giorno (assente se è già quella). */
  onShowDay?: (day: string) => void;
  eventStyle: (event: CalendarEvent) => CSSProperties | undefined;
  /** Calendario esterno d'origine, per il nome accessibile. */
  eventSource: (event: CalendarEvent) => string;
}

/** Griglia oraria della vista Giorno e Settimana: un clic su uno spazio
 * vuoto propone un evento di mezz'ora a quell'orario. */
export default function TimeGrid({
  days,
  today,
  events,
  renderAllDay,
  onCreate,
  onOpenEvent,
  onEventMenu,
  onDayMenu,
  onShowDay,
  eventStyle,
  eventSource,
}: TimeGridProps) {
  const scroller = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (scroller.current) scroller.current.scrollTop = FIRST_VISIBLE_HOUR * HOUR_HEIGHT;
  }, []);

  const now = new Date();
  const nowMinutes = now.getHours() * 60 + now.getMinutes();
  const columns = { "--days": days.length, "--hour-height": `${HOUR_HEIGHT}px` } as CSSProperties;

  return (
    <div className="time-grid" ref={scroller} style={columns}>
      <div className="time-grid-top">
        <div className="time-grid-row">
          <div className="time-grid-gutter" />
          {days.map((day) => (
            <div className={`time-grid-day-head${day === today ? " today" : ""}`} key={day}>
              {onShowDay ? (
                <button
                  aria-current={day === today ? "date" : undefined}
                  className="time-grid-day-name"
                  onClick={() => onShowDay(day)}
                  title={t.showDayOf}
                  type="button"
                >
                  {formatDay(day)}
                </button>
              ) : (
                <span aria-current={day === today ? "date" : undefined} className="time-grid-day-name">
                  {formatDay(day, { weekday: "long" })}
                </span>
              )}
              <button
                aria-label={t.newEventOn(formatDay(day, { year: "numeric" }))}
                className="calendar-add"
                onClick={() => onCreate(day)}
                title={t.newEvent}
                type="button"
              >
                +
              </button>
            </div>
          ))}
        </div>
        <div className="time-grid-row time-grid-all-day">
          <div className="time-grid-gutter">{t.allDayShort}</div>
          {days.map((day) => (
            <div className="time-grid-all-day-cell" key={day} onContextMenu={(pointer) => onDayMenu(pointer, day)}>
              {renderAllDay(day)}
            </div>
          ))}
        </div>
      </div>
      <div className="time-grid-row time-grid-body">
        <div aria-hidden="true" className="time-grid-gutter time-grid-hours">
          {Array.from({ length: 23 }, (_, index) => (
            <span key={index} style={{ top: (index + 1) * HOUR_HEIGHT }}>
              {pad(index + 1)}:00
            </span>
          ))}
        </div>
        {days.map((day) => (
          <div
            aria-label={`${formatDay(day, { year: "numeric" })}. ${t.timeGridHint}`}
            className={`time-grid-column${day === today ? " today" : ""}`}
            data-day={day}
            key={day}
            onClick={(pointer) => {
              // Solo lo spazio vuoto: i clic sugli eventi li aprono.
              if (pointer.target !== pointer.currentTarget) return;
              const offset = pointer.clientY - pointer.currentTarget.getBoundingClientRect().top;
              onCreate(day, slotTime(offset));
            }}
            onContextMenu={(pointer) => onDayMenu(pointer, day)}
            role="group"
            title={t.timeGridHint}
          >
            {day === today && (
              <span aria-hidden="true" className="time-grid-now" style={{ top: (nowMinutes / 60) * HOUR_HEIGHT }} />
            )}
            {placeEvents(events, day).map(({ event, start, end, column, columns: count }) => {
              const source = event.calendar_provider ? ` (${eventSource(event)})` : "";
              const range = `${formatTime(event.start_at as string)} – ${formatTime(event.end_at as string)}`;
              return (
                <button
                  aria-label={`${event.title}, ${range}${source}`}
                  className={`time-grid-event${event.calendar_provider ? " external" : ""}`}
                  key={event.id}
                  onClick={() => onOpenEvent(event)}
                  onContextMenu={(pointer) => onEventMenu(pointer, event)}
                  style={{
                    ...eventStyle(event),
                    top: (start / 60) * HOUR_HEIGHT,
                    height: (Math.max(end - start, MIN_DRAWN_MINUTES) / 60) * HOUR_HEIGHT - 2,
                    left: `calc(${(column / count) * 100}% + 2px)`,
                    width: `calc(${100 / count}% - 4px)`,
                  }}
                  title={`${event.title} · ${range}${source}`}
                  type="button"
                >
                  <span className="time-grid-event-title">{event.title}</span>
                  <span className="calendar-time">{range}</span>
                </button>
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
}
