import { Node } from "@tiptap/core";
import type { Editor } from "@tiptap/react";
import { t } from "../i18n";

/**
 * Testo cifrato con password dentro la nota. Il nodo contiene solo il testo
 * cifrato (formato age, creato dal backend) e un suggerimento facoltativo: il
 * testo in chiaro esiste solo nel dialog che lo mostra e non entra mai nel
 * documento, nell'indice di ricerca o nella sincronizzazione.
 */

export const ENCRYPTED_TEXT_EVENT = "greatnote-encrypted-text";

/** Stessi controlli strutturali di `is_armored_ciphertext` in Rust. */
const ARMOR = /^-----BEGIN AGE ENCRYPTED FILE-----\r?\n[A-Za-z0-9+/=\r\n]+\r?\n-----END AGE ENCRYPTED FILE-----$/;
export const MAX_CIPHERTEXT_BYTES = 64 * 1024;
export const MAX_HINT_CHARS = 100;
export const MAX_SECRET_BYTES = 32 * 1024;
export const MIN_PASSPHRASE_CHARS = 8;

export function isArmoredCiphertext(value: unknown): value is string {
  return typeof value === "string" && value.length <= MAX_CIPHERTEXT_BYTES && ARMOR.test(value.trimEnd());
}

export function cleanHint(value: unknown): string | null {
  if (typeof value !== "string") return null;
  // eslint-disable-next-line no-control-regex
  const hint = value.replace(/[\u0000-\u001f\u007f-\u009f]/g, " ").trim().slice(0, MAX_HINT_CHARS);
  return hint === "" ? null : hint;
}

export interface EncryptedTextAttrs {
  ciphertext: string;
  hint: string | null;
}

/** Richiesta al dialog: creare un testo cifrato o aprirne uno esistente. */
export type EncryptedTextRequest =
  | {
      mode: "create";
      /** Testo selezionato da cifrare (vuoto: si scrive nel dialog). */
      initialText: string;
      insert: (attrs: EncryptedTextAttrs) => void;
    }
  | {
      mode: "open";
      attrs: EncryptedTextAttrs;
      editable: boolean;
      /** Sostituisce il testo cifrato (stessa posizione, annullabile). */
      update: (attrs: EncryptedTextAttrs) => void;
      /** Rimuove la cifratura: il testo torna in chiaro nella nota. */
      decryptInPlace: (text: string) => void;
    };

function request(detail: EncryptedTextRequest) {
  window.dispatchEvent(new CustomEvent<EncryptedTextRequest>(ENCRYPTED_TEXT_EVENT, { detail }));
}

/** Paragrafi del testo decifrato, con a capo interni come interruzioni. */
function textToBlocks(text: string) {
  return text.split(/\n{2,}/).map((block) => ({
    type: "paragraph",
    content: block
      .split("\n")
      .flatMap((line, index) => [
        ...(index > 0 ? [{ type: "hardBreak" }] : []),
        ...(line ? [{ type: "text", text: line }] : []),
      ]),
  }));
}

/** Apre il dialog per cifrare la selezione (o un testo nuovo). */
export function startEncryptedText(editor: Editor) {
  if (!editor.isEditable) return;
  const { from, to, empty } = editor.state.selection;
  const initialText = empty ? "" : editor.state.doc.textBetween(from, to, "\n\n", "\n");
  request({
    mode: "create",
    initialText,
    insert: (attrs) => {
      editor
        .chain()
        .focus()
        .insertContentAt({ from, to }, { type: "encryptedText", attrs })
        .run();
    },
  });
}

export const EncryptedText = Node.create({
  name: "encryptedText",
  group: "block",
  atom: true,
  draggable: true,
  selectable: true,

  addAttributes() {
    return {
      ciphertext: { default: null },
      hint: { default: null },
    };
  },

  parseHTML() {
    return [
      {
        tag: "pre[data-encrypted-text]",
        // Prima dei blocchi di codice, che riconoscono anche `pre`.
        priority: 100,
        getAttrs: (element) => {
          const pre = element as HTMLElement;
          const ciphertext = pre.textContent?.trim() ?? "";
          return isArmoredCiphertext(ciphertext)
            ? { ciphertext, hint: cleanHint(pre.getAttribute("data-hint")) }
            : false;
        },
      },
    ];
  },

  renderHTML({ node }) {
    return [
      "pre",
      { "data-encrypted-text": "age", ...(node.attrs.hint ? { "data-hint": node.attrs.hint } : {}) },
      node.attrs.ciphertext ?? "",
    ];
  },

  addKeyboardShortcuts() {
    return {
      "Mod-Shift-x": ({ editor }) => {
        startEncryptedText(editor as Editor);
        return true;
      },
    };
  },

  addNodeView() {
    return ({ node: initial, editor, getPos }) => {
      let node = initial;
      const dom = document.createElement("div");
      dom.className = "note-secret";
      dom.contentEditable = "false";
      const button = document.createElement("button");
      button.type = "button";
      button.className = "note-secret-button";
      const icon = document.createElement("span");
      icon.className = "note-secret-icon";
      icon.setAttribute("aria-hidden", "true");
      icon.textContent = "🔒";
      const label = document.createElement("span");
      label.className = "note-secret-label";
      const hint = document.createElement("span");
      hint.className = "note-secret-hint";
      button.append(icon, label, hint);
      dom.append(button);

      const position = () => (typeof getPos === "function" ? getPos() : undefined);
      /** Nodo ancora al suo posto e invariato (il dialog può restare aperto). */
      const current = (attrs: EncryptedTextAttrs) => {
        const at = position();
        if (typeof at !== "number") return null;
        const found = editor.state.doc.nodeAt(at);
        return found && found.type.name === "encryptedText" && found.attrs.ciphertext === attrs.ciphertext
          ? { at, found }
          : null;
      };

      const render = () => {
        label.textContent = t.secretLocked;
        hint.textContent = node.attrs.hint ? t.secretHint(node.attrs.hint as string) : "";
        button.setAttribute("aria-label", t.secretOpenLabel);
        button.title = t.secretOpenLabel;
      };

      button.addEventListener("click", (event) => {
        event.preventDefault();
        const attrs = { ciphertext: node.attrs.ciphertext as string, hint: (node.attrs.hint as string | null) ?? null };
        request({
          mode: "open",
          attrs,
          editable: editor.isEditable,
          update: (next) => {
            const target = current(attrs);
            if (!target || !editor.isEditable) return;
            editor.view.dispatch(editor.state.tr.setNodeMarkup(target.at, undefined, next));
          },
          decryptInPlace: (text) => {
            const target = current(attrs);
            if (!target || !editor.isEditable) return;
            editor
              .chain()
              .focus()
              .insertContentAt({ from: target.at, to: target.at + target.found.nodeSize }, textToBlocks(text))
              .run();
          },
        });
      });
      render();

      return {
        dom,
        update: (updated) => {
          if (updated.type.name !== "encryptedText") return false;
          node = updated;
          render();
          return true;
        },
        stopEvent: (event) => event.target instanceof HTMLElement && button.contains(event.target),
        ignoreMutation: () => true,
      };
    };
  },
});
