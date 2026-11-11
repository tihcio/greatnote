import { Extension, Mark } from "@tiptap/react";
import type { Editor } from "@tiptap/react";

/** Devono coincidere con `TEXT_ALIGNMENTS` e `MAX_INDENT` in src/domain/note.rs. */
export const TEXT_ALIGNMENTS = ["center", "right"] as const;
export type TextAlignment = (typeof TEXT_ALIGNMENTS)[number];
export const MAX_INDENT = 8;
/** Larghezza di un livello di rientro, la stessa dell'esportazione HTML. */
const INDENT_STEP_EM = 2;
const LAYOUT_NODES = ["paragraph", "heading"];

export function isTextAlignment(value: unknown): value is TextAlignment {
  return TEXT_ALIGNMENTS.includes(value as TextAlignment);
}

export function indentLevel(value: unknown): number | null {
  return typeof value === "number" && Number.isInteger(value) && value >= 1 && value <= MAX_INDENT
    ? value
    : null;
}

/**
 * Allineamento e rientro di paragrafi e titoli. A sinistra e senza rientro
 * sono il valore predefinito (`null`), così le note esistenti non cambiano.
 */
export const TextLayout = Extension.create({
  name: "textLayout",
  addGlobalAttributes() {
    return [
      {
        types: LAYOUT_NODES,
        attributes: {
          textAlign: {
            default: null,
            parseHTML: (element) => {
              const value = element.style.textAlign || element.getAttribute("align");
              return isTextAlignment(value) ? value : null;
            },
            renderHTML: (attributes) =>
              isTextAlignment(attributes.textAlign)
                ? { style: `text-align: ${attributes.textAlign}` }
                : {},
          },
          indent: {
            default: null,
            parseHTML: (element) => {
              const margin = /^(\d+(?:\.\d+)?)em$/.exec(element.style.marginLeft);
              return margin ? indentLevel(Math.round(Number(margin[1]) / INDENT_STEP_EM)) : null;
            },
            renderHTML: (attributes) => {
              const level = indentLevel(attributes.indent);
              return level ? { style: `margin-left: ${level * INDENT_STEP_EM}em` } : {};
            },
          },
        },
      },
    ];
  },
});

export const Superscript = Mark.create({
  name: "superscript",
  excludes: "superscript subscript",
  parseHTML: () => [{ tag: "sup" }],
  renderHTML: ({ HTMLAttributes }) => ["sup", HTMLAttributes, 0],
});

export const Subscript = Mark.create({
  name: "subscript",
  excludes: "subscript superscript",
  parseHTML: () => [{ tag: "sub" }],
  renderHTML: ({ HTMLAttributes }) => ["sub", HTMLAttributes, 0],
});

/** Allineamento dei blocchi selezionati; `null` riporta a sinistra. */
export function setTextAlign(editor: Editor, align: TextAlignment | null) {
  const chain = editor.chain().focus();
  for (const type of LAYOUT_NODES) chain.updateAttributes(type, { textAlign: align });
  chain.run();
}

/** Allineamento del blocco in cui si trova il cursore. */
export function currentTextAlign(editor: Editor): TextAlignment | null {
  for (const type of LAYOUT_NODES) {
    const value = editor.getAttributes(type).textAlign;
    if (isTextAlignment(value)) return value;
  }
  return null;
}

function listItemType(editor: Editor): "listItem" | "taskItem" | null {
  if (editor.isActive("taskItem")) return "taskItem";
  if (editor.isActive("listItem")) return "listItem";
  return null;
}

/**
 * Aumenta (`+1`) o riduce (`-1`) il rientro. Negli elenchi sposta la voce di
 * un livello; altrove cambia il rientro di ogni paragrafo o titolo selezionato.
 */
export function changeIndent(editor: Editor, delta: 1 | -1) {
  const item = listItemType(editor);
  if (item) {
    const chain = editor.chain().focus();
    if (delta > 0) chain.sinkListItem(item).run();
    else chain.liftListItem(item).run();
    return;
  }
  editor
    .chain()
    .focus()
    .command(({ tr, state }) => {
      const { from, to } = state.selection;
      state.doc.nodesBetween(from, to, (node, position) => {
        if (!LAYOUT_NODES.includes(node.type.name)) return true;
        const current = indentLevel(node.attrs.indent) ?? 0;
        const next = Math.min(MAX_INDENT, Math.max(0, current + delta));
        if (next !== current) {
          tr.setNodeMarkup(position, undefined, { ...node.attrs, indent: next || null });
        }
        return false;
      });
      return true;
    })
    .run();
}

/** Vero se il rientro può ancora cambiare nella direzione indicata. */
export function canChangeIndent(editor: Editor, delta: 1 | -1): boolean {
  const item = listItemType(editor);
  if (item) {
    return delta > 0 ? editor.can().sinkListItem(item) : editor.can().liftListItem(item);
  }
  let possible = false;
  const { from, to } = editor.state.selection;
  editor.state.doc.nodesBetween(from, to, (node) => {
    if (!LAYOUT_NODES.includes(node.type.name)) return true;
    const current = indentLevel(node.attrs.indent) ?? 0;
    if (delta > 0 ? current < MAX_INDENT : current > 0) possible = true;
    return false;
  });
  return possible;
}
