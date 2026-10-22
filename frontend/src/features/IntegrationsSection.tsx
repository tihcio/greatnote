import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { errorMessage, formatDateTime } from "../lib/format";
import CalendarIntegrations from "./calendar/CalendarIntegrations";

interface McpAuditEntry {
  at: string;
  tool: string;
  ok: boolean;
  results: number;
  note_id?: string;
  error?: string;
}

interface McpStatus {
  enabled: boolean;
  allow_write: boolean;
  command: string;
  args: string[];
  audit: McpAuditEntry[];
}

const READ_ONLY_TOOLS = ["search_notes", "read_note", "list_notebooks", "list_tags", "list_tasks"];
const WRITE_TOOLS = ["create_note", "append_to_note", "create_task", "complete_task"];

/** Argomento sicuro per la shell (POSIX): tra apici se serve. */
function shellQuote(value: string): string {
  return /^[\w@%+=:,./-]+$/.test(value) ? value : `'${value.replace(/'/g, `'\\''`)}'`;
}

/**
 * Integrazioni: server MCP locale, in sola lettura salvo che l'utente
 * consenta le modifiche non distruttive. Il client MCP (Claude
 * Code, Claude Desktop o altri) avvia Greatnote con `--mcp` e comunica
 * tramite stdin/stdout: nessuna porta di rete viene aperta.
 */
export default function IntegrationsSection() {
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const load = useCallback(() => {
    void invoke<McpStatus>("get_mcp_status")
      .then(setStatus)
      .catch((cause: unknown) => setError(errorMessage(cause)));
  }, []);

  useEffect(load, [load]);

  const setEnabled = async (enabled: boolean) => {
    setError(null);
    setNotice(null);
    try {
      setStatus(await invoke<McpStatus>("set_mcp_enabled", { enabled }));
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const setAllowWrite = async (allowed: boolean) => {
    setError(null);
    setNotice(null);
    try {
      setStatus(await invoke<McpStatus>("set_mcp_allow_write", { allowed }));
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const copy = async (text: string) => {
    try {
      await invoke("write_clipboard_text", { text });
      setNotice(t.mcpCopied);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  if (!status) {
    return error ? (
      <p className="dialog-error" role="alert">
        {error}
      </p>
    ) : (
      <p className="data-hint">{t.loading}</p>
    );
  }

  const claudeCommand = ["claude", "mcp", "add", "greatnote", "--", status.command, ...status.args]
    .map(shellQuote)
    .join(" ");
  const jsonConfig = JSON.stringify(
    { mcpServers: { greatnote: { command: status.command, args: status.args } } },
    null,
    2,
  );

  return (
    <div className="ai-settings integrations-settings">
      <CalendarIntegrations />
      <fieldset>
        <legend>{t.mcpTitle}</legend>
        <p className="data-hint">{t.mcpIntro}</p>
        <label className="checkbox-field">
          <input checked={status.enabled} onChange={(event) => void setEnabled(event.target.checked)} type="checkbox" />
          <span>{t.mcpEnable}</span>
        </label>
        <p className="data-hint">{status.enabled ? t.mcpEnabledHint : t.mcpDisabledHint}</p>
        <label className="checkbox-field">
          <input
            checked={status.allow_write}
            disabled={!status.enabled}
            onChange={(event) => void setAllowWrite(event.target.checked)}
            type="checkbox"
          />
          <span>{t.mcpAllowWrite}</span>
        </label>
        <p className="data-hint">{t.mcpAllowWriteHint}</p>
        <p className="data-hint">
          {t.mcpTools} <code>{READ_ONLY_TOOLS.join(", ")}</code>.{" "}
          {status.allow_write ? (
            <>
              {t.mcpWriteTools} <code>{WRITE_TOOLS.join(", ")}</code>.
            </>
          ) : (
            t.mcpNoWriteTools
          )}
        </p>
      </fieldset>
      <fieldset>
        <legend>{t.mcpSetup}</legend>
        <p className="data-hint">{t.mcpSetupClaudeCode}</p>
        <pre className="integration-snippet">{claudeCommand}</pre>
        <div className="ai-inline-actions">
          <button className="data-action" onClick={() => void copy(claudeCommand)} type="button">
            {t.mcpCopyCommand}
          </button>
        </div>
        <p className="data-hint">{t.mcpSetupOther}</p>
        <pre className="integration-snippet">{jsonConfig}</pre>
        <div className="ai-inline-actions">
          <button className="data-action" onClick={() => void copy(jsonConfig)} type="button">
            {t.mcpCopyJson}
          </button>
        </div>
        {notice && <p className="data-hint">{notice}</p>}
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
      </fieldset>
      <fieldset>
        <legend>{t.mcpActivity}</legend>
        <p className="data-hint">{t.mcpActivityHint}</p>
        {status.audit.length === 0 ? (
          <p className="data-hint">{t.mcpNoActivity}</p>
        ) : (
          <ul className="integration-audit">
            {status.audit.map((entry, index) => (
              <li key={`${entry.at}-${index}`}>
                <time>{formatDateTime(entry.at)}</time>
                <code>{entry.tool}</code>
                <span className={entry.ok ? undefined : "integration-audit-error"}>
                  {entry.ok ? t.mcpResults(entry.results) : entry.error ?? t.mcpFailed}
                </span>
              </li>
            ))}
          </ul>
        )}
        <div className="ai-inline-actions">
          <button className="data-action" onClick={load} type="button">
            {t.mcpRefresh}
          </button>
        </div>
      </fieldset>
    </div>
  );
}
