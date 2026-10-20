/**
 * Annotazioni sulle immagini: forme vettoriali con coordinate relative
 * all'immagine (0–1) e spessori relativi alla larghezza, così restano nitide
 * e proporzionate a qualunque dimensione. Le regole coincidono con
 * `validate_annotations` (src/domain/note.rs).
 */

export type AnnotationKind = "pen" | "highlighter" | "arrow" | "rect" | "ellipse" | "text";
export type Point = [number, number];

export interface Annotation {
  kind: AnnotationKind;
  color: string;
  /** Spessore (o altezza del testo) in frazione della larghezza. */
  size: number;
  points: Point[];
  text?: string;
}

export const MAX_ANNOTATIONS = 300;
const MAX_POINTS = 2000;
const KINDS: AnnotationKind[] = ["pen", "highlighter", "arrow", "rect", "ellipse", "text"];

const round = (value: number) => Math.round(value * 10_000) / 10_000;
const clampCoordinate = (value: number) => Math.min(1.05, Math.max(-0.05, value));

/** Tiene solo forme valide, con coordinate arrotondate (documento compatto). */
export function sanitizeAnnotations(value: unknown): Annotation[] | null {
  if (!Array.isArray(value)) return null;
  const shapes: Annotation[] = [];
  for (const item of value.slice(0, MAX_ANNOTATIONS)) {
    if (!item || typeof item !== "object") continue;
    const shape = item as Record<string, unknown>;
    const kind = shape.kind as AnnotationKind;
    if (!KINDS.includes(kind)) continue;
    if (typeof shape.color !== "string" || !/^#[0-9a-f]{6}$/i.test(shape.color)) continue;
    if (typeof shape.size !== "number" || !(shape.size >= 0.0005 && shape.size <= 0.2)) continue;
    if (!Array.isArray(shape.points)) continue;
    const points = shape.points
      .filter(
        (point): point is [number, number] =>
          Array.isArray(point) && point.length === 2 && point.every((n) => typeof n === "number" && Number.isFinite(n)),
      )
      .slice(0, MAX_POINTS)
      .map(([x, y]) => [round(clampCoordinate(x)), round(clampCoordinate(y))] as Point);
    const expected = kind === "text" ? 1 : kind === "pen" || kind === "highlighter" ? null : 2;
    if (expected !== null ? points.length !== expected : points.length < 2) continue;
    const next: Annotation = { kind, color: shape.color.toLowerCase(), size: round(shape.size), points };
    if (kind === "text") {
      if (typeof shape.text !== "string" || !shape.text.trim()) continue;
      next.text = shape.text.slice(0, 500);
    }
    shapes.push(next);
  }
  return shapes.length > 0 ? shapes : null;
}

/** Elemento SVG descritto in modo neutro (DOM o React). */
export interface SvgElement {
  tag: "path" | "rect" | "ellipse" | "text";
  attrs: Record<string, string | number>;
  lines?: string[];
}

/** Larghezza logica del disegno: l'altezza segue le proporzioni. */
export const VIEW_WIDTH = 1000;

function boundingBox(points: Point[]) {
  const xs = points.map(([x]) => x);
  const ys = points.map(([, y]) => y);
  return { left: Math.min(...xs), top: Math.min(...ys), right: Math.max(...xs), bottom: Math.max(...ys) };
}

/** Forme da disegnare per un'annotazione, in un riquadro largo
 *  `VIEW_WIDTH` e alto `height`. */
export function annotationElements(shape: Annotation, height: number): SvgElement[] {
  const w = VIEW_WIDTH;
  const toX = (x: number) => round(x * w);
  const toY = (y: number) => round(y * height);
  const stroke = Math.max(0.5, shape.size * w);
  const base = {
    fill: "none",
    stroke: shape.color,
    "stroke-width": stroke,
    "stroke-linecap": "round",
    "stroke-linejoin": "round",
  };
  switch (shape.kind) {
    case "pen":
    case "highlighter": {
      const d = shape.points.map(([x, y], index) => `${index === 0 ? "M" : "L"}${toX(x)} ${toY(y)}`).join(" ");
      return [
        {
          tag: "path",
          attrs:
            shape.kind === "highlighter"
              ? { ...base, d, "stroke-opacity": 0.42, "stroke-linecap": "square", style: "mix-blend-mode: multiply" }
              : { ...base, d },
        },
      ];
    }
    case "arrow": {
      const [[x1, y1], [x2, y2]] = shape.points;
      const [ax, ay, bx, by] = [toX(x1), toY(y1), toX(x2), toY(y2)];
      const angle = Math.atan2(by - ay, bx - ax);
      const head = Math.max(10, stroke * 4);
      const left = [bx - head * Math.cos(angle - 0.45), by - head * Math.sin(angle - 0.45)];
      const right = [bx - head * Math.cos(angle + 0.45), by - head * Math.sin(angle + 0.45)];
      const d = `M${ax} ${ay} L${bx} ${by} M${round(left[0])} ${round(left[1])} L${bx} ${by} L${round(right[0])} ${round(right[1])}`;
      return [{ tag: "path", attrs: { ...base, d } }];
    }
    case "rect": {
      const box = boundingBox(shape.points);
      return [
        {
          tag: "rect",
          attrs: {
            ...base,
            x: toX(box.left),
            y: toY(box.top),
            width: toX(box.right) - toX(box.left),
            height: toY(box.bottom) - toY(box.top),
            rx: stroke,
          },
        },
      ];
    }
    case "ellipse": {
      const box = boundingBox(shape.points);
      return [
        {
          tag: "ellipse",
          attrs: {
            ...base,
            cx: round((toX(box.left) + toX(box.right)) / 2),
            cy: round((toY(box.top) + toY(box.bottom)) / 2),
            rx: round((toX(box.right) - toX(box.left)) / 2),
            ry: round((toY(box.bottom) - toY(box.top)) / 2),
          },
        },
      ];
    }
    case "text": {
      const fontSize = Math.max(8, shape.size * w);
      return [
        {
          tag: "text",
          attrs: {
            x: toX(shape.points[0][0]),
            y: toY(shape.points[0][1]) + fontSize,
            fill: shape.color,
            "font-size": round(fontSize),
            "font-weight": 600,
            "font-family": "Inter, 'Noto Sans', sans-serif",
            stroke: "#ffffff",
            "stroke-width": round(fontSize * 0.16),
            "paint-order": "stroke",
            "stroke-linejoin": "round",
          },
          lines: (shape.text ?? "").split("\n"),
        },
      ];
    }
  }
}

/** Disegna le annotazioni in un elemento SVG (vista nell'editor). */
export function renderAnnotationsInto(svg: SVGSVGElement, shapes: Annotation[], height: number) {
  const namespace = "http://www.w3.org/2000/svg";
  svg.replaceChildren();
  svg.setAttribute("viewBox", `0 0 ${VIEW_WIDTH} ${round(height)}`);
  for (const shape of shapes) {
    for (const element of annotationElements(shape, height)) {
      const node = document.createElementNS(namespace, element.tag);
      for (const [key, value] of Object.entries(element.attrs)) node.setAttribute(key, String(value));
      element.lines?.forEach((line, index) => {
        const span = document.createElementNS(namespace, "tspan");
        span.setAttribute("x", String(element.attrs.x));
        span.setAttribute("dy", index === 0 ? "0" : "1.2em");
        span.textContent = line;
        node.append(span);
      });
      svg.append(node);
    }
  }
}

/** Riquadro (relativo) di un'annotazione, per la gomma. */
export function annotationBounds(shape: Annotation, aspect: number) {
  if (shape.kind === "text") {
    const [x, y] = shape.points[0];
    const lines = (shape.text ?? "").split("\n");
    const longest = Math.max(...lines.map((line) => line.length), 1);
    return {
      left: x,
      top: y,
      right: x + longest * shape.size * 0.6,
      bottom: y + lines.length * shape.size * 1.25 * aspect,
    };
  }
  const box = boundingBox(shape.points);
  const pad = shape.size;
  return { left: box.left - pad, top: box.top - pad * aspect, right: box.right + pad, bottom: box.bottom + pad * aspect };
}

/** Evento con cui l'immagine nel testo chiede di aprire l'editor delle
 *  annotazioni. */
export const ANNOTATE_IMAGE_EVENT = "greatnote-annotate-image";

export interface AnnotateImageDetail {
  attachmentId: string;
  alt: string | null;
  annotations: Annotation[];
  /** Salva le annotazioni sul nodo (annullabile con Ctrl+Z). */
  save: (annotations: Annotation[] | null) => void;
}
