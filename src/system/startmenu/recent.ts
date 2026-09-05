import { useSyncExternalStore } from "react";

/**
 * 最近使用（批次E，规格 4.6.2 / N4）：
 * 纯本地 localStorage 记录（零网络），开始菜单"最近"行 + 全局搜索历史共用。
 * kind: "app" = 官方软件 / "sys" = 系统入口（explorer/recycle/settings...）/ "tp" = 第三方。
 */

export interface RecentEntry {
  kind: "app" | "sys" | "tp";
  /** app = AppMode；sys = 系统窗口 kind；tp = 第三方 id */
  id: string;
  name: string;
  ts: number;
}

const KEY = "variable:recent:v1";
const MAX = 8;

let cache: RecentEntry[] | null = null;
const listeners = new Set<() => void>();

function load(): RecentEntry[] {
  if (cache) return cache;
  try {
    const raw = localStorage.getItem(KEY);
    cache = raw ? (JSON.parse(raw) as RecentEntry[]) : [];
  } catch {
    cache = [];
  }
  return cache;
}

function persist(list: RecentEntry[]): void {
  cache = list;
  try {
    localStorage.setItem(KEY, JSON.stringify(list));
  } catch {
    /* storage full — 内存态继续可用 */
  }
  for (const l of listeners) l();
}

export function pushRecent(kind: RecentEntry["kind"], id: string, name: string): void {
  const list = load().filter((e) => !(e.kind === kind && e.id === id));
  list.unshift({ kind, id, name, ts: Date.now() });
  persist(list.slice(0, MAX));
}

export function getRecent(): RecentEntry[] {
  return load();
}

export function useRecent(): RecentEntry[] {
  return useSyncExternalStore(
    (cb) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    () => load(),
    () => [] as RecentEntry[],
  );
}
