/**
 * 化境 V-12（车道 S）：高频使用本地计数。
 * 纯 localStorage（前缀 variable:start:…），零网络、零全局钩子 —— 只在
 * StartMenu 点击应用时由组件调用 bumpUsage 累加；topUsed 取前 N 名。
 * 诚实边界：这是 UI 层近似统计（仅统计「从开始菜单启动」的次数），
 * 不是系统级使用洞察（那属 N-31 领地）。
 */

const KEY = "variable:start:usage:v1";

type Counts = Record<string, number>;

let cache: Counts | null = null;
const listeners = new Set<() => void>();

function load(): Counts {
  if (cache) return cache;
  try {
    const raw = localStorage.getItem(KEY);
    cache = raw ? (JSON.parse(raw) as Counts) : {};
    if (!cache || typeof cache !== "object") cache = {};
  } catch {
    cache = {};
  }
  return cache;
}

function persist(c: Counts): void {
  cache = c;
  try {
    localStorage.setItem(KEY, JSON.stringify(c));
  } catch {
    /* storage full — 内存态继续可用 */
  }
  for (const l of listeners) l();
}

/** 开始菜单点击应用时累加一次（id = 网格项 id，如 app-write / tp-xxx / fd-xxx）。 */
export function bumpUsage(id: string): void {
  if (!id) return;
  const cur = load();
  persist({ ...cur, [id]: (cur[id] ?? 0) + 1 });
}

/** 单项计数。 */
export function usageCount(id: string): number {
  return load()[id] ?? 0;
}

/** 全量计数（只读快照）。 */
export function usageCounts(): Counts {
  return { ...load() };
}

/** 前 N 名（count>0；按次数降序、次数同则按 id 稳定排序）。 */
export function topUsed(n: number): { id: string; count: number }[] {
  return Object.entries(load())
    .filter(([, c]) => c > 0)
    .sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : 1))
    .slice(0, Math.max(0, n))
    .map(([id, count]) => ({ id, count }));
}

/** React 订阅：监听器集合（组件侧用 useSyncExternalStore + 标量快照）。 */
export function subscribeUsage(cb: () => void): () => void {
  listeners.add(cb);
  return () => {
    listeners.delete(cb);
  };
}
