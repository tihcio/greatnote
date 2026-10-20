import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { t } from "../i18n";
import type { Note, Notebook, SearchResult, Space, Tag } from "../types";
import PlannerDialog from "./PlannerDialog";

export interface SwitchTarget {
  key: string;
  kind: "note" | "notebook" | "space" | "tag" | "place";
  label: string;
  detail?: string;
  run: () => void;
}

interface QuickSwitcherProps {
  notebooks: Notebook[];
  spaces: Space[];
  tags: Tag[];
  /** Sezioni dell'app (Home, Attività…) già pronte da aprire. */
  places: SwitchTarget[];
  onOpenNote: (note: Note) => void;
  onOpenNotebook: (id: string) => void;
  onOpenSpace: (id: string) => void;
  onOpenTag: (id: string) => void;
  onClose: () => void;
}

const MAX_RESULTS = 30;
const kindLabels: Record<SwitchTarget["kind"], () => string> = {
  note: () => t.switchKindNote,
  notebook: () => t.trashKindNotebook,
  space: () => t.trashKindSpace,
  tag: () => t.trashKindTag,
  place: () => t.switchKindPlace,
};

/** "Vai a…": apertura rapida di note, taccuini, spazi, tag e sezioni. */
export default function QuickSwitcher({
  notebooks,
  spaces,
  tags,
  places,
  onOpenNote,
  onOpenNotebook,
  onOpenSpace,
  onOpenTag,
  onClose,
}: QuickSwitcherProps) {
  const [query, setQuery] = useState("");
  const [notes, setNotes] = useState<Note[]>([]);
  const [active, setActive] = useState(0);
  const list = useRef<HTMLUListElement>(null);

  useEffect(() => {
    let cancelled = false;
    const timer = window.setTimeout(() => {
      const trimmed = query.trim();
      // Senza testo: le note recenti; con testo: ricerca nel titolo e nel
      // contenuto (le parole vengono cercate come frase).
      const request = trimmed
        ? invoke<SearchResult[]>("search_notes", {
            query: `"${trimmed.replaceAll('"', "")}"`,
            scope: "all",
            scopeId: null,
            limit: 20,
            offset: 0,
          })
        : invoke<SearchResult[]>("search_notes", {
            query: "",
            scope: "recent",
            scopeId: null,
            limit: 10,
            offset: 0,
          });
      void request
        .then((results) => {
          if (!cancelled) setNotes(results.map((result) => result.note));
        })
        .catch(() => {
          if (!cancelled) setNotes([]);
        });
    }, 120);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [query]);

  const targets = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const matches = (label: string) => !needle || label.toLowerCase().includes(needle);
    const result: SwitchTarget[] = [
      ...notes.map((note) => ({
        key: `note-${note.id}`,
        kind: "note" as const,
        label: note.title || t.untitled,
        detail: notebooks.find((notebook) => notebook.id === note.notebook_id)?.name,
        run: () => onOpenNote(note),
      })),
      ...places.filter((place) => matches(place.label)),
      ...notebooks
        .filter((notebook) => matches(notebook.name))
        .map((notebook) => ({
          key: `notebook-${notebook.id}`,
          kind: "notebook" as const,
          label: notebook.name,
          run: () => onOpenNotebook(notebook.id),
        })),
      ...spaces
        .filter((space) => matches(space.name))
        .map((space) => ({
          key: `space-${space.id}`,
          kind: "space" as const,
          label: space.name,
          run: () => onOpenSpace(space.id),
        })),
      ...tags
        .filter((tag) => matches(tag.name))
        .map((tag) => ({
          key: `tag-${tag.id}`,
          kind: "tag" as const,
          label: `# ${tag.name}`,
          run: () => onOpenTag(tag.id),
        })),
    ];
    return result.slice(0, MAX_RESULTS);
  }, [notebooks, notes, onOpenNote, onOpenNotebook, onOpenSpace, onOpenTag, places, query, spaces, tags]);

  useEffect(() => setActive(0), [query]);
  useEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const choose = (target: SwitchTarget | undefined) => {
    if (!target) return;
    onClose();
    target.run();
  };

  return (
    <PlannerDialog mark="⌕" onClose={onClose} title={t.quickSwitcherTitle}>
      <div className="organization-dialog-form switcher">
        <input
          aria-activedescendant={targets[active] ? `switch-${targets[active].key}` : undefined}
          aria-controls="switcher-results"
          aria-label={t.quickSwitcherPlaceholder}
          className="switcher-input"
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "ArrowDown") {
              event.preventDefault();
              setActive((current) => Math.min(current + 1, targets.length - 1));
            } else if (event.key === "ArrowUp") {
              event.preventDefault();
              setActive((current) => Math.max(current - 1, 0));
            } else if (event.key === "Enter") {
              event.preventDefault();
              choose(targets[active]);
            }
          }}
          placeholder={t.quickSwitcherPlaceholder}
          role="combobox"
          aria-expanded="true"
          value={query}
        />
        <ul className="switcher-results" id="switcher-results" ref={list} role="listbox">
          {targets.length === 0 ? (
            <li className="detail-empty">{t.noMatches}</li>
          ) : (
            targets.map((target, index) => (
              <li
                aria-selected={index === active}
                className={`switcher-item${index === active ? " active" : ""}`}
                data-index={index}
                id={`switch-${target.key}`}
                key={target.key}
                onClick={() => choose(target)}
                onMouseMove={() => setActive(index)}
                role="option"
              >
                <span className="switcher-kind">{kindLabels[target.kind]()}</span>
                <span className="switcher-label">{target.label}</span>
                {target.detail && <small>{target.detail}</small>}
              </li>
            ))
          )}
        </ul>
      </div>
    </PlannerDialog>
  );
}
