/**
 * 批次C（规格 5.7.3）：引用位置"内容已更新"高亮。
 * 通过 ProseMirror Decoration 给过期引用锚点打上 xref-stale 类 + 徽标文案，
 * 编辑过程中随文档变化自动映射位置。零网络、纯本地状态。
 */
import { Extension } from "@tiptap/core";
import { Plugin, PluginKey } from "@tiptap/pm/state";
import { Decoration, DecorationSet, type EditorView } from "@tiptap/pm/view";
import type { Node as PmNode } from "@tiptap/pm/model";

const key = new PluginKey<DecorationSet>("xrefStale");

function buildDecos(doc: PmNode, hrefs: ReadonlySet<string>, label: string): DecorationSet {
  if (hrefs.size === 0) return DecorationSet.empty;
  const out: Decoration[] = [];
  doc.descendants((node, pos) => {
    if (!node.isInline || node.marks.length === 0) return;
    for (const mark of node.marks) {
      const href = mark.attrs.href;
      if (mark.type.name === "xrefRef" && typeof href === "string" && hrefs.has(href)) {
        out.push(Decoration.inline(pos, pos + node.nodeSize, {
          class: "xref-stale",
          "data-xref-stale": label,
        }));
      }
    }
  });
  return out.length > 0 ? DecorationSet.create(doc, out) : DecorationSet.empty;
}

/** 外部（useEffect）随过期清单变化刷新高亮；事务仅含 meta，不触发保存。 */
export function setXrefStaleHighlight(view: EditorView, hrefs: ReadonlySet<string>, label: string): void {
  const tr = view.state.tr.setMeta(key, buildDecos(view.state.doc, hrefs, label));
  view.dispatch(tr);
}

export function xrefStaleExtension(): Extension {
  return Extension.create({
    name: "xrefStale",
    addProseMirrorPlugins() {
      return [
        new Plugin<DecorationSet>({
          key,
          state: {
            init: () => DecorationSet.empty,
            apply: (tr, old) => {
              const meta = tr.getMeta(key);
              if (meta) return meta as DecorationSet;
              if (tr.docChanged) return old.map(tr.mapping, tr.doc);
              return old;
            },
          },
          props: {
            decorations: (state) => key.getState(state),
          },
        }),
      ];
    },
  });
}
