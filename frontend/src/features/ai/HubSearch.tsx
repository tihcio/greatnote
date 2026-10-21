import { useCallback, useEffect, useState } from "react";
import type { FormEvent } from "react";
import { t } from "../../i18n";
import { askConfirmation } from "../../lib/confirm";
import { errorMessage, formatFileSize } from "../../lib/format";
import { aiApi, variantSize } from "./aiApi";
import type { BuiltinRole, HubRepo, HubRepoDetails, HubVariant, ModelFit } from "./aiApi";

interface HubSearchProps {
  role: BuiltinRole;
  roleLabel: string;
  /** Modelli già nell'elenco (curati o registrati). */
  knownIds: Set<string>;
  engineAvailable: boolean;
  /** Il download è partito: il catalogo va riletto. */
  onDownloadStarted: (modelId: string, total: number) => void;
  onClose: () => void;
}

const EMBEDDING_PIPELINES = ["feature-extraction", "sentence-similarity"];

export function FitBadge({ fit }: { fit: ModelFit }) {
  return (
    <span className={`ai-fit ai-fit-${fit}`} title={t.aiBuiltinFitHint(fit)}>
      {t.aiBuiltinFit(fit)}
    </span>
  );
}

/**
 * Ricerca dei modelli GGUF su Hugging Face per un ruolo (ADR 0021): elenco
 * dei repository più scaricati, varianti con dimensione e adattezza a questo
 * computer, download con conferma. Le richieste partono solo dopo che
 * l'utente ha aperto la ricerca.
 */
export default function HubSearch({
  role,
  roleLabel,
  knownIds,
  engineAvailable,
  onDownloadStarted,
  onClose,
}: HubSearchProps) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<HubRepo[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [details, setDetails] = useState<Record<string, HubRepoDetails>>({});
  const [loadingRepo, setLoadingRepo] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const search = useCallback(
    async (text: string) => {
      setSearching(true);
      setError(null);
      setMessage(null);
      setExpanded(null);
      try {
        setResults(await aiApi.searchHubModels(text, role));
      } catch (cause) {
        setResults(null);
        setError(errorMessage(cause));
      } finally {
        setSearching(false);
      }
    },
    [role],
  );

  // All'apertura: i modelli più scaricati per il ruolo.
  useEffect(() => {
    void search("");
  }, [search]);

  const submit = (event: FormEvent) => {
    event.preventDefault();
    void search(query);
  };

  const toggle = async (repo: HubRepo) => {
    if (expanded === repo.id) {
      setExpanded(null);
      return;
    }
    setExpanded(repo.id);
    if (details[repo.id]) return;
    setLoadingRepo(repo.id);
    setError(null);
    try {
      const loaded = await aiApi.hubModelDetails(repo.id, role);
      setDetails((current) => ({ ...current, [repo.id]: loaded }));
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setLoadingRepo(null);
    }
  };

  const download = async (repo: HubRepoDetails, variant: HubVariant) => {
    setError(null);
    setMessage(null);
    const size = variantSize(variant);
    const fitLabel = t.aiBuiltinFit(variant.fit).toLowerCase();
    const confirmed = await askConfirmation({
      title: t.aiBuiltinDownloadConfirmTitle,
      message:
        t.aiBuiltinHubDownloadConfirm(
          variant.model.name,
          formatFileSize(size),
          variant.model.license,
          fitLabel,
        ) + (variant.fit === "too-large" ? t.aiBuiltinHubTooLargeWarning : ""),
      confirmLabel: t.aiBuiltinDownload,
    });
    if (!confirmed) return;
    try {
      await aiApi.downloadHubModel(repo.repo, role, variant.model.id);
      onDownloadStarted(variant.model.id, size);
      setMessage(t.aiBuiltinHubStarted(variant.model.name));
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  return (
    <section aria-label={t.aiBuiltinHubTitle(roleLabel)} className="ai-hub">
      <div className="ai-hub-header">
        <h5>{t.aiBuiltinHubTitle(roleLabel)}</h5>
        <button className="data-action" onClick={onClose} type="button">
          {t.aiBuiltinHubClose}
        </button>
      </div>
      <p className="data-hint">{t.aiBuiltinHubPrivacy}</p>
      <form className="ai-hub-search" onSubmit={submit} role="search">
        <label className="dialog-field">
          <span>{t.aiBuiltinHubQuery}</span>
          <input
            maxLength={100}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t.aiBuiltinHubPlaceholder}
            spellCheck={false}
            type="search"
            value={query}
          />
        </label>
        <button className="data-action primary" disabled={searching} type="submit">
          {t.aiBuiltinHubSearch}
        </button>
      </form>

      {searching && (
        <p className="data-hint" role="status">
          {t.aiBuiltinHubSearching}
        </p>
      )}
      {!searching && results?.length === 0 && <p className="data-hint">{t.aiBuiltinHubNoResults}</p>}

      {!searching && results && results.length > 0 && (
        <ul className="ai-hub-results">
          {results.map((repo) => {
            const open = expanded === repo.id;
            const loaded = details[repo.id];
            return (
              <li key={repo.id}>
                <div className="ai-hub-repo">
                  <span>
                    <strong>{repo.id}</strong>
                    {repo.architecture && <span className="ai-badge">{repo.architecture}</span>}
                    <small className="data-hint ai-builtin-meta">
                      {t.aiBuiltinHubRepoMeta(repo.downloads, repo.likes, repo.license)}
                    </small>
                    {repo.gated && <small className="data-hint ai-builtin-meta">{t.aiBuiltinHubGated}</small>}
                  </span>
                  {!repo.gated && (
                    <button
                      aria-expanded={open}
                      className="data-action"
                      onClick={() => void toggle(repo)}
                      type="button"
                    >
                      {open ? t.aiBuiltinHubHideVariants : t.aiBuiltinHubShowVariants}
                    </button>
                  )}
                </div>
                {open && loadingRepo === repo.id && (
                  <p className="data-hint" role="status">
                    {t.aiBuiltinHubLoadingVariants}
                  </p>
                )}
                {open && loaded && renderVariants(loaded)}
              </li>
            );
          })}
        </ul>
      )}

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
    </section>
  );

  function renderVariants(repo: HubRepoDetails) {
    if (role === "vision" && !repo.has_projector) {
      return <p className="data-hint">{t.aiBuiltinHubNoProjector}</p>;
    }
    if (repo.variants.length === 0) {
      return <p className="data-hint">{t.aiBuiltinHubNoVariants}</p>;
    }
    const notEmbedding =
      role === "embedding" && !EMBEDDING_PIPELINES.includes(repo.pipeline_tag ?? "");
    return (
      <div className="ai-hub-variants">
        {notEmbedding && <p className="ai-remote-warning">{t.aiBuiltinHubNotEmbedding}</p>}
        {repo.variants.map((variant, index) => {
          const added = knownIds.has(variant.model.id);
          return (
            <div className="ai-builtin-model" key={variant.model.id}>
              <span>
                <strong>{variant.quantization ?? variant.model.name}</strong>
                {repo.recommended === index && (
                  <span className="ai-badge">{t.aiBuiltinHubRecommendedForYou}</span>
                )}
                <small className="data-hint ai-builtin-meta">
                  {t.aiBuiltinHubVariantMeta(formatFileSize(variantSize(variant)), variant.model.memory_gb)}{" "}
                  <FitBadge fit={variant.fit} />
                </small>
              </span>
              <div className="ai-inline-actions">
                {added ? (
                  <span className="data-hint">{t.aiBuiltinHubAlreadyAdded}</span>
                ) : (
                  <button
                    className="data-action"
                    disabled={!engineAvailable}
                    onClick={() => void download(repo, variant)}
                    type="button"
                  >
                    {t.aiBuiltinDownloadSize(formatFileSize(variantSize(variant)))}
                  </button>
                )}
              </div>
            </div>
          );
        })}
      </div>
    );
  }
}
