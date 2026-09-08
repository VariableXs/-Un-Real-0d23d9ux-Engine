/**
 * AI-13 M-50 事件风暴削峰（Event Storm Shedding）
 *
 * 三档声明式频道：merge（合并批次）/ latest（只留最新）/ queue（保序排队）。
 * 目标：1000 事件/秒合成风暴下「最新必达 100%」+ 回调次数收敛。
 */

export type ShedStrategy = "merge" | "latest" | "queue";

export interface ShedChannelOptions<T> {
  strategy: ShedStrategy;
  /** 时间窗（ms），窗口结束才 flush；latest/merge 生效 */
  windowMs?: number;
  /** merge：批量合并函数（同窗事件数组 → 单负载） */
  merge?: (batch: T[]) => T;
  /** queue：队列上限，超出丢最旧（最新必达） */
  maxQueue?: number;
}

interface ChannelState<T> {
  strategy: ShedStrategy;
  windowMs: number;
  merge?: (batch: T[]) => T;
  maxQueue?: number;
  handler: (payload: T) => void;
  timer: ReturnType<typeof setTimeout> | null;
  pending: T[];
  /** 统计：入队/实际回调次数（削峰收敛验证用） */
  received: number;
  delivered: number;
}

export interface ShedStats {
  received: number;
  delivered: number;
}

const channels = new Map<string, ChannelState<any>>(); // eslint-disable-line @typescript-eslint/no-explicit-any

function flush(ch: ChannelState<any>): void {
  ch.timer = null;
  if (ch.pending.length === 0) return;
  const batch = ch.pending;
  ch.pending = [];
  ch.delivered += 1;
  if (ch.strategy === "merge" && ch.merge) {
    ch.handler(ch.merge(batch));
  } else if (ch.strategy === "latest") {
    ch.handler(batch[batch.length - 1]);
  } else {
    for (const item of batch) ch.handler(item);
  }
}

/** 注册（或覆盖注册）一个削峰频道。 */
export function shedChannel<T>(name: string, opts: ShedChannelOptions<T>, handler: (payload: T) => void): void {
  channels.set(name, {
    strategy: opts.strategy,
    windowMs: opts.windowMs ?? 100,
    merge: opts.merge,
    maxQueue: opts.maxQueue,
    handler,
    timer: null,
    pending: [],
    received: 0,
    delivered: 0,
  });
}

/** 频道入队。未注册的频道直接丢弃（防御式，无静默泄漏）。 */
export function shedEmit<T>(name: string, payload: T): void {
  const ch = channels.get(name);
  if (!ch) return;
  ch.received += 1;
  if (ch.strategy === "latest") {
    ch.pending = [payload];
  } else {
    ch.pending.push(payload);
    if (ch.strategy === "queue" && ch.maxQueue !== undefined) {
      while (ch.pending.length > ch.maxQueue) ch.pending.shift();
    }
  }
  if (ch.timer === null) {
    ch.timer = setTimeout(() => flush(ch), ch.windowMs);
  }
}

export function shedStats(name: string): ShedStats | null {
  const ch = channels.get(name);
  return ch ? { received: ch.received, delivered: ch.delivered } : null;
}

export function shedReset(name?: string): void {
  if (name) {
    const ch = channels.get(name);
    if (ch && ch.timer !== null) clearTimeout(ch.timer);
    channels.delete(name);
  } else {
    for (const [k, ch] of channels) {
      if (ch.timer !== null) clearTimeout(ch.timer);
      channels.delete(k);
    }
  }
}

/** 迁移点清单（M-50 验收走查用）：winman 轮询 / imwatch / 设置广播 / 硬件面板 / VWM 状态。 */
export const SHED_MIGRATION_SOURCES = [
  "winman-poll",
  "imwatch-status",
  "settings-broadcast",
  "hardware-panel",
  "vwm-state",
] as const;
