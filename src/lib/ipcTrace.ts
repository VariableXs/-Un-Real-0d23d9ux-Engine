/**
 * AI-20 质量门禁与收官组 — M-80 IPC 调用追踪（IPC Trace View）。
 *
 * 仅 dev 构建生效（ipc.ts 在 `import.meta.env.DEV` 分支内动态 import 本模块，
 * release 构建经 Vite 死代码剔除后本模块零残留 —— 验收口径：dist 内 rg
 * "ipcTrace" 零命中）。
 *
 * 环形缓冲（500 次 invoke）：命令名、耗时、成功/失败；>100ms 标红；
 * 面板（Ctrl+Alt+F12，仅 dev 注册）瀑布图 + 按命令名过滤。
 */

export interface IpcTraceRecord {
  cmd: string;
  /** 耗时（ms） */
  ms: number;
  ok: boolean;
  /** 失败时的错误码/消息（截断） */
  error: string | null;
  ts: number;
}

export const IPC_TRACE_MAX = 500;
export const IPC_SLOW_MS = 100;

type Listener = (records: readonly IpcTraceRecord[]) => void;

const ring: IpcTraceRecord[] = [];
const listeners = new Set<Listener>();

function notify(): void {
  const snap = [...ring];
  for (const l of listeners) l(snap);
}

/** 埋点包装（ipc.ts dev 分支调用；raw = 真实 invoke）。 */
export async function traceInvoke<T>(
  cmd: string,
  raw: () => Promise<T>,
): Promise<T> {
  const t0 = performance.now();
  try {
    const r = await raw();
    push({ cmd, ms: performance.now() - t0, ok: true, error: null, ts: Date.now() });
    return r;
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    push({ cmd, ms: performance.now() - t0, ok: false, error: msg.slice(0, 160), ts: Date.now() });
    throw e;
  }
}

function push(rec: IpcTraceRecord): void {
  ring.push(rec);
  if (ring.length > IPC_TRACE_MAX) ring.splice(0, ring.length - IPC_TRACE_MAX);
  notify();
}

/** 面板订阅（卸载必须退订）。 */
export function subscribeIpcTrace(l: Listener): () => void {
  listeners.add(l);
  l([...ring]);
  return () => listeners.delete(l);
}

/** 当前快照。 */
export function ipcTraceSnapshot(): IpcTraceRecord[] {
  return [...ring];
}

/** 按命令名过滤（前缀匹配；空串 = 全部）。 */
export function filterIpcTrace(records: readonly IpcTraceRecord[], query: string): IpcTraceRecord[] {
  const q = query.trim().toLowerCase();
  if (!q) return [...records];
  return records.filter((r) => r.cmd.toLowerCase().includes(q));
}

/** 瀑布行视图模型：相对面板时间窗的起止比例 + 慢调用标记。 */
export interface WaterfallRow {
  rec: IpcTraceRecord;
  /** 起点比例 0..1（相对窗口内最早 ts） */
  left: number;
  /** 宽度比例 0..1（相对窗口内最大耗时；最小可见 0.5%） */
  width: number;
  slow: boolean;
}

export function buildWaterfall(records: readonly IpcTraceRecord[]): WaterfallRow[] {
  if (records.length === 0) return [];
  const t0 = records[0]?.ts ?? 0;
  const t1 = records[records.length - 1]?.ts ?? t0;
  const span = Math.max(t1 - t0, 1);
  const maxMs = Math.max(...records.map((r) => r.ms), 1);
  return records.map((rec) => ({
    rec,
    left: (rec.ts - t0) / span,
    width: Math.max(rec.ms / maxMs, 0.005),
    slow: rec.ms > IPC_SLOW_MS,
  }));
}

export function clearIpcTrace(): void {
  ring.length = 0;
  notify();
}
