import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { language, t } from "../i18n";
import { askConfirmation } from "../lib/confirm";
import { errorMessage, formatDate } from "../lib/format";
import { askText } from "../lib/prompt";
import type { Template } from "../types";

interface TemplatesViewProps {
  refreshKey: number;
  /** Crea una nota dal modello e la apre. */
  onUse: (template: Template) => void;
  onChanged: () => void;
}

function preview(template: Template): string {
  const parts: string[] = [];
  const walk = (node: { text?: string; content?: unknown[] }) => {
    if (node.text) parts.push(node.text);
    node.content?.forEach((child) => walk(child as { text?: string; content?: unknown[] }));
  };
  walk(template.content as { content?: unknown[] });
  return parts.join(" · ").slice(0, 160);
}

/** Modelli predefiniti e dell'utente: usa, rinomina, sposta nel cestino. */
export default function TemplatesView({ refreshKey, onUse, onChanged }: TemplatesViewProps) {
  const [templates, setTemplates] = useState<Template[]>([]);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setTemplates(await invoke<Template[]>("list_templates", { language }));
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  const rename = async (template: Template) => {
    const name = await askText({
      title: t.renameTemplate,
      label: t.templateName,
      initialValue: template.name,
      confirmLabel: t.saveTask,
    });
    if (!name) return;
    try {
      await invoke<Template>("rename_template", { id: template.id, name });
      await load();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const trash = async (template: Template) => {
    const confirmed = await askConfirmation({
      title: t.actionMoveToTrash,
      message: t.confirmTrashTemplate(template.name),
      confirmLabel: t.actionMoveToTrash,
    });
    if (!confirmed) return;
    try {
      await invoke("move_item_to_trash", { kind: "template", id: template.id });
      await load();
      onChanged();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const builtin = templates.filter((template) => template.builtin);
  const own = templates.filter((template) => !template.builtin);
  const card = (template: Template) => (
    <li className="template-card" key={template.id}>
      <div className="template-body">
        <strong>{template.name}</strong>
        <p>{preview(template) || "—"}</p>
        {template.updated_at && <small>{t.updatedOn} {formatDate(template.updated_at)}</small>}
      </div>
      <div className="template-actions">
        <button className="subtle-action primary-action" onClick={() => onUse(template)} type="button">
          {t.useTemplate}
        </button>
        {!template.builtin && (
          <>
            <button className="text-button" onClick={() => void rename(template)} type="button">
              {t.renameTemplate}
            </button>
            <button className="text-button destructive-text" onClick={() => void trash(template)} type="button">
              {t.actionMoveToTrash}
            </button>
          </>
        )}
      </div>
    </li>
  );

  return (
    <section className="workspace-panel" aria-labelledby="templates-title">
      <header className="workspace-header">
        <div>
          <h1 id="templates-title">{t.templatesNav}</h1>
          <p className="workspace-subtitle">{t.templatesSubtitle}</p>
        </div>
      </header>
      <div className="workspace-scroll">
        {error && (
          <div className="inline-error" role="alert">
            {error}
          </div>
        )}
        <section className="reminder-section" aria-labelledby="own-templates">
          <h2 id="own-templates">{t.myTemplates}</h2>
          {own.length === 0 ? (
            <p className="workspace-empty">{t.noOwnTemplates}</p>
          ) : (
            <ul className="template-grid">{own.map(card)}</ul>
          )}
        </section>
        <section className="reminder-section" aria-labelledby="builtin-templates">
          <h2 id="builtin-templates">{t.builtinTemplates}</h2>
          <ul className="template-grid">{builtin.map(card)}</ul>
        </section>
      </div>
    </section>
  );
}
