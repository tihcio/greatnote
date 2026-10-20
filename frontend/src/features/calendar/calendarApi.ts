import { invoke } from "@tauri-apps/api/core";
import type { CalendarEvent, EventInput } from "../../types";

/** Calendari esterni (Google Calendar, Microsoft Outlook/365). */
export type CalendarProvider = "google" | "microsoft";

/** Abbonamento iCal in sola lettura (l'indirizzo resta nel portachiavi). */
export interface CalendarSubscription {
  id: string;
  name: string;
  color: string | null;
  last_sync_at: string | null;
  last_error: string | null;
  events: number;
}

export interface ExternalCalendar {
  id: string;
  account_id: string;
  remote_id: string;
  name: string;
  color: string | null;
  can_edit: boolean;
  is_primary: boolean;
  selected: boolean;
}

export interface CalendarAccount {
  id: string;
  provider: CalendarProvider;
  label: string;
  last_sync_at: string | null;
  last_error: string | null;
  calendars: ExternalCalendar[];
}

export interface CalendarClients {
  google_client_id: string;
  microsoft_client_id: string;
}

export interface CalendarStatus {
  clients: CalendarClients;
  google_has_secret: boolean;
  /** Registrazioni incorporate nella build: basta accedere e consentire. */
  builtin_google: boolean;
  builtin_microsoft: boolean;
  accounts: CalendarAccount[];
  subscriptions: CalendarSubscription[];
  syncing: boolean;
  connecting: boolean;
}

/** Calendario con il suo account, per etichette e scelte nei dialog. */
export interface CalendarInfo {
  calendar: ExternalCalendar;
  account: CalendarAccount;
}

export const CALENDAR_SYNC_EVENT = "calendar-sync-finished";

export const calendarApi = {
  status: () => invoke<CalendarStatus>("get_calendar_status"),
  /** `googleClientSecret`: null lascia quello salvato, "" lo elimina. */
  setClients: (clients: CalendarClients, googleClientSecret: string | null) =>
    invoke<CalendarStatus>("set_calendar_clients", { clients, googleClientSecret }),
  connect: (provider: CalendarProvider) => invoke<CalendarStatus>("connect_calendar_account", { provider }),
  cancelConnect: () => invoke<void>("cancel_calendar_connect"),
  disconnect: (accountId: string) => invoke<CalendarStatus>("disconnect_calendar_account", { accountId }),
  setSelected: (calendarId: string, selected: boolean) =>
    invoke<CalendarStatus>("set_external_calendar_selected", { calendarId, selected }),
  syncNow: () => invoke<CalendarStatus>("sync_calendars_now"),
  addSubscription: (url: string, name: string | null, color: string | null) =>
    invoke<CalendarStatus>("add_calendar_subscription", { url, name, color }),
  updateSubscription: (id: string, name: string, color: string | null) =>
    invoke<CalendarStatus>("update_calendar_subscription", { id, name, color }),
  removeSubscription: (id: string) => invoke<CalendarStatus>("remove_calendar_subscription", { id }),
  createEvent: (calendarId: string, input: EventInput) =>
    invoke<CalendarEvent>("create_external_event", { calendarId, input }),
  updateEvent: (id: string, input: EventInput) => invoke<CalendarEvent>("update_external_event", { id, input }),
  deleteEvent: (id: string) => invoke<void>("delete_external_event", { id }),
  openEvent: (id: string) => invoke<void>("open_external_event", { id }),
};

/** Mappa id calendario → calendario e account. */
export function calendarIndex(accounts: CalendarAccount[]): Map<string, CalendarInfo> {
  const map = new Map<string, CalendarInfo>();
  for (const account of accounts) {
    for (const calendar of account.calendars) map.set(calendar.id, { calendar, account });
  }
  return map;
}

/** Colori proposti per i calendari iCal. */
export const SUBSCRIPTION_COLORS = ["#1e88e5", "#43a047", "#e53935", "#fb8c00", "#8e24aa", "#00897b", "#6d4c41", "#546e7a"];

/** Colore sicuro per lo stile (solo #rgb/#rrggbb). */
export function safeColor(color: string | null | undefined): string | undefined {
  return color && /^#[0-9a-f]{3}([0-9a-f]{3})?$/i.test(color) ? color : undefined;
}
