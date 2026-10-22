import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** Evento emesso dal backend a ogni cambio di stato (ADR 0023). */
export const UPDATE_STATUS_EVENT = "update-status";

export type UpdatePhase =
  | { state: "idle" }
  | { state: "checking" }
  | { state: "up-to-date" }
  | { state: "downloading"; version: string; downloaded: number; total: number | null }
  | { state: "ready"; version: string; notes: string | null }
  | { state: "installing" }
  | { state: "error"; message: string };

export interface UpdateStatus {
  current_version: string;
  /** Falso nella versione di sviluppo. */
  supported: boolean;
  auto_update: boolean;
  /** Falso per i pacchetti `.deb`/`.rpm`: si installano solo su richiesta. */
  installs_on_exit: boolean;
  last_check_at: string | null;
  phase: UpdatePhase;
}

export const updatesApi = {
  status: () => invoke<UpdateStatus>("get_update_status"),
  check: () => invoke<UpdateStatus>("check_for_updates"),
  setAutoUpdate: (enabled: boolean) => invoke<UpdateStatus>("set_auto_update", { enabled }),
  /** Installa e riavvia: chi chiama salva prima le bozze. */
  installNow: () => invoke<void>("install_update_now"),
};

/** Stato degli aggiornamenti, aggiornato dagli eventi del backend. */
export function useUpdateStatus(enabled: boolean): [UpdateStatus | null, (status: UpdateStatus) => void] {
  const [status, setStatus] = useState<UpdateStatus | null>(null);

  useEffect(() => {
    if (!enabled) return;
    let stop: (() => void) | undefined;
    let cancelled = false;
    void updatesApi.status().then(
      (value) => {
        if (!cancelled) setStatus(value);
      },
      () => undefined,
    );
    void listen<UpdateStatus>(UPDATE_STATUS_EVENT, (event) => setStatus(event.payload)).then((unlisten) => {
      if (cancelled) unlisten();
      else stop = unlisten;
    });
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [enabled]);

  return [status, setStatus];
}
