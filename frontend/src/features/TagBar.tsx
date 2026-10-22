import { useMemo, useRef, useState } from "react";
import { t } from "../i18n";
import Icon from "../components/Icon";
import { openContextMenu } from "../lib/contextMenu";
import type { Tag } from "../types";

interface TagBarProps {
  tags: Tag[];
  selected: Tag[];
  disabled?: boolean;
  /** Salva l'insieme dei tag della nota. */
  onChange: (next: Tag[]) => Promise<void>;
  /** Crea un nuovo tag con questo nome e lo restituisce. */
  onCreate: (name: string) => Promise<Tag | null>;
  /** Mostra le note con il tag (menu contestuale dell'etichetta). */
  onOpen?: (tag: Tag) => void;
}

const MAX_SUGGESTIONS = 8;

/**
 * Tag della nota nella barra in basso: solo quelli assegnati, come etichette
 * rimovibili, e un campo "Aggiungi tag" con suggerimenti dai tag esistenti.
 * Invio aggiunge (o crea) il tag, Backspace sul campo vuoto toglie l'ultimo.
 */
export default function TagBar({ tags, selected, disabled, onChange, onCreate, onOpen }: TagBarProps) {
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [highlighted, setHighlighted] = useState(0);
  const [busy, setBusy] = useState(false);
  const input = useRef<HTMLInputElement>(null);

  const trimmed = query.trim();
  const suggestions = useMemo(() => {
    const lower = trimmed.toLocaleLowerCase();
    const chosen = new Set(selected.map((tag) => tag.id));
    return tags
      .filter((tag) => !chosen.has(tag.id) && tag.name.toLocaleLowerCase().includes(lower))
      .sort((a, b) => {
        const aStarts = a.name.toLocaleLowerCase().startsWith(lower) ? 0 : 1;
        const bStarts = b.name.toLocaleLowerCase().startsWith(lower) ? 0 : 1;
        return aStarts - bStarts || a.name.localeCompare(b.name);
      })
      .slice(0, MAX_SUGGESTIONS);
  }, [selected, tags, trimmed]);
  const exact = tags.find((tag) => tag.name.toLocaleLowerCase() === trimmed.toLocaleLowerCase());
  const canCreate = trimmed.length > 0 && !exact;
  const options = [
    ...suggestions.map((tag) => ({ key: tag.id, label: tag.name, tag })),
    ...(canCreate ? [{ key: "__create", label: t.createTagNamed(trimmed), tag: null }] : []),
  ];

  const add = async (tag: Tag | null) => {
    if (busy) return;
    setBusy(true);
    try {
      const target = tag ?? exact ?? (trimmed ? await onCreate(trimmed) : null);
      if (target && !selected.some((item) => item.id === target.id)) {
        await onChange([...selected, target]);
      }
      setQuery("");
      setHighlighted(0);
    } finally {
      setBusy(false);
      input.current?.focus();
    }
  };

  const remove = (tag: Tag) => void onChange(selected.filter((item) => item.id !== tag.id));

  return (
    <div className="tag-bar" aria-label={t.noteTags} role="group">
      <Icon className="tag-bar-icon" name="tag" size={17} />
      {selected.map((tag) => (
        <span
          className="tag-chip"
          key={tag.id}
          onContextMenu={(event) =>
            openContextMenu(event, tag.name, [
              ...(onOpen ? [{ id: "ctx-open-tag", label: t.showNotesWithTag, run: () => onOpen(tag) }] : []),
              { id: "ctx-remove-tag", label: t.removeTagNamed(tag.name), enabled: !disabled, run: () => remove(tag) },
            ])
          }
        >
          {tag.name}
          {!disabled && (
            <button
              aria-label={t.removeTagNamed(tag.name)}
              className="tag-chip-remove"
              onClick={() => remove(tag)}
              title={t.removeTagNamed(tag.name)}
              type="button"
            >
              <Icon name="x" size={12} />
            </button>
          )}
        </span>
      ))}
      {!disabled && (
        <div className="tag-input-wrap">
          <input
            aria-autocomplete="list"
            aria-controls="tag-suggestions"
            aria-expanded={open && options.length > 0}
            aria-label={t.tagBarPlaceholder}
            className="tag-input"
            maxLength={100}
            onBlur={() => window.setTimeout(() => setOpen(false), 120)}
            onChange={(event) => {
              setQuery(event.target.value);
              setOpen(true);
              setHighlighted(0);
            }}
            onFocus={() => setOpen(true)}
            onKeyDown={(event) => {
              if (event.key === "ArrowDown") {
                event.preventDefault();
                setOpen(true);
                setHighlighted((index) => Math.min(index + 1, options.length - 1));
              } else if (event.key === "ArrowUp") {
                event.preventDefault();
                setHighlighted((index) => Math.max(index - 1, 0));
              } else if (event.key === "Enter") {
                event.preventDefault();
                const option = options[highlighted];
                if (option) void add(option.tag);
                else if (trimmed) void add(null);
              } else if (event.key === "Escape") {
                if (open) {
                  event.stopPropagation();
                  setOpen(false);
                }
              } else if (event.key === "Backspace" && query === "" && selected.length > 0) {
                remove(selected[selected.length - 1]);
              }
            }}
            placeholder={t.tagBarPlaceholder}
            ref={input}
            role="combobox"
            value={query}
          />
          {open && options.length > 0 && (
            <ul className="tag-suggestions" id="tag-suggestions" role="listbox">
              {options.map((option, index) => (
                <li
                  aria-selected={index === highlighted}
                  className={`tag-suggestion${index === highlighted ? " highlighted" : ""}${option.tag ? "" : " create"}`}
                  key={option.key}
                  onMouseDown={(event) => {
                    event.preventDefault();
                    void add(option.tag);
                  }}
                  onMouseEnter={() => setHighlighted(index)}
                  role="option"
                >
                  {option.tag ? <Icon name="tag" size={14} /> : <Icon name="plus" size={14} />}
                  {option.label}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  );
}
