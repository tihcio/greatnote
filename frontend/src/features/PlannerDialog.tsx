import { useEffect, useRef } from "react";
import type { ReactNode } from "react";
import { t } from "../i18n";

interface PlannerDialogProps {
  title: string;
  mark: string;
  busy?: boolean;
  onClose: () => void;
  children: ReactNode;
}

/** Dialog modale accessibile: Esc e clic sullo sfondo chiudono, il focus va
 * al primo campo e torna all'elemento precedente alla chiusura. */
export default function PlannerDialog({ title, mark, busy, onClose, children }: PlannerDialogProps) {
  const dialog = useRef<HTMLElement>(null);
  const titleId = useRef(`planner-dialog-${Math.random().toString(36).slice(2)}`);

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const first =
      dialog.current?.querySelector<HTMLElement>("input, textarea, select") ??
      dialog.current?.querySelector<HTMLElement>("button");
    first?.focus();
    return () => previous?.focus();
  }, []);

  const close = () => {
    if (!busy) onClose();
  };

  return (
    <div
      className="dialog-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) close();
      }}
    >
      <section
        aria-busy={busy}
        aria-labelledby={titleId.current}
        aria-modal="true"
        className="organization-dialog planner-dialog"
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            event.stopPropagation();
            close();
          }
        }}
        ref={dialog}
        role="dialog"
      >
        <header className="organization-dialog-header">
          <div className="dialog-mark" aria-hidden="true">
            {mark}
          </div>
          <div>
            <h2 id={titleId.current}>{title}</h2>
          </div>
          <button
            aria-label={t.close}
            className="dialog-close-button"
            disabled={busy}
            onClick={close}
            type="button"
          >
            ×
          </button>
        </header>
        {children}
      </section>
    </div>
  );
}
