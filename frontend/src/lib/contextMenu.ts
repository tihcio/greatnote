/**
 * Menu contestuali (tasto destro, tasto Menu o Shift+F10) aperti da
 * qualsiasi componente, mostrati da `ContextMenuHost`. Un menu alla volta.
 *
 * Il gestore chiama `openContextMenu` dal proprio `onContextMenu`; l'host
 * blocca il menu predefinito della webview ovunque e, sui campi di testo
 * senza un menu proprio, mostra quello standard (taglia, copia, incolla).
 */
import type { MenuEntry } from "../app/menu";

export interface ContextMenuRequest {
  x: number;
  y: number;
  label: string;
  items: MenuEntry[];
}

type Listener = (request: ContextMenuRequest | null) => void;

let listener: Listener | null = null;

export function subscribeContextMenu(next: Listener): () => void {
  listener = next;
  return () => {
    if (listener === next) listener = null;
  };
}

interface PointerLike {
  clientX: number;
  clientY: number;
  target?: EventTarget | null;
  preventDefault?: () => void;
  stopPropagation?: () => void;
}

/**
 * Apre il menu sul puntatore. Aperto da tastiera (coordinate 0,0) il menu
 * compare sotto l'elemento attivo. Voci vuote: nessun menu.
 */
export function openContextMenu(event: PointerLike, label: string, items: MenuEntry[]) {
  event.preventDefault?.();
  event.stopPropagation?.();
  if (items.length === 0 || !listener) return;
  let { clientX: x, clientY: y } = event;
  if (x === 0 && y === 0 && event.target instanceof Element) {
    const rect = event.target.getBoundingClientRect();
    x = rect.left + Math.min(24, rect.width / 2);
    y = rect.bottom;
  }
  listener({ x, y, label, items: trimSeparators(items) });
}

export function closeContextMenu() {
  listener?.(null);
}

/** Toglie i separatori iniziali, finali e doppi (voci condizionali). */
export function trimSeparators(items: MenuEntry[]): MenuEntry[] {
  const result: MenuEntry[] = [];
  for (const item of items) {
    if (item.kind === "separator" && (result.length === 0 || result[result.length - 1].kind === "separator")) {
      continue;
    }
    result.push(item);
  }
  while (result.length > 0 && result[result.length - 1].kind === "separator") result.pop();
  return result;
}

type TextField = HTMLInputElement | HTMLTextAreaElement;

const TEXT_INPUT_TYPES = new Set(["text", "search", "url", "email", "tel", "password", "number", ""]);

/** Campo di testo modificabile sotto il puntatore, se c'è. */
export function textFieldAt(target: EventTarget | null): TextField | null {
  if (target instanceof HTMLTextAreaElement) return target;
  if (target instanceof HTMLInputElement && TEXT_INPUT_TYPES.has(target.type)) return target;
  return null;
}

/** Inserisce testo al cursore del campo, come una digitazione. */
export function insertIntoField(field: TextField, text: string) {
  const start = field.selectionStart ?? field.value.length;
  const end = field.selectionEnd ?? start;
  const value = field instanceof HTMLInputElement ? text.replace(/\s*\n\s*/g, " ") : text;
  field.setRangeText(value, start, end, "end");
  field.dispatchEvent(new Event("input", { bubbles: true }));
}
