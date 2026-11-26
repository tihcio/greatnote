import { useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { diffWords, documentText, plainText } from "../lib/diff";
import { errorMessage, formatDateTime } from "../lib/format";
import PlannerDialog from "./PlannerDialog";

export interface McpRequest {
  id: string;
  action: "trash_note" | "replace_note";
  note_id: string;
  note_title: string;
  note_content: unknown;
  title: string | null;
  content: unknown;
  reason: string;
  status: "pending" | "approved" | "rejected" | "expired" | "failed";
  error: string | null;
  created_at: string;
  decided_at: string | null;
  note_changed: boolean;
  note_missing: boolean;
}

const MAX_VISIBLE = 3;

function actionLabel(request: McpRequest): string {
  const title = request.note_title || t.untitled;
  return request.action === "trash_note" ? t.mcpRequestTrash(title) : t.mcpRequestReplace(title);
}

interface McpRequestsProps {
  requests: McpRequest[];
  /** Prima della conferma: salva le modifiche in sospeso della nota aperta.
   *  `false` annulla la conferma. */
  beforeApprove: (request: McpRequest) => Promise<boolean>;
  /** Dopo una decisione: elenco aggiornato ed editor ricaricato. */
  onDecided: (request: McpRequest, approved: boolean) => void;
}

/**
 * Richieste di un'app di AI collegata via MCP (cestino, riscrittura): il
 * server non le applica, aspettano qui la decisione dell'utente. Il
 * riquadro resta finché non vengono decise (ADR 0014).
 */
export default function McpRequests({ requests, beforeApprove, onDecided }: McpRequestsProps) {
  const [open, setOpen] = useState<McpRequest | null>(null);
  if (requests.length === 0) return null;
  const visible = requests.slice(0, MAX_VISIBLE);
  return (
    <>
      <aside aria-label={t.mcpRequestsTitle(requests.length)} aria-live="polite" className="reminder-alerts mcp-requests">
        <header>
          <strong>
            <span aria-hidden="true">⚠ </span>
            {t.mcpRequestsTitle(requests.length)}
          </strong>
        </header>
        <ul>
          {visible.map((request) => (
            <li key={request.id}>
              <div className="reminder-main">
                <strong>{actionLabel(request)}</strong>
                {request.reason && <span className="mcp-request-reason">«{request.reason}»</span>}
                <time className="reminder-time">{formatDateTime(request.created_at)}</time>
              </div>
              <div className="ai-inline-actions">
                <button className="data-action" onClick={() => setOpen(request)} type="button">
                  {t.mcpRequestReview}
                </button>
              </div>
            </li>
          ))}
        </ul>
        {requests.length > visible.length && (
          <p className="reminder-more">{t.moreItems(requests.length - visible.length)}</p>
        )}
      </aside>
      {open && (
        <McpRequestDialog
          beforeApprove={beforeApprove}
          onClose={() => setOpen(null)}
          onDecided={(request, approved) => {
            setOpen(null);
            onDecided(request, approved);
          }}
          request={open}
        />
      )}
    </>
  );
}

interface McpRequestDialogProps {
  request: McpRequest;
  beforeApprove: (request: McpRequest) => Promise<boolean>;
  onDecided: (request: McpRequest, approved: boolean) => void;
  onClose: () => void;
}

function McpRequestDialog({ request, beforeApprove, onDecided, onClose }: McpRequestDialogProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const replace = request.action === "replace_note";
  const diff = useMemo(
    () =>
      replace
        ? diffWords(
            documentText(request.note_title, request.note_content),
            documentText(request.title ?? request.note_title, request.content),
          )
        : [],
    [replace, request],
  );

  const decide = async (approve: boolean) => {
    setBusy(true);
    setError(null);
    try {
      if (approve && !(await beforeApprove(request))) {
        setBusy(false);
        return;
      }
      const decided = await invoke<McpRequest>(approve ? "approve_mcp_request" : "reject_mcp_request", {
        id: request.id,
      });
      onDecided(decided, approve);
    } catch (cause) {
      setError(errorMessage(cause));
      setBusy(false);
    }
  };

  return (
    <PlannerDialog busy={busy} mark="⚠" onClose={onClose} title={actionLabel(request)}>
      <div className="organization-dialog-form history-dialog mcp-request-dialog">
        <p className="data-hint">{replace ? t.mcpRequestReplaceIntro : t.mcpRequestTrashIntro}</p>
        {request.reason && (
          <p className="data-hint">
            {t.mcpRequestReason} <q>{request.reason}</q>
          </p>
        )}
        {request.note_missing && (
          <p className="dialog-error" role="alert">
            {t.mcpRequestNoteMissing}
          </p>
        )}
        {request.note_changed && !request.note_missing && (
          <p className="dialog-error" role="alert">
            {t.mcpRequestNoteChanged}
          </p>
        )}
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
        <div className="history-preview">
          {replace ? (
            <>
              <p className="data-hint">{t.historyDiffLegend}</p>
              <pre aria-label={t.mcpRequestChanges} className="ai-diff">
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
          ) : (
            <>
              <h3>{request.note_title || t.untitled}</h3>
              <pre>{plainText((request.note_content ?? {}) as { content?: unknown[] }) || "—"}</pre>
            </>
          )}
        </div>
        <footer className="organization-dialog-actions">
          <button className="dialog-cancel-button" disabled={busy} onClick={onClose} type="button">
            {t.mcpRequestLater}
          </button>
          <button className="dialog-cancel-button" disabled={busy} onClick={() => void decide(false)} type="button">
            {t.mcpRequestReject}
          </button>
          <button
            className="dialog-submit-button"
            disabled={busy || request.note_missing}
            onClick={() => void decide(true)}
            type="button"
          >
            {replace ? t.mcpRequestApproveReplace : t.mcpRequestApproveTrash}
          </button>
        </footer>
      </div>
    </PlannerDialog>
  );
}
