import { useEffect, useState } from "react";
import { t } from "../i18n";
import { MAX_TABLE_COLUMNS, MAX_TABLE_ROWS } from "../editor/tableActions";
import { clampSize, subscribeTableSize } from "../lib/tableSize";
import type { TableSizeRequest } from "../lib/tableSize";
import PlannerDialog from "./PlannerDialog";

/** Righe e colonne della griglia di anteprima (tabelle più grandi si
 * scrivono nei campi numerici). */
const GRID_ROWS = 8;
const GRID_COLUMNS = 10;
const DEFAULT_SIZE = 3;

/** Dialog per `askTableSize`, montato una volta in ogni finestra. */
export default function TableSizeHost() {
  const [request, setRequest] = useState<TableSizeRequest | null>(null);
  const [rows, setRows] = useState(String(DEFAULT_SIZE));
  const [cols, setCols] = useState(String(DEFAULT_SIZE));
  const [withHeaderRow, setWithHeaderRow] = useState(true);

  useEffect(
    () =>
      subscribeTableSize((next) => {
        setRequest(next);
        if (next) {
          setRows(String(DEFAULT_SIZE));
          setCols(String(DEFAULT_SIZE));
        }
      }),
    [],
  );

  if (!request) return null;
  const rowCount = clampSize(rows, MAX_TABLE_ROWS);
  const colCount = clampSize(cols, MAX_TABLE_COLUMNS);
  const valid = rowCount !== null && colCount !== null;
  const submit = (r = rowCount, c = colCount) => {
    if (r === null || c === null) return;
    request.resolve({ rows: r, cols: c, withHeaderRow });
  };
  const pick = (r: number, c: number) => {
    setRows(String(r));
    setCols(String(c));
  };
  const shownRows = rowCount ?? 0;
  const shownCols = colCount ?? 0;

  return (
    <PlannerDialog mark="▦" onClose={() => request.resolve(null)} title={t.tableInsertTitle}>
      <form
        className="organization-dialog-form table-size-dialog"
        onSubmit={(event) => {
          event.preventDefault();
          submit();
        }}
      >
        <div className="table-size-fields">
          <label className="dialog-field">
            <span>{t.tableRows}</span>
            <input
              inputMode="numeric"
              max={MAX_TABLE_ROWS}
              min={1}
              onChange={(event) => setRows(event.target.value)}
              type="number"
              value={rows}
            />
          </label>
          <span aria-hidden="true" className="table-size-times">
            ×
          </span>
          <label className="dialog-field">
            <span>{t.tableColumns}</span>
            <input
              inputMode="numeric"
              max={MAX_TABLE_COLUMNS}
              min={1}
              onChange={(event) => setCols(event.target.value)}
              type="number"
              value={cols}
            />
          </label>
        </div>
        <div
          aria-label={t.tableGridLabel(shownRows, shownCols)}
          className="table-size-grid"
          onKeyDown={(event) => {
            const moves: Record<string, [number, number]> = {
              ArrowUp: [-1, 0],
              ArrowDown: [1, 0],
              ArrowLeft: [0, -1],
              ArrowRight: [0, 1],
            };
            const move = moves[event.key];
            if (move) {
              event.preventDefault();
              pick(
                Math.min(Math.max(shownRows + move[0], 1), MAX_TABLE_ROWS),
                Math.min(Math.max(shownCols + move[1], 1), MAX_TABLE_COLUMNS),
              );
            } else if (event.key === "Enter") {
              event.preventDefault();
              submit();
            }
          }}
          role="group"
          style={{ gridTemplateColumns: `repeat(${GRID_COLUMNS}, 1fr)` }}
          tabIndex={0}
        >
          {Array.from({ length: GRID_ROWS * GRID_COLUMNS }, (_, index) => {
            const r = Math.floor(index / GRID_COLUMNS) + 1;
            const c = (index % GRID_COLUMNS) + 1;
            const selected = r <= shownRows && c <= shownCols;
            return (
              <span
                aria-hidden="true"
                className={`table-size-cell${selected ? " selected" : ""}`}
                data-cell={`${r}x${c}`}
                key={index}
                onClick={() => submit(r, c)}
                onMouseEnter={() => pick(r, c)}
              />
            );
          })}
        </div>
        <p aria-live="polite" className="data-hint table-size-summary">
          {valid ? t.tableGridLabel(shownRows, shownCols) : t.tableSizeInvalid(MAX_TABLE_ROWS, MAX_TABLE_COLUMNS)}
        </p>
        <label className="checkbox-field">
          <input
            checked={withHeaderRow}
            onChange={(event) => setWithHeaderRow(event.target.checked)}
            type="checkbox"
          />
          <span>{t.tableFirstRowHeader}</span>
        </label>
        <footer className="organization-dialog-actions">
          <button className="dialog-cancel-button" onClick={() => request.resolve(null)} type="button">
            {t.cancel}
          </button>
          <button className="dialog-submit-button" disabled={!valid} type="submit">
            {t.tableInsertAction}
          </button>
        </footer>
      </form>
    </PlannerDialog>
  );
}
