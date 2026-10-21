import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../../i18n";
import { askConfirmation } from "../../lib/confirm";
import { errorMessage, formatDateTime } from "../../lib/format";
import Icon from "../../components/Icon";
import type { IconName } from "../../components/Icon";
import { aiApi, historyTarget } from "./aiApi";
import type { HistoryEntry, HistoryKind, SourceTarget } from "./aiApi";

const PAGE = 50;

const KIND_ICONS: Record<HistoryKind, IconName> = {
  chat: "sparkle",
  transform: "notes",
  tags: "tag",
  tasks: "check",
  search: "search",
};

function kindLabel(kind: HistoryKind): string {
  return {
    chat: t.aiHistoryChat,
    transform: t.aiHistoryTransform,
    tags: t.aiHistoryTags,
    tasks: t.aiHistoryTasks,
    search: t.aiHistorySearch,
  }[kind];
}

interface AiHistoryPanelProps {
  enabled: boolean;
  onOpenNote: (noteId: string) => void;
  /** Apre la nota, l'attività o l'evento di una fonte. */
  onOpenSource: (target: SourceTarget) => void;
  /** Riprende una conversazione nella chat. */
  onResume: (conversationId: string) => void;
  onOpenSettings: () => void;
}

/**
 * Cronologia delle richieste AI: ricerca, dettaglio con risposta e fonti,
 * copia, ripresa della conversazione ed eliminazione (singola o totale).
 */
export default function AiHistoryPanel({
  enabled,
  onOpenNote,
  onOpenSource,
  onResume,
  onOpenSettings,
}: AiHistoryPanelProps) {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [query, setQuery] = useState("");
  const [expanded, setExpanded] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const load = useCallback(
    async (offset: number) => {
      try {
        const page = await aiApi.listHistory(query.trim() || null, PAGE, offset);
        setEntries((current) => (offset === 0 ? page : [...current, ...page]));
        setHasMore(page.length === PAGE);
        setError(null);
      } catch (cause) {
        setError(errorMessage(cause));
      }
    },
    [query],
  );

  useEffect(() => {
    const timer = window.setTimeout(() => void load(0), query ? 250 : 0);
    return () => window.clearTimeout(timer);
  }, [load, query]);

  const remove = async (entry: HistoryEntry) => {
    try {
      await aiApi.deleteHistoryEntry(entry.id);
      setEntries((current) => current.filter((item) => item.id !== entry.id));
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const clearAll = async () => {
    const confirmed = await askConfirmation({
      title: t.aiHistoryClear,
      message: t.aiHistoryClearConfirm,
      confirmLabel: t.aiHistoryClear,
      destructive: true,
    });
    if (!confirmed) return;
    try {
      await aiApi.clearHistory();
      setEntries([]);
      setNotice(t.aiHistoryCleared);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const copy = async (entry: HistoryEntry) => {
    try {
      await invoke("write_clipboard_text", { text: entry.response });
      setNotice(t.aiCopied);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  return (
    <>
      <div className="workspace-toolbar">
        <label className="search-field workspace-search">
          <span aria-hidden="true" className="search-icon">
            <Icon name="search" size={15} />
          </span>
          <input
            aria-label={t.aiHistorySearch}
            maxLength={200}
            onChange={(event) => setQuery(event.target.value)}
            placeholder={t.aiHistorySearchPlaceholder}
            type="search"
            value={query}
          />
        </label>
        <button
          className="data-action destructive"
          disabled={entries.length === 0 && !query}
          onClick={() => void clearAll()}
          type="button"
        >
          {t.aiHistoryClear}
        </button>
      </div>
      <div className="workspace-scroll">
        {!enabled && (
          <p className="data-hint ai-history-off">
            {t.aiHistoryDisabled}{" "}
            <button className="text-button" onClick={onOpenSettings} type="button">
              {t.aiSetupAction}
            </button>
          </p>
        )}
        {error && (
          <div className="inline-error" role="alert">
            {error}
          </div>
        )}
        {notice && (
          <p className="data-hint" role="status">
            {notice}
          </p>
        )}
        {entries.length === 0 ? (
          <p className="workspace-empty">{query ? t.noSearchResults : t.aiHistoryEmpty}</p>
        ) : (
          <ul className="ai-history-list">
            {entries.map((entry) => {
              const open = expanded === entry.id;
              return (
                <li className={`ai-history-item${entry.error ? " failed" : ""}`} key={entry.id}>
                  <button
                    aria-expanded={open}
                    className="ai-history-summary"
                    onClick={() => setExpanded(open ? null : entry.id)}
                    type="button"
                  >
                    <span className="ai-history-kind" title={kindLabel(entry.kind)}>
                      <Icon name={KIND_ICONS[entry.kind]} size={16} />
                    </span>
                    <span className="ai-history-text">
                      <span className="ai-history-prompt">{entry.prompt}</span>
                      <span className="ai-history-meta">
                        {kindLabel(entry.kind)} · {formatDateTime(entry.created_at)} ·{" "}
                        {entry.model || "—"} ({entry.local ? t.aiLocal : t.aiRemote})
                        {entry.error && ` · ${t.aiHistoryFailed}`}
                      </span>
                    </span>
                    <Icon className={open ? "chevron-open" : undefined} name="chevron-down" size={16} />
                  </button>
                  {open && (
                    <div className="ai-history-detail">
                      {entry.error ? (
                        <p className="dialog-error">{entry.error}</p>
                      ) : (
                        <div className="ai-output">{entry.response || "—"}</div>
                      )}
                      {entry.sources.length > 0 && (
                        <div className="ai-history-sources">
                          <span>{t.aiSourcesUsed(entry.sources.length)}:</span>
                          {entry.sources.map((source) => (
                            <button
                              className="text-button"
                              key={`${source.number}-${source.note_id}-${source.item_id ?? ""}`}
                              onClick={() => onOpenSource(historyTarget(source))}
                              type="button"
                            >
                              [{source.number}] {source.kind && source.kind !== "note" && `${t.aiSourceKind(source.kind)}: `}
                              {source.title || t.untitled}
                            </button>
                          ))}
                        </div>
                      )}
                      <div className="ai-inline-actions">
                        {entry.kind === "chat" && entry.conversation_id && (
                          <button
                            className="data-action primary"
                            onClick={() => onResume(entry.conversation_id ?? "")}
                            type="button"
                          >
                            {t.aiHistoryResume}
                          </button>
                        )}
                        {entry.note_id && (
                          <button className="data-action" onClick={() => onOpenNote(entry.note_id ?? "")} type="button">
                            {t.aiHistoryOpenNote}
                          </button>
                        )}
                        {entry.response && (
                          <button className="data-action" onClick={() => void copy(entry)} type="button">
                            {t.aiCopy}
                          </button>
                        )}
                        <button className="data-action destructive" onClick={() => void remove(entry)} type="button">
                          {t.aiHistoryDelete}
                        </button>
                      </div>
                    </div>
                  )}
                </li>
              );
            })}
          </ul>
        )}
        {hasMore && (
          <button className="load-more-button" onClick={() => void load(entries.length)} type="button">
            {t.loadMore}
          </button>
        )}
      </div>
    </>
  );
}
