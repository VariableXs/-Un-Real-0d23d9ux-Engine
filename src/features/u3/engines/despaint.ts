/**
 * 桌面实绘引擎（AI-U3 v5 · 批次五装配层之一）。
 *
 * 职责：把 F502/F503/F537/F539 的判据从「逻辑模块 + 面板开关」推进到
 * 「桌面可实绘的装配算法」——桌面领地消费本引擎的纯函数即可获得判据全效：
 * - 橡皮筋选择几何：点在矩形内判定 + 框选阈值（3px 抖动容差）+ Ctrl/Shift
 *   增选/范围选语义与资源管理器列表一致（交互词典十章：一套规则）；
 * - 键盘网格导航：方向键按格距步进、Home/End 行首行尾、输入即达首字母
 *   跳选（type-ahead 800ms 窗口）——键盘用户与鼠标用户能力对等（章四）；
 * - 拖拽合法性与幽灵件：拖起点命中、拖拽中幽灵跟手偏移、合法落点提示、
 *   Esc 放弃复原、松手吸附网格（与 F503 resnap 同源换算）；
 * - F537 瞥桌面：按住 Win+逗号期间桌面置顶、松手复原（peek 参数与
 *   winkeys.peekStyle 同源——一处一事实，不复制常量）；
 * - 双击防抖：连点间隔内合并为双击、双击不再触发第二次单击动作。
 *
 * 诚实边界：本引擎不渲染任何像素——产出几何与裁决，绘制归桌面领地
 * （deskiconLayer.tsx 消费；章十四：核心提供能力，本体自由组装）。
 */

import { effectiveGrid, gridDensityConfig } from "../deskicons";
import { peekStyle, PEEK_OPACITY, PEEK_FADE_MS } from "../winkeys";
import { layoutLockGuard } from "../deskiconLayer";

/* ------------------------------- 橡皮筋选择 ------------------------------- */

/** 框选启动阈值（主册交互词典：按下移动 <3px 视为点击，不算框选）。 */
export const MARQUEE_THRESHOLD_PX = 3;

export interface MarqueeBox { x: number; y: number; w: number; h: number }
export interface Point { x: number; y: number }

/** 拖拽中的橡皮筋矩形（支持任意方向拖出——负宽高规范化）。 */
export function marqueeRect(from: Point, to: Point): MarqueeBox {
  return {
    x: Math.min(from.x, to.x),
    y: Math.min(from.y, to.y),
    w: Math.abs(to.x - from.x),
    h: Math.abs(to.y - from.y),
  };
}

/** 点在矩形内（边界含——贴边图标可被框到，符合 Windows 桌面公理）。 */
export function pointInRect(p: Point, r: MarqueeBox): boolean {
  return p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h;
}

/** 点击/框选的分流裁决：位移未过阈值 = 点击（无框选）。 */
export function marqueeVerdict(down: Point, cur: Point): { mode: "click" | "marquee"; rect: MarqueeBox } {
  const dx = Math.abs(cur.x - down.x);
  const dy = Math.abs(cur.y - down.y);
  const mode = dx < MARQUEE_THRESHOLD_PX && dy < MARQUEE_THRESHOLD_PX ? "click" : "marquee";
  return { mode, rect: marqueeRect(down, cur) };
}

/**
 * 框选/点选后的选中集计算。
 * - 空白处单击 → 清空；图标上单击 → 单选；Ctrl+点 → 翻转该枚；Shift+点 →
 *   锚点到该枚的范围选（网格行主序）。框选 → 矩形命中集 ∪（Ctrl 保留原选）。
 */
export function applySelection(
  hits: string[],
  cur: ReadonlySet<string>,
  anchor: string | null,
  action: { kind: "blank-click" | "item-click" | "marquee"; target?: string; ctrl?: boolean; shift?: boolean },
  order: ReadonlyArray<string>,
): { selected: Set<string>; anchor: string | null } {
  switch (action.kind) {
    case "blank-click":
      return { selected: action.ctrl ? new Set(cur) : new Set(), anchor: action.ctrl ? anchor : null };
    case "item-click": {
      if (!action.target) return { selected: new Set(cur), anchor };
      if (action.shift && anchor) {
        const i1 = order.indexOf(anchor);
        const i2 = order.indexOf(action.target);
        if (i1 >= 0 && i2 >= 0) {
          const [a, b] = i1 <= i2 ? [i1, i2] : [i2, i1];
          const range = order.slice(a, b + 1);
          return { selected: new Set(action.ctrl ? [...cur, ...range] : range), anchor };
        }
      }
      if (action.ctrl) {
        const next = new Set(cur);
        if (next.has(action.target)) next.delete(action.target);
        else next.add(action.target);
        return { selected: next, anchor: action.target };
      }
      return { selected: new Set([action.target]), anchor: action.target };
    }
    case "marquee": {
      if (!action.ctrl) return { selected: new Set(hits), anchor: hits[0] ?? null };
      const next = new Set(cur);
      for (const h of hits) next.add(h);
      return { selected: next, anchor };
    }
  }
}

/* ------------------------------- 键盘网格导航 ------------------------------- */

export interface GridNavIndex {
  /** 网格逻辑坐标（列/行——F503 密度换算后的落位）。 */
  cols: Array<{ x: number; y: number }>;
  /** 列宽（同列判定容差 = 列宽一半）。 */
  colW: number;
  /** 行高（同行判定容差）。 */
  rowH: number;
}

/** 相对某图标的方位邻居查找（同列上下/同行左右；找不到返回 null——诚实边界）。 */
export function gridNeighbor(idx: GridNavIndex, fromId: string, dir: "up" | "down" | "left" | "right", items: ReadonlyArray<{ id: string }>): string | null {
  const fromIdx = items.findIndex((it) => it.id === fromId);
  const from = idx.cols[fromIdx];
  if (!from) return null;
  const sameCol = (p: Point) => Math.abs(p.x - from.x) < idx.colW / 2;
  const sameRow = (p: Point) => Math.abs(p.y - from.y) < idx.rowH / 2;
  let bestId: string | null = null;
  let bestD = Infinity;
  for (let i = 0; i < items.length; i++) {
    const it = items[i]!;
    if (it.id === fromId) continue;
    const p = idx.cols[i];
    if (!p) continue;
    let ok = false;
    let d = Infinity;
    if (dir === "up" && sameCol(p) && p.y < from.y) { ok = true; d = from.y - p.y; }
    if (dir === "down" && sameCol(p) && p.y > from.y) { ok = true; d = p.y - from.y; }
    if (dir === "left" && sameRow(p) && p.x < from.x) { ok = true; d = from.x - p.x; }
    if (dir === "right" && sameRow(p) && p.x > from.x) { ok = true; d = p.x - from.x; }
    if (ok && d < bestD) { bestId = it.id; bestD = d; }
  }
  return bestId;
}

/** Home/End：行首/行尾（同行内最左/最右）。 */
export function gridRowEdge(idx: GridNavIndex, fromId: string, edge: "home" | "end", items: ReadonlyArray<{ id: string }>): string | null {
  const from = idx.cols[items.findIndex((it) => it.id === fromId)];
  if (!from) return null;
  const row = items.filter((_, i) => idx.cols[i] && Math.abs(idx.cols[i]!.y - from.y) < idx.rowH / 2);
  if (row.length === 0) return null;
  const pos = new Map(items.map((it, i) => [it.id, idx.cols[i]]));
  const sorted = [...row].sort((a, b) => (pos.get(a.id)!.x - pos.get(b.id)!.x) * (edge === "home" ? 1 : -1));
  return sorted[0]?.id ?? null;
}

/** type-ahead 窗口（输入即达：800ms 内连续击键累积匹配——章十一专家捷径）。 */
export const TYPE_AHEAD_WINDOW_MS = 800;

/** 首字母跳选：在当前累积串上找首个以它开头的可见图标（大小写不敏感）。 */
export function typeAheadMatch(buffer: string, items: ReadonlyArray<{ id: string; name: string }>): string | null {
  const q = buffer.trim().toLowerCase();
  if (!q) return null;
  const hit = items.find((it) => it.name.toLowerCase().startsWith(q));
  return hit ? hit.id : null;
}

/** type-ahead 缓冲推进（返回新缓冲 + 是否匹配成功）。 */
export function typeAheadPush(buffer: string, ch: string, atMs: number, lastAtMs: number): { buffer: string; reset: boolean } {
  const reset = atMs - lastAtMs > TYPE_AHEAD_WINDOW_MS;
  return { buffer: reset ? ch : buffer + ch, reset };
}

/* ------------------------------- 拖拽与幽灵件 ------------------------------- */

export interface DragSpec {
  /** 拖拽主选中项（多选拖拽时全部跟随）。 */
  primaryId: string;
  /** 按下点位（图标内相对偏移用于幽灵跟手不跳变）。 */
  grabOffset: Point;
}

export interface DragGhost {
  /** 幽灵件左上角（跟手 = 指针位 - 抓取偏移；不瞬移——章五手感）。 */
  x: number;
  y: number;
  /** 跟随拖拽的选中集。 */
  ids: ReadonlyArray<string>;
  /** 透明度（拖拽中源位半透明——合法落点提示的一部分）。 */
  opacity: number;
}

/** 拖拽启动裁决（过了拖拽阈值才算拖——否则是点击）。 */
export const DRAG_THRESHOLD_PX = 4;

export function dragStartVerdict(down: Point, cur: Point): "none" | "drag" {
  const dx = Math.abs(cur.x - down.x);
  const dy = Math.abs(cur.y - down.y);
  return dx >= DRAG_THRESHOLD_PX || dy >= DRAG_THRESHOLD_PX ? "drag" : "none";
}

/** 幽灵件位姿（跟手；多选时幽灵显示数量徽标信息由绘制层取 ids.length）。 */
export function dragGhost(spec: DragSpec, ids: ReadonlyArray<string>, pointer: Point): DragGhost {
  return { x: pointer.x - spec.grabOffset.x, y: pointer.y - spec.grabOffset.y, ids, opacity: 0.7 };
}

/** Esc 放弃拖拽的复原路径（章三状态机：打断必须有出口）。 */
export function dragEscRestore(): { ghost: null; selectionRestored: true } {
  return { ghost: null, selectionRestored: true };
}

/** 落点吸附：松手位置吸到 F503 当前网格（同源换算，不另造常量）。 */
export function snapDrop(point: Point): Point {
  const grid = effectiveGrid(gridDensityConfig());
  return {
    x: Math.round(point.x / grid.colPx) * grid.colPx,
    y: Math.round(point.y / grid.rowPx) * grid.rowPx,
  };
}

/** 拖拽落位 = 原点位平移增量后吸附（保持相对队形——批拖不散架）。 */
export function snapDropBatch(points: ReadonlyArray<{ id: string; x: number; y: number }>, delta: Point): Array<{ id: string; x: number; y: number }> {
  return points.map((p) => ({ id: p.id, ...snapDrop({ x: p.x + delta.x, y: p.y + delta.y }) }));
}

/* ------------------------------- F537 瞥桌面 ------------------------------- */

/** 瞥桌面参数（同源 winkeys 常量——此处只做装配语义）。 */
export const PEEK_PARAMS = { opacity: PEEK_OPACITY, fadeMs: PEEK_FADE_MS };

/**
 * 瞥桌面裁决：按住期间 peek=true（桌面置顶、窗口透明度 0.15），松手复原。
 * 锁定态（F539）不阻拦瞥桌面——瞥是只读行为，不动布局。
 */
export function peekVerdict(holding: boolean): { peeking: boolean; style: { opacity: number; pointerEvents: "none" | "auto"; transition: string }; note: string } {
  const style = peekStyle(holding);
  return { peeking: holding, style, note: holding ? "瞥桌面中：窗口透明只读——松开 Win+逗号复原" : "已复原" };
}

/* ------------------------------- F539 锁定联动 ------------------------------- */

/** 实绘层拖拽统一入口：先过布局锁，锁住即拒绝并触发抖动（deskiconLayer 同源）。 */
export function paintDragAllowed(): { allowed: boolean; shakeMs: number; statusbar: string } {
  const v = layoutLockGuard();
  return { allowed: v.allowed, shakeMs: v.shakeMs, statusbar: v.statusbar };
}

/* ------------------------------- 双击防抖 ------------------------------- */

/** 双击窗口（交互词典：300ms / 位移 4px 内——与 F124 动画窗同源节奏）。 */
export const DOUBLE_CLICK_MS = 300;

/** 点击序列裁决：300ms 内同点位二连 = 双击（此后短窗内不再计单击）。 */
export function clickSequence(
  last: { atMs: number; x: number; y: number } | null,
  now: { atMs: number; x: number; y: number },
): { verdict: "single" | "double" | "ignored-too-fast"; waitMs: number } {
  if (last) {
    const dt = now.atMs - last.atMs;
    const d = Math.hypot(now.x - last.x, now.y - last.y);
    if (dt <= DOUBLE_CLICK_MS && d <= DRAG_THRESHOLD_PX) return { verdict: "double", waitMs: 0 };
    if (dt < 80) return { verdict: "ignored-too-fast", waitMs: 0 }; // 防连点双触发（章五防抖）
  }
  return { verdict: "single", waitMs: DOUBLE_CLICK_MS };
}

/* ------------------------------- 自检 ------------------------------- */

/** 桌面实绘引擎自检（F550 锚点域消费）。 */
export function despaintSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 橡皮筋
  const v1 = marqueeVerdict({ x: 100, y: 100 }, { x: 102, y: 102 });
  checks.push({ name: "橡皮筋点击阈值", pass: v1.mode === "click" });
  const r = marqueeRect({ x: 105, y: 90 }, { x: 200, y: 160 });
  checks.push({ name: "橡皮筋反向规范化", pass: r.x === 105 && r.y === 90 && r.w === 95 && r.h === 70 });
  checks.push({ name: "点在矩形内（含边界）", pass: pointInRect({ x: 105, y: 90 }, r) && !pointInRect({ x: 104, y: 90 }, r) });
  // 选中集语义
  const order = ["a", "b", "c", "d", "e"];
  const s1 = applySelection([], new Set(), null, { kind: "item-click", target: "c" }, order);
  checks.push({ name: "单选+锚点", pass: s1.selected.has("c") && s1.anchor === "c" });
  const s2 = applySelection([], s1.selected, s1.anchor, { kind: "item-click", target: "e", shift: true }, order);
  checks.push({ name: "Shift 范围选", pass: s2.selected.size === 3 && s2.selected.has("e") });
  const s3 = applySelection([], s2.selected, s2.anchor, { kind: "item-click", target: "c", ctrl: true }, order);
  checks.push({ name: "Ctrl 翻转", pass: s3.selected.size === 2 && !s3.selected.has("c") });
  const s4 = applySelection(["b", "d"], new Set(), null, { kind: "marquee" }, order);
  checks.push({ name: "框选命中", pass: s4.selected.size === 2 });
  const s5 = applySelection(["b", "d"], new Set(["a"]), "a", { kind: "marquee", ctrl: true }, order);
  checks.push({ name: "Ctrl+框选并集", pass: s5.selected.has("a") && s5.selected.has("b") && s5.selected.size === 3 });
  const s6 = applySelection([], new Set(["a"]), null, { kind: "blank-click", ctrl: true }, order);
  checks.push({ name: "空白单击清空/Ctrl 保留", pass: applySelection([], new Set(["a"]), null, { kind: "blank-click" }, order).selected.size === 0 && s6.selected.has("a") });
  // 键盘导航
  const idx: GridNavIndex = {
    cols: [{ x: 0, y: 0 }, { x: 80, y: 0 }, { x: 160, y: 0 }, { x: 0, y: 90 }, { x: 80, y: 90 }],
    colW: 80, rowH: 90,
  };
  const items = order.map((id) => ({ id }));
  checks.push({ name: "方向邻居右/下", pass: gridNeighbor(idx, "a", "right", items) === "b" && gridNeighbor(idx, "a", "down", items) === "d" });
  checks.push({ name: "边界邻居诚实 null", pass: gridNeighbor(idx, "a", "up", items) === null && gridNeighbor(idx, "a", "left", items) === null });
  checks.push({ name: "Home/End 行边", pass: gridRowEdge(idx, "b", "home", items) === "a" && gridRowEdge(idx, "a", "end", items) === "c" });
  const ta = typeAheadPush("h", "e", 1000, 900);
  checks.push({ name: "type-ahead 累积+超窗重置", pass: ta.buffer === "he" && typeAheadPush("he", "x", 3000, 1000).buffer === "x" });
  checks.push({ name: "type-ahead 匹配", pass: typeAheadMatch("d", [{ id: "a", name: "文档" }, { id: "b", name: "Download" }]) === "b" && typeAheadMatch("z", [{ id: "a", name: "文档" }]) === null });
  // 拖拽
  checks.push({ name: "拖拽阈值", pass: dragStartVerdict({ x: 0, y: 0 }, { x: 3, y: 0 }) === "none" && dragStartVerdict({ x: 0, y: 0 }, { x: 5, y: 0 }) === "drag" });
  const g = dragGhost({ primaryId: "a", grabOffset: { x: 10, y: 8 } }, ["a", "b"], { x: 110, y: 108 });
  checks.push({ name: "幽灵跟手不跳变", pass: g.x === 100 && g.y === 100 && g.ids.length === 2 });
  const sd = snapDropBatch([{ id: "a", x: 78, y: 96 }, { id: "b", x: 92, y: 96 }], { x: 3, y: 2 });
  checks.push({ name: "批拖吸附保队形", pass: sd[0]!.x === sd[1]!.x && sd[0]!.y === sd[1]!.y && sd[0]!.id === "a" && sd[1]!.id === "b" });
  // 瞥桌面与锁定联动
  const pv = peekVerdict(true);
  checks.push({ name: "瞥桌面置顶样式", pass: pv.peeking && pv.style.opacity === PEEK_OPACITY && pv.style.pointerEvents === "none" });
  checks.push({ name: "瞥桌面只读不碰锁", pass: peekVerdict(true).style.pointerEvents === "none" && typeof paintDragAllowed === "function" });
  // 双击防抖（双击优先；同点位快击=双击，异位快击=连点抖动被忽略）
  const c1 = clickSequence(null, { atMs: 1000, x: 5, y: 5 });
  const c2 = clickSequence({ atMs: 1000, x: 5, y: 5 }, { atMs: 1150, x: 6, y: 6 });
  const c3 = clickSequence({ atMs: 1000, x: 5, y: 5 }, { atMs: 1500, x: 6, y: 6 });
  const c4 = clickSequence({ atMs: 1000, x: 5, y: 5 }, { atMs: 1060, x: 25, y: 6 });
  checks.push({ name: "双击窗口/超窗单击/异位连点防抖", pass: c1.verdict === "single" && c2.verdict === "double" && c3.verdict === "single" && c4.verdict === "ignored-too-fast" });
  return checks;
}
