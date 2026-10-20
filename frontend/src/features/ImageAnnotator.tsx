import { useCallback, useEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { t } from "../i18n";
import { askConfirmation } from "../lib/confirm";
import { attachmentImageUrl } from "../editor/attachmentImage";
import {
  ANNOTATE_IMAGE_EVENT,
  MAX_ANNOTATIONS,
  VIEW_WIDTH,
  annotationBounds,
  annotationElements,
  sanitizeAnnotations,
} from "../editor/imageAnnotations";
import type { AnnotateImageDetail, Annotation, AnnotationKind, Point } from "../editor/imageAnnotations";

type Tool = AnnotationKind | "eraser";

const COLORS = ["#e53935", "#fb8c00", "#fdd835", "#43a047", "#1e88e5", "#8e24aa", "#111111", "#ffffff"];
/** Spessori per strumento (frazione della larghezza): sottile, medio, spesso. */
const SIZES: Record<AnnotationKind, [number, number, number]> = {
  pen: [0.003, 0.006, 0.012],
  highlighter: [0.015, 0.03, 0.05],
  arrow: [0.003, 0.006, 0.01],
  rect: [0.003, 0.006, 0.01],
  ellipse: [0.003, 0.006, 0.01],
  text: [0.025, 0.04, 0.06],
};
const MIN_STEP = 0.002;

function toolLabel(tool: Tool): string {
  return {
    pen: t.annotatePen,
    highlighter: t.annotateHighlighter,
    arrow: t.annotateArrow,
    rect: t.annotateRect,
    ellipse: t.annotateEllipse,
    text: t.annotateText,
    eraser: t.annotateEraser,
  }[tool];
}

const TOOL_ICONS: Record<Tool, string> = {
  pen: "✎",
  highlighter: "▌",
  arrow: "↗",
  rect: "▭",
  ellipse: "◯",
  text: "T",
  eraser: "⌫",
};

function Shape({ shape, height }: { shape: Annotation; height: number }) {
  return (
    <>
      {annotationElements(shape, height).map((element, index) => {
        const props = Object.fromEntries(
          Object.entries(element.attrs)
            .filter(([key]) => key !== "style")
            .map(([key, value]) => [key.replace(/-([a-z])/g, (_, letter: string) => letter.toUpperCase()), value]),
        );
        const style = element.attrs.style === "mix-blend-mode: multiply" ? { mixBlendMode: "multiply" as const } : undefined;
        if (element.tag === "text") {
          return (
            <text key={index} {...props} style={style}>
              {element.lines?.map((line, lineIndex) => (
                <tspan dy={lineIndex === 0 ? 0 : "1.2em"} key={lineIndex} x={element.attrs.x}>
                  {line}
                </tspan>
              ))}
            </text>
          );
        }
        const Tag = element.tag;
        return <Tag key={index} {...props} style={style} />;
      })}
    </>
  );
}

interface AnnotatorProps {
  request: AnnotateImageDetail;
  onClose: () => void;
}

/**
 * Editor delle annotazioni di un'immagine. Disegna sopra l'immagine forme
 * vettoriali salvate nella nota; l'immagine originale non viene modificata.
 */
function ImageAnnotator({ request, onClose }: AnnotatorProps) {
  const [url, setUrl] = useState<string | null>(null);
  const [aspect, setAspect] = useState(0);
  const [shapes, setShapes] = useState<Annotation[]>(request.annotations);
  const [undo, setUndo] = useState<Annotation[][]>([]);
  const [redo, setRedo] = useState<Annotation[][]>([]);
  const [tool, setTool] = useState<Tool>("pen");
  const [color, setColor] = useState(COLORS[0]);
  const [sizeLevel, setSizeLevel] = useState<0 | 1 | 2>(1);
  const [draft, setDraft] = useState<Annotation | null>(null);
  const [textAt, setTextAt] = useState<Point | null>(null);
  const [textValue, setTextValue] = useState("");
  const [error, setError] = useState<string | null>(null);
  const surface = useRef<SVGSVGElement>(null);
  const textInput = useRef<HTMLTextAreaElement>(null);
  const drawing = useRef(false);
  const dirty = JSON.stringify(shapes) !== JSON.stringify(request.annotations);

  useEffect(() => {
    if (textAt) window.requestAnimationFrame(() => textInput.current?.focus());
  }, [textAt]);

  useEffect(() => {
    void attachmentImageUrl(request.attachmentId)
      .then(setUrl)
      .catch(() => setError(t.imageUnavailable));
  }, [request.attachmentId]);

  const commit = useCallback(
    (next: Annotation[]) => {
      if (next.length > MAX_ANNOTATIONS) {
        setError(t.annotateTooMany(MAX_ANNOTATIONS));
        return;
      }
      setUndo((current) => [...current.slice(-99), shapes]);
      setRedo([]);
      setShapes(next);
    },
    [shapes],
  );

  const undoLast = useCallback(() => {
    setUndo((current) => {
      if (current.length === 0) return current;
      setRedo((future) => [shapes, ...future]);
      setShapes(current[current.length - 1]);
      return current.slice(0, -1);
    });
  }, [shapes]);

  const redoLast = useCallback(() => {
    setRedo((future) => {
      if (future.length === 0) return future;
      setUndo((current) => [...current, shapes]);
      setShapes(future[0]);
      return future.slice(1);
    });
  }, [shapes]);

  const close = useCallback(async () => {
    if (
      dirty &&
      !(await askConfirmation({
        title: t.annotateDiscardTitle,
        message: t.annotateDiscardMessage,
        confirmLabel: t.annotateDiscard,
        destructive: true,
      }))
    ) {
      return;
    }
    onClose();
  }, [dirty, onClose]);

  const save = () => {
    request.save(sanitizeAnnotations(shapes));
    onClose();
  };

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (textAt) return;
      const modifier = event.ctrlKey || event.metaKey;
      if (modifier && event.key.toLowerCase() === "z") {
        event.preventDefault();
        if (event.shiftKey) redoLast();
        else undoLast();
      } else if (modifier && event.key.toLowerCase() === "y") {
        event.preventDefault();
        redoLast();
      } else if (modifier && event.key === "Enter") {
        event.preventDefault();
        save();
      } else if (event.key === "Escape") {
        event.preventDefault();
        void close();
      }
    };
    window.addEventListener("keydown", onKeyDown, true);
    return () => window.removeEventListener("keydown", onKeyDown, true);
  });

  const height = aspect ? VIEW_WIDTH / aspect : VIEW_WIDTH;

  /** Punto relativo all'immagine (0–1). */
  const pointAt = (event: ReactPointerEvent): Point => {
    const rect = surface.current?.getBoundingClientRect();
    if (!rect || rect.width === 0 || rect.height === 0) return [0, 0];
    const clamp = (value: number) => Math.min(1, Math.max(0, value));
    return [clamp((event.clientX - rect.left) / rect.width), clamp((event.clientY - rect.top) / rect.height)];
  };

  const onPointerDown = (event: ReactPointerEvent<SVGSVGElement>) => {
    if (event.button !== 0 || textAt) return;
    const point = pointAt(event);
    if (tool === "eraser") {
      // Rimuove la forma più in alto sotto il puntatore.
      const index = [...shapes]
        .reverse()
        .findIndex((shape) => {
          const box = annotationBounds(shape, aspect || 1);
          return point[0] >= box.left && point[0] <= box.right && point[1] >= box.top && point[1] <= box.bottom;
        });
      if (index >= 0) commit(shapes.filter((_, position) => position !== shapes.length - 1 - index));
      return;
    }
    if (tool === "text") {
      // Senza l'azione predefinita il clic non toglie il focus al campo che
      // sta per comparire.
      event.preventDefault();
      setTextAt(point);
      setTextValue("");
      return;
    }
    event.currentTarget.setPointerCapture(event.pointerId);
    drawing.current = true;
    setDraft({ kind: tool, color, size: SIZES[tool][sizeLevel], points: [point, point] });
  };

  const onPointerMove = (event: ReactPointerEvent<SVGSVGElement>) => {
    if (!drawing.current || !draft) return;
    const point = pointAt(event);
    if (draft.kind === "pen" || draft.kind === "highlighter") {
      const last = draft.points[draft.points.length - 1];
      if (Math.hypot(point[0] - last[0], point[1] - last[1]) < MIN_STEP || draft.points.length >= 2000) return;
      setDraft({ ...draft, points: [...draft.points, point] });
    } else {
      setDraft({ ...draft, points: [draft.points[0], point] });
    }
  };

  const onPointerUp = () => {
    if (!drawing.current || !draft) return;
    drawing.current = false;
    const [first, last] = [draft.points[0], draft.points[draft.points.length - 1]];
    const tiny = Math.hypot(last[0] - first[0], last[1] - first[1]) < 0.005 && draft.points.length < 4;
    // Un clic con la penna lascia un punto; le forme troppo piccole no.
    if (!tiny || draft.kind === "pen" || draft.kind === "highlighter") {
      const shape =
        draft.kind === "pen" || draft.kind === "highlighter"
          ? { ...draft, points: draft.points.length > 2 ? draft.points.slice(1) : draft.points }
          : draft;
      commit([...shapes, shape]);
    }
    setDraft(null);
  };

  const addText = () => {
    const text = textValue.trim();
    if (textAt && text) {
      commit([...shapes, { kind: "text", color, size: SIZES.text[sizeLevel], points: [textAt], text: text.slice(0, 500) }]);
    }
    setTextAt(null);
    setTextValue("");
  };

  const tools: Tool[] = ["pen", "highlighter", "arrow", "rect", "ellipse", "text", "eraser"];
  return (
    <div className="dialog-backdrop annotator-backdrop">
      <section aria-label={t.annotateTitle} aria-modal="true" className="annotator" role="dialog">
        <header className="annotator-toolbar">
          <div className="annotator-group" role="radiogroup" aria-label={t.annotateTools}>
            {tools.map((item) => (
              <button
                aria-checked={tool === item}
                aria-label={toolLabel(item)}
                className={`annotator-tool${tool === item ? " selected" : ""}`}
                key={item}
                onClick={() => setTool(item)}
                role="radio"
                title={toolLabel(item)}
                type="button"
              >
                {TOOL_ICONS[item]}
              </button>
            ))}
          </div>
          <div className="annotator-group" role="radiogroup" aria-label={t.annotateColor}>
            {COLORS.map((value) => (
              <button
                aria-checked={color === value}
                aria-label={value}
                className={`annotator-color${color === value ? " selected" : ""}`}
                key={value}
                onClick={() => setColor(value)}
                role="radio"
                style={{ background: value }}
                type="button"
              />
            ))}
          </div>
          <div className="annotator-group" role="radiogroup" aria-label={t.annotateSize}>
            {([0, 1, 2] as const).map((level) => (
              <button
                aria-checked={sizeLevel === level}
                aria-label={[t.annotateThin, t.annotateMedium, t.annotateThick][level]}
                className={`annotator-tool${sizeLevel === level ? " selected" : ""}`}
                key={level}
                onClick={() => setSizeLevel(level)}
                role="radio"
                title={[t.annotateThin, t.annotateMedium, t.annotateThick][level]}
                type="button"
              >
                <span className="annotator-dot" style={{ width: 4 + level * 5, height: 4 + level * 5 }} />
              </button>
            ))}
          </div>
          <div className="annotator-group">
            <button className="annotator-tool" disabled={undo.length === 0} onClick={undoLast} title={t.undo} type="button">
              ↶
            </button>
            <button className="annotator-tool" disabled={redo.length === 0} onClick={redoLast} title={t.redo} type="button">
              ↷
            </button>
            <button
              className="annotator-text-button"
              disabled={shapes.length === 0}
              onClick={() => commit([])}
              type="button"
            >
              {t.annotateClear}
            </button>
          </div>
          <div className="annotator-actions">
            <button className="dialog-cancel-button" onClick={() => void close()} type="button">
              {t.cancel}
            </button>
            <button className="dialog-submit-button" onClick={save} type="button">
              {t.annotateSave}
            </button>
          </div>
        </header>
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
        <div className="annotator-stage">
          {url && (
            <div
              className="annotator-canvas"
              // L'immagine occupa lo spazio disponibile (anche se piccola),
              // con le sue proporzioni.
              style={
                aspect
                  ? { aspectRatio: `${aspect}`, width: `min(100%, calc((94vh - 190px) * ${aspect}))` }
                  : undefined
              }
            >
              <img
                alt={request.alt ?? ""}
                draggable={false}
                onLoad={(event) => {
                  const image = event.currentTarget;
                  if (image.naturalWidth && image.naturalHeight) setAspect(image.naturalWidth / image.naturalHeight);
                }}
                src={url}
              />
              <svg
                aria-label={t.annotateSurface}
                className={`annotator-surface tool-${tool}`}
                onPointerDown={onPointerDown}
                onPointerMove={onPointerMove}
                onPointerUp={onPointerUp}
                preserveAspectRatio="none"
                ref={surface}
                role="img"
                viewBox={`0 0 ${VIEW_WIDTH} ${height}`}
              >
                {shapes.map((shape, index) => (
                  <Shape height={height} key={index} shape={shape} />
                ))}
                {draft && <Shape height={height} shape={draft} />}
              </svg>
              {textAt && (
                <textarea
                  aria-label={t.annotateTextLabel}
                  className="annotator-text-input"
                  maxLength={500}
                  onBlur={addText}
                  onChange={(event) => setTextValue(event.target.value)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" && !event.shiftKey) {
                      event.preventDefault();
                      addText();
                    } else if (event.key === "Escape") {
                      event.preventDefault();
                      event.stopPropagation();
                      setTextAt(null);
                    }
                  }}
                  placeholder={t.annotateTextPlaceholder}
                  ref={textInput}
                  style={{ left: `${textAt[0] * 100}%`, top: `${textAt[1] * 100}%`, color }}
                  value={textValue}
                />
              )}
            </div>
          )}
        </div>
        <footer className="annotator-hint">{t.annotateHint}</footer>
      </section>
    </div>
  );
}

/** Apre l'editor quando un'immagine nel testo lo richiede. */
export default function ImageAnnotationHost() {
  const [request, setRequest] = useState<AnnotateImageDetail | null>(null);
  useEffect(() => {
    const open = (event: Event) => setRequest((event as CustomEvent<AnnotateImageDetail>).detail);
    window.addEventListener(ANNOTATE_IMAGE_EVENT, open);
    return () => window.removeEventListener(ANNOTATE_IMAGE_EVENT, open);
  }, []);
  if (!request) return null;
  return <ImageAnnotator key={request.attachmentId} onClose={() => setRequest(null)} request={request} />;
}
