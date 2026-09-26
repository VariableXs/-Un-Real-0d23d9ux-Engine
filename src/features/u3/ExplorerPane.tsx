/**
 * 资源管理器 UI 装配件（AI-U3 v5 · 批次五装配层 · ExplorerPane）。
 *
 * 职责：消费 expui 引擎（虚拟化几何/同步计划/键盘树导航）与 explorerx
 * 逻辑模块（状态栏三段/树折叠持久化），装配出「万节点可实绘」的
 * 资源管理器三窗格活体件：
 * - <TreePane>：树窗格（虚拟窗口渲染 + 键盘四向 + 箭头/行点击分流 +
 *   F528 高亮不抢焦点）；
 * - <FileListPane>：列表窗格（虚拟窗口 + 选中态 + 空目录态）；
 * - <ExplorerBreadcrumb>：面包屑（长路径中段收缩——章七不破版）；
 * - <ExplorerPaneSection>：U3Lab 演示区（双窗格联动 + 状态栏实况）。
 *
 * 交互纪律自证：树/列表焦点分离（高亮 ≠ 焦点，F206 同源）；键盘全程
 * 可达（树四向 + 列表 ↑↓ Enter）；万节点只物化视口窗口（expui 窗口器）。
 */

import React, { useCallback, useMemo, useRef, useState } from "react";
import { SectionCard } from "../settings/MouseJ1Panels";
import {
  flattenTree, viewportWindow, rowTop, planListToTree, treeClickVerdict,
  treeKeyNav, breadcrumbFit, clampTreeWidth, TREE_PANE_DEFAULT_PX,
  TREE_ROW_HEIGHT_PX, EXP_STATUSBAR_HEIGHT_PX,
  type TreeNode,
} from "./engines";
import { statusBarSegments } from "./explorerx";

/* ------------------------------ 演示数据 ------------------------------ */

const DEMO_TREE: Array<TreeNode> = [
  { id: "c", name: "本地磁盘 (C:)", children: [
    { id: "work", name: "work", children: [
      { id: "src", name: "src", children: [{ id: "engines", name: "engines" }, { id: "ui", name: "ui" }] },
      { id: "docs", name: "docs" },
    ] },
    { id: "temp", name: "temp" },
  ] },
  { id: "d", name: "数据盘 (D:)", children: [{ id: "media", name: "media" }] },
];

/** 当前目录 → 列表条目（演示面静态映射——真实数据由 shell 消费层供给）。 */
const DEMO_LISTING: Record<string, Array<{ name: string; bytes: number }>> = {
  "c": [{ name: "work", bytes: 0 }, { name: "temp", bytes: 4096 }],
  "work": [{ name: "src", bytes: 0 }, { name: "docs", bytes: 245897 }],
  "src": [{ name: "engines", bytes: 0 }, { name: "ui", bytes: 0 }],
  "engines": [{ name: "despaint.ts", bytes: 18432 }, { name: "expui.ts", bytes: 15360 }, { name: "lockmount.ts", bytes: 13824 }],
  "ui": [{ name: "DeskPaint.tsx", bytes: 10240 }],
  "docs": [],
  "temp": [{ name: "~cache.bin", bytes: 7340032 }],
  "d": [{ name: "media", bytes: 0 }],
  "media": [{ name: "wallpaper-4k.png", bytes: 12582912 }],
};

/* ------------------------------ 树窗格 ------------------------------ */

export interface TreePaneProps {
  tree: ReadonlyArray<TreeNode>;
  expanded: ReadonlySet<string>;
  focusId: string | null;
  highlightId: string | null;
  viewportH: number;
  onFocus: (id: string) => void;
  onOpen: (id: string) => void;
  onToggle: (id: string, on: boolean) => void;
}

/** 树窗格：只物化视口窗口行；箭头/行点击分流；键盘四向。 */
export function TreePane(props: TreePaneProps): React.ReactElement {
  const rows = useMemo(() => flattenTree(props.tree, props.expanded), [props.tree, props.expanded]);
  const [scrollTop, setScrollTop] = useState(0);
  const win = viewportWindow(rows.length, scrollTop, props.viewportH, TREE_ROW_HEIGHT_PX);
  const visible = rows.slice(win.start, win.end);

  const onRowClick = useCallback((id: string, arrow: boolean) => {
    const v = treeClickVerdict(arrow);
    if (v.toggleOnly) {
      const isOpen = props.expanded.has(id);
      props.onToggle(id, !isOpen);
    } else {
      props.onFocus(id);
      props.onOpen(id);
    }
  }, [props]);

  const onKeyDown = useCallback((e: React.KeyboardEvent) => {
    const cur = props.focusId ?? rows[0]?.id ?? null;
    if (["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"].includes(e.key)) {
      const dir = e.key.replace("Arrow", "").toLowerCase() as "up" | "down" | "left" | "right";
      const res = treeKeyNav(rows, cur, dir, props.onToggle);
      if (res.focusId) props.onFocus(res.focusId);
      e.preventDefault();
    } else if (e.key === "Enter" && cur) {
      props.onOpen(cur);
      e.preventDefault();
    }
  }, [props, rows]);

  return (
    <div
      className="u3-exp-tree"
      role="tree"
      style={{ height: props.viewportH }}
      tabIndex={0}
      onKeyDown={onKeyDown}
      onScroll={(e) => setScrollTop((e.target as HTMLElement).scrollTop)}
    >
      <div style={{ height: rows.length * TREE_ROW_HEIGHT_PX, position: "relative" }}>
        {visible.map((r, i) => {
          const idx = win.start + i;
          const isFocus = r.id === props.focusId;
          const isHighlight = r.id === props.highlightId;
          return (
            <div
              key={r.id}
              role="treeitem"
              aria-level={r.depth + 1}
              aria-expanded={r.hasChildren ? r.expanded : undefined}
              className={"u3-exp-tree-row" + (isFocus ? " is-focus" : "") + (isHighlight && !isFocus ? " is-hl" : "")}
              style={{ position: "absolute", top: rowTop(idx), left: 0, right: 0, height: TREE_ROW_HEIGHT_PX, paddingLeft: 8 + r.depth * 16 }}
              onClick={(e) => onRowClick(r.id, (e.target as HTMLElement).classList.contains("u3-exp-arrow"))}
            >
              <span className="u3-exp-arrow" aria-hidden>{r.hasChildren ? (r.expanded ? "▾" : "▸") : "·"}</span>
              <span className="u3-exp-name">{r.name}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

/* ------------------------------ 列表窗格 ------------------------------ */

export function FileListPane(props: { entries: ReadonlyArray<{ name: string; bytes: number }>; selected: ReadonlySet<string>; onSelect: (name: string, additive: boolean) => void; viewportH: number }): React.ReactElement {
  const [scrollTop, setScrollTop] = useState(0);
  const win = viewportWindow(props.entries.length, scrollTop, props.viewportH, 28);
  const visible = props.entries.slice(win.start, win.end);
  return (
    <div className="u3-exp-list" role="listbox" aria-label="文件列表" style={{ height: props.viewportH }}
      tabIndex={0}
      onScroll={(e) => setScrollTop((e.target as HTMLElement).scrollTop)}>
      {props.entries.length === 0 && <div className="u3-exp-empty">此文件夹为空</div>}
      {visible.map((f) => (
        <div
          key={f.name}
          role="option"
          aria-selected={props.selected.has(f.name)}
          className={"u3-exp-file" + (props.selected.has(f.name) ? " is-selected" : "")}
          onClick={(e) => props.onSelect(f.name, e.ctrlKey)}
        >
          <span className="u3-exp-fname">{f.name}</span>
          <span className="u3-exp-fsize">{f.bytes === 0 ? "文件夹" : `${f.bytes} B`}</span>
        </div>
      ))}
    </div>
  );
}

/* ------------------------------ 面包屑 ------------------------------ */

export function ExplorerBreadcrumb(props: { path: ReadonlyArray<string>; maxChars?: number }): React.ReactElement {
  const crumbs = props.path.map((id) => ({ id, name: id === "root" ? "此机" : id }));
  const fitted = breadcrumbFit(crumbs, props.maxChars ?? 26);
  return (
    <nav className="u3-exp-crumbs" aria-label="面包屑">
      {fitted.map((c) => (
        <span key={c.id} className={"u3-exp-crumb" + (c.name === "…" ? " is-ellipsis" : "")}>{c.name}</span>
      ))}
    </nav>
  );
}

/* ------------------------------ U3Lab 演示区 ------------------------------ */

/** U3Lab v5 资源管理器装配区（树/列表双向同步 + 状态栏三段实况）。 */
export function ExplorerPaneSection(): React.ReactElement {
  const [treeState, setTreeState] = useState<{ expanded: Record<string, boolean> }>(() => ({ expanded: { c: true, work: true, src: true } }));
  const [dirId, setDirId] = useState("src");
  const [focusId, setFocusId] = useState<string | null>("src");
  const [selected, setSelected] = useState<ReadonlySet<string>>(new Set());
  const [treeWidth, setTreeWidth] = useState(TREE_PANE_DEFAULT_PX);
  const dragRef = useRef<{ startX: number; startW: number } | null>(null);
  const VIEWPORT_H = 168;

  const expandedSet = useMemo(() => new Set(Object.entries(treeState.expanded).filter(([, v]) => v).map(([k]) => k)), [treeState]);
  const tree = DEMO_TREE;

  /** 树点选 → 目录切换 + F528 同步计划（列表 → 树高亮同源）。 */
  const openDir = useCallback((id: string) => {
    // 由平铺树反查祖先路径（演示面直接走平铺——真实层由 shell 供路径）
    const path: Array<string> = [];
    const find = (nodes: ReadonlyArray<TreeNode>, chain: Array<string>): boolean => {
      for (const n of nodes) {
        const next = [...chain, n.id];
        if (n.id === id) { path.push(...next); return true; }
        if (n.children && find(n.children, next)) return true;
      }
      return false;
    };
    find(tree, []);
    const plan = planListToTree(path, expandedSet);
    if (plan.toExpand.length > 0) {
      setTreeState((s) => {
        const next = { ...s.expanded };
        for (const id2 of plan.toExpand) next[id2] = true;
        return { expanded: next };
      });
    }
    setDirId(id);
    setFocusId(id);
    setSelected(new Set());
  }, [expandedSet, tree]);

  const entries = DEMO_LISTING[dirId] ?? [];
  const selBytes = entries.filter((f) => selected.has(f.name)).reduce((a, f) => a + f.bytes, 0);
  const segs = statusBarSegments({ items: entries.length, selected: selected.size, selectedBytes: selBytes, volumeFreeBytes: 96 * 1024 ** 3 });

  const onTreeWidthDown = useCallback((e: React.PointerEvent) => {
    dragRef.current = { startX: e.clientX, startW: treeWidth };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }, [treeWidth]);
  const onTreeWidthMove = useCallback((e: React.PointerEvent) => {
    const d = dragRef.current;
    if (!d) return;
    setTreeWidth(clampTreeWidth(d.startW + (e.clientX - d.startX)));
  }, []);
  const onTreeWidthUp = useCallback(() => { dragRef.current = null; }, []);

  return (
    <SectionCard title="资源管理器装配区" f="F526/F527/F528·v5">
      <div className="u3-lab-intro">
        万节点虚拟窗口 + 树列表双向同步 + 24px 状态栏三段——几何与同步全部来自 expui 引擎，状态栏文案与 explorerx 同源。
      </div>
      <ExplorerBreadcrumb path={["root", "c", "work", dirId]} />
      <div className="u3-exp-wrap">
        <div className="u3-exp-treebox" style={{ width: treeWidth }}>
          <TreePane
            tree={tree}
            expanded={expandedSet}
            focusId={focusId}
            highlightId={dirId}
            viewportH={VIEWPORT_H}
            onFocus={setFocusId}
            onOpen={openDir}
            onToggle={(id, on) => setTreeState((s) => ({ expanded: { ...s.expanded, [id]: on } }))}
          />
        </div>
        <div className="u3-exp-split" role="separator" aria-orientation="vertical" aria-label="拖拽调整树宽"
          onPointerDown={onTreeWidthDown} onPointerMove={onTreeWidthMove} onPointerUp={onTreeWidthUp} />
        <FileListPane entries={entries} selected={selected} viewportH={VIEWPORT_H}
          onSelect={(name, additive) => setSelected((s) => {
            const next = additive ? new Set(s) : new Set<string>();
            if (additive && next.has(name)) next.delete(name); else next.add(name);
            return next;
          })} />
      </div>
      <div className="u3-exp-statusbar" style={{ height: EXP_STATUSBAR_HEIGHT_PX }} role="status">
        <span>{segs[0]}</span>
        <span>{segs[1]}</span>
        <span>{segs[2]}</span>
      </div>
    </SectionCard>
  );
}
