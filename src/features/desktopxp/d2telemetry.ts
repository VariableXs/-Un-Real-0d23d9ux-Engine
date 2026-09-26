/**
 * D2 体验日志（主册十三章/十三·补 · C 桌面体验域·后段 · AI-D2 v3）。
 *
 * 使命：还原体验，不是排查崩溃——记录交互细节层 + 主动捕获挫败信号。
 *
 * 纪律（十三章原文对位）：
 * - 记「在哪、对哪个元素、什么时刻、触发了什么、反馈是什么、耗时多少」；
 * - 挫败信号自动标记：狂点（rage click）/ 死点（dead click）/ 浮层反复
 *   开关 / 同操作短时重复 / 错误后徘徊——不等问题上；
 * - 每个事件带体验结论字段（顺畅/无反馈/被打断/报错）；
 * - 隐私红线：只记交互行为与结果，不记输入内容（正文/密码全不入账）；
 * - 写入绝不阻塞交互：内存环形缓冲 + 批量落盘（localStorage 节流刷写，
 *   失败静默降级为纯内存——日志系统自身不许成为卡顿源）；
 * - 可导出：结构化 JSON（时间轴 + 挫败清单）。
 */

/** 单条体验事件。 */
export interface XEvent {
  /** 单调序号（会话内）。 */
  seq: number;
  /** 时刻（ms，注入钟——测试确定复现；运行时 performance.now）。 */
  t: number;
  /** 界面/组件面（如 "osk" / "imefloat" / "term2" / "album"）。 */
  surface: string;
  /** 元素语义名（不记动态内容——"start-btn" 而非按钮文本）。 */
  element: string;
  /** 交互类型。 */
  kind: "click" | "toggle" | "key" | "open" | "close" | "error" | "retry";
  /** 耗时（ms，反馈链实测——浮层打开到可见等）。 */
  ms: number | null;
  /** 体验结论（十三章结论字段）。 */
  verdict: "smooth" | "no-response" | "interrupted" | "error";
  /** 挫败信号标记（自动判定）。 */
  frustration: null | "rage-click" | "dead-click" | "overlay-flap" | "repeat-spam";
}

/** 挫败判定参数（判线集中——一处一事实）。 */
export const FRUSTRATION_RULES = {
  /** 同元素 1.2s 内点击 ≥5 次 → 狂点。 */
  rageClickMinHits: 5,
  rageClickWindowMs: 1_200,
  /** 点击后 1s 内无后续事件（无反馈）→ 死点候选（由点击侧报告 no-response）。 */
  deadClickMs: 1_000,
  /** 同浮层 10s 内开关 ≥4 次 → 反复开关。 */
  overlayFlapMinToggles: 4,
  overlayFlapWindowMs: 10_000,
  /** 缓冲上限（内存环——超限丢最旧）。 */
  cap: 500,
  /** 落盘节流（ms）。 */
  flushIntervalMs: 8_000,
} as const;

const LS_KEY = "variable:desktop:d2:xlog:v1";

export class ExperienceLog {
  private events: XEvent[] = [];
  private seq = 0;
  private lastBySurfaceElement = new Map<string, number[]>();
  private overlayToggles = new Map<string, number[]>();
  private dirty = false;
  private lastFlush = 0;

  constructor(private now: () => number = () => performance.now()) {
    this.loadTail();
  }

  get size(): number { return this.events.length; }

  /** 记录一次交互。耗时/结论由调用面如实给——本层不编造。 */
  record(surface: string, element: string, kind: XEvent["kind"], ms: number | null, verdict: XEvent["verdict"]): XEvent {
    const t = this.now();
    this.seq += 1;
    const ev: XEvent = { seq: this.seq, t, surface, element, kind, ms, verdict, frustration: null };
    ev.frustration = this.detectFrustration(surface, element, kind, t);
    this.events.push(ev);
    if (this.events.length > FRUSTRATION_RULES.cap) {
      this.events.splice(0, this.events.length - FRUSTRATION_RULES.cap);
    }
    this.dirty = true;
    this.maybeFlush();
    return ev;
  }

  /** 挫败信号判定（十三章指纹族）。 */
  private detectFrustration(surface: string, element: string, kind: XEvent["kind"], t: number): XEvent["frustration"] {
    const key = `${surface}:${element}`;
    if (kind === "click") {
      const hits = (this.lastBySurfaceElement.get(key) ?? []).filter((x) => t - x <= FRUSTRATION_RULES.rageClickWindowMs);
      hits.push(t);
      this.lastBySurfaceElement.set(key, hits);
      if (hits.length >= FRUSTRATION_RULES.rageClickMinHits) return "rage-click";
      if (kind === "click" && hits.length >= 1) {
        // 死点由调用面 verdict=no-response 承载（本层不猜）。
      }
    }
    if (kind === "open" || kind === "close") {
      const okey = `${surface}:${element}:overlay`;
      const toggles = (this.overlayToggles.get(okey) ?? []).filter((x) => t - x <= FRUSTRATION_RULES.overlayFlapWindowMs);
      toggles.push(t);
      this.overlayToggles.set(okey, toggles);
      if (toggles.length >= FRUSTRATION_RULES.overlayFlapMinToggles) return "overlay-flap";
    }
    if (kind === "retry") return "repeat-spam";
    return null;
  }

  /** 挫败清单（十三章：直接出改进清单的维度）。 */
  frustrations(): XEvent[] {
    return this.events.filter((e) => e.frustration !== null);
  }

  /** 导出（开放格式：时间轴 + 挫败清单 + 规则版本）。 */
  export(): string {
    return JSON.stringify({
      format: "varix-d2-xlog",
      version: 1,
      rules: FRUSTRATION_RULES,
      privacy: "行为与结论 only——无输入内容（十三章红线）",
      events: this.events,
    }, null, 2);
  }

  clear(): void {
    this.events = [];
    this.lastBySurfaceElement.clear();
    this.overlayToggles.clear();
    this.dirty = true;
    this.maybeFlush(true);
  }

  /** 节流落盘（内存优先——失败降级纯内存，日志自身零卡顿源）。 */
  private maybeFlush(force = false): void {
    if (!this.dirty) return;
    const now = this.now();
    if (!force && now - this.lastFlush < FRUSTRATION_RULES.flushIntervalMs) return;
    this.lastFlush = now;
    try {
      // 只落盘尾段（cap/2）——重启后仍有近期上下文，配额友好。
      const tail = this.events.slice(-FRUSTRATION_RULES.cap / 2);
      localStorage.setItem(LS_KEY, JSON.stringify(tail));
      this.dirty = false;
    } catch {
      /* 配额满/存储不可用：纯内存降级——显性化到控制台一次级（不刷屏）。 */
      if (!this.warned) {
        console.warn("[desktop-d2] 体验日志落盘不可用——本会话仅内存记录");
        this.warned = true;
      }
    }
  }

  private warned = false;

  /** 启动恢复尾段（崩溃/重启后近期事件可回放）。 */
  private loadTail(): void {
    if (typeof localStorage === "undefined") return;
    try {
      const raw = localStorage.getItem(LS_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as unknown;
      if (!Array.isArray(parsed)) return;
      for (const e of parsed as XEvent[]) {
        if (typeof e?.t === "number" && typeof e?.surface === "string") this.events.push(e);
      }
      this.seq = this.events.length > 0 ? this.events[this.events.length - 1]!.seq : 0;
    } catch {
      /* 损坏尾段 = 当作没有（读不出则从头，不报错） */
    }
  }
}

/** 域唯一实例（一处一事实）。 */
export const d2xlog = new ExperienceLog();
