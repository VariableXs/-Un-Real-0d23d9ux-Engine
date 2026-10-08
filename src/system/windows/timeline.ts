import type { VwmRect, VwmWin } from "./vwm";

/**
 * N-01 窗口时间机器（NEXT-40 · AI-2 窗口路）：
 * 把「窗口布局」变成可回放的历史——任意时刻的整套 VWM 窗口状态
 * （位置/层级/分屏态）可快照、命名、一键恢复。
 * - 快照契约：`TimelineSnap { ts, name?, vwm: TimelineWin[] }`
 *   （VWM 层快照；嵌入窗口恢复属 embed 域协作项，本模块仅承载引用字段）；
 * - 自动快照触发：布局变化去抖 8s；手动快照：「定格此刻」；
 * - 恢复策略三级：精确恢复（仅重摆位置——VWM 窗口恒在内存，均走此级）、
 *   尽力恢复（缺失窗口如实报告）、诚实报告（绝不静默失败）；
 * - 保留策略：上限 200 条，超限 FIFO 淘汰；快照损坏时跳过该条不影响其余。
 *
 * 持久化：localStorage（`variable:vwm:timeline`）——与 vwm.ts 既有几何持久化
 * 同一通道；落盘 data/timeline/*.vsnap 由后端批次接管时替换此实现。
 */

export interface TimelineWin {
  id: string;
  app: string;
  x: number;
  y: number;
  w: number;
  h: number;
  state: "normal" | "max";
  minimized: boolean;
  z: number;
}

export interface TimelineSnap {
  ts: number;
  name?: string;
  vwm: TimelineWin[];
}

const KEY = "variable:vwm:timeline";
/** 保留上限（条）。200MB 约束由后端接管时按字节计；前端按条数 FIFO。 */
export const TIMELINE_CAP = 200;
/** 自动快照去抖（ms）。 */
export const AUTO_DEBOUNCE_MS = 8000;

function loadRaw(): unknown {
  try {
    return JSON.parse(localStorage.getItem(KEY) ?? "[]");
  } catch {
    return [];
  }
}

/** 读取全部快照（损坏条目跳过，不影响其余——验收 ③）。 */
export function listSnaps(): TimelineSnap[] {
  const raw = loadRaw();
  if (!Array.isArray(raw)) return [];
  const out: TimelineSnap[] = [];
  for (const item of raw) {
    if (
      item &&
      typeof item === "object" &&
      typeof (item as TimelineSnap).ts === "number" &&
      Array.isArray((item as TimelineSnap).vwm)
    ) {
      out.push(item as TimelineSnap);
    }
  }
  return out.sort((a, b) => b.ts - a.ts); // 新→旧
}

function persistAll(snaps: TimelineSnap[]): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(snaps));
  } catch {
    /* storage full → 本次不持久，内存态仍可用 */
  }
}

/** 从 VWM 窗口数组生成快照（ts=0 表示由调用方注入；默认当前时刻）。 */
export function takeSnap(wins: VwmWin[], name?: string, now = Date.now()): TimelineSnap {
  return {
    ts: now,
    ...(name ? { name } : {}),
    vwm: wins.map((w) => ({
      id: w.id,
      app: w.app as string,
      x: w.x,
      y: w.y,
      w: w.w,
      h: w.h,
      state: w.state,
      minimized: w.minimized,
      z: w.z,
    })),
  };
}

/** 追加快照（FIFO 淘汰超限旧快照）。返回保留后的快照数。 */
export function pushSnap(snap: TimelineSnap): number {
  const all = listSnaps().reverse(); // 旧→新
  all.push(snap);
  while (all.length > TIMELINE_CAP) all.shift();
  persistAll(all);
  return all.length;
}

/** 手动快照入口（任务栏右键「定格此刻」）。 */
export function snapshotNow(wins: VwmWin[], name?: string): TimelineSnap {
  const snap = takeSnap(wins, name);
  pushSnap(snap);
  return snap;
}

// ---------- 自动快照去抖 ----------

let autoTimer: number | null = null;

/** 布局变化时调用（VWM store 订阅侧）：去抖 8s 后自动快照。 */
export function scheduleAutoSnap(wins: VwmWin[]): void {
  if (autoTimer !== null) window.clearTimeout(autoTimer);
  autoTimer = window.setTimeout(() => {
    autoTimer = null;
    pushSnap(takeSnap(wins));
  }, AUTO_DEBOUNCE_MS);
}

/** 取消挂起的自动快照（环境休眠前 / 测试用）。 */
export function cancelAutoSnap(): void {
  if (autoTimer !== null) {
    window.clearTimeout(autoTimer);
    autoTimer = null;
  }
}

// ---------- 恢复 ----------

export interface RestorePlan {
  /** 精确恢复：快照窗口仍在当前 VWM 中 → 目标几何（仅重摆位置，不改状态语义）。 */
  exact: TimelineWin[];
  /** 尽力恢复：快照窗口已不存在 → 如实列出（恢复执行器可按登记项重启，本层不代劳）。 */
  missing: string[];
}

/**
 * 生成恢复计划：精确/尽力/诚实报告三级。
 * VWM 虚拟窗口常驻内存，凡 id 仍存在走「精确恢复」（仅重摆位置/层级）；
 * id 不存在的列入 missing 由调用方决定是否走重启路径——绝不静默丢弃。
 */
export function planRestore(snap: TimelineSnap, currentWins: { id: string }[]): RestorePlan {
  const ids = new Set(currentWins.map((w) => w.id));
  const exact: TimelineWin[] = [];
  const missing: string[] = [];
  for (const w of snap.vwm) {
    if (ids.has(w.id)) exact.push(w);
    else missing.push(w.id);
  }
  return { exact, missing };
}

/** 精确恢复的目标几何（供 vwm 层应用；返回 id → rect/z 映射）。 */
export function restoreTargets(plan: RestorePlan): Record<string, Partial<VwmRect> & { z: number }> {
  const out: Record<string, Partial<VwmRect> & { z: number }> = {};
  for (const w of plan.exact) {
    out[w.id] = { x: w.x, y: w.y, w: w.w, h: w.h, z: w.z };
  }
  return out;
}

/** 清空时间线（设置页用；返回清除条数）。 */
export function clearTimeline(): number {
  const n = listSnaps().length;
  persistAll([]);
  return n;
}
