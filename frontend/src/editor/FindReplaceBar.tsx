import { useCallback, useEffect, useRef, useState } from "react";
import type { Editor } from "@tiptap/react";
import { t } from "../i18n";
import { setSearchHighlights } from "./searchHighlight";

interface Match {
  from: number;
  to: number;
}

/**
 * Posizioni delle occorrenze (senza distinzione di maiuscole) nei nodi di
 * testo. Un'occorrenza divisa fra stili diversi non viene trovata.
 */
export function findMatches(editor: Editor, query: string): Match[] {
  if (!query) return [];
  const needle = query.toLocaleLowerCase();
  const matches: Match[] = [];
  editor.state.doc.descendants((node, position) => {
    if (!node.isText || !node.text) return;
    const haystack = node.text.toLocaleLowerCase();
    let index = haystack.indexOf(needle);
    while (index >= 0) {
      matches.push({ from: position + index, to: position + index + query.length });
      index = haystack.indexOf(needle, index + Math.max(needle.length, 1));
    }
  });
  return matches;
}

interface FindReplaceBarProps {
  editor: Editor;
  readOnly: boolean;
  onClose: () => void;
}

/** Trova e sostituisci nella nota (Ctrl+H). */
export default function FindReplaceBar({ editor, readOnly, onClose }: FindReplaceBarProps) {
  const [query, setQuery] = useState("");
  const [replacement, setReplacement] = useState("");
  const [current, setCurrent] = useState(0);
  const [matches, setMatches] = useState<Match[]>([]);
  const input = useRef<HTMLInputElement>(null);

  useEffect(() => {
    input.current?.focus();
    const selected = editor.state.doc.textBetween(editor.state.selection.from, editor.state.selection.to);
    if (selected && !selected.includes("\n")) setQuery(selected);
  }, [editor]);

  const recompute = useCallback(() => setMatches(findMatches(editor, query)), [editor, query]);
  useEffect(() => {
    recompute();
    setCurrent(0);
    editor.on("update", recompute);
    return () => {
      editor.off("update", recompute);
    };
  }, [editor, recompute]);

  useEffect(() => {
    setSearchHighlights(editor, matches, current);
  }, [current, editor, matches]);
  useEffect(() => () => setSearchHighlights(editor, [], 0), [editor]);

  const select = (index: number) => {
    const match = matches[index];
    if (!match) return;
    setCurrent(index);
    editor.chain().setTextSelection(match).scrollIntoView().run();
  };

  const step = (direction: 1 | -1) => {
    if (matches.length === 0) return;
    select((current + direction + matches.length) % matches.length);
  };

  const replaceCurrent = () => {
    const match = matches[current];
    if (!match || readOnly) return;
    const transaction = editor.state.tr;
    if (replacement) transaction.insertText(replacement, match.from, match.to);
    else transaction.delete(match.from, match.to);
    editor.view.dispatch(transaction);
    // Le posizioni cambiano: la prossima occorrenza diventa quella corrente.
    const next = findMatches(editor, query);
    setMatches(next);
    if (next.length > 0) {
      const index = Math.min(current, next.length - 1);
      setCurrent(index);
      editor.chain().setTextSelection(next[index]).scrollIntoView().run();
    }
  };

  const replaceAll = () => {
    if (readOnly || matches.length === 0) return;
    const transaction = editor.state.tr;
    // Dall'ultima alla prima, così le posizioni precedenti restano valide.
    for (const match of [...matches].reverse()) {
      if (replacement) transaction.insertText(replacement, match.from, match.to);
      else transaction.delete(match.from, match.to);
    }
    editor.view.dispatch(transaction);
    setMatches(findMatches(editor, query));
  };

  return (
    <div
      aria-label={t.findReplace}
      className="find-replace-bar"
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          onClose();
          editor.commands.focus();
        }
      }}
      role="search"
    >
      <input
        aria-label={t.findLabel}
        onChange={(event) => setQuery(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            step(event.shiftKey ? -1 : 1);
          }
        }}
        placeholder={t.findLabel}
        ref={input}
        value={query}
      />
      <span aria-live="polite" className="find-count">
        {query ? t.matchCount(matches.length === 0 ? 0 : current + 1, matches.length) : ""}
      </span>
      <button aria-label={t.findPrevious} className="toolbar-button" disabled={matches.length === 0} onClick={() => step(-1)} title={t.findPrevious} type="button">
        ↑
      </button>
      <button aria-label={t.findNext} className="toolbar-button" disabled={matches.length === 0} onClick={() => step(1)} title={t.findNext} type="button">
        ↓
      </button>
      {!readOnly && (
        <>
          <input
            aria-label={t.replaceLabel}
            onChange={(event) => setReplacement(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") {
                event.preventDefault();
                replaceCurrent();
              }
            }}
            placeholder={t.replaceLabel}
            value={replacement}
          />
          <button className="subtle-action" disabled={matches.length === 0} onClick={replaceCurrent} type="button">
            {t.replaceOne}
          </button>
          <button className="subtle-action" disabled={matches.length === 0} onClick={replaceAll} type="button">
            {t.replaceAll}
          </button>
        </>
      )}
      <button aria-label={t.close} className="toolbar-button" onClick={onClose} title={t.close} type="button">
        ×
      </button>
    </div>
  );
}
