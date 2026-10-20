/**
 * Conferme dell'app al posto di `window.confirm`, che in WebKitGTK/Tauri non
 * è affidabile (può non comparire o rispondere subito). Una sola richiesta
 * alla volta, mostrata da `ConfirmHost`.
 */

export interface ConfirmOptions {
  title: string;
  message: string;
  confirmLabel: string;
  cancelLabel?: string;
  /** Azione irreversibile: pulsante rosso e focus iniziale su Annulla. */
  destructive?: boolean;
  /** Risposta se il dialog viene chiuso con Esc o clic sullo sfondo. */
  dismissValue?: boolean;
}

export interface ConfirmRequest extends ConfirmOptions {
  resolve: (value: boolean) => void;
}

type Listener = (request: ConfirmRequest | null) => void;

let listener: Listener | null = null;
let current: ConfirmRequest | null = null;

export function subscribeConfirm(next: Listener): () => void {
  listener = next;
  next(current);
  return () => {
    if (listener === next) listener = null;
  };
}

export function askConfirmation(options: ConfirmOptions): Promise<boolean> {
  // Una richiesta ancora aperta viene chiusa con la sua risposta prudente.
  current?.resolve(current.dismissValue ?? false);
  return new Promise((resolve) => {
    const request: ConfirmRequest = {
      ...options,
      resolve: (value) => {
        if (current === request) {
          current = null;
          listener?.(null);
        }
        resolve(value);
      },
    };
    current = request;
    if (listener) {
      listener(request);
    } else {
      // Nessun host montato: meglio non eseguire l'azione.
      request.resolve(false);
    }
  });
}
