/**
 * AI-13 Z-59 空闲渲染冻结（Idle Render Freeze）
 *
 * 最小化 / document.hidden / 非当前桌面检测 → 注册方 rAF 暂停；
 * 恢复时先预渲染一帧再放行（<100ms 感知）。
 * 豁免清单：视频 / 下载 / 音频（keepAlive 站点不冻结）。
 */

export type FreezeReason = "hidden" | "minimized" | "none";

export interface FreezeHandle {
  site: string;
  /** 每帧回调；冻结期间不再调用 */
  tick: (now: number) => void;
  /** 豁免站点（视频/下载/音频）不冻结 */
  exempt?: boolean;
}

interface FreezeState {
  handles: Set<FreezeHandle>;
  frozen: FreezeReason;
  rafId: number;
  running: boolean;
  lastNow: number;
}

const st: FreezeState = { handles: new Set(), frozen: "none", rafId: 0, running: false, lastNow: 0 };

type FreezeListener = (reason: FreezeReason) => void;
const listeners = new Set<FreezeListener>();

export function onFreezeChange(cb: FreezeListener): () => void {
  listeners.add(cb);
  return () => listeners.delete(cb);
}

export function freezeReason(): FreezeReason {
  return st.frozen;
}

/** 注册一个渲染站点。exempt=true（视频/下载/音频）在冻结期仍继续。 */
export function registerFreezable(handle: FreezeHandle): () => void {
  st.handles.add(handle);
  ensureLoop();
  return () => st.handles.delete(handle);
}

/** 环境事件接入：document.visibilitychange / winman 最小化通知 / VWM 桌面切换。 */
export function setOccluded(reason: Exclude<FreezeReason, "none"> | null): void {
  const next: FreezeReason = reason ?? "none";
  if (st.frozen === next) return;
  st.frozen = next;
  if (next === "none") {
    // 恢复：预渲染一帧（立即对所有未豁免站点各跑一次 tick）再继续常规循环
    const now = performance.now();
    for (const h of st.handles) h.tick(now);
    st.lastNow = now;
  }
  for (const l of listeners) l(next);
}

function ensureLoop(): void {
  if (st.running) return;
  st.running = true;
  const loop = (now: number): void => {
    if (st.handles.size === 0) {
      st.running = false;
      return;
    }
    const dt = st.lastNow === 0 ? 16 : Math.min(250, now - st.lastNow);
    st.lastNow = now;
    if (st.frozen === "none") {
      for (const h of st.handles) {
        if (!h.exempt) h.tick(now);
      }
      // 豁免站点始终以限频运行（冻结期也跑，正常期同样跑）
      for (const h of st.handles) {
        if (h.exempt) h.tick(now);
      }
    } else {
      for (const h of st.handles) {
        if (h.exempt) h.tick(now);
      }
    }
    void dt;
    st.rafId = requestAnimationFrame(loop);
  };
  st.rafId = requestAnimationFrame(loop);
}

/** 单测/干净退出用。 */
export function resetFreeze(): void {
  cancelAnimationFrame(st.rafId);
  st.handles.clear();
  st.frozen = "none";
  st.running = false;
  listeners.clear();
}
