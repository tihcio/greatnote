import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { askConfirmation } from "../lib/confirm";
import { diffWords } from "../lib/diff";
import { errorMessage, formatDateTime } from "../lib/format";
import type { Note, NoteRevision, NoteRevisionDetail } from "../types";
import { aiApi, aiReady, newRequestId } from "./ai/aiApi";
import type { AiStatus, Provenance } from "./ai/aiApi";
import { needsConfirmation, useRememberConsent } from "./ai/remoteConsent";
import PlannerDialog from "./PlannerDialog";

interface NoteHistoryDialogProps {
  noteId: string;
  /** Salva le modifiche in corso prima di elencare o ripristinare. */
  beforeLoad: () => Promise<boolean>;
  onRestored: (note: Note) => void;
  onClose: () => void;
  /** Stato dell'assistente AI (null finché non è caricato). */
  aiStatus: AiStatus | null;
  onAiStatus: (status: AiStatus) => void;
}

type View = "version" | "diff" | "ai";
type AiPhase = "idle" | "confirm" | "running" | "done" | "error";

function plainText(node: { text?: string; content?: unknown[] }): string {
  if (node.text) return node.text;
  return (node.content ?? [])
    .map((child) => plainText(child as { text?: string; content?: unknown[] }))
    .join(node.content && node.content.length > 0 ? "\n" : "");
}

const documentText = (title: string, content: unknown) =>
  `${title}\n\n${plainText(content as { content?: unknown[] })}`;

/** Consenso per i server remoti, mostrato solo quando serve. */
function RemoteConsent({ status, onStatus, onSend }: { status: AiStatus; onStatus: (status: AiStatus) => void; onSend: () => void }) {
  const consent = useRememberConsent(status, onStatus);
  return (
    <div className="ai-remote-warning" role="note">
      <strong>{t.aiRemoteTitle}</strong>
      <p>{t.historyAiRemoteMessage(status.host)}</p>
      {consent.checkbox}
      <button
        className="dialog-submit-button"
        onClick={() => {
          void consent.commit();
          onSend();
        }}
        type="button"
      >
        {t.aiSendTo(status.host)}
      </button>
    </div>
  );
}

/**
 * "Cronologia nota": revisioni locali con anteprima, confronto con la
 * versione attuale (locale, senza AI), spiegazione facoltativa delle
 * modifiche con l'assistente AI e ripristino.
 */
export default function NoteHistoryDialog({ noteId, beforeLoad, onRestored, onClose, aiStatus, onAiStatus }: NoteHistoryDialogProps) {
  const [revisions, setRevisions] = useState<NoteRevision[] | null>(null);
  const [selected, setSelected] = useState<NoteRevisionDetail | null>(null);
  const [current, setCurrent] = useState<Note | null>(null);
  const [view, setView] = useState<View>("version");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [aiPhase, setAiPhase] = useState<AiPhase>("idle");
  const [aiText, setAiText] = useState("");
  const [aiError, setAiError] = useState<string | null>(null);
  const [provenance, setProvenance] = useState<Provenance | null>(null);
  const requestId = useRef<string | null>(null);

  const stopAi = () => {
    if (requestId.current) void aiApi.cancel(requestId.current);
    requestId.current = null;
  };

  // Una richiesta in corso non sopravvive alla chiusura del dialogo.
  useEffect(() => stopAi, []);

  useEffect(() => {
    void (async () => {
      if (!(await beforeLoad())) return;
      try {
        const [loaded, note] = await Promise.all([
          invoke<NoteRevision[]>("list_note_revisions", { noteId }),
          invoke<Note>("get_note", { noteId }),
        ]);
        setRevisions(loaded);
        setCurrent(note);
        if (loaded[0]) {
          setSelected(await invoke<NoteRevisionDetail>("get_note_revision", { revisionId: loaded[0].id }));
        }
      } catch (cause) {
        setError(errorMessage(cause));
      }
    })();
  }, [beforeLoad, noteId]);

  const resetAi = () => {
    stopAi();
    setAiPhase("idle");
    setAiText("");
    setAiError(null);
    setProvenance(null);
  };

  const select = async (revision: NoteRevision) => {
    try {
      const detail = await invoke<NoteRevisionDetail>("get_note_revision", { revisionId: revision.id });
      resetAi();
      if (view === "ai") setView("diff");
      setSelected(detail);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const diff = useMemo(() => {
    if (view !== "diff" || !selected || !current) return [];
    return diffWords(
      documentText(selected.revision.title, selected.content),
      documentText(current.title, current.content),
    );
  }, [current, selected, view]);
  const identical = diff.length > 0 && diff.every((part) => part.kind === "same");

  const runAi = async () => {
    if (!selected) return;
    const id = newRequestId();
    requestId.current = id;
    setAiPhase("running");
    setAiText("");
    setAiError(null);
    setProvenance(null);
    try {
      const answer = await aiApi.compareRevision(id, selected.revision.id, (delta) => {
        if (requestId.current === id) setAiText((text) => text + delta);
      });
      if (requestId.current !== id) return;
      setAiText(answer.text);
      setProvenance(answer.provenance);
      setAiPhase("done");
    } catch (cause) {
      if (requestId.current !== id) return;
      setAiError(errorMessage(cause));
      setAiPhase("error");
    } finally {
      if (requestId.current === id) requestId.current = null;
    }
  };

  const explain = () => {
    setView("ai");
    if (!aiStatus) return;
    if (needsConfirmation(aiStatus)) setAiPhase("confirm");
    else void runAi();
  };

  const restore = async () => {
    if (!selected) return;
    const confirmed = await askConfirmation({
      title: t.restoreRevisionTitle,
      message: t.confirmRestoreRevision(formatDateTime(selected.revision.created_at)),
      confirmLabel: t.actionRestore,
    });
    if (!confirmed) return;
    stopAi();
    setBusy(true);
    try {
      onRestored(await invoke<Note>("restore_note_revision", { revisionId: selected.revision.id }));
      onClose();
    } catch (cause) {
      setError(errorMessage(cause));
      setBusy(false);
    }
  };

  const tab = (value: View, label: string) => (
    <button
      aria-selected={view === value}
      className={`segment${view === value ? " selected" : ""}`}
      onClick={() => setView(value)}
      role="tab"
      type="button"
    >
      {label}
    </button>
  );

  return (
    <PlannerDialog
      busy={busy}
      mark="↺"
      onClose={() => {
        stopAi();
        onClose();
      }}
      title={t.noteHistoryTitle}
    >
      <div className="organization-dialog-form history-dialog">
        <p className="data-hint">{t.noteHistoryIntro}</p>
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
        {revisions === null && !error && <p className="detail-empty">{t.loading}</p>}
        {revisions && revisions.length === 0 && <p className="detail-empty">{t.noRevisions}</p>}
        {revisions && revisions.length > 0 && (
          <div className="history-layout">
            <ul className="history-list" aria-label={t.noteHistoryTitle}>
              {revisions.map((revision) => (
                <li key={revision.id}>
                  <button
                    aria-current={selected?.revision.id === revision.id ? "true" : undefined}
                    className={`history-item${selected?.revision.id === revision.id ? " active" : ""}`}
                    onClick={() => void select(revision)}
                    type="button"
                  >
                    <strong>{formatDateTime(revision.created_at)}</strong>
                    <small>
                      {revision.title} · {t.wordCount(revision.word_count)}
                    </small>
                  </button>
                </li>
              ))}
            </ul>
            <div className="history-pane">
              <div className="segmented" role="tablist" aria-label={t.noteHistoryTitle}>
                {tab("version", t.historyViewVersion)}
                {tab("diff", t.historyViewDiff)}
                {tab("ai", t.historyViewAi)}
              </div>
              <div className="history-preview" aria-live="polite">
                {selected && view === "version" && (
                  <>
                    <h3>{selected.revision.title}</h3>
                    <pre>{plainText(selected.content as { content?: unknown[] }) || "—"}</pre>
                  </>
                )}
                {selected && view === "diff" && (
                  <>
                    <p className="data-hint">{identical ? t.historyNoChanges : t.historyDiffLegend}</p>
                    <pre className="ai-diff" aria-label={t.historyViewDiff}>
                      {diff.map((part, index) =>
                        part.kind === "same" ? (
                          <span key={index}>{part.text}</span>
                        ) : part.kind === "added" ? (
                          <ins key={index}>{part.text}</ins>
                        ) : (
                          <del key={index}>{part.text}</del>
                        ),
                      )}
                    </pre>
                  </>
                )}
                {selected && view === "ai" && (
                  <>
                    {!aiReady(aiStatus) && <p className="data-hint">{t.historyAiUnavailable}</p>}
                    {aiReady(aiStatus) && aiPhase === "idle" && (
                      <button className="text-button" onClick={explain} type="button">
                        {t.historyAiExplain}
                      </button>
                    )}
                    {aiReady(aiStatus) && aiStatus && aiPhase === "confirm" && (
                      <RemoteConsent onSend={() => void runAi()} onStatus={onAiStatus} status={aiStatus} />
                    )}
                    {(aiPhase === "running" || aiPhase === "done") && (
                      <div aria-busy={aiPhase === "running"} className="history-ai-output">
                        {aiText || t.aiThinking}
                      </div>
                    )}
                    {aiError && (
                      <p className="dialog-error" role="alert">
                        {aiError}
                      </p>
                    )}
                    {provenance && aiPhase === "done" && (
                      <p className="ai-provenance">{t.aiProvenance(provenance.model, provenance.host, provenance.local)}</p>
                    )}
                  </>
                )}
              </div>
            </div>
          </div>
        )}
        <footer className="organization-dialog-actions">
          {aiPhase === "running" && (
            <button
              className="dialog-cancel-button"
              onClick={() => {
                stopAi();
                setAiPhase("idle");
              }}
              type="button"
            >
              {t.aiStop}
            </button>
          )}
          {selected && aiReady(aiStatus) && aiPhase !== "running" && aiPhase !== "confirm" && !(view === "ai" && aiPhase === "idle") && (
            <button className="dialog-cancel-button" onClick={explain} type="button">
              {aiPhase === "done" || aiPhase === "error" ? t.aiRetry : t.historyAiExplain}
            </button>
          )}
          <button
            className="dialog-cancel-button"
            onClick={() => {
              stopAi();
              onClose();
            }}
            type="button"
          >
            {t.close}
          </button>
          <button className="dialog-submit-button" disabled={!selected || busy} onClick={() => void restore()} type="button">
            {t.restoreThisVersion}
          </button>
        </footer>
      </div>
    </PlannerDialog>
  );
}
