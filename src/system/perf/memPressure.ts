/**
 * AI-13 Z-60 内存压力自适应：水位三档监听 → 清缓存 / 冻结后台 / 提示（不代杀）。
 * 契约（ASCENT 交接点）：降档映射先约；恢复不反向自动升级（只降不升 + 手动覆盖）。
 */

export type MemTier = 0 | 1 | 2; // 0 normal / 1 watch / 2 critical

export interface MemPressureEvent {
  tier: MemTier;
  /** 压力源：前端 heap 或后端系统内存 */
  source: "heap" | "system";
  detail?: { usedMb?: number; limitMb?: number; loadPct?: number };
  ts: number;
}

type MemListener = (e: MemPressureEvent) => void;

const listeners = new Set<MemListener>();
let currentTier: MemTier = 0;
let watchTimer: ReturnType<typeof setInterval> | null = null;

/** 降档动作表（与 AI-18 氛围降档、U-21 帧预算联动） */
export const TIER_ACTIONS: Record<MemTier, string[]> = {
  0: [],
  1: ["clear-caches", "halve-particles"],
  2: ["clear-caches", "freeze-idle-render", "floor-frame-tier", "notify-user"],
};

export function memTier(): MemTier {
  return currentTier;
}

export function onMemPressure(cb: MemListener): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

function emit(e: MemPressureEvent): void {
  for (const l of listeners) l(e);
}

/** 只降不升的档位推进（恢复需显式 manualReset，红线 R7）。 */
export function applyTier(next: MemTier, source: MemPressureEvent["source"], detail?: MemPressureEvent["detail"]): MemTier {
  if (next > currentTier) {
    currentTier = next;
    emit({ tier: currentTier, source, detail, ts: Date.now() });
  }
  return currentTier;
}

export function manualReset(): void {
  currentTier = 0;
}

/** 由 heap 水位推导档位：used/limit ≥0.85 → watch，≥0.93 → critical。 */
export function tierFromHeap(usedMb: number, limitMb: number): MemTier {
  if (limitMb <= 0) return 0;
  const r = usedMb / limitMb;
  return r >= 0.93 ? 2 : r >= 0.85 ? 1 : 0;
}

/** 由系统负载推导档位（后端 perf_mem_snapshot）。 */
export function tierFromLoad(loadPct: number): MemTier {
  return loadPct >= 90 ? 2 : loadPct >= 80 ? 1 : 0;
}

/** 周期监听（后端 5s 采样已有 warden；这里 10s 拉一次对齐水位）。 */
export function startMemWatch(poll?: () => Promise<{ loadPct: number; tier: MemTier } | null>): void {
  if (watchTimer) return;
  const tick = (): void => {
    // heap 侧（WebView2 有 performance.memory）
    const mem = (performance as unknown as { memory?: { usedJSHeapSize: number; jsHeapSizeLimit: number } }).memory;
    if (mem) {
      const usedMb = mem.usedJSHeapSize / 1048576;
      const limitMb = mem.jsHeapSizeLimit / 1048576;
      applyTier(tierFromHeap(usedMb, limitMb), "heap", { usedMb: Math.round(usedMb), limitMb: Math.round(limitMb) });
    }
    // 系统侧
    if (poll) {
      void poll().then((s) => {
        if (s) applyTier(tierFromLoad(s.loadPct), "system", { loadPct: s.loadPct });
      }).catch(() => {});
    }
  };
  tick();
  watchTimer = setInterval(tick, 10_000);
}

export function stopMemWatch(): void {
  if (watchTimer) {
    clearInterval(watchTimer);
    watchTimer = null;
  }
}

/** 测试钩子。 */
export function resetMemPressure(): void {
  stopMemWatch();
  currentTier = 0;
  listeners.clear();
}
