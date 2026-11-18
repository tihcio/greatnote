import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { isMobile } from "../app/platform";
import { t } from "../i18n";
import { errorMessage } from "../lib/format";

/**
 * Su Android: avviso se Greatnote non è esente dall'ottimizzazione della
 * batteria. Su alcuni telefoni (vivo, Xiaomi, Oppo…) senza esenzione l'app
 * chiusa viene congelata e il sistema cancella le notifiche programmate dei
 * promemoria. Il pulsante apre la richiesta di sistema; tornando nell'app lo
 * stato viene riletto.
 */
export default function BackgroundNotice() {
  const [allowed, setAllowed] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const check = useCallback(() => {
    void invoke<boolean>("get_background_allowed")
      .then((value) => setAllowed(value !== false))
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    if (!isMobile) return;
    check();
    const recheck = () => {
      if (document.visibilityState === "visible") check();
    };
    document.addEventListener("visibilitychange", recheck);
    return () => document.removeEventListener("visibilitychange", recheck);
  }, [check]);

  if (!isMobile || allowed) return null;
  return (
    <div className="inline-error notification-notice" role="status">
      <span>{t.backgroundRestricted}</span>
      <button
        className="subtle-action"
        onClick={() =>
          void invoke("request_background_allowed").catch((cause: unknown) => setError(errorMessage(cause)))
        }
        type="button"
      >
        {t.backgroundAllow}
      </button>
      {error && <span>{error}</span>}
    </div>
  );
}
