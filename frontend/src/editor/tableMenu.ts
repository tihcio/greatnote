import type { Editor } from "@tiptap/react";
import { separator } from "../app/menu";
import type { MenuEntry } from "../app/menu";
import { t } from "../i18n";
import { tableActions } from "./tableActions";
import type { TableAction, TableActionId } from "./tableActions";

const entry = (action: TableAction, prefix: string): MenuEntry => ({
  id: `${prefix}${action.id}`,
  label: action.label,
  enabled: action.enabled,
  checked: action.checked,
  destructive: action.destructive,
  run: action.run,
});

const sections: TableActionId[][] = [
  ["row-before", "row-after", "col-before", "col-after"],
  ["row-delete", "col-delete"],
  ["merge", "split", "clear"],
  ["header-row", "header-col"],
  ["table-delete"],
];

/** Tutti i comandi della tabella, in sezioni (sottomenu «Tabella»). */
export function tableMenuEntries(editor: Editor, prefix = "table-"): MenuEntry[] {
  const actions = new Map(tableActions(editor).map((action) => [action.id, action]));
  const result: MenuEntry[] = [];
  for (const section of sections) {
    if (result.length > 0) result.push(separator);
    for (const id of section) {
      const action = actions.get(id);
      if (action) result.push(entry(action, prefix));
    }
  }
  return result;
}

/**
 * Voci del menu contestuale con il clic destro in una tabella: righe e
 * colonne subito raggiungibili, gli altri comandi nel sottomenu.
 */
export function tableContextEntries(editor: Editor): MenuEntry[] {
  const actions = new Map(tableActions(editor).map((action) => [action.id, action]));
  const direct: TableActionId[] = ["row-before", "row-after", "col-before", "col-after", "row-delete", "col-delete"];
  const result: MenuEntry[] = [];
  for (const id of direct) {
    const action = actions.get(id);
    if (!action) continue;
    if (id === "row-delete") result.push(separator);
    result.push(entry(action, "ed-table-"));
  }
  for (const id of ["merge", "split"] as const) {
    const action = actions.get(id);
    // Unisci e dividi compaiono solo quando sono possibili.
    if (action?.enabled) result.push(entry(action, "ed-table-"));
  }
  result.push({ kind: "submenu", id: "ed-table", label: t.tableMenu, items: tableMenuEntries(editor, "ed-table-all-") });
  return result;
}
