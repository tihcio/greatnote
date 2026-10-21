import { useState } from "react";
import { t } from "../../i18n";
import { errorMessage, formatDateTime, formatFileSize } from "../../lib/format";
import { updatesApi } from "./updatesApi";
import type { UpdatePhase, UpdateStatus } from "./updatesApi";

interface UpdatesSectionProps {
  status: UpdateStatus | null;
  onStatus: (status: UpdateStatus) => void;
  /** Salva le bozze, poi installa e riavvia. */
  onInstall: () => Promise<void>;
}

function phaseText(phase: UpdatePhase): string {
  switch (phase.state) {
    case "idle":
      return t.updateIdle;
    case "checking":
      return t.updateChecking;
    case "up-to-date":
      return t.updateUpToDate;
    case "downloading":
      return phase.total
        ? t.updateDownloading(phase.version, Math.round((phase.downloaded / phase.total) * 100))
        : t.updateDownloadingSize(phase.version, formatFileSize(phase.downloaded));
    case "ready":
      return t.updateReady(phase.version);
    case "installing":
      return t.updateInstalling;
    case "error":
      return phase.message;
  }
}

/** Impostazioni › Aggiornamenti: versione, controllo e installazione. */
export default function UpdatesSection({ status, onStatus, onInstall }: UpdatesSectionProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!status) return <p className="data-hint">{t.loading}</p>;
  const phase = status.phase;
  const working = busy || phase.state === "checking" || phase.state === "downloading" || phase.state === "installing";

  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <fieldset>
        <legend>{t.updatesTitle}</legend>
        <p>{t.updateCurrentVersion(status.current_version)}</p>
        {!status.supported ? (
          <p className="data-hint">{t.updateDevBuild}</p>
        ) : (
          <>
            <p className={phase.state === "error" ? "dialog-error" : undefined} role="status">
              {phaseText(phase)}
            </p>
            {status.last_check_at && (
              <p className="data-hint">{t.updateLastCheck(formatDateTime(status.last_check_at))}</p>
            )}
            {phase.state === "ready" && (
              <>
                {phase.notes && <p className="update-notes">{phase.notes}</p>}
                <p className="data-hint">{t.updateReadyHint}</p>
              </>
            )}
            <div className="ai-inline-actions">
              {phase.state === "ready" ? (
                <button
                  className="data-action primary"
                  disabled={working}
                  onClick={() => void run(onInstall)}
                  type="button"
                >
                  {t.updateInstallNow}
                </button>
              ) : (
                <button
                  className="data-action"
                  disabled={working}
                  onClick={() => void run(async () => onStatus(await updatesApi.check()))}
                  type="button"
                >
                  {t.updateCheckNow}
                </button>
              )}
            </div>
          </>
        )}
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
      </fieldset>
      {status.supported && (
        <fieldset>
          <legend>{t.updateAutomatic}</legend>
          <label className="checkbox-field">
            <input
              checked={status.auto_update}
              disabled={busy}
              onChange={(event) => {
                const enabled = event.target.checked;
                void run(async () => onStatus(await updatesApi.setAutoUpdate(enabled)));
              }}
              type="checkbox"
            />
            <span>{t.updateAutomaticLabel}</span>
          </label>
          <p className="data-hint">{t.updateAutomaticHint}</p>
        </fieldset>
      )}
    </>
  );
}
