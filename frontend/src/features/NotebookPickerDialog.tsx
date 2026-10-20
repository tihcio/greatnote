import { useMemo, useState } from "react";
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
  onClose: () => void;
}

/** Scelta del taccuino di destinazione per "Sposta in…" e "Copia in…". */
export default function NotebookPickerDialog({
  title,
  confirmLabel,
  notebooks,
  spaces,
  currentNotebookId = null,
  excludeCurrent = false,
  onConfirm,
  onClose,
}: NotebookPickerDialogProps) {
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<string | null | undefined>(undefined);
  const spaceNames = useMemo(
    () => new Map(spaces.map((space) => [space.id, space.name])),
    [spaces],
  );
  const options = [
    { id: null as string | null, name: t.noNotebook, space: null as string | null },
    ...notebooks.map((notebook) => ({
      id: notebook.id as string | null,
      name: notebook.name,
      space: notebook.space_id ? spaceNames.get(notebook.space_id) ?? null : null,
    })),
  ].filter(
    (option) =>
      !(excludeCurrent && option.id === currentNotebookId) &&
      option.name.toLowerCase().includes(filter.trim().toLowerCase()),
  );

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
          {options.length === 0 ? (
            <p className="detail-empty">{t.noMatches}</p>
          ) : (
            options.map((option) => (
              <label className="picker-option" key={option.id ?? "none"}>
                <input
                  checked={selected === option.id}
                  name="notebook-target"
                  onChange={() => setSelected(option.id)}
                  type="radio"
                />
                <span className="picker-name">{option.name}</span>
                {option.space && <small>{option.space}</small>}
                {option.id === currentNotebookId && <small>{t.currentLocation}</small>}
              </label>
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
