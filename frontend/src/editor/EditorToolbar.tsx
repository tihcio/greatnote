import { useEffect, useReducer, useRef, useState } from "react";
import type { ReactNode } from "react";
import type { Editor } from "@tiptap/react";
import { t } from "../i18n";
import { separator } from "../app/menu";
import type { MenuAction, MenuEntry } from "../app/menu";
import { askText } from "../lib/prompt";
import MenuList from "../features/MenuList";
import { blockCommands } from "./commands";
import { FONT_FAMILIES, FONT_SIZES, HIGHLIGHT_COLORS, TEXT_COLORS, isSafeLink } from "./extensions";

interface EditorToolbarProps {
  editor: Editor;
  onAttach: () => void;
  onAddTask: () => void;
  /** Voci del menu AI (assente fuori dall'app desktop). */
  aiItems?: () => MenuEntry[];
}

function ToolbarMenu({ label, text, items }: { label: string; text: ReactNode; items: () => MenuEntry[] }) {
  const [open, setOpen] = useState(false);
  const container = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const close = (event: MouseEvent) => {
      if (!container.current?.contains(event.target as Node)) setOpen(false);
    };
    window.addEventListener("mousedown", close);
    return () => window.removeEventListener("mousedown", close);
  }, [open]);
  return (
    <div className="toolbar-menu" ref={container}>
      <button
        aria-expanded={open}
        aria-haspopup="menu"
        aria-label={label}
        className="toolbar-button toolbar-dropdown"
        onClick={() => setOpen((current) => !current)}
        onMouseDown={(event) => event.preventDefault()}
        title={label}
        type="button"
      >
        {text}
        <span aria-hidden="true" className="toolbar-caret">
          ▾
        </span>
      </button>
      {open && (
        <MenuList
          items={items()}
          label={label}
          onClose={() => setOpen(false)}
          onRun={(action: MenuAction) => {
            setOpen(false);
            void action.run();
          }}
          style={{ position: "absolute", left: 0, top: "100%" }}
        />
      )}
    </div>
  );
}

function ToolbarButton({
  label,
  active,
  disabled,
  onClick,
  children,
}: {
  label: string;
  active?: boolean;
  disabled?: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      aria-label={label}
      aria-pressed={active}
      className={`toolbar-button${active ? " is-active" : ""}`}
      disabled={disabled}
      onClick={onClick}
      onMouseDown={(event) => event.preventDefault()}
      title={label}
      type="button"
    >
      {children}
    </button>
  );
}

export async function editLink(editor: Editor) {
  const current = editor.getAttributes("link").href as string | undefined;
  const url = await askText({
    title: t.linkTitle,
    label: t.linkUrl,
    initialValue: current ?? "https://",
    confirmLabel: t.saveTask,
    maxLength: 2048,
    validate: (value) => (isSafeLink(value) ? null : t.linkInvalid),
  });
  if (!url) return;
  const chain = editor.chain().focus().extendMarkRange("link");
  if (editor.state.selection.empty && !current) {
    chain.insertContent({ type: "text", text: url, marks: [{ type: "link", attrs: { href: url } }] }).run();
  } else {
    chain.setLink({ href: url }).run();
  }
}

/** Barra degli strumenti dell'editor, sul modello delle app di note. */
export default function EditorToolbar({ editor, onAttach, onAddTask, aiItems }: EditorToolbarProps) {
  // Aggiorna lo stato dei pulsanti quando cambia selezione o contenuto.
  const [, refresh] = useReducer((value: number) => value + 1, 0);
  useEffect(() => {
    editor.on("transaction", refresh);
    return () => {
      editor.off("transaction", refresh);
    };
  }, [editor]);

  const action = (id: string, label: string, run: () => void, extra: Partial<MenuAction> = {}): MenuAction => ({
    id,
    label,
    run,
    ...extra,
  });
  const style = editor.getAttributes("textStyle") as { fontFamily?: string; fontSize?: string; color?: string };
  const headingLevel = [1, 2, 3].find((level) => editor.isActive("heading", { level }));
  const inTable = editor.isActive("table");

  const insertItems = (): MenuEntry[] => [
    ...blockCommands()
      .filter((command) => !["paragraph", "heading-1", "heading-2", "heading-3"].includes(command.id))
      .map((command) => action(`insert-${command.id}`, command.label, () => command.run(editor))),
    separator,
    action("insert-link", t.insertLink, () => void editLink(editor)),
    action("insert-attachment", t.addAttachment, onAttach),
    action("insert-task", t.insertLinkedTask, onAddTask),
  ];

  const styleItems = (): MenuEntry[] => [
    action("style-paragraph", t.blockText, () => editor.chain().focus().setParagraph().run(), {
      checked: !headingLevel && editor.isActive("paragraph"),
      radio: true,
    }),
    ...[1, 2, 3].map((level) =>
      action(`style-h${level}`, t.blockHeading(level), () =>
        editor.chain().focus().toggleHeading({ level: level as 1 | 2 | 3 }).run(), {
        checked: headingLevel === level,
        radio: true,
      }),
    ),
  ];

  const fontItems = (): MenuEntry[] => [
    action("font-default", t.fontDefault, () => editor.chain().focus().unsetFontFamily().run(), {
      checked: !style.fontFamily,
      radio: true,
    }),
    ...FONT_FAMILIES.map((family) =>
      action(`font-${family.value}`, family.label, () => editor.chain().focus().setFontFamily(family.value).run(), {
        checked: style.fontFamily === family.value,
        radio: true,
      }),
    ),
  ];

  const sizeItems = (): MenuEntry[] => [
    action("size-default", t.fontDefault, () => editor.chain().focus().unsetFontSize().run(), {
      checked: !style.fontSize,
      radio: true,
    }),
    ...FONT_SIZES.map((size) =>
      action(`size-${size}`, size.replace("px", ""), () => editor.chain().focus().setFontSize(size).run(), {
        checked: style.fontSize === size,
        radio: true,
      }),
    ),
  ];

  const colorItems = (): MenuEntry[] => [
    action("color-default", t.colorDefault, () => editor.chain().focus().unsetColor().run(), {
      checked: !style.color,
      radio: true,
    }),
    ...TEXT_COLORS.map((color, index) =>
      action(`color-${color}`, t.colorName(index), () => editor.chain().focus().setColor(color).run(), {
        checked: style.color === color,
        radio: true,
      }),
    ),
  ];

  const highlightItems = (): MenuEntry[] => [
    action("highlight-none", t.highlightNone, () => editor.chain().focus().unsetHighlight().run()),
    ...HIGHLIGHT_COLORS.map((color, index) =>
      action(`highlight-${color}`, t.highlightName(index), () =>
        editor.chain().focus().toggleHighlight({ color }).run(), {
        checked: editor.isActive("highlight", { color }),
        radio: true,
      }),
    ),
  ];

  const moreItems = (): MenuEntry[] => [
    action("strike", t.strike, () => editor.chain().focus().toggleStrike().run(), {
      checked: editor.isActive("strike"),
    }),
    action("code", t.code, () => editor.chain().focus().toggleCode().run(), {
      checked: editor.isActive("code"),
    }),
    action("quote", t.quote, () => editor.chain().focus().toggleBlockquote().run(), {
      checked: editor.isActive("blockquote"),
    }),
    action("code-block", t.codeBlock, () => editor.chain().focus().toggleCodeBlock().run(), {
      checked: editor.isActive("codeBlock"),
    }),
    separator,
    action("link", t.insertLink, () => void editLink(editor)),
    action("unlink", t.removeLink, () => editor.chain().focus().unsetLink().run(), {
      enabled: editor.isActive("link"),
    }),
    action("clear", t.clearFormatting, () => editor.chain().focus().unsetAllMarks().clearNodes().run()),
    separator,
    {
      kind: "submenu",
      id: "table",
      label: t.tableMenu,
      enabled: inTable,
      items: [
        action("row-before", t.tableRowBefore, () => editor.chain().focus().addRowBefore().run()),
        action("row-after", t.tableRowAfter, () => editor.chain().focus().addRowAfter().run()),
        action("col-before", t.tableColumnBefore, () => editor.chain().focus().addColumnBefore().run()),
        action("col-after", t.tableColumnAfter, () => editor.chain().focus().addColumnAfter().run()),
        separator,
        action("row-delete", t.tableDeleteRow, () => editor.chain().focus().deleteRow().run()),
        action("col-delete", t.tableDeleteColumn, () => editor.chain().focus().deleteColumn().run()),
        action("table-delete", t.tableDelete, () => editor.chain().focus().deleteTable().run(), {
          destructive: true,
        }),
      ],
    },
  ];

  const styleLabel = headingLevel ? t.blockHeading(headingLevel) : t.blockText;
  const fontLabel = FONT_FAMILIES.find((family) => family.value === style.fontFamily)?.label ?? t.fontDefault;
  const sizeLabel = style.fontSize ? style.fontSize.replace("px", "") : "15";

  return (
    <div className="editor-toolbar" role="toolbar" aria-label={t.formatting}>
      <ToolbarMenu items={insertItems} label={t.insertMenu} text={<span className="toolbar-insert">＋ {t.insertMenu}</span>} />
      <span className="toolbar-divider" />
      <ToolbarButton disabled={!editor.can().undo()} label={t.undo} onClick={() => editor.chain().focus().undo().run()}>
        ↶
      </ToolbarButton>
      <ToolbarButton disabled={!editor.can().redo()} label={t.redo} onClick={() => editor.chain().focus().redo().run()}>
        ↷
      </ToolbarButton>
      <span className="toolbar-divider" />
      <ToolbarMenu items={styleItems} label={t.textStyle} text={styleLabel} />
      <ToolbarMenu items={fontItems} label={t.fontFamily} text={fontLabel} />
      <ToolbarMenu items={sizeItems} label={t.fontSize} text={sizeLabel} />
      <span className="toolbar-divider" />
      <ToolbarMenu
        items={colorItems}
        label={t.textColor}
        text={<span className="color-swatch" style={{ background: style.color ?? "currentColor" }} />}
      />
      <ToolbarButton active={editor.isActive("bold")} label={t.bold} onClick={() => editor.chain().focus().toggleBold().run()}>
        <strong>B</strong>
      </ToolbarButton>
      <ToolbarButton active={editor.isActive("italic")} label={t.italic} onClick={() => editor.chain().focus().toggleItalic().run()}>
        <em>I</em>
      </ToolbarButton>
      <ToolbarButton active={editor.isActive("underline")} label={t.underline} onClick={() => editor.chain().focus().toggleUnderline().run()}>
        <span className="underline-icon">U</span>
      </ToolbarButton>
      <ToolbarMenu items={highlightItems} label={t.highlight} text={<span className="highlight-icon">A</span>} />
      <span className="toolbar-divider" />
      <ToolbarButton active={editor.isActive("bulletList")} label={t.bulletList} onClick={() => editor.chain().focus().toggleBulletList().run()}>
        •≡
      </ToolbarButton>
      <ToolbarButton active={editor.isActive("orderedList")} label={t.orderedList} onClick={() => editor.chain().focus().toggleOrderedList().run()}>
        1≡
      </ToolbarButton>
      <ToolbarButton active={editor.isActive("taskList")} label={t.checklist} onClick={() => editor.chain().focus().toggleTaskList().run()}>
        ☑
      </ToolbarButton>
      <ToolbarMenu items={moreItems} label={t.moreFormatting} text={t.moreFormatting} />
      {aiItems && (
        <>
          <span className="toolbar-divider" />
          <ToolbarMenu items={aiItems} label={t.aiMenuLabel} text={<span className="toolbar-ai">✦ AI</span>} />
        </>
      )}
    </div>
  );
}
