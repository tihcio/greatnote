import { Node } from "@tiptap/core";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import {
  ANNOTATE_IMAGE_EVENT,
  VIEW_WIDTH,
  renderAnnotationsInto,
  sanitizeAnnotations,
} from "./imageAnnotations";
import type { AnnotateImageDetail } from "./imageAnnotations";

/**
 * Immagini nel testo della nota. Il nodo contiene solo l'id di un allegato
 * della nota: i byte arrivano dal backend (comando verificato con l'hash),
 * mai da URL o percorsi, quindi un'immagine incollata dal web non diventa
 * una richiesta di rete.
 */

const urls = new Map<string, Promise<string>>();

export function attachmentImageUrl(attachmentId: string): Promise<string> {
  let url = urls.get(attachmentId);
  if (!url) {
    url = invoke<ArrayBuffer>("read_attachment_image", { attachmentId }).then((bytes) =>
      URL.createObjectURL(new Blob([bytes])),
    );
    // Un errore non resta in cache: il prossimo tentativo riprova.
    url.catch(() => urls.delete(attachmentId));
    urls.set(attachmentId, url);
  }
  return url;
}

/** Libera l'immagine in cache (allegato eliminato). */
export function forgetAttachmentImage(attachmentId: string) {
  const url = urls.get(attachmentId);
  urls.delete(attachmentId);
  void url?.then((value) => URL.revokeObjectURL(value)).catch(() => undefined);
}

/** Larghezze rapide (px); `null` = dimensione naturale. */
const PRESET_WIDTHS: { label: () => string; width: number | null }[] = [
  { label: () => t.imageSizeSmall, width: 240 },
  { label: () => t.imageSizeMedium, width: 480 },
  { label: () => t.imageSizeLarge, width: 720 },
  { label: () => t.imageSizeOriginal, width: null },
];

export const AttachmentImage = Node.create({
  name: "image",
  group: "block",
  atom: true,
  draggable: true,
  selectable: true,

  addAttributes() {
    return {
      attachmentId: { default: null },
      alt: { default: null },
      width: { default: null },
      annotations: { default: null },
    };
  },

  parseHTML() {
    return [
      {
        tag: "img[data-attachment-id]",
        getAttrs: (element) => {
          const id = (element as HTMLElement).getAttribute("data-attachment-id");
          return id && /^[A-Za-z0-9-]{1,64}$/.test(id)
            ? { attachmentId: id, alt: (element as HTMLElement).getAttribute("alt") }
            : false;
        },
      },
    ];
  },

  renderHTML({ node }) {
    return ["img", { "data-attachment-id": node.attrs.attachmentId, alt: node.attrs.alt ?? "" }];
  },

  addNodeView() {
    return ({ node: initial, editor, getPos }) => {
      let node = initial;
      let aspect = 0; // larghezza / altezza naturale
      const dom = document.createElement("figure");
      dom.className = "note-image";
      dom.contentEditable = "false";
      const frame = document.createElement("div");
      frame.className = "note-image-frame";
      const image = document.createElement("img");
      image.draggable = false;
      const overlay = document.createElementNS("http://www.w3.org/2000/svg", "svg");
      overlay.setAttribute("class", "note-image-annotations");
      overlay.setAttribute("aria-hidden", "true");
      overlay.setAttribute("preserveAspectRatio", "none");
      const handle = document.createElement("span");
      handle.className = "note-image-resize";
      handle.title = t.imageResize;
      const tools = document.createElement("div");
      tools.className = "note-image-tools";
      const caption = document.createElement("figcaption");
      frame.append(image, overlay, handle, tools);
      dom.append(frame, caption);

      /** Aggiorna gli attributi del nodo con una transazione annullabile. */
      const setAttrs = (patch: Record<string, unknown>) => {
        const position = typeof getPos === "function" ? getPos() : undefined;
        if (typeof position !== "number" || !editor.isEditable) return;
        const current = editor.state.doc.nodeAt(position);
        if (!current || current.type.name !== "image") return;
        editor.view.dispatch(
          editor.state.tr.setNodeMarkup(position, undefined, { ...current.attrs, ...patch }),
        );
      };

      const annotate = () => {
        if (!editor.isEditable || !node.attrs.attachmentId) return;
        window.dispatchEvent(
          new CustomEvent<AnnotateImageDetail>(ANNOTATE_IMAGE_EVENT, {
            detail: {
              attachmentId: node.attrs.attachmentId as string,
              alt: (node.attrs.alt as string | null) ?? null,
              annotations: sanitizeAnnotations(node.attrs.annotations) ?? [],
              save: (annotations) => setAttrs({ annotations }),
            },
          }),
        );
      };

      for (const preset of PRESET_WIDTHS) {
        const button = document.createElement("button");
        button.type = "button";
        button.textContent = preset.label();
        button.addEventListener("mousedown", (event) => event.preventDefault());
        button.addEventListener("click", () => setAttrs({ width: preset.width }));
        tools.append(button);
      }
      const annotateButton = document.createElement("button");
      annotateButton.type = "button";
      annotateButton.className = "annotate";
      annotateButton.textContent = t.imageAnnotate;
      annotateButton.addEventListener("mousedown", (event) => event.preventDefault());
      annotateButton.addEventListener("click", annotate);
      tools.append(annotateButton);
      image.addEventListener("dblclick", annotate);

      // Ridimensionamento trascinando l'angolo in basso a destra.
      handle.addEventListener("pointerdown", (event) => {
        if (!editor.isEditable) return;
        event.preventDefault();
        event.stopPropagation();
        const startX = event.clientX;
        const startWidth = frame.getBoundingClientRect().width;
        const available = dom.getBoundingClientRect().width || startWidth;
        handle.setPointerCapture(event.pointerId);
        const move = (moveEvent: PointerEvent) => {
          const width = Math.min(available, Math.max(40, startWidth + moveEvent.clientX - startX));
          frame.style.width = `${Math.round(width)}px`;
        };
        const up = (upEvent: PointerEvent) => {
          handle.releasePointerCapture(upEvent.pointerId);
          handle.removeEventListener("pointermove", move);
          handle.removeEventListener("pointerup", up);
          const width = Math.round(frame.getBoundingClientRect().width);
          setAttrs({ width: Math.min(2400, Math.max(40, width)) });
        };
        handle.addEventListener("pointermove", move);
        handle.addEventListener("pointerup", up);
      });

      const drawAnnotations = () => {
        const shapes = sanitizeAnnotations(node.attrs.annotations) ?? [];
        if (!aspect || shapes.length === 0) {
          overlay.replaceChildren();
          return;
        }
        renderAnnotationsInto(overlay, shapes, VIEW_WIDTH / aspect);
      };
      image.addEventListener("load", () => {
        if (image.naturalWidth && image.naturalHeight) {
          aspect = image.naturalWidth / image.naturalHeight;
          drawAnnotations();
        }
      });

      const render = () => {
        image.alt = node.attrs.alt ?? "";
        caption.textContent = node.attrs.alt ?? "";
        frame.style.width = node.attrs.width ? `${node.attrs.width}px` : "";
        handle.hidden = !editor.isEditable;
        tools.hidden = !editor.isEditable;
        dom.classList.remove("missing");
        dom.removeAttribute("data-missing");
        drawAnnotations();
        const id = node.attrs.attachmentId as string | null;
        if (!id) return;
        void attachmentImageUrl(id)
          .then((url) => {
            if (node.attrs.attachmentId === id && image.src !== url) image.src = url;
          })
          .catch(() => {
            dom.classList.add("missing");
            dom.setAttribute("data-missing", t.imageUnavailable);
          });
      };
      render();
      return {
        dom,
        update: (updated) => {
          if (updated.type.name !== "image") return false;
          const changed = updated.attrs.attachmentId !== node.attrs.attachmentId;
          node = updated;
          if (changed) {
            aspect = 0;
            image.removeAttribute("src");
          }
          render();
          return true;
        },
        // Gli eventi dei controlli non diventano selezioni dell'editor.
        stopEvent: (event) =>
          event.target instanceof HTMLElement &&
          (event.target.closest(".note-image-tools") !== null || event.target === handle),
        ignoreMutation: () => true,
      };
    };
  },
});
