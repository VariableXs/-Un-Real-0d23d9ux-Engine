/**
 * F379 表头排序指示（H 域 · AI-H4）：
 * 表头点击排序三态循环：升序（▲）/降序（▼）/取消排序（回默认序）——指示箭头在
 * 表头文字右侧 4px、当前排序列表头加粗；Shift+点击=多级排序（先按日期再按名称，
 * 最多三级，指示带 1/2/3 序号）；排序变更即时反映且滚动位置保持（不跳回顶部）。
 * 判据（主册 F379）：三态循环；多级排序指示与上限；滚动位置保持判据；与 F219 记忆联动；
 * 即时性 <100ms。
 * 依赖锚点：F219 排序与视图记忆。
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

/** 箭头与文字间距（判据 4px）。 */
export const ARROW_GAP_PX = 4;
/** 多级排序上限（判据：最多三级）。 */
export const MAX_SORT_LEVELS = 3;
/** 排序变更即时性预算（判据 <100ms）。 */
export const APPLY_BUDGET_MS = 100;

export type SortDir = "asc" | "desc";

export interface SortKey {
  columnId: string;
  dir: SortDir;
}

export interface HeaderSortState {
  /** 有序多级排序键（[0] 为主序）。 */
  keys: SortKey[];
}

export function initialSort(): HeaderSortState {
  return { keys: [] };
}

/**
 * 点击表头三态循环（判据）：
 * - 未排 → 升序；升序 → 降序；降序 → 取消（回默认序）。
 * - Shift+点击 = 加入/更新多级尾键（不取消）；超三级淘汰最旧尾键。
 */
export function clickHeader(state: HeaderSortState, columnId: string, shift: boolean): HeaderSortState {
  if (!shift) {
    const cur = state.keys.find((k) => k.columnId === columnId);
    if (!cur) return { keys: [{ columnId, dir: "asc" }] };
    if (cur.dir === "asc") return { keys: [{ columnId, dir: "desc" }] };
    return { keys: [] };
  }
  const existing = state.keys.find((k) => k.columnId === columnId);
  if (!existing) {
    const keys = [...state.keys, { columnId, dir: "asc" as SortDir }];
    return { keys: keys.length > MAX_SORT_LEVELS ? keys.slice(keys.length - MAX_SORT_LEVELS) : keys };
  }
  if (existing.dir === "asc") {
    return { keys: state.keys.map((k) => (k.columnId === columnId ? { ...k, dir: "desc" } : k)) };
  }
  return { keys: state.keys.filter((k) => k.columnId !== columnId) };
}

export interface SortIndicator {
  /** ▲ / ▼ / null（未排）。 */
  arrow: "▲" | "▼" | null;
  /** 多级序号（主序=1；单级排序不显示序号）。 */
  level: number | null;
  /** 当前排序列表头加粗。 */
  bold: boolean;
}

/** 指示模型（判据「箭头+加粗+多级序号」）。 */
export function indicatorFor(state: HeaderSortState, columnId: string): SortIndicator {
  const idx = state.keys.findIndex((k) => k.columnId === columnId);
  if (idx < 0) return { arrow: null, level: null, bold: false };
  return { arrow: state.keys[idx]!.dir === "asc" ? "▲" : "▼", level: state.keys.length > 1 ? idx + 1 : null, bold: true };
}

/** 滚动位置保持（判据）：排序变更不改滚动偏移——本函数恒等透传（结构上无重置路径）。 */
export function scrollAfterSort(scrollTop: number): number {
  return scrollTop;
}

/** 即时性审计（判据 <100ms）。 */
export function withinApplyBudget(appliedAtMs: number, clickedAtMs: number): boolean {
  return appliedAtMs - clickedAtMs < APPLY_BUDGET_MS;
}

/* ---------- F219 记忆联动 ---------- */

const KEY = h4Key("f379", "sort");

/** 记忆当前排序（F219 同源——重启后保持）。 */
export function persistSort(state: HeaderSortState, store: KvStore = defaultStore()): boolean {
  return writeJson(store, KEY, state.keys);
}

export function loadSort(store: KvStore = defaultStore()): HeaderSortState {
  return { keys: readJson<SortKey[]>(store, KEY, [], Array.isArray).slice(0, MAX_SORT_LEVELS) };
}

/** 排序执行（比较器工厂：逐级比较；同值保持原序——稳定排序判据）。 */
export function comparator<T>(keys: SortKey[], field: (row: T, columnId: string) => number | string): (a: T, b: T) => number {
  return (a, b) => {
    for (const k of keys) {
      const va = field(a, k.columnId);
      const vb = field(b, k.columnId);
      if (va === vb) continue;
      const cmp = typeof va === "number" && typeof vb === "number" ? va - vb : String(va).localeCompare(String(vb), "zh-Hans-CN");
      return k.dir === "asc" ? cmp : -cmp;
    }
    return 0;
  };
}
