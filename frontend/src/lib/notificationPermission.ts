import { invoke } from "@tauri-apps/api/core";
import { isMobile } from "../app/platform";

/**
 * Su Android i promemoria sono notifiche di sistema e servono il permesso
 * delle notifiche (src-tauri/src/reminder_alarms.rs). Mostra la richiesta di
 * Android se non è ancora stata decisa e restituisce lo stato: `granted`,
 * `denied` o `prompt`. Sul desktop è sempre `granted`.
 */
export async function ensureNotificationPermission(): Promise<string> {
  if (!isMobile) return "granted";
  return invoke<string>("request_notification_permission");
}

const ASKED_KEY = "greatnote.notifications.askedAtStart";

/**
 * All'avvio su Android: se ci sono promemoria programmati (anche arrivati
 * dalla sincronizzazione, creati su un altro dispositivo) e il permesso non
 * è mai stato deciso, lo chiede una volta. Le volte successive l'avviso
 * resta nella vista Promemoria, senza insistere.
 */
export async function askNotificationPermissionAtStart(hasScheduledReminders: boolean): Promise<void> {
  if (!isMobile || !hasScheduledReminders) return;
  try {
    if (window.localStorage.getItem(ASKED_KEY)) return;
  } catch {
    // Memoria non disponibile: si chiede comunque solo se il permesso manca.
  }
  const state = await invoke<string>("get_notification_permission");
  if (state !== "prompt") return;
  try {
    window.localStorage.setItem(ASKED_KEY, "1");
  } catch {
    // Nessun ricordo della richiesta: al prossimo avvio si ripete.
  }
  await invoke<string>("request_notification_permission");
}
