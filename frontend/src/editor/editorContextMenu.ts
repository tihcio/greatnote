import { invoke } from "@tauri-apps/api/core";
import type { Editor } from "@tiptap/react";
import type { MouseEvent as ReactMouseEvent } from "react";
import { separator } from "../app/menu";
import type { MenuEntry } from "../app/menu";
import { t } from "../i18n";
import { openContextMenu } from "../lib/contextMenu";
import { editLink } from "./EditorToolbar";
import { startEncryptedText } from "./encryptedText";
import { isSafeLink } from "./extensions";

export interface EditorMenuOptions {
  /** Incolla dagli appunti (l'app usa il proprio comando con i paragrafi). */
  paste?: (plain: boolean) => void;
  findReplace?: () => void;
  /** Voci AI sulla selezione o sulla nota (solo nell'app principale). */
  aiItems?: () => MenuEntry[];
  onError?: (message: string) => void;
}

/** Incolla il testo degli appunti al cursore, un paragrafo per riga. */
async function pastePlainText(editor: Editor) {
  const text = await invoke<string>("read_clipboard_text");
  if (!text) return;
  const lines = text.replace(/\r\n?/g, "\n").split("\n");
  if (lines.length === 1) {
    editor.chain().focus().insertContent(text).run();
    return;
  }
  editor
    .chain()
    .focus()
    .insertContent(
      lines.map((line) => (line ? { type: "paragraph", content: [{ type: "text", text: line }] } : { type: "paragraph" })),
    )
    .run();
}

/**
 * Con il clic destro fuori dalla selezione il cursore va dove si è
 * cliccato, come nei programmi di scrittura: le voci (collegamento,
 * tabella) riguardano quel punto.
 */
function moveCaretToPointer(editor: Editor, event: { clientX: number; clientY: number }) {
  if (event.clientX === 0 && event.clientY === 0) return;
  const found = editor.view.posAtCoords({ left: event.clientX, top: event.clientY });
  if (!found) return;
  const { from, to } = editor.state.selection;
  if (found.pos >= from && found.pos <= to && from !== to) return;
  if (found.inside >= 0) {
    const node = editor.state.doc.nodeAt(found.inside);
    // Nodi atomici (immagini, testo cifrato): restano selezionati come blocco.
    if (node?.isAtom && !node.isInline) return;
  }
  editor.commands.setTextSelection(found.pos);
}

/** Menu contestuale dell'editor della nota. */
export function openEditorContextMenu(
  event: ReactMouseEvent,
  editor: Editor,
  options: EditorMenuOptions = {},
) {
  if (event.nativeEvent.defaultPrevented) return;
  moveCaretToPointer(editor, event);
  const editable = editor.isEditable;
  const hasSelection = !editor.state.selection.empty;
  const onLink = editor.isActive("link");
  const href = editor.getAttributes("link").href as string | undefined;
  const inTable = editor.isActive("table");
  const exec = (command: "cut" | "copy") => () => {
    editor.commands.focus();
    document.execCommand(command);
  };
  const paste = (plain: boolean) => () => {
    if (options.paste) options.paste(plain);
    else void pastePlainText(editor).catch((cause: unknown) => options.onError?.(String(cause)));
  };
  const mark = (id: string, label: string, name: string, run: () => void, accelerator?: string): MenuEntry => ({
    id,
    label,
    accelerator,
    checked: editor.isActive(name),
    run,
  });

  const items: MenuEntry[] = [
    {
      id: "ed-undo",
      label: t.undo,
      accelerator: "Ctrl+Z",
      enabled: editable && editor.can().undo(),
      run: () => void editor.chain().focus().undo().run(),
    },
    {
      id: "ed-redo",
      label: t.redo,
      accelerator: "Ctrl+Y",
      enabled: editable && editor.can().redo(),
      run: () => void editor.chain().focus().redo().run(),
    },
    separator,
    { id: "ed-cut", label: t.menuCut, accelerator: "Ctrl+X", enabled: editable && hasSelection, run: exec("cut") },
    { id: "ed-copy", label: t.menuCopy, accelerator: "Ctrl+C", enabled: hasSelection, run: exec("copy") },
    { id: "ed-paste", label: t.menuPaste, accelerator: "Ctrl+V", enabled: editable, run: paste(false) },
    {
      id: "ed-paste-plain",
      label: t.menuPastePlain,
      accelerator: "Ctrl+Shift+V",
      enabled: editable,
      run: paste(true),
    },
    { id: "ed-select-all", label: t.menuSelectAll, accelerator: "Ctrl+A", run: () => void editor.chain().focus().selectAll().run() },
  ];

  if (onLink && href) {
    items.push(
      separator,
      {
        id: "ed-link-open",
        label: t.openLink,
        enabled: isSafeLink(href),
        run: () =>
          void invoke("open_note_link", { href }).catch((cause: unknown) => options.onError?.(String(cause))),
      },
      {
        id: "ed-link-copy",
        label: t.copyLinkAddress,
        run: () =>
          void invoke("write_clipboard_text", { text: href }).catch((cause: unknown) => options.onError?.(String(cause))),
      },
    );
    if (editable) {
      items.push(
        { id: "ed-link-edit", label: t.editLink, run: () => void editLink(editor) },
        { id: "ed-link-remove", label: t.removeLink, run: () => void editor.chain().focus().extendMarkRange("link").unsetLink().run() },
      );
    }
  }

  if (editable) {
    items.push(separator);
    if (!onLink) items.push({ id: "ed-link-insert", label: t.insertLink, run: () => void editLink(editor) });
    if (hasSelection) {
      items.push({
        kind: "submenu",
        id: "ed-format",
        label: t.formatMenu,
        items: [
          mark("ed-bold", t.bold, "bold", () => editor.chain().focus().toggleBold().run(), "Ctrl+B"),
          mark("ed-italic", t.italic, "italic", () => editor.chain().focus().toggleItalic().run(), "Ctrl+I"),
          mark("ed-underline", t.underline, "underline", () => editor.chain().focus().toggleUnderline().run(), "Ctrl+U"),
          mark("ed-strike", t.strike, "strike", () => editor.chain().focus().toggleStrike().run()),
          mark("ed-code", t.code, "code", () => editor.chain().focus().toggleCode().run(), "Ctrl+E"),
          separator,
          { id: "ed-clear", label: t.clearFormatting, run: () => void editor.chain().focus().unsetAllMarks().run() },
        ],
      });
    }
    if (inTable) {
      items.push({
        kind: "submenu",
        id: "ed-table",
        label: t.tableMenu,
        items: [
          { id: "ed-row-before", label: t.tableRowBefore, run: () => void editor.chain().focus().addRowBefore().run() },
          { id: "ed-row-after", label: t.tableRowAfter, run: () => void editor.chain().focus().addRowAfter().run() },
          { id: "ed-col-before", label: t.tableColumnBefore, run: () => void editor.chain().focus().addColumnBefore().run() },
          { id: "ed-col-after", label: t.tableColumnAfter, run: () => void editor.chain().focus().addColumnAfter().run() },
          separator,
          { id: "ed-row-delete", label: t.tableDeleteRow, run: () => void editor.chain().focus().deleteRow().run() },
          { id: "ed-col-delete", label: t.tableDeleteColumn, run: () => void editor.chain().focus().deleteColumn().run() },
          {
            id: "ed-table-delete",
            label: t.tableDelete,
            destructive: true,
            run: () => void editor.chain().focus().deleteTable().run(),
          },
        ],
      });
    }
    items.push({
      id: "ed-encrypt",
      label: hasSelection ? t.encryptSelection : t.encryptText,
      accelerator: "Ctrl+Shift+X",
      run: () => startEncryptedText(editor),
    });
    const ai = options.aiItems?.() ?? [];
    if (ai.length > 0) items.push({ kind: "submenu", id: "ed-ai", label: t.aiMenu, items: ai });
  }
  if (options.findReplace) {
    items.push(separator, { id: "ed-find", label: t.findReplace, accelerator: "Ctrl+H", run: options.findReplace });
  }

  openContextMenu(event, t.editorContextLabel, items);
}
