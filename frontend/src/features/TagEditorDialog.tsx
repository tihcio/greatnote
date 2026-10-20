import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { errorMessage } from "../lib/format";
import type { Tag } from "../types";
import PlannerDialog from "./PlannerDialog";

interface TagEditorDialogProps {
  noteId: string;
  tags: Tag[];
  selected: Tag[];
  onSaved: (tags: Tag[], allTags: Tag[]) => void;
  onClose: () => void;
}

/** "Modifica tag…": assegna tag esistenti e ne crea di nuovi. */
export default function TagEditorDialog({ noteId, tags, selected, onSaved, onClose }: TagEditorDialogProps) {
  const [allTags, setAllTags] = useState(tags);
  const [chosen, setChosen] = useState(() => new Set(selected.map((tag) => tag.id)));
  const [filter, setFilter] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const query = filter.trim();
  const visible = allTags.filter((tag) => tag.name.toLowerCase().includes(query.toLowerCase()));
  const exactMatch = allTags.some((tag) => tag.name.toLowerCase() === query.toLowerCase());

  const toggle = (id: string) =>
    setChosen((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  const createTag = async () => {
    if (!query || exactMatch) return;
    setBusy(true);
    try {
      const tag = await invoke<Tag>("create_tag", { name: query });
      setAllTags((current) => [...current, tag].sort((a, b) => a.name.localeCompare(b.name)));
      setChosen((current) => new Set(current).add(tag.id));
      setFilter("");
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  const save = async () => {
    setBusy(true);
    try {
      const tagIds = allTags.filter((tag) => chosen.has(tag.id)).map((tag) => tag.id);
      await invoke("set_note_tags", { noteId, tagIds });
      onSaved(
        allTags.filter((tag) => chosen.has(tag.id)),
        allTags,
      );
      onClose();
    } catch (cause) {
      setError(errorMessage(cause));
      setBusy(false);
    }
  };

  return (
    <PlannerDialog busy={busy} mark="#" onClose={onClose} title={t.editTagsTitle}>
      <form
        className="organization-dialog-form"
        onSubmit={(event) => {
          event.preventDefault();
          void save();
        }}
      >
        <div className="inline-form">
          <input
            aria-label={t.filterOrCreateTag}
            maxLength={100}
            onChange={(event) => setFilter(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && query && !exactMatch) {
                event.preventDefault();
                void createTag();
              }
            }}
            placeholder={t.filterOrCreateTag}
            value={filter}
          />
          <button
            className="subtle-action"
            disabled={busy || !query || exactMatch}
            onClick={() => void createTag()}
            type="button"
          >
            {t.createTagAction}
          </button>
        </div>
        <div className="picker-list" role="group" aria-label={t.tagsSectionLabel}>
          {visible.length === 0 ? (
            <p className="detail-empty">{t.noMatches}</p>
          ) : (
            visible.map((tag) => (
              <label className="picker-option" key={tag.id}>
                <input checked={chosen.has(tag.id)} onChange={() => toggle(tag.id)} type="checkbox" />
                <span className="picker-name"># {tag.name}</span>
              </label>
            ))
          )}
        </div>
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
        <footer className="organization-dialog-actions">
          <button className="dialog-cancel-button" onClick={onClose} type="button">
            {t.cancel}
          </button>
          <button className="dialog-submit-button" disabled={busy} type="submit">
            {t.saveTask}
          </button>
        </footer>
      </form>
    </PlannerDialog>
  );
}
