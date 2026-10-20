import { useEffect, useRef, useState } from "react";
import type { Menu, MenuAction } from "../app/menu";
import MenuList from "./MenuList";

interface MenuBarProps {
  menus: Menu[];
  label: string;
}

/** Ripristina il focus dov'era prima del menu, poi esegue l'azione. */
export function runWithRestoredFocus(action: MenuAction, previous: Element | null) {
  if (previous instanceof HTMLElement && document.contains(previous)) {
    previous.focus({ preventScroll: true });
  }
  window.setTimeout(() => void action.run(), 0);
}

/** Barra dei menu dell'app (`role="menubar"`), navigabile da tastiera. */
export default function MenuBar({ menus, label }: MenuBarProps) {
  const [open, setOpen] = useState<number | null>(null);
  const bar = useRef<HTMLDivElement>(null);
  const previousFocus = useRef<Element | null>(null);

  useEffect(() => {
    if (open === null) return;
    const onPointerDown = (event: MouseEvent) => {
      if (!bar.current?.contains(event.target as Node)) setOpen(null);
    };
    window.addEventListener("mousedown", onPointerDown);
    return () => window.removeEventListener("mousedown", onPointerDown);
  }, [open]);

  const openMenu = (index: number) => {
    if (open === null) previousFocus.current = document.activeElement;
    setOpen(index);
  };

  const close = (restore = true) => {
    setOpen(null);
    if (restore && previousFocus.current instanceof HTMLElement) {
      previousFocus.current.focus({ preventScroll: true });
    }
  };

  const topButtons = () =>
    Array.from(bar.current?.querySelectorAll<HTMLElement>(":scope > div > [data-menubar-item]") ?? []);

  return (
    <div aria-label={label} className="menu-bar" ref={bar} role="menubar">
      {menus.map((menu, index) => (
        <div className="menu-bar-entry" key={menu.id}>
          <button
            aria-expanded={open === index}
            aria-haspopup="menu"
            className={`menu-bar-item${open === index ? " open" : ""}`}
            data-menubar-item
            id={`menubar-${menu.id}`}
            onClick={() => (open === index ? close() : openMenu(index))}
            onKeyDown={(event) => {
              const count = menus.length;
              if (event.key === "ArrowRight" || event.key === "ArrowLeft") {
                event.preventDefault();
                const next = (index + (event.key === "ArrowRight" ? 1 : count - 1)) % count;
                topButtons()[next]?.focus();
                if (open !== null) setOpen(next);
              } else if (event.key === "ArrowDown" || event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                openMenu(index);
              } else if (event.key === "Escape" && open !== null) {
                event.preventDefault();
                close();
              }
            }}
            onMouseEnter={() => open !== null && open !== index && setOpen(index)}
            onMouseDown={(event) => event.preventDefault()}
            role="menuitem"
            tabIndex={index === 0 ? 0 : -1}
            type="button"
          >
            {menu.label}
          </button>
          {open === index && (
            <MenuList
              items={menu.items}
              labelledBy={`menubar-${menu.id}`}
              onClose={() => close()}
              onNavigate={(direction) => {
                const next = (index + direction + menus.length) % menus.length;
                setOpen(next);
                topButtons()[next]?.focus();
              }}
              onRun={(action) => {
                const previous = previousFocus.current;
                setOpen(null);
                runWithRestoredFocus(action, previous);
              }}
              style={{ position: "absolute", left: 0, top: "100%" }}
            />
          )}
        </div>
      ))}
    </div>
  );
}
