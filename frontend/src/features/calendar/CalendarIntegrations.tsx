import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { t } from "../../i18n";
import { askConfirmation } from "../../lib/confirm";
import { errorMessage, formatDateTime } from "../../lib/format";
import { CALENDAR_SYNC_EVENT, calendarApi, safeColor } from "./calendarApi";
import type { CalendarAccount, CalendarProvider, CalendarStatus } from "./calendarApi";
import IcsSubscriptions from "./IcsSubscriptions";

export const providerName = (provider: CalendarProvider | "ics") =>
  provider === "google" ? t.calendarGoogle : provider === "microsoft" ? t.calendarMicrosoft : t.icsProvider;

/**
 * Impostazioni dei calendari esterni: registrazione dell'app presso Google e
 * Microsoft (ID client inseriti dall'utente), collegamento degli account nel
 * browser, scelta dei calendari, sincronizzazione e scollegamento.
 */
export default function CalendarIntegrations() {
  const [status, setStatus] = useState<CalendarStatus | null>(null);
  const [googleId, setGoogleId] = useState("");
  const [googleSecret, setGoogleSecret] = useState("");
  const [microsoftId, setMicrosoftId] = useState("");
  const [busy, setBusy] = useState(false);
  const [connecting, setConnecting] = useState<CalendarProvider | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const apply = useCallback((next: CalendarStatus) => {
    setStatus(next);
    setGoogleId(next.clients.google_client_id);
    setMicrosoftId(next.clients.microsoft_client_id);
  }, []);

  const load = useCallback(() => {
    void calendarApi
      .status()
      .then(apply)
      .catch((cause: unknown) => setError(errorMessage(cause)));
  }, [apply]);

  useEffect(() => {
    load();
    let stop: (() => void) | null = null;
    let cancelled = false;
    void listen(CALENDAR_SYNC_EVENT, () => {
      void calendarApi.status().then(setStatus).catch(() => undefined);
    }).then((unlisten) => (cancelled ? unlisten() : (stop = unlisten)));
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [load]);

  const run = async (action: () => Promise<CalendarStatus>, done?: string) => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const next = await action();
      setStatus(next);
      if (done) setNotice(done);
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  };

  const saveClients = async () => {
    await run(
      () =>
        calendarApi.setClients(
          { google_client_id: googleId, microsoft_client_id: microsoftId },
          googleSecret.trim() ? googleSecret : null,
        ),
      t.calendarClientsSaved,
    );
    setGoogleSecret("");
  };

  const connect = async (provider: CalendarProvider) => {
    setConnecting(provider);
    await run(() => calendarApi.connect(provider), t.calendarConnected);
    setConnecting(null);
  };

  const disconnect = async (account: CalendarAccount) => {
    const confirmed = await askConfirmation({
      title: t.calendarDisconnectTitle,
      message: t.calendarDisconnectMessage(account.label, providerName(account.provider)),
      confirmLabel: t.calendarDisconnect,
    });
    if (confirmed) await run(() => calendarApi.disconnect(account.id));
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

  // Senza registrazioni incluse nella build né configurazioni esistenti il
  // collegamento non è utilizzabile: la sezione non viene mostrata (ADR 0015).
  const available =
    status.builtin_google ||
    status.builtin_microsoft ||
    status.accounts.length > 0 ||
    Boolean(status.clients.google_client_id || status.clients.microsoft_client_id);
  const subscriptions = <IcsSubscriptions onStatus={setStatus} status={status} />;
  if (!available) return subscriptions;

  const changedClients =
    googleId.trim() !== status.clients.google_client_id ||
    microsoftId.trim() !== status.clients.microsoft_client_id ||
    googleSecret.trim() !== "";

  return (
    <>
      {subscriptions}
      <fieldset className="calendar-integrations">
        <legend>{t.calendarIntegrationsTitle}</legend>
        <p className="data-hint">{t.calendarIntegrationsIntro}</p>
        {status.accounts.map((account) => (
          <section className="calendar-account" key={account.id} aria-label={`${providerName(account.provider)} ${account.label}`}>
            <header>
              <strong>{providerName(account.provider)}</strong>
              <span>{account.label}</span>
            </header>
            <p className={account.last_error ? "dialog-error" : "data-hint"} role={account.last_error ? "alert" : undefined}>
              {account.last_error
                ? t.calendarSyncError(account.last_error)
                : account.last_sync_at
                  ? t.calendarLastSync(formatDateTime(account.last_sync_at))
                  : t.calendarNeverSynced}
            </p>
            <ul className="calendar-account-calendars">
              {account.calendars.map((calendar) => (
                <li key={calendar.id}>
                  <label className="checkbox-field">
                    <input
                      checked={calendar.selected}
                      disabled={busy}
                      onChange={(event) => void run(() => calendarApi.setSelected(calendar.id, event.target.checked))}
                      type="checkbox"
                    />
                    <span className="calendar-swatch" style={{ background: safeColor(calendar.color) }} aria-hidden="true" />
                    <span>
                      {calendar.name}
                      {!calendar.can_edit && <small> · {t.calendarReadOnly}</small>}
                    </span>
                  </label>
                </li>
              ))}
            </ul>
            <div className="ai-inline-actions">
              <button className="data-action destructive" disabled={busy} onClick={() => void disconnect(account)} type="button">
                {t.calendarDisconnect}
              </button>
            </div>
          </section>
        ))}
        {status.accounts.length > 0 && (
          <div className="ai-inline-actions">
            <button
              className="data-action"
              disabled={busy || status.syncing}
              onClick={() => void run(calendarApi.syncNow, t.calendarSynced)}
              type="button"
            >
              {status.syncing ? t.calendarSyncing : t.calendarSyncNow}
            </button>
          </div>
        )}
        {connecting ? (
          <div className="ai-remote-warning" role="status">
            <strong>{t.calendarConnectingTitle}</strong>
            <p>{t.calendarConnectingHint(providerName(connecting))}</p>
            <button className="data-action" onClick={() => void calendarApi.cancelConnect()} type="button">
              {t.cancel}
            </button>
          </div>
        ) : (
          <div className="ai-inline-actions">
            {(["google", "microsoft"] as const).map((provider) => {
              const configured =
                provider === "google"
                  ? status.builtin_google || status.clients.google_client_id
                  : status.builtin_microsoft || status.clients.microsoft_client_id;
              return (
                <button
                  className="data-action primary"
                  disabled={busy || !configured}
                  key={provider}
                  onClick={() => void connect(provider)}
                  title={configured ? undefined : t.calendarNeedsClientId}
                  type="button"
                >
                  {t.calendarConnect(providerName(provider))}
                </button>
              );
            })}
          </div>
        )}
        {notice && <p className="data-hint">{notice}</p>}
        {error && (
          <p className="dialog-error" role="alert">
            {error}
          </p>
        )}
      </fieldset>
      <details className="calendar-advanced" open={!status.builtin_google && !status.builtin_microsoft}>
        <summary>{t.calendarClientsTitle}</summary>
        <p className="data-hint">
          {status.builtin_google || status.builtin_microsoft ? t.calendarClientsOverride : t.calendarClientsIntro}
        </p>
        <label className="dialog-field">
          <span>{t.calendarGoogleClientId}</span>
          <input
            autoComplete="off"
            onChange={(event) => setGoogleId(event.target.value)}
            placeholder="1234567890-abc.apps.googleusercontent.com"
            spellCheck={false}
            value={googleId}
          />
        </label>
        <label className="dialog-field">
          <span>{t.calendarGoogleClientSecret}</span>
          <input
            autoComplete="off"
            onChange={(event) => setGoogleSecret(event.target.value)}
            placeholder={status.google_has_secret ? t.calendarSecretSaved : t.calendarSecretOptional}
            spellCheck={false}
            type="password"
            value={googleSecret}
          />
        </label>
        <label className="dialog-field">
          <span>{t.calendarMicrosoftClientId}</span>
          <input
            autoComplete="off"
            onChange={(event) => setMicrosoftId(event.target.value)}
            placeholder="00000000-0000-0000-0000-000000000000"
            spellCheck={false}
            value={microsoftId}
          />
        </label>
        <div className="ai-inline-actions">
          <button className="data-action primary" disabled={busy || !changedClients} onClick={() => void saveClients()} type="button">
            {t.calendarSaveClients}
          </button>
          {status.google_has_secret && (
            <button
              className="data-action"
              disabled={busy}
              onClick={() => void run(() => calendarApi.setClients(status.clients, ""))}
              type="button"
            >
              {t.calendarRemoveSecret}
            </button>
          )}
        </div>
      </details>
    </>
  );
}
