import type { JSONContent } from "@tiptap/react";
import { FONT_FAMILIES, FONT_SIZES, isSafeLink } from "./extensions";
import { cleanHint, isArmoredCiphertext } from "./encryptedText";
import { sanitizeAnnotations } from "./imageAnnotations";

const HEX_COLOR = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i;

function rgbToHex(value: string): string | null {
  const match = /^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/i.exec(value.trim());
  if (!match) return null;
  return `#${match
    .slice(1, 4)
    .map((part) => Math.min(255, Number(part)).toString(16).padStart(2, "0"))
    .join("")}`;
}

function color(value: unknown): string | null {
  if (typeof value !== "string") return null;
  if (HEX_COLOR.test(value)) return value;
  return rgbToHex(value);
}

type Mark = NonNullable<JSONContent["marks"]>[number];

function sanitizeMark(mark: Mark): Mark | null {
  const attrs = (mark.attrs ?? {}) as Record<string, unknown>;
  switch (mark.type) {
    case "textStyle": {
      const next: Record<string, unknown> = {};
      const parsedColor = color(attrs.color);
      if (parsedColor) next.color = parsedColor;
      if (FONT_FAMILIES.some((family) => family.value === attrs.fontFamily)) {
        next.fontFamily = attrs.fontFamily;
      }
      if (typeof attrs.fontSize === "string" && FONT_SIZES.includes(attrs.fontSize)) {
        next.fontSize = attrs.fontSize;
      }
      return Object.keys(next).length > 0 ? { type: "textStyle", attrs: next } : null;
    }
    case "highlight": {
      const parsedColor = color(attrs.color);
      return parsedColor ? { type: "highlight", attrs: { color: parsedColor } } : { type: "highlight" };
    }
    case "link":
      return typeof attrs.href === "string" && isSafeLink(attrs.href) ? mark : null;
    default:
      return mark;
  }
}

/**
 * Porta il documento entro le regole della validazione Rust: stili incollati
 * da altre app (es. `rgb(…)`, dimensioni libere) vengono convertiti o
 * rimossi invece di far fallire il salvataggio.
 */
export function sanitizeDocument(node: JSONContent): JSONContent {
  if (node.type === "image") {
    // Solo gli attributi ammessi dalla validazione Rust.
    const attrs = (node.attrs ?? {}) as Record<string, unknown>;
    if (typeof attrs.attachmentId !== "string" || !/^[A-Za-z0-9-]{1,64}$/.test(attrs.attachmentId)) {
      return { type: "paragraph" };
    }
    const width = typeof attrs.width === "number" ? Math.round(attrs.width) : null;
    return {
      type: "image",
      attrs: {
        attachmentId: attrs.attachmentId,
        alt: typeof attrs.alt === "string" ? attrs.alt.slice(0, 500) : null,
        width: width !== null && width >= 40 && width <= 2400 ? width : null,
        annotations: sanitizeAnnotations(attrs.annotations),
      },
    };
  }
  if (node.type === "encryptedText") {
    const attrs = (node.attrs ?? {}) as Record<string, unknown>;
    // Un testo cifrato non valido non può essere ricostruito: resta un
    // paragrafo vuoto invece di bloccare il salvataggio della nota.
    if (!isArmoredCiphertext(attrs.ciphertext)) return { type: "paragraph" };
    return {
      type: "encryptedText",
      attrs: { ciphertext: attrs.ciphertext.trimEnd(), hint: cleanHint(attrs.hint) },
    };
  }
  const next: JSONContent = { ...node };
  if (node.marks) {
    const marks = node.marks.map(sanitizeMark).filter((mark): mark is Mark => mark !== null);
    if (marks.length > 0) next.marks = marks;
    else delete next.marks;
  }
  if (node.content) next.content = node.content.map(sanitizeDocument);
  return next;
}
