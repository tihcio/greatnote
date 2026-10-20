import { Extension } from "@tiptap/react";
import type { Editor } from "@tiptap/react";
import { Plugin, PluginKey } from "@tiptap/pm/state";
import { Decoration, DecorationSet } from "@tiptap/pm/view";

interface Range {
  from: number;
  to: number;
}

const key = new PluginKey<DecorationSet>("searchHighlight");

/** Evidenzia le occorrenze di "Trova" anche quando l'editor non ha il focus. */
export const SearchHighlight = Extension.create({
  name: "searchHighlight",
  addProseMirrorPlugins() {
    return [
      new Plugin<DecorationSet>({
        key,
        state: {
          init: () => DecorationSet.empty,
          apply(transaction, decorations) {
            const meta = transaction.getMeta(key) as { ranges: Range[]; current: number } | undefined;
            if (meta) {
              return DecorationSet.create(
                transaction.doc,
                meta.ranges.map((range, index) =>
                  Decoration.inline(range.from, range.to, {
                    class: index === meta.current ? "search-match current" : "search-match",
                  }),
                ),
              );
            }
            return decorations.map(transaction.mapping, transaction.doc);
          },
        },
        props: {
          decorations(state) {
            return key.getState(state);
          },
        },
      }),
    ];
  },
});

export function setSearchHighlights(editor: Editor, ranges: Range[], current: number) {
  if (editor.isDestroyed) return;
  editor.view.dispatch(editor.state.tr.setMeta(key, { ranges, current }).setMeta("addToHistory", false));
}
