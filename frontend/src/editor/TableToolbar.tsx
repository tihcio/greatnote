import { useEffect, useLayoutEffect, useReducer, useRef, useState } from "react";
import type { Editor } from "@tiptap/react";
import { t } from "../i18n";
import { tableActions } from "./tableActions";
import type { TableAction, TableActionId } from "./tableActions";

/** Disegni 16×16 dei comandi: la riga o colonna evidenziata è quella che
 * il comando aggiunge o toglie. */
const glyphs: Record<TableActionId, string> = {
  "row-before": "M2 7h12v7H2zM2 10.5h12M8 7v7|M8 1v4M6 3h4",
  "row-after": "M2 2h12v7H2zM2 5.5h12M8 2v7|M8 11v4M6 13h4",
  "col-before": "M7 2h7v12H7zM10.5 2v12M7 8h7|M1 8h4M3 6v4",
  "col-after": "M2 2h7v12H2zM5.5 2v12M2 8h7|M11 8h4M13 6v4",
  "row-delete": "M2 2h12v12H2zM2 6h12M2 10h12|M5.5 6.5l5 3M10.5 6.5l-5 3",
  "col-delete": "M2 2h12v12H2zM6 2v12M10 2v12|M6.5 5.5l3 5M9.5 5.5l-3 5",
  merge: "M2 3h12v10H2z|M4.5 8h7M9.5 6l2 2-2 2M6.5 6l-2 2 2 2",
  split: "M2 3h12v10H2zM8 3v10|M3.5 8h3M5 6.5 3.5 8 5 9.5M12.5 8h-3M11 6.5l1.5 1.5L11 9.5",
  clear: "M2 2h12v12H2zM8 2v12M2 8h12|M10 10l3 3M13 10l-3 3",
  "header-row": "M2 2h12v12H2zM2 6h12M8 6v8|M2 2h12v4H2z",
  "header-col": "M2 2h12v12H2zM6 2v12M6 8h8|M2 2h4v12H2z",
  "table-delete": "M3 4h10M6 4V2.5h4V4M4.5 4l.7 9.5h5.6l.7-9.5M7 6.5v5M9 6.5v5",
};

/** Gruppi di pulsanti separati da una linea. */
const groups: TableActionId[][] = [
  ["row-before", "row-after", "col-before", "col-after"],
  ["row-delete", "col-delete"],
  ["merge", "split", "clear"],
  ["header-row", "header-col"],
  ["table-delete"],
];

function Glyph({ id }: { id: TableActionId }) {
  const [outline, accent] = glyphs[id].split("|");
  return (
    <svg aria-hidden="true" fill="none" height="16" viewBox="0 0 16 16" width="16">
      <path d={outline} stroke="currentColor" strokeLinejoin="round" strokeWidth="1.2" />
      {accent && (
        <path
          className="table-toolbar-accent"
          d={accent}
          stroke="currentColor"
          strokeLinecap="round"
          strokeLinejoin="round"
          strokeWidth="1.4"
        />
      )}
    </svg>
  );
}

/** Tabella che contiene il cursore, come elemento del DOM dell'editor. */
function activeTable(editor: Editor): HTMLTableElement | null {
  const { $from } = editor.state.selection;
  for (let depth = $from.depth; depth > 0; depth -= 1) {
    if ($from.node(depth).type.name !== "table") continue;
    const dom = editor.view.nodeDOM($from.before(depth));
    if (dom instanceof HTMLTableElement) return dom;
    if (dom instanceof HTMLElement) return dom.querySelector("table");
    return null;
  }
  return null;
}

/**
 * Barra dei comandi mostrata sopra la tabella in cui si scrive: aggiunge o
 * toglie righe e colonne, unisce e divide celle, imposta le intestazioni.
 */
export default function TableToolbar({ editor }: { editor: Editor }) {
  const [, refresh] = useReducer((value: number) => value + 1, 0);
  const [focused, setFocused] = useState(() => editor.isFocused);
  const [position, setPosition] = useState<{ top: number; left: number } | null>(null);
  const bar = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onFocus = () => setFocused(true);
    const onBlur = ({ event }: { event: FocusEvent }) => {
      // Il focus passato a un pulsante della barra non la nasconde.
      if (bar.current && event.relatedTarget instanceof Node && bar.current.contains(event.relatedTarget)) return;
      setFocused(false);
    };
    editor.on("transaction", refresh);
    editor.on("focus", onFocus);
    editor.on("blur", onBlur);
    return () => {
      editor.off("transaction", refresh);
      editor.off("focus", onFocus);
      editor.off("blur", onBlur);
    };
  }, [editor]);

  const visible = focused && editor.isEditable && editor.isActive("table");

  useLayoutEffect(() => {
    // Aggiorna solo se cambia: l'effetto gira a ogni rendering.
    const update = (next: { top: number; left: number } | null) =>
      setPosition((previous) =>
        previous === next || (previous && next && previous.top === next.top && previous.left === next.left)
          ? previous
          : next,
      );
    if (!visible) {
      update(null);
      return;
    }
    const place = () => {
      const table = activeTable(editor);
      if (!table) {
        update(null);
        return;
      }
      const rect = table.getBoundingClientRect();
      const scroller = editor.view.dom.closest(".editor-workspace") ?? editor.view.dom;
      const bounds = scroller.getBoundingClientRect();
      const height = bar.current?.offsetHeight ?? 36;
      const width = bar.current?.offsetWidth ?? 0;
      // Sopra la tabella; se lo spazio manca resta in cima all'area visibile
      // finché la tabella è sullo schermo.
      const top = Math.max(rect.top - height - 6, Math.max(bounds.top, 0) + 4);
      if (top > rect.bottom - height || rect.bottom < bounds.top) {
        update(null);
        return;
      }
      const maxLeft = window.innerWidth - width - 8;
      update({ top: Math.round(top), left: Math.round(Math.max(8, Math.min(rect.left, maxLeft))) });
    };
    place();
    window.addEventListener("scroll", place, true);
    window.addEventListener("resize", place);
    return () => {
      window.removeEventListener("scroll", place, true);
      window.removeEventListener("resize", place);
    };
  });

  if (!visible) return null;
  const actions = new Map<TableActionId, TableAction>(tableActions(editor).map((action) => [action.id, action]));

  return (
    <div
      aria-label={t.tableToolbarLabel}
      className="table-toolbar"
      ref={bar}
      role="toolbar"
      style={position ? { top: position.top, left: position.left } : { visibility: "hidden" }}
    >
      {groups.map((group, index) => (
        <div className="table-toolbar-group" key={index}>
          {group.map((id) => {
            const action = actions.get(id);
            if (!action) return null;
            return (
              <button
                aria-label={action.label}
                aria-pressed={action.checked === undefined ? undefined : action.checked}
                className={`table-toolbar-button${action.destructive ? " destructive" : ""}${action.checked ? " active" : ""}`}
                disabled={!action.enabled}
                key={id}
                onClick={action.run}
                // Il clic non toglie il focus all'editor né la selezione delle celle.
                onMouseDown={(event) => event.preventDefault()}
                title={action.label}
                type="button"
              >
                <Glyph id={id} />
              </button>
            );
          })}
        </div>
      ))}
    </div>
  );
}
