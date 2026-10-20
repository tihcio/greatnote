import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { t } from "../../i18n";
import { askConfirmation } from "../../lib/confirm";
import { errorMessage, formatFileSize } from "../../lib/format";
import { MODEL_DOWNLOAD_FINISHED_EVENT, MODEL_DOWNLOAD_PROGRESS_EVENT, aiApi } from "./aiApi";
import type {
  AiSettings,
  BuiltinCatalog,
  BuiltinModel,
  BuiltinRole,
  ModelDownloadFinished,
  ModelDownloadProgress,
} from "./aiApi";

interface BuiltinModelsProps {
  draft: AiSettings;
  patch: (value: Partial<AiSettings>) => void;
}

type ModelField = "chat_model" | "embedding_model" | "vision_model";

const ROLES: { role: BuiltinRole; field: ModelField; optional: boolean }[] = [
  { role: "chat", field: "chat_model", optional: false },
  { role: "embedding", field: "embedding_model", optional: true },
  { role: "vision", field: "vision_model", optional: true },
];

function roleLabel(role: BuiltinRole): string {
  if (role === "chat") return t.aiBuiltinRoleChat;
  if (role === "embedding") return t.aiBuiltinRoleEmbedding;
  return t.aiBuiltinRoleVision;
}

function roleHint(role: BuiltinRole): string {
  if (role === "chat") return t.aiBuiltinRoleChatHint;
  if (role === "embedding") return t.aiBuiltinRoleEmbeddingHint;
  return t.aiBuiltinRoleVisionHint;
}

/** Modello da proporre per un ruolo: il consigliato se scaricato, altrimenti
 *  il primo scaricato. */
function proposal(models: BuiltinModel[], role: BuiltinRole): BuiltinModel | undefined {
  const installed = models.filter((model) => model.role === role && model.installed);
  return installed.find((model) => model.recommended) ?? installed[0];
}

/**
 * Motore AI integrato (ADR 0020): per chat, contesto (embedding) e visione
 * mostra i modelli del catalogo con dimensione, memoria e licenza; si
 * scaricano con conferma, avanzamento e interruzione, e si scelgono con i
 * pulsanti di opzione (salvati con le altre impostazioni).
 */
export default function BuiltinModels({ draft, patch }: BuiltinModelsProps) {
  const [catalog, setCatalog] = useState<BuiltinCatalog | null>(null);
  const [progress, setProgress] = useState<Record<string, ModelDownloadProgress>>({});
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** Modelli scaricati dall'ultimo aggiornamento del catalogo. */
  const justDownloaded = useRef<Set<string>>(new Set());

  const forget = useCallback((modelId: string) => {
    setProgress((current) => {
      const next = { ...current };
      delete next[modelId];
      return next;
    });
  }, []);

  const load = useCallback(async () => {
    try {
      setCatalog(await aiApi.builtinCatalog());
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }, []);

  useEffect(() => {
    void load();
    const stops: (() => void)[] = [];
    let cancelled = false;
    const keep = (registration: Promise<() => void>) =>
      void registration.then((stop) => (cancelled ? stop() : stops.push(stop)));
    keep(
      listen<ModelDownloadProgress>(MODEL_DOWNLOAD_PROGRESS_EVENT, (event) =>
        setProgress((current) => ({ ...current, [event.payload.model_id]: event.payload })),
      ),
    );
    keep(
      listen<ModelDownloadFinished>(MODEL_DOWNLOAD_FINISHED_EVENT, (event) => {
        const result = event.payload;
        forget(result.model_id);
        if (result.error) setError(result.error);
        else if (!result.cancelled) justDownloaded.current.add(result.model_id);
        void load();
      }),
    );
    return () => {
      cancelled = true;
      stops.forEach((stop) => stop());
    };
  }, [forget, load]);

  // Propone un modello scaricato per la chat se manca, per contesto e
  // visione appena ne viene scaricato uno (lasciandoli vuoti se l'utente ha
  // scelto "Nessuno"), e sostituisce i nomi rimasti da un altro provider.
  useEffect(() => {
    if (!catalog) return;
    const known = new Set(catalog.models.map((model) => model.id));
    const fresh = justDownloaded.current;
    justDownloaded.current = new Set();
    const proposed: Partial<AiSettings> = {};
    for (const { role, field, optional } of ROLES) {
      const current = draft[field];
      if (current && known.has(current)) continue;
      const model = proposal(catalog.models, role);
      const wanted = current !== "" || !optional || (model !== undefined && fresh.has(model.id));
      if (wanted && (model?.id ?? "") !== current) proposed[field] = model?.id ?? "";
    }
    if (Object.keys(proposed).length > 0) patch(proposed);
    // Solo quando cambia il catalogo (modello scaricato o eliminato).
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [catalog]);

  const run = async (action: () => Promise<void>) => {
    setError(null);
    setMessage(null);
    try {
      await action();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const startDownloads = async (models: BuiltinModel[]) => {
    for (const model of models) {
      setProgress((current) => ({
        ...current,
        [model.id]: { model_id: model.id, done: model.size_bytes - model.missing_bytes, total: model.size_bytes },
      }));
      try {
        await aiApi.downloadBuiltinModel(model.id);
      } catch (cause) {
        forget(model.id);
        throw cause;
      }
    }
  };

  const download = (model: BuiltinModel) =>
    run(async () => {
      const confirmed = await askConfirmation({
        title: t.aiBuiltinDownloadConfirmTitle,
        message: t.aiBuiltinDownloadConfirm(model.name, formatFileSize(model.missing_bytes)),
        confirmLabel: t.aiBuiltinDownload,
      });
      if (confirmed) await startDownloads([model]);
    });

  const downloadRecommended = (models: BuiltinModel[]) =>
    run(async () => {
      const total = models.reduce((sum, model) => sum + model.missing_bytes, 0);
      const confirmed = await askConfirmation({
        title: t.aiBuiltinDownloadConfirmTitle,
        message: t.aiBuiltinDownloadAllConfirm(
          models.map((model) => model.name).join(", "),
          formatFileSize(total),
        ),
        confirmLabel: t.aiBuiltinDownload,
      });
      if (confirmed) await startDownloads(models);
    });

  const remove = (model: BuiltinModel) =>
    run(async () => {
      const confirmed = await askConfirmation({
        title: t.aiBuiltinDelete,
        message: t.aiBuiltinDeleteConfirm(model.name),
        confirmLabel: t.aiBuiltinDelete,
        destructive: true,
      });
      if (!confirmed) return;
      setCatalog(await aiApi.deleteBuiltinModel(model.id));
      setMessage(t.aiBuiltinDeleted(model.name));
    });

  if (!catalog) {
    return (
      <fieldset>
        <legend>{t.aiBuiltinTitle}</legend>
        {error ? (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        ) : (
          <p className="data-hint">{t.aiLoading}</p>
        )}
      </fieldset>
    );
  }

  const missingRecommended = catalog.models.filter(
    (model) => model.recommended && !model.installed && !model.downloading && !progress[model.id],
  );
  const nothingInstalled = !catalog.models.some((model) => model.installed);

  return (
    <fieldset>
      <legend>{t.aiBuiltinTitle}</legend>
      <p className="data-hint">{t.aiBuiltinIntro}</p>
      {!catalog.engine_available && (
        <p className="dialog-error" role="alert">
          {t.aiBuiltinEngineMissing}
        </p>
      )}
      {catalog.engine_available && nothingInstalled && missingRecommended.length > 0 && (
        <div className="ai-builtin-suggest">
          <span>
            {t.aiBuiltinSuggest(
              formatFileSize(missingRecommended.reduce((sum, model) => sum + model.missing_bytes, 0)),
            )}
          </span>
          <button className="data-action primary" onClick={() => void downloadRecommended(missingRecommended)} type="button">
            {t.aiBuiltinDownloadRecommended}
          </button>
        </div>
      )}
      {ROLES.map(({ role, field, optional }) => (
        <div aria-label={roleLabel(role)} className="ai-builtin-group" key={role} role="radiogroup">
          <h4>{roleLabel(role)}</h4>
          <p className="data-hint">{roleHint(role)}</p>
          {catalog.models
            .filter((model) => model.role === role)
            .map((model) => {
              const current = progress[model.id];
              const downloading = Boolean(current) || model.downloading;
              const percent = current && current.total > 0 ? Math.floor((current.done / current.total) * 100) : 0;
              return (
                <div className="ai-builtin-model" key={model.id}>
                  <label className="checkbox-field">
                    <input
                      checked={draft[field] === model.id}
                      disabled={!model.installed}
                      name={`builtin-${role}`}
                      onChange={() => patch({ [field]: model.id } as Partial<AiSettings>)}
                      type="radio"
                    />
                    <span>
                      <strong>{model.name}</strong>
                      {model.recommended && <span className="ai-badge">{t.aiBuiltinRecommended}</span>}
                      <small className="data-hint ai-builtin-meta">
                        {t.aiBuiltinMeta(formatFileSize(model.size_bytes), model.memory_gb, model.license)}
                      </small>
                      <small className="data-hint ai-builtin-meta">{t.aiBuiltinDescription(model.id)}</small>
                    </span>
                  </label>
                  <div className="ai-inline-actions">
                    {downloading ? (
                      <>
                        <progress aria-label={model.name} max={100} value={percent} />
                        <span className="data-hint" role="status">
                          {t.aiBuiltinDownloading(percent)}
                        </span>
                        <button
                          className="data-action"
                          onClick={() => void run(() => aiApi.cancelBuiltinDownload(model.id))}
                          type="button"
                        >
                          {t.aiBuiltinCancel}
                        </button>
                      </>
                    ) : model.installed ? (
                      <button className="data-action destructive" onClick={() => void remove(model)} type="button">
                        {t.aiBuiltinDelete}
                      </button>
                    ) : (
                      <button
                        className="data-action"
                        disabled={!catalog.engine_available}
                        onClick={() => void download(model)}
                        type="button"
                      >
                        {t.aiBuiltinDownloadSize(formatFileSize(model.missing_bytes))}
                      </button>
                    )}
                  </div>
                </div>
              );
            })}
          {optional && (
            <label className="checkbox-field">
              <input
                checked={!draft[field]}
                name={`builtin-${role}`}
                onChange={() => patch({ [field]: "" } as Partial<AiSettings>)}
                type="radio"
              />
              <span>{t.aiBuiltinNone}</span>
            </label>
          )}
        </div>
      ))}
      <p className="data-hint">{t.aiBuiltinStorage(formatFileSize(catalog.used_bytes), catalog.models_directory)}</p>
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
