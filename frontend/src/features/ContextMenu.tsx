import { useEffect, useRef } from "react";
import type { MenuEntry } from "../app/menu";
import { runWithRestoredFocus } from "./MenuBar";
import MenuList from "./MenuList";

export interface ContextMenuState {
  x: number;
  y: number;
  items: MenuEntry[];
  label: string;
}

interface ContextMenuProps {
  menu: ContextMenuState;
  onClose: () => void;
}

/** Menu contestuale (tasto destro o Shift+F10) posizionato sul puntatore. */
export default function ContextMenu({ menu, onClose }: ContextMenuProps) {
  const previousFocus = useRef<Element | null>(document.activeElement);
  const container = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const onPointerDown = (event: MouseEvent) => {
      if (!container.current?.contains(event.target as Node)) onClose();
    };
    const onBlur = () => onClose();
    window.addEventListener("mousedown", onPointerDown);
    window.addEventListener("blur", onBlur);
    window.addEventListener("resize", onBlur);
    return () => {
      window.removeEventListener("mousedown", onPointerDown);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("resize", onBlur);
    };
  }, [onClose]);

  return (
    <div className="context-menu-layer" ref={container}>
      <MenuList
        items={menu.items}
        label={menu.label}
        onClose={() => {
          onClose();
          if (previousFocus.current instanceof HTMLElement) previousFocus.current.focus();
        }}
        onRun={(action) => {
          onClose();
          runWithRestoredFocus(action, previousFocus.current);
        }}
        style={{ position: "fixed", left: menu.x, top: menu.y }}
      />
    </div>
  );
}
