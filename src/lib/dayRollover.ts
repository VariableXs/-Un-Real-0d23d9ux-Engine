/**
 * AI-20 质量门禁与收官组 — M-90 跨午夜会话正确性（Midnight Rollover）。
 *
 * 统一日界事件 `day://rollover`：时钟、日历、今日简报（M-69）、DND 日程
 * （Z-44）、每日统计等全部「今日」类消费方订阅同一事件，跨午夜无需重启
 * 或聚焦触发即正确翻页。
 *
 * 实现：
 * - 统一定时器（默认 30s 轮询本地日期键）；
 * - 系统时间手动调整监测：|now - lastTick| 远超轮询间隔 → 立即重查
 *   （覆盖用户手动改系统时间 ±1 天场景）；
 * - visibilitychange 回到前台立即重查（挂机睡眠唤醒补偿）。
 */

export const DAY_ROLLOVER_EVENT = "day://rollover";

export interface DayRolloverDetail {
  /** 翻页前日期键 YYYY-MM-DD */
  from: string;
  /** 翻页后日期键 YYYY-MM-DD */
  to: string;
  /** 触发源：tick=轮询 / timejump=系统时间调整 / visible=前台恢复 */
  via: "tick" | "timejump" | "visible";
}

/** 本地日期键（YYYY-MM-DD；测试虚拟时钟友好）。 */
export function dayKeyOf(ts: number): string {
  const d = new Date(ts);
  const m = `${d.getMonth() + 1}`.padStart(2, "0");
  const day = `${d.getDate()}`.padStart(2, "0");
  return `${d.getFullYear()}-${m}-${day}`;
}

/** 定时器实现（注入 now/tick 以便虚拟时钟测试）。 */
export class DayRolloverWatcher {
  private timer: ReturnType<typeof setInterval> | null = null;
  private lastKey: string;
  private lastTick: number;
  private onDocVisible: (() => void) | null = null;

  constructor(
    private readonly now: () => number = () => Date.now(),
    private readonly intervalMs = 30_000,
    private readonly emit: (detail: DayRolloverDetail) => void = (d) => {
      window.dispatchEvent(new CustomEvent(DAY_ROLLOVER_EVENT, { detail: d }));
    },
  ) {
    const t = this.now();
    this.lastKey = dayKeyOf(t);
    this.lastTick = t;
  }

  /** 启动监听；返回停止函数。幂等（重复 start 先卸载旧实例）。 */
  start(): () => void {
    this.stop();
    this.timer = setInterval(() => this.check("tick"), this.intervalMs);
    if (typeof document !== "undefined") {
      this.onDocVisible = () => this.check("visible");
      document.addEventListener("visibilitychange", this.onDocVisible);
    }
    return () => this.stop();
  }

  stop(): void {
    if (this.timer !== null) {
      clearInterval(this.timer);
      this.timer = null;
    }
    if (this.onDocVisible !== null) {
      document.removeEventListener("visibilitychange", this.onDocVisible);
      this.onDocVisible = null;
    }
  }

  /**
   * 单次检查：日期键变化 → 发日界事件。
   * 系统时间手动调整：|now - lastTick| > 间隔×2 + 10s 宽限 → 视为跳变，
   * 以 timejump 源立即重查（不等下一个 tick）。
   */
  check(via: DayRolloverDetail["via"]): DayRolloverDetail | null {
    const t = this.now();
    const jumped = Math.abs(t - this.lastTick) > this.intervalMs * 2 + 10_000;
    this.lastTick = t;
    const key = dayKeyOf(t);
    if (key === this.lastKey) return null;
    const detail: DayRolloverDetail = { from: this.lastKey, to: key, via: jumped && via === "tick" ? "timejump" : via };
    this.lastKey = key;
    this.emit(detail);
    return detail;
  }
}

let singleton: DayRolloverWatcher | null = null;

/** App 启动安装统一日界监听（幂等；返回卸载函数）。 */
export function initDayRollover(): () => void {
  if (singleton) return () => singleton?.stop();
  singleton = new DayRolloverWatcher();
  return singleton.start();
}

/** 订阅日界事件（消费方统一入口；返回退订函数）。 */
export function onDayRollover(handler: (detail: DayRolloverDetail) => void): () => void {
  const listener = (e: Event): void => {
    handler((e as CustomEvent<DayRolloverDetail>).detail);
  };
  window.addEventListener(DAY_ROLLOVER_EVENT, listener);
  return () => window.removeEventListener(DAY_ROLLOVER_EVENT, listener);
}
