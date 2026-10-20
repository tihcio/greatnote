import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export type ThemePreference = "system" | "light" | "dark";
export type NoteWidth = "normal" | "wide" | "full";
export type StartView = "home" | "notes";
/** Disposizione delle colonne: tutto, editor ampliato, solo editor. */
export type NoteLayout = "normal" | "expanded" | "focus";
/** Elenco delle note: schede con anteprima o righe compatte. */
export type NoteListMode = "cards" | "list";
export type SidebarSection = "navigation" | "recent" | "spaces" | "notebooks" | "tags";

const SIDEBAR_SECTIONS: SidebarSection[] = ["navigation", "recent", "spaces", "notebooks", "tags"];

export interface UiPreferences {
  theme: ThemePreference;
  zoom: number;
  noteWidth: NoteWidth;
  startView: StartView;
  sidebarHidden: boolean;
  showNoteCounts: boolean;
  showRecentNotes: boolean;
  showCreateButtons: boolean;
  groupInMore: boolean;
  showNotebooksInSpaces: boolean;
  slashCommands: boolean;
  noteListMode: NoteListMode;
  collapsedSidebarSections: SidebarSection[];
  /** Larghezze delle colonne (px), modificabili trascinandone i bordi. */
  sidebarWidth: number;
  listWidth: number;
}

export const SIDEBAR_WIDTH = { default: 230, min: 180, max: 480 };
export const LIST_WIDTH = { default: 294, min: 220, max: 720 };
/** Spazio minimo lasciato all'editor quando si allargano le colonne. */
export const MIN_EDITOR_WIDTH = 380;

export const DEFAULT_PREFERENCES: UiPreferences = {
  theme: "system",
  zoom: 1,
  noteWidth: "normal",
  startView: "home",
  sidebarHidden: false,
  showNoteCounts: false,
  showRecentNotes: true,
  showCreateButtons: true,
  groupInMore: true,
  showNotebooksInSpaces: true,
  slashCommands: true,
  noteListMode: "cards",
  collapsedSidebarSections: [],
  sidebarWidth: SIDEBAR_WIDTH.default,
  listWidth: LIST_WIDTH.default,
};

export const ZOOM_STEPS = [0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2];

/** Unisce i valori salvati con i predefiniti scartando tipi non validi. */
function sanitize(stored: unknown): UiPreferences {
  const result: UiPreferences = { ...DEFAULT_PREFERENCES };
  if (!stored || typeof stored !== "object") return result;
  const source = stored as Record<string, unknown>;
  for (const key of Object.keys(DEFAULT_PREFERENCES) as (keyof UiPreferences)[]) {
    if (key === "collapsedSidebarSections") continue;
    const value = source[key];
    if (typeof value === typeof DEFAULT_PREFERENCES[key]) {
      (result as unknown as Record<string, unknown>)[key] = value;
    }
  }
  const collapsedSections = source.collapsedSidebarSections;
  result.collapsedSidebarSections = Array.isArray(collapsedSections)
    ? collapsedSections.filter(
        (section): section is SidebarSection =>
          typeof section === "string" && SIDEBAR_SECTIONS.includes(section as SidebarSection),
      )
    : [];
  if (!["system", "light", "dark"].includes(result.theme)) result.theme = "system";
  if (!["normal", "wide", "full"].includes(result.noteWidth)) result.noteWidth = "normal";
  if (!["home", "notes"].includes(result.startView)) result.startView = "home";
  if (!["cards", "list"].includes(result.noteListMode)) result.noteListMode = "cards";
  const width = (value: number, range: { default: number; min: number; max: number }) =>
    Number.isFinite(value) ? Math.round(Math.min(range.max, Math.max(range.min, value))) : range.default;
  result.sidebarWidth = width(result.sidebarWidth, SIDEBAR_WIDTH);
  result.listWidth = width(result.listWidth, LIST_WIDTH);
  if (!(result.zoom >= 0.5 && result.zoom <= 2)) result.zoom = 1;
  return result;
}

export function applyTheme(theme: ThemePreference) {
  if (theme === "system") {
    delete document.documentElement.dataset.theme;
  } else {
    document.documentElement.dataset.theme = theme;
  }
  void invoke("set_window_theme", { theme }).catch(() => undefined);
}

/**
 * Preferenze salvate nel database (tabella `settings`, inclusa nei backup).
 * Le modifiche si applicano subito e vengono salvate con un breve ritardo.
 */
export function usePreferences(enabled: boolean) {
  const [preferences, setPreferences] = useState<UiPreferences>(DEFAULT_PREFERENCES);
  const [loaded, setLoaded] = useState(!enabled);
  const saveTimer = useRef<number | null>(null);

  useEffect(() => {
    if (!enabled) return;
    void invoke<unknown>("get_ui_preferences")
      .then((stored) => setPreferences(sanitize(stored)))
      .catch(() => undefined)
      .finally(() => setLoaded(true));
  }, [enabled]);

  useEffect(() => {
    applyTheme(preferences.theme);
  }, [preferences.theme]);

  useEffect(() => {
    if (!enabled) return;
    void invoke<number>("set_zoom", { level: preferences.zoom }).catch(() => undefined);
  }, [enabled, preferences.zoom]);

  const update = useCallback(
    (patch: Partial<UiPreferences>) => {
      setPreferences((current) => {
        const next = sanitize({ ...current, ...patch });
        if (enabled) {
          if (saveTimer.current) window.clearTimeout(saveTimer.current);
          saveTimer.current = window.setTimeout(() => {
            void invoke("set_ui_preferences", { preferences: next }).catch(() => undefined);
          }, 300);
        }
        return next;
      });
    },
    [enabled],
  );

  return { preferences, update, loaded };
}

export function nextZoom(current: number, direction: 1 | -1): number {
  if (direction > 0) return ZOOM_STEPS.find((step) => step > current + 0.001) ?? ZOOM_STEPS.at(-1)!;
  return [...ZOOM_STEPS].reverse().find((step) => step < current - 0.001) ?? ZOOM_STEPS[0];
}
