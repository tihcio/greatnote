import { useState } from "react";
import { t } from "../i18n";
import type { Notebook, Space } from "../types";
import PlannerDialog from "./PlannerDialog";

interface NotebookPickerDialogProps {
  title: string;
  confirmLabel: string;
  notebooks: Notebook[];
  spaces: Space[];
  /** Taccuino attuale della nota (escluso per lo spostamento). */
  currentNotebookId?: string | null;
  excludeCurrent?: boolean;
  onConfirm: (notebookId: string | null) => void;
  /**
   * Crea un taccuino nello spazio indicato (`null`: fuori dagli spazi) e lo
   * usa come destinazione. Una nota sta sempre in un taccuino, mai
   * direttamente in uno spazio: senza questa azione uno spazio appena
   * creato, ancora vuoto, non sarebbe raggiungibile da qui.
   */
  onCreateNotebook?: (spaceId: string | null) => void;
  /** Solo i taccuini di questo spazio (nota trascinata su uno spazio). */
  onlySpaceId?: string | null;
  onClose: () => void;
}

interface Group {
  space: Space | null;
  notebooks: Notebook[];
}

/** Scelta del taccuino di destinazione per "Sposta in…" e "Copia in…",
 * con i taccuini raggruppati per spazio. */
export default function NotebookPickerDialog({
  title,
  confirmLabel,
  notebooks,
  spaces,
  currentNotebookId = null,
  excludeCurrent = false,
  onConfirm,
  onCreateNotebook,
  onlySpaceId = null,
  onClose,
}: NotebookPickerDialogProps) {
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<string | null | undefined>(undefined);
  const needle = filter.trim().toLowerCase();
  const matches = (text: string) => text.toLowerCase().includes(needle);
  const available = (notebook: Notebook) => !(excludeCurrent && notebook.id === currentNotebookId);

  const knownSpaces = new Set(spaces.map((space) => space.id));
  const allGroups: Group[] = [
    {
      space: null,
      notebooks: notebooks.filter(
        (notebook) => !notebook.space_id || !knownSpaces.has(notebook.space_id),
      ),
    },
    ...spaces.map((space) => ({
      space,
      notebooks: notebooks.filter((notebook) => notebook.space_id === space.id),
    })),
  ];
  const groups = onlySpaceId
    ? allGroups.filter((group) => group.space?.id === onlySpaceId)
    : allGroups;

  // Con un filtro: i taccuini che lo contengono, oppure tutto lo spazio se
  // è il nome dello spazio a contenerlo.
  const visible = groups
    .map((group) => {
      const spaceMatches = group.space !== null && needle !== "" && matches(group.space.name);
      return {
        ...group,
        empty: group.notebooks.length === 0,
        notebooks: group.notebooks.filter(
          (notebook) => available(notebook) && (spaceMatches || matches(notebook.name)),
        ),
        spaceMatches,
      };
    })
    .filter((group) => {
      if (group.space === null) return true;
      return needle === "" || group.spaceMatches || group.notebooks.length > 0;
    });
  const showNoNotebook =
    !onlySpaceId && !(excludeCurrent && currentNotebookId === null) && matches(t.noNotebook);

  const option = (id: string | null, name: string) => (
    <label className="picker-option" key={id ?? "none"}>
      <input
        checked={selected === id}
        name="notebook-target"
        onChange={() => setSelected(id)}
        type="radio"
      />
      <span className="picker-name">{name}</span>
      {id === currentNotebookId && <small>{t.currentLocation}</small>}
    </label>
  );
  const createButton = (space: Space | null) =>
    onCreateNotebook && (
      <button
        className="picker-create"
        onClick={() => onCreateNotebook(space?.id ?? null)}
        type="button"
      >
        ＋ {space ? t.newNotebookInSpace(space.name) : t.newNotebookOutsideSpaces}
      </button>
    );

  const nothing =
    !showNoNotebook &&
    visible.every((group) => group.notebooks.length === 0 && !(group.space && needle === ""));

  return (
    <PlannerDialog mark="▤" onClose={onClose} title={title}>
      <form
        className="organization-dialog-form"
        onSubmit={(event) => {
          event.preventDefault();
          if (selected !== undefined) onConfirm(selected);
        }}
      >
        <label className="dialog-field">
          <span>{t.filterNotebooks}</span>
          <input onChange={(event) => setFilter(event.target.value)} type="search" value={filter} />
        </label>
        <div className="picker-list" role="radiogroup" aria-label={t.notebooksLabel}>
          {nothing ? (
            <p className="detail-empty">{t.noMatches}</p>
          ) : (
            visible.map((group) => (
              <div className="picker-group" key={group.space?.id ?? "no-space"}>
                {group.space && <div className="picker-group-heading">{group.space.name}</div>}
                {group.space === null && showNoNotebook && option(null, t.noNotebook)}
                {group.notebooks.map((notebook) => option(notebook.id, notebook.name))}
                {group.space && group.empty && needle === "" && (
                  <p className="picker-group-empty">{t.spaceHasNoNotebooks}</p>
                )}
                {needle === "" && createButton(group.space)}
              </div>
            ))
          )}
        </div>
        <footer className="organization-dialog-actions">
          <button className="dialog-cancel-button" onClick={onClose} type="button">
            {t.cancel}
          </button>
          <button className="dialog-submit-button" disabled={selected === undefined} type="submit">
            {confirmLabel}
          </button>
        </footer>
      </form>
    </PlannerDialog>
  );
}
