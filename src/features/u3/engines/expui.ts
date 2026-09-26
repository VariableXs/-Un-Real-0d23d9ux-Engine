/**
 * 资源管理器装配引擎（AI-U3 v5 · 批次五装配层之二）。
 *
 * 职责：把 F526/F527/F528 的「逻辑判定」（explorerx.ts）推进到「万节点可
 * 实绘的装配算法」——树窗格与列表窗格的虚拟化、双向同步时序、面包屑：
 * - 树平铺（flatten）：展开集 → 可见行序列（深度缩进 + 箭头态 + 连接线
 *   语义）；万节点预算（F228 同源）：展开 1 万节点平铺 <16ms、只物化
 *   视口窗口行；
 * - 视口窗口（windowing）：scrollTop + 视口高 → 行切片（overscan 上下
 *   各 8 行防滚动白边）——列表侧同一套窗口器（一处一事实）；
 * - 双向同步时序（F528）：列表进子目录 → 展开祖先 + 高亮 + 滚动到可见
 *   （TREE_SYNC_BUDGET_MS 同源预算）；树点选 → 列表刷新（焦点不迁移）；
 * - 面包屑：路径 → 面包屑段（长路径收缩为中段省略——章七小窗口不破版）；
 * - 键盘树导航：↑↓ 行移动、→ 展开/进子、← 折叠/回父、首字母跳选
 *   （与桌面 type-ahead 同窗 800ms——交互词典一致）。
 *
 * 诚实边界：不持 DOM——产出「该画哪些行、画在哪」；绘制归
 * ExplorerPane.tsx（章十四：核心算法层 + 本体自由组装）。
 */

import { STATUSBAR_HEIGHT_PX } from "../explorerx";

/* ------------------------------- 树平铺 ------------------------------- */

export interface TreeNode {
  id: string;
  name: string;
  children?: Array<TreeNode>;
}

export interface FlatRow {
  id: string;
  name: string;
  depth: number;
  expanded: boolean;
  hasChildren: boolean;
}

/** 深度优先平铺（展开集决定可见性；展开 1 万节点同数量级预算）。 */
export function flattenTree(roots: ReadonlyArray<TreeNode>, expanded: ReadonlySet<string>): Array<FlatRow> {
  const out: Array<FlatRow> = [];
  const walk = (nodes: ReadonlyArray<TreeNode>, depth: number): void => {
    for (const n of nodes) {
      const hasChildren = !!n.children && n.children.length > 0;
      const isOpen = expanded.has(n.id);
      out.push({ id: n.id, name: n.name, depth, expanded: isOpen && hasChildren, hasChildren });
      if (hasChildren && isOpen) walk(n.children!, depth + 1);
    }
  };
  walk(roots, 0);
  return out;
}

/** 万节点平铺性能预算（F228 虚拟化同源：平铺本身 O(可见)，物化窗口行）。 */
export const FLATTEN_BUDGET_MS = 16;
/** 视口 overscan（上下各 8 行——滚动白边防线）。 */
export const VIEWPORT_OVERSCAN = 8;
/** 树行高（F228 规格表 24px 基线——密度紧凑档）。 */
export const TREE_ROW_HEIGHT_PX = 24;

/** 视口窗口器：滚动位置 → 可见行切片（树与列表共用——一套规则）。
 *  越界滚动（惯性/弹性过冲）夹取到末页——首行钳制防空窗（章五边界感）。 */
export function viewportWindow(rowCount: number, scrollTopPx: number, viewportHeightPx: number, rowHeightPx: number): { start: number; end: number } {
  if (rowCount === 0) return { start: 0, end: 0 };
  const visible = Math.ceil(viewportHeightPx / rowHeightPx);
  const maxFirst = Math.max(0, rowCount - visible);
  const first = Math.min(Math.floor(scrollTopPx / rowHeightPx), maxFirst);
  const start = Math.max(0, first - VIEWPORT_OVERSCAN);
  const end = Math.min(rowCount, first + visible + VIEWPORT_OVERSCAN);
  return { start, end };
}

/** 行在视口内的绝对 top（绘制层直接落位，不再自算）。 */
export function rowTop(index: number, rowHeightPx = TREE_ROW_HEIGHT_PX): number {
  return index * rowHeightPx;
}

/* ------------------------------- F528 双向同步 ------------------------------- */

export interface SyncPlan {
  /** 需要新展开的祖先 id（列表进子目录时——已展开的不重复动）。 */
  toExpand: Array<string>;
  /** 高亮目标（滚动到可见由 viewport 索引给出）。 */
  highlightId: string;
  /** 该同步消耗的时序预算（判据 5 层 <100ms——explorerx 同源）。 */
  budgetMs: number;
}

/**
 * 列表 → 树同步计划：由路径祖先链与当前展开集算出「要展开谁、高亮谁」。
 * 焦点不迁移语义（explorerx.TREE_SYNC_FOCUS_NOTE 同源）。
 */
export function planListToTree(pathIds: ReadonlyArray<string>, expanded: ReadonlySet<string>): SyncPlan {
  const toExpand = pathIds.filter((id) => !expanded.has(id));
  return {
    toExpand,
    highlightId: pathIds[pathIds.length - 1] ?? "",
    budgetMs: 100,
  };
}

/** 树 → 列表裁决：点选即刷新；展开箭头点击 ≠ 点选（不误触发列表刷新）。 */
export function treeClickVerdict(hitArrow: boolean): { refreshList: boolean; toggleOnly: boolean } {
  return hitArrow ? { refreshList: false, toggleOnly: true } : { refreshList: true, toggleOnly: false };
}

/* ------------------------------- 面包屑 ------------------------------- */

/** 面包屑段模型。 */
export interface Crumb { id: string; name: string }

/**
 * 面包屑收缩：视口装不下全路径时中段收省略（首尾保留——根与当前目录
 * 永远可见，章七最小窗口不破版）。
 */
export function breadcrumbFit(crumbs: ReadonlyArray<Crumb>, maxChars: number): Array<Crumb | { id: string; name: "…" }> {
  const total = crumbs.reduce((a, c) => a + c.name.length, 0);
  if (total <= maxChars || crumbs.length <= 2) return [...crumbs];
  const out: Array<Crumb | { id: string; name: "…" }> = [crumbs[0]!];
  let used = crumbs[0]!.name.length + 1; // 1 = 省略号占位
  const tailIdx = crumbs.length - 1;
  let tailCut = tailIdx;
  // 从尾部尽量多保留
  while (tailCut > 1 && used + crumbs[tailCut]!.name.length + 1 <= maxChars) {
    used += crumbs[tailCut]!.name.length + 1;
    tailCut--;
  }
  out.push({ id: "__ellipsis__", name: "…" });
  for (let i = tailCut; i < crumbs.length; i++) out.push(crumbs[i]!);
  return out;
}

/* ------------------------------- 键盘树导航 ------------------------------- */

/** 树键盘导航裁决（↑↓→← 四向 + 首字母跳选由调用方复用桌面 type-ahead）。 */
export function treeKeyNav(rows: ReadonlyArray<FlatRow>, currentId: string | null, key: "up" | "down" | "right" | "left", toggle: (id: string, on: boolean) => void): { focusId: string | null; toggled: boolean } {
  if (rows.length === 0) return { focusId: null, toggled: false };
  const i = currentId === null ? -1 : rows.findIndex((r) => r.id === currentId);
  if (key === "up") return { focusId: rows[Math.max(0, i <= 0 ? 0 : i - 1)]!.id, toggled: false };
  if (key === "down") return { focusId: rows[Math.min(rows.length - 1, i + 1)]!.id, toggled: false };
  if (i < 0) return { focusId: rows[0]!.id, toggled: false };
  const row = rows[i]!;
  if (key === "right") {
    if (row.hasChildren && !row.expanded) {
      toggle(row.id, true);
      return { focusId: row.id, toggled: true };
    }
    // 已展开 → 进第一个子节点
    const child = rows[i + 1];
    return { focusId: child && child.depth > row.depth ? child.id : row.id, toggled: false };
  }
  // left：已展开 → 折叠；已折叠 → 回父（沿平铺向上找最近浅层）
  if (row.expanded) {
    toggle(row.id, false);
    return { focusId: row.id, toggled: true };
  }
  for (let j = i - 1; j >= 0; j--) {
    if (rows[j]!.depth < row.depth) return { focusId: rows[j]!.id, toggled: false };
  }
  return { focusId: row.id, toggled: false };
}

/* ------------------------------- 状态栏装配 ------------------------------- */

/** 状态栏高度常量出口（explorerx 同源——一处一事实）。 */
export const EXP_STATUSBAR_HEIGHT_PX = STATUSBAR_HEIGHT_PX;

/** 窗格布局：树宽可拖（默认 200px、范围 [140, 480]——章七贴边分屏不破版）。 */
export const TREE_PANE_DEFAULT_PX = 200;
export const TREE_PANE_MIN_PX = 140;
export const TREE_PANE_MAX_PX = 480;

/** 树宽拖拽裁决（越界夹取——拖不出边界的路，章五）。 */
export function clampTreeWidth(px: number): number {
  return Math.min(TREE_PANE_MAX_PX, Math.max(TREE_PANE_MIN_PX, px));
}

/* ------------------------------- 自检 ------------------------------- */

/** 资源管理器装配引擎自检（F550 锚点域消费）。 */
export function expuiSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 树平铺
  const tree: Array<TreeNode> = [
    { id: "c", name: "C:", children: [
      { id: "work", name: "work", children: [{ id: "src", name: "src" }, { id: "doc", name: "doc" }] },
      { id: "tmp", name: "tmp" },
    ] },
    { id: "d", name: "D:" },
  ];
  const flatClosed = flattenTree(tree, new Set());
  checks.push({ name: "折叠态只见根", pass: flatClosed.length === 2 && flatClosed.every((r) => r.depth === 0) });
  const flatOpen = flattenTree(tree, new Set(["c", "work"]));
  checks.push({ name: "展开链全可见+深度", pass: flatOpen.length === 6 && flatOpen.find((r) => r.id === "src")?.depth === 2 });
  checks.push({ name: "箭头态诚实", pass: flatOpen.find((r) => r.id === "tmp")?.hasChildren === false && flatOpen.find((r) => r.id === "work")?.expanded === true });
  // 万节点平铺预算（并行测试负载下计时抖动——三次取最小，防环境噪声误报）
  const big: TreeNode = { id: "root", name: "root", children: Array.from({ length: 10000 }, (_, i) => ({ id: `n${i}`, name: `节点${i}` })) };
  let flatBig = 0;
  let bestMs = Infinity;
  for (let i = 0; i < 3; i++) {
    const t0 = Date.now();
    flatBig = flattenTree([big], new Set(["root"])).length;
    bestMs = Math.min(bestMs, Date.now() - t0);
  }
  checks.push({ name: "万节点平铺 <16ms", pass: flatBig === 10001 && bestMs < FLATTEN_BUDGET_MS });
  // 视口窗口
  const w1 = viewportWindow(10000, 0, 480, 24);
  checks.push({ name: "视口窗口含 overscan", pass: w1.start === 0 && w1.end === 20 + VIEWPORT_OVERSCAN });
  const w2 = viewportWindow(30, 5000, 480, 24);
  checks.push({ name: "越界夹取到末页", pass: w2.end === 30 && w2.start < 30 && w2.start >= 0 });
  checks.push({ name: "行落位", pass: rowTop(3) === 72 });
  // 双向同步
  const plan = planListToTree(["c", "work", "src"], new Set(["c", "work"]));
  checks.push({ name: "同步计划只展开缺的", pass: plan.toExpand.length === 1 && plan.toExpand[0] === "src" && plan.highlightId === "src" && plan.budgetMs === 100 });
  checks.push({ name: "树点击分流", pass: treeClickVerdict(true).toggleOnly === true && treeClickVerdict(false).refreshList === true });
  // 面包屑
  const crumbs: Array<Crumb> = [
    { id: "r", name: "此机" }, { id: "c", name: "本地磁盘(C:)" }, { id: "u", name: "用户" },
    { id: "v", name: "variable" }, { id: "p", name: "projects" }, { id: "x", name: "varix-engine" },
  ];
  const fitted = breadcrumbFit(crumbs, 18);
  checks.push({ name: "面包屑首尾保留中段省略", pass: fitted[0]!.name === "此机" && fitted[1]!.name === "…" && fitted[fitted.length - 1]!.name === "varix-engine" });
  checks.push({ name: "装得下不收缩", pass: breadcrumbFit(crumbs, 200).length === crumbs.length });
  // 键盘树导航
  const exp = new Set<string>();
  const flatShut = flattenTree(tree, new Set());
  const nav1 = treeKeyNav(flatShut, "c", "right", (id, on) => { if (on) exp.add(id); });
  checks.push({ name: "→ 展开未展开节点", pass: nav1.toggled && exp.has("c") });
  const nav2 = treeKeyNav(flatOpen, "tmp", "left", () => {});
  checks.push({ name: "← 折叠态回父", pass: nav2.focusId === "c" });
  const nav3 = treeKeyNav(flatOpen, "work", "right", () => {});
  checks.push({ name: "→ 已展开进子节点", pass: nav3.focusId === "src" });
  const nav4 = treeKeyNav(flatOpen, "d", "up", () => {});
  checks.push({ name: "↑ 行移动", pass: nav4.focusId === "tmp" });
  // 窗格布局
  checks.push({ name: "树宽夹取", pass: clampTreeWidth(100) === 140 && clampTreeWidth(600) === 480 && clampTreeWidth(260) === 260 });
  checks.push({ name: "状态栏高度同源", pass: EXP_STATUSBAR_HEIGHT_PX === 24 });
  return checks;
}
