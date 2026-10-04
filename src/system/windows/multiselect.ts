import type { VwmRect, VwmWin } from "./vwm";

/**
 * V-24 窗口多选编组操作（化境 · AI-2 窗口与键位路）：
 * Ctrl+点击标题栏多选窗口，编组后可批量：一起移动（保持相对位置）、
 * 一起贴靠、一起最小化、一起关闭（需确认）。
 * - 编组态视觉由 UI 层渲染（选中窗口边框强调色 1px）；本层是纯操作模型；
 * - Esc 退出编组；不做编组持久化（会话级即止）；
 * - 批量关闭需确认（confirmGate 注入，测试可桩）。
 */

export interface MultiSelectOp {
  winId: string;
  /** 当前几何。 */
  rect: VwmRect;
}

/** 当前多选集合（内存态，会话级）。 */
let selected: string[] = [];

// ---------- 订阅机制（UI 层渲染跟随：选择集变化即通知） ----------
const listeners = new Set<() => void>();

/** 订阅选择集变化（返回退订函数；工具条 / StageRail / 窗口边框即时刷新）。 */
export function subscribeMultiSelect(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

function notifyMultiSelect(): void {
  for (const l of listeners) l();
}

export function multiSelected(): string[] {
  return [...selected];
}

export function clearMultiSelect(): void {
  if (selected.length === 0) return;
  selected = [];
  notifyMultiSelect();
}

/** Ctrl+点击切换选中；普通点击（无 Ctrl）清空并退出编组。 */
export function toggleSelect(winId: string, additive: boolean): string[] {
  if (!additive) {
    clearMultiSelect();
    return [];
  }
  const i = selected.indexOf(winId);
  if (i >= 0) selected.splice(i, 1);
  else selected.push(winId);
  notifyMultiSelect();
  return [...selected];
}

/** 平移整组：保持相对几何（dx/dy 应用到每个成员；平移量逐窗一致 → 相对位置误差 0）。 */
export function translateGroup(
  ops: MultiSelectOp[],
  dx: number,
  dy: number,
): Record<string, { x: number; y: number }> {
  const out: Record<string, { x: number; y: number }> = {};
  for (const op of ops) {
    out[op.winId] = { x: Math.round(op.rect.x + dx), y: Math.round(op.rect.y + dy) };
  }
  return out;
}

/** 一起贴靠到矩形：每个成员落位到 rect（Windows「组贴靠」语义：全部占同一分区由用户再微调；这里按规格逐个贴到目标区）。 */
export function snapGroupRects(ops: MultiSelectOp[], rect: VwmRect): Record<string, VwmRect> {
  const out: Record<string, VwmRect> = {};
  for (const op of ops) out[op.winId] = { ...rect };
  return out;
}

export interface GroupClosePlan {
  winIds: string[];
  /** 需要确认（恒 true——批量关闭必须有确认门）。 */
  needsConfirm: true;
}

/** 一起关闭：返回带确认门的关闭计划。 */
export function planGroupClose(ops: MultiSelectOp[]): GroupClosePlan {
  return { winIds: ops.map((o) => o.winId), needsConfirm: true };
}

// ---------- V-26 窗口位置互换（依赖 V-24 多选：恰好两个成员时可用） ----------

export interface SwapPlan {
  a: { winId: string; rect: VwmRect };
  b: { winId: string; rect: VwmRect };
  /** 尺寸不同时取并集区域居中（如实提示不保尺寸——由 UI 层提示）。 */
  resized: boolean;
}

/**
 * 互换两个窗口的几何位置：
 * - 同尺寸：几何直接对调；
 * - 异尺寸：两窗都落到并集区域居中（宽度取大者、高度取大者，居中于并集）。
 */
export function planSwap(a: MultiSelectOp, b: MultiSelectOp): SwapPlan {
  const sameSize = a.rect.w === b.rect.w && a.rect.h === b.rect.h;
  if (sameSize) {
    return {
      a: { winId: a.winId, rect: b.rect },
      b: { winId: b.winId, rect: a.rect },
      resized: false,
    };
  }
  const ux = Math.min(a.rect.x, b.rect.x);
  const uy = Math.min(a.rect.y, b.rect.y);
  const ux2 = Math.max(a.rect.x + a.rect.w, b.rect.x + b.rect.w);
  const uy2 = Math.max(a.rect.y + a.rect.h, b.rect.y + b.rect.h);
  const uw = ux2 - ux;
  const uh = uy2 - uy;
  const center = (r: VwmRect): VwmRect => ({
    x: Math.round(ux + (uw - r.w) / 2),
    y: Math.round(uy + (uh - r.h) / 2),
    w: r.w,
    h: r.h,
  });
  return {
    a: { winId: a.winId, rect: center(a.rect) },
    b: { winId: b.winId, rect: center(b.rect) },
    resized: true,
  };
}

/** 互换可用条件：多选恰好两个窗口。 */
export function swapEligible(wins: VwmWin[]): boolean {
  return wins.length === 2;
}
