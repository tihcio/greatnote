/**
 * Scelta di righe e colonne di una nuova tabella con un dialog dell'app.
 * Una richiesta alla volta, come `askText`.
 */

export interface TableSize {
  rows: number;
  cols: number;
  withHeaderRow: boolean;
}

type Listener = (request: TableSizeRequest | null) => void;

export interface TableSizeRequest {
  resolve: (size: TableSize | null) => void;
}

let listener: Listener | null = null;
let current: TableSizeRequest | null = null;

export function subscribeTableSize(next: Listener): () => void {
  listener = next;
  next(current);
  return () => {
    if (listener === next) listener = null;
  };
}

export function askTableSize(): Promise<TableSize | null> {
  current?.resolve(null);
  return new Promise((resolve) => {
    const request: TableSizeRequest = {
      resolve: (size) => {
        if (current === request) {
          current = null;
          listener?.(null);
        }
        resolve(size);
      },
    };
    current = request;
    if (listener) listener(request);
    else request.resolve(null);
  });
}

/** Valore intero tra 1 e `max`, o `null` se non è un numero valido. */
export function clampSize(value: string, max: number): number | null {
  if (!/^\d+$/.test(value.trim())) return null;
  const parsed = Number(value);
  if (parsed < 1) return null;
  return Math.min(parsed, max);
}
