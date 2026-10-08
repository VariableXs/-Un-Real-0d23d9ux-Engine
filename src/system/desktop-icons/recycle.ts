/**
 * V-08 回收站角标：轮询 rec_count（ipc 已封装 recCount()）。
 * - 空回收站 = 无角标；> 0 = 数量角标（99+ 截断在组件层）。
 * 诚实边界：配额/容量数据后端未提供 → 只有「空 / N 个」两态，不做 90% 预警；
 *   查询失败（非 Tauri 环境 / 命令缺失）→ 返回 null，调用方不显示角标（诚实降级）。
 */
import { ipc } from "../../lib/ipc";

/** 轮询周期（规格：2s + 窗口 focus 立即刷新）。 */
export const RECYCLE_POLL_MS = 2000;

export type RecycleBadge = { kind: "empty" } | { kind: "count"; n: number };

/** 计数 → 角标状态（纯函数；null = 查询不可用，不显示）。 */
export function badgeForCount(n: number | null): RecycleBadge | null {
  if (n === null || !Number.isFinite(n) || n < 0) return null;
  return n > 0 ? { kind: "count", n: Math.floor(n) } : { kind: "empty" };
}

/** 拉取回收站计数；任何失败如实返回 null（不抛出、不假报）。 */
export async function fetchRecycleCount(): Promise<number | null> {
  try {
    const n = await ipc.recCount();
    return typeof n === "number" ? n : null;
  } catch {
    return null;
  }
}

/**
 * 启动轮询：立即取一次 + interval 周期 + 窗口 focus 立即刷新。
 * 返回停止函数（组件卸载调用）。
 */
export function startRecyclePolling(cb: (n: number | null) => void, intervalMs: number = RECYCLE_POLL_MS): () => void {
  let alive = true;
  const tick = (): void => {
    void fetchRecycleCount().then((n) => {
      if (alive) cb(n);
    });
  };
  tick();
  const timer = window.setInterval(tick, intervalMs);
  const onFocus = (): void => tick();
  window.addEventListener("focus", onFocus);
  return () => {
    alive = false;
    window.clearInterval(timer);
    window.removeEventListener("focus", onFocus);
  };
}