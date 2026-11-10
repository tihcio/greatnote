import { useEffect, useMemo, useRef, useState } from "react";
import type { ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import Icon from "../components/Icon";
import type { IconName } from "../components/Icon";
import { t } from "../i18n";
import type { Note, Notebook, SearchResult, Space, Tag } from "../types";

export interface SwitchTarget {
  key: string;
  kind: "note" | "notebook" | "space" | "tag" | "place";
  label: string;
  /** Titolo con i marcatori di evidenziazione della ricerca, se presente. */
  highlighted?: string;
  detail?: string;
  snippet?: string;
  icon?: IconName;
  run: () => void;
}

/** Ambito "qui": il taccuino, lo spazio o il tag aperto in questo momento. */
export interface SwitchScope {
  kind: "notebook" | "space" | "tag";
  id: string;
  label: string;
}

interface QuickSwitcherProps {
  notebooks: Notebook[];
  spaces: Space[];
  tags: Tag[];
  /** Sezioni dell'app (Home, Attività…) già pronte da aprire. */
  places: SwitchTarget[];
  scope: SwitchScope | null;
  onOpenNote: (note: Note) => void;
  onOpenNotebook: (id: string) => void;
  onOpenSpace: (id: string) => void;
  onOpenTag: (id: string) => void;
  onClose: () => void;
}

const MAX_RESULTS = 40;
const kindIcons: Record<SwitchTarget["kind"], IconName> = {
  note: "notes",
  notebook: "notebook",
  space: "space",
  tag: "tag",
  place: "home",
};

/** I risultati di ricerca racchiudono i termini trovati tra U+E000 e U+E001. */
function renderHighlighted(text: string): ReactNode {
  const parts = text.split(/([][^]*[])/g);
  return parts.map((part, index) =>
    part.startsWith("") && part.endsWith("") ? (
      <mark className="search-hit" key={index}>
        {part.slice(1, -1)}
      </mark>
    ) : (
      part
    ),
  );
}

/** Riquadro di ricerca: note (titolo e contenuto), taccuini, spazi, tag e
 * sezioni, in un pannello sovrapposto alla finestra. */
export default function QuickSwitcher({
  notebooks,
  spaces,
  tags,
  places,
  scope,
  onOpenNote,
  onOpenNotebook,
  onOpenSpace,
  onOpenTag,
  onClose,
}: QuickSwitcherProps) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [failed, setFailed] = useState(false);
  const [active, setActive] = useState(0);
  const [here, setHere] = useState(false);
  const input = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLUListElement>(null);
  const limited = here && scope !== null;

  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    input.current?.focus();
    return () => previous?.focus();
  }, []);

  useEffect(() => {
    let cancelled = false;
    const timer = window.setTimeout(() => {
      const trimmed = query.trim();
      // Senza testo: le note recenti (o quelle dell'ambito scelto); con testo:
      // ricerca nel titolo e nel contenuto (le parole sono cercate come frase).
      void invoke<SearchResult[]>("search_notes", {
        query: trimmed ? `"${trimmed.replaceAll('"', "")}"` : "",
        scope: limited && scope ? scope.kind : trimmed ? "all" : "recent",
        scopeId: limited && scope ? scope.id : null,
        limit: trimmed ? 20 : 10,
        offset: 0,
      })
        .then((found) => {
          if (cancelled) return;
          setResults(found);
          setFailed(false);
        })
        .catch(() => {
          if (cancelled) return;
          setResults([]);
          setFailed(true);
        });
    }, 120);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [query, limited, scope]);

  const needle = query.trim().toLowerCase();

  const noteTargets = useMemo<SwitchTarget[]>(
    () =>
      results.map(({ note, highlighted_title, snippet }) => ({
        key: `note-${note.id}`,
        kind: "note",
        label: note.title || t.untitled,
        highlighted: needle && highlighted_title ? highlighted_title : undefined,
        snippet: needle && snippet ? snippet : undefined,
        detail: notebooks.find((notebook) => notebook.id === note.notebook_id)?.name,
        run: () => onOpenNote(note),
      })),
    [needle, notebooks, onOpenNote, results],
  );

  const otherTargets = useMemo<SwitchTarget[]>(() => {
    // Con un ambito ristretto si cercano solo le note di quel contenitore.
    if (limited) return [];
    const matches = (label: string) => !needle || label.toLowerCase().includes(needle);
    return [
      // Le sezioni sono sempre nella riga in alto: nell'elenco solo se cercate.
      ...(needle ? places.filter((place) => matches(place.label)) : []),
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
          label: tag.name,
          run: () => onOpenTag(tag.id),
        })),
    ].slice(0, Math.max(0, MAX_RESULTS - noteTargets.length));
  }, [limited, needle, noteTargets.length, notebooks, onOpenNotebook, onOpenSpace, onOpenTag, places, spaces, tags]);

  const targets = useMemo(() => [...noteTargets, ...otherTargets], [noteTargets, otherTargets]);

  useEffect(() => setActive(0), [query, limited]);
  useEffect(() => {
    list.current?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  const choose = (target: SwitchTarget | undefined) => {
    if (!target) return;
    onClose();
    target.run();
  };

  const renderTarget = (target: SwitchTarget, index: number) => (
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
      <span aria-hidden="true" className="switcher-icon">
        <Icon name={target.icon ?? kindIcons[target.kind]} size={16} />
      </span>
      <span className="switcher-text">
        <span className="switcher-label">
          {target.highlighted ? renderHighlighted(target.highlighted) : target.label}
        </span>
        {target.snippet && <span className="switcher-snippet">{renderHighlighted(target.snippet)}</span>}
      </span>
      {target.detail && (
        <small>
          <Icon name="notebook" size={13} />
          {target.detail}
        </small>
      )}
    </li>
  );

  return (
    <div
      className="dialog-backdrop switcher-backdrop"
      onMouseDown={(event) => {
        if (event.target === event.currentTarget) onClose();
      }}
    >
      <section
        aria-label={t.quickSwitcherTitle}
        aria-modal="true"
        className="switcher"
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            event.stopPropagation();
            onClose();
          }
        }}
        role="dialog"
      >
        <label className="switcher-field">
          <Icon name="search" size={18} />
          <input
            aria-activedescendant={targets[active] ? `switch-${targets[active].key}` : undefined}
            aria-controls="switcher-results"
            aria-expanded="true"
            aria-label={t.quickSwitcherPlaceholder}
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
            ref={input}
            role="combobox"
            value={query}
          />
        </label>

        <nav aria-label={t.switcherSections} className="switcher-places">
          {places.map((place) => (
            <button key={place.key} onClick={() => choose(place)} type="button">
              <Icon name={place.icon ?? kindIcons.place} size={14} />
              {place.label}
            </button>
          ))}
        </nav>

        {scope && (
          <div className="switcher-scope">
            <span id="switcher-scope-label">{t.switcherSearchIn}</span>
            <div aria-labelledby="switcher-scope-label" role="group">
              <button aria-pressed={!limited} onClick={() => setHere(false)} type="button">
                {t.switcherEverywhere}
              </button>
              <button aria-pressed={limited} onClick={() => setHere(true)} type="button">
                <Icon name={kindIcons[scope.kind]} size={14} />
                <span>{scope.label}</span>
              </button>
            </div>
          </div>
        )}

        <ul className="switcher-results" id="switcher-results" ref={list} role="listbox">
          {targets.length === 0 ? (
            <li className="detail-empty" role="presentation">
              {failed ? t.switcherSearchFailed : t.noMatches}
            </li>
          ) : (
            <>
              {noteTargets.length > 0 && (
                <li className="switcher-heading" role="presentation">
                  {needle || limited ? t.switcherNotes : t.switcherRecent}
                </li>
              )}
              {noteTargets.map((target, index) => renderTarget(target, index))}
              {otherTargets.length > 0 && (
                <li className="switcher-heading" role="presentation">
                  {t.quickSwitcherTitle}
                </li>
              )}
              {otherTargets.map((target, index) => renderTarget(target, noteTargets.length + index))}
            </>
          )}
        </ul>

        <footer className="switcher-hints">
          <span>
            <kbd>↑</kbd>
            <kbd>↓</kbd> {t.switcherHintSelect}
          </span>
          <span>
            <kbd>↵</kbd> {t.switcherHintOpen}
          </span>
          <span>
            <kbd>Esc</kbd> {t.close}
          </span>
        </footer>
      </section>
    </div>
  );
}
