/**
 * Modello dei menu (barra dei menu e menu contestuali) e gestione delle
 * scorciatoie da tastiera. Ogni voce ha un'azione reale: una voce senza
 * azione disponibile viene mostrata disattivata.
 */

export interface MenuAction {
  kind?: "action";
  id: string;
  label: string;
  /** Es. "Ctrl+Shift+N", "Alt+Left", "F11", "Del". */
  accelerator?: string;
  /** Scorciatoie aggiuntive non mostrate (es. Ctrl+F oltre a Ctrl+K). */
  aliases?: string[];
  /**
   * La scorciatoia è gestita nativamente dal campo attivo (annulla, copia…):
   * viene mostrata nel menu ma non intercettata globalmente.
   */
  native?: boolean;
  enabled?: boolean;
  checked?: boolean;
  /** Voce a scelta singola (es. tema): mostra un pallino invece della spunta. */
  radio?: boolean;
  destructive?: boolean;
  run: () => void | Promise<void>;
}

export interface MenuSubmenu {
  kind: "submenu";
  id: string;
  label: string;
  enabled?: boolean;
  items: MenuEntry[];
}

export interface MenuSeparator {
  kind: "separator";
}

export type MenuEntry = MenuAction | MenuSubmenu | MenuSeparator;

export interface Menu {
  id: string;
  label: string;
  items: MenuEntry[];
}

export const separator: MenuSeparator = { kind: "separator" };

const isMac = navigator.userAgent.includes("Mac");

interface ParsedAccelerator {
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
  key: string;
}

const KEY_ALIASES: Record<string, string> = {
  plus: "+",
  "=": "+",
  minus: "-",
  comma: ",",
  left: "arrowleft",
  right: "arrowright",
  up: "arrowup",
  down: "arrowdown",
  del: "delete",
  esc: "escape",
};

function normalizeKey(key: string): string {
  const lower = key.toLowerCase();
  return KEY_ALIASES[lower] ?? lower;
}

export function parseAccelerator(accelerator: string): ParsedAccelerator {
  // "Ctrl++" significa Ctrl e il tasto "+".
  const parts = accelerator.endsWith("++")
    ? [...accelerator.slice(0, -2).split("+"), "+"]
    : accelerator.split("+");
  const key = parts.pop() ?? "";
  const modifiers = parts.map((part) => part.toLowerCase());
  return {
    ctrl: modifiers.includes("ctrl") || modifiers.includes("cmd"),
    alt: modifiers.includes("alt"),
    shift: modifiers.includes("shift"),
    key: normalizeKey(key),
  };
}

function eventKey(event: KeyboardEvent): string {
  // Con Alt o Shift `event.key` cambia secondo il layout: per lettere e
  // cifre si usa il tasto fisico.
  if (/^Key[A-Z]$/.test(event.code)) return event.code.slice(3).toLowerCase();
  if (/^Digit[0-9]$/.test(event.code)) return event.code.slice(5);
  if (event.code === "Equal" || event.code === "NumpadAdd") return "+";
  if (event.code === "Minus" || event.code === "NumpadSubtract") return "-";
  if (event.code === "Comma") return ",";
  if (event.code === "Slash") return "/";
  return normalizeKey(event.key);
}

export function matchesAccelerator(event: KeyboardEvent, accelerator: string): boolean {
  const parsed = parseAccelerator(accelerator);
  const ctrl = isMac ? event.metaKey : event.ctrlKey;
  // Per "+" lo Shift fa parte del tasto su molte tastiere: si ignora.
  const shiftMatches = parsed.key === "+" || parsed.shift === event.shiftKey;
  return (
    parsed.ctrl === ctrl &&
    parsed.alt === event.altKey &&
    shiftMatches &&
    parsed.key === eventKey(event)
  );
}

/** Testo della scorciatoia per il menu, adattato alla piattaforma. */
export function displayAccelerator(accelerator: string): string {
  return accelerator
    .split("+")
    .map((part) =>
      part === "Ctrl" && isMac
        ? "⌘"
        : part === "Left"
          ? "←"
          : part === "Right"
            ? "→"
            : part,
    )
    .join("+")
    .replace("++", "+ +");
}

function* actions(entries: MenuEntry[]): Generator<MenuAction> {
  for (const entry of entries) {
    if (entry.kind === "separator") continue;
    if (entry.kind === "submenu") {
      if (entry.enabled !== false) yield* actions(entry.items);
      continue;
    }
    yield entry;
  }
}

/** Azione associata alla combinazione di tasti, se c'è. */
export function findAction(menus: Menu[], event: KeyboardEvent): MenuAction | null {
  for (const menu of menus) {
    for (const action of actions(menu.items)) {
      if (action.native) continue;
      const accelerators = [action.accelerator, ...(action.aliases ?? [])].filter(
        (value): value is string => Boolean(value),
      );
      if (accelerators.some((accelerator) => matchesAccelerator(event, accelerator))) {
        return action;
      }
    }
  }
  return null;
}
