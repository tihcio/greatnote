import type { Editor } from "@tiptap/react";
import { t } from "../i18n";
import { startEncryptedText } from "./encryptedText";

export interface BlockCommand {
  id: string;
  label: string;
  /** Parole aggiuntive per il filtro dei comandi rapidi. */
  keywords: string;
  icon: string;
  run: (editor: Editor) => void;
}

function today(withTime: boolean): string {
  return new Intl.DateTimeFormat(undefined, {
    day: "numeric",
    month: "long",
    year: "numeric",
    ...(withTime ? { hour: "2-digit", minute: "2-digit" } : {}),
  }).format(new Date());
}

/** Blocchi inseribili: "Inserisci", "Comandi rapidi" e menu "/". */
export function blockCommands(): BlockCommand[] {
  return [
    {
      id: "paragraph",
      label: t.blockText,
      keywords: "testo paragrafo text paragraph",
      icon: "¶",
      run: (editor) => editor.chain().focus().setParagraph().run(),
    },
    ...[1, 2, 3].map((level) => ({
      id: `heading-${level}`,
      label: t.blockHeading(level),
      keywords: `titolo heading h${level}`,
      icon: `H${level}`,
      run: (editor: Editor) =>
        editor.chain().focus().toggleHeading({ level: level as 1 | 2 | 3 }).run(),
    })),
    {
      id: "bullet-list",
      label: t.bulletList,
      keywords: "elenco puntato lista bullet list",
      icon: "•",
      run: (editor) => editor.chain().focus().toggleBulletList().run(),
    },
    {
      id: "ordered-list",
      label: t.orderedList,
      keywords: "elenco numerato ordered list",
      icon: "1.",
      run: (editor) => editor.chain().focus().toggleOrderedList().run(),
    },
    {
      id: "checklist",
      label: t.checklist,
      keywords: "checklist attività caselle todo task",
      icon: "☑",
      run: (editor) => editor.chain().focus().toggleTaskList().run(),
    },
    {
      id: "quote",
      label: t.quote,
      keywords: "citazione quote blockquote",
      icon: "“",
      run: (editor) => editor.chain().focus().toggleBlockquote().run(),
    },
    {
      id: "code-block",
      label: t.codeBlock,
      keywords: "codice code blocco",
      icon: "</>",
      run: (editor) => editor.chain().focus().toggleCodeBlock().run(),
    },
    {
      id: "table",
      label: t.insertTable,
      keywords: "tabella table griglia",
      icon: "▦",
      run: (editor) =>
        editor.chain().focus().insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run(),
    },
    {
      id: "divider",
      label: t.divider,
      keywords: "separatore linea divider rule",
      icon: "—",
      run: (editor) => editor.chain().focus().setHorizontalRule().run(),
    },
    {
      id: "date",
      label: t.insertDate,
      keywords: "data oggi date today",
      icon: "📅",
      run: (editor) => editor.chain().focus().insertContent(today(false)).run(),
    },
    {
      id: "date-time",
      label: t.insertDateTime,
      keywords: "ora data adesso time now",
      icon: "🕘",
      run: (editor) => editor.chain().focus().insertContent(today(true)).run(),
    },
    {
      id: "encrypted-text",
      label: t.encryptText,
      keywords: "cifra cifrato password segreto encrypt encrypted secret",
      icon: "🔒",
      run: (editor) => startEncryptedText(editor),
    },
  ];
}
