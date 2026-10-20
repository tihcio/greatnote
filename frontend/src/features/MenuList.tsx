import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { CSSProperties, KeyboardEvent as ReactKeyboardEvent } from "react";
import { displayAccelerator } from "../app/menu";
import type { MenuAction, MenuEntry } from "../app/menu";

interface MenuListProps {
  items: MenuEntry[];
  /** Esegue l'azione dopo aver chiuso i menu. */
  onRun: (action: MenuAction) => void;
  onClose: () => void;
  /** Freccia sinistra in un sottomenu: torna al menu padre. */
  onCollapse?: () => void;
  /** Freccia destra/sinistra nel menu principale: menu adiacente. */
  onNavigate?: (direction: 1 | -1) => void;
  style?: CSSProperties;
  labelledBy?: string;
  label?: string;
  autoFocus?: boolean;
}

function focusable(entry: MenuEntry): boolean {
  return entry.kind !== "separator" && entry.enabled !== false;
}

/** Elenco di voci accessibile (`role="menu"`) con sottomenu annidati. */
export default function MenuList({
  items,
  onRun,
  onClose,
  onCollapse,
  onNavigate,
  style,
  labelledBy,
  label,
  autoFocus = true,
}: MenuListProps) {
  const list = useRef<HTMLUListElement>(null);
  const [openSubmenu, setOpenSubmenu] = useState<number | null>(null);
  const [position, setPosition] = useState<CSSProperties | undefined>(style);

  // Tiene il menu dentro la finestra (menu contestuali vicino ai bordi).
  useLayoutEffect(() => {
    const element = list.current;
    if (!element || !style || style.position !== "fixed") {
      setPosition(style);
      return;
    }
    const rect = element.getBoundingClientRect();
    const left = Math.min(Number(style.left ?? 0), window.innerWidth - rect.width - 8);
    const top = Math.min(Number(style.top ?? 0), window.innerHeight - rect.height - 8);
    setPosition({ ...style, left: Math.max(8, left), top: Math.max(8, top) });
  }, [style]);

  useEffect(() => {
    if (!autoFocus) return;
    const first = items.findIndex(focusable);
    if (first >= 0) {
      list.current?.querySelectorAll<HTMLElement>(":scope > li > [role^='menuitem']")[first]?.focus();
    }
  }, [autoFocus, items]);

  const buttons = () =>
    Array.from(list.current?.querySelectorAll<HTMLElement>(":scope > li > [role^='menuitem']") ?? []);

  const moveFocus = (current: number, direction: 1 | -1) => {
    const elements = buttons();
    for (let step = 1; step <= items.length; step += 1) {
      const index = (current + direction * step + items.length) % items.length;
      if (focusable(items[index])) {
        elements[index]?.focus();
        return;
      }
    }
  };

  const onKeyDown = (event: ReactKeyboardEvent, index: number) => {
    const entry = items[index];
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        moveFocus(index, 1);
        break;
      case "ArrowUp":
        event.preventDefault();
        moveFocus(index, -1);
        break;
      case "Home":
        event.preventDefault();
        moveFocus(-1, 1);
        break;
      case "End":
        event.preventDefault();
        moveFocus(items.length, -1);
        break;
      case "ArrowRight":
        event.preventDefault();
        if (entry.kind === "submenu" && entry.enabled !== false) setOpenSubmenu(index);
        else onNavigate?.(1);
        break;
      case "ArrowLeft":
        event.preventDefault();
        if (onCollapse) onCollapse();
        else onNavigate?.(-1);
        break;
      case "Escape":
        event.preventDefault();
        event.stopPropagation();
        if (onCollapse) onCollapse();
        else onClose();
        break;
      case "Tab":
        event.preventDefault();
        onClose();
        break;
      default:
        break;
    }
  };

  return (
    <ul
      aria-label={label}
      aria-labelledby={labelledBy}
      className="menu-list"
      onMouseDown={(event) => event.preventDefault()}
      ref={list}
      role="menu"
      style={position}
    >
      {items.map((entry, index) => {
        if (entry.kind === "separator") {
          return <li className="menu-separator" key={`separator-${index}`} role="separator" />;
        }
        const disabled = entry.enabled === false;
        if (entry.kind === "submenu") {
          const open = openSubmenu === index && !disabled;
          return (
            <li
              className="menu-entry"
              key={entry.id}
              onMouseEnter={() => !disabled && setOpenSubmenu(index)}
              onMouseLeave={() => setOpenSubmenu((current) => (current === index ? null : current))}
              role="none"
            >
              <button
                aria-disabled={disabled}
                aria-expanded={open}
                aria-haspopup="menu"
                className="menu-item"
                onClick={() => !disabled && setOpenSubmenu(index)}
                onKeyDown={(event) => {
                  if ((event.key === "Enter" || event.key === " ") && !disabled) {
                    event.preventDefault();
                    setOpenSubmenu(index);
                    return;
                  }
                  onKeyDown(event, index);
                }}
                role="menuitem"
                tabIndex={-1}
                type="button"
              >
                <span className="menu-check" aria-hidden="true" />
                <span className="menu-label">{entry.label}</span>
                <span className="menu-arrow" aria-hidden="true">
                  ›
                </span>
              </button>
              {open && (
                <MenuList
                  items={entry.items}
                  label={entry.label}
                  onClose={onClose}
                  onCollapse={() => {
                    setOpenSubmenu(null);
                    buttons()[index]?.focus();
                  }}
                  onRun={onRun}
                  style={{ position: "absolute", left: "100%", top: -5 }}
                />
              )}
            </li>
          );
        }
        const role =
          entry.checked === undefined ? "menuitem" : entry.radio ? "menuitemradio" : "menuitemcheckbox";
        return (
          <li className="menu-entry" key={entry.id} role="none">
            <button
              aria-checked={entry.checked === undefined ? undefined : entry.checked}
              aria-disabled={disabled}
              className={`menu-item${entry.destructive ? " destructive" : ""}`}
              onClick={() => !disabled && onRun(entry)}
              onKeyDown={(event) => {
                if ((event.key === "Enter" || event.key === " ") && !disabled) {
                  event.preventDefault();
                  onRun(entry);
                  return;
                }
                onKeyDown(event, index);
              }}
              onMouseEnter={() => setOpenSubmenu(null)}
              role={role}
              tabIndex={-1}
              type="button"
            >
              <span className="menu-check" aria-hidden="true">
                {entry.checked ? (entry.radio ? "●" : "✓") : ""}
              </span>
              <span className="menu-label">{entry.label}</span>
              {entry.accelerator && (
                <span className="menu-accelerator">{displayAccelerator(entry.accelerator)}</span>
              )}
            </button>
          </li>
        );
      })}
    </ul>
  );
}
