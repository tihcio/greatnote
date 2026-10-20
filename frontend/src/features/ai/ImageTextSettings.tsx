import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { t } from "../../i18n";
import { errorMessage } from "../../lib/format";
import { IMAGE_TEXT_FINISHED_EVENT, IMAGE_TEXT_PROGRESS_EVENT, aiApi } from "./aiApi";
import type { AiSettings, ImageTextCapabilities, ImageTextFinished, IndexProgress } from "./aiApi";

interface ImageTextSettingsProps {
  draft: AiSettings;
  /** Impostazioni salvate: la lettura di tutte le immagini usa queste. */
  saved: AiSettings;
  patch: (value: Partial<AiSettings>) => void;
}

/**
 * Lettura del testo nelle immagini: motore (modello AI con visione o
 * Tesseract locale), lettura automatica degli allegati e lettura di tutte le
 * immagini già presenti, con avanzamento e annullamento.
 */
export default function ImageTextSettings({ draft, saved, patch }: ImageTextSettingsProps) {
  const [capabilities, setCapabilities] = useState<ImageTextCapabilities | null>(null);
  const [progress, setProgress] = useState<IndexProgress | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    void aiApi
      .imageTextCapabilities()
      .then(setCapabilities)
      .catch((cause: unknown) => setError(errorMessage(cause)));
  }, []);

  useEffect(() => {
    load();
    const stops: (() => void)[] = [];
    let cancelled = false;
    const keep = (registration: Promise<() => void>) =>
      void registration.then((stop) => (cancelled ? stop() : stops.push(stop)));
    keep(listen<IndexProgress>(IMAGE_TEXT_PROGRESS_EVENT, (event) => setProgress(event.payload)));
    keep(
      listen<ImageTextFinished>(IMAGE_TEXT_FINISHED_EVENT, (event) => {
        setProgress(null);
        const result = event.payload;
        if (result.error) setError(result.error);
        else setMessage(t.imageTextBatchDone(result.read, result.failed, result.skipped, result.cancelled));
        load();
      }),
    );
    return () => {
      cancelled = true;
      stops.forEach((stop) => stop());
    };
  }, [load]);

  const startAll = async () => {
    setError(null);
    setMessage(null);
    try {
      await aiApi.startImageTextAll(true);
      setProgress({ done: 0, total: capabilities?.pending_images ?? 0 });
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const tesseract = capabilities?.tesseract ?? null;
  const unsaved =
    draft.ocr_engine !== saved.ocr_engine ||
    draft.vision_model !== saved.vision_model ||
    draft.vision_max_side !== saved.vision_max_side;
  return (
    <fieldset>
      <legend>{t.imageTextTitle}</legend>
      <p className="data-hint">{t.imageTextIntro}</p>
      <div className="settings-radios" role="radiogroup" aria-label={t.imageTextEngine}>
        <label className="checkbox-field">
          <input
            checked={draft.ocr_engine === "ai"}
            name="ocr-engine"
            onChange={() => patch({ ocr_engine: "ai" })}
            type="radio"
          />
          <span>{t.imageTextEngineAi}</span>
        </label>
        <label className="checkbox-field">
          <input
            checked={draft.ocr_engine === "tesseract"}
            disabled={!tesseract && draft.ocr_engine !== "tesseract"}
            name="ocr-engine"
            onChange={() => patch({ ocr_engine: "tesseract" })}
            type="radio"
          />
          <span>
            {tesseract
              ? t.imageTextEngineTesseract(tesseract.version, tesseract.languages.join(", "))
              : t.imageTextTesseractMissing}
          </span>
        </label>
      </div>
      {draft.ocr_engine === "ai" && draft.provider === "openai-compatible" && (
        <label className="dialog-field">
          <span>{t.imageTextVisionModel}</span>
          <input
            maxLength={200}
            onChange={(event) => patch({ vision_model: event.target.value })}
            placeholder={draft.chat_model || "qwen2.5vl:7b"}
            value={draft.vision_model}
          />
          <small className="data-hint">{t.imageTextVisionHint}</small>
        </label>
      )}
      {draft.ocr_engine === "ai" && (
        <label className="dialog-field">
          <span>{t.imageTextMaxSide}</span>
          <select
            onChange={(event) => patch({ vision_max_side: Number(event.target.value) })}
            value={draft.vision_max_side}
          >
            {[512, 768, 1024, 1536, 2048].map((side) => (
              <option key={side} value={side}>
                {t.imageTextSide(side)}
              </option>
            ))}
          </select>
          <small className="data-hint">{t.imageTextMaxSideHint}</small>
        </label>
      )}
      <label className="checkbox-field">
        <input
          checked={draft.ocr_on_attach}
          onChange={(event) => patch({ ocr_on_attach: event.target.checked })}
          type="checkbox"
        />
        <span>{t.imageTextOnAttach}</span>
      </label>
      <div className="ai-inline-actions">
        {progress ? (
          <>
            <span className="data-hint" role="status">
              {t.imageTextProgress(progress.done, progress.total)}
            </span>
            <button className="data-action" onClick={() => void aiApi.cancelImageTextAll()} type="button">
              {t.aiStop}
            </button>
          </>
        ) : (
          <button
            className="data-action"
            disabled={unsaved || !capabilities || capabilities.pending_images === 0}
            onClick={() => void startAll()}
            title={unsaved ? t.aiUnsavedChanges : undefined}
            type="button"
          >
            {t.imageTextReadAll(capabilities?.pending_images ?? 0)}
          </button>
        )}
      </div>
      {message && (
        <p className="data-hint" role="status">
          {message}
        </p>
      )}
      {error && (
        <p className="dialog-error" role="alert">
          {error}
        </p>
      )}
    </fieldset>
  );
}
