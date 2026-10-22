import { invoke } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { separator } from "../app/menu";
import type { MenuEntry } from "../app/menu";
import { t } from "../i18n";
import { insertIntoField, openContextMenu, subscribeContextMenu, textFieldAt } from "../lib/contextMenu";
import type { ContextMenuRequest } from "../lib/contextMenu";
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

/** Menu standard dei campi di testo, come nelle app desktop. */
function textFieldItems(field: HTMLInputElement | HTMLTextAreaElement): MenuEntry[] {
  const editable = !field.readOnly && !field.disabled;
  const selected = (field.selectionStart ?? 0) !== (field.selectionEnd ?? 0);
  // Come nei sistemi desktop, il contenuto di una password non si copia.
  const secret = field instanceof HTMLInputElement && field.type === "password";
  return [
    { id: "field-undo", label: t.undo, accelerator: "Ctrl+Z", enabled: editable, run: () => void document.execCommand("undo") },
    { id: "field-redo", label: t.redo, accelerator: "Ctrl+Y", enabled: editable, run: () => void document.execCommand("redo") },
    separator,
    { id: "field-cut", label: t.menuCut, accelerator: "Ctrl+X", enabled: editable && selected && !secret, run: () => void document.execCommand("cut") },
    { id: "field-copy", label: t.menuCopy, accelerator: "Ctrl+C", enabled: selected && !secret, run: () => void document.execCommand("copy") },
    {
      id: "field-paste",
      label: t.menuPaste,
      accelerator: "Ctrl+V",
      enabled: editable,
      run: async () => {
        const text = await invoke<string>("read_clipboard_text").catch(() => "");
        if (text) insertIntoField(field, text);
      },
    },
    { id: "field-delete", label: t.actionDelete, enabled: editable && selected, run: () => void document.execCommand("delete") },
    separator,
    { id: "field-select-all", label: t.menuSelectAll, accelerator: "Ctrl+A", enabled: field.value !== "", run: () => field.select() },
  ];
}

/**
 * Host dei menu contestuali (`openContextMenu`), montato una volta per
 * finestra. Blocca il menu della webview («Ricarica», «Indietro»…), che non
 * ha senso in un'app desktop; nella versione di sviluppo Shift+clic destro
 * lo lascia comparire per l'ispezione.
 */
export function ContextMenuHost() {
  const [menu, setMenu] = useState<ContextMenuRequest | null>(null);
  useEffect(() => subscribeContextMenu(setMenu), []);
  useEffect(() => {
    const onContextMenu = (event: MouseEvent) => {
      if (event.defaultPrevented) return;
      if (import.meta.env.DEV && event.shiftKey) return;
      event.preventDefault();
      const field = textFieldAt(event.target);
      if (field) {
        field.focus();
        openContextMenu(event, t.menuEdit, textFieldItems(field));
        return;
      }
      // Testo selezionato fuori dai campi (dialog, anteprime): solo copia.
      const selection = window.getSelection()?.toString() ?? "";
      if (selection) {
        openContextMenu(event, t.menuEdit, [
          { id: "page-copy", label: t.menuCopy, accelerator: "Ctrl+C", run: () => void document.execCommand("copy") },
        ]);
      }
    };
    window.addEventListener("contextmenu", onContextMenu);
    return () => window.removeEventListener("contextmenu", onContextMenu);
  }, []);
  if (!menu) return null;
  return <ContextMenu key={`${menu.x}:${menu.y}:${menu.label}`} menu={menu} onClose={() => setMenu(null)} />;
}
