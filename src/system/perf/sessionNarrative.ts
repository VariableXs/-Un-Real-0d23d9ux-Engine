/**
 * AI-13 U-23 崩溃叙事（Crash Narratives）前端侧：
 * 全局错误捕获 + 30s 会话快照（localStorage ring）→ 下次启动恢复卡片。
 * 后端 dump/转储见 shell/perf.rs M-53。
 */

export interface CrashEvent {
  /** unhandledrejection | error | manual */
  kind: string;
  message: string;
  /** 归因站点（窗口/面板名） */
  site: string;
  ts: number;
}

export interface SessionNarrative {
  sessionStart: number;
  lastAlive: number;
  events: CrashEvent[];
}

const KEY = "vxs:sessionNarrative";
const RING_CAP = 20;
let timer: ReturnType<typeof setInterval> | null = null;
let handlers: ((e: CrashEvent) => void) | null = null;

function read(): SessionNarrative | null {
  try {
    const raw = localStorage.getItem(KEY);
    return raw ? (JSON.parse(raw) as SessionNarrative) : null;
  } catch {
    return null;
  }
}

function write(n: SessionNarrative): void {
  try { localStorage.setItem(KEY, JSON.stringify(n)); } catch { /* ignore */ }
}

/** 上次会话是否异常退出（最后快照里存在事件，或 lastAlive 之后没有正常关闭标记）。 */
export function previousSessionCrashed(): boolean {
  const prev = read();
  return !!prev && prev.events.length > 0;
}

/** 上次会话叙事（恢复卡片数据）。 */
export function previousNarrative(): SessionNarrative | null {
  return read();
}

/** 安装全局捕获 + 30s 快照心跳（幂等）。 */
export function installSessionNarrative(site: string): void {
  if (timer) return;
  const start = Date.now();
  const state: SessionNarrative = { sessionStart: start, lastAlive: start, events: [] };
  // 覆盖写：新会话开始（旧叙事若异常，恢复卡片读取前先由 UI 取走）
  write(state);
  const record = (kind: string, message: string): void => {
    const cur = read() ?? state;
    const ev: CrashEvent = { kind, message: String(message).slice(0, 300), site, ts: Date.now() };
    cur.events.push(ev);
    while (cur.events.length > RING_CAP) cur.events.shift();
    cur.lastAlive = Date.now();
    write(cur);
    handlers?.(ev);
  };
  window.addEventListener("error", (e) => record("error", e.message));
  window.addEventListener("unhandledrejection", (e) => record("unhandledrejection", String((e as PromiseRejectionEvent).reason)));
  timer = setInterval(() => {
    const cur = read();
    if (cur) {
      cur.lastAlive = Date.now();
      write(cur);
    }
  }, 30_000);
}

/** 恢复卡片：读取并清除旧叙事（一次性）。 */
export function takePreviousNarrative(): SessionNarrative | null {
  const prev = read();
  if (prev && prev.events.length > 0) {
    try { localStorage.removeItem(KEY); } catch { /* ignore */ }
    return prev;
  }
  return null;
}

/** 事件订阅（UI toast 用）。 */
export function onCrashEvent(cb: (e: CrashEvent) => void): void {
  handlers = cb;
}

export function stopSessionNarrative(): void {
  if (timer) {
    clearInterval(timer);
    timer = null;
  }
}
