import { useRef } from "react";
import type { KeyboardEvent, PointerEvent } from "react";

interface ColumnResizerProps {
  /** Bordo destro della barra laterale o dell'elenco delle note. */
  edge: "sidebar" | "list";
  /** Variabile CSS sull'elemento `.app-shell` (es. `--sidebar-width`). */
  variable: string;
  label: string;
  value: number;
  min: number;
  /** Massimo calcolato al momento (dipende dalla finestra). */
  max: () => number;
  defaultValue: number;
  onCommit: (width: number) => void;
}

/**
 * Bordo trascinabile tra due colonne. Durante il trascinamento aggiorna solo
 * la variabile CSS (nessun nuovo rendering); al rilascio salva la larghezza.
 * Da tastiera: frecce (±16 px, con Maiusc ±64), Home/Fine; doppio clic
 * ripristina la larghezza predefinita.
 */
export default function ColumnResizer({
  edge,
  variable,
  label,
  value,
  min,
  max,
  defaultValue,
  onCommit,
}: ColumnResizerProps) {
  const dragging = useRef(false);
  const shell = (element: Element) => element.closest<HTMLElement>(".app-shell");
  const clamp = (width: number) => Math.round(Math.min(Math.max(min, max()), Math.max(min, width)));

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    const target = event.currentTarget;
    const container = shell(target);
    if (!container) return;
    const startX = event.clientX;
    const startWidth = value;
    let width = startWidth;
    dragging.current = true;
    target.setPointerCapture(event.pointerId);
    container.classList.add("resizing-columns");
    const move = (moveEvent: globalThis.PointerEvent) => {
      width = clamp(startWidth + moveEvent.clientX - startX);
      container.style.setProperty(variable, `${width}px`);
      // Anche la posizione del bordo della barra laterale segue il mouse.
      if (edge === "sidebar") container.style.setProperty("--sidebar-column", `${width}px`);
    };
    // Fine del trascinamento anche se il puntatore viene perso (rilascio
    // fuori dalla finestra, cattura annullata): la pagina non resta mai in
    // stato di ridimensionamento.
    const finish = (endEvent: globalThis.PointerEvent) => {
      if (!dragging.current) return;
      dragging.current = false;
      try {
        if (target.hasPointerCapture(endEvent.pointerId)) {
          target.releasePointerCapture(endEvent.pointerId);
        }
      } catch {
        // La cattura era già stata rilasciata.
      }
      target.removeEventListener("pointermove", move);
      target.removeEventListener("pointerup", finish);
      target.removeEventListener("pointercancel", finish);
      target.removeEventListener("lostpointercapture", finish);
      container.classList.remove("resizing-columns");
      target.blur();
      if (width !== startWidth) onCommit(width);
    };
    target.addEventListener("pointermove", move);
    target.addEventListener("pointerup", finish);
    target.addEventListener("pointercancel", finish);
    target.addEventListener("lostpointercapture", finish);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = event.shiftKey ? 64 : 16;
    const next =
      event.key === "ArrowLeft"
        ? value - step
        : event.key === "ArrowRight"
          ? value + step
          : event.key === "Home"
            ? min
            : event.key === "End"
              ? max()
              : null;
    if (next === null) return;
    event.preventDefault();
    onCommit(clamp(next));
  };

  return (
    <div
      aria-label={label}
      aria-orientation="vertical"
      aria-valuemax={max()}
      aria-valuemin={min}
      aria-valuenow={value}
      className={`column-resizer ${edge}-edge`}
      onDoubleClick={(event) => {
        event.currentTarget.blur();
        onCommit(clamp(defaultValue));
      }}
      // Il clic del mouse non dà il focus (la linea resta solo con la tastiera).
      onMouseDown={(event) => event.preventDefault()}
      onKeyDown={onKeyDown}
      onPointerDown={onPointerDown}
      role="separator"
      tabIndex={0}
      title={label}
    />
  );
}
