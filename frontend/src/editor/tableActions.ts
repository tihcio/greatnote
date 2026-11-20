import type { Editor } from "@tiptap/react";
import { CellSelection } from "@tiptap/pm/tables";
import { t } from "../i18n";

export type TableActionId =
  | "row-before"
  | "row-after"
  | "col-before"
  | "col-after"
  | "row-delete"
  | "col-delete"
  | "merge"
  | "split"
  | "clear"
  | "header-row"
  | "header-col"
  | "table-delete";

export interface TableAction {
  id: TableActionId;
  label: string;
  enabled: boolean;
  checked?: boolean;
  destructive?: boolean;
  run: () => void;
}

/** Svuota le celle selezionate (o quella col cursore) lasciando la struttura. */
function clearCells(editor: Editor): boolean {
  return editor
    .chain()
    .focus()
    .command(({ state, tr, dispatch }) => {
      const { selection } = state;
      const cells: { pos: number; size: number }[] = [];
      if (selection instanceof CellSelection) {
        selection.forEachCell((node, pos) => cells.push({ pos, size: node.nodeSize }));
      } else {
        const { $from } = selection;
        for (let depth = $from.depth; depth > 0; depth -= 1) {
          const node = $from.node(depth);
          if (node.type.name === "tableCell" || node.type.name === "tableHeader") {
            cells.push({ pos: $from.before(depth), size: node.nodeSize });
            break;
          }
        }
      }
      if (cells.length === 0) return false;
      if (dispatch) {
        const paragraph = state.schema.nodes.paragraph;
        // Dall'ultima alla prima: le posizioni precedenti restano valide.
        for (const cell of cells.sort((a, b) => b.pos - a.pos)) {
          tr.replaceWith(cell.pos + 1, cell.pos + cell.size - 1, paragraph.create());
        }
      }
      return true;
    })
    .run();
}

/** Prima riga o prima colonna della tabella corrente già d'intestazione. */
function headerActive(editor: Editor, kind: "row" | "col"): boolean {
  const { $from } = editor.state.selection;
  for (let depth = $from.depth; depth > 0; depth -= 1) {
    const table = $from.node(depth);
    if (table.type.name !== "table") continue;
    const firstRow = table.firstChild;
    if (!firstRow) return false;
    if (kind === "row") {
      let all = firstRow.childCount > 0;
      firstRow.forEach((cell) => {
        if (cell.type.name !== "tableHeader") all = false;
      });
      return all;
    }
    let all = table.childCount > 0;
    table.forEach((row) => {
      if (row.firstChild?.type.name !== "tableHeader") all = false;
    });
    return all;
  }
  return false;
}

/**
 * Comandi sulla tabella in cui si trova il cursore: gli stessi nella barra
 * della tabella, nel menu contestuale e nel menu «Altro» della barra.
 */
export function tableActions(editor: Editor): TableAction[] {
  const editable = editor.isEditable && editor.isActive("table");
  const can = editor.can();
  const chain = () => editor.chain().focus();
  return [
    { id: "row-before", label: t.tableRowBefore, enabled: editable, run: () => void chain().addRowBefore().run() },
    { id: "row-after", label: t.tableRowAfter, enabled: editable, run: () => void chain().addRowAfter().run() },
    { id: "col-before", label: t.tableColumnBefore, enabled: editable, run: () => void chain().addColumnBefore().run() },
    { id: "col-after", label: t.tableColumnAfter, enabled: editable, run: () => void chain().addColumnAfter().run() },
    { id: "row-delete", label: t.tableDeleteRow, enabled: editable, run: () => void chain().deleteRow().run() },
    { id: "col-delete", label: t.tableDeleteColumn, enabled: editable, run: () => void chain().deleteColumn().run() },
    { id: "merge", label: t.tableMergeCells, enabled: editable && can.mergeCells(), run: () => void chain().mergeCells().run() },
    { id: "split", label: t.tableSplitCell, enabled: editable && can.splitCell(), run: () => void chain().splitCell().run() },
    { id: "clear", label: t.tableClearCells, enabled: editable, run: () => void clearCells(editor) },
    {
      id: "header-row",
      label: t.tableHeaderRow,
      enabled: editable,
      checked: editable && headerActive(editor, "row"),
      run: () => void chain().toggleHeaderRow().run(),
    },
    {
      id: "header-col",
      label: t.tableHeaderColumn,
      enabled: editable,
      checked: editable && headerActive(editor, "col"),
      run: () => void chain().toggleHeaderColumn().run(),
    },
    {
      id: "table-delete",
      label: t.tableDelete,
      enabled: editable,
      destructive: true,
      run: () => void chain().deleteTable().run(),
    },
  ];
}

/** Limiti del dialog d'inserimento: tabelle più grandi restano possibili
 * aggiungendo righe e colonne. */
export const MAX_TABLE_ROWS = 100;
export const MAX_TABLE_COLUMNS = 20;
