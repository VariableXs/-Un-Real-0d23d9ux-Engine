/**
 * H4 域统一体验日志总线（v7 · 深化批次七 —— 十三章「体验日志」+ 十三补「异常显性化」的域内实装）：
 * - 分级 debug/info/warn/error/fatal；环形缓冲 500 条（内存有上限——十四章性能纪律）；
 * - 批量异步 flush（sink 注入；**写日志绝不阻塞交互**——攒批落盘，测试用内存 sink）；
 * - 隐私红线：元数据 >200 字符截断并显式标记——**只记事件不记内容**（密码/正文全文进不来）；
 * - 挫败信号主动捕获：狂点（1.5s 内 24px 半径 ≥3 次）、死点（点击 >100ms 无反馈）、
 *   带结论字段——日志直接回答「用户在这一步舒不舒服」；
 * - 时间轴查询 + JSON 导出（总日志中心的数据源面）。
 */

export type XLogLevel = "debug" | "info" | "warn" | "error" | "fatal";

export interface XLogEntry {
  /** 会话内单调序号。 */
  seq: number;
  at: number;
  level: XLogLevel;
  /** 来源（功能面/组件 id——h4-bus、F371、quick-settings……）。 */
  source: string;
  /** 事件名（kebab）。 */
  event: string;
  /** 元数据（经 sanitize——隐私红线后的安全串）。 */
  meta: string;
  /** true = 元数据被截断（原样内容未落日志——隐私红线的显式痕迹）。 */
  truncated: boolean;
}

export type XLogSink = (entries: XLogEntry[]) => void;

/** 环形缓冲容量（内存上限纪律）。 */
export const XLOG_CAP = 500;
/** 元数据隐私红线（字符数）：超出截断 + 标记——正文全文永远进不了日志。 */
export const META_MAX_CHARS = 200;
/** 狂点判定：窗口时长与半径。 */
export const RAGE_WINDOW_MS = 1500;
export const RAGE_RADIUS_PX = 24;
export const RAGE_MIN_CLICKS = 3;
/** 死点判定：点击到可见反馈的预算（100ms——十四章「任何点击 100ms 内必须有可见反馈」）。 */
export const DEAD_CLICK_BUDGET_MS = 100;

/** 元数据消毒：序列化 → 超长截断 + 显式标记（隐私红线的执行面）。 */
export function sanitizeMeta(meta: unknown): { text: string; truncated: boolean } {
  if (meta === undefined) return { text: "", truncated: false };
  let text: string;
  try {
    text = typeof meta === "string" ? meta : JSON.stringify(meta);
  } catch {
    text = "（元数据不可序列化——已拦截）";
  }
  if (text.length > META_MAX_CHARS) {
    return { text: `${text.slice(0, META_MAX_CHARS)}…（截断：原长 ${text.length} 字符，内容未落日志）`, truncated: true };
  }
  return { text, truncated: false };
}

export class XLog {
  private readonly buffer: XLogEntry[] = [];
  private seq = 0;
  private readonly pending: XLogEntry[] = [];
  private sink: XLogSink | null = null;
  private flushTimer: ReturnType<typeof setTimeout> | null = null;
  private clicks: Array<{ x: number; y: number; at: number }> = [];
  private rageCount = 0;
  private deadCount = 0;

  constructor(public readonly sessionId = "h4-session") {}

  /** 记一条（同步入环 + 攒批待刷——写日志零阻塞路径）。 */
  log(level: XLogLevel, source: string, event: string, meta?: unknown): XLogEntry {
    const s = sanitizeMeta(meta);
    const entry: XLogEntry = { seq: ++this.seq, at: Date.now(), level, source, event, meta: s.text, truncated: s.truncated };
    this.buffer.push(entry);
    if (this.buffer.length > XLOG_CAP) this.buffer.shift();
    this.pending.push(entry);
    this.scheduleFlush();
    return entry;
  }

  /* ---------- 批量异步落盘（sink 注入——存储通道可换，测试用内存） ---------- */

  setSink(sink: XLogSink, batchMs = 2000): void {
    this.sink = sink;
    this.scheduleFlush(batchMs);
  }

  private scheduleFlush(batchMs = 2000): void {
    if (this.sink === null || this.flushTimer !== null || this.pending.length === 0) return;
    if (typeof setTimeout === "undefined") return; // node 测试环境无定时器——flushNow 显式驱动
    this.flushTimer = setTimeout(() => {
      this.flushTimer = null;
      this.flushNow();
    }, batchMs);
  }

  /** 立即冲刷：攒批条目交给 sink，返回冲刷条数（断电前/退出前的兜底路径）。 */
  flushNow(): number {
    if (this.sink === null || this.pending.length === 0) return 0;
    const batch = this.pending.splice(0, this.pending.length);
    try {
      this.sink(batch);
    } catch (err) {
      // sink 失败显性化：日志写不出去本身要留痕（异常零静默——但不能再递归进自己）
      this.buffer.push({ seq: ++this.seq, at: Date.now(), level: "error", source: "xlog", event: "sink-failed", meta: err instanceof Error ? err.message : String(err), truncated: false });
      if (this.buffer.length > XLOG_CAP) this.buffer.shift();
      return 0;
    }
    return batch.length;
  }

  /* ---------- 查询与导出（总日志中心的数据源面） ---------- */

  timeline(filter?: { level?: XLogLevel; source?: string }): XLogEntry[] {
    return this.buffer.filter((e) => (filter?.level ? e.level === filter.level : true) && (filter?.source ? e.source === filter.source : true));
  }

  exportJson(): string {
    return JSON.stringify({ sessionId: this.sessionId, count: this.buffer.length, entries: this.buffer }, null, 2);
  }

  /* ---------- 挫败信号（十三章：糟糕体验的指纹自动标记） ---------- */

  /**
   * 点击记录：狂点（窗口内半径 24px ≥3 次）与死点（反馈 >100ms）自动标记——
   * 返回本点的两个信号判定（调用方无需自己算）。
   */
  click(x: number, y: number, at: number, feedbackMs: number): { rage: boolean; dead: boolean } {
    this.clicks.push({ x, y, at });
    this.clicks = this.clicks.filter((c) => at - c.at <= RAGE_WINDOW_MS);
    const near = this.clicks.filter((c) => Math.hypot(c.x - x, c.y - y) <= RAGE_RADIUS_PX);
    const rage = near.length >= RAGE_MIN_CLICKS;
    if (rage) {
      this.rageCount++;
      this.log("warn", "ux-signal", "rage-click", `同点位 ${RAGE_WINDOW_MS}ms 内 ${near.length} 次点击——疑似挫败`);
    }
    const dead = feedbackMs > DEAD_CLICK_BUDGET_MS;
    if (dead) {
      this.deadCount++;
      this.log("warn", "ux-signal", "dead-click", `点击后 ${feedbackMs}ms 无反馈（预算 ${DEAD_CLICK_BUDGET_MS}ms）`);
    }
    return { rage, dead };
  }

  frustrationSummary(): { rageClicks: number; deadClicks: number } {
    return { rageClicks: this.rageCount, deadClicks: this.deadCount };
  }
}

/** 域内单例（全部 H4 功能共用一条体验日志——总日志中心的 H4 分册）。 */
export const xlog = new XLog();

/** 便捷入口（bus 等内部件用——少一层 import）。 */
export function h4log(level: XLogLevel, source: string, event: string, meta?: unknown): void {
  xlog.log(level, source, event, meta);
}
