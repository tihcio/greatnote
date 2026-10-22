import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { EditorContent, useEditor } from "@tiptap/react";
import type { JSONContent } from "@tiptap/react";
import { t } from "../i18n";
import { usePreferences } from "../app/preferences";
import EditorToolbar from "../editor/EditorToolbar";
import { openEditorContextMenu } from "../editor/editorContextMenu";
import { editorExtensions } from "../editor/extensions";
import FindReplaceBar from "../editor/FindReplaceBar";
import { sanitizeDocument } from "../editor/sanitize";
import SlashMenu from "../editor/SlashMenu";
import { errorMessage } from "../lib/format";
import type { Note, NoteDraft, SaveState } from "../types";
import ConfirmHost from "./ConfirmHost";
import { ContextMenuHost } from "./ContextMenu";
import EncryptedTextHost from "./EncryptedTextDialog";
import ImageAnnotationHost from "./ImageAnnotator";
import PromptHost from "./PromptHost";

export const NOTE_CHANGED_EVENT = "note-changed";

/**
 * Finestra dedicata a una nota ("Apri in una nuova finestra"): titolo,
 * editor e salvataggio automatico. La finestra principale chiude la nota
 * quando la apre qui, così non viene modificata in due punti.
 */
export default function NoteWindow({ noteId }: { noteId: string }) {
  const { preferences } = usePreferences(true);
  const [note, setNote] = useState<Note | null>(null);
  const [title, setTitle] = useState("");
  const [saveState, setSaveState] = useState<SaveState>("saved");
  const [error, setError] = useState<string | null>(null);
  const [isFindOpen, setIsFindOpen] = useState(false);
  const draftRef = useRef<{ title: string; content: JSONContent } | null>(null);
  const dirtyRef = useRef(false);
  const timerRef = useRef<number | null>(null);
  const chainRef = useRef<Promise<boolean>>(Promise.resolve(true));
  const closingRef = useRef(false);
  /** Versione della nota su cui lavora questa finestra (ADR 0017). */
  const baseRef = useRef<string | null>(null);

  const flush = useCallback(async (): Promise<boolean> => {
    if (timerRef.current) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    const operation = chainRef.current.then(async () => {
      const draft = draftRef.current;
      if (!dirtyRef.current || !draft) return true;
      dirtyRef.current = false;
      setSaveState("saving");
      const content = sanitizeDocument(draft.content);
      try {
        await invoke<NoteDraft>("save_note_draft", { noteId, title: draft.title, content });
        const saved = await invoke<Note>("update_note", {
          id: noteId,
          title: draft.title,
          content,
          baseUpdatedAt: baseRef.current,
        });
        baseRef.current = saved.updated_at;
        setSaveState("saved");
        setError(null);
        void emit(NOTE_CHANGED_EVENT, { id: noteId });
        return true;
      } catch (cause) {
        dirtyRef.current = true;
        setSaveState("error");
        setError(errorMessage(cause));
        return false;
      }
    });
    chainRef.current = operation;
    return operation;
  }, [noteId]);

  const schedule = useCallback(() => {
    dirtyRef.current = true;
    setSaveState("saving");
    if (timerRef.current) window.clearTimeout(timerRef.current);
    timerRef.current = window.setTimeout(() => void flush(), 650);
  }, [flush]);

  const editor = useEditor({
    extensions: editorExtensions,
    content: { type: "doc", content: [{ type: "paragraph" }] },
    editorProps: {
      attributes: {
        class: "note-editor",
        "aria-label": t.editorContent,
        "data-placeholder": t.editorPlaceholder,
      },
    },
    onUpdate: ({ editor: current }) => {
      if (!draftRef.current) return;
      draftRef.current = { ...draftRef.current, content: current.getJSON() };
      schedule();
    },
  });

  useEffect(() => {
    if (!editor) return;
    void invoke<Note>("get_note", { noteId })
      .then((loaded) => {
        setNote(loaded);
        baseRef.current = loaded.updated_at;
        setTitle(loaded.title);
        draftRef.current = { title: loaded.title, content: loaded.content };
        editor.commands.setContent(loaded.content, { emitUpdate: false });
        void getCurrentWindow().setTitle(loaded.title || t.untitled);
      })
      .catch((cause: unknown) => setError(errorMessage(cause)));
  }, [editor, noteId]);

  // Alla chiusura le modifiche vengono salvate prima di chiudere.
  useEffect(() => {
    const appWindow = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    void appWindow
      .onCloseRequested(async (event) => {
        if (closingRef.current) return;
        event.preventDefault();
        if (await flush()) {
          closingRef.current = true;
          try {
            // In Tauri 2 la chiusura effettiva è `destroy()`: chiamarla subito
            // evita una seconda richiesta di chiusura.
            await appWindow.destroy();
          } catch (cause) {
            closingRef.current = false;
            setError(errorMessage(cause));
          }
        }
      })
      .then((stop) => {
        unlisten = stop;
      });
    return () => unlisten?.();
  }, [flush]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      const modifier = event.ctrlKey || event.metaKey;
      if (!modifier || event.altKey) return;
      const key = event.key.toLowerCase();
      if (key === "s") {
        event.preventDefault();
        void flush();
      } else if (key === "h" || key === "f") {
        event.preventDefault();
        setIsFindOpen(true);
      } else if (key === "w") {
        event.preventDefault();
        void getCurrentWindow().close();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [flush]);

  const saveLabel = saveState === "saving" ? t.saving : saveState === "error" ? t.saveError : t.saved;

  return (
    <div className="note-window">
      {error && (
        <div className="error-banner" role="alert">
          <span>{error}</span>
          {saveState === "error" && (
            <button className="error-retry" onClick={() => void flush()} type="button">
              {t.retry}
            </button>
          )}
        </div>
      )}
      <div className="editor-workspace">
        <div className={`editor-document width-${preferences.noteWidth}`}>
          <div className="note-window-status">
            <div className={`save-indicator ${saveState}`} role="status">
              <span className="save-dot" />
              {saveLabel}
            </div>
          </div>
          <input
            aria-label={t.titlePlaceholder}
            className="title-input"
            disabled={!note}
            onChange={(event) => {
              setTitle(event.target.value);
              if (draftRef.current) {
                draftRef.current = { ...draftRef.current, title: event.target.value };
                schedule();
                void getCurrentWindow().setTitle(event.target.value || t.untitled);
              }
            }}
            placeholder={t.titlePlaceholder}
            value={title}
          />
          {editor && note && <EditorToolbar editor={editor} onAddTask={() => undefined} onAttach={() => undefined} />}
          {isFindOpen && editor && (
            <FindReplaceBar editor={editor} onClose={() => setIsFindOpen(false)} readOnly={false} />
          )}
          <EditorContent
            editor={editor}
            onContextMenu={(event) => {
              if (editor) openEditorContextMenu(event, editor, { findReplace: () => setIsFindOpen(true), onError: setError });
            }}
          />
          {editor && <SlashMenu editor={editor} enabled={preferences.slashCommands} />}
        </div>
      </div>
      <ImageAnnotationHost />
      <EncryptedTextHost flush={flush} noteId={noteId} />
      <PromptHost />
      <ConfirmHost />
      <ContextMenuHost />
    </div>
  );
}
