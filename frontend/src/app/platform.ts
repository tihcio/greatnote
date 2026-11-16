// Piattaforma su cui gira l'interfaccia. Su Android e iOS l'app usa una
// disposizione a una colonna pensata per il touch (docs/ANDROID_PORTING.md).
//
// Il riconoscimento usa lo user agent della webview perché serve prima del
// primo disegno, quando il backend non ha ancora risposto. `?mobile` forza la
// disposizione mobile nel banco di prova dell'interfaccia.

import type { MenuEntry } from "./menu";

const agent = navigator.userAgent;

export const isMobile =
  /Android|iPhone|iPad/i.test(agent) || new URLSearchParams(window.location.search).has("mobile");

if (isMobile) {
  document.documentElement.dataset.platform = "mobile";
  watchKeyboard();
}

/**
 * Segnala al CSS (`data-keyboard="open"`) quando la tastiera virtuale è
 * aperta, così la nota può cedere spazio alla barra della formattazione.
 * La webview non espone lo stato della tastiera: si deduce dall'altezza
 * della finestra, che su Android si riduce quando la tastiera compare.
 */
function watchKeyboard() {
  let width = window.innerWidth;
  let tallest = window.innerHeight;
  const update = () => {
    // Rotazione: l'altezza di riferimento va misurata di nuovo.
    if (window.innerWidth !== width) {
      width = window.innerWidth;
      tallest = window.innerHeight;
    }
    tallest = Math.max(tallest, window.innerHeight);
    const open = window.innerHeight < tallest * 0.78;
    if (open) document.documentElement.dataset.keyboard = "open";
    else delete document.documentElement.dataset.keyboard;
  };
  window.addEventListener("resize", update);
}

/**
 * Voci di menu senza un equivalente su mobile: finestre e schede (c'è una
 * sola schermata) e stampa (la webview non ha un dialog di stampa).
 */
const DESKTOP_ONLY_ACTIONS = new Set([
  "open-window",
  "open-tab",
  "ctx-open-window",
  "ctx-open-tab",
  "ctx-pdf",
  "ctx-print",
  "export-pdf",
  "print",
]);

/** Toglie le voci non disponibili sulla piattaforma e i separatori rimasti soli. */
export function availableEntries(entries: MenuEntry[]): MenuEntry[] {
  if (!isMobile) return entries;
  const kept: MenuEntry[] = [];
  for (const entry of entries) {
    if (entry.kind === "separator") {
      if (kept.length > 0 && kept[kept.length - 1].kind !== "separator") kept.push(entry);
    } else if (!DESKTOP_ONLY_ACTIONS.has(entry.id)) {
      kept.push(entry.kind === "submenu" ? { ...entry, items: availableEntries(entry.items) } : entry);
    }
  }
  while (kept.length > 0 && kept[kept.length - 1].kind === "separator") kept.pop();
  return kept;
}
