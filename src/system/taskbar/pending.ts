import { useSyncExternalStore } from "react";

/**
 * M-13 任务栏等待态（AI-03 任务栏与托盘组）：
 * 启动应用到图标出现之间显示占位呼吸图标（复用 .skeleton shimmer）。
 * - 800ms 内重复启动被拦截（绝不重复 ShellExecute）
 * - 8s 未出现 → 占位转「仍在启动」（点击显示详情：耗时）
 * 不做启动失败自动重试（自愈中心 N-25 的地盘）。
 */

export const LAUNCH_DEBOUNCE_MS = 800;
export const LAUNCH_TIMEOUT_MS = 8000;

export interface PendingLaunch {
  key: string;
  name: string;
  startedAt: number;
}

interface PendingState {
  items: PendingLaunch[];
}

const listeners = new Set<() => void>();
let state: PendingState = { items: [] };

function emit(): void {
  state = { items: [...state.items] };
  for (const l of listeners) l();
}

export function subscribePending(cb: () => void): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

export function getPendingItems(now: number = Date.now()): PendingLaunch[] {
  return state.items.filter((i) => now - i.startedAt < LAUNCH_TIMEOUT_MS * 4);
}

export function usePendingLaunches(): PendingLaunch[] {
  return useSyncExternalStore(subscribePending, () => state.items, () => [] as PendingLaunch[]);
}

/** 800ms 防抖锁：key 在锁窗口内 → false（拦截重复启动）。 */
export function canLaunch(key: string, now: number = Date.now()): boolean {
  const hit = state.items.find((i) => i.key === key);
  return !hit || now - hit.startedAt >= LAUNCH_DEBOUNCE_MS;
}

/** 登记/续期一次启动。 */
export function markLaunch(key: string, name: string, now: number = Date.now()): void {
  const rest = state.items.filter((i) => i.key !== key);
  state = { items: [...rest, { key, name, startedAt: now }] };
  emit();
}

/** 运行态已确认（轮询发现实例）→ 撤占位。 */
export function settleLaunch(key: string): void {
  if (!state.items.some((i) => i.key === key)) return;
  state = { items: state.items.filter((i) => i.key !== key) };
  emit();
}

/** 等待态呈现：0..8s = pending；>8s = still-launching（如实转文案）。 */
export function pendingPhase(startedAt: number, now: number): "pending" | "slow" {
  return now - startedAt >= LAUNCH_TIMEOUT_MS ? "slow" : "pending";
}
