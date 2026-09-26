/**
 * J 鼠标域 · J1 体验日志（体验章十三/十三·补 执法件）。
 *
 * 使命不是排查崩溃，是还原体验：把 J1 域的每次交互事件记到交互细节层
 * （哪类交互、对哪类目标、什么反馈、耗时多少），并主动捕获挫败信号——
 * 狂点（rage click）、死点（dead click）、手势中途放弃、锚标秒开秒关。
 *
 * 隐私与代价红线（判据原文）：
 * - 只记交互行为与结果，不记用户输入内容（无文本、无剪贴板、无坐标全文——
 *   坐标只记象限与粗粒度格）；
 * - 环形缓冲 500 条上限，写入 O(1) 不阻塞交互（无 IO，导出才序列化）；
 * - 可整体关闭（telemetry.enabled=false 时全部调用零分配直通）。
 */

export type J1EventKind =
  | "click"
  | "wheel"
  | "gesture"
  | "gesture-aborted"
  | "side-button"
  | "autoscroll-start"
  | "autoscroll-exit"
  | "seam-hold"
  | "magnet-snap"
  | "slow-tune"
  | "profile-switch";

/** 每条事件：时间 / 类型 / 结论字段（顺畅/卡顿/无反馈/被打断/报错）+ 粗粒度上下文。 */
export interface J1Event {
  at: number;
  kind: J1EventKind;
  /** 体验结论字段（十三章判据：日志能直接回答"这一步舒不舒服"）。 */
  verdict: "smooth" | "laggy" | "no-feedback" | "interrupted" | "error";
  /** 粗粒度目标描述（控件角色/类目，不含文本内容）。 */
  target: string;
  /** 耗时（ms，可时省略为 0）。 */
  durMs: number;
  /** 粗粒度位置格（屏幕 3×3 象限，隐私红线：不记精确坐标）。 */
  zone: 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9;
}

/** 挫败信号事件（自动标记成体验事件，不用等投诉）。 */
export interface FrustrationSignal {
  at: number;
  kind: "rage-click" | "dead-click" | "gesture-abandoned" | "anchor-bounce";
  detail: string;
  zone: J1Event["zone"];
}

const RING_CAP = 500;
/** 狂点判定：2s 内同格 ≥3 次点击。 */
const RAGE_WINDOW_MS = 2000;
const RAGE_COUNT = 3;
/** 死点判定：点击目标与上次完全相同且无反馈事件回流。 */
const DEAD_CLICK_MIN_GAP_MS = 60000;

function zoneOf(x: number, y: number): J1Event["zone"] {
  const w = window.innerWidth || 1920;
  const h = window.innerHeight || 1080;
  const col = x < w / 3 ? 1 : x < (w * 2) / 3 ? 2 : 3;
  const row = y < h / 3 ? 0 : y < (h * 2) / 3 ? 3 : 6;
  return (row + col) as J1Event["zone"];
}

export class J1Telemetry {
  enabled = true;
  private ring: J1Event[] = [];
  private frustrations: FrustrationSignal[] = [];
  private clickTimes: { at: number; zone: J1Event["zone"]; target: string }[] = [];
  private lastClick: { at: number; target: string; feedbackSeen: boolean } | null = null;
  private anchorOpenedAt = 0;

  /** 记录一条交互事件（关闭态零分配直通）。 */
  log(kind: J1EventKind, verdict: J1Event["verdict"], target: string, x: number, y: number, durMs = 0): void {
    if (!this.enabled) return;
    this.ring.push({ at: Date.now(), kind, verdict, target, durMs, zone: zoneOf(x, y) });
    if (this.ring.length > RING_CAP) this.ring.shift();
    if (kind === "click") this.trackClick(verdict, target, x, y);
  }

  private trackClick(verdict: J1Event["verdict"], target: string, x: number, y: number): void {
    const now = Date.now();
    const zone = zoneOf(x, y);
    // 狂点：窗口内同格计数。
    this.clickTimes = this.clickTimes.filter((c) => now - c.at <= RAGE_WINDOW_MS);
    this.clickTimes.push({ at: now, zone, target });
    const sameZone = this.clickTimes.filter((c) => c.zone === zone).length;
    if (sameZone >= RAGE_COUNT) {
      this.frustrations.push({ at: now, kind: "rage-click", detail: `${sameZone} 次点击落在同格（${RAGE_WINDOW_MS}ms 内）`, zone });
      this.clickTimes = [];
    }
    // 死点：同目标连续点击且前次无反馈结论。
    if (this.lastClick && this.lastClick.target === target && !this.lastClick.feedbackSeen && now - this.lastClick.at < DEAD_CLICK_MIN_GAP_MS) {
      this.frustrations.push({ at: now, kind: "dead-click", detail: `同目标重复点击且前次无反馈（${target}）`, zone });
    }
    this.lastClick = { at: now, target, feedbackSeen: verdict !== "no-feedback" };
  }

  /** 锚标生命线（开/关成对记，秒开秒关=锚标弹跳挫败信号）。 */
  anchorLifecycle(phase: "open" | "close", x: number, y: number): void {
    if (!this.enabled) return;
    if (phase === "open") {
      this.anchorOpenedAt = Date.now();
      this.log("autoscroll-start", "smooth", "autoscroll-anchor", x, y);
    } else {
      const dur = Date.now() - this.anchorOpenedAt;
      this.log("autoscroll-exit", dur < 300 ? "interrupted" : "smooth", "autoscroll-anchor", x, y, dur);
      if (dur < 300) {
        this.frustrations.push({ at: Date.now(), kind: "anchor-bounce", detail: `锚标 ${dur}ms 内开合（误触中键？）`, zone: zoneOf(x, y) });
      }
    }
  }

  gestureOutcome(hit: boolean, steps: number, x: number, y: number): void {
    if (!this.enabled) return;
    if (hit) this.log("gesture", "smooth", `gesture(${steps} 步)`, x, y);
    else this.log("gesture-aborted", "no-feedback", `gesture(${steps} 步)`, x, y);
    // 手势中途放弃：画了轨迹但识别失败=被打断（墨迹淡出用户却什么都没发生）。
    if (!hit && steps >= 2) {
      this.frustrations.push({ at: Date.now(), kind: "gesture-abandoned", detail: `${steps} 步轨迹未命中任何手势`, zone: zoneOf(x, y) });
    }
  }

  /** 查询：最挫败的十次（直接出体验改进清单的口径）。 */
  worstTen(): FrustrationSignal[] {
    return [...this.frustrations].slice(-10).reverse();
  }

  /** 导出：统一时间轴 JSON（可回放的操作故事线）。 */
  exportTimeline(): string {
    return JSON.stringify(
      {
        format: "vx-j1-telemetry",
        version: 1,
        exportedAt: Date.now(),
        events: this.ring,
        frustrations: this.frustrations,
        summary: {
          total: this.ring.length,
          byVerdict: this.ring.reduce<Record<string, number>>((acc, e) => {
            acc[e.verdict] = (acc[e.verdict] ?? 0) + 1;
            return acc;
          }, {}),
          frustrationCount: this.frustrations.length,
        },
      },
      null,
      2,
    );
  }

  /** 清空（隐私控制入口——用户能看见、能控制、能撤销）。 */
  clear(): void {
    this.ring = [];
    this.frustrations = [];
    this.clickTimes = [];
    this.lastClick = null;
  }

  get size(): number {
    return this.ring.length;
  }

  get frustrationCount(): number {
    return this.frustrations.length;
  }
}

/** J1 域唯一遥测实例（一处一事实；面板与 runtime 共用）。 */
export const j1Telemetry = new J1Telemetry();
