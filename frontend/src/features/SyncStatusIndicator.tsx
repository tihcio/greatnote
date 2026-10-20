import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { t } from "../i18n";
import { formatDateTime } from "../lib/format";
import { SYNC_FINISHED_EVENT } from "./SyncSection";
import type { SyncView } from "./SyncSection";

interface SyncStatusIndicatorProps {
  /** Cambia quando le impostazioni si chiudono: lo stato va riletto. */
  refreshKey: unknown;
}

/**
 * Stato dell'archivio nella barra laterale: solo locale, sincronizzato con
 * Dropbox o con una cartella, sincronizzazione in corso o in errore. Si
 * aggiorna a ogni sincronizzazione (evento `sync-finished`).
 */
export default function SyncStatusIndicator({ refreshKey }: SyncStatusIndicatorProps) {
  const [view, setView] = useState<SyncView | null>(null);

  const load = useCallback(() => {
    // Se lo stato non è leggibile resta l'indicazione dell'archivio locale.
    void invoke<SyncView>("get_sync_status").then(setView, () => setView(null));
  }, []);

  useEffect(() => {
    load();
  }, [load, refreshKey]);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let cancelled = false;
    void listen(SYNC_FINISHED_EVENT, load).then((unlisten) => {
      if (cancelled) unlisten();
      else stop = unlisten;
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [load]);

  const settings = view?.settings;
  const enabled = Boolean(
    settings?.enabled && (settings.transport === "dropbox" ? settings.dropbox_account : settings.folder),
  );
  let state = "local";
  let label = t.localArchive;
  if (view && enabled) {
    if (view.running) {
      state = "syncing";
      label = t.syncStatusRunning;
    } else if (view.last_error) {
      state = "error";
      label = t.syncStatusError;
    } else {
      state = "synced";
      label = settings?.transport === "dropbox" ? t.syncStatusDropbox : t.syncStatusFolder;
    }
  }
  const title =
    view && enabled
      ? view.last_error ??
        (view.last_sync_at ? t.syncStatusLast(formatDateTime(view.last_sync_at)) : t.syncStatusNever)
      : undefined;

  return (
    <>
      <span aria-hidden="true" className={`status-dot ${state}`} />
      <span role="status" title={title}>
        {label}
      </span>
    </>
  );
}
