import { useState } from "react";
import { t } from "../../i18n";
import { aiApi } from "./aiApi";
import type { AiStatus } from "./aiApi";

/** Serve una conferma prima di inviare: server remoto e conferma attiva. */
export function needsConfirmation(status: AiStatus): boolean {
  return !status.local && status.settings.confirm_remote_sends;
}

/**
 * Casella "Non chiedermelo più" mostrata con la conferma d'invio (spuntata):
 * confermando, le richieste successive allo stesso server partono senza
 * conferma. Si riattiva dalle impostazioni AI.
 */
export function useRememberConsent(status: AiStatus, onStatus: (status: AiStatus) => void) {
  const [remember, setRemember] = useState(true);
  const commit = async () => {
    if (!remember || !needsConfirmation(status)) return;
    try {
      onStatus(await aiApi.updateSettings({ ...status.settings, confirm_remote_sends: false }));
    } catch {
      // La richiesta parte comunque; la conferma verrà chiesta di nuovo.
    }
  };
  const checkbox = (
    <label className="checkbox-field ai-remember-consent">
      <input checked={remember} onChange={(event) => setRemember(event.target.checked)} type="checkbox" />
      <span>{t.aiDontAskAgain(status.host)}</span>
    </label>
  );
  return { commit, checkbox };
}
