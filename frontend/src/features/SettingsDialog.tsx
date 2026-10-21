import { useState } from "react";
import UpdatesSection from "./updates/UpdatesSection";
import type { UpdateStatus } from "./updates/updatesApi";
import { t } from "../i18n";
import type { NoteWidth, StartView, ThemePreference, UiPreferences } from "../app/preferences";
import { ZOOM_STEPS } from "../app/preferences";
import type { Notebook } from "../types";
import AiSettingsSection from "./ai/AiSettingsSection";
import IntegrationsSection from "./IntegrationsSection";
import SyncSection from "./SyncSection";
import type { AiStatus, IndexProgress } from "./ai/aiApi";
import PlannerDialog from "./PlannerDialog";

export type SettingsSection =
  | "general"
  | "sidebar"
  | "notes"
  | "ai"
  | "integrations"
  | "sync"
  | "data"
  | "updates";

interface SettingsDialogProps {
  preferences: UiPreferences;
  initialSection?: SettingsSection;
  aiStatus: AiStatus | null;
  aiProgress: IndexProgress | null;
  notebooks: Notebook[];
  onAiStatus: (status: AiStatus) => void;
  onStartIndexing: () => void;
  onCancelIndexing: () => void;
  onChange: (patch: Partial<UiPreferences>) => void;
  onOpenData: () => void;
  onShowShortcuts: () => void;
  /** Apre una nota (per esempio da un conflitto di sincronizzazione). */
  onOpenNote: (noteId: string) => void;
  updateStatus: UpdateStatus | null;
  onUpdateStatus: (status: UpdateStatus) => void;
  onInstallUpdate: () => Promise<void>;
  onClose: () => void;
}

/** Impostazioni (Ctrl+,): ogni opzione è applicata e salvata subito. */
export default function SettingsDialog({
  preferences,
  initialSection = "general",
  aiStatus,
  aiProgress,
  notebooks,
  onAiStatus,
  onStartIndexing,
  onCancelIndexing,
  onChange,
  onOpenData,
  onShowShortcuts,
  onOpenNote,
  updateStatus,
  onUpdateStatus,
  onInstallUpdate,
  onClose,
}: SettingsDialogProps) {
  const [section, setSection] = useState<SettingsSection>(initialSection);
  const sections: { id: SettingsSection; label: string }[] = [
    { id: "general", label: t.settingsGeneral },
    { id: "sidebar", label: t.settingsSidebar },
    { id: "notes", label: t.settingsNotes },
    { id: "ai", label: t.settingsAi },
    { id: "integrations", label: t.settingsIntegrations },
    { id: "sync", label: t.settingsSync },
    { id: "data", label: t.settingsData },
    { id: "updates", label: t.updatesTitle },
  ];
  const toggle = (key: keyof UiPreferences, label: string) => (
    <label className="checkbox-field">
      <input
        checked={Boolean(preferences[key])}
        onChange={(event) => onChange({ [key]: event.target.checked } as Partial<UiPreferences>)}
        type="checkbox"
      />
      <span>{label}</span>
    </label>
  );
  const radios = <T extends string>(
    name: string,
    value: T,
    options: { value: T; label: string }[],
    update: (value: T) => void,
  ) => (
    <div className="settings-radios" role="radiogroup">
      {options.map((option) => (
        <label className="checkbox-field" key={option.value}>
          <input
            checked={value === option.value}
            name={name}
            onChange={() => update(option.value)}
            type="radio"
          />
          <span>{option.label}</span>
        </label>
      ))}
    </div>
  );

  return (
    <PlannerDialog mark="⚙" onClose={onClose} title={t.settingsTitle}>
      <div className="settings-layout">
        <nav aria-label={t.settingsTitle} className="settings-nav">
          {sections.map((item) => (
            <button
              aria-current={section === item.id ? "page" : undefined}
              className={`settings-nav-item${section === item.id ? " selected" : ""}`}
              key={item.id}
              onClick={() => setSection(item.id)}
              type="button"
            >
              {item.label}
            </button>
          ))}
        </nav>
        <div className="settings-content">
          {section === "general" && (
            <>
              <fieldset>
                <legend>{t.settingsAppearance}</legend>
                {radios<ThemePreference>("theme", preferences.theme, [
                  { value: "system", label: t.themeSystem },
                  { value: "light", label: t.themeLight },
                  { value: "dark", label: t.themeDark },
                ], (theme) => onChange({ theme }))}
              </fieldset>
              <label className="dialog-field">
                <span>{t.settingsZoom}</span>
                <select
                  onChange={(event) => onChange({ zoom: Number(event.target.value) })}
                  value={preferences.zoom}
                >
                  {ZOOM_STEPS.map((step) => (
                    <option key={step} value={step}>
                      {Math.round(step * 100)}%
                    </option>
                  ))}
                </select>
              </label>
              <fieldset>
                <legend>{t.settingsStartView}</legend>
                {radios<StartView>("start", preferences.startView, [
                  { value: "home", label: t.homeNav },
                  { value: "notes", label: t.allNotes },
                ], (startView) => onChange({ startView }))}
              </fieldset>
            </>
          )}
          {section === "sidebar" && (
            <fieldset>
              <legend>{t.settingsSidebar}</legend>
              {toggle("sidebarHidden", t.hideSidebar)}
              {toggle("showNoteCounts", t.showNoteCounts)}
              {toggle("showRecentNotes", t.showRecentNotesInSidebar)}
              {toggle("showCreateButtons", t.showCreateButtons)}
              {toggle("groupInMore", t.groupInMore)}
              {toggle("showNotebooksInSpaces", t.showNotebooksInSpaces)}
            </fieldset>
          )}
          {section === "notes" && (
            <>
              <fieldset>
                <legend>{t.noteWidth}</legend>
                {radios<NoteWidth>("width", preferences.noteWidth, [
                  { value: "normal", label: t.noteWidthNormal },
                  { value: "wide", label: t.noteWidthWide },
                  { value: "full", label: t.noteWidthFull },
                ], (noteWidth) => onChange({ noteWidth }))}
              </fieldset>
              <fieldset>
                <legend>{t.slashCommands}</legend>
                {toggle("slashCommands", t.enableSlashCommands)}
                <p className="data-hint">{t.slashCommandsHint}</p>
              </fieldset>
              <button className="data-action" onClick={onShowShortcuts} type="button">
                {t.keyboardShortcuts}
              </button>
            </>
          )}
          {section === "ai" && (
            <AiSettingsSection
              notebooks={notebooks}
              onCancelIndexing={onCancelIndexing}
              onStartIndexing={onStartIndexing}
              onStatus={onAiStatus}
              progress={aiProgress}
              status={aiStatus}
            />
          )}
          {section === "integrations" && <IntegrationsSection />}
          {section === "sync" && (
            <SyncSection
              onOpenNote={(noteId) => {
                onClose();
                onOpenNote(noteId);
              }}
            />
          )}
          {section === "updates" && (
            <UpdatesSection onInstall={onInstallUpdate} onStatus={onUpdateStatus} status={updateStatus} />
          )}
          {section === "data" && (
            <>
              <p className="data-hint">{t.settingsDataHint}</p>
              <button
                className="data-action primary"
                onClick={() => {
                  onClose();
                  onOpenData();
                }}
                type="button"
              >
                {t.dataAndBackup}
              </button>
            </>
          )}
        </div>
      </div>
    </PlannerDialog>
  );
}
