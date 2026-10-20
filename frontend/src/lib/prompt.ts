/**
 * Richiesta di un testo con un dialog dell'app (al posto di
 * `window.prompt`, non affidabile in WebKitGTK). Una richiesta alla volta.
 */

export interface PromptOptions {
  title: string;
  label: string;
  initialValue?: string;
  placeholder?: string;
  confirmLabel: string;
  maxLength?: number;
  /** Restituisce un messaggio d'errore se il valore non è accettabile. */
  validate?: (value: string) => string | null;
}

export interface PromptRequest extends PromptOptions {
  resolve: (value: string | null) => void;
}

type Listener = (request: PromptRequest | null) => void;

let listener: Listener | null = null;
let current: PromptRequest | null = null;

export function subscribePrompt(next: Listener): () => void {
  listener = next;
  next(current);
  return () => {
    if (listener === next) listener = null;
  };
}

export function askText(options: PromptOptions): Promise<string | null> {
  current?.resolve(null);
  return new Promise((resolve) => {
    const request: PromptRequest = {
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
    if (listener) listener(request);
    else request.resolve(null);
  });
}
