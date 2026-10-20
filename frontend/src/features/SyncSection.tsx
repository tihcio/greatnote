import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { t } from "../i18n";
import { askConfirmation } from "../lib/confirm";
import { errorMessage, formatDateTime } from "../lib/format";

export type SyncTransport = "folder" | "dropbox";

export interface SyncSettings {
  enabled: boolean;
  transport: SyncTransport;
  folder: string;
  dropbox_account: string;
  device_name: string;
}

export interface SyncConflict {
  id: string;
  entity: string;
  entity_key: string;
  title: string;
  from_device: string;
  created_at: string;
}

export interface SyncView {
  settings: SyncSettings;
  device_id: string | null;
  last_sync_at: string | null;
  last_error: string | null;
  pending_changes: number;
  missing_attachments: number;
  devices: {
    device_id: string;
    name: string;
    applied_seq: number;
    last_seen_at: string | null;
    this_device: boolean;
  }[];
  conflicts: SyncConflict[];
  running: boolean;
  /** Questa versione include il collegamento a Dropbox. */
  dropbox_available: boolean;
  connecting: boolean;
}

export const SYNC_FINISHED_EVENT = "sync-finished";

interface SyncSectionProps {
  onOpenNote: (noteId: string) => void;
}

/**
 * Sincronizzazione tra dispositivi tramite una cartella già sincronizzata
 * (Nextcloud, Syncthing, Dropbox…): attivazione, stato, dispositivi e
 * conflitti conservati.
 */
export default function SyncSection({ onOpenNote }: SyncSectionProps) {
  const [view, setView] = useState<SyncView | null>(null);
  const [folder, setFolder] = useState("");
  const [deviceName, setDeviceName] = useState("");
  const [transport, setTransport] = useState<SyncTransport>("dropbox");
  const [connecting, setConnecting] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const next = await invoke<SyncView>("get_sync_status");
      setView(next);
      return next;
    } catch (cause) {
      setError(errorMessage(cause));
      return null;
    }
  }, []);

  useEffect(() => {
    void load().then((next) => {
      if (next) {
        setFolder(next.settings.folder);
        setDeviceName(next.settings.device_name);
        // Prima configurazione: Dropbox se disponibile, altrimenti cartella.
        setTransport(
          next.settings.enabled || next.settings.dropbox_account || next.settings.folder
            ? next.settings.transport
            : next.dropbox_available
              ? "dropbox"
              : "folder",
        );
      }
    });
    let stop: (() => void) | null = null;
    let cancelled = false;
    void listen(SYNC_FINISHED_EVENT, () => void load()).then((unlisten) =>
      cancelled ? unlisten() : (stop = unlisten),
    );
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [load]);

  const run = async (action: () => Promise<SyncView>, done?: string) => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      setView(await action());
      if (done) setNotice(done);
    } catch (cause) {
      setError(errorMessage(cause));
      void load();
    } finally {
      setBusy(false);
    }
  };

  const pickFolder = async () => {
    try {
      const picked = await invoke<string | null>("pick_sync_folder", {
        title: t.syncPickFolder,
      });
      if (picked) setFolder(picked);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const folderSettings = (): SyncSettings => ({
    enabled: true,
    transport: "folder",
    folder,
    dropbox_account: view?.settings.dropbox_account ?? "",
    device_name: deviceName,
  });

  const enable = async () => {
    if (!view) return;
    if (view.settings.enabled && view.settings.transport === "folder" && folder === view.settings.folder) {
      await run(() => invoke<SyncView>("set_sync_settings", { settings: folderSettings() }), t.syncSaved);
      return;
    }
    const confirmed = await askConfirmation({
      title: t.syncEnableTitle,
      message: t.syncEnableMessage(folder),
      confirmLabel: t.syncEnable,
    });
    if (confirmed) {
      await run(() => invoke<SyncView>("set_sync_settings", { settings: folderSettings() }), t.syncEnabled);
    }
  };

  const connectDropbox = async () => {
    const confirmed = await askConfirmation({
      title: t.dropboxConnectTitle,
      message: t.dropboxConnectMessage,
      confirmLabel: t.dropboxConnect,
    });
    if (!confirmed) return;
    setConnecting(true);
    await run(() => invoke<SyncView>("connect_dropbox", { deviceName }), t.dropboxConnected);
    setConnecting(false);
  };

  const disconnectDropbox = async () => {
    const confirmed = await askConfirmation({
      title: t.dropboxDisconnectTitle,
      message: t.dropboxDisconnectMessage,
      confirmLabel: t.dropboxDisconnect,
    });
    if (confirmed) await run(() => invoke<SyncView>("disconnect_dropbox"));
  };

  const disable = async () => {
    if (!view) return;
    const confirmed = await askConfirmation({
      title: t.syncDisableTitle,
      message: t.syncDisableMessage,
      confirmLabel: t.syncDisable,
    });
    if (confirmed) {
      await run(() =>
        invoke<SyncView>("set_sync_settings", {
          settings: { ...view.settings, enabled: false },
        }),
      );
    }
  };

  const dismiss = async (conflict: SyncConflict) => {
    try {
      await invoke("dismiss_sync_conflict", { id: conflict.id });
      void load();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  if (!view) {
    return error ? (
      <p className="dialog-error" role="alert">
        {error}
      </p>
    ) : (
      <p className="data-hint">{t.loading}</p>
    );
  }

  const enabled = view.settings.enabled;
  const dropboxActive = enabled && view.settings.transport === "dropbox";
  const folderActive = enabled && view.settings.transport === "folder";
  const otherDevices = view.devices.filter((device) => !device.this_device);

  return (
    <div className="ai-settings sync-settings">
      <fieldset>
        <legend>{t.syncTitle}</legend>
        <p className="data-hint">{t.syncIntro}</p>
        <div className="segmented sync-transport" role="radiogroup" aria-label={t.syncWhere}>
          {(["dropbox", "folder"] as const).map((value) => (
            <button
              aria-checked={transport === value}
              className={`segment${transport === value ? " selected" : ""}`}
              key={value}
              onClick={() => setTransport(value)}
              role="radio"
              type="button"
            >
              {value === "dropbox" ? t.syncTransportDropbox : t.syncTransportFolder}
            </button>
          ))}
        </div>
        <label className="dialog-field">
          <span>{t.syncDeviceName}</span>
          <input
            maxLength={100}
            onChange={(event) => setDeviceName(event.target.value)}
            placeholder={t.syncDeviceNamePlaceholder}
            value={deviceName}
          />
        </label>
        {transport === "dropbox" ? (
          <>
            <p className="data-hint">{t.dropboxIntro}</p>
            {dropboxActive ? (
              <p className="sync-account">{t.dropboxConnectedAs(view.settings.dropbox_account)}</p>
            ) : !view.dropbox_available ? (
              <p className="data-hint">{t.dropboxUnavailable}</p>
            ) : null}
            {connecting || view.connecting ? (
              <div className="ai-remote-warning" role="status">
                <strong>{t.calendarConnectingTitle}</strong>
                <p>{t.dropboxConnectingHint}</p>
                <button className="data-action" onClick={() => void invoke("cancel_dropbox_connect")} type="button">
                  {t.cancel}
                </button>
              </div>
            ) : (
              <div className="ai-inline-actions">
                {dropboxActive ? (
                  <>
                    <button
                      className="data-action"
                      disabled={busy || view.running}
                      onClick={() => void run(() => invoke<SyncView>("sync_now"), t.syncDone)}
                      type="button"
                    >
                      {view.running || busy ? t.syncRunning : t.syncNow}
                    </button>
                    <button
                      className="data-action destructive"
                      disabled={busy}
                      onClick={() => void disconnectDropbox()}
                      type="button"
                    >
                      {t.dropboxDisconnect}
                    </button>
                  </>
                ) : (
                  <button
                    className="data-action primary"
                    disabled={busy || !view.dropbox_available}
                    onClick={() => void connectDropbox()}
                    type="button"
                  >
                    {t.dropboxConnect}
                  </button>
                )}
              </div>
            )}
          </>
        ) : (
          <>
            <div className="dialog-field">
              <span>{t.syncFolder}</span>
              <div className="sync-folder-row">
                <input
                  aria-label={t.syncFolder}
                  onChange={(event) => setFolder(event.target.value)}
                  placeholder={t.syncFolderPlaceholder}
                  spellCheck={false}
                  value={folder}
                />
                <button className="data-action" disabled={busy} onClick={() => void pickFolder()} type="button">
                  {t.syncChoose}
                </button>
              </div>
            </div>
            <div className="ai-inline-actions">
              <button
                className="data-action primary"
                disabled={busy || !folder.trim()}
                onClick={() => void enable()}
                type="button"
              >
                {folderActive ? t.syncSave : t.syncEnable}
              </button>
              {folderActive && (
                <>
                  <button
                    className="data-action"
                    disabled={busy || view.running}
                    onClick={() => void run(() => invoke<SyncView>("sync_now"), t.syncDone)}
                    type="button"
                  >
                    {view.running || busy ? t.syncRunning : t.syncNow}
                  </button>
                  <button
                    className="data-action destructive"
                    disabled={busy}
                    onClick={() => void disable()}
                    type="button"
                  >
                    {t.syncDisable}
                  </button>
                </>
              )}
            </div>
          </>
        )}
        {enabled && (
          <dl className="sync-facts">
            <div>
              <dt>{t.syncLastSync}</dt>
              <dd>{view.last_sync_at ? formatDateTime(view.last_sync_at) : t.syncNever}</dd>
            </div>
            <div>
              <dt>{t.syncPending}</dt>
              <dd>{view.pending_changes}</dd>
            </div>
            {view.missing_attachments > 0 && (
              <div>
                <dt>{t.syncMissingAttachments}</dt>
                <dd>{view.missing_attachments}</dd>
              </div>
            )}
          </dl>
        )}
        {enabled && view.last_error && (
          <p className="dialog-error" role="alert">
            {t.syncErrorPrefix} {view.last_error}
          </p>
        )}
        {notice && <p className="data-hint">{notice}</p>}
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
      </fieldset>
      {enabled && (
        <fieldset>
          <legend>{t.syncDevices}</legend>
          {otherDevices.length === 0 ? (
            <p className="data-hint">{t.syncNoDevices}</p>
          ) : (
            <ul className="integration-audit sync-devices">
              {otherDevices.map((device) => (
                <li key={device.device_id}>
                  <strong>{device.name || t.syncUnnamedDevice}</strong>
                  <span>{device.last_seen_at ? t.syncDeviceSeen(formatDateTime(device.last_seen_at)) : ""}</span>
                </li>
              ))}
            </ul>
          )}
        </fieldset>
      )}
      {view.conflicts.length > 0 && (
        <fieldset>
          <legend>{t.syncConflicts}</legend>
          <p className="data-hint">{t.syncConflictsIntro}</p>
          <ul className="sync-conflicts">
            {view.conflicts.map((conflict) => (
              <li key={conflict.id}>
                <div>
                  <strong>{conflict.title || t.untitled}</strong>
                  <small>
                    {t.syncConflictFrom(conflict.from_device || t.syncThisDevice, formatDateTime(conflict.created_at))}
                  </small>
                </div>
                {conflict.entity === "notes" && (
                  <button className="text-button" onClick={() => onOpenNote(conflict.entity_key)} type="button">
                    {t.syncOpenNote}
                  </button>
                )}
                <button className="text-button" onClick={() => void dismiss(conflict)} type="button">
                  {t.syncDismiss}
                </button>
              </li>
            ))}
          </ul>
        </fieldset>
      )}
      <details className="ics-help">
        <summary>{t.syncHowTitle}</summary>
        <p className="data-hint">{t.syncHowFolder}</p>
        <p className="data-hint">{t.syncHowConflicts}</p>
        <p className="data-hint">{t.syncHowPrivacy}</p>
        <p className="data-hint">{t.syncHowCopy}</p>
      </details>
    </div>
  );
}
