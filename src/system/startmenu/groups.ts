import type { RecentEntry } from "./recent";
import { usageCounts } from "./usage";

/**
 * 化境 V-12（车道 S）：开始菜单顶部两个自动分组的数据逻辑。
 * - 「最近添加」= 第三方 addedAt 近 14 天 + 最近使用记录中近 14 天的条目（去重）
 * - 「高频」= 本地使用计数 Top N（默认 8），默认关（localStorage 开关）
 * 数据只读消费：第三方登记（useThirdApps）与 recent.ts（useRecent），零网络。
 */

export const RECENT_DAYS = 14;
export const RECENT_WINDOW_MS = RECENT_DAYS * 24 * 60 * 60 * 1000;
export const HIGH_FREQ_TOP_N = 8;

export interface RecentAddedEntry {
  /** 网格项 id（app-* / sys-* / tool-* / tp-*），供点击直达。 */
  gridId: string;
  label: string;
  ts: number;
}

/** recent.ts 的 kind/id → 开始菜单网格项 id（kind "sys" 含工具 tool-* 特例）。 */
export function recentGridId(kind: RecentEntry["kind"], id: string): string {
  if (kind === "app") return `app-${id}`;
  if (kind === "tp") return `tp-${id}`;
  return id.startsWith("tool-") ? id : `sys-${id}`;
}

/**
 * 「最近添加」：第三方登记近 14 天（addedAt）∪ 最近使用近 14 天，按 gridId 去重
 * （tp 同时命中时保留 addedAt 版本——安装时间比使用时间更贴近「添加」语义）。
 * 纯函数，now 显式传入便于测试。
 */
export function recentlyAdded(
  thirds: { id: string; name: string; addedAt: number }[],
  recents: RecentEntry[],
  now: number,
  windowMs = RECENT_WINDOW_MS,
): RecentAddedEntry[] {
  const out = new Map<string, RecentAddedEntry>();
  for (const a of thirds) {
    if (!a.addedAt || now - a.addedAt > windowMs || now < a.addedAt) continue;
    out.set(`tp-${a.id}`, { gridId: `tp-${a.id}`, label: a.name, ts: a.addedAt });
  }
  const tpIds = new Set(thirds.map((a) => a.id));
  for (const r of recents) {
    if (now - r.ts > windowMs || now < r.ts) continue;
    if (r.kind === "tp" && tpIds.has(r.id)) continue; // 已由 addedAt 覆盖
    const gridId = recentGridId(r.kind, r.id);
    if (out.has(gridId)) continue;
    out.set(gridId, { gridId, label: r.name, ts: r.ts });
  }
  return [...out.values()].sort((a, b) => b.ts - a.ts);
}

/** 「高频」：counts（usage.ts）映射到网格项，取 count>0 前 n（降序）。 */
export function topUsedItems<T extends { id: string }>(
  items: T[],
  n: number,
  counts: Record<string, number> = usageCounts(),
): { item: T; count: number }[] {
  return Object.entries(counts)
    .filter(([id, c]) => c > 0 && items.some((it) => it.id === id))
    .sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1))
    .slice(0, Math.max(0, n))
    .map(([id, count]) => ({ item: items.find((it) => it.id === id) as T, count }));
}

// ---------- 分组开关持久化（localStorage，前缀 variable:start:…） ----------

const HFREQ_KEY = "variable:start:hfreq:v1";

/** 高频分组开关（默认关：隐私偏好保守，规格 V-12）。 */
export function highFreqEnabled(): boolean {
  try {
    return localStorage.getItem(HFREQ_KEY) === "1";
  } catch {
    return false;
  }
}

export function setHighFreqEnabled(v: boolean): void {
  try {
    localStorage.setItem(HFREQ_KEY, v ? "1" : "0");
  } catch {
    /* storage blocked — 本次会话内开关仍生效（组件 state 镜像） */
  }
}
