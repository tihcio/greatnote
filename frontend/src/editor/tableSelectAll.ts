import { Extension } from "@tiptap/react";
import { CellSelection, TableMap } from "@tiptap/pm/tables";

/**
 * Ctrl/Cmd+A con il cursore in una tabella seleziona tutte le sue celle; un
 * secondo Ctrl/Cmd+A seleziona l'intera nota, come sempre.
 */
export const TableSelectAll = Extension.create({
  name: "tableSelectAll",
  addKeyboardShortcuts() {
    return {
      "Mod-a": ({ editor }) => {
        const { state, view } = editor;
        const { $from } = state.selection;
        for (let depth = $from.depth; depth > 0; depth -= 1) {
          const table = $from.node(depth);
          if (table.type.name !== "table") continue;
          const map = TableMap.get(table);
          const start = $from.start(depth);
          const first = start + map.map[0];
          const last = start + map.map[map.map.length - 1];
          const selection = state.selection;
          const whole =
            selection instanceof CellSelection &&
            Math.min(selection.$anchorCell.pos, selection.$headCell.pos) === first &&
            Math.max(selection.$anchorCell.pos, selection.$headCell.pos) === last;
          if (whole) return false;
          view.dispatch(state.tr.setSelection(CellSelection.create(state.doc, first, last)));
          return true;
        }
        return false;
      },
    };
  },
});
