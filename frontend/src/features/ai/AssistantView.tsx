import { useEffect, useRef, useState } from "react";
import type { FormEvent, ReactNode } from "react";
import { t } from "../../i18n";
import Icon from "../../components/Icon";
import AiHistoryPanel from "./AiHistoryPanel";
import { errorMessage } from "../../lib/format";
import { aiApi, aiReady, newRequestId } from "./aiApi";
import type { AiStatus, ChatMessage, ContextSource, IndexProgress, PreparedContext, Provenance, SemanticResult } from "./aiApi";

interface Turn {
  question: string;
  answer: string;
  sources: ContextSource[];
  provenance: Provenance | null;
  warning: string | null;
  error: string | null;
  streaming: boolean;
}

interface PendingSend {
  question: string;
  prepared: PreparedContext;
  selected: boolean[];
}

export interface AssistantContextNote {
  id: string;
  title: string;
}

interface AssistantViewProps {
  status: AiStatus | null;
  progress: IndexProgress | null;
  contextNote: AssistantContextNote | null;
  onClearContextNote: () => void;
  onOpenNote: (noteId: string) => void;
  onOpenSettings: () => void;
  onStartIndexing: () => void;
  onCancelIndexing: () => void;
  onStatus: (status: AiStatus) => void;
}

const METHOD_LABEL = () => ({
  semantic: t.aiMethodSemantic,
  keyword: t.aiMethodKeyword,
  selected: t.aiMethodSelected,
});

/** Testo della risposta con le citazioni `[n]` trasformate in collegamenti
 *  alle note sorgente (solo i numeri di fonti realmente inviate). */
function AnswerText({
  text,
  sources,
  onOpenNote,
}: {
  text: string;
  sources: ContextSource[];
  onOpenNote: (noteId: string) => void;
}) {
  const paragraphs = text.split(/\n{2,}/);
  return (
    <>
      {paragraphs.map((paragraph, index) => {
        const parts: ReactNode[] = paragraph.split(/(\[\d{1,2}\])/g).map((part, position) => {
          const match = /^\[(\d{1,2})\]$/.exec(part);
          const source = match ? sources.find((item) => item.number === Number(match[1])) : undefined;
          if (!source) return part;
          return (
            <button
              className="ai-citation"
              key={position}
              onClick={() => onOpenNote(source.note_id)}
              title={t.aiOpenSource(source.title || t.untitled)}
              type="button"
            >
              {part}
            </button>
          );
        });
        return <p key={index}>{parts}</p>;
      })}
    </>
  );
}

function SourceList({
  sources,
  onOpenNote,
}: {
  sources: ContextSource[];
  onOpenNote: (noteId: string) => void;
}) {
  if (sources.length === 0) return null;
  const labels = METHOD_LABEL();
  return (
    <details className="ai-sources">
      <summary>{t.aiSourcesUsed(sources.length)}</summary>
      <ol>
        {sources.map((source) => (
          <li key={`${source.number}-${source.note_id}-${source.char_start}`}>
            <button className="text-button" onClick={() => onOpenNote(source.note_id)} type="button">
              [{source.number}] {source.title || t.untitled}
            </button>
            <span className="ai-source-method">
              {labels[source.method]}
              {source.score !== null && ` · ${Math.round(source.score * 100)}%`}
            </span>
            <p className="ai-source-excerpt">{source.text.slice(0, 280)}{source.text.length > 280 ? "…" : ""}</p>
          </li>
        ))}
      </ol>
    </details>
  );
}

export default function AssistantView({
  status,
  progress,
  contextNote,
  onClearContextNote,
  onOpenNote,
  onOpenSettings,
  onStartIndexing,
  onCancelIndexing,
  onStatus,
}: AssistantViewProps) {
  const [tab, setTab] = useState<"chat" | "search" | "history">("chat");
  // Identifica la conversazione nella cronologia; cambia con "Nuova conversazione".
  const [conversationId, setConversationId] = useState(newRequestId);
  const [question, setQuestion] = useState("");
  const [turns, setTurns] = useState<Turn[]>([]);
  const [pending, setPending] = useState<PendingSend | null>(null);
  const [busy, setBusy] = useState(false);
  /** "Non chiedermelo più" nella conferma d'invio (spuntata). */
  const [rememberConsent, setRememberConsent] = useState(true);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SemanticResult[] | null>(null);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [searching, setSearching] = useState(false);
  const requestId = useRef<string | null>(null);
  const scroller = useRef<HTMLDivElement>(null);
  const ready = aiReady(status);

  useEffect(() => {
    scroller.current?.scrollTo({ top: scroller.current.scrollHeight });
  }, [turns, pending]);

  // Uscendo dalla vista si annulla l'eventuale richiesta in corso.
  useEffect(
    () => () => {
      if (requestId.current) void aiApi.cancel(requestId.current);
    },
    [],
  );

  const updateLast = (patch: Partial<Turn> | ((turn: Turn) => Partial<Turn>)) =>
    setTurns((current) => {
      if (current.length === 0) return current;
      const last = current[current.length - 1];
      const next = typeof patch === "function" ? patch(last) : patch;
      return [...current.slice(0, -1), { ...last, ...next }];
    });

  const history = (): ChatMessage[] =>
    turns
      .filter((turn) => !turn.error && turn.answer)
      .flatMap((turn) => [
        { role: "user" as const, content: turn.question },
        { role: "assistant" as const, content: turn.answer },
      ]);

  const send = async (text: string, prepared: PreparedContext, sources: ContextSource[]) => {
    const id = newRequestId();
    requestId.current = id;
    const previous = history();
    setTurns((current) => [
      ...current,
      {
        question: text,
        answer: "",
        sources: [],
        provenance: null,
        warning: prepared.warning,
        error: null,
        streaming: true,
      },
    ]);
    try {
      const answer = await aiApi.chat(id, text, previous, sources, conversationId, (delta) => {
        if (requestId.current === id) updateLast((turn) => ({ answer: turn.answer + delta }));
      });
      if (requestId.current !== id) return;
      updateLast({ answer: answer.text, sources: answer.sources, provenance: answer.provenance, streaming: false });
    } catch (cause) {
      if (requestId.current !== id) return;
      updateLast({ error: errorMessage(cause), streaming: false });
    } finally {
      if (requestId.current === id) requestId.current = null;
      setBusy(false);
    }
  };

  /** Riprende una conversazione dalla cronologia. */
  const resume = async (id: string) => {
    try {
      const entries = await aiApi.conversation(id);
      setTurns(
        entries.map((entry) => ({
          question: entry.prompt,
          answer: entry.response,
          // Le fonti salvate hanno solo nota e titolo: bastano per i link.
          sources: entry.sources.map((source) => ({
            ...source,
            char_start: 0,
            char_end: 0,
            text: "",
            method: "selected" as const,
            score: null,
          })),
          provenance: { host: entry.host, model: entry.model, local: entry.local },
          warning: null,
          error: entry.error,
          streaming: false,
        })),
      );
      setConversationId(id);
      setTab("chat");
    } catch (cause) {
      setSearchError(errorMessage(cause));
    }
  };

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    const text = question.trim();
    if (!text || busy || !status) return;
    setBusy(true);
    const id = newRequestId();
    requestId.current = id;
    try {
      const prepared = await aiApi.prepareChat(id, text, contextNote?.id ?? null);
      if (requestId.current !== id) return;
      requestId.current = null;
      setQuestion("");
      if (prepared.local || !status.settings.confirm_remote_sends) {
        await send(text, prepared, prepared.sources);
      } else {
        // Server remoto: l'utente vede e sceglie cosa inviare.
        setPending({ question: text, prepared, selected: prepared.sources.map(() => true) });
        setBusy(false);
      }
    } catch (cause) {
      if (requestId.current !== id) return;
      requestId.current = null;
      setTurns((current) => [
        ...current,
        {
          question: text,
          answer: "",
          sources: [],
          provenance: null,
          warning: null,
          error: errorMessage(cause),
          streaming: false,
        },
      ]);
      setBusy(false);
    }
  };

  const confirmPending = async () => {
    if (!pending) return;
    const { question: text, prepared, selected } = pending;
    setPending(null);
    setBusy(true);
    if (rememberConsent && status) {
      // Le prossime richieste partono senza conferma; le fonti restano
      // visibili sotto ogni risposta.
      void aiApi
        .updateSettings({ ...status.settings, confirm_remote_sends: false })
        .then(onStatus)
        .catch(() => undefined);
    }
    await send(text, prepared, prepared.sources.filter((_, index) => selected[index]));
  };

  const stop = () => {
    if (requestId.current) void aiApi.cancel(requestId.current);
    requestId.current = null;
    updateLast((turn) => (turn.streaming ? { streaming: false, error: t.aiStopped } : {}));
    setBusy(false);
  };

  const search = async (event: FormEvent) => {
    event.preventDefault();
    const text = query.trim();
    if (!text) return;
    setSearching(true);
    setSearchError(null);
    try {
      setResults(await aiApi.semanticSearch(newRequestId(), text, 20));
    } catch (cause) {
      setSearchError(errorMessage(cause));
    } finally {
      setSearching(false);
    }
  };

  const index = status?.index;
  const indexing = Boolean(status?.indexing || progress);

  const indexPanel = status?.settings.embedding_model ? (
    <div className="ai-index-status" role="status">
      <span>
        {indexing && progress
          ? t.aiIndexProgress(progress.done, progress.total)
          : t.aiIndexSummary(index?.indexed_notes ?? 0, index?.pending_notes ?? 0)}
      </span>
      {indexing ? (
        <button className="text-button" onClick={onCancelIndexing} type="button">
          {t.aiStop}
        </button>
      ) : (
        (index?.pending_notes ?? 0) > 0 && (
          <button className="text-button" onClick={onStartIndexing} type="button">
            {t.aiUpdateIndex}
          </button>
        )
      )}
    </div>
  ) : (
    <p className="data-hint">{t.aiNoEmbeddingModel}</p>
  );

  return (
    <section className="workspace-panel assistant-panel" aria-labelledby="assistant-title">
      <header className="workspace-header">
        <div>
          <h1 id="assistant-title">{t.assistantNav}</h1>
          <p className="workspace-subtitle">
            {ready && status
              ? t.aiConnectedTo(
                  status.settings.provider === "claude-code"
                    ? `Claude Code${status.settings.chat_model && status.settings.chat_model !== "default" ? ` · ${status.settings.chat_model}` : ""}`
                    : status.settings.chat_model,
                  status.settings.provider === "builtin" ? t.aiBuiltinHost : status.host,
                  status.local,
                )
              : t.assistantSubtitle}
          </p>
        </div>
        <div className="segmented" role="tablist" aria-label={t.assistantNav}>
          <button
            aria-selected={tab === "chat"}
            className={`segment${tab === "chat" ? " selected" : ""}`}
            onClick={() => setTab("chat")}
            role="tab"
            type="button"
          >
            {t.aiChatTab}
          </button>
          <button
            aria-selected={tab === "search"}
            className={`segment${tab === "search" ? " selected" : ""}`}
            onClick={() => setTab("search")}
            role="tab"
            type="button"
          >
            {t.aiSearchTab}
          </button>
          <button
            aria-selected={tab === "history"}
            className={`segment${tab === "history" ? " selected" : ""}`}
            onClick={() => setTab("history")}
            role="tab"
            type="button"
          >
            {t.aiHistoryTab}
          </button>
        </div>
      </header>

      {!ready ? (
        <div className="workspace-scroll">
          <div className="ai-setup-card">
            <h2>{t.aiSetupTitle}</h2>
            <p>{status?.settings.enabled ? t.aiSetupIncomplete : t.aiSetupBody}</p>
            <p className="data-hint">{t.aiPrivacyNote}</p>
            <button className="data-action primary" onClick={onOpenSettings} type="button">
              {t.aiSetupAction}
            </button>
          </div>
        </div>
      ) : tab === "chat" ? (
        <>
          <div className="workspace-scroll ai-chat" ref={scroller}>
            {turns.length === 0 && !pending && (
              <div className="ai-chat-empty">
                <p>{t.aiChatEmpty}</p>
                <p className="data-hint">{status?.settings.keep_history ? t.aiChatSaved : t.aiChatNotSaved}</p>
                {indexPanel}
              </div>
            )}
            {turns.map((turn, index) => (
              <article className="ai-turn" key={index}>
                <p className="ai-question">{turn.question}</p>
                <div className="ai-answer" aria-busy={turn.streaming} aria-live={turn.streaming ? "polite" : undefined}>
                  {turn.warning && <p className="data-hint">{turn.warning}</p>}
                  {turn.answer ? (
                    <AnswerText onOpenNote={onOpenNote} sources={turn.sources} text={turn.answer} />
                  ) : (
                    turn.streaming && <p className="data-hint">{t.aiThinking}</p>
                  )}
                  {turn.error && (
                    <p className="dialog-error" role="alert">
                      {turn.error}
                    </p>
                  )}
                  <SourceList onOpenNote={onOpenNote} sources={turn.sources} />
                  {turn.provenance && (
                    <p className="ai-provenance">
                      {t.aiProvenance(turn.provenance.model, turn.provenance.host, turn.provenance.local)}
                    </p>
                  )}
                </div>
              </article>
            ))}
            {pending && (
              <div className="ai-remote-warning" role="group" aria-label={t.aiRemoteTitle}>
                <strong>{t.aiRemoteTitle}</strong>
                <p>{t.aiRemoteChatMessage(pending.prepared.host)}</p>
                {pending.prepared.sources.length === 0 ? (
                  <p className="data-hint">{t.aiNoSourcesFound}</p>
                ) : (
                  <ul className="ai-suggestions">
                    {pending.prepared.sources.map((source, index) => (
                      <li key={`${source.note_id}-${source.char_start}`}>
                        <label className="checkbox-field">
                          <input
                            checked={pending.selected[index]}
                            onChange={(event) =>
                              setPending({
                                ...pending,
                                selected: pending.selected.map((value, position) =>
                                  position === index ? event.target.checked : value,
                                ),
                              })
                            }
                            type="checkbox"
                          />
                          <span>
                            {source.title || t.untitled} — {t.aiCharacters(source.text.length)}
                          </span>
                        </label>
                      </li>
                    ))}
                  </ul>
                )}
                <label className="checkbox-field ai-remember-consent">
                  <input
                    checked={rememberConsent}
                    onChange={(event) => setRememberConsent(event.target.checked)}
                    type="checkbox"
                  />
                  <span>{t.aiDontAskAgain(pending.prepared.host)}</span>
                </label>
                <div className="organization-dialog-actions">
                  <button className="dialog-submit-button" onClick={() => void confirmPending()} type="button">
                    {t.aiSendTo(pending.prepared.host)}
                  </button>
                  <button className="dialog-cancel-button" onClick={() => setPending(null)} type="button">
                    {t.cancel}
                  </button>
                </div>
              </div>
            )}
          </div>
          <form className="ai-composer" onSubmit={(event) => void submit(event)}>
            {contextNote && (
              <div className="ai-context-chip">
                {t.aiContextNote(contextNote.title || t.untitled)}
                <button aria-label={t.aiRemoveContext} className="text-button" onClick={onClearContextNote} type="button">
                  ✕
                </button>
              </div>
            )}
            <div className="ai-composer-row">
              <textarea
                aria-label={t.aiQuestionLabel}
                disabled={Boolean(pending)}
                maxLength={4000}
                onChange={(event) => setQuestion(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && !event.shiftKey) {
                    event.preventDefault();
                    event.currentTarget.form?.requestSubmit();
                  }
                }}
                placeholder={contextNote ? t.aiQuestionAboutNote : t.aiQuestionPlaceholder}
                rows={2}
                value={question}
              />
              {busy ? (
                <button className="dialog-cancel-button" onClick={stop} type="button">
                  {t.aiStop}
                </button>
              ) : (
                <button className="dialog-submit-button" disabled={!question.trim() || Boolean(pending)} type="submit">
                  {t.aiAsk}
                </button>
              )}
            </div>
            {turns.length > 0 && !busy && (
              <button
                className="text-button"
                onClick={() => {
                  setTurns([]);
                  setConversationId(newRequestId());
                }}
                type="button"
              >
                {t.aiNewConversation}
              </button>
            )}
          </form>
        </>
      ) : tab === "history" ? (
        <AiHistoryPanel
          enabled={Boolean(status?.settings.keep_history)}
          onOpenNote={onOpenNote}
          onOpenSettings={onOpenSettings}
          onResume={(id) => void resume(id)}
        />
      ) : !status?.settings.embedding_model ? (
        <div className="workspace-scroll">
          <div className="ai-setup-card">
            <h2>{t.aiSearchTab}</h2>
            <p>{t.aiSemanticExplanation}</p>
            <p className="data-hint">
              {status?.settings.provider === "claude-code" ? t.aiClaudeNoEmbeddings : t.aiSemanticNeedsModel}
            </p>
            <button className="data-action primary" onClick={onOpenSettings} type="button">
              {t.aiSetupAction}
            </button>
          </div>
        </div>
      ) : (
        <>
          <form className="workspace-toolbar" onSubmit={(event) => void search(event)}>
            <label className="search-field workspace-search">
              <span aria-hidden="true" className="search-icon">
                <Icon name="search" size={15} />
              </span>
              <input
                aria-label={t.aiSearchPlaceholder}
                maxLength={500}
                onChange={(event) => setQuery(event.target.value)}
                placeholder={t.aiSearchPlaceholder}
                type="search"
                value={query}
              />
            </label>
            <button
              className="dialog-submit-button"
              disabled={!query.trim() || searching}
              type="submit"
            >
              {t.aiSearchAction}
            </button>
          </form>
          <div className="workspace-scroll">
            <p className="data-hint">{t.aiSemanticExplanation}</p>
            {indexPanel}
            {(status.index.indexed_notes ?? 0) === 0 && !indexing && (
              <p className="data-hint">{t.aiSemanticEmptyIndex}</p>
            )}
            {searchError && (
              <div className="inline-error" role="alert">
                {searchError}
              </div>
            )}
            {searching && <p className="data-hint">{t.aiSearching}</p>}
            {results && !searching && results.length === 0 && <p className="workspace-empty">{t.noSearchResults}</p>}
            {results && results.length > 0 && (
              <ul className="ai-results">
                {results.map((result) => (
                  <li key={result.note.id}>
                    <button className="ai-result-item" onClick={() => onOpenNote(result.note.id)} type="button">
                      <span className="ai-result-title">
                        {result.note.title || t.untitled}
                        <span className="ai-source-method">{Math.round(result.score * 100)}%</span>
                      </span>
                      <span className="ai-source-excerpt">{t.aiWhySuggested(result.excerpt)}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </>
      )}
    </section>
  );
}
