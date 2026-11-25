import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import { errorMessage } from "../lib/format";

interface BackgroundStatus {
  enabled: boolean;
  autostart: boolean;
  autostart_supported: boolean;
}

/** Modalità in background del desktop (ADR 0031): Greatnote resta attivo
 * nell'area di notifica perché i promemoria arrivino a finestra chiusa. */
export default function BackgroundSection({ onChange }: { onChange: (enabled: boolean) => void }) {
  const [status, setStatus] = useState<BackgroundStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void invoke<BackgroundStatus>("get_background_settings")
      .then(setStatus)
      .catch((cause: unknown) => setError(errorMessage(cause)));
  }, []);

  const save = async (enabled: boolean, autostart: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const saved = await invoke<BackgroundStatus>("set_background_settings", {
        settings: { enabled, autostart },
      });
      setStatus(saved);
      onChange(saved.enabled);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <fieldset className="background-settings">
      <legend>{t.backgroundTitle}</legend>
      <label className="checkbox-field">
        <input
          checked={status?.enabled ?? false}
          disabled={busy || !status}
          onChange={(event) => void save(event.target.checked, event.target.checked && (status?.autostart ?? false))}
          type="checkbox"
        />
        <span>{t.backgroundEnabled}</span>
      </label>
      <label className="checkbox-field">
        <input
          checked={status?.autostart ?? false}
          disabled={busy || !status?.enabled || !status.autostart_supported}
          onChange={(event) => void save(true, event.target.checked)}
          type="checkbox"
        />
        <span>{t.backgroundAutostart}</span>
      </label>
      <p className="data-hint">
        {status && !status.autostart_supported ? t.backgroundAutostartUnsupported : t.backgroundHint}
      </p>
      {error && (
        <p className="dialog-error" role="alert">
          {error}
        </p>
      )}
    </fieldset>
  );
}
