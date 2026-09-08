/**
 * AI-13 Z-58 UI 线程健康面板：FPS / 长任务 / 提交耗时 / 内存 采集。
 * 红线：关闭态零采集（stop 后 observer 断开、rAF 停止、无任何回调注册）。
 */

export interface HealthSample {
  fps: number;
  /** 长任务（>50ms）计数（窗口内） */
  longTasks: number;
  /** 最长任务 ms */
  worstTaskMs: number;
  /** performance.memory.usedJSHeapSize（如可用，Chrome/WebView2） */
  heapUsedMb: number | null;
  ts: number;
}

export interface HealthReport {
  samples: HealthSample[];
  avgFps: number;
  totalLongTasks: number;
  worstTaskMs: number;
}

type State = "off" | "on";

let state: State = "off";
let samples: HealthSample[] = [];
let observer: PerformanceObserver | null = null;
let rafId = 0;
let longTasks = 0;
let worstTaskMs = 0;
let frames = 0;
let windowStart = 0;
let onSample: ((s: HealthSample) => void) | null = null;
const WINDOW_MS = 2000;

export function isCollecting(): boolean {
  return state === "on";
}

/** 开启采集（Z-58 默认关闭，需用户显式开启）。 */
export function startHealthCollection(cb?: (s: HealthSample) => void): void {
  if (state === "on") return;
  state = "on";
  samples = [];
  longTasks = 0;
  worstTaskMs = 0;
  frames = 0;
  windowStart = performance.now();
  onSample = cb ?? null;
  // 长任务观测（WebView2 支持 longtask）
  try {
    observer = new PerformanceObserver((list) => {
      for (const e of list.getEntries()) {
        longTasks += 1;
        worstTaskMs = Math.max(worstTaskMs, e.duration);
      }
    });
    observer.observe({ entryTypes: ["longtask"] });
  } catch {
    observer = null; // 环境不支持则静默降级
  }
  const loop = (t: number): void => {
    if (state !== "on") return;
    frames += 1;
    if (t - windowStart >= WINDOW_MS) {
      const mem = (performance as unknown as { memory?: { usedJSHeapSize: number } }).memory;
      const sample: HealthSample = {
        fps: Math.round((frames * 1000) / (t - windowStart)),
        longTasks,
        worstTaskMs: Math.round(worstTaskMs * 10) / 10,
        heapUsedMb: mem ? Math.round(mem.usedJSHeapSize / 1048576) : null,
        ts: Date.now(),
      };
      samples.push(sample);
      if (samples.length > 300) samples.shift();
      frames = 0;
      longTasks = 0;
      windowStart = t;
      onSample?.(sample);
    }
    rafId = requestAnimationFrame(loop);
  };
  rafId = requestAnimationFrame(loop);
}

/** 停止并断开全部采集（零采集验证点）。 */
export function stopHealthCollection(): void {
  state = "off";
  if (observer) {
    observer.disconnect();
    observer = null;
  }
  cancelAnimationFrame(rafId);
  onSample = null;
}

export function healthReport(): HealthReport {
  const avgFps = samples.length === 0 ? 0 : Math.round((samples.reduce((a, s) => a + s.fps, 0) / samples.length) * 10) / 10;
  const totalLongTasks = samples.reduce((a, s) => a + s.longTasks, 0);
  const worst = samples.reduce((a, s) => Math.max(a, s.worstTaskMs), 0);
  return { samples: [...samples], avgFps, totalLongTasks, worstTaskMs: worst };
}

export function resetHealth(): void {
  samples = [];
}

/** 归因标记（Z-58）：把健康样本与站点关联（调用方传入站点名）。 */
export function attributeSite(sampleTs: number, site: string): void {
  const last = samples[samples.length - 1];
  if (last && Math.abs(last.ts - sampleTs) < WINDOW_MS * 2) {
    (last as HealthSample & { site?: string }).site = site;
  }
}
