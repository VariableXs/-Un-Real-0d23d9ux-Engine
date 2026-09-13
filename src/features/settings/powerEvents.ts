/**
 * UNREAL-X AI-02 · 族0016 电源事件（X00351 档 · Variable 侧事件流）。
 *
 * 事件流环形记录 + 订阅 API + 过滤查询；事件类型白名单，
 * 详情字节数钳制，非法类型记钳制不崩溃。纯逻辑模块。
 */

export const POWER_EVENT_TYPES = [
  "sleep", "wake", "shutdown", "restart", "battery-low", "thermal-warn", "lid", "plugged",
] as const;
export type PowerEventType = (typeof POWER_EVENT_TYPES)[number];

export function isPowerEventType(t: string): t is PowerEventType {
  return (POWER_EVENT_TYPES as readonly string[]).includes(t);
}

export interface PowerEvent {
  type: PowerEventType;
  /** 发生时刻（调用方时钟）。 */
  stamp: number;
  /** 详情（≤120 字符，超长截断）。 */
  detail: string;
}

export const POWER_EVENT_CAPACITY = 32;
const DETAIL_MAX = 120;

type Listener = (e: PowerEvent) => void;

/** 电源事件流。 */
export class PowerEventStream {
  events: PowerEvent[] = [];
  private listeners = new Set<Listener>();
  clamped = 0;

  /** 记录一条事件（非法类型记钳制；详情截断到 120 字符）。 */
  record(type: string, stamp: number, detail = ""): PowerEvent | null {
    if (!isPowerEventType(type)) {
      this.clamped += 1;
      return null;
    }
    const e: PowerEvent = { type, stamp: Math.max(0, Math.round(stamp) || 0), detail: String(detail).slice(0, DETAIL_MAX) };
    this.events.unshift(e);
    if (this.events.length > POWER_EVENT_CAPACITY) this.events.pop();
    for (const l of this.listeners) {
      try { l(e); } catch { /* 订阅者异常不阻断事件流 */ }
    }
    return e;
  }

  /** 订阅：返回退订函数（订阅 API）。 */
  subscribe(l: Listener): () => void {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  }

  subscriberCount(): number {
    return this.listeners.size;
  }

  /** 按类型过滤查询（空 = 全部）。 */
  query(types: readonly PowerEventType[] = []): PowerEvent[] {
    if (types.length === 0) return [...this.events];
    return this.events.filter((e) => types.includes(e.type));
  }

  /** 最近一次某类事件（无则 null）。 */
  lastOf(type: PowerEventType): PowerEvent | null {
    return this.events.find((e) => e.type === type) ?? null;
  }

  /** 事件行（设置页事件列表直接渲染）。 */
  static line(e: PowerEvent): string {
    const zh: Record<PowerEventType, string> = {
      sleep: "睡眠", wake: "唤醒", shutdown: "关机", restart: "重启",
      "battery-low": "低电量", "thermal-warn": "过热预警", lid: "盖盖", plugged: "接通电源",
    };
    const d = e.detail ? ` · ${e.detail}` : "";
    return `[${e.stamp}] ${zh[e.type]}${d}`;
  }

  /** 净身：清空事件流与订阅。 */
  reset(): void {
    this.events = [];
    this.listeners.clear();
    this.clamped = 0;
  }
}
