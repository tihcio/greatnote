import { useCallback, useEffect, useState } from "react";
import { t } from "../../i18n";
import { askConfirmation } from "../../lib/confirm";
import { errorMessage } from "../../lib/format";
import type { Notebook } from "../../types";
import { aiApi } from "./aiApi";
import BuiltinModels from "./BuiltinModels";
import ImageTextSettings from "./ImageTextSettings";
import type { AiSettings, AiStatus, ConnectionTest, IndexProgress } from "./aiApi";

interface AiSettingsSectionProps {
  status: AiStatus | null;
  progress: IndexProgress | null;
  notebooks: Notebook[];
  onStatus: (status: AiStatus) => void;
  onStartIndexing: () => void;
  onCancelIndexing: () => void;
}

/** Alias dei modelli accettati da Claude Code ("default": quello dell'account). */
const CLAUDE_MODELS = ["default", "sonnet", "opus", "haiku", "fable"];

const PRESETS = [
  { id: "ollama", label: "Ollama", url: "http://localhost:11434/v1" },
  { id: "openwebui", label: "Open WebUI", url: "http://localhost:3000/api" },
  { id: "lmstudio", label: "LM Studio", url: "http://localhost:1234/v1" },
  { id: "llamacpp", label: "llama.cpp server", url: "http://localhost:8080/v1" },
];

function isLoopback(url: string): boolean {
  const match = /^https?:\/\/(\[[^\]]+\]|[^/:]+)/i.exec(url.trim());
  if (!match) return false;
  const host = match[1].toLowerCase();
  return host === "localhost" || host.endsWith(".localhost") || host.startsWith("127.") || host === "[::1]";
}

/**
 * Impostazioni AI: tutto spento per impostazione predefinita. Le modifiche
 * si salvano con "Salva" (il server e i modelli vanno provati prima), la
 * chiave API va nel portachiavi di sistema e non torna mai all'interfaccia.
 */
export default function AiSettingsSection({
  status,
  progress,
  notebooks,
  onStatus,
  onStartIndexing,
  onCancelIndexing,
}: AiSettingsSectionProps) {
  const [draft, setDraft] = useState<AiSettings | null>(status?.settings ?? null);
  const [test, setTest] = useState<ConnectionTest | null>(null);
  const [testing, setTesting] = useState(false);
  const [saving, setSaving] = useState(false);
  const [apiKey, setApiKey] = useState("");
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [loadError, setLoadError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoadError(null);
    try {
      onStatus(await aiApi.status());
    } catch (cause) {
      setLoadError(errorMessage(cause));
    }
  }, [onStatus]);

  // Lo stato arriva dall'app; se manca (errore all'avvio) lo si richiede qui,
  // così un errore è visibile nel dialog e non resta nascosto dietro.
  useEffect(() => {
    if (!status) void load();
    // Solo all'apertura della sezione.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!draft && status) setDraft(status.settings);
  }, [draft, status]);

  if (!draft || !status) {
    return loadError ? (
      <div className="ai-settings">
        <p className="dialog-error" role="alert">
          {t.aiStatusError(loadError)}
        </p>
        <button className="data-action" onClick={() => void load()} type="button">
          {t.retry}
        </button>
      </div>
    ) : (
      <p className="data-hint">{t.aiLoading}</p>
    );
  }

  const remote =
    draft.provider === "claude-code" || (draft.provider === "openai-compatible" && !isLoopback(draft.base_url));
  const changed = JSON.stringify(draft) !== JSON.stringify(status.settings) || apiKey.trim() !== "";
  const openWebUi = /:3000\/api$/.test(draft.base_url.trim().replace(/\/+$/, ""));
  const patch = (value: Partial<AiSettings>) => {
    setDraft({ ...draft, ...value });
    setMessage(null);
  };

  const run = async (action: () => Promise<void>) => {
    setError(null);
    setMessage(null);
    try {
      await action();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const save = () =>
    run(async () => {
      if (remote && draft.enabled && !draft.remote_consent) {
        throw new Error(t.aiConsentRequired);
      }
      setSaving(true);
      try {
        // Una chiave digitata viene salvata insieme alle impostazioni.
        if (apiKey.trim()) {
          await aiApi.setApiKey(apiKey);
          setApiKey("");
        }
        const next = await aiApi.updateSettings(draft);
        onStatus(next);
        setDraft(next.settings);
        setMessage(t.aiSettingsSaved);
      } finally {
        setSaving(false);
      }
    });

  const testConnection = () =>
    run(async () => {
      setTesting(true);
      setTest(null);
      try {
        const result = await aiApi.testConnection(draft, apiKey.trim() || null);
        setTest(result);
        setMessage(t.aiConnectionOk(result.models.length));
      } finally {
        setTesting(false);
      }
    });

  const saveKey = () =>
    run(async () => {
      const next = await aiApi.setApiKey(apiKey);
      setApiKey("");
      onStatus(next);
      setDraft({ ...draft, use_api_key: next.settings.use_api_key });
      setMessage(t.aiKeySaved);
    });

  const deleteKey = () =>
    run(async () => {
      const next = await aiApi.deleteApiKey();
      onStatus(next);
      setDraft({ ...draft, use_api_key: false });
      setMessage(t.aiKeyDeleted);
    });

  const clearIndex = () =>
    run(async () => {
      const confirmed = await askConfirmation({
        title: t.aiClearIndex,
        message: t.aiClearIndexConfirm,
        confirmLabel: t.aiClearIndex,
        destructive: true,
      });
      if (!confirmed) return;
      onStatus(await aiApi.clearIndex());
      setMessage(t.aiIndexCleared);
    });

  const clearHistory = () =>
    run(async () => {
      const confirmed = await askConfirmation({
        title: t.aiHistoryClear,
        message: t.aiHistoryClearConfirm,
        confirmLabel: t.aiHistoryClear,
        destructive: true,
      });
      if (!confirmed) return;
      await aiApi.clearHistory();
      setMessage(t.aiHistoryCleared);
    });

  const modelOptions = test?.models ?? [];
  const modelField = (key: "chat_model" | "embedding_model", label: string, hint: string) => (
    <label className="dialog-field">
      <span>{label}</span>
      <input
        list={modelOptions.length > 0 ? `ai-models-${key}` : undefined}
        maxLength={200}
        onChange={(event) => patch({ [key]: event.target.value } as Partial<AiSettings>)}
        placeholder={key === "chat_model" ? "qwen3:8b" : "nomic-embed-text"}
        value={draft[key]}
      />
      {modelOptions.length > 0 && (
        <datalist id={`ai-models-${key}`}>
          {modelOptions.map((model) => (
            <option key={model} value={model} />
          ))}
        </datalist>
      )}
      <small className="data-hint">{hint}</small>
    </label>
  );

  const indexing = Boolean(status.indexing || progress);

  return (
    <div className="ai-settings">
      {changed && (
        <div className="ai-unsaved" role="status">
          <span>{t.aiUnsavedChanges}</span>
          <button className="data-action primary" disabled={saving} onClick={() => void save()} type="button">
            {t.aiSaveSettings}
          </button>
        </div>
      )}
      <fieldset>
        <legend>{t.aiSettingsTitle}</legend>
        <label className="checkbox-field">
          <input checked={draft.enabled} onChange={(event) => patch({ enabled: event.target.checked })} type="checkbox" />
          <span>{t.aiEnable}</span>
        </label>
        <p className="data-hint">{t.aiPrivacyNote}</p>
      </fieldset>

      <fieldset>
        <legend>{t.aiProvider}</legend>
        <div className="segmented ai-provider-switch" role="radiogroup" aria-label={t.aiProvider}>
          <button
            aria-checked={draft.provider === "builtin"}
            className={`segment${draft.provider === "builtin" ? " selected" : ""}`}
            onClick={() => {
              setTest(null);
              // I nomi dei modelli degli altri provider non valgono qui: il
              // pannello del motore integrato propone quelli scaricati.
              patch({ provider: "builtin", chat_model: "", embedding_model: "", vision_model: "" });
            }}
            role="radio"
            type="button"
          >
            {t.aiProviderBuiltin}
          </button>
          <button
            aria-checked={draft.provider === "openai-compatible"}
            className={`segment${draft.provider === "openai-compatible" ? " selected" : ""}`}
            onClick={() => {
              setTest(null);
              patch(
                draft.provider === "builtin"
                  ? { provider: "openai-compatible", chat_model: "", embedding_model: "", vision_model: "" }
                  : { provider: "openai-compatible" },
              );
            }}
            role="radio"
            type="button"
          >
            {t.aiProviderOpenai}
          </button>
          <button
            aria-checked={draft.provider === "claude-code"}
            className={`segment${draft.provider === "claude-code" ? " selected" : ""}`}
            onClick={() => {
              setTest(null);
              patch({ provider: "claude-code", chat_model: CLAUDE_MODELS.includes(draft.chat_model) ? draft.chat_model : "default" });
            }}
            role="radio"
            type="button"
          >
            Claude Code
          </button>
        </div>
      </fieldset>

      {draft.provider === "builtin" ? (
        <BuiltinModels draft={draft} patch={patch} />
      ) : draft.provider === "claude-code" ? (
        <>
      <fieldset>
        <legend>Claude Code</legend>
        <p className="data-hint">{t.aiClaudeIntro}</p>
        <label className="dialog-field">
          <span>{t.aiClaudePath}</span>
          <input
            maxLength={500}
            onChange={(event) => patch({ claude_path: event.target.value })}
            placeholder={t.aiClaudePathPlaceholder}
            spellCheck={false}
            value={draft.claude_path}
          />
        </label>
        <label className="dialog-field">
          <span>{t.aiChatModel}</span>
          <select onChange={(event) => patch({ chat_model: event.target.value })} value={draft.chat_model || "default"}>
            {CLAUDE_MODELS.map((model) => (
              <option key={model} value={model}>
                {model === "default" ? t.aiClaudeDefaultModel : model}
              </option>
            ))}
          </select>
        </label>
        <label className="checkbox-field ai-consent">
          <input
            checked={draft.remote_consent}
            onChange={(event) => patch({ remote_consent: event.target.checked })}
            type="checkbox"
          />
          <span>{t.aiClaudeConsent}</span>
        </label>
        {draft.remote_consent && (
          <label className="checkbox-field ai-consent">
            <input
              checked={draft.confirm_remote_sends}
              onChange={(event) => patch({ confirm_remote_sends: event.target.checked })}
              type="checkbox"
            />
            <span>{t.aiConfirmEachSend}</span>
          </label>
        )}
        <div className="ai-inline-actions">
          <button className="data-action" disabled={testing} onClick={() => void testConnection()} type="button">
            {testing ? t.aiTesting : t.aiClaudeTest}
          </button>
        </div>
        {test?.claude_code && (
          <p className="data-hint" role="status">
            {t.aiClaudeStatus(test.claude_code.version, test.claude_code.auth_method ?? "—", test.claude_code.executable)}
          </p>
        )}
        <p className="data-hint">{t.aiClaudeSafety}</p>
      </fieldset>

        </>
      ) : (
        <>
      <fieldset>
        <legend>{t.aiServer}</legend>
        <div className="ai-presets">
          {PRESETS.map((preset) => (
            <button
              aria-pressed={draft.base_url === preset.url}
              className={`segment${draft.base_url === preset.url ? " selected" : ""}`}
              key={preset.id}
              onClick={() => patch({ base_url: preset.url })}
              type="button"
            >
              {preset.label}
            </button>
          ))}
        </div>
        <label className="dialog-field">
          <span>{t.aiServerUrl}</span>
          <input
            maxLength={500}
            onChange={(event) => patch({ base_url: event.target.value })}
            placeholder="http://localhost:11434/v1"
            spellCheck={false}
            value={draft.base_url}
          />
          <small className="data-hint">{remote ? t.aiServerRemoteHint : t.aiServerLocalHint}</small>
          {openWebUi && <small className="data-hint">{t.aiOpenWebUiHint}</small>}
        </label>
        {remote && (
          <label className="checkbox-field ai-consent">
            <input
              checked={draft.remote_consent}
              onChange={(event) => patch({ remote_consent: event.target.checked })}
              type="checkbox"
            />
            <span>{t.aiRemoteConsent}</span>
          </label>
        )}
        {draft.remote_consent && (
          <label className="checkbox-field ai-consent">
            <input
              checked={draft.confirm_remote_sends}
              onChange={(event) => patch({ confirm_remote_sends: event.target.checked })}
              type="checkbox"
            />
            <span>{t.aiConfirmEachSend}</span>
          </label>
        )}
        <div className="ai-inline-actions">
          <button className="data-action" disabled={testing} onClick={() => void testConnection()} type="button">
            {testing ? t.aiTesting : t.aiTestConnection}
          </button>
        </div>
        {test && test.models.length > 0 && (
          <p className="data-hint">{t.aiAvailableModels(test.models.slice(0, 12).join(", "))}</p>
        )}
        {modelField("chat_model", t.aiChatModel, t.aiChatModelHint)}
        {modelField("embedding_model", t.aiEmbeddingModel, t.aiEmbeddingModelHint)}
      </fieldset>

      <fieldset>
        <legend>{t.aiApiKey}</legend>
        <p className="data-hint">{draft.use_api_key ? t.aiKeyStored : t.aiKeyNotStored}</p>
        <div className="ai-inline-actions">
          <input
            aria-label={t.aiApiKey}
            autoComplete="off"
            className="ai-key-input"
            maxLength={4096}
            onChange={(event) => setApiKey(event.target.value)}
            placeholder={t.aiApiKeyPlaceholder}
            spellCheck={false}
            type="password"
            value={apiKey}
          />
          <button className="data-action" disabled={!apiKey.trim()} onClick={() => void saveKey()} type="button">
            {t.aiSaveKey}
          </button>
          {draft.use_api_key && (
            <button className="data-action destructive" onClick={() => void deleteKey()} type="button">
              {t.aiDeleteKey}
            </button>
          )}
        </div>
      </fieldset>

        </>
      )}

      <fieldset>
        <legend>{t.aiPrivacy}</legend>
        <p className="data-hint">{t.aiExcludedHint}</p>
        {notebooks.length === 0 ? (
          <p className="data-hint">{t.aiNoNotebooks}</p>
        ) : (
          <div className="ai-excluded-list">
            {notebooks.map((notebook) => (
              <label className="checkbox-field" key={notebook.id}>
                <input
                  checked={draft.excluded_notebook_ids.includes(notebook.id)}
                  onChange={(event) =>
                    patch({
                      excluded_notebook_ids: event.target.checked
                        ? [...draft.excluded_notebook_ids, notebook.id]
                        : draft.excluded_notebook_ids.filter((id) => id !== notebook.id),
                    })
                  }
                  type="checkbox"
                />
                <span>{notebook.name}</span>
              </label>
            ))}
          </div>
        )}
        <label className="dialog-field">
          <span>{t.aiMaxContext}</span>
          <input
            max={60000}
            min={2000}
            onChange={(event) => patch({ max_context_chars: Number(event.target.value) })}
            step={1000}
            type="number"
            value={draft.max_context_chars}
          />
        </label>
        <label className="dialog-field">
          <span>{t.aiTimeout}</span>
          <input
            max={600}
            min={10}
            onChange={(event) => patch({ timeout_seconds: Number(event.target.value) })}
            type="number"
            value={draft.timeout_seconds}
          />
        </label>
      </fieldset>

      <ImageTextSettings draft={draft} patch={patch} saved={status.settings} />

      <fieldset>
        <legend>{t.aiHistoryTab}</legend>
        <label className="checkbox-field">
          <input
            checked={draft.keep_history}
            onChange={(event) => patch({ keep_history: event.target.checked })}
            type="checkbox"
          />
          <span>{t.aiKeepHistory}</span>
        </label>
        <p className="data-hint">{t.aiKeepHistoryHint}</p>
        <div className="ai-inline-actions">
          <button className="data-action destructive" onClick={() => void clearHistory()} type="button">
            {t.aiHistoryClear}
          </button>
        </div>
      </fieldset>

      <fieldset>
        <legend>{t.aiSemanticIndex}</legend>
        {status.settings.embedding_model ? (
          <>
            <p className="data-hint" role="status">
              {indexing && progress
                ? t.aiIndexProgress(progress.done, progress.total)
                : t.aiIndexSummary(status.index.indexed_notes, status.index.pending_notes)}
            </p>
            <label className="checkbox-field">
              <input
                checked={draft.auto_index}
                onChange={(event) => patch({ auto_index: event.target.checked })}
                type="checkbox"
              />
              <span>{t.aiAutoIndex}</span>
            </label>
            {remote && draft.auto_index && <p className="data-hint">{t.aiAutoIndexRemoteHint}</p>}
            <div className="ai-inline-actions">
              {indexing ? (
                <button className="data-action" onClick={onCancelIndexing} type="button">
                  {t.aiStop}
                </button>
              ) : (
                <button
                  className="data-action"
                  disabled={!status.settings.enabled || changed}
                  onClick={onStartIndexing}
                  type="button"
                >
                  {t.aiUpdateIndex}
                </button>
              )}
              <button className="data-action destructive" disabled={indexing} onClick={() => void clearIndex()} type="button">
                {t.aiClearIndex}
              </button>
            </div>
          </>
        ) : (
          <p className="data-hint">
            {draft.provider === "claude-code" ? t.aiClaudeNoEmbeddings : t.aiNoEmbeddingModel}
          </p>
        )}
      </fieldset>

      {error && (
        <p className="dialog-error" role="alert">
          {error}
        </p>
      )}
      {message && (
        <p className="data-hint" role="status">
          {message}
        </p>
      )}
      <div className="ai-inline-actions ai-save-row">
        <button className="data-action primary" disabled={!changed || saving} onClick={() => void save()} type="button">
          {t.aiSaveSettings}
        </button>
        {changed && (
          <button
            className="data-action"
            onClick={() => {
              setDraft(status.settings);
              setApiKey("");
            }}
            type="button"
          >
            {t.aiDiscardChanges}
          </button>
        )}
      </div>
    </div>
  );
}
