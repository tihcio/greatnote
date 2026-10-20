import { useEffect, useRef, useState } from "react";
import type { Editor } from "@tiptap/react";
import { t } from "../i18n";
import { blockCommands } from "./commands";
import type { BlockCommand } from "./commands";

interface SlashState {
  from: number;
  to: number;
  query: string;
  left: number;
  top: number;
}

/**
 * Menu dei comandi rapidi: digitando "/" a inizio riga (o dopo uno spazio)
 * compare l'elenco dei blocchi, filtrabile scrivendo.
 */
export default function SlashMenu({ editor, enabled }: { editor: Editor; enabled: boolean }) {
  const [state, setState] = useState<SlashState | null>(null);
  const [active, setActive] = useState(0);
  const stateRef = useRef<SlashState | null>(null);
  stateRef.current = state;

  const query = state?.query.toLowerCase() ?? "";
  const commands = blockCommands().filter(
    (command) =>
      !query || command.label.toLowerCase().includes(query) || command.keywords.includes(query),
  );
  const commandsRef = useRef<BlockCommand[]>(commands);
  commandsRef.current = commands;
  const activeRef = useRef(active);
  activeRef.current = active;

  useEffect(() => {
    if (!enabled) {
      setState(null);
      return;
    }
    const detect = () => {
      const { selection } = editor.state;
      if (!selection.empty || !editor.isFocused || editor.isActive("codeBlock")) {
        setState(null);
        return;
      }
      const { $from } = selection;
      const before = $from.parent.textBetween(0, $from.parentOffset, "\n", "\n");
      const match = /(^|\s)\/([\p{L}\d]*)$/u.exec(before);
      if (!match) {
        setState(null);
        return;
      }
      const from = selection.from - match[2].length - 1;
      const coords = editor.view.coordsAtPos(selection.from);
      setState((current) => {
        if (!current || current.query !== match[2]) setActive(0);
        return { from, to: selection.from, query: match[2], left: coords.left, top: coords.bottom + 6 };
      });
    };
    const hide = () => setState(null);
    editor.on("selectionUpdate", detect);
    editor.on("update", detect);
    editor.on("blur", hide);
    return () => {
      editor.off("selectionUpdate", detect);
      editor.off("update", detect);
      editor.off("blur", hide);
    };
  }, [editor, enabled]);

  const choose = (command: BlockCommand | undefined) => {
    const current = stateRef.current;
    if (!command || !current) return;
    editor.chain().focus().deleteRange({ from: current.from, to: current.to }).run();
    command.run(editor);
    setState(null);
  };

  useEffect(() => {
    if (!state) return;
    const dom = editor.view.dom;
    const onKeyDown = (event: KeyboardEvent) => {
      const list = commandsRef.current;
      if (list.length === 0) return;
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        event.stopPropagation();
        const direction = event.key === "ArrowDown" ? 1 : -1;
        setActive((current) => (current + direction + list.length) % list.length);
      } else if (event.key === "Enter" || event.key === "Tab") {
        event.preventDefault();
        event.stopPropagation();
        choose(list[activeRef.current]);
      } else if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        setState(null);
      }
    };
    dom.addEventListener("keydown", onKeyDown, true);
    return () => dom.removeEventListener("keydown", onKeyDown, true);
    // `choose` legge lo stato dai ref: basta reagire all'apertura del menu.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [editor, state !== null]);

  if (!state || commands.length === 0) return null;
  return (
    <ul
      aria-label={t.slashCommands}
      className="menu-list slash-menu"
      onMouseDown={(event) => event.preventDefault()}
      role="listbox"
      style={{ position: "fixed", left: state.left, top: state.top }}
    >
      {commands.map((command, index) => (
        <li
          aria-selected={index === active}
          className={`slash-item${index === active ? " active" : ""}`}
          key={command.id}
          onClick={() => choose(command)}
          onMouseMove={() => setActive(index)}
          role="option"
        >
          <span aria-hidden="true" className="slash-icon">
            {command.icon}
          </span>
          {command.label}
        </li>
      ))}
    </ul>
  );
}
