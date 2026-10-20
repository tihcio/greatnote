import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { errorMessage, formatDateTime, formatFileSize } from "../lib/format";
import type { NoteInfo } from "../types";
import PlannerDialog from "./PlannerDialog";

interface NoteInfoDialogProps {
  noteId: string;
  onClose: () => void;
  onShowHistory: () => void;
}

/** "Informazioni nota": metadati, statistiche e collegamenti. */
export default function NoteInfoDialog({ noteId, onClose, onShowHistory }: NoteInfoDialogProps) {
  const [info, setInfo] = useState<NoteInfo | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void invoke<NoteInfo>("note_info", { noteId })
      .then(setInfo)
      .catch((cause: unknown) => setError(errorMessage(cause)));
  }, [noteId]);

  const rows: [string, string][] = info
    ? [
        [t.infoTitle, info.title],
        [t.noteNotebook, info.notebook_name ?? t.noNotebook],
        [t.infoSpace, info.space_name ?? "—"],
        [t.noteTags, info.tags.length > 0 ? info.tags.join(", ") : "—"],
        [t.createdOn, formatDateTime(info.created_at)],
        [t.updatedOn, formatDateTime(info.updated_at)],
        [t.infoStatus, info.archived_at ? t.archivedNotes : info.is_pinned ? t.pinnedNotes : t.infoActive],
        [t.infoWords, String(info.word_count)],
        [t.infoCharacters, String(info.character_count)],
        [t.attachments, `${info.attachment_count} · ${formatFileSize(info.attachment_bytes)}`],
        [t.infoLinks, t.infoLinkCounts(info.link_count, info.backlink_count)],
        [t.noteTasksTitle, String(info.open_task_count)],
        [t.noteRemindersTitle, String(info.reminder_count)],
        [t.infoRevisions, String(info.revision_count)],
        [t.infoSource, info.source ?? t.infoSourceLocal],
        [t.infoId, info.id],
      ]
    : [];

  return (
    <PlannerDialog mark="ⓘ" onClose={onClose} title={t.noteInfoTitle}>
      <div className="organization-dialog-form">
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
        {!info && !error && <p className="detail-empty">{t.loading}</p>}
        {info && (
          <dl className="info-grid">
            {rows.map(([label, value]) => (
              <div key={label}>
                <dt>{label}</dt>
                <dd>{value}</dd>
              </div>
            ))}
          </dl>
        )}
        <footer className="organization-dialog-actions">
          <button className="dialog-cancel-button" onClick={onShowHistory} type="button">
            {t.noteHistoryAction}
          </button>
          <button className="dialog-submit-button" onClick={onClose} type="button">
            {t.close}
          </button>
        </footer>
      </div>
    </PlannerDialog>
  );
}
