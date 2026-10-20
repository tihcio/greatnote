import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { askConfirmation } from "../lib/confirm";
import { errorMessage, formatDateTime } from "../lib/format";
import type { TrashItem, TrashKind } from "../types";

const kindIcons: Record<TrashKind, string> = {
  space: "◇",
  notebook: "▤",
  tag: "#",
  task: "☑",
  event: "▦",
  template: "▧",
};

export const trashKindLabels: Record<TrashKind, () => string> = {
  space: () => t.trashKindSpace,
  notebook: () => t.trashKindNotebook,
  tag: () => t.trashKindTag,
  task: () => t.trashKindTask,
  event: () => t.trashKindEvent,
  template: () => t.trashKindTemplate,
};

function relatedText(item: TrashItem): string | null {
  if (item.related_count === 0) return null;
  switch (item.kind) {
    case "notebook":
      return t.trashRelatedNotes(item.related_count);
    case "space":
      return t.trashRelatedNotebooks(item.related_count);
    case "tag":
      return t.trashRelatedTaggedNotes(item.related_count);
    default:
      return null;
  }
}

interface TrashItemsPanelProps {
  items: TrashItem[];
  /** Dopo un ripristino o un'eliminazione: ricarica cestino e dati collegati. */
  onChanged: () => void;
}

/** Spazi, taccuini, tag, attività ed eventi nel cestino, sopra le note. */
export default function TrashItemsPanel({ items, onChanged }: TrashItemsPanelProps) {
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  if (items.length === 0) return null;

  const act = async (item: TrashItem, command: "restore_trash_item" | "delete_trash_item") => {
    if (command === "delete_trash_item") {
      const confirmed = await askConfirmation({
        title: t.titleDeletePermanently,
        message: t.confirmDeleteTrashItem(trashKindLabels[item.kind](), item.title),
        confirmLabel: t.actionDeletePermanently,
        destructive: true,
      });
      if (!confirmed) return;
    }
    setBusy(item.id);
    try {
      await invoke(command, { kind: item.kind, id: item.id });
      setError(null);
      onChanged();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(null);
    }
  };

  return (
    <section className="trash-items" aria-labelledby="trash-items-title">
      <h2 id="trash-items-title">
        {t.trashOtherItems} <span className="segment-count">{items.length}</span>
      </h2>
      {error && (
        <p className="inline-error" role="alert">
          {error}
        </p>
      )}
      <ul>
        {items.map((item) => {
          const related = relatedText(item);
          return (
            <li className="trash-item" key={`${item.kind}-${item.id}`}>
              <span className="trash-item-icon" aria-hidden="true">
                {kindIcons[item.kind]}
              </span>
              <div className="trash-item-text">
                <strong>{item.title}</strong>
                <small>
                  {trashKindLabels[item.kind]()} · {t.trashedOn(formatDateTime(item.deleted_at))}
                  {related && ` · ${related}`}
                </small>
              </div>
              <div className="trash-item-actions">
                <button
                  aria-label={`${t.actionRestore}: ${item.title}`}
                  className="text-button"
                  disabled={busy !== null}
                  onClick={() => void act(item, "restore_trash_item")}
                  type="button"
                >
                  {t.actionRestore}
                </button>
                <button
                  aria-label={`${t.actionDeletePermanently}: ${item.title}`}
                  className="text-button destructive-text"
                  disabled={busy !== null}
                  onClick={() => void act(item, "delete_trash_item")}
                  title={t.actionDeletePermanently}
                  type="button"
                >
                  ×
                </button>
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
