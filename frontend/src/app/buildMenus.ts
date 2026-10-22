import { t } from "../i18n";
import type { BlockCommand } from "../editor/commands";
import type { Menu, MenuAction, MenuEntry } from "./menu";
import { separator } from "./menu";
import type { NoteLayout, NoteWidth, ThemePreference, UiPreferences } from "./preferences";

export type ExportKind = "markdown" | "html" | "text";

/** Stato e azioni dell'app usati per costruire i menu. */
export interface MenuContext {
  isDesktop: boolean;
  hasNote: boolean;
  noteEditable: boolean;
  noteTrashed: boolean;
  notePinned: boolean;
  noteArchived: boolean;
  canGoBack: boolean;
  canGoForward: boolean;
  tabs: { id: string; label: string; active: boolean }[];
  preferences: UiPreferences;
  layout: NoteLayout;
  /** Barra laterale effettivamente visibile (opzione e vista della nota). */
  sidebarVisible: boolean;
  blockCommands: BlockCommand[];
  editorAvailable: boolean;

  newNote: () => void;
  newNotebook: () => void;
  newSpace: () => void;
  newEvent: () => void;
  newTask: () => void;
  newTag: () => void;
  newNoteFromTemplate: () => void;
  saveNow: () => void;
  importData: () => void;
  exportNote: (kind: ExportKind) => void;
  printNote: () => void;
  openData: () => void;
  quit: () => void;

  undo: () => void;
  redo: () => void;
  cut: () => void;
  copy: () => void;
  paste: (plain: boolean) => void;
  selectAll: () => void;
  focusSearch: () => void;
  quickSwitch: () => void;

  reloadData: () => void;
  reloadWindow: () => void;
  goBack: () => void;
  goForward: () => void;
  zoom: (direction: 0 | 1 | -1) => void;
  toggleFullscreen: () => void;
  setPreference: (patch: Partial<UiPreferences>) => void;

  openNoteInWindow: () => void;
  openNoteInTab: () => void;
  setLayout: (layout: NoteLayout) => void;
  cycleLayout: (direction: 1 | -1) => void;
  copyNoteAs: (kind: "markdown" | "text") => void;
  moveNote: () => void;
  copyNoteTo: () => void;
  duplicateNote: () => void;
  editTags: () => void;
  toggleShortcut: () => void;
  toggleArchive: () => void;
  copyNoteLink: () => void;
  copyInternalLink: () => void;
  findReplace: () => void;
  runBlockCommand: (command: BlockCommand) => void;
  noteInfo: () => void;
  noteHistory: () => void;
  saveAsTemplate: () => void;
  trashNote: () => void;

  newTab: () => void;
  nextTab: (direction: 1 | -1) => void;
  selectTab: (id: string) => void;
  closeTab: () => void;
  minimize: () => void;

  openSettings: () => void;
  openAssistant: () => void;
  /** Voci AI per la nota aperta (o la sola voce di configurazione). */
  aiItems: MenuEntry[];
  checkIntegrity: () => void;
  showShortcuts: () => void;
  showAbout: () => void;
}

function item(
  id: string,
  label: string,
  run: () => void,
  extra: Partial<Omit<MenuAction, "id" | "label" | "run">> = {},
): MenuAction {
  return { id, label, run, ...extra };
}

/** Voci relative alla nota aperta, condivise con il menu contestuale. */
export function noteItems(context: MenuContext): MenuEntry[] {
  const note = context.hasNote;
  const editable = context.noteEditable;
  return [
    item("open-window", t.menuOpenInNewWindow, context.openNoteInWindow, {
      accelerator: "Alt+O",
      enabled: note && editable,
    }),
    item("open-tab", t.menuOpenInNewTab, context.openNoteInTab, {
      accelerator: "Alt+Shift+O",
      enabled: note,
    }),
    separator,
    {
      kind: "submenu",
      id: "share",
      label: t.menuCopyAs,
      enabled: note,
      items: [
        item("copy-markdown", t.menuCopyAsMarkdown, () => context.copyNoteAs("markdown"), {
          accelerator: "Alt+S",
        }),
        item("copy-text", t.menuCopyAsText, () => context.copyNoteAs("text")),
      ],
    },
    separator,
    item("move-note", t.menuMoveNote, context.moveNote, {
      accelerator: "Alt+Shift+M",
      enabled: note && editable,
    }),
    item("copy-note", t.menuCopyNoteTo, context.copyNoteTo, { enabled: note && editable }),
    item("duplicate-note", t.menuDuplicateNote, context.duplicateNote, { enabled: note && editable }),
    separator,
    item("edit-tags", t.menuEditTags, context.editTags, {
      accelerator: "Alt+Ctrl+T",
      enabled: note && editable,
    }),
    separator,
    item(
      "toggle-shortcut",
      context.notePinned ? t.unpinNote : t.pinNote,
      context.toggleShortcut,
      { enabled: note && editable },
    ),
    item(
      "toggle-archive",
      context.noteArchived ? t.unarchiveNote : t.archiveNote,
      context.toggleArchive,
      { enabled: note && editable },
    ),
    item("save-template", t.menuSaveAsTemplate, context.saveAsTemplate, { enabled: note && editable }),
    item("copy-link", t.menuCopyNoteLink, context.copyNoteLink, {
      accelerator: "Alt+Ctrl+L",
      enabled: note,
    }),
    item("copy-internal-link", t.menuCopyInternalLink, context.copyInternalLink, {
      accelerator: "Alt+Ctrl+Shift+C",
      enabled: note,
    }),
    separator,
    item("note-info", t.menuNoteInfo, context.noteInfo, {
      accelerator: "Ctrl+Shift+I",
      enabled: note && editable,
    }),
    item("note-history", t.menuNoteHistory, context.noteHistory, { enabled: note && editable }),
    separator,
    item("trash-note", t.moveToTrash, context.trashNote, {
      accelerator: "Del",
      native: true,
      enabled: note && editable,
      destructive: true,
    }),
  ];
}

export function buildMenus(context: MenuContext): Menu[] {
  const note = context.hasNote;
  const editable = context.noteEditable;
  const preferences = context.preferences;
  const toggle = (key: keyof UiPreferences, label: string, accelerator?: string) =>
    item(`pref-${key}`, label, () => context.setPreference({ [key]: !preferences[key] }), {
      checked: Boolean(preferences[key]),
      accelerator,
    });
  const theme = (value: ThemePreference, label: string) =>
    item(`theme-${value}`, label, () => context.setPreference({ theme: value }), {
      checked: preferences.theme === value,
      radio: true,
    });
  const width = (value: NoteWidth, label: string) =>
    item(`width-${value}`, label, () => context.setPreference({ noteWidth: value }), {
      checked: preferences.noteWidth === value,
      radio: true,
    });
  const layout = (value: NoteLayout, label: string) =>
    item(`layout-${value}`, label, () => context.setLayout(value), {
      checked: context.layout === value,
      radio: true,
    });

  return [
    {
      id: "file",
      label: t.menuFile,
      items: [
        item("new-note", t.newNote, context.newNote, { accelerator: "Ctrl+N" }),
        item("new-notebook", t.menuNewNotebook, context.newNotebook, {
          accelerator: "Alt+Shift+N",
          aliases: ["Ctrl+Shift+N"],
        }),
        item("new-space", t.menuNewSpace, context.newSpace, { accelerator: "Alt+Shift+S" }),
        item("new-event", t.menuNewEvent, context.newEvent, { accelerator: "Ctrl+E" }),
        item("new-task", t.menuNewTask, context.newTask, { accelerator: "Alt+T" }),
        item("new-tag", t.menuNewTag, context.newTag, { accelerator: "Ctrl+Shift+T" }),
        item("new-from-template", t.menuNewFromTemplate, context.newNoteFromTemplate),
        separator,
        item("save-now", t.menuSaveNow, context.saveNow, { accelerator: "Ctrl+S", enabled: note && editable }),
        separator,
        item("import", t.menuImport, context.importData),
        {
          kind: "submenu",
          id: "export-note",
          label: t.menuExportNote,
          enabled: note && editable,
          items: [
            item("export-md", t.exportAsMarkdown, () => context.exportNote("markdown")),
            item("export-html", t.exportAsHtml, () => context.exportNote("html")),
            item("export-txt", t.exportAsText, () => context.exportNote("text")),
          ],
        },
        item("export-pdf", t.menuExportPdf, context.printNote, { enabled: note }),
        item("print", t.menuPrint, context.printNote, { accelerator: "Ctrl+P", enabled: note }),
        separator,
        item("data", t.dataAndBackup, context.openData),
        separator,
        item("quit", t.menuQuit, context.quit, { accelerator: "Ctrl+Shift+Q" }),
      ],
    },
    {
      id: "edit",
      label: t.menuEdit,
      items: [
        item("undo", t.undo, context.undo, { accelerator: "Ctrl+Z", native: true }),
        item("redo", t.redo, context.redo, { accelerator: "Ctrl+Y", native: true }),
        separator,
        item("cut", t.menuCut, context.cut, { accelerator: "Ctrl+X", native: true }),
        item("copy", t.menuCopy, context.copy, { accelerator: "Ctrl+C", native: true }),
        item("paste", t.menuPaste, () => context.paste(false), { accelerator: "Ctrl+V", native: true }),
        item("paste-plain", t.menuPastePlain, () => context.paste(true), {
          accelerator: "Ctrl+Shift+V",
        }),
        separator,
        item("select-all", t.menuSelectAll, context.selectAll, { accelerator: "Ctrl+A", native: true }),
        separator,
        item("search", t.menuSearchNotes, context.focusSearch, {
          accelerator: "Ctrl+K",
          aliases: ["Ctrl+F"],
        }),
        item("switch", t.menuSwitchTo, context.quickSwitch, { accelerator: "Ctrl+Q" }),
        item("find-replace", t.findReplace, context.findReplace, {
          accelerator: "Ctrl+H",
          enabled: note && context.editorAvailable,
        }),
        separator,
        item(
          "encrypt-selection",
          t.encryptSelection,
          () => {
            const command = context.blockCommands.find((entry) => entry.id === "encrypted-text");
            if (command) context.runBlockCommand(command);
          },
          { accelerator: "Ctrl+Shift+X", enabled: note && editable && context.editorAvailable },
        ),
      ],
    },
    {
      id: "view",
      label: t.menuView,
      items: [
        item("reload", t.menuReload, context.reloadData, { accelerator: "Ctrl+R" }),
        item("force-reload", t.menuForceReload, context.reloadWindow, { accelerator: "Ctrl+Shift+R" }),
        separator,
        item("back", t.menuBack, context.goBack, { accelerator: "Alt+Left", enabled: context.canGoBack }),
        item("forward", t.menuForward, context.goForward, {
          accelerator: "Alt+Right",
          enabled: context.canGoForward,
        }),
        separator,
        item("zoom-reset", t.menuActualSize, () => context.zoom(0), { accelerator: "Ctrl+0" }),
        item("zoom-in", t.menuZoomIn, () => context.zoom(1), { accelerator: "Ctrl++" }),
        item("zoom-out", t.menuZoomOut, () => context.zoom(-1), { accelerator: "Ctrl+-" }),
        item("fullscreen", t.menuFullscreen, context.toggleFullscreen, { accelerator: "F11" }),
        separator,
        item(
          "pref-sidebarHidden",
          t.hideSidebar,
          () => context.setPreference({ sidebarHidden: context.sidebarVisible }),
          { checked: !context.sidebarVisible, accelerator: "F10" },
        ),
        toggle("showNoteCounts", t.showNoteCounts),
        toggle("showRecentNotes", t.showRecentNotesInSidebar),
        toggle("showCreateButtons", t.showCreateButtons),
        toggle("groupInMore", t.groupInMore),
        toggle("showNotebooksInSpaces", t.showNotebooksInSpaces),
        separator,
        {
          kind: "submenu",
          id: "theme",
          label: t.menuTheme,
          items: [theme("system", t.themeSystem), theme("light", t.themeLight), theme("dark", t.themeDark)],
        },
      ],
    },
    {
      id: "note",
      label: t.menuNote,
      items: [
        ...noteItems(context).slice(0, 2),
        {
          kind: "submenu",
          id: "layout",
          label: t.menuNoteLayout,
          items: [
            layout("normal", t.layoutNormal),
            layout("expanded", t.layoutExpanded),
            layout("focus", t.layoutFocus),
          ],
        },
        item("layout-back", t.menuLayoutBack, () => context.cycleLayout(-1), {
          accelerator: "Alt+Ctrl+Left",
        }),
        item("layout-forward", t.menuLayoutForward, () => context.cycleLayout(1), {
          accelerator: "Alt+Ctrl+Right",
        }),
        ...noteItems(context).slice(2, -4),
        item("find-replace-note", t.findReplace, context.findReplace, {
          accelerator: "Ctrl+H",
          enabled: note && context.editorAvailable,
        }),
        {
          kind: "submenu",
          id: "width",
          label: t.noteWidth,
          items: [
            width("normal", t.noteWidthNormal),
            width("wide", t.noteWidthWide),
            width("full", t.noteWidthFull),
          ],
        },
        {
          kind: "submenu",
          id: "slash",
          label: t.slashCommands,
          items: [
            item("slash-toggle", t.enableSlashCommands, () =>
              context.setPreference({ slashCommands: !preferences.slashCommands }), {
              checked: preferences.slashCommands,
            }),
            separator,
            ...context.blockCommands.map((command) =>
              item(`block-${command.id}`, command.label, () => context.runBlockCommand(command), {
                enabled: note && editable && context.editorAvailable,
              }),
            ),
          ],
        },
        ...noteItems(context).slice(-4),
      ],
    },
    {
      id: "window",
      label: t.menuWindow,
      items: [
        item("new-tab", t.menuNewTab, context.newTab, { accelerator: "Alt+Shift+T" }),
        item("next-tab", t.menuNextTab, () => context.nextTab(1), {
          accelerator: "Ctrl+Tab",
          enabled: context.tabs.length > 1,
        }),
        item("previous-tab", t.menuPreviousTab, () => context.nextTab(-1), {
          accelerator: "Ctrl+Shift+Tab",
          enabled: context.tabs.length > 1,
        }),
        {
          kind: "submenu",
          id: "select-tab",
          label: t.menuSelectTab,
          items: context.tabs.map((tab, index) =>
            item(`tab-${tab.id}`, tab.label, () => context.selectTab(tab.id), {
              checked: tab.active,
              radio: true,
              accelerator: index < 9 ? `Alt+${index + 1}` : undefined,
            }),
          ),
        },
        item("close-tab", t.menuCloseTab, context.closeTab, {
          accelerator: "Ctrl+W",
          enabled: context.tabs.length > 1,
        }),
        item("minimize", t.menuMinimize, context.minimize, { accelerator: "Ctrl+M" }),
      ],
    },
    {
      id: "tools",
      label: t.menuTools,
      items: [
        item("settings", t.settingsTitle, context.openSettings, { accelerator: "Ctrl+," }),
        separator,
        item("assistant", t.assistantNav, context.openAssistant, { accelerator: "Ctrl+Shift+A" }),
        { kind: "submenu", id: "ai-note", label: t.aiNoteMenu, items: context.aiItems },
        separator,
        item("data-tools", t.dataAndBackup, context.openData),
        item("integrity", t.runIntegrityCheck, context.checkIntegrity),
      ],
    },
    {
      id: "help",
      label: t.menuHelp,
      items: [
        item("shortcuts", t.keyboardShortcuts, context.showShortcuts, { accelerator: "Ctrl+/" }),
        item("about", t.aboutTitle, context.showAbout),
      ],
    },
  ];
}
