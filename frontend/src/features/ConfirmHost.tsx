import { useEffect, useRef, useState } from "react";
import { t } from "../i18n";
import { subscribeConfirm } from "../lib/confirm";
import type { ConfirmRequest } from "../lib/confirm";

/** Dialog di conferma modale (`alertdialog`), montato una volta nell'app. */
export default function ConfirmHost() {
  const [request, setRequest] = useState<ConfirmRequest | null>(null);
  const cancelButton = useRef<HTMLButtonElement>(null);
  const confirmButton = useRef<HTMLButtonElement>(null);

  useEffect(() => subscribeConfirm(setRequest), []);

  useEffect(() => {
    if (!request) return;
    const previous = document.activeElement as HTMLElement | null;
    // Per le azioni irreversibili il focus parte da Annulla: un Invio
    // involontario non elimina nulla.
    (request.destructive ? cancelButton : confirmButton).current?.focus();
    return () => previous?.focus();
  }, [request]);

  if (!request) return null;
  const dismiss = () => request.resolve(request.dismissValue ?? false);

  return (
    <div
      className="dialog-backdrop confirm-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) dismiss();
      }}
    >
      <section
        aria-describedby="confirm-dialog-message"
        aria-labelledby="confirm-dialog-title"
        aria-modal="true"
        className="organization-dialog confirm-dialog"
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            event.stopPropagation();
            dismiss();
          } else if (event.key === "Tab") {
            // Il focus resta fra i due pulsanti del dialog.
            event.preventDefault();
            const next =
              document.activeElement === cancelButton.current ? confirmButton : cancelButton;
            next.current?.focus();
          }
        }}
        role="alertdialog"
      >
        <header className="confirm-header">
          <div className={`dialog-mark${request.destructive ? " destructive-mark" : ""}`} aria-hidden="true">
            {request.destructive ? "!" : "?"}
          </div>
          <h2 id="confirm-dialog-title">{request.title}</h2>
        </header>
        <p className="confirm-message" id="confirm-dialog-message">
          {request.message}
        </p>
        <footer className="organization-dialog-actions">
          <button
            className="dialog-cancel-button"
            onClick={() => request.resolve(false)}
            ref={cancelButton}
            type="button"
          >
            {request.cancelLabel ?? t.cancel}
          </button>
          <button
            className={`dialog-submit-button${request.destructive ? " destructive-submit" : ""}`}
            onClick={() => request.resolve(true)}
            ref={confirmButton}
            type="button"
          >
            {request.confirmLabel}
          </button>
        </footer>
      </section>
    </div>
  );
}
