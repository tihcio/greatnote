import { useState } from "react";
import { t } from "../../i18n";
import { askConfirmation } from "../../lib/confirm";
import { errorMessage, formatDateTime } from "../../lib/format";
import { askText } from "../../lib/prompt";
import { SUBSCRIPTION_COLORS, calendarApi, safeColor } from "./calendarApi";
import type { CalendarStatus, CalendarSubscription } from "./calendarApi";

interface IcsSubscriptionsProps {
  status: CalendarStatus;
  onStatus: (status: CalendarStatus) => void;
}

/**
 * Calendari iCal in sola lettura: basta incollare l'indirizzo segreto del
 * calendario (Google, Outlook, Apple…), senza accesso all'account né
 * registrazioni. L'indirizzo viene conservato nel portachiavi.
 */
export default function IcsSubscriptions({ status, onStatus }: IcsSubscriptionsProps) {
  const [url, setUrl] = useState("");
  const [name, setName] = useState("");
  const [color, setColor] = useState(SUBSCRIPTION_COLORS[status.subscriptions.length % SUBSCRIPTION_COLORS.length]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const run = async (action: () => Promise<CalendarStatus>, done?: string) => {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      onStatus(await action());
      if (done) setNotice(done);
      return true;
    } catch (cause) {
      setError(errorMessage(cause));
      return false;
    } finally {
      setBusy(false);
    }
  };

  const add = async () => {
    const added = await run(() => calendarApi.addSubscription(url, name.trim() || null, color), t.icsAdded);
    if (added) {
      setUrl("");
      setName("");
      setColor(SUBSCRIPTION_COLORS[(status.subscriptions.length + 1) % SUBSCRIPTION_COLORS.length]);
    }
  };

  const rename = async (subscription: CalendarSubscription) => {
    const value = await askText({
      title: t.icsRenameTitle,
      label: t.icsName,
      initialValue: subscription.name,
      confirmLabel: t.icsRename,
      maxLength: 200,
      validate: (text) => (text.trim() ? null : t.icsNameRequired),
    });
    if (value) await run(() => calendarApi.updateSubscription(subscription.id, value, subscription.color));
  };

  const recolor = (subscription: CalendarSubscription, next: string) =>
    void run(() => calendarApi.updateSubscription(subscription.id, subscription.name, next));

  const remove = async (subscription: CalendarSubscription) => {
    const confirmed = await askConfirmation({
      title: t.icsRemoveTitle,
      message: t.icsRemoveMessage(subscription.name),
      confirmLabel: t.icsRemove,
    });
    if (confirmed) await run(() => calendarApi.removeSubscription(subscription.id));
  };

  const swatches = (selected: string | null, onPick: (value: string) => void, label: string) => (
    <div className="ics-colors" role="radiogroup" aria-label={label}>
      {SUBSCRIPTION_COLORS.map((value) => (
        <button
          aria-checked={selected === value}
          aria-label={value}
          className={`ics-color${selected === value ? " selected" : ""}`}
          disabled={busy}
          key={value}
          onClick={() => onPick(value)}
          role="radio"
          style={{ background: value }}
          type="button"
        />
      ))}
    </div>
  );

  return (
    <fieldset className="ics-subscriptions">
      <legend>{t.icsTitle}</legend>
      <p className="data-hint">{t.icsIntro}</p>
      {status.subscriptions.map((subscription) => (
        <section className="calendar-account" key={subscription.id} aria-label={subscription.name}>
          <header>
            <span className="calendar-swatch" aria-hidden="true" style={{ background: safeColor(subscription.color) }} />
            <strong>{subscription.name}</strong>
            <span>{t.icsEventCount(subscription.events)}</span>
          </header>
          <p className={subscription.last_error ? "dialog-error" : "data-hint"} role={subscription.last_error ? "alert" : undefined}>
            {subscription.last_error
              ? t.calendarSyncError(subscription.last_error)
              : subscription.last_sync_at
                ? t.calendarLastSync(formatDateTime(subscription.last_sync_at))
                : t.calendarNeverSynced}
          </p>
          {swatches(subscription.color, (value) => recolor(subscription, value), t.icsColor)}
          <div className="ai-inline-actions">
            <button className="data-action" disabled={busy} onClick={() => void rename(subscription)} type="button">
              {t.icsRename}
            </button>
            <button className="data-action destructive" disabled={busy} onClick={() => void remove(subscription)} type="button">
              {t.icsRemove}
            </button>
          </div>
        </section>
      ))}
      <form
        className="ics-add"
        onSubmit={(event) => {
          event.preventDefault();
          void add();
        }}
      >
        <label className="dialog-field">
          <span>{t.icsUrl}</span>
          <input
            autoComplete="off"
            onChange={(event) => setUrl(event.target.value)}
            placeholder="https://calendar.google.com/calendar/ical/…/basic.ics"
            required
            spellCheck={false}
            type="url"
            value={url}
          />
        </label>
        <label className="dialog-field">
          <span>{t.icsName}</span>
          <input maxLength={200} onChange={(event) => setName(event.target.value)} placeholder={t.icsNamePlaceholder} value={name} />
        </label>
        {swatches(color, setColor, t.icsColor)}
        <div className="ai-inline-actions">
          <button className="data-action primary" disabled={busy || !url.trim()} type="submit">
            {busy ? t.icsAdding : t.icsAdd}
          </button>
        </div>
      </form>
      <details className="ics-help">
        <summary>{t.icsHelpTitle}</summary>
        <p className="data-hint">{t.icsHelpGoogle}</p>
        <p className="data-hint">{t.icsHelpOutlook}</p>
        <p className="data-hint">{t.icsHelpPrivacy}</p>
      </details>
      {notice && <p className="data-hint">{notice}</p>}
      {error && (
        <p className="dialog-error" role="alert">
          {error}
        </p>
      )}
    </fieldset>
  );
}
