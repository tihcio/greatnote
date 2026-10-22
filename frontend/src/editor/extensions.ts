import Highlight from "@tiptap/extension-highlight";
import { TaskItem, TaskList } from "@tiptap/extension-list";
import { Table, TableCell, TableHeader, TableRow } from "@tiptap/extension-table";
import { Color, FontFamily, FontSize, TextStyle } from "@tiptap/extension-text-style";
import StarterKit from "@tiptap/starter-kit";
import { AttachmentImage } from "./attachmentImage";
import { EncryptedText } from "./encryptedText";
import { SearchHighlight } from "./searchHighlight";

/** Schemi ammessi per i link, gli stessi della validazione Rust. */
export function isSafeLink(url: string): boolean {
  return /^(https?:\/\/|mailto:)\S+$/i.test(url.trim());
}

/**
 * Estensioni dell'editor. Ogni nodo e contrassegno qui deve essere ammesso da
 * `validate_document` (src/domain/note.rs) e gestito dalle esportazioni.
 */
export const editorExtensions = [
  StarterKit.configure({
    link: {
      openOnClick: false,
      autolink: true,
      defaultProtocol: "https",
      isAllowedUri: (url) => isSafeLink(url),
    },
  }),
  TaskList,
  TaskItem.configure({ nested: true }),
  Table.configure({ resizable: false }),
  TableRow,
  TableHeader,
  TableCell,
  Highlight.configure({ multicolor: true }),
  TextStyle,
  Color,
  FontFamily,
  FontSize,
  AttachmentImage,
  EncryptedText,
  SearchHighlight,
];

export const FONT_FAMILIES = [
  { value: "sans-serif", label: "Sans Serif" },
  { value: "serif", label: "Serif" },
  { value: "monospace", label: "Monospace" },
] as const;

/** Devono coincidere con `FONT_SIZES` in src/domain/note.rs. */
export const FONT_SIZES = ["11px", "13px", "15px", "17px", "20px", "24px", "28px", "32px", "40px"];

export const TEXT_COLORS = [
  "#262921", "#6b7280", "#b42318", "#c4320a", "#b54708", "#3f7d3a", "#1d6fa3", "#5b3fb5", "#a3238f",
];

export const HIGHLIGHT_COLORS = ["#fff3a3", "#ffd6a5", "#ffc9c9", "#c9f2d0", "#cde4ff", "#e5d4ff"];
