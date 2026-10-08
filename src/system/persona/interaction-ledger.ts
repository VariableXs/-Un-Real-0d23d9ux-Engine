/**
 * 十三章 体验日志深化 · E 域交互台账（挫败信号捕获 + 隐私结构红线 + 回放）。
 *
 * 主册判据延伸：
 * - 十三章「记录到交互细节层」「主动捕获挫败信号」「可回放」——台账是
 *   E 域二十页的统一交互日志：页面/元素/时刻/动作/反馈/耗时/结论七元组；
 * - 挫败指纹机械化：rage click（1.5s 内同元素 ≥3 点）、dead click（点击
 *   无反馈元素）、浮层反复开关（30s 内开关 ≥3 次）、错误后徘徊；
 * - 隐私红线在**数据结构层**：事件类型不含内容字段（value/text 永远不
 *   进接口）——违规在编译期不可能；
 * - 「日志写入绝不阻塞交互」：批量缓冲 + 上限截断（内存恒定）。
 */

// ---------- 事件（结构即隐私——七元组无内容位） ----------

export type InteractionAction = "click" | "dblclick" | "contextmenu" | "hover" | "keydown" | "drag" | "scroll";
export type InteractionVerdict = "smooth" | "laggy" | "no-feedback" | "interrupted" | "error";

export interface InteractionEvent {
  page: string;
  elementId: string;
  at: number;
  action: InteractionAction;
  /** 可见反馈延迟 ms（100ms 红线的实测位）。 */
  feedbackMs: number | null;
  /** 本次交互耗时 ms（浮层为开关全程）。 */
  durationMs: number | null;
  verdict: InteractionVerdict;
}

export interface FrustrationSignal {
  kind: "rage-click" | "dead-click" | "popover-flap" | "error-wander";
  elementId: string;
  at: number;
  /** 人话结论（直接回答"这一步舒不舒服"）。 */
  message: string;
}

// ---------- 台账（环形缓冲 + 挫败检测） ----------

export const BUFFER_MAX = 500;
const RAGE_WINDOW_MS = 1500;
const RAGE_COUNT = 3;
const FLAP_WINDOW_MS = 30_000;
const FLAP_COUNT = 3;

export class InteractionLedger {
  private buffer: InteractionEvent[] = [];
  private signals: FrustrationSignal[] = [];
  /** 浮层开关配对表（flap 检测的状态源）。 */
  private popoverToggles = new Map<string, number[]>();

  record(ev: InteractionEvent): FrustrationSignal[] {
    this.buffer.push(ev);
    if (this.buffer.length > BUFFER_MAX) this.buffer.splice(0, this.buffer.length - BUFFER_MAX);
    const fresh: FrustrationSignal[] = [];
    fresh.push(...this.detectRageClick(ev));
    if (ev.verdict === "no-feedback") fresh.push(this.deadClick(ev));
    return fresh;
  }

  /** 浮层开关登记（打开/关闭各记一次——flap 在跨越阈值的那一次触发，同窗不重复报）。 */
  togglePopover(page: string, elementId: string, at: number): FrustrationSignal | null {
    const key = `${page}:${elementId}`;
    const stamps = [...(this.popoverToggles.get(key) ?? []), at].filter((t) => at - t <= FLAP_WINDOW_MS);
    this.popoverToggles.set(key, stamps);
    if (stamps.length === FLAP_COUNT) {
      const sig: FrustrationSignal = { kind: "popover-flap", elementId, at, message: `30 秒内开关 ${stamps.length} 次——交互目标可能没被满足（找不到想要的东西）` };
      this.signals.push(sig);
      return sig;
    }
    return null;
  }

  private detectRageClick(ev: InteractionEvent): FrustrationSignal[] {
    if (ev.action !== "click") return [];
    const recent = this.buffer.filter((e) => e.elementId === ev.elementId && e.action === "click" && ev.at - e.at <= RAGE_WINDOW_MS);
    if (recent.length >= RAGE_COUNT && recent.length % RAGE_COUNT === 0) {
      const sig: FrustrationSignal = { kind: "rage-click", elementId: ev.elementId, at: ev.at, message: `1.5 秒内连点 ${recent.length} 次——按钮没给出可信反馈或没起作用` };
      this.signals.push(sig);
      return [sig];
    }
    return [];
  }

  private deadClick(ev: InteractionEvent): FrustrationSignal {
    const sig: FrustrationSignal = { kind: "dead-click", elementId: ev.elementId, at: ev.at, message: "点击无可反馈区域（dead click）——元素要么不可点却像可点，要么反馈丢失" };
    this.signals.push(sig);
    return sig;
  }

  /** 错误后徘徊（同页错误态下继续多点——无路可走的信号）。 */
  recordErrorWander(page: string, at: number): FrustrationSignal {
    const sig: FrustrationSignal = { kind: "error-wander", elementId: page, at, message: "错误呈现后用户仍在原地多点——下一步引导不够清晰" };
    this.signals.push(sig);
    return sig;
  }

  get events(): readonly InteractionEvent[] {
    return this.buffer;
  }

  get frustrations(): readonly FrustrationSignal[] {
    return this.signals;
  }

  /** 最挫败十次清单（十三章的可执行产物——改进直出）。 */
  topFrustrations(limit = 10): FrustrationSignal[] {
    return [...this.signals].sort((a, b) => b.at - a.at).slice(0, limit);
  }

  /** 回放故事线（时间轴串起可读操作序列）。 */
  replay(fromMs: number, toMs: number): string[] {
    return this.buffer
      .filter((e) => e.at >= fromMs && e.at <= toMs)
      .map((e) => `${e.at} ${e.page}/${e.elementId} ${e.action} → ${e.verdict}${e.feedbackMs !== null ? `（反馈 ${e.feedbackMs}ms）` : ""}`);
  }

  /** 导出（零内容字段——结构即隐私；含挫败信号与统计）。 */
  exportLedger(now: number): { format: "vx-interaction-ledger"; version: 1; exportedAt: number; events: InteractionEvent[]; frustrations: FrustrationSignal[]; stats: { total: number; smoothRatio: number; p95FeedbackMs: number | null } } {
    const withFeedback = this.buffer.filter((e) => e.feedbackMs !== null).map((e) => e.feedbackMs!).sort((a, b) => a - b);
    const p95 = withFeedback.length > 0 ? withFeedback[Math.min(withFeedback.length - 1, Math.ceil(withFeedback.length * 0.95) - 1)]! : null;
    const smooth = this.buffer.filter((e) => e.verdict === "smooth").length;
    return {
      format: "vx-interaction-ledger",
      version: 1,
      exportedAt: now,
      events: [...this.buffer],
      frustrations: [...this.signals],
      stats: { total: this.buffer.length, smoothRatio: this.buffer.length > 0 ? Math.round((smooth / this.buffer.length) * 1000) / 1000 : 1, p95FeedbackMs: p95 },
    };
  }

  clear(): void {
    this.buffer = [];
    this.signals = [];
    this.popoverToggles.clear();
  }
}
