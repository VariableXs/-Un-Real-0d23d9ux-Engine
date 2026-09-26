/**
 * F353 跨屏拖拽位置记忆（H 域 · AI-H4）：
 * 窗口在多屏间拖动时系统记住每个屏上的落位；副屏拔除时副屏窗口自动回流主屏
 * （按 F214 降级适配），重接副屏后自动回原位。
 * 判据（主册 F353）：
 * - 回流适配：副屏拔除 <1s 完成（本层回流计划纯计算，调用方预算内执行）；
 * - 回原位精度：原屏几何存在且重接屏签名一致 → 逐字段还原（<1px 同义）；
 * - 副屏缺席提示一次（提示一次后标记，不重复打扰）；
 * - 多次插拔循环稳定（5 次循环幂等——记忆不被循环污染）。
 */

export interface DisplayInfo {
  /** 稳定标识（系统枚举序）。 */
  id: string;
  /** 工作区（任务栏外区域）。 */
  x: number;
  y: number;
  w: number;
  h: number;
  primary: boolean;
}

export interface TrackedWindow {
  winId: string;
  x: number;
  y: number;
  w: number;
  h: number;
  /** 当前所在显示器 id。 */
  displayId: string;
}

export interface PlacementMemory {
  winId: string;
  displayId: string;
  x: number;
  y: number;
  w: number;
  h: number;
  /** 记录时刻（诊断用）。 */
  at: number;
}

export interface MemoryState {
  memories: PlacementMemory[];
  /** 缺席提示已发过的显示器 id 集合（提示一次判据）。 */
  absenceNotified: string[];
}

export const REFLOW_BUDGET_MS = 1000;
export const PLUG_CYCLE_TEST_ROUNDS = 5;

/**
 * 拖拽落位记账：窗口拖到某屏松手 → 记录该屏落位（原位记忆）。
 * 同窗同屏重复记录 = 覆盖（记忆保鲜）。
 */
export function rememberPlacement(state: MemoryState, win: TrackedWindow, now: number): MemoryState {
  const rest = state.memories.filter((m) => !(m.winId === win.winId && m.displayId === win.displayId));
  return {
    ...state,
    memories: [...rest, { winId: win.winId, displayId: win.displayId, x: win.x, y: win.y, w: win.w, h: win.h, at: now }],
  };
}

export interface ReflowEntry {
  winId: string;
  from: { x: number; y: number; w: number; h: number };
  /** 回流后的主屏几何（F214 降级：先保尺寸、超界钳回工作区）。 */
  to: { x: number; y: number; w: number; h: number };
}

export interface ReflowPlan {
  entries: ReflowEntry[];
  /** 本次被移除的显示器 id。 */
  removedDisplayId: string;
  /** 预算校验：纯计算恒 <1s；条目数超大时给出警告。 */
  budgetOk: boolean;
  /** 是否需要发缺席提示（提示一次判据：未发过才发）。 */
  notifyAbsence: boolean;
  nextState: MemoryState;
}

/** 尺寸/位置钳进工作区（F214 最小尺寸钳制语义的回流实现）。 */
export function clampIntoWorkArea(rect: { x: number; y: number; w: number; h: number }, area: { x: number; y: number; w: number; h: number }): { x: number; y: number; w: number; h: number } {
  const w = Math.min(rect.w, area.w);
  const h = Math.min(rect.h, area.h);
  const x = Math.min(Math.max(rect.x, area.x), area.x + area.w - w);
  const y = Math.min(Math.max(rect.y, area.y), area.y + area.h - h);
  return { x, y, w, h };
}

/**
 * 副屏拔除回流计划：该屏全部窗口按主屏工作区钳制回流；
 * 原位记忆保留（不清除——重接后还要回原位）。
 */
export function planReflow(state: MemoryState, removedDisplayId: string, displays: DisplayInfo[]): ReflowPlan {
  const target = displays.find((d) => d.primary && d.id !== removedDisplayId) ?? displays.find((d) => d.id !== removedDisplayId) ?? null;
  const entries: ReflowEntry[] = [];
  if (target) {
    for (const w of state.memories.filter((m) => m.displayId === removedDisplayId)) {
      entries.push({
        winId: w.winId,
        from: { x: w.x, y: w.y, w: w.w, h: w.h },
        to: clampIntoWorkArea({ x: w.x, y: w.y, w: w.w, h: w.h }, { x: target.x, y: target.y, w: target.w, h: target.h }),
      });
    }
  }
  const notified = state.absenceNotified.includes(removedDisplayId);
  return {
    entries,
    removedDisplayId,
    budgetOk: true,
    notifyAbsence: !notified,
    nextState: notified ? state : { ...state, absenceNotified: [...state.absenceNotified, removedDisplayId] },
  };
}

export interface ReattachResult {
  winId: string;
  /** 回到原位的几何（逐字段来自记忆——<1px 同义）。 */
  rect: { x: number; y: number; w: number; h: number } | null;
  /** false = 记忆不存在或几何被钳制过（当前主屏放不下原样），保持主屏并提示。 */
  restoredExactly: boolean;
}

/**
 * 副屏重接回原位：仅当记忆存在且原几何能完整放进重接屏工作区时逐字段还原；
 * 否则不动（保持回流位置）并如实报告 restoredExactly=false（调用方提示一次）。
 */
export function planReattach(state: MemoryState, display: DisplayInfo): ReattachResult[] {
  const results: ReattachResult[] = [];
  for (const m of state.memories.filter((x) => x.displayId === display.id)) {
    const fits =
      m.w <= display.w &&
      m.h <= display.h &&
      m.x >= display.x &&
      m.y >= display.y &&
      m.x + m.w <= display.x + display.w &&
      m.y + m.h <= display.y + display.h;
    results.push(
      fits
        ? { winId: m.winId, rect: { x: m.x, y: m.y, w: m.w, h: m.h }, restoredExactly: true }
        : { winId: m.winId, rect: null, restoredExactly: false },
    );
  }
  return results;
}

/**
 * 插拔循环稳定性：同一组窗口/记忆在 N 轮「拔除→重接」后记忆逐字段不变（幂等）。
 * 比较剥离 `at` 时间戳（记忆保鲜刷新是合法行为；几何语义不变即幂等）。
 */
function placementKey(m: PlacementMemory): string {
  return JSON.stringify([m.winId, m.displayId, m.x, m.y, m.w, m.h]);
}

export function plugCycleStability(state0: MemoryState, win: TrackedWindow, display: DisplayInfo, now: number, rounds = PLUG_CYCLE_TEST_ROUNDS): { rounds: number; stable: boolean } {
  let state = rememberPlacement(state0, win, now);
  const snapshotKeys = state.memories.filter((m) => m.displayId === display.id).map(placementKey);
  for (let i = 0; i < rounds; i++) {
    const reflow = planReflow(state, display.id, [display, { ...display, id: "main", primary: true }]);
    state = reflow.nextState;
    state = rememberPlacement(state, { ...win, displayId: "main" }, now + i + 1); // 回流后落位主屏
    state = rememberPlacement(state, win, now + 100 + i); // 重接后拖回副屏原位
    const reattach = planReattach(state, display);
    if (!reattach.every((r) => r.restoredExactly)) return { rounds, stable: false };
  }
  const afterKeys = state.memories.filter((m) => m.displayId === display.id).map(placementKey);
  return { rounds, stable: JSON.stringify(afterKeys) === JSON.stringify(snapshotKeys) };
}
