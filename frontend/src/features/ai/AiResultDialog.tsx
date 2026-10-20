import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../../i18n";
import { diffWords } from "../../lib/diff";
import { errorMessage } from "../../lib/format";
import PlannerDialog from "../PlannerDialog";
import { aiApi, newRequestId } from "./aiApi";
import { needsConfirmation, useRememberConsent } from "./remoteConsent";
import type { AiStatus, Provenance, TransformAction } from "./aiApi";

export interface AiTransformRequest {
  action: TransformAction;
  label: string;
  /** Testo inviato: la selezione o l'intera nota. */
  text: string;
  scope: "selection" | "note";
  noteId: string | null;
}

interface AiResultDialogProps {
  request: AiTransformRequest;
  status: AiStatus;
  onStatus: (status: AiStatus) => void;
  /** Applica il risultato nell'editor (annullabile con Ctrl+Z). */
  onApply: (mode: "replace" | "insert", markdown: string) => Promise<void>;
  onClose: () => void;
}

type Phase = "confirm" | "running" | "done" | "error";

/**
 * Anteprima di una modifica AI: il testo arriva in streaming, si può
 * confrontare con l'originale e viene applicato solo con un comando
 * esplicito. Con un server remoto chiede prima conferma dell'invio.
 */
export default function AiResultDialog({ request, status, onStatus, onApply, onClose }: AiResultDialogProps) {
  const [phase, setPhase] = useState<Phase>(needsConfirmation(status) ? "confirm" : "running");
  const consent = useRememberConsent(status, onStatus);
  const [result, setResult] = useState("");
  const [provenance, setProvenance] = useState<Provenance | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<"result" | "diff">("result");
  const [notice, setNotice] = useState<string | null>(null);
  const requestId = useRef<string | null>(null);

  const run = useCallback(async () => {
    const id = newRequestId();
    requestId.current = id;
    setPhase("running");
    setResult("");
    setError(null);
    setNotice(null);
    try {
      const answer = await aiApi.transform(id, request.noteId, request.action, request.text, (delta) => {
        if (requestId.current === id) setResult((current) => current + delta);
      });
      if (requestId.current !== id) return;
      setResult(answer.text);
      setProvenance(answer.provenance);
      setPhase("done");
    } catch (cause) {
      if (requestId.current !== id) return;
      setError(errorMessage(cause));
      setPhase("error");
    } finally {
      if (requestId.current === id) requestId.current = null;
    }
  }, [request]);

  useEffect(() => {
    if (!needsConfirmation(status)) void run();
    // L'avvio automatico avviene una sola volta, all'apertura.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const cancelAndClose = () => {
    if (requestId.current) void aiApi.cancel(requestId.current);
    requestId.current = null;
    onClose();
  };

  const diff = useMemo(
    () => (phase === "done" && view === "diff" ? diffWords(request.text, result) : []),
    [phase, request.text, result, view],
  );

  const apply = async (mode: "replace" | "insert") => {
    try {
      await onApply(mode, result);
      onClose();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const copy = async () => {
    try {
      await invoke("write_clipboard_text", { text: result });
      setNotice(t.aiCopied);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  return (
    <PlannerDialog mark="✦" onClose={cancelAndClose} title={request.label}>
      <div className="organization-dialog-form ai-result-dialog">
        <p className="data-hint">
          {request.scope === "selection" ? t.aiScopeSelection : t.aiScopeNote} ·{" "}
          {t.aiCharacters(request.text.length)}
        </p>
        {phase === "confirm" && (
          <div className="ai-remote-warning" role="note">
            <strong>{t.aiRemoteTitle}</strong>
            <p>{t.aiRemoteTextMessage(status.host)}</p>
            {consent.checkbox}
          </div>
        )}
        {phase !== "confirm" && phase !== "error" && (
          <>
            {phase === "done" && (
              <div className="segmented" role="tablist" aria-label={request.label}>
                <button
                  aria-selected={view === "result"}
                  className={`segment${view === "result" ? " selected" : ""}`}
                  onClick={() => setView("result")}
                  role="tab"
                  type="button"
                >
                  {t.aiResult}
                </button>
                <button
                  aria-selected={view === "diff"}
                  className={`segment${view === "diff" ? " selected" : ""}`}
                  onClick={() => setView("diff")}
                  role="tab"
                  type="button"
                >
                  {t.aiCompare}
                </button>
              </div>
            )}
            {view === "diff" && phase === "done" ? (
              <div className="ai-output ai-diff" aria-label={t.aiCompare}>
                {diff.map((part, index) =>
                  part.kind === "same" ? (
                    <span key={index}>{part.text}</span>
                  ) : part.kind === "added" ? (
                    <ins key={index}>{part.text}</ins>
                  ) : (
                    <del key={index}>{part.text}</del>
                  ),
                )}
              </div>
            ) : (
              <div aria-busy={phase === "running"} aria-live="polite" className="ai-output">
                {result || (phase === "running" ? t.aiThinking : "")}
              </div>
            )}
          </>
        )}
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
        {notice && <p className="data-hint">{notice}</p>}
        {provenance && phase === "done" && (
          <p className="ai-provenance">
            {t.aiProvenance(provenance.model, provenance.host, provenance.local)} {t.aiReviewHint}
          </p>
        )}
        <footer className="organization-dialog-actions">
          {phase === "confirm" && (
            <button
              className="dialog-submit-button"
              onClick={() => {
                void consent.commit();
                void run();
              }}
              type="button"
            >
              {t.aiSendTo(status.host)}
            </button>
          )}
          {phase === "running" && (
            <button className="dialog-cancel-button" onClick={cancelAndClose} type="button">
              {t.aiStop}
            </button>
          )}
          {phase === "done" && (
            <>
              <button className="dialog-submit-button" onClick={() => void apply("replace")} type="button">
                {request.scope === "selection" ? t.aiReplaceSelection : t.aiReplaceNote}
              </button>
              <button className="dialog-cancel-button" onClick={() => void apply("insert")} type="button">
                {t.aiInsertBelow}
              </button>
              <button className="dialog-cancel-button" onClick={() => void copy()} type="button">
                {t.aiCopy}
              </button>
            </>
          )}
          {(phase === "done" || phase === "error") && (
            <button className="dialog-cancel-button" onClick={() => void run()} type="button">
              {t.aiRetry}
            </button>
          )}
          {phase !== "running" && (
            <button className="dialog-cancel-button" onClick={cancelAndClose} type="button">
              {phase === "done" ? t.aiDiscard : t.cancel}
            </button>
          )}
        </footer>
      </div>
    </PlannerDialog>
  );
}
