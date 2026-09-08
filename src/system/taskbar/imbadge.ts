import { useSyncExternalStore } from "react";
import { listen } from "@tauri-apps/api/event";

/**
 * M-14 托盘 IM 未读聚合（AI-03 任务栏与托盘组）：
 - 数据源：后端 imwatch.rs 推送 `sys://im-msg`（只读窗口标题，红线不破）。
 * - 每应用未读数 = 标题里的计数（与 Rust 端 has_unread_marker 同口径）
 * - 总徽标 = 各 IM 之和，99+ 封顶；应用关闭（不再推送）后 60s 无更新即清零
 * 不读聊天内容、不解析窗口内容、不提供回复。
 */

const listeners = new Set<() => void>();
let counts: Record<string, number> = {};
let lastSeen: Record<string, number> = {};

function emit(): void {
  counts = { ...counts };
  for (const l of listeners) l();
}

export function subscribeIm(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

export function getImCounts(): Record<string, number> {
  return counts;
}

export function useImCounts(): Record<string, number> {
  return useSyncExternalStore(subscribeIm, () => counts, () => ({}));
}

/** 与 Rust imwatch has_unread_marker 同口径：标题里的「(3)」「（12）」「【3】」→ 数值。 */
export function parseUnreadFromTitle(title: string): number | null {
  const re = /[(（【](\d{1,3})[)）】]/;
  const m = re.exec(title);
  return m ? Number(m[1]) : null;
}

/** 聚合总数（99+ 封顶由 UI 层处理；此处给原始和）。 */
export function imTotal(map: Record<string, number>): number {
  return Object.values(map).reduce((a, b) => a + b, 0);
}

/** IM 应用关闭后 60s 无更新 → 计数清零（imwatch 停止推送该应用）。 */
export function expireStale(now: number = Date.now()): void {
  let changed = false;
  for (const [k, ts] of Object.entries(lastSeen)) {
    if (now - ts > 60_000) {
      if (counts[k]) {
        const { [k]: _drop, ...rest } = counts;
        counts = rest;
        changed = true;
      }
      const { [k]: _dropTs, ...restTs } = lastSeen;
      lastSeen = restTs;
      changed = true;
    }
  }
  if (changed) emit();
}

let started = false;
/** 挂事件监听（桌面 shell 挂载时调用一次；重复调用幂等）。 */
export function startImWatcher(): () => void {
  if (started) return () => {};
  started = true;
  const un = listen<{ app: string; title: string }>("sys://im-msg", (e) => {
    const app = e.payload?.app;
    if (!app) return;
    const n = parseUnreadFromTitle(e.payload.title ?? "");
    lastSeen[app] = Date.now();
    const cur = counts[app] ?? 0;
    const next = n === null ? 0 : n;
    if (next !== cur) {
      if (next === 0) delete counts[app];
      else counts[app] = next;
      emit();
    }
  });
  const gc = window.setInterval(() => expireStale(), 15_000);
  return () => {
    void un.then((f) => f()).catch(() => {});
    window.clearInterval(gc);
    started = false;
  };
}
