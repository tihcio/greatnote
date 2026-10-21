import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { DragEvent, FormEvent, MouseEvent as ReactMouseEvent, ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { EditorContent, useEditor } from "@tiptap/react";
import type { JSONContent } from "@tiptap/react";
import type {
  CalendarEvent,
  Notebook,
  Attachment,
  ExportFormat,
  Note,
  NoteDraft,
  NoteLink,
  NoteView,
  SaveState,
  SearchResult,
  Reminder,
  Space,
  Tag,
  SidebarCounts,
  Task,
  TaskCounts,
  Template,
  TrashItem,
  TrashKind,
  EmptyTrashSummary,
  Workspace,
} from "./types";
import { language, t } from "./i18n";
import logoUrl from "./assets/logo.png";
import { errorMessage, formatDate, formatFileSize, formatShortDate } from "./lib/format";
import CalendarView from "./features/CalendarView";
import DataDialog from "./features/DataDialog";
import ConfirmHost from "./features/ConfirmHost";
import { askConfirmation } from "./lib/confirm";
import NotePlanner from "./features/NotePlanner";
import ReminderAlerts from "./features/ReminderAlerts";
import RemindersView from "./features/RemindersView";
import SyncStatusIndicator from "./features/SyncStatusIndicator";
import TasksView from "./features/TasksView";
import { buildMenus, noteItems } from "./app/buildMenus";
import type { ExportKind, MenuContext } from "./app/buildMenus";
import { findAction } from "./app/menu";
import type { MenuEntry } from "./app/menu";
import { LIST_WIDTH, MIN_EDITOR_WIDTH, SIDEBAR_WIDTH, nextZoom, usePreferences } from "./app/preferences";
import type { NoteLayout, SidebarSection } from "./app/preferences";
import { blockCommands } from "./editor/commands";
import type { BlockCommand } from "./editor/commands";
import EditorToolbar from "./editor/EditorToolbar";
import { editorExtensions } from "./editor/extensions";
import FindReplaceBar from "./editor/FindReplaceBar";
import { sanitizeDocument } from "./editor/sanitize";
import SlashMenu from "./editor/SlashMenu";
import ContextMenu from "./features/ContextMenu";
import type { ContextMenuState } from "./features/ContextMenu";
import FilesView from "./features/FilesView";
import { AboutDialog, ShortcutsDialog } from "./features/HelpDialogs";
import HomeView from "./features/HomeView";
import MenuBar from "./features/MenuBar";
import NotebookPickerDialog from "./features/NotebookPickerDialog";
import NoteHistoryDialog from "./features/NoteHistoryDialog";
import NoteInfoDialog from "./features/NoteInfoDialog";
import PromptHost from "./features/PromptHost";
import QuickSwitcher from "./features/QuickSwitcher";
import type { SwitchTarget } from "./features/QuickSwitcher";
import { formatTime } from "./lib/dates";
import SettingsDialog from "./features/SettingsDialog";
import { SYNC_FINISHED_EVENT } from "./features/SyncSection";
import type { SyncView } from "./features/SyncSection";
import TagBar from "./features/TagBar";
import ImageAnnotationHost from "./features/ImageAnnotator";
import Icon from "./components/Icon";
import ColumnResizer from "./components/ColumnResizer";
import type { SettingsSection } from "./features/SettingsDialog";
import AiResultDialog from "./features/ai/AiResultDialog";
import type { AiTransformRequest } from "./features/ai/AiResultDialog";
import AiSuggestionsDialog from "./features/ai/AiSuggestionsDialog";
import AssistantView from "./features/ai/AssistantView";
import type { AssistantContextNote } from "./features/ai/AssistantView";
import type { SourceTarget } from "./features/ai/aiApi";
import { aiNoteItems } from "./features/ai/aiActions";
import { INDEX_FINISHED_EVENT, INDEX_PROGRESS_EVENT, aiApi, aiReady } from "./features/ai/aiApi";
import type { AiStatus, IndexFinished, IndexProgress, TransformAction } from "./features/ai/aiApi";
import TagEditorDialog from "./features/TagEditorDialog";
import TemplatesView from "./features/TemplatesView";
import { askText } from "./lib/prompt";
import TrashItemsPanel from "./features/TrashItemsPanel";

const PAGE_SIZE = 100;
const EMPTY_DOCUMENT: JSONContent = {
  type: "doc",
  content: [{ type: "paragraph" }],
};
const isDesktop = "__TAURI_INTERNALS__" in window;

type OrganizationDialog =
  | {
      kind: "space";
      intent: "create" | "edit";
      id?: string;
      name: string;
      description: string;
    }
  | {
      kind: "notebook";
      intent: "create" | "edit";
      id?: string;
      name: string;
      description: string;
      spaceId: string | null;
    }
  | {
      kind: "tag";
      intent: "create" | "edit";
      id?: string;
      name: string;
    };

function renderSearchSnippet(snippet: string): ReactNode {
  const parts = snippet.split(/([\uE000][^\uE001]*[\uE001])/g);
  return parts.map((part, index) =>
    part.startsWith("\uE000") && part.endsWith("\uE001") ? (
      <mark className="search-hit" key={index}>
        {part.slice(1, -1)}
      </mark>
    ) : (
      part
    ),
  );
}

function plainText(content: JSONContent): string {
  if (content.text) {
    return content.text;
  }
  return content.content?.map(plainText).join(" ") ?? "";
}

interface Location {
  workspace: Workspace;
  view: NoteView;
  noteId: string | null;
  label: string;
}

interface TabState {
  id: string;
  history: Location[];
  index: number;
}

const MAX_HISTORY = 50;
const newId = () => Math.random().toString(36).slice(2, 10);
const locationKey = (location: Pick<Location, "workspace" | "view" | "noteId">) =>
  `${location.workspace}|${location.view.kind}|${"id" in location.view ? location.view.id : ""}|${location.noteId ?? ""}`;
const LAYOUTS: NoteLayout[] = ["normal", "expanded", "focus"];

const SIDEBAR_RECENT_LIMIT = 5;

/**
 * Note recenti della barra laterale con posizioni stabili: aprire una nota
 * già in elenco non la sposta (cambiarla sotto il puntatore è fastidioso);
 * le note nuove entrano in cima al posto delle meno recenti. Titoli e
 * contenuti seguono i dati aggiornati; le note non più recenti (es. nel
 * cestino) escono dall'elenco.
 */
function mergeRecent(current: Note[], fresh: Note[]): Note[] {
  const freshById = new Map(fresh.map((note) => [note.id, note]));
  const currentIds = new Set(current.map((note) => note.id));
  const kept = current.flatMap((note) => {
    const updated = freshById.get(note.id);
    return updated ? [updated] : [];
  });
  const added = fresh.filter((note) => !currentIds.has(note.id));
  return [...added, ...kept].slice(0, SIDEBAR_RECENT_LIMIT);
}

export default function App() {
  const [notes, setNotes] = useState<Note[]>([]);
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [noteLinks, setNoteLinks] = useState<NoteLink[]>([]);
  const [linkCandidates, setLinkCandidates] = useState<Note[]>([]);
  const [selectedLinkTarget, setSelectedLinkTarget] = useState("");
  const [isUploadingAttachment, setIsUploadingAttachment] = useState(false);
  const [searchTitles, setSearchTitles] = useState<Record<string, string>>({});
  const [searchSnippets, setSearchSnippets] = useState<Record<string, string>>({});
  const [noteListVersion, setNoteListVersion] = useState(0);
  const [spaces, setSpaces] = useState<Space[]>([]);
  const [notebooks, setNotebooks] = useState<Notebook[]>([]);
  const [tags, setTags] = useState<Tag[]>([]);
  const [drafts, setDrafts] = useState<NoteDraft[]>([]);
  const [noteTags, setNoteTags] = useState<Tag[]>([]);
  const [selectedView, setSelectedView] = useState<NoteView>({ kind: "all" });
  const isTrashView = selectedView.kind === "trash";
  const isArchiveView = selectedView.kind === "archived";
  const [activeNote, setActiveNote] = useState<Note | null>(null);
  const [title, setTitle] = useState("");
  const [search, setSearch] = useState("");
  const [saveState, setSaveState] = useState<SaveState>("saved");
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [isDataDialogOpen, setIsDataDialogOpen] = useState(false);
  const [dataDialogTab, setDataDialogTab] = useState<"import" | "export" | "backup" | "integrity">("import");
  const { preferences, update: updatePreferences, loaded: preferencesLoaded } = usePreferences(isDesktop);
  const [layout, setLayout] = useState<NoteLayout>("normal");
  const [contextMenu, setContextMenu] = useState<ContextMenuState | null>(null);
  const [isSwitcherOpen, setIsSwitcherOpen] = useState(false);
  const [isSettingsOpen, setIsSettingsOpen] = useState(false);
  const [settingsSection, setSettingsSection] = useState<SettingsSection>("general");
  const [aiStatus, setAiStatus] = useState<AiStatus | null>(null);
  const [aiProgress, setAiProgress] = useState<IndexProgress | null>(null);
  const [aiTransform, setAiTransform] = useState<AiTransformRequest | null>(null);
  const [aiSuggestions, setAiSuggestions] = useState<"tags" | "tasks" | null>(null);
  const [assistantNote, setAssistantNote] = useState<AssistantContextNote | null>(null);
  /** Intervallo dell'editor da sostituire con il risultato di una modifica AI. */
  const aiRangeRef = useRef<{ from: number; to: number }>({ from: 0, to: 0 });
  const [isShortcutsOpen, setIsShortcutsOpen] = useState(false);
  const [isAboutOpen, setIsAboutOpen] = useState(false);
  const [isNoteInfoOpen, setIsNoteInfoOpen] = useState(false);
  const [isHistoryOpen, setIsHistoryOpen] = useState(false);
  const [isTagEditorOpen, setIsTagEditorOpen] = useState(false);
  const [isFindOpen, setIsFindOpen] = useState(false);
  const [notebookPicker, setNotebookPicker] = useState<"move" | "copy" | null>(null);
  const [taskCreateRequest, setTaskCreateRequest] = useState(0);
  const [eventCreateRequest, setEventCreateRequest] = useState(0);
  const [sidebarCounts, setSidebarCounts] = useState<SidebarCounts | null>(null);
  const [sidebarRecent, setSidebarRecent] = useState<Note[]>([]);
  const [isMoreOpen, setIsMoreOpen] = useState(false);
  const [tabs, setTabs] = useState<TabState[]>(() => [
    {
      id: newId(),
      history: [{ workspace: "home", view: { kind: "all" }, noteId: null, label: "" }],
      index: 0,
    },
  ]);
  const [activeTabId, setActiveTabId] = useState(() => tabs[0].id);
  const navigatingRef = useRef(false);
  const startViewAppliedRef = useRef(false);
  const [workspace, setWorkspace] = useState<Workspace>(isDesktop ? "home" : "notes");
  const [reminders, setReminders] = useState<Reminder[]>([]);
  const [taskCounts, setTaskCounts] = useState<TaskCounts | null>(null);
  const [plannerVersion, setPlannerVersion] = useState(0);
  const [pendingTask, setPendingTask] = useState<Task | null>(null);
  const [pendingEvent, setPendingEvent] = useState<CalendarEvent | null>(null);
  const [trashItems, setTrashItems] = useState<TrashItem[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [hasMore, setHasMore] = useState(false);
  const [isCreating, setIsCreating] = useState(false);
  const [organizationDialog, setOrganizationDialog] = useState<OrganizationDialog | null>(null);
  const [isSavingOrganization, setIsSavingOrganization] = useState(false);
  const [organizationDialogError, setOrganizationDialogError] = useState<string | null>(null);
  const organizationNameInput = useRef<HTMLInputElement>(null);
  const titleInput = useRef<HTMLInputElement>(null);
  const searchInput = useRef<HTMLInputElement>(null);
  const attachmentInput = useRef<HTMLInputElement>(null);
  const createNotebookShortcut = useRef<() => void>(() => {});
  const activeNoteRef = useRef<Note | null>(null);
  const activeNoteTrashedRef = useRef(false);
  const saveChainRef = useRef<Promise<boolean>>(Promise.resolve(true));
  const allowWindowCloseRef = useRef(false);
  const draftRef = useRef<{ title: string; content: JSONContent }>({
    title: "",
    content: EMPTY_DOCUMENT,
  });
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  /** Vero se la nota aperta ha modifiche non ancora salvate. Cambiare nota
   * senza modifiche non deve salvarla: aggiornerebbe la data di modifica e
   * sposterebbe la nota nella lista. */
  const dirtyRef = useRef(false);

  const pasteImagesRef = useRef<(files: File[]) => Promise<void>>(async () => undefined);
  const editor = useEditor({
    extensions: editorExtensions,
    content: EMPTY_DOCUMENT,
    editorProps: {
      attributes: {
        class: "note-editor",
        "aria-label": t.editorContent,
        "data-placeholder": t.editorPlaceholder,
      },
      // Immagini incollate: diventano allegati della nota e compaiono nel testo.
      handlePaste: (_view, event) => {
        const images = Array.from(event.clipboardData?.files ?? []).filter((file) =>
          file.type.startsWith("image/"),
        );
        if (images.length === 0) return false;
        void pasteImagesRef.current(images);
        return true;
      },
    },
    onUpdate: ({ editor: currentEditor }) => {
      draftRef.current = { ...draftRef.current, content: currentEditor.getJSON() };
      dirtyRef.current = true;
      setSaveState("saving");
      scheduleSave();
    },
  });

  const flushDraft = useCallback(async (note: Note | null): Promise<boolean> => {
    if (!note || activeNoteTrashedRef.current) {
      return true;
    }
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }

    const operation = saveChainRef.current.then(async () => {
      if (!dirtyRef.current) {
        return true;
      }
      const draft = {
        ...draftRef.current,
        content: sanitizeDocument(draftRef.current.content),
      };
      // Una modifica durante il salvataggio riporta il flag a vero.
      dirtyRef.current = false;
      setSaveState("saving");
      try {
        await invoke<NoteDraft>("save_note_draft", {
          noteId: note.id,
          title: draft.title,
          content: draft.content,
        });
        // Versione su cui l'editor lavorava: se nel frattempo un altro
        // dispositivo ha cambiato il testo, il backend lo conserva.
        const base = activeNoteRef.current?.id === note.id ? activeNoteRef.current : note;
        const saved = await invoke<Note>("update_note", {
          id: note.id,
          title: draft.title,
          content: draft.content,
          baseUpdatedAt: base.updated_at,
        });
        setDrafts((current) => current.filter((item) => item.note_id !== note.id));
        // Aggiornata sul posto: la lista non si riordina mentre la si usa.
        // Il nuovo ordine compare al prossimo caricamento della vista.
        setNotes((current) => current.map((item) => (item.id === saved.id ? saved : item)));
        if (activeNoteRef.current?.id === note.id) {
          activeNoteRef.current = saved;
          setActiveNote(saved);
          setTitle(draft.title);
        }
        setError(null);
        setSaveState("saved");
        return true;
      } catch (cause) {
        dirtyRef.current = true;
        setError(errorMessage(cause));
        setSaveState("error");
        return false;
      }
    });
    saveChainRef.current = operation;
    return operation;
  }, []);

  const scheduleSave = useCallback(() => {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
    }
    const note = activeNoteRef.current;
    timerRef.current = setTimeout(() => {
      timerRef.current = null;
      void flushDraft(note);
    }, 650);
  }, [flushDraft]);

  useEffect(() => {
    if (!isDesktop) {
      setIsLoading(false);
      return;
    }
    let cancelled = false;
    void Promise.all([
      invoke<Space[]>("list_spaces"),
      invoke<Notebook[]>("list_notebooks"),
      invoke<Tag[]>("list_tags"),
      invoke<NoteDraft[]>("list_note_drafts"),
    ])
      .then(([loadedSpaces, loadedNotebooks, loadedTags, loadedDrafts]) => {
        if (!cancelled) {
          setSpaces(loadedSpaces);
          setNotebooks(loadedNotebooks);
          setTags(loadedTags);
          setDrafts(loadedDrafts);
        }
      })
      .catch((cause: unknown) => {
        if (!cancelled) {
          setError(errorMessage(cause));
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!isDesktop) {
      setIsLoading(false);
      return;
    }
    let cancelled = false;
    const debounce = window.setTimeout(() => {
      const scopeId = "id" in selectedView ? selectedView.id : null;
      void invoke<SearchResult[]>("search_notes", {
        query: search,
        scope: selectedView.kind,
        scopeId,
        limit: PAGE_SIZE,
        offset: 0,
      })
        .then((results) => {
          if (cancelled) return;
          setNotes(results.map((result) => result.note));
          setSearchTitles(
            Object.fromEntries(
              results.map((result) => [result.note.id, result.highlighted_title]),
            ),
          );
          setSearchSnippets(
            Object.fromEntries(results.map((result) => [result.note.id, result.snippet])),
          );
          setHasMore(results.length === PAGE_SIZE);
          setError(null);
        })
        .catch((cause: unknown) => {
          if (!cancelled) {
            setNotes([]);
            setSearchTitles({});
            setSearchSnippets({});
            setHasMore(false);
            setError(errorMessage(cause));
          }
        })
        .finally(() => {
          if (!cancelled) setIsLoading(false);
        });
    }, search.trim() ? 180 : 0);
    setIsLoading(true);
    return () => {
      cancelled = true;
      window.clearTimeout(debounce);
    };
  }, [selectedView, search, noteListVersion]);

  useEffect(() => {
    if (!editor || editor.isDestroyed || !activeNote) {
      return;
    }
    editor.commands.setContent(activeNote.content, { emitUpdate: false });
  }, [activeNote?.id, editor]);

  useEffect(() => {
    setSelectedLinkTarget("");
    if (!isDesktop || !activeNote || isTrashView) {
      setAttachments([]);
      setNoteLinks([]);
      return;
    }
    let cancelled = false;
    void Promise.all([
      invoke<Attachment[]>("list_note_attachments", { noteId: activeNote.id }),
      invoke<NoteLink[]>("list_note_links", { sourceNoteId: activeNote.id }),
    ])
      .then(([loadedAttachments, loadedLinks]) => {
        if (!cancelled) {
          setAttachments(loadedAttachments);
          setNoteLinks(loadedLinks);
        }
      })
      .catch((cause: unknown) => {
        if (!cancelled) setError(errorMessage(cause));
      });
    return () => {
      cancelled = true;
    };
  }, [activeNote?.id, isTrashView]);

  useEffect(() => {
    editor?.setEditable(!isTrashView);
  }, [editor, isTrashView]);

  useEffect(
    () => () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
      }
    },
    [],
  );

  useEffect(() => {
    if (!isDesktop) {
      return;
    }

    const appWindow = getCurrentWindow();
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void appWindow
      .onCloseRequested(async (event) => {
        if (allowWindowCloseRef.current) {
          return;
        }
        event.preventDefault();
        if (await flushDraft(activeNoteRef.current)) {
          allowWindowCloseRef.current = true;
          try {
            // In Tauri 2 la chiusura effettiva è `destroy()`: chiamarla subito
            // evita una seconda richiesta di chiusura.
            await appWindow.destroy();
          } catch (cause) {
            allowWindowCloseRef.current = false;
            setError(errorMessage(cause));
          }
        }
      })
      .then((stopListening) => {
        if (cancelled) {
          stopListening();
        } else {
          unlisten = stopListening;
        }
      })
      .catch((cause: unknown) => setError(errorMessage(cause)));

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [flushDraft]);

  const refreshAiStatus = useCallback(async () => {
    if (!isDesktop) return;
    try {
      setAiStatus(await aiApi.status());
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }, []);

  useEffect(() => {
    if (!isDesktop) return;
    void refreshAiStatus();
    const stops: (() => void)[] = [];
    let cancelled = false;
    const keep = (registration: Promise<() => void>) =>
      void registration.then((stop) => (cancelled ? stop() : stops.push(stop)));
    keep(listen<IndexProgress>(INDEX_PROGRESS_EVENT, (event) => setAiProgress(event.payload)));
    keep(
      listen<IndexFinished>(INDEX_FINISHED_EVENT, (event) => {
        setAiProgress(null);
        void refreshAiStatus();
        const { error: failure, report } = event.payload;
        if (failure) setError(t.aiIndexFailed(failure));
        else if (report && report.failed > 0) setNotice(t.aiIndexPartial(report.failed, report.last_error ?? ""));
      }),
    );
    return () => {
      cancelled = true;
      stops.forEach((stop) => stop());
    };
  }, [refreshAiStatus]);

  const refreshPlanner = useCallback(async () => {
    if (!isDesktop) return;
    try {
      const [loadedReminders, loadedCounts] = await Promise.all([
        invoke<Reminder[]>("list_reminders"),
        invoke<TaskCounts>("task_counts"),
      ]);
      setReminders(loadedReminders);
      setTaskCounts(loadedCounts);
      setPlannerVersion((version) => version + 1);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }, []);

  useEffect(() => {
    if (!isDesktop) return;
    void invoke("set_ui_language", { value: language }).catch(() => undefined);
    void refreshPlanner();
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void listen<Reminder[]>("reminders-fired", () => void refreshPlanner()).then((stop) => {
      if (cancelled) stop();
      else unlisten = stop;
    });
    // Note modificate in una finestra separata: aggiorna elenco e anteprime.
    let unlistenNotes: (() => void) | undefined;
    void listen("note-changed", () => setNoteListVersion((version) => version + 1)).then((stop) => {
      if (cancelled) stop();
      else unlistenNotes = stop;
    });
    // Aggiorna i contatori di "oggi" anche quando cambia il giorno.
    const interval = window.setInterval(() => void refreshPlanner(), 5 * 60 * 1000);
    return () => {
      cancelled = true;
      unlisten?.();
      unlistenNotes?.();
      window.clearInterval(interval);
    };
  }, [refreshPlanner]);

  const loadTrashItems = useCallback(async () => {
    if (!isDesktop) return;
    try {
      setTrashItems(await invoke<TrashItem[]>("list_trash_items"));
    } catch (cause) {
      setError(errorMessage(cause));
    }
  }, []);

  useEffect(() => {
    if (workspace === "notes" && selectedView.kind === "trash") void loadTrashItems();
  }, [loadTrashItems, selectedView.kind, workspace, noteListVersion]);

  const selectNote = useCallback(
    async (note: Note, forceUntrashed = false) => {
      if (activeNoteRef.current?.id === note.id) {
        return;
      }
      const saved = await flushDraft(activeNoteRef.current);
      if (!saved) {
        return;
      }
      let selectedNote = note;
      let recoveredDraft = drafts.find((draft) => draft.note_id === note.id);
      if (!recoveredDraft) {
        try {
          const availableDrafts = await invoke<NoteDraft[]>("list_note_drafts");
          setDrafts(availableDrafts);
          recoveredDraft = availableDrafts.find((draft) => draft.note_id === note.id);
        } catch (cause) {
          setError(errorMessage(cause));
          return;
        }
      }
      if (recoveredDraft) {
        // Chiudere il dialog senza scegliere conserva le modifiche.
        if (
          await askConfirmation({
            title: t.titleRecoverDraft,
            message: t.recoverDraftPrompt(note.title),
            confirmLabel: t.actionRecoverDraft,
            cancelLabel: t.actionDiscardDraft,
            dismissValue: true,
          })
        ) {
          selectedNote = {
            ...note,
            title: recoveredDraft.title,
            content: recoveredDraft.content,
          };
        } else {
          try {
            await invoke("discard_note_draft", { noteId: note.id });
            setDrafts((current) => current.filter((draft) => draft.note_id !== note.id));
          } catch (cause) {
            setError(errorMessage(cause));
            return;
          }
        }
      }
      activeNoteRef.current = selectedNote;
      activeNoteTrashedRef.current = !forceUntrashed && selectedView.kind === "trash";
      draftRef.current = { title: selectedNote.title, content: selectedNote.content };
      // Una bozza recuperata va salvata; altrimenti la nota è invariata.
      dirtyRef.current = selectedNote !== note;
      setTitle(selectedNote.title);
      setActiveNote(selectedNote);
      if (
        !activeNoteTrashedRef.current &&
        !selectedNote.archived_at
      ) {
        // La vista Recenti si aggiorna quando viene riaperta, non sotto il
        // cursore mentre si sceglie una nota.
        void invoke<void>("record_note_visit", { noteId: note.id }).catch((cause: unknown) =>
          setError(errorMessage(cause)),
        );
      }
      if (activeNoteTrashedRef.current) {
        setNoteTags([]);
      } else {
        void invoke<Tag[]>("list_note_tags", { noteId: note.id })
          .then(setNoteTags)
          .catch((cause: unknown) => setError(errorMessage(cause)));
      }
      setError(null);
      setSaveState(recoveredDraft && selectedNote !== note ? "saving" : "saved");
      if (recoveredDraft && selectedNote !== note) {
        void flushDraft(selectedNote);
      }
    },
    [drafts, flushDraft, selectedView],
  );

  const createNote = useCallback(async () => {
    if (isCreating) {
      return;
    }
    const saved = await flushDraft(activeNoteRef.current);
    if (!saved) {
      return;
    }
    setIsCreating(true);
    try {
      const notebookId = selectedView.kind === "notebook" ? selectedView.id : null;
      const note = await invoke<Note>("create_note", {
        title: t.untitled,
        content: EMPTY_DOCUMENT,
        notebookId,
      });
      void invoke<void>("record_note_visit", { noteId: note.id })
        .then(() => setNoteListVersion((version) => version + 1))
        .catch((cause: unknown) => setError(errorMessage(cause)));
      setNotes((current) => [note, ...current.filter((item) => item.id !== note.id)]);
      activeNoteRef.current = note;
      activeNoteTrashedRef.current = false;
      draftRef.current = { title: note.title, content: note.content };
      dirtyRef.current = false;
      setActiveNote(note);
      setNoteTags([]);
      setTitle(note.title);
      setSearch("");
      setWorkspace("notes");
      if (selectedView.kind !== "notebook") {
        setSelectedView({ kind: "all" });
      }
      setError(null);
      setSaveState("saved");
      window.requestAnimationFrame(() => titleInput.current?.focus());
    } catch (cause) {
      setError(errorMessage(cause));
    } finally {
      setIsCreating(false);
    }
  }, [flushDraft, isCreating, selectedView]);

  const updateTitle = (value: string) => {
    setTitle(value);
    draftRef.current = { ...draftRef.current, title: value };
    dirtyRef.current = true;
    setSaveState("saving");
    scheduleSave();
  };

  const loadMore = async () => {
    try {
      const scopeId = "id" in selectedView ? selectedView.id : null;
      const nextPage = await invoke<SearchResult[]>("search_notes", {
        query: search,
        scope: selectedView.kind,
        scopeId,
        limit: PAGE_SIZE,
        offset: notes.length,
      });
      setNotes((current) => [...current, ...nextPage.map((result) => result.note)]);
      setSearchTitles((current) => ({
        ...current,
        ...Object.fromEntries(
          nextPage.map((result) => [result.note.id, result.highlighted_title]),
        ),
      }));
      setSearchSnippets((current) => ({
        ...current,
        ...Object.fromEntries(nextPage.map((result) => [result.note.id, result.snippet])),
      }));
      setHasMore(nextPage.length === PAGE_SIZE);
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  useEffect(() => {
    if (!notice) return;
    const timer = setTimeout(() => setNotice(null), 6000);
    return () => clearTimeout(timer);
  }, [notice]);

  const exportActiveNote = async (format: ExportFormat | "pdf") => {
    const note = activeNoteRef.current;
    if (!note) return;
    if ((timerRef.current || saveState !== "saved") && !(await flushDraft(note))) return;
    try {
      if (format === "pdf") {
        await invoke("print_window");
        return;
      }
      const saved = await invoke<string | null>("export_note_file", {
        noteId: note.id,
        format,
        language,
        title: t.exportNoteTitle,
      });
      if (saved) setNotice(t.noteExported(saved));
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const handleImported = async (notebookId: string | null) => {
    try {
      await refreshOrganization();
      if (notebookId) {
        await changeView({ kind: "notebook", id: notebookId });
      }
      setNoteListVersion((version) => version + 1);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const refreshOrganization = async () => {
    const [loadedSpaces, loadedNotebooks, loadedTags] = await Promise.all([
      invoke<Space[]>("list_spaces"),
      invoke<Notebook[]>("list_notebooks"),
      invoke<Tag[]>("list_tags"),
    ]);
    setSpaces(loadedSpaces);
    setNotebooks(loadedNotebooks);
    setTags(loadedTags);
  };

  const changeView = async (view: NoteView) => {
    setWorkspace("notes");
    if (
      selectedView.kind === view.kind &&
      "id" in selectedView &&
      "id" in view &&
      selectedView.id === view.id
    ) {
      return;
    }
    if (
      selectedView.kind === view.kind &&
      !("id" in view)
    ) {
      return;
    }
    if (!(await flushDraft(activeNoteRef.current))) {
      return;
    }
    activeNoteRef.current = null;
    activeNoteTrashedRef.current = false;
    setActiveNote(null);
    setNoteTags([]);
    setSelectedView(view);
    setError(null);
  };

  const openWorkspace = async (next: Workspace) => {
    if (next === workspace) return;
    if (!(await flushDraft(activeNoteRef.current))) return;
    setPendingTask(null);
    setPendingEvent(null);
    setTaskCreateRequest(0);
    setEventCreateRequest(0);
    setWorkspace(next);
    setError(null);
  };

  const openNoteFromPlanner = async (noteId: string) => {
    setWorkspace("notes");
    await openLinkedNote(noteId);
  };

  const openReminder = async (reminder: Reminder) => {
    try {
      if (reminder.entity_type === "note") {
        await openNoteFromPlanner(reminder.entity_id);
      } else if (reminder.entity_type === "task") {
        const task = await invoke<Task>("get_task", { id: reminder.entity_id });
        if (!(await flushDraft(activeNoteRef.current))) return;
        setPendingTask(task);
        setWorkspace("tasks");
      } else {
        const event = await invoke<CalendarEvent>("get_event", { id: reminder.entity_id });
        if (!(await flushDraft(activeNoteRef.current))) return;
        setPendingEvent(event);
        setWorkspace("calendar");
      }
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  /** Apre la nota, l'attività o l'evento citato dall'assistente. */
  const openAiSource = async (target: SourceTarget) => {
    try {
      if (target.kind === "task") {
        const task = await invoke<Task>("get_task", { id: target.id });
        if (!(await flushDraft(activeNoteRef.current))) return;
        setPendingTask(task);
        setWorkspace("tasks");
      } else if (target.kind === "event") {
        const event = await invoke<CalendarEvent>("get_event", { id: target.id });
        if (!(await flushDraft(activeNoteRef.current))) return;
        setPendingEvent(event);
        setWorkspace("calendar");
      } else {
        await openNoteFromPlanner(target.id);
      }
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const firedReminders = reminders.filter((reminder) => reminder.status === "fired");
  const urgentTaskCount = (taskCounts?.today ?? 0) + (taskCounts?.overdue ?? 0);

  const createSpace = () => {
    setOrganizationDialogError(null);
    setOrganizationDialog({
      kind: "space",
      intent: "create",
      name: "",
      description: "",
    });
  };

  const editSpace = (space: Space) => {
    setOrganizationDialogError(null);
    setOrganizationDialog({
      kind: "space",
      intent: "edit",
      id: space.id,
      name: space.name,
      description: space.description,
    });
  };

  /** Dopo uno spostamento o un ripristino dal cestino i collegamenti della
   * nota aperta (taccuino, tag) possono essere cambiati. */
  const refreshActiveNoteLinks = async () => {
    const note = activeNoteRef.current;
    if (!note || activeNoteTrashedRef.current) return;
    const current = await invoke<Note>("get_note", { noteId: note.id });
    // Solo i metadati: il contenuto in modifica resta quello dell'editor.
    const updated = { ...note, notebook_id: current.notebook_id };
    activeNoteRef.current = updated;
    setActiveNote(updated);
    setNoteTags(await invoke<Tag[]>("list_note_tags", { noteId: note.id }));
  };

  const moveItemToTrash = async (kind: TrashKind, id: string) => {
    await invoke("move_item_to_trash", { kind, id });
    await refreshOrganization();
    await refreshActiveNoteLinks();
    setNoteListVersion((version) => version + 1);
    setNotice(t.itemMovedToTrash);
  };

  const handleTrashItemsChanged = async () => {
    try {
      await Promise.all([loadTrashItems(), refreshOrganization(), refreshPlanner()]);
      await refreshActiveNoteLinks();
      setNoteListVersion((version) => version + 1);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const deleteSpace = async (space: Space) => {
    if (!(await askConfirmation({ title: t.titleDeleteSpace, message: t.confirmDeleteSpace(space.name), confirmLabel: t.actionMoveToTrash }))) return;
    try {
      await moveItemToTrash("space", space.id);
      if (selectedView.kind === "space" && selectedView.id === space.id) {
        await changeView({ kind: "all" });
      }
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const createNotebook = (spaceId: string | null = null) => {
    setOrganizationDialogError(null);
    setOrganizationDialog({
      kind: "notebook",
      intent: "create",
      name: "",
      description: "",
      spaceId,
    });
  };
  createNotebookShortcut.current = () => createNotebook();

  const editNotebook = (notebook: Notebook) => {
    setOrganizationDialogError(null);
    setOrganizationDialog({
      kind: "notebook",
      intent: "edit",
      id: notebook.id,
      name: notebook.name,
      description: notebook.description,
      spaceId: notebook.space_id,
    });
  };

  const deleteNotebook = async (notebook: Notebook) => {
    if (!(await askConfirmation({ title: t.titleDeleteNotebook, message: t.confirmDeleteNotebook(notebook.name), confirmLabel: t.actionMoveToTrash }))) return;
    try {
      await moveItemToTrash("notebook", notebook.id);
      if (selectedView.kind === "notebook" && selectedView.id === notebook.id) {
        await changeView({ kind: "all" });
      }
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const createTag = () => {
    setOrganizationDialogError(null);
    setOrganizationDialog({ kind: "tag", intent: "create", name: "" });
  };

  const editTag = (tag: Tag) => {
    setOrganizationDialogError(null);
    setOrganizationDialog({
      kind: "tag",
      intent: "edit",
      id: tag.id,
      name: tag.name,
    });
  };

  const submitOrganizationDialog = async (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault();
    if (!organizationDialog || isSavingOrganization) return;
    setIsSavingOrganization(true);
    try {
      if (organizationDialog.kind === "space") {
        const { id, name, description, intent } = organizationDialog;
        if (intent === "edit" && id) {
          await invoke<Space>("update_space", { id, name, description });
        } else {
          await invoke<Space>("create_space", { name, description });
        }
      } else if (organizationDialog.kind === "notebook") {
        const { id, name, description, spaceId, intent } = organizationDialog;
        if (intent === "edit" && id) {
          await invoke<Notebook>("update_notebook", { id, spaceId, name, description });
        } else {
          await invoke<Notebook>("create_notebook", { spaceId, name, description });
        }
      } else if (organizationDialog.intent === "edit" && organizationDialog.id) {
        await invoke<Tag>("update_tag", {
          id: organizationDialog.id,
          name: organizationDialog.name,
        });
      } else {
        await invoke<Tag>("create_tag", { name: organizationDialog.name });
      }
      await refreshOrganization();
      if (activeNoteRef.current && !activeNoteTrashedRef.current) {
        setNoteTags(await invoke<Tag[]>("list_note_tags", { noteId: activeNoteRef.current.id }));
      }
      setOrganizationDialog(null);
    } catch (cause) {
      setOrganizationDialogError(errorMessage(cause));
    } finally {
      setIsSavingOrganization(false);
    }
  };

  const deleteTag = async (tag: Tag) => {
    if (!(await askConfirmation({ title: t.titleDeleteTag, message: t.confirmDeleteTag(tag.name), confirmLabel: t.actionMoveToTrash }))) return;
    try {
      await moveItemToTrash("tag", tag.id);
      if (selectedView.kind === "tag" && selectedView.id === tag.id) {
        await changeView({ kind: "all" });
      }
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const trashActiveNote = async () => {
    const note = activeNoteRef.current;
    if (!note || activeNoteTrashedRef.current) return;
    if (!(await askConfirmation({ title: t.titleTrashNote, message: t.confirmTrashNote, confirmLabel: t.actionMoveToTrash }))) return;
    if (!(await flushDraft(note))) return;
    try {
      await invoke("trash_note", { noteId: note.id });
      activeNoteRef.current = null;
      activeNoteTrashedRef.current = false;
      setActiveNote(null);
      setNoteTags([]);
      setNotes((current) => current.filter((item) => item.id !== note.id));
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const toggleActiveNotePin = async () => {
    const note = activeNoteRef.current;
    if (!note || activeNoteTrashedRef.current) return;
    try {
      const updated = await invoke<Note>("set_note_pinned", {
        noteId: note.id,
        pinned: !note.is_pinned,
      });
      activeNoteRef.current = updated;
      setActiveNote(updated);
      setNotes((current) => current.map((item) => item.id === updated.id ? updated : item));
      setNoteListVersion((version) => version + 1);
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const setActiveNoteArchived = async (archived: boolean) => {
    const note = activeNoteRef.current;
    if (!note || activeNoteTrashedRef.current) return;
    if (archived && !(await askConfirmation({ title: t.titleArchiveNote, message: t.confirmArchiveNote, confirmLabel: t.actionArchive }))) return;
    if (!(await flushDraft(note))) return;
    try {
      const updated = await invoke<Note>("set_note_archived", { noteId: note.id, archived });
      if (archived) {
        activeNoteRef.current = null;
        setActiveNote(null);
        setNoteTags([]);
      } else {
        activeNoteRef.current = updated;
        setActiveNote(updated);
      }
      setNotes((current) => current.filter((item) => item.id !== note.id));
      setNoteListVersion((version) => version + 1);
      if (!archived) setSelectedView({ kind: "all" });
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const restoreActiveNote = async () => {
    const note = activeNoteRef.current;
    if (!note || !activeNoteTrashedRef.current) return;
    try {
      const restored = await invoke<Note>("restore_note", { noteId: note.id });
      activeNoteRef.current = restored;
      activeNoteTrashedRef.current = false;
      draftRef.current = { title: restored.title, content: restored.content };
      dirtyRef.current = false;
      setActiveNote(restored);
      setTitle(restored.title);
      setNotes((current) => current.filter((item) => item.id !== restored.id));
      setSelectedView(restored.archived_at ? { kind: "archived" } : { kind: "all" });
      setError(null);
      setNoteTags(await invoke<Tag[]>("list_note_tags", { noteId: restored.id }));
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const deleteActiveNotePermanently = async () => {
    const note = activeNoteRef.current;
    if (!note || !activeNoteTrashedRef.current) return;
    if (!(await askConfirmation({ title: t.titleDeletePermanently, message: t.confirmDeletePermanently(note.title || t.untitled), confirmLabel: t.actionDeletePermanently, destructive: true }))) return;
    try {
      await invoke("delete_note_permanently", { noteId: note.id });
      activeNoteRef.current = null;
      activeNoteTrashedRef.current = false;
      dirtyRef.current = false;
      setActiveNote(null);
      setNoteTags([]);
      setNotes((current) => current.filter((item) => item.id !== note.id));
      setNotice(t.noteDeletedPermanently);
      setError(null);
      void refreshPlanner();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const emptyTrash = async () => {
    if (!(await askConfirmation({ title: t.emptyTrashAction, message: t.confirmEmptyTrash, confirmLabel: t.emptyTrashAction, destructive: true }))) return;
    try {
      const summary = await invoke<EmptyTrashSummary>("empty_trash");
      activeNoteRef.current = null;
      activeNoteTrashedRef.current = false;
      dirtyRef.current = false;
      setActiveNote(null);
      setNoteTags([]);
      setNotes([]);
      setHasMore(false);
      setTrashItems([]);
      setNotice(t.trashEmptied(summary.notes + summary.items));
      setError(null);
      void refreshPlanner();
      void refreshOrganization();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const moveActiveNote = async (notebookId: string | null) => {
    const note = activeNoteRef.current;
    if (!note || activeNoteTrashedRef.current || note.notebook_id === notebookId) return;
    if (!(await flushDraft(note))) return;
    try {
      const moved = await invoke<Note>("move_note", { noteId: note.id, notebookId });
      activeNoteRef.current = moved;
      setActiveNote(moved);
      setNotes((current) => current.map((item) => item.id === moved.id ? moved : item));
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  /** Tag della nota dalla barra in basso: salva l'insieme completo. */
  const setActiveNoteTags = async (nextTags: Tag[]) => {
    const note = activeNoteRef.current;
    if (!note || activeNoteTrashedRef.current) return;
    try {
      await invoke("set_note_tags", { noteId: note.id, tagIds: nextTags.map((item) => item.id) });
      setNoteTags(nextTags);
      if (
        selectedView.kind === "tag" &&
        !nextTags.some((item) => item.id === selectedView.id)
      ) {
        setNotes((current) => current.filter((item) => item.id !== note.id));
      }
      setNoteListVersion((version) => version + 1);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const createTagNamed = async (name: string): Promise<Tag | null> => {
    try {
      const tag = await invoke<Tag>("create_tag", { name });
      setTags((current) => [...current, tag].sort((a, b) => a.name.localeCompare(b.name)));
      return tag;
    } catch (cause) {
      setError(errorMessage(cause));
      return null;
    }
  };

  const addAttachmentFiles = async (files: FileList | File[]) => {
    const note = activeNoteRef.current;
    if (!note || activeNoteTrashedRef.current || isUploadingAttachment) return;
    setIsUploadingAttachment(true);
    try {
      for (const file of Array.from(files)) {
        if (file.size > 25 * 1024 * 1024) {
          throw new Error(t.attachmentTooLarge);
        }
        const bytes = new Uint8Array(await file.arrayBuffer());
        let binary = "";
        const chunkSize = 0x8000;
        for (let index = 0; index < bytes.length; index += chunkSize) {
          binary += String.fromCharCode(...bytes.subarray(index, index + chunkSize));
        }
        const attachment = await invoke<Attachment>("add_note_attachment", {
          noteId: note.id,
          filename: file.name,
          dataBase64: btoa(binary),
        });
        setAttachments((current) => [...current, attachment]);
        if (attachment.mime_type.startsWith("image/")) {
          insertImage(attachment);
          if (aiStatus?.settings.ocr_on_attach) void readImageText(attachment, true);
        }
      }
      setNoteListVersion((version) => version + 1);
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause) || t.attachmentError);
    } finally {
      setIsUploadingAttachment(false);
      if (attachmentInput.current) attachmentInput.current.value = "";
    }
  };

  pasteImagesRef.current = (files: File[]) => addAttachmentFiles(files);

  /** Inserisce l'immagine nel testo, dopo il blocco corrente. */
  const insertImage = (attachment: Attachment) => {
    if (!editor || editor.isDestroyed || activeNoteTrashedRef.current) return;
    editor
      .chain()
      .focus()
      .insertContent({
        type: "image",
        attrs: { attachmentId: attachment.id, alt: attachment.original_filename },
      })
      .run();
  };

  const [readingImages, setReadingImages] = useState<Set<string>>(() => new Set());

  /** Legge il testo di un'immagine (OCR) e aggiorna il pannello allegati. */
  const readImageText = async (attachment: Attachment, quiet = false) => {
    setReadingImages((current) => new Set(current).add(attachment.id));
    try {
      const updated = await invoke<Attachment>("read_image_text", {
        requestId: crypto.randomUUID(),
        attachmentId: attachment.id,
      });
      setAttachments((current) => current.map((item) => (item.id === updated.id ? updated : item)));
      if (!quiet) {
        setNotice(updated.ocr_status === "done" ? t.imageTextRead : t.imageTextFailed(updated.ocr_error ?? ""));
      }
    } catch (cause) {
      if (!quiet) setError(errorMessage(cause));
    } finally {
      setReadingImages((current) => {
        const next = new Set(current);
        next.delete(attachment.id);
        return next;
      });
    }
  };

  const openAttachment = async (attachment: Attachment) => {
    if (!(await askConfirmation({ title: t.titleOpenAttachment, message: t.confirmOpenAttachment(attachment.original_filename), confirmLabel: t.actionOpen }))) return;
    const note = activeNoteRef.current;
    if (!note) return;
    try {
      await invoke("open_note_attachment", {
        noteId: note.id,
        attachmentId: attachment.id,
      });
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const removeAttachment = async (attachment: Attachment) => {
    const note = activeNoteRef.current;
    if (!note || !(await askConfirmation({ title: t.titleRemoveAttachment, message: t.confirmRemoveAttachment(attachment.original_filename), confirmLabel: t.actionRemove, destructive: true }))) {
      return;
    }
    try {
      await invoke("delete_note_attachment", {
        noteId: note.id,
        attachmentId: attachment.id,
      });
      setAttachments((current) => current.filter((item) => item.id !== attachment.id));
      setNoteListVersion((version) => version + 1);
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const loadLinkCandidates = async () => {
    try {
      const results = await invoke<SearchResult[]>("search_notes", {
        query: "",
        scope: "all",
        scopeId: null,
        limit: 500,
        offset: 0,
      });
      setLinkCandidates(results.map((result) => result.note));
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const addNoteLink = async () => {
    const note = activeNoteRef.current;
    if (!note || !selectedLinkTarget || activeNoteTrashedRef.current) return;
    try {
      await invoke("create_note_link", {
        sourceNoteId: note.id,
        targetNoteId: selectedLinkTarget,
        label: null,
      });
      setNoteLinks(await invoke<NoteLink[]>("list_note_links", { sourceNoteId: note.id }));
      setSelectedLinkTarget("");
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const openLinkedNote = async (targetNoteId: string) => {
    try {
      const linkedNote = await invoke<Note>("get_note", { noteId: targetNoteId });
      if (selectedView.kind !== "all") await changeView({ kind: "all" });
      setSearch("");
      setNotes((current) => [
        linkedNote,
        ...current.filter((item) => item.id !== linkedNote.id),
      ]);
      await selectNote(linkedNote, true);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const removeNoteLink = async (targetNoteId: string) => {
    const note = activeNoteRef.current;
    if (!note) return;
    try {
      await invoke("delete_note_link", {
        sourceNoteId: note.id,
        targetNoteId,
      });
      setNoteLinks((current) =>
        current.filter((link) => link.target_note_id !== targetNoteId),
      );
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const handleAttachmentDrop = (event: DragEvent<HTMLDivElement>) => {
    if (event.dataTransfer.files.length === 0) return;
    event.preventDefault();
    event.stopPropagation();
    void addAttachmentFiles(event.dataTransfer.files);
  };

  const titleForView = (view: NoteView): string =>
    view.kind === "trash"
      ? t.trash
      : view.kind === "pinned"
        ? t.pinnedNotes
        : view.kind === "recent"
          ? t.recentNotes
          : view.kind === "archived"
            ? t.archivedNotes
            : view.kind === "space"
              ? spaces.find((space) => space.id === view.id)?.name ?? t.allNotes
              : view.kind === "notebook"
                ? notebooks.find((notebook) => notebook.id === view.id)?.name ?? t.allNotes
                : view.kind === "unfiled"
                  ? t.uncategorized
                  : view.kind === "tag"
                    ? `# ${tags.find((tag) => tag.id === view.id)?.name ?? ""}`
                    : t.allNotes;

  const workspaceLabels: Record<Workspace, string> = {
    home: t.homeNav,
    notes: t.allNotes,
    tasks: t.tasksNav,
    reminders: t.remindersNav,
    calendar: t.calendarNav,
    files: t.filesNav,
    templates: t.templatesNav,
    assistant: t.assistantNav,
  };

  // -------------------------------------------------------------------------
  // Navigazione: schede e cronologia avanti/indietro

  const currentLocation = useMemo(
    () => ({ workspace, view: selectedView, noteId: activeNote?.id ?? null }),
    [activeNote?.id, selectedView, workspace],
  );
  const currentKey = locationKey(currentLocation);
  const currentLabel = activeNote
    ? title || t.untitled
    : workspace === "notes"
      ? titleForView(selectedView)
      : workspaceLabels[workspace];
  const activeTab = tabs.find((tab) => tab.id === activeTabId) ?? tabs[0];

  useEffect(() => {
    if (navigatingRef.current || !isDesktop) return;
    setTabs((current) =>
      current.map((tab) => {
        if (tab.id !== activeTabId) return tab;
        const here = tab.history[tab.index];
        if (here && locationKey(here) === currentKey) {
          if (here.label === currentLabel) return tab;
          const history = [...tab.history];
          history[tab.index] = { ...here, label: currentLabel };
          return { ...tab, history };
        }
        const history = [
          ...tab.history.slice(0, tab.index + 1),
          { ...currentLocation, label: currentLabel },
        ].slice(-MAX_HISTORY);
        return { ...tab, history, index: history.length - 1 };
      }),
    );
    // currentLocation è derivato da currentKey.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeTabId, currentKey, currentLabel]);

  const clearActiveNote = () => {
    activeNoteRef.current = null;
    activeNoteTrashedRef.current = false;
    dirtyRef.current = false;
    setActiveNote(null);
    setNoteTags([]);
  };

  /** Porta l'interfaccia in una posizione senza registrarla nella cronologia. */
  const goTo = async (location: Location): Promise<boolean> => {
    if (!(await flushDraft(activeNoteRef.current))) return false;
    navigatingRef.current = true;
    try {
      setContextMenu(null);
      setIsFindOpen(false);
      setTaskCreateRequest(0);
      setEventCreateRequest(0);
      setWorkspace(location.workspace);
      setSelectedView(location.view);
      if (location.noteId) {
        try {
          const note = await invoke<Note>("get_note", { noteId: location.noteId });
          setNotes((current) => [note, ...current.filter((item) => item.id !== note.id)]);
          await selectNote(note, true);
        } catch {
          clearActiveNote();
        }
      } else {
        clearActiveNote();
      }
      return true;
    } finally {
      // Il render successivo non deve registrare la posizione raggiunta.
      window.setTimeout(() => {
        navigatingRef.current = false;
      }, 0);
    }
  };

  const goBack = async () => {
    const tab = activeTab;
    if (tab.index <= 0) return;
    if (await goTo(tab.history[tab.index - 1])) {
      setTabs((current) => current.map((item) => (item.id === tab.id ? { ...item, index: item.index - 1 } : item)));
    }
  };

  const goForward = async () => {
    const tab = activeTab;
    if (tab.index >= tab.history.length - 1) return;
    if (await goTo(tab.history[tab.index + 1])) {
      setTabs((current) => current.map((item) => (item.id === tab.id ? { ...item, index: item.index + 1 } : item)));
    }
  };

  const openTab = async (location?: Location) => {
    const target: Location = location ?? {
      workspace: "home",
      view: { kind: "all" },
      noteId: null,
      label: t.homeNav,
    };
    if (!(await flushDraft(activeNoteRef.current))) return;
    const tab: TabState = { id: newId(), history: [target], index: 0 };
    setTabs((current) => [...current, tab]);
    setActiveTabId(tab.id);
    await goTo(target);
  };

  const selectTab = async (id: string) => {
    const tab = tabs.find((item) => item.id === id);
    if (!tab || id === activeTabId) return;
    if (!(await flushDraft(activeNoteRef.current))) return;
    setActiveTabId(id);
    await goTo(tab.history[tab.index]);
  };

  const cycleTab = (direction: 1 | -1) => {
    const index = tabs.findIndex((tab) => tab.id === activeTabId);
    const next = tabs[(index + direction + tabs.length) % tabs.length];
    if (next) void selectTab(next.id);
  };

  const closeTab = async (id = activeTabId) => {
    if (tabs.length <= 1) return;
    const index = tabs.findIndex((tab) => tab.id === id);
    const remaining = tabs.filter((tab) => tab.id !== id);
    if (id !== activeTabId) {
      setTabs(remaining);
      return;
    }
    if (!(await flushDraft(activeNoteRef.current))) return;
    const next = remaining[Math.min(index, remaining.length - 1)];
    setTabs(remaining);
    setActiveTabId(next.id);
    await goTo(next.history[next.index]);
  };

  // Vista iniziale scelta nelle impostazioni (una sola volta, all'avvio).
  useEffect(() => {
    if (!preferencesLoaded || startViewAppliedRef.current || !isDesktop) return;
    startViewAppliedRef.current = true;
    if (preferences.startView === "notes") {
      navigatingRef.current = true;
      setWorkspace("notes");
      setTabs((current) =>
        current.map((tab, index) =>
          index === 0
            ? { ...tab, history: [{ workspace: "notes", view: { kind: "all" }, noteId: null, label: t.allNotes }], index: 0 }
            : tab,
        ),
      );
      window.setTimeout(() => {
        navigatingRef.current = false;
      }, 0);
    }
  }, [preferences.startView, preferencesLoaded]);

  // -------------------------------------------------------------------------
  // Barra laterale: contatori e note recenti

  useEffect(() => {
    if (!isDesktop) return;
    void invoke<SidebarCounts>("sidebar_counts")
      .then(setSidebarCounts)
      .catch(() => setSidebarCounts(null));
  }, [noteListVersion, plannerVersion, spaces, notebooks, tags, trashItems]);

  useEffect(() => {
    if (!isDesktop || !preferences.showRecentNotes) return;
    void invoke<SearchResult[]>("search_notes", {
      query: "",
      scope: "recent",
      scopeId: null,
      limit: SIDEBAR_RECENT_LIMIT,
      offset: 0,
    })
      .then((results) =>
        setSidebarRecent((current) => mergeRecent(current, results.map((result) => result.note))),
      )
      .catch(() => setSidebarRecent([]));
  }, [activeNote?.id, noteListVersion, preferences.showRecentNotes]);

  // -------------------------------------------------------------------------
  // Azioni dei menu

  const openNoteById = async (noteId: string) => {
    setWorkspace("notes");
    await openLinkedNote(noteId);
  };

  const startNewTask = async () => {
    if (!(await flushDraft(activeNoteRef.current))) return;
    setPendingTask(null);
    setWorkspace("tasks");
    setTaskCreateRequest((value) => value + 1);
  };

  const startNewEvent = async () => {
    if (!(await flushDraft(activeNoteRef.current))) return;
    setPendingEvent(null);
    setWorkspace("calendar");
    setEventCreateRequest((value) => value + 1);
  };

  const openDataDialog = (tab: "import" | "export" | "backup" | "integrity") => {
    setDataDialogTab(tab);
    setIsDataDialogOpen(true);
  };

  const focusSearch = () => {
    if (workspace !== "notes") setWorkspace("notes");
    window.requestAnimationFrame(() => {
      searchInput.current?.focus();
      searchInput.current?.select();
    });
  };

  const activeTextField = (): HTMLInputElement | HTMLTextAreaElement | null => {
    const element = document.activeElement;
    return element instanceof HTMLInputElement || element instanceof HTMLTextAreaElement ? element : null;
  };

  const insertIntoField = (field: HTMLInputElement | HTMLTextAreaElement, text: string) => {
    const start = field.selectionStart ?? field.value.length;
    const end = field.selectionEnd ?? start;
    const value = field instanceof HTMLInputElement ? text.replace(/\s*\n\s*/g, " ") : text;
    field.setRangeText(value, start, end, "end");
    field.dispatchEvent(new Event("input", { bubbles: true }));
  };

  const editAction = (command: "undo" | "redo" | "cut" | "copy" | "selectAll") => {
    if (editor?.isFocused) {
      if (command === "undo") editor.chain().focus().undo().run();
      else if (command === "redo") editor.chain().focus().redo().run();
      else if (command === "selectAll") editor.chain().focus().selectAll().run();
      else document.execCommand(command);
      return;
    }
    const field = activeTextField();
    if (command === "selectAll" && field) {
      field.select();
      return;
    }
    document.execCommand(command);
  };

  const pasteText = async (plain: boolean) => {
    try {
      const text = await invoke<string>("read_clipboard_text");
      if (!text) return;
      if (editor?.isFocused || (!activeTextField() && editor && activeNote && !isTrashView)) {
        const paragraphs = text.replace(/\r\n?/g, "\n").split("\n");
        const content = paragraphs.map((line) =>
          line ? { type: "paragraph", content: [{ type: "text", text: line }] } : { type: "paragraph" },
        );
        // Una sola riga: testo in linea, senza creare un nuovo paragrafo.
        if (paragraphs.length === 1) editor?.chain().focus().insertContent(text).run();
        else editor?.chain().focus().insertContent(content).run();
        void plain;
        return;
      }
      const field = activeTextField();
      if (field) insertIntoField(field, text);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const reloadData = async () => {
    if (!(await flushDraft(activeNoteRef.current))) return;
    try {
      await refreshOrganization();
      await refreshPlanner();
      await loadTrashItems();
      setNoteListVersion((version) => version + 1);
      const note = activeNoteRef.current;
      if (note && !activeNoteTrashedRef.current) {
        const fresh = await invoke<Note>("get_note", { noteId: note.id });
        activeNoteRef.current = fresh;
        draftRef.current = { title: fresh.title, content: fresh.content };
        dirtyRef.current = false;
        setActiveNote(fresh);
        setTitle(fresh.title);
        editor?.commands.setContent(fresh.content, { emitUpdate: false });
      }
      setNotice(t.dataReloaded);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  /** Dopo una sincronizzazione con modifiche remote: elenchi aggiornati e,
   *  se la nota aperta è cambiata e non ha modifiche in sospeso, editor
   *  ricaricato. Con modifiche in sospeso il salvataggio conserva il testo
   *  arrivato (cronologia e conflitti). */
  const applySyncChanges = useCallback(
    async (changedNotes: string[]) => {
      try {
        await refreshOrganization();
        await refreshPlanner();
        await loadTrashItems();
        setNoteListVersion((version) => version + 1);
        const note = activeNoteRef.current;
        if (note && changedNotes.includes(note.id) && !dirtyRef.current && !timerRef.current) {
          const fresh = await invoke<Note>("get_note", { noteId: note.id }).catch(() => null);
          if (fresh && activeNoteRef.current?.id === fresh.id && !dirtyRef.current) {
            activeNoteRef.current = fresh;
            draftRef.current = { title: fresh.title, content: fresh.content };
            setActiveNote(fresh);
            setTitle(fresh.title);
            editor?.commands.setContent(fresh.content, { emitUpdate: false });
          }
        }
      } catch (cause) {
        setError(errorMessage(cause));
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [editor],
  );

  // Stato della sincronizzazione per l'indicatore in basso.
  const [syncInfo, setSyncInfo] = useState<{ enabled: boolean; at: string | null; error: string | null; conflicts: number }>({
    enabled: false,
    at: null,
    error: null,
    conflicts: 0,
  });
  const refreshSyncInfo = useCallback(async () => {
    try {
      const view = await invoke<SyncView>("get_sync_status");
      setSyncInfo({
        enabled: view.settings.enabled,
        at: view.last_sync_at,
        error: view.last_error,
        conflicts: view.conflicts.length,
      });
    } catch {
      // Indicatore facoltativo: un errore qui non blocca nulla.
    }
  }, []);
  useEffect(() => {
    void refreshSyncInfo();
    let stop: (() => void) | undefined;
    let cancelled = false;
    void listen<{ report: { imported: number; changed_notes: string[] } | null; error: string | null }>(
      SYNC_FINISHED_EVENT,
      (event) => {
        void refreshSyncInfo();
        const report = event.payload.report;
        if (report && report.imported > 0) void applySyncChanges(report.changed_notes);
      },
    ).then((unlisten) => (cancelled ? unlisten() : (stop = unlisten)));
    return () => {
      cancelled = true;
      stop?.();
    };
  }, [applySyncChanges, refreshSyncInfo]);

  const reloadWindow = async () => {
    if (await flushDraft(activeNoteRef.current)) window.location.reload();
  };

  const quitApp = async () => {
    if (!(await flushDraft(activeNoteRef.current))) return;
    allowWindowCloseRef.current = true;
    await invoke("quit_app");
  };

  const changeZoom = (direction: 0 | 1 | -1) => {
    updatePreferences({ zoom: direction === 0 ? 1 : nextZoom(preferences.zoom, direction) });
  };

  const withNote = async (action: (note: Note) => Promise<void>) => {
    const note = activeNoteRef.current;
    if (!note) return;
    if (!activeNoteTrashedRef.current && !(await flushDraft(note))) return;
    try {
      await action(note);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const openNoteInWindow = () =>
    withNote(async (note) => {
      await invoke("open_note_window", { noteId: note.id, title: note.title || t.untitled });
      // La nota resta modificabile in una sola finestra alla volta.
      clearActiveNote();
      setNotice(t.noteOpenedInWindow);
    });

  const openNoteInTab = () =>
    withNote(async (note) => {
      await openTab({ workspace: "notes", view: selectedView, noteId: note.id, label: note.title || t.untitled });
    });

  const copyNoteAs = (kind: "markdown" | "text") =>
    withNote(async (note) => {
      await invoke("copy_note_as_text", { noteId: note.id, format: kind, language });
      setNotice(kind === "markdown" ? t.copiedAsMarkdown : t.copiedAsText);
    });

  const copyNoteLink = (internal: boolean) =>
    withNote(async (note) => {
      const link = `greatnote://note/${note.id}`;
      const text = internal ? `[${note.title || t.untitled}](${link})` : link;
      await invoke("write_clipboard_text", { text });
      setNotice(t.linkCopied);
    });

  const duplicateActiveNote = () =>
    withNote(async (note) => {
      const copy = await invoke<Note>("copy_note", {
        noteId: note.id,
        target: "same",
        titleSuffix: t.copySuffix,
      });
      setNoteListVersion((version) => version + 1);
      setNotes((current) => [copy, ...current]);
      await selectNote(copy, true);
      setNotice(t.noteDuplicated);
    });

  const moveOrCopyNote = async (mode: "move" | "copy", notebookId: string | null) => {
    setNotebookPicker(null);
    if (mode === "move") {
      await moveActiveNote(notebookId);
      setNoteListVersion((version) => version + 1);
      return;
    }
    await withNote(async (note) => {
      const copy = await invoke<Note>("copy_note", {
        noteId: note.id,
        target: notebookId ?? "unfiled",
        titleSuffix: null,
      });
      setNoteListVersion((version) => version + 1);
      setNotice(t.noteCopiedTo(notebooks.find((notebook) => notebook.id === notebookId)?.name ?? t.noNotebook));
      void copy;
    });
  };

  const saveAsTemplate = () =>
    withNote(async (note) => {
      const name = await askText({
        title: t.menuSaveAsTemplate,
        label: t.templateName,
        initialValue: note.title || t.untitled,
        confirmLabel: t.saveTask,
      });
      if (!name) return;
      await invoke<Template>("create_template_from_note", { noteId: note.id, name });
      setNotice(t.templateSaved(name));
    });

  const useTemplate = async (template: Template) => {
    if (!(await flushDraft(activeNoteRef.current))) return;
    try {
      const notebookId = selectedView.kind === "notebook" ? selectedView.id : null;
      const note = await invoke<Note>("create_note_from_template", {
        templateId: template.id,
        notebookId,
        language,
      });
      setWorkspace("notes");
      setNoteListVersion((version) => version + 1);
      setNotes((current) => [note, ...current]);
      await selectNote(note, true);
      window.requestAnimationFrame(() => titleInput.current?.focus());
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const pasteNoteLink = async () => {
    const note = activeNoteRef.current;
    if (!note) return;
    try {
      const text = await invoke<string>("read_clipboard_text");
      const target = /greatnote:\/\/note\/([0-9a-f-]{36})/i.exec(text)?.[1];
      if (!target) {
        setError(t.noNoteLinkInClipboard);
        return;
      }
      await invoke("create_note_link", { sourceNoteId: note.id, targetNoteId: target, label: null });
      setNoteLinks(await invoke<NoteLink[]>("list_note_links", { sourceNoteId: note.id }));
      setError(null);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  // La barra laterale è nascosta dall'opzione o dalla vista "Concentrazione".
  const sidebarVisible = !preferences.sidebarHidden && layout !== "focus";
  const toggleSidebar = () => {
    if (sidebarVisible) {
      updatePreferences({ sidebarHidden: true });
      return;
    }
    // Ripristina sempre una disposizione con la navigazione visibile.
    if (layout === "focus") setLayout("normal");
    updatePreferences({ sidebarHidden: false });
  };

  // Le viste della nota hanno senso solo con una nota: uscendo dalle note si
  // torna alla disposizione normale, così la navigazione resta raggiungibile.
  useEffect(() => {
    if (workspace !== "notes" && layout !== "normal") setLayout("normal");
  }, [layout, workspace]);

  const cycleLayout = (direction: 1 | -1) => {
    setLayout((current) => LAYOUTS[(LAYOUTS.indexOf(current) + direction + LAYOUTS.length) % LAYOUTS.length]);
  };

  const historyBeforeLoad = useCallback(() => flushDraft(activeNoteRef.current), [flushDraft]);

  const handleRevisionRestored = (restored: Note) => {
    activeNoteRef.current = restored;
    draftRef.current = { title: restored.title, content: restored.content };
    dirtyRef.current = false;
    setActiveNote(restored);
    setTitle(restored.title);
    editor?.commands.setContent(restored.content, { emitUpdate: false });
    setNotes((current) => current.map((item) => (item.id === restored.id ? restored : item)));
    setNotice(t.revisionRestored);
  };

  // -------------------------------------------------------------------------
  // Funzioni AI (facoltative): ogni proposta passa da un'anteprima.

  const openSettings = (section: SettingsSection = "general") => {
    setSettingsSection(section);
    setIsSettingsOpen(true);
  };

  const startAiIndexing = async () => {
    try {
      await aiApi.startIndexing();
      setAiProgress({ done: 0, total: aiStatus?.index.pending_notes ?? 0 });
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const cancelAiIndexing = () => {
    void aiApi.cancelIndexing().catch((cause: unknown) => setError(errorMessage(cause)));
  };

  const startAiTransform = (action: TransformAction, label: string) => {
    if (!editor || !activeNote || isTrashView) return;
    const { from, to, empty } = editor.state.selection;
    const selectionText = empty ? "" : editor.state.doc.textBetween(from, to, "\n\n", "\n").trim();
    const text = selectionText || editor.getText({ blockSeparator: "\n\n" }).trim();
    if (!text) {
      setNotice(t.aiNothingToProcess);
      return;
    }
    aiRangeRef.current = selectionText ? { from, to } : { from: 0, to: editor.state.doc.content.size };
    setAiTransform({ action, label, text, scope: selectionText ? "selection" : "note", noteId: activeNote.id });
  };

  const applyAiResult = async (mode: "replace" | "insert", markdown: string) => {
    if (!editor || editor.isDestroyed) throw new Error(t.aiEditorUnavailable);
    const converted = (await aiApi.markdownToDocument(markdown)) as JSONContent;
    const content = sanitizeDocument(converted).content ?? [];
    const size = editor.state.doc.content.size;
    const from = Math.min(aiRangeRef.current.from, size);
    const to = Math.min(aiRangeRef.current.to, size);
    if (mode === "replace") {
      editor.chain().focus().insertContentAt({ from, to }, content).run();
    } else {
      // Dopo il blocco che contiene la fine della selezione.
      const end = editor.state.doc.resolve(to);
      editor.chain().focus().insertContentAt(end.depth >= 1 ? end.after(1) : to, content).run();
    }
    setNotice(t.aiApplied);
  };

  const askAiAboutNote = async () => {
    const note = activeNoteRef.current;
    if (!note || !(await flushDraft(note))) return;
    setAssistantNote({ id: note.id, title: draftRef.current.title || note.title });
    await openWorkspace("assistant");
  };

  const openAiSuggestions = async (kind: "tags" | "tasks") => {
    const note = activeNoteRef.current;
    if (!note || !(await flushDraft(note))) return;
    setAiSuggestions(kind);
  };

  const aiItems = () =>
    aiNoteItems({
      ready: aiReady(aiStatus),
      hasNote: Boolean(activeNote) && workspace === "notes" && !isTrashView && Boolean(editor),
      transform: startAiTransform,
      askAboutNote: () => void askAiAboutNote(),
      suggestTags: () => void openAiSuggestions("tags"),
      extractTasks: () => void openAiSuggestions("tasks"),
      openSettings: () => openSettings("ai"),
    });

  const menuContext: MenuContext = {
    isDesktop,
    hasNote: Boolean(activeNote) && workspace === "notes",
    noteEditable: Boolean(activeNote) && !isTrashView,
    noteTrashed: isTrashView,
    notePinned: Boolean(activeNote?.is_pinned),
    noteArchived: Boolean(activeNote?.archived_at),
    canGoBack: activeTab.index > 0,
    canGoForward: activeTab.index < activeTab.history.length - 1,
    tabs: tabs.map((tab) => ({
      id: tab.id,
      label: tab.history[tab.index]?.label || t.homeNav,
      active: tab.id === activeTabId,
    })),
    preferences,
    layout,
    sidebarVisible,
    blockCommands: blockCommands(),
    editorAvailable: Boolean(editor) && !isTrashView,
    newNote: () => void createNote(),
    newNotebook: () => createNotebook(),
    newSpace: () => createSpace(),
    newEvent: () => void startNewEvent(),
    newTask: () => void startNewTask(),
    newTag: () => createTag(),
    newNoteFromTemplate: () => void openWorkspace("templates"),
    saveNow: () => void flushDraft(activeNoteRef.current).then((saved) => saved && setNotice(t.savedNow)),
    importData: () => openDataDialog("import"),
    exportNote: (kind: ExportKind) => void exportActiveNote(kind),
    printNote: () => void exportActiveNote("pdf"),
    openData: () => openDataDialog("backup"),
    quit: () => void quitApp(),
    undo: () => editAction("undo"),
    redo: () => editAction("redo"),
    cut: () => editAction("cut"),
    copy: () => editAction("copy"),
    paste: (plain) => void pasteText(plain),
    selectAll: () => editAction("selectAll"),
    focusSearch,
    quickSwitch: () => setIsSwitcherOpen(true),
    reloadData: () => void reloadData(),
    reloadWindow: () => void reloadWindow(),
    goBack: () => void goBack(),
    goForward: () => void goForward(),
    zoom: changeZoom,
    toggleFullscreen: () => void invoke("toggle_fullscreen").catch((cause: unknown) => setError(errorMessage(cause))),
    setPreference: (patch) => {
      // Mostrare la barra laterale dal menu esce anche dalla concentrazione.
      if (patch.sidebarHidden === false && layout === "focus") setLayout("normal");
      updatePreferences(patch);
    },
    openNoteInWindow: () => void openNoteInWindow(),
    openNoteInTab: () => void openNoteInTab(),
    setLayout,
    cycleLayout,
    copyNoteAs: (kind) => void copyNoteAs(kind),
    moveNote: () => setNotebookPicker("move"),
    copyNoteTo: () => setNotebookPicker("copy"),
    duplicateNote: () => void duplicateActiveNote(),
    editTags: () => setIsTagEditorOpen(true),
    toggleShortcut: () => void toggleActiveNotePin(),
    toggleArchive: () => void setActiveNoteArchived(!activeNote?.archived_at),
    copyNoteLink: () => void copyNoteLink(false),
    copyInternalLink: () => void copyNoteLink(true),
    findReplace: () => setIsFindOpen(true),
    runBlockCommand: (command: BlockCommand) => {
      if (editor && !isTrashView) command.run(editor);
    },
    noteInfo: () => setIsNoteInfoOpen(true),
    noteHistory: () => setIsHistoryOpen(true),
    saveAsTemplate: () => void saveAsTemplate(),
    trashNote: () => void trashActiveNote(),
    newTab: () => void openTab(),
    nextTab: cycleTab,
    selectTab: (id) => void selectTab(id),
    closeTab: () => void closeTab(),
    minimize: () => void invoke("minimize_window"),
    openSettings: () => openSettings("general"),
    openAssistant: () => void openWorkspace("assistant"),
    aiItems: aiItems(),
    checkIntegrity: () => openDataDialog("integrity"),
    showShortcuts: () => setIsShortcutsOpen(true),
    showAbout: () => setIsAboutOpen(true),
  };
  const menus = buildMenus(menuContext);
  const menusRef = useRef(menus);
  menusRef.current = menus;

  // Scorciatoie: un solo gestore, guidato dai menu. Le combinazioni già
  // gestite dall'editor (es. Ctrl+E per il codice) non vengono intercettate.
  useEffect(() => {
    if (!isDesktop) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || event.isComposing) return;
      if (document.querySelector(".dialog-backdrop")) return;
      const action = findAction(menusRef.current, event);
      if (!action || action.enabled === false) return;
      event.preventDefault();
      setContextMenu(null);
      void action.run();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const openNoteContextMenu = async (event: ReactMouseEvent | { clientX: number; clientY: number }, note: Note) => {
    if ("preventDefault" in event) event.preventDefault();
    const position = { x: event.clientX, y: event.clientY };
    if (activeNoteRef.current?.id !== note.id) await selectNote(note);
    setContextMenu({ ...position, items: [], label: note.title || t.untitled });
  };

  const noteContextItems: MenuEntry[] = isTrashView
    ? [
        { id: "ctx-restore", label: t.restoreNote, run: () => void restoreActiveNote() },
        {
          id: "ctx-delete",
          label: t.deletePermanently,
          destructive: true,
          run: () => void deleteActiveNotePermanently(),
        },
      ]
    : [
        { id: "ctx-new", label: t.newNote, accelerator: "Ctrl+N", run: () => void createNote() },
        { kind: "separator" },
        ...noteItems(menuContext).slice(0, -4),
        {
          kind: "submenu",
          id: "ctx-export",
          label: t.menuExportNote,
          items: [
            { id: "ctx-md", label: t.exportAsMarkdown, run: () => void exportActiveNote("markdown") },
            { id: "ctx-html", label: t.exportAsHtml, run: () => void exportActiveNote("html") },
            { id: "ctx-txt", label: t.exportAsText, run: () => void exportActiveNote("text") },
          ],
        },
        { id: "ctx-pdf", label: t.menuExportPdf, run: () => void exportActiveNote("pdf") },
        { id: "ctx-print", label: t.menuPrint, accelerator: "Ctrl+P", run: () => void exportActiveNote("pdf") },
        ...noteItems(menuContext).slice(-4),
      ];

  const switchPlaces: SwitchTarget[] = (
    ["home", "tasks", "reminders", "calendar", "files", "templates", "assistant"] as Workspace[]
  ).map((place) => ({
    key: `place-${place}`,
    kind: "place",
    label: workspaceLabels[place],
    run: () => void openWorkspace(place),
  }));

  const filteredNotes = notes;
  const viewTitle = titleForView(selectedView);
  const saveLabel =
    saveState === "saving" ? t.saving : saveState === "error" ? t.saveError : t.saved;

  const organizationDialogTitle = organizationDialog
    ? organizationDialog.kind === "space"
      ? organizationDialog.intent === "create" ? t.createSpaceTitle : t.editSpaceTitle
      : organizationDialog.kind === "notebook"
        ? organizationDialog.intent === "create" ? t.createNotebookTitle : t.editNotebookTitle
        : organizationDialog.intent === "create" ? t.createTagTitle : t.editTagTitle
    : "";

  const renderNotebook = (notebook: Notebook, nested: boolean) => {
    const selected = workspace === "notes" && selectedView.kind === "notebook" && selectedView.id === notebook.id;
    return (
      <div className={`organization-entry${nested ? " nested" : ""}`} key={notebook.id}>
        <button
          aria-current={selected ? "page" : undefined}
          className={`navigation-item organization-select${selected ? " selected" : ""}`}
          onClick={() => void changeView({ kind: "notebook", id: notebook.id })}
          type="button"
        >
          <span className="navigation-icon" aria-hidden="true"><Icon name="notebook" size={17} /></span>
          <span className="organization-name">{notebook.name}</span>
          {preferences.showNoteCounts && (
            <span className="navigation-count">{sidebarCounts?.notebooks[notebook.id] ?? 0}</span>
          )}
        </button>
        <div className="organization-actions">
          <button
            aria-label={`${t.editItem}: ${notebook.name}`}
            onClick={() => void editNotebook(notebook)}
            title={t.editItem}
            type="button"
          >⋯</button>
          <button
            aria-label={`${t.deleteItem}: ${notebook.name}`}
            onClick={() => void deleteNotebook(notebook)}
            title={t.deleteItem}
            type="button"
          >×</button>
        </div>
      </div>
    );
  };

  const isSidebarSectionCollapsed = (section: SidebarSection) =>
    preferences.collapsedSidebarSections.includes(section);
  const toggleSidebarSection = (section: SidebarSection) => {
    const collapsed = isSidebarSectionCollapsed(section);
    updatePreferences({
      collapsedSidebarSections: collapsed
        ? preferences.collapsedSidebarSections.filter((item) => item !== section)
        : [...preferences.collapsedSidebarSections, section],
    });
  };
  const sidebarSectionHeader = (section: SidebarSection, label: string, action?: ReactNode) => {
    const collapsed = isSidebarSectionCollapsed(section);
    const contentId = `sidebar-section-${section}`;
    return (
      <div className="sidebar-section-heading">
        <button
          aria-controls={contentId}
          aria-expanded={!collapsed}
          aria-label={collapsed ? t.expandSidebarSection(label) : t.collapseSidebarSection(label)}
          className="sidebar-section-toggle"
          data-sidebar-section={section}
          onClick={() => toggleSidebarSection(section)}
          title={collapsed ? t.expandSidebarSection(label) : t.collapseSidebarSection(label)}
          type="button"
        >
          <span>{label}</span>
          <span aria-hidden="true" className="sidebar-section-chevron">{collapsed ? "▸" : "▾"}</span>
        </button>
        {action}
      </div>
    );
  };

  return (
    <div className={`app-frame${preferences.sidebarHidden ? " sidebar-hidden" : ""} layout-${layout}`}>
      {isDesktop && <MenuBar label={t.menuBarLabel} menus={menus} />}
      {isDesktop && (
        <div className="tab-bar" role="tablist" aria-label={t.tabsLabel}>
          <button
            aria-label={sidebarVisible ? t.hideSidebar : t.showSidebar}
            aria-pressed={sidebarVisible}
            className={`tab-nav-button sidebar-toggle${sidebarVisible ? "" : " collapsed"}`}
            onClick={toggleSidebar}
            title={`${sidebarVisible ? t.hideSidebar : t.showSidebar} (F10)`}
            type="button"
          >
            {sidebarVisible ? "⟨⟨" : "⟩⟩"}
          </button>
          <button
            aria-label={t.menuBack}
            className="tab-nav-button"
            disabled={!menuContext.canGoBack}
            onClick={() => void goBack()}
            title={t.menuBack}
            type="button"
          >
            ←
          </button>
          <button
            aria-label={t.menuForward}
            className="tab-nav-button"
            disabled={!menuContext.canGoForward}
            onClick={() => void goForward()}
            title={t.menuForward}
            type="button"
          >
            →
          </button>
          {tabs.map((tab) => {
            const label = tab.history[tab.index]?.label || t.homeNav;
            const selected = tab.id === activeTabId;
            return (
              <div className={`tab${selected ? " selected" : ""}`} key={tab.id}>
                <button
                  aria-selected={selected}
                  className="tab-label"
                  onClick={() => void selectTab(tab.id)}
                  role="tab"
                  title={label}
                  type="button"
                >
                  {label}
                </button>
                {tabs.length > 1 && (
                  <button
                    aria-label={`${t.menuCloseTab}: ${label}`}
                    className="tab-close"
                    onClick={() => void closeTab(tab.id)}
                    title={t.menuCloseTab}
                    type="button"
                  >
                    ×
                  </button>
                )}
              </div>
            );
          })}
          <button
            aria-label={t.menuNewTab}
            className="tab-nav-button"
            onClick={() => void openTab()}
            title={t.menuNewTab}
            type="button"
          >
            ＋
          </button>
        </div>
      )}
    <div
      className="app-shell"
      style={
        {
          "--sidebar-width": `${preferences.sidebarWidth}px`,
          "--list-width": `${preferences.listWidth}px`,
          "--sidebar-column": sidebarVisible ? `${preferences.sidebarWidth}px` : "0px",
        } as React.CSSProperties
      }
    >
      <aside className="sidebar" aria-label={t.navigation}>

        <div className="brand">
          <img alt="" className="brand-mark" height={30} src={logoUrl} width={30} />
          <div>
            {/* Il marchio non si traduce: "Great" in grassetto, "note" semigrassetto. */}
            <div className="brand-name">
              <span className="brand-great">Great</span>
              <span className="brand-note">note</span>
            </div>
            <div className="brand-tagline">{t.tagline}</div>
          </div>
        </div>

        {isDesktop && (
          <button className="sidebar-search" onClick={focusSearch} type="button">
            <Icon name="search" size={16} />
            <span className="sidebar-search-label">{t.search}</span>
            <kbd>Ctrl+K</kbd>
          </button>
        )}

        <div className="create-buttons">
          <button className="new-note-button" onClick={() => void createNote()} type="button">
            <Icon name="note-plus" size={18} />
            {t.newNoteShort}
          </button>
          {isDesktop && preferences.showCreateButtons && (
            <>
              <button
                aria-label={t.newTaskAction}
                className="create-icon-button"
                onClick={() => void startNewTask()}
                title={t.newTaskAction}
                type="button"
              >
                <Icon name="check" size={18} />
              </button>
              <button
                aria-label={t.newEvent}
                className="create-icon-button"
                onClick={() => void startNewEvent()}
                title={t.newEvent}
                type="button"
              >
                <Icon name="calendar-plus" size={18} />
              </button>
            </>
          )}
        </div>

        <section className="organization-section sidebar-collapsible-section sidebar-navigation-section" aria-label={t.navigationNotes}>
          {sidebarSectionHeader("navigation", t.notesLabel)}
          <div
            className="sidebar-section-content"
            hidden={isSidebarSectionCollapsed("navigation")}
            id="sidebar-section-navigation"
          >
        <nav className="main-navigation" aria-label={t.navigationNotes}>
          {(() => {
            const count = (value: number | undefined) =>
              preferences.showNoteCounts && value !== undefined ? (
                <span className="navigation-count">{value}</span>
              ) : null;
            const workspaceItem = (target: Workspace, icon: ReactNode, label: string, badge?: ReactNode) => (
              <button
                aria-current={workspace === target ? "page" : undefined}
                className={`navigation-item${workspace === target ? " selected" : ""}`}
                key={target}
                onClick={() => void openWorkspace(target)}
                title={label}
                type="button"
              >
                <span className="navigation-icon" aria-hidden="true">
                  {icon}
                </span>
                {label}
                {badge}
              </button>
            );
            const viewItem = (view: NoteView, icon: ReactNode, label: string, badge?: ReactNode) => {
              const selected = workspace === "notes" && selectedView.kind === view.kind;
              return (
                <button
                  aria-current={selected ? "page" : undefined}
                  className={`navigation-item${selected ? " selected" : ""}`}
                  key={view.kind}
                  onClick={() => void changeView(view)}
                  title={label}
                  type="button"
                >
                  <span className="navigation-icon" aria-hidden="true">
                    {icon}
                  </span>
                  {label}
                  {badge}
                </button>
              );
            };
            const trashIcon = <Icon name="trash" />;
            const secondary = [
              viewItem({ kind: "recent" }, <Icon name="clock" />, t.recentNotes),
              viewItem({ kind: "archived" }, <Icon name="archive" />, t.archivedNotes, count(sidebarCounts?.archived)),
            ];
            return (
              <>
                {isDesktop && workspaceItem("home", <Icon name="home" />, t.homeNav)}
                {viewItem({ kind: "pinned" }, <Icon name="star" />, t.pinnedNotes, count(sidebarCounts?.pinned))}
                {viewItem({ kind: "all" }, <Icon name="notes" />, t.allNotes, count(sidebarCounts?.all))}
                {isDesktop &&
                  workspaceItem(
                    "tasks",
                    <Icon name="check" />,
                    t.tasksNav,
                    urgentTaskCount > 0 ? (
                      <span
                        aria-label={t.taskCountsLabel(taskCounts?.today ?? 0, taskCounts?.overdue ?? 0)}
                        className={`navigation-count${taskCounts?.overdue ? " alert-count" : ""}`}
                      >
                        {urgentTaskCount}
                      </span>
                    ) : null,
                  )}
                {isDesktop &&
                  workspaceItem(
                    "reminders",
                    <Icon name="bell" />,
                    t.remindersNav,
                    firedReminders.length > 0 ? (
                      <span aria-label={t.reminderAlertTitle(firedReminders.length)} className="navigation-count alert-count">
                        {firedReminders.length}
                      </span>
                    ) : null,
                  )}
                {isDesktop && workspaceItem("files", <Icon name="file" />, t.filesNav, count(sidebarCounts?.files))}
                {isDesktop && workspaceItem("calendar", <Icon name="calendar" />, t.calendarNav)}
                {isDesktop && workspaceItem("templates", <Icon name="template" />, t.templatesNav)}
                {isDesktop && workspaceItem("assistant", <Icon name="sparkle" />, t.assistantNav)}
                {preferences.groupInMore ? (
                  <>
                    <button
                      aria-expanded={isMoreOpen}
                      className="navigation-item"
                      onClick={() => setIsMoreOpen((current) => !current)}
                      title={t.moreNav}
                      type="button"
                    >
                      <span className="navigation-icon" aria-hidden="true">
                        <Icon name="more" />
                      </span>
                      {t.moreNav}
                      <span className="navigation-count" aria-hidden="true">
                        {isMoreOpen ? "▴" : "▾"}
                      </span>
                    </button>
                    {(isMoreOpen ||
                      (workspace === "notes" && ["recent", "archived"].includes(selectedView.kind))) && (
                      <div className="navigation-group">{secondary}</div>
                    )}
                  </>
                ) : (
                  secondary
                )}
                {viewItem({ kind: "trash" }, trashIcon, t.trash, count(sidebarCounts?.trash))}
              </>
            );
          })()}
        </nav>
          </div>
        </section>

        {isDesktop && preferences.showRecentNotes && sidebarRecent.length > 0 && (
          <section className="organization-section" aria-label={t.recentNotes}>
            {sidebarSectionHeader("recent", t.recentNotes)}
            <div className="sidebar-section-content" hidden={isSidebarSectionCollapsed("recent")} id="sidebar-section-recent">
              {sidebarRecent.map((note) => (
                <button
                  aria-current={activeNote?.id === note.id ? "true" : undefined}
                  className={`navigation-item organization-select recent-note${activeNote?.id === note.id ? " selected" : ""}`}
                  key={note.id}
                  onClick={() => void openNoteById(note.id)}
                  title={note.title || t.untitled}
                  type="button"
                >
                  <span className="navigation-icon" aria-hidden="true">
                    <Icon name="notes" size={16} />
                  </span>
                  <span className="organization-name">{note.title || t.untitled}</span>
                </button>
              ))}
            </div>
          </section>
        )}

        <section className="organization-section" aria-label={t.spacesLabel}>
          {sidebarSectionHeader("spaces", t.spacesLabel, (
            <button
              aria-label={t.addSpace}
              className="section-add-button"
              onClick={() => void createSpace()}
              title={t.addSpace}
              type="button"
            >
              +
            </button>
          ))}
          <div className="sidebar-section-content" hidden={isSidebarSectionCollapsed("spaces")} id="sidebar-section-spaces">
          {spaces.map((space) => (
            <div key={space.id}>
              <div className="organization-entry">
                <button
                  aria-current={workspace === "notes" && selectedView.kind === "space" && selectedView.id === space.id ? "page" : undefined}
                  className={`navigation-item organization-select${workspace === "notes" && selectedView.kind === "space" && selectedView.id === space.id ? " selected" : ""}`}
                  onClick={() => void changeView({ kind: "space", id: space.id })}
                  type="button"
                >
                  <span className="navigation-icon" aria-hidden="true"><Icon name="space" size={17} /></span>
                  <span className="organization-name">{space.name}</span>
                  {preferences.showNoteCounts && (
                    <span className="navigation-count">{sidebarCounts?.spaces[space.id] ?? 0}</span>
                  )}
                </button>
                <div className="organization-actions">
                  <button
                    aria-label={`${t.addNotebook}: ${space.name}`}
                    onClick={() => void createNotebook(space.id)}
                    title={t.addNotebook}
                    type="button"
                  >+</button>
                  <button
                    aria-label={`${t.editItem}: ${space.name}`}
                    onClick={() => void editSpace(space)}
                    title={t.editItem}
                    type="button"
                  >⋯</button>
                  <button
                    aria-label={`${t.deleteItem}: ${space.name}`}
                    onClick={() => void deleteSpace(space)}
                    title={t.deleteItem}
                    type="button"
                  >×</button>
                </div>
              </div>
              {!preferences.showNotebooksInSpaces &&
                notebooks
                  .filter((notebook) => notebook.space_id === space.id)
                  .map((notebook) => renderNotebook(notebook, true))}
            </div>
          ))}
          </div>
        </section>

        <section className="organization-section" aria-label={t.notebooksLabel}>
          {sidebarSectionHeader("notebooks", t.notebooksLabel, (
            <button
              aria-label={t.addNotebook}
              className="section-add-button"
              onClick={() => void createNotebook()}
              title={t.addNotebook}
              type="button"
            >
              +
            </button>
          ))}
          <div className="sidebar-section-content" hidden={isSidebarSectionCollapsed("notebooks")} id="sidebar-section-notebooks">
          {notebooks
            .filter((notebook) => preferences.showNotebooksInSpaces || !notebook.space_id)
            .map((notebook) => renderNotebook(notebook, false))}
          <button
            aria-current={workspace === "notes" && selectedView.kind === "unfiled" ? "page" : undefined}
            className={`navigation-item organization-select${workspace === "notes" && selectedView.kind === "unfiled" ? " selected" : ""}`}
            onClick={() => void changeView({ kind: "unfiled" })}
            type="button"
          >
            <span className="navigation-icon" aria-hidden="true"><Icon name="inbox" size={17} /></span>
            <span className="organization-name">{t.uncategorized}</span>
            {preferences.showNoteCounts && <span className="navigation-count">{sidebarCounts?.unfiled ?? 0}</span>}
          </button>
          </div>
        </section>

        <section className="organization-section" aria-label={t.tagsSectionLabel}>
          {sidebarSectionHeader("tags", t.tagsSectionLabel, (
            <button
              aria-label={t.addTag}
              className="section-add-button"
              onClick={() => void createTag()}
              title={t.addTag}
              type="button"
            >
              +
            </button>
          ))}
          <div className="sidebar-section-content" hidden={isSidebarSectionCollapsed("tags")} id="sidebar-section-tags">
          {tags.map((tag) => (
            <div className="organization-entry" key={tag.id}>
              <button
                aria-current={workspace === "notes" && selectedView.kind === "tag" && selectedView.id === tag.id ? "page" : undefined}
                className={`navigation-item organization-select${workspace === "notes" && selectedView.kind === "tag" && selectedView.id === tag.id ? " selected" : ""}`}
                onClick={() => void changeView({ kind: "tag", id: tag.id })}
                type="button"
              >
                <span className="navigation-icon" aria-hidden="true"><Icon name="tag" size={17} /></span>
                <span className="organization-name">{tag.name}</span>
                {preferences.showNoteCounts && (
                  <span className="navigation-count">{sidebarCounts?.tags[tag.id] ?? 0}</span>
                )}
              </button>
              <div className="organization-actions">
                <button
                  aria-label={`${t.editItem}: ${tag.name}`}
                  onClick={() => void editTag(tag)}
                  title={t.editItem}
                  type="button"
                >⋯</button>
                <button
                  aria-label={`${t.deleteItem}: ${tag.name}`}
                  onClick={() => void deleteTag(tag)}
                  title={t.deleteItem}
                  type="button"
                >×</button>
              </div>
            </div>
          ))}
          </div>
        </section>

        <div className="sidebar-footer">
          {isDesktop ? (
            <SyncStatusIndicator refreshKey={isSettingsOpen} />
          ) : (
            <>
              <span className="status-dot unavailable" />
              <span>{t.previewMode}</span>
            </>
          )}
          {isDesktop && (
            <button
              aria-label={t.settingsTitle}
              className="text-button sidebar-data-button"
              onClick={() => setIsSettingsOpen(true)}
              title={t.settingsTitle}
              type="button"
            >
              ⚙
            </button>
          )}
        </div>
      </aside>

      {workspace === "home" && (
        <HomeView
          counts={taskCounts}
          onCreateEvent={() => void startNewEvent()}
          onCreateNote={() => void createNote()}
          onCreateTask={() => void startNewTask()}
          onOpenCalendar={() => void openWorkspace("calendar")}
          onOpenNote={(note) => void openNoteById(note.id)}
          onOpenReminders={() => void openWorkspace("reminders")}
          onOpenShortcuts={() => void changeView({ kind: "pinned" })}
          onOpenTasks={() => void openWorkspace("tasks")}
          refreshKey={plannerVersion + noteListVersion}
          reminders={reminders}
        />
      )}
      {workspace === "files" && (
        <FilesView onOpenNote={(noteId) => void openNoteById(noteId)} refreshKey={noteListVersion} />
      )}
      {workspace === "templates" && (
        <TemplatesView
          onChanged={() => void loadTrashItems()}
          onUse={(template) => void useTemplate(template)}
          refreshKey={noteListVersion}
        />
      )}
      {workspace === "tasks" && (
        <TasksView
          counts={taskCounts}
          createRequest={taskCreateRequest}
          initialTask={pendingTask}
          onChanged={() => void refreshPlanner()}
          onOpenNote={(noteId) => void openNoteFromPlanner(noteId)}
          refreshKey={plannerVersion}
        />
      )}
      {workspace === "reminders" && (
        <RemindersView
          onChanged={() => void refreshPlanner()}
          onOpen={(reminder) => void openReminder(reminder)}
          refreshKey={plannerVersion}
          reminders={reminders}
        />
      )}
      {workspace === "calendar" && (
        <CalendarView
          createRequest={eventCreateRequest}
          initialEvent={pendingEvent}
          onChanged={() => void refreshPlanner()}
          onOpenNote={(noteId) => void openNoteFromPlanner(noteId)}
          refreshKey={plannerVersion}
        />
      )}
      {workspace === "assistant" && (
        <AssistantView
          contextNote={assistantNote}
          onCancelIndexing={cancelAiIndexing}
          onClearContextNote={() => setAssistantNote(null)}
          onOpenNote={(noteId) => void openNoteById(noteId)}
          onOpenSource={(target) => void openAiSource(target)}
          onOpenSettings={() => openSettings("ai")}
          onStartIndexing={() => void startAiIndexing()}
          onStatus={setAiStatus}
          progress={aiProgress}
          status={aiStatus}
        />
      )}
      {workspace === "notes" && (
      <>
      <section className="note-list-panel" aria-label={t.allNotes}>

        <div className="list-header">
          <div className="list-heading-row">
            <h1>
              {viewTitle}
              <span className="list-heading-count">{filteredNotes.length}</span>
            </h1>
          </div>
          <div className="list-toolbar">
            <button
              aria-label={t.newNote}
              className="list-tool"
              onClick={() => void createNote()}
              title={t.newNote}
              type="button"
            >
              <Icon name="note-plus" />
            </button>
            <button
              aria-label={preferences.noteListMode === "cards" ? t.listModeList : t.listModeCards}
              aria-pressed={preferences.noteListMode === "cards"}
              className="list-tool"
              onClick={() =>
                updatePreferences({ noteListMode: preferences.noteListMode === "cards" ? "list" : "cards" })
              }
              title={preferences.noteListMode === "cards" ? t.listModeList : t.listModeCards}
              type="button"
            >
              <Icon name={preferences.noteListMode === "cards" ? "list" : "grid"} />
            </button>
          </div>
          <label className="search-field">
            <span aria-hidden="true" className="search-icon">
              <Icon name="search" size={15} />
            </span>
            <input
              ref={searchInput}
              aria-label={t.search}
              onChange={(event) => setSearch(event.target.value)}
              placeholder={t.search}
              type="search"
              value={search}
            />
            {search && (
              <button
                aria-label={t.clearSearch}
                className="clear-search"
                onClick={() => setSearch("")}
                type="button"
              >
                ×
              </button>
            )}
          </label>
          <div className="list-count-row">
            {isTrashView && (filteredNotes.length > 0 || trashItems.length > 0) && (
              <button
                className="text-button destructive-text"
                onClick={() => void emptyTrash()}
                type="button"
              >
                {t.emptyTrashAction}
              </button>
            )}
          </div>
        </div>

        <div className="notes-scroll-area">
          {isTrashView && (
            <TrashItemsPanel items={trashItems} onChanged={() => void handleTrashItemsChanged()} />
          )}
          {isLoading ? (
            <div className="list-message" role="status">
              <span className="loading-spinner" />
              <span>{t.saving}</span>
            </div>
          ) : filteredNotes.length === 0 ? (
            <div className="list-message empty-list">
              <span className="empty-note-icon" aria-hidden="true">
                ✳
              </span>
              <p>
                {isTrashView
                  ? trashItems.length > 0
                    ? t.noTrashedNotes
                    : t.emptyTrash
                  : isArchiveView
                    ? t.noArchiveResults
                    : selectedView.kind === "pinned"
                      ? t.noPinnedResults
                      : selectedView.kind === "recent"
                        ? t.noRecentResults
                    : search
                      ? t.noSearchResults
                      : t.emptyTitle}
              </p>
              {!search && !isTrashView && !isArchiveView && (
                <button className="text-button" onClick={() => void createNote()} type="button">
                  {t.newNote}
                </button>
              )}
            </div>
          ) : (
            <ul className={`note-list mode-${preferences.noteListMode}`}>
              {filteredNotes.map((note) => (
                <li key={note.id}>
                  <button
                    aria-current={activeNote?.id === note.id ? "true" : undefined}
                    aria-haspopup="menu"
                    className={`note-list-item${activeNote?.id === note.id ? " active" : ""}`}
                    onClick={() => void selectNote(note)}
                    onContextMenu={(event) => void openNoteContextMenu(event, note)}
                    onKeyDown={(event) => {
                      if ((event.key === "ContextMenu" || (event.shiftKey && event.key === "F10"))) {
                        event.preventDefault();
                        const rect = event.currentTarget.getBoundingClientRect();
                        void openNoteContextMenu({ clientX: rect.left + 24, clientY: rect.bottom - 8 }, note);
                      } else if (event.key === "Delete" && activeNote?.id === note.id && !isTrashView) {
                        event.preventDefault();
                        void trashActiveNote();
                      }
                    }}
                    type="button"
                  >
                    <span className="note-item-title">
                      {searchTitles[note.id]
                        ? renderSearchSnippet(searchTitles[note.id])
                        : note.title || t.untitled}
                    </span>
                    <span className="note-item-preview">
                      {searchSnippets[note.id]
                        ? renderSearchSnippet(searchSnippets[note.id])
                        : plainText(note.content).trim()}
                    </span>
                    <span className="note-item-date">{formatShortDate(note.updated_at)}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          {hasMore && (
            <button className="load-more-button" onClick={() => void loadMore()} type="button">
              {t.loadMore}
            </button>
          )}
        </div>
      </section>

      <main className="editor-panel">
        {error && (
          <div className="error-banner" role="alert">
            <span>{error}</span>
            {saveState === "error" && activeNote && (
              <button
                className="error-retry"
                onClick={() => void flushDraft(activeNoteRef.current)}
                type="button"
              >
                {t.retry}
              </button>
            )}
          </div>
        )}

        {activeNote ? (
          <>
            <div className="editor-topbar">
              <div className="editor-view-buttons">
                <button
                  aria-label={layout === "normal" ? t.layoutExpanded : t.layoutNormal}
                  aria-pressed={layout !== "normal"}
                  className="topbar-icon"
                  onClick={() => setLayout(layout === "normal" ? "expanded" : "normal")}
                  title={layout === "normal" ? t.layoutExpanded : t.layoutNormal}
                  type="button"
                >
                  <Icon name="collapse" className={layout === "normal" ? undefined : "flipped"} />
                </button>
                <button
                  aria-label={t.layoutFocus}
                  aria-pressed={layout === "focus"}
                  className="topbar-icon"
                  onClick={() => setLayout(layout === "focus" ? "normal" : "focus")}
                  title={t.layoutFocus}
                  type="button"
                >
                  <Icon name="expand" />
                </button>
              </div>
              <nav aria-label={t.breadcrumbLabel} className="breadcrumb">
                <button
                  className="breadcrumb-notebook"
                  disabled={isTrashView}
                  onClick={() => setNotebookPicker("move")}
                  title={t.menuMoveNote}
                  type="button"
                >
                  <Icon name="notebook" size={15} />
                  {notebooks.find((notebook) => notebook.id === activeNote.notebook_id)?.name ?? t.noNotebook}
                </button>
                <Icon className="breadcrumb-separator" name="chevron-right" size={13} />
                <span className="breadcrumb-current">{title || t.untitled}</span>
              </nav>
              <div className="editor-actions">
                {isTrashView ? (
                  <>
                    <button className="subtle-action" onClick={() => void restoreActiveNote()} type="button">
                      {t.restoreNote}
                    </button>
                    <button
                      className="subtle-action destructive-action"
                      onClick={() => void deleteActiveNotePermanently()}
                      type="button"
                    >
                      {t.deletePermanently}
                    </button>
                  </>
                ) : (
                  <>
                    {activeNote.archived_at && <span className="archived-badge">{t.archivedBadge}</span>}
                    <button
                      aria-label={activeNote.is_pinned ? t.unpinNote : t.pinNote}
                      aria-pressed={activeNote.is_pinned}
                      className={`topbar-icon${activeNote.is_pinned ? " is-on" : ""}`}
                      onClick={() => void toggleActiveNotePin()}
                      title={activeNote.is_pinned ? t.unpinNote : t.pinNote}
                      type="button"
                    >
                      <Icon name="star" />
                    </button>
                    <div className="export-button">
                      <Icon name="export" size={16} />
                      <span>{t.exportNote}</span>
                      <select
                        aria-label={t.exportNote}
                        onChange={(event) => {
                          const format = event.target.value as ExportFormat | "pdf" | "";
                          event.target.value = "";
                          if (format) void exportActiveNote(format);
                        }}
                        value=""
                      >
                        <option value="" disabled>
                          {t.exportNote}
                        </option>
                        <option value="markdown">{t.exportAsMarkdown}</option>
                        <option value="html">{t.exportAsHtml}</option>
                        <option value="text">{t.exportAsText}</option>
                        <option value="pdf">{t.exportAsPdf}</option>
                      </select>
                    </div>
                  </>
                )}
                <button
                  aria-haspopup="menu"
                  aria-label={t.noteActions}
                  className="topbar-icon"
                  onClick={(event) => {
                    const rect = event.currentTarget.getBoundingClientRect();
                    setContextMenu({ x: rect.right - 220, y: rect.bottom + 4, items: [], label: t.noteActions });
                  }}
                  title={t.noteActions}
                  type="button"
                >
                  <Icon name="more" />
                </button>
              </div>
            </div>

            {editor && !isTrashView && (
              <div className="editor-toolbar-row">
                <EditorToolbar
                  editor={editor}
                  onAddTask={() => {
                    const panel = document.querySelector<HTMLInputElement>(".note-detail-panel .inline-form input");
                    panel?.scrollIntoView({ block: "center" });
                    panel?.focus();
                  }}
                  aiItems={isDesktop ? aiItems : undefined}
                  onAttach={() => attachmentInput.current?.click()}
                />
              </div>
            )}
            <div className="editor-workspace">
              <div
                className={`editor-document width-${preferences.noteWidth}`}
                onDragOverCapture={(event) => {
                  if (event.dataTransfer.types.includes("Files")) event.preventDefault();
                }}
                onDropCapture={handleAttachmentDrop}
              >
                <h1 className="print-title" aria-hidden="true">
                  {title || t.untitled}
                </h1>
                <input
                  ref={titleInput}
                  aria-label={t.titlePlaceholder}
                  className="title-input"
                  onChange={(event) => updateTitle(event.target.value)}
                  placeholder={t.titlePlaceholder}
                  disabled={isTrashView}
                  value={title}
                />

                {isFindOpen && editor && (
                  <FindReplaceBar editor={editor} onClose={() => setIsFindOpen(false)} readOnly={isTrashView} />
                )}
                <EditorContent editor={editor} />
                {editor && <SlashMenu editor={editor} enabled={preferences.slashCommands && !isTrashView} />}
                <div className="note-details-below">
                {!isTrashView && (
                  <>
                    <section className="note-detail-panel" aria-label={t.attachments}>
                      <div className="note-detail-heading">
                        <h2>{t.attachments}</h2>
                        <button
                          className="text-button"
                          disabled={isUploadingAttachment}
                          onClick={() => attachmentInput.current?.click()}
                          type="button"
                        >
                          {isUploadingAttachment ? t.uploadingAttachment : t.addAttachment}
                        </button>
                      </div>
                      <input
                        ref={attachmentInput}
                        accept=".txt,.md,.csv,.png,.jpg,.jpeg,.gif,.pdf,text/plain,image/png,image/jpeg,image/gif,application/pdf"
                        className="visually-hidden"
                        multiple
                        onChange={(event) => {
                          if (event.target.files) void addAttachmentFiles(event.target.files);
                        }}
                        type="file"
                      />
                      {attachments.length === 0 ? (
                        <p className="detail-empty">{t.noAttachments}</p>
                      ) : (
                        <ul className="attachment-list">
                          {attachments.map((attachment) => (
                            <li key={attachment.id}>
                              <button
                                className="attachment-open-button"
                                onClick={() => void openAttachment(attachment)}
                                title={t.openAttachment}
                                type="button"
                              >
                                <span className="attachment-name">{attachment.original_filename}</span>
                                <span className="attachment-meta">
                                  {attachment.mime_type} · {formatFileSize(attachment.size_bytes)}
                                </span>
                              </button>
                              {attachment.mime_type.startsWith("image/") && (
                                <span className="attachment-image-actions">
                                  <button
                                    className="text-button"
                                    onClick={() => insertImage(attachment)}
                                    type="button"
                                  >
                                    {t.insertImageInText}
                                  </button>
                                  {readingImages.has(attachment.id) ? (
                                    <span className="attachment-ocr-status">{t.imageTextReading}</span>
                                  ) : (
                                    <button
                                      className="text-button"
                                      onClick={() => void readImageText(attachment)}
                                      title={attachment.ocr_error ?? undefined}
                                      type="button"
                                    >
                                      {attachment.ocr_status === "done"
                                        ? t.imageTextReread
                                        : attachment.ocr_status === "failed"
                                          ? t.imageTextRetry
                                          : t.imageTextReadAction}
                                    </button>
                                  )}
                                </span>
                              )}
                              <button
                                aria-label={`${t.removeAttachment}: ${attachment.original_filename}`}
                                className="attachment-remove-button"
                                onClick={() => void removeAttachment(attachment)}
                                title={t.removeAttachment}
                                type="button"
                              >
                                ×
                              </button>
                              {attachment.ocr_status === "done" && (
                                <details className="attachment-ocr-text">
                                  <summary>
                                    {t.imageTextSummary(attachment.ocr_engine ?? "")}
                                  </summary>
                                  <pre>{attachment.ocr_text || t.imageTextEmpty}</pre>
                                </details>
                              )}
                            </li>
                          ))}
                        </ul>
                      )}
                    </section>

                    <section className="note-detail-panel" aria-label={t.noteLinks}>
                      <div className="note-detail-heading">
                        <h2>{t.noteLinks}</h2>
                        <div className="note-link-controls">
                          <select
                            aria-label={t.linkNote}
                            onFocus={() => {
                              if (linkCandidates.length === 0) void loadLinkCandidates();
                            }}
                            onChange={(event) => setSelectedLinkTarget(event.target.value)}
                            value={selectedLinkTarget}
                          >
                            <option value="">{t.chooseLinkTarget}</option>
                            {linkCandidates
                              .filter((candidate) => candidate.id !== activeNote.id)
                              .map((candidate) => (
                                <option key={candidate.id} value={candidate.id}>
                                  {candidate.title || t.untitled}
                                </option>
                              ))}
                          </select>
                          <button
                            className="text-button"
                            disabled={!selectedLinkTarget}
                            onClick={() => void addNoteLink()}
                            type="button"
                          >
                            {t.addNoteLink}
                          </button>
                          <button className="text-button" onClick={() => void pasteNoteLink()} type="button">
                            {t.pasteNoteLink}
                          </button>
                        </div>
                      </div>
                      {noteLinks.length === 0 ? (
                        <p className="detail-empty">{t.noNoteLinks}</p>
                      ) : (
                        <ul className="note-link-list">
                          {noteLinks.map((link) => (
                            <li key={link.target_note_id}>
                              <button
                                className="note-link-open-button"
                                onClick={() => void openLinkedNote(link.target_note_id)}
                                type="button"
                              >
                                {link.label || link.title}
                              </button>
                              <button
                                aria-label={`${t.removeNoteLink}: ${link.title}`}
                                className="attachment-remove-button"
                                onClick={() => void removeNoteLink(link.target_note_id)}
                                title={t.removeNoteLink}
                                type="button"
                              >
                                ×
                              </button>
                            </li>
                          ))}
                        </ul>
                      )}
                    </section>

                    <NotePlanner
                      noteId={activeNote.id}
                      onChanged={() => void refreshPlanner()}
                      onOpenNote={(noteId) => void openNoteFromPlanner(noteId)}
                      refreshKey={plannerVersion}
                    />
                  </>
                )}
                </div>
              </div>
            </div>

            <footer className="editor-footer">
              {!isTrashView && (
                <button
                  aria-label={t.noteReminderShortcut}
                  className="topbar-icon"
                  onClick={() => {
                    const planner = document.querySelector<HTMLElement>(".note-reminders-panel");
                    planner?.scrollIntoView({ behavior: "smooth", block: "center" });
                    planner?.querySelector<HTMLElement>("button, input")?.focus();
                  }}
                  title={t.noteReminderShortcut}
                  type="button"
                >
                  <Icon name="bell-plus" />
                </button>
              )}
              <TagBar
                disabled={isTrashView}
                onChange={setActiveNoteTags}
                onCreate={createTagNamed}
                selected={noteTags}
                tags={tags}
              />
              <span
                className={`save-indicator ${isTrashView ? "saved" : saveState}`}
                role="status"
                title={`${t.createdOn} ${formatDate(activeNote.created_at)} · ${t.updatedOn} ${formatDate(activeNote.updated_at)}`}
              >
                {isTrashView ? t.inTrashStatus : saveLabel}
              </span>
              {syncInfo.enabled && (
                <button
                  className={`sync-indicator${syncInfo.error ? " error" : syncInfo.conflicts > 0 ? " warning" : ""}`}
                  onClick={() => openSettings("sync")}
                  title={syncInfo.error ?? t.syncOpenSettings}
                  type="button"
                >
                  {syncInfo.error
                    ? t.syncIndicatorError
                    : syncInfo.conflicts > 0
                      ? t.syncIndicatorConflicts(syncInfo.conflicts)
                      : syncInfo.at
                        ? t.syncIndicatorAt(formatTime(syncInfo.at))
                        : t.syncIndicatorPending}
                </button>
              )}
            </footer>
          </>
        ) : (
          <div className="welcome-state">
            <div className="welcome-orbit" aria-hidden="true">
              <span className="orbit-ring" />
              <span className="orbit-spark">✳</span>
              <span className="orbit-dot" />
            </div>
            <h2>{isDesktop ? t.selectTitle : t.emptyTitle}</h2>
            <p>{isDesktop ? t.selectBody : t.desktopOnly}</p>
            {isDesktop && !isLoading && notes.length === 0 && (
              <button className="primary-button" onClick={() => void createNote()} type="button">
                <span aria-hidden="true">＋</span>
                {t.newNote}
              </button>
            )}
          </div>
        )}
      </main>
      </>
      )}
        {sidebarVisible && (
          <ColumnResizer
            edge="sidebar"
            defaultValue={SIDEBAR_WIDTH.default}
            label={t.resizeSidebar}
            max={() => {
              const shell = document.querySelector(".app-shell")?.getBoundingClientRect().width ?? 1200;
              const list = workspace === "notes" && layout === "normal" ? preferences.listWidth : 0;
              return Math.min(SIDEBAR_WIDTH.max, shell - list - MIN_EDITOR_WIDTH);
            }}
            min={SIDEBAR_WIDTH.min}
            onCommit={(sidebarWidth) => updatePreferences({ sidebarWidth })}
            value={preferences.sidebarWidth}
            variable="--sidebar-width"
          />
        )}
        {workspace === "notes" && layout === "normal" && (
          <ColumnResizer
            edge="list"
            defaultValue={LIST_WIDTH.default}
            label={t.resizeNoteList}
            max={() => {
              const shell = document.querySelector(".app-shell")?.getBoundingClientRect().width ?? 1200;
              const sidebar = sidebarVisible ? preferences.sidebarWidth : 0;
              return Math.min(LIST_WIDTH.max, shell - sidebar - MIN_EDITOR_WIDTH);
            }}
            min={LIST_WIDTH.min}
            onCommit={(listWidth) => updatePreferences({ listWidth })}
            value={preferences.listWidth}
            variable="--list-width"
          />
        )}
    </div>
      {workspace !== "reminders" && (
        <ReminderAlerts
          onChanged={() => void refreshPlanner()}
          onOpen={(reminder) => void openReminder(reminder)}
          onShowAll={() => void openWorkspace("reminders")}
          reminders={firedReminders}
        />
      )}
      {contextMenu && (
        <ContextMenu
          menu={{ ...contextMenu, items: noteContextItems }}
          onClose={() => setContextMenu(null)}
        />
      )}
      {isSwitcherOpen && (
        <QuickSwitcher
          notebooks={notebooks}
          onClose={() => setIsSwitcherOpen(false)}
          onOpenNote={(note) => void openNoteById(note.id)}
          onOpenNotebook={(id) => void changeView({ kind: "notebook", id })}
          onOpenSpace={(id) => void changeView({ kind: "space", id })}
          onOpenTag={(id) => void changeView({ kind: "tag", id })}
          places={switchPlaces}
          spaces={spaces}
          tags={tags}
        />
      )}
      {isSettingsOpen && (
        <SettingsDialog
          aiProgress={aiProgress}
          aiStatus={aiStatus}
          initialSection={settingsSection}
          notebooks={notebooks}
          onAiStatus={setAiStatus}
          onCancelIndexing={cancelAiIndexing}
          onStartIndexing={() => void startAiIndexing()}
          onChange={updatePreferences}
          onClose={() => {
            setIsSettingsOpen(false);
            void refreshSyncInfo();
          }}
          onOpenData={() => openDataDialog("backup")}
          onOpenNote={(noteId) => void openNoteById(noteId)}
          onShowShortcuts={() => {
            setIsSettingsOpen(false);
            setIsShortcutsOpen(true);
          }}
          preferences={preferences}
        />
      )}
      {aiTransform && aiStatus && (
        <AiResultDialog
          onApply={applyAiResult}
          onClose={() => setAiTransform(null)}
          onStatus={setAiStatus}
          request={aiTransform}
          status={aiStatus}
        />
      )}
      {aiSuggestions && aiStatus && activeNote && (
        <AiSuggestionsDialog
          kind={aiSuggestions}
          noteId={activeNote.id}
          noteTitle={title || activeNote.title}
          onApplied={() => {
            if (aiSuggestions === "tasks") {
              void refreshPlanner();
              return;
            }
            const noteId = activeNote.id;
            void Promise.all([invoke<Tag[]>("list_tags"), invoke<Tag[]>("list_note_tags", { noteId })])
              .then(([allTags, selected]) => {
                setTags(allTags);
                if (activeNoteRef.current?.id === noteId) setNoteTags(selected);
                setNoteListVersion((version) => version + 1);
              })
              .catch((cause: unknown) => setError(errorMessage(cause)));
          }}
          onClose={() => setAiSuggestions(null)}
          onStatus={setAiStatus}
          status={aiStatus}
        />
      )}
      <ImageAnnotationHost />
      {isShortcutsOpen && <ShortcutsDialog menus={menus} onClose={() => setIsShortcutsOpen(false)} />}
      {isAboutOpen && <AboutDialog onClose={() => setIsAboutOpen(false)} />}
      {isNoteInfoOpen && activeNote && (
        <NoteInfoDialog
          noteId={activeNote.id}
          onClose={() => setIsNoteInfoOpen(false)}
          onShowHistory={() => {
            setIsNoteInfoOpen(false);
            setIsHistoryOpen(true);
          }}
        />
      )}
      {isHistoryOpen && activeNote && (
        <NoteHistoryDialog
          aiStatus={aiStatus}
          beforeLoad={historyBeforeLoad}
          onAiStatus={setAiStatus}
          noteId={activeNote.id}
          onClose={() => setIsHistoryOpen(false)}
          onRestored={handleRevisionRestored}
        />
      )}
      {isTagEditorOpen && activeNote && (
        <TagEditorDialog
          noteId={activeNote.id}
          onClose={() => setIsTagEditorOpen(false)}
          onSaved={(selected, allTags) => {
            setNoteTags(selected);
            setTags(allTags);
            setNoteListVersion((version) => version + 1);
          }}
          selected={noteTags}
          tags={tags}
        />
      )}
      {notebookPicker && activeNote && (
        <NotebookPickerDialog
          confirmLabel={notebookPicker === "move" ? t.menuMoveNoteConfirm : t.menuCopyNoteConfirm}
          currentNotebookId={activeNote.notebook_id}
          excludeCurrent={notebookPicker === "move"}
          notebooks={notebooks}
          onClose={() => setNotebookPicker(null)}
          onConfirm={(notebookId) => void moveOrCopyNote(notebookPicker, notebookId)}
          spaces={spaces}
          title={notebookPicker === "move" ? t.menuMoveNote : t.menuCopyNoteTo}
        />
      )}
      <PromptHost />
      <ConfirmHost />
      {notice && (
        <div className="notice-toast" role="status">
          {notice}
        </div>
      )}
      {isDataDialogOpen && (
        <DataDialog
          initialTab={dataDialogTab}
          notebooks={notebooks}
          onClose={() => setIsDataDialogOpen(false)}
          onImported={(notebookId) => void handleImported(notebookId)}
        />
      )}
      {organizationDialog && (
        <div
          className="dialog-backdrop"
          onMouseDown={(event) => {
            if (event.target === event.currentTarget && !isSavingOrganization) {
              setOrganizationDialog(null);
            }
          }}
        >
          <section
            aria-labelledby="organization-dialog-title"
            aria-modal="true"
            className="organization-dialog"
            onKeyDown={(event) => {
              if (event.key === "Escape" && !isSavingOrganization) {
                event.preventDefault();
                setOrganizationDialog(null);
              }
            }}
            role="dialog"
          >
            <header className="organization-dialog-header">
              <div className="dialog-mark" aria-hidden="true">
                {organizationDialog.kind === "space"
                  ? "◇"
                  : organizationDialog.kind === "notebook"
                    ? "▤"
                    : "#"}
              </div>
              <div>
                <h2 id="organization-dialog-title">{organizationDialogTitle}</h2>
                <p>{t.dialogSubtitle}</p>
              </div>
              <button
                aria-label={t.cancel}
                className="dialog-close-button"
                disabled={isSavingOrganization}
                onClick={() => setOrganizationDialog(null)}
                type="button"
              >
                ×
              </button>
            </header>

            <form
              className="organization-dialog-form"
              onSubmit={(event) => void submitOrganizationDialog(event)}
            >
              <label className="dialog-field">
                <span>{t.nameLabel}</span>
                <input
                  ref={organizationNameInput}
                  autoFocus
                  maxLength={organizationDialog.kind === "tag" ? 100 : 200}
                  onChange={(event) => {
                    const value = event.target.value;
                    setOrganizationDialog((current) =>
                      current ? { ...current, name: value } : null,
                    );
                    setOrganizationDialogError(null);
                  }}
                  placeholder={
                    organizationDialog.kind === "space"
                      ? t.spaceNamePrompt
                      : organizationDialog.kind === "notebook"
                        ? t.notebookNamePrompt
                        : t.tagNamePrompt
                  }
                  required
                  value={organizationDialog.name}
                />
              </label>

              {organizationDialog.kind !== "tag" && (
                <label className="dialog-field">
                  <span>
                    {t.descriptionLabel}
                    <span className="field-optional">{t.optionalLabel}</span>
                  </span>
                  <textarea
                    maxLength={4096}
                    onChange={(event) => {
                      const value = event.target.value;
                      setOrganizationDialog((current) =>
                        current && current.kind !== "tag"
                          ? { ...current, description: value }
                          : current,
                      );
                    }}
                    placeholder={
                      organizationDialog.kind === "space"
                        ? t.spaceDescriptionPrompt
                        : t.notebookDescriptionPrompt
                    }
                    rows={3}
                    value={organizationDialog.description}
                  />
                </label>
              )}

              {organizationDialog.kind === "notebook" && (
                <label className="dialog-field">
                  <span>{t.spaceLabel}</span>
                  <select
                    onChange={(event) => {
                      const value = event.target.value || null;
                      setOrganizationDialog((current) =>
                        current?.kind === "notebook"
                          ? { ...current, spaceId: value }
                          : current,
                      );
                    }}
                    value={organizationDialog.spaceId ?? ""}
                  >
                    <option value="">{t.noSpace}</option>
                    {spaces.map((space) => (
                      <option key={space.id} value={space.id}>
                        {space.name}
                      </option>
                    ))}
                  </select>
                </label>
              )}

              {organizationDialogError && (
                <div className="dialog-error" role="alert">
                  {organizationDialogError}
                </div>
              )}

              <footer className="organization-dialog-actions">
                <button
                  className="dialog-cancel-button"
                  disabled={isSavingOrganization}
                  onClick={() => setOrganizationDialog(null)}
                  type="button"
                >
                  {t.cancel}
                </button>
                <button
                  className="dialog-submit-button"
                  disabled={isSavingOrganization}
                  type="submit"
                >
                  {isSavingOrganization ? t.savingChanges : t.save}
                </button>
              </footer>
            </form>
          </section>
        </div>
      )}
    </div>
  );
}
