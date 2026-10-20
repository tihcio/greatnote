import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { displayAccelerator } from "../app/menu";
import type { Menu, MenuEntry } from "../app/menu";
import { errorMessage, formatFileSize } from "../lib/format";
import type { AppInfo } from "../types";
import PlannerDialog from "./PlannerDialog";

function collect(entries: MenuEntry[], prefix: string, output: [string, string][]) {
  for (const entry of entries) {
    if (entry.kind === "separator") continue;
    if (entry.kind === "submenu") {
      collect(entry.items, `${prefix}${entry.label} › `, output);
    } else if (entry.accelerator) {
      output.push([`${prefix}${entry.label}`, displayAccelerator(entry.accelerator)]);
    }
  }
}

/** Elenco delle scorciatoie generato dai menu: sempre allineato alle azioni. */
export function ShortcutsDialog({ menus, onClose }: { menus: Menu[]; onClose: () => void }) {
  return (
    <PlannerDialog mark="⌨" onClose={onClose} title={t.keyboardShortcuts}>
      <div className="organization-dialog-form shortcuts-dialog">
        {menus.map((menu) => {
          const rows: [string, string][] = [];
          collect(menu.items, "", rows);
          if (rows.length === 0) return null;
          return (
            <section key={menu.id}>
              <h3>{menu.label}</h3>
              <dl className="info-grid">
                {rows.map(([label, keys], index) => (
                  <div key={`${index}-${label}`}>
                    <dt>{label}</dt>
                    <dd>
                      <kbd>{keys}</kbd>
                    </dd>
                  </div>
                ))}
              </dl>
            </section>
          );
        })}
        <section>
          <h3>{t.editorShortcuts}</h3>
          <dl className="info-grid">
            {[
              ["Ctrl+B", t.bold],
              ["Ctrl+I", t.italic],
              ["Ctrl+U", t.underline],
              ["Ctrl+Shift+S", t.strike],
              ["Ctrl+E", t.code],
              ["Ctrl+Shift+8", t.bulletList],
              ["Ctrl+Shift+7", t.orderedList],
              ["Ctrl+Shift+9", t.checklist],
              ["/", t.slashCommands],
            ].map(([keys, label]) => (
              <div key={keys}>
                <dt>{label}</dt>
                <dd>
                  <kbd>{keys}</kbd>
                </dd>
              </div>
            ))}
          </dl>
        </section>
      </div>
    </PlannerDialog>
  );
}

/** "Informazioni su Greatnote": versione, schema e cartella dati. */
export function AboutDialog({ onClose }: { onClose: () => void }) {
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    void invoke<AppInfo>("app_info")
      .then(setInfo)
      .catch((cause: unknown) => setError(errorMessage(cause)));
  }, []);
  return (
    <PlannerDialog mark="G" onClose={onClose} title={t.aboutTitle}>
      <div className="organization-dialog-form">
        <p className="data-hint">{t.aboutDescription}</p>
        {error && <p className="dialog-error">{error}</p>}
        {info && (
          <dl className="info-grid">
            <div>
              <dt>{t.aboutVersion}</dt>
              <dd>{info.version}</dd>
            </div>
            <div>
              <dt>{t.aboutSchema}</dt>
              <dd>v{info.schema_version}</dd>
            </div>
            <div>
              <dt>{t.aboutDataFolder}</dt>
              <dd className="selectable">{info.data_directory}</dd>
            </div>
            <div>
              <dt>{t.aboutDatabaseSize}</dt>
              <dd>{formatFileSize(info.database_bytes)}</dd>
            </div>
          </dl>
        )}
        <footer className="organization-dialog-actions">
          <button className="dialog-submit-button" onClick={onClose} type="button">
            {t.close}
          </button>
        </footer>
      </div>
    </PlannerDialog>
  );
}
