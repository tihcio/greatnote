import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../../i18n";
import { errorMessage } from "../../lib/format";
import type { Tag, Task } from "../../types";
import PlannerDialog from "../PlannerDialog";
import { aiApi, newRequestId } from "./aiApi";
import { needsConfirmation, useRememberConsent } from "./remoteConsent";
import type { AiStatus, Provenance } from "./aiApi";

interface AiSuggestionsDialogProps {
  kind: "tags" | "tasks";
  noteId: string;
  noteTitle: string;
  status: AiStatus;
  onStatus: (status: AiStatus) => void;
  /** Chiamata dopo aver applicato le proposte scelte. */
  onApplied: () => void;
  onClose: () => void;
}

interface Suggestion {
  title: string;
  dueDate: string | null;
  selected: boolean;
  /** Per i tag: esiste già con questo nome. */
  existing: boolean;
}

/**
 * Tag o attività proposti dall'AI per la nota: l'utente sceglie (e può
 * correggere) cosa applicare; niente viene creato senza conferma.
 */
export default function AiSuggestionsDialog({
  kind,
  noteId,
  noteTitle,
  status,
  onStatus,
  onApplied,
  onClose,
}: AiSuggestionsDialogProps) {
  const [phase, setPhase] = useState<"confirm" | "running" | "done" | "error">(
    needsConfirmation(status) ? "confirm" : "running",
  );
  const consent = useRememberConsent(status, onStatus);
  const [suggestions, setSuggestions] = useState<Suggestion[]>([]);
  const [provenance, setProvenance] = useState<Provenance | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const requestId = useRef<string | null>(null);

  const run = useCallback(async () => {
    const id = newRequestId();
    requestId.current = id;
    setPhase("running");
    setError(null);
    try {
      if (kind === "tags") {
        const [answer, tags] = await Promise.all([aiApi.suggestTags(id, noteId), invoke<Tag[]>("list_tags")]);
        if (requestId.current !== id) return;
        const names = new Set(tags.map((tag) => tag.name.toLocaleLowerCase()));
        setSuggestions(
          answer.tags.map((name) => ({
            title: name,
            dueDate: null,
            selected: true,
            existing: names.has(name.toLocaleLowerCase()),
          })),
        );
        setProvenance(answer.provenance);
      } else {
        const answer = await aiApi.extractTasks(id, noteId);
        if (requestId.current !== id) return;
        setSuggestions(
          answer.tasks.map((task) => ({ title: task.title, dueDate: task.due_date, selected: true, existing: false })),
        );
        setProvenance(answer.provenance);
      }
      setPhase("done");
    } catch (cause) {
      if (requestId.current !== id) return;
      setError(errorMessage(cause));
      setPhase("error");
    } finally {
      if (requestId.current === id) requestId.current = null;
    }
  }, [kind, noteId]);

  useEffect(() => {
    if (!needsConfirmation(status)) void run();
    // L'avvio automatico avviene una sola volta, all'apertura.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const close = () => {
    if (requestId.current) void aiApi.cancel(requestId.current);
    requestId.current = null;
    onClose();
  };

  const update = (index: number, patch: Partial<Suggestion>) =>
    setSuggestions((current) => current.map((item, position) => (position === index ? { ...item, ...patch } : item)));

  const apply = async () => {
    const chosen = suggestions.filter((item) => item.selected && item.title.trim());
    if (chosen.length === 0) {
      onClose();
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (kind === "tags") {
        const [allTags, noteTags] = await Promise.all([
          invoke<Tag[]>("list_tags"),
          invoke<Tag[]>("list_note_tags", { noteId }),
        ]);
        const ids = new Set(noteTags.map((tag) => tag.id));
        for (const item of chosen) {
          const name = item.title.trim();
          const existing = allTags.find((tag) => tag.name.toLocaleLowerCase() === name.toLocaleLowerCase());
          const tag = existing ?? (await invoke<Tag>("create_tag", { name }));
          ids.add(tag.id);
        }
        await invoke("set_note_tags", { noteId, tagIds: [...ids] });
      } else {
        for (const item of chosen) {
          await invoke<Task>("create_task", {
            input: {
              title: item.title.trim(),
              description: "",
              priority: 0,
              due_date: item.dueDate,
              note_id: noteId,
              flagged: false,
            },
          });
        }
      }
      onApplied();
      onClose();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  const title = kind === "tags" ? t.aiSuggestTags : t.aiExtractTasks;
  return (
    <PlannerDialog busy={busy} mark="✦" onClose={close} title={title}>
      <div className="organization-dialog-form ai-result-dialog">
        <p className="data-hint">{noteTitle || t.untitled}</p>
        {phase === "confirm" && (
          <div className="ai-remote-warning" role="note">
            <strong>{t.aiRemoteTitle}</strong>
            <p>{t.aiRemoteNoteMessage(noteTitle || t.untitled, status.host)}</p>
            {consent.checkbox}
          </div>
        )}
        {phase === "running" && (
          <p aria-busy="true" className="ai-output" role="status">
            {t.aiThinking}
          </p>
        )}
        {phase === "done" &&
          (suggestions.length === 0 ? (
            <p className="workspace-empty">{kind === "tags" ? t.aiNoTags : t.aiNoTasks}</p>
          ) : (
            <ul className="ai-suggestions">
              {suggestions.map((item, index) => (
                <li key={index}>
                  <input
                    aria-label={t.aiIncludeSuggestion(item.title)}
                    checked={item.selected}
                    onChange={(event) => update(index, { selected: event.target.checked })}
                    type="checkbox"
                  />
                  <input
                    aria-label={kind === "tags" ? t.tagNamePrompt : t.taskTitleLabel}
                    className="ai-suggestion-title"
                    maxLength={kind === "tags" ? 100 : 200}
                    onChange={(event) => update(index, { title: event.target.value })}
                    value={item.title}
                  />
                  {kind === "tags" && item.existing && <span className="ai-badge">{t.aiExistingTag}</span>}
                  {kind === "tasks" && (
                    <input
                      aria-label={t.taskDueLabel}
                      onChange={(event) => update(index, { dueDate: event.target.value || null })}
                      type="date"
                      value={item.dueDate ?? ""}
                    />
                  )}
                </li>
              ))}
            </ul>
          ))}
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
        {provenance && phase === "done" && (
          <p className="ai-provenance">{t.aiProvenance(provenance.model, provenance.host, provenance.local)}</p>
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
          {phase === "done" && suggestions.length > 0 && (
            <button className="dialog-submit-button" disabled={busy} onClick={() => void apply()} type="button">
              {kind === "tags" ? t.aiApplyTags : t.aiCreateTasks}
            </button>
          )}
          {(phase === "done" || phase === "error") && (
            <button className="dialog-cancel-button" disabled={busy} onClick={() => void run()} type="button">
              {t.aiRetry}
            </button>
          )}
          <button className="dialog-cancel-button" disabled={busy} onClick={close} type="button">
            {phase === "running" ? t.aiStop : t.cancel}
          </button>
        </footer>
      </div>
    </PlannerDialog>
  );
}
