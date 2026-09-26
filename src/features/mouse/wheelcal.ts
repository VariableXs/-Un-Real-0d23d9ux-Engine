/**
 * J 鼠标域 · 滚轮标定引擎（v5 · 深化批次五 · F605/F612）。
 *
 * v4 的滚轮链有两处「拍脑袋」：① 行模式下「3 行」到底折算多少像素——没人
 * 知道，各应用自行假设行高；② F612 的增益映射是手调线性（3..8 档/秒 →
 * 3..12 行），没有从真实滚轮节奏数据校准过。本模块把标定做成工程件：
 * - 行高估计：像素模式/行模式事件对（同一次滚动的两种口味）回归出
 *   px/行——应用真实行高入账，滚动位移从「行数玄学」变「像素实测」；
 * - 节奏-行数标定：采集（档/秒 → 用户实际行数偏好）样本，幂律拟合
 *   （log-log 最小二乘）出「这个人的增益曲线」——F612 从固定线性升级为
 *   可标定曲线（默认仍是出厂线性——标定是专家捷径，章十一渐进披露）；
 * - 标定向导状态机：采集→拟合→预览→采纳四步，中断可弃、采纳才生效。
 *
 * 消费端：滚轮标定向导面板（v5 新增）+ 单测。
 */

/* ------------------------------- 行高估计 ------------------------------- */

/** 单条滚动样本：同一时刻采到的行口径与像素口径位移。 */
export interface WheelPairSample {
  /** 行模式 deltaY（行数）。 */
  lines: number;
  /** 像素模式 deltaY（px，同一次滚动的另一容器口径）。 */
  px: number;
}

/** 行高估计器：对样本流做 EMA（稳健均值——离群样本慢慢淡出）。 */
export class LineHeightEstimator {
  private value: number | null = null;
  private n = 0;

  constructor(private readonly alpha = 0.2) {}

  /**
   * @returns 当前估计 px/行（样本 <3 返回 null——数字不够不出结论）。
   */
  feed(s: WheelPairSample): number | null {
    if (s.lines === 0 || s.px === 0) return this.value;
    const ratio = s.px / s.lines;
    if (ratio <= 0 || ratio > 400) return this.value; // 离群（1px 线高或 400px 异常）拒绝
    this.value = this.value === null ? ratio : this.value + this.alpha * (ratio - this.value);
    this.n++;
    return this.n < 3 ? null : Math.round(this.value! * 10) / 10;
  }

  get samples(): number {
    return this.n;
  }

  get estimate(): number | null {
    return this.n < 3 ? null : Math.round((this.value ?? 0) * 10) / 10;
  }
}

/* ------------------------------- 节奏-行数幂律拟合 ------------------------------- */

/** 标定样本：滚轮节奏（档/秒）与用户当次偏好行数。 */
export interface PaceSample {
  /** 档/秒。 */
  notchesPerSec: number;
  /** 用户在该节奏下选择的行数（向导第二问的答案）。 */
  lines: number;
}

/**
 * 幂律拟合 lines = k · pace^b（log-log 最小二乘）：
 * - b=0：与节奏无关的固定行数（慢滚党）；
 * - b>0：节奏越快行数越多（F612 语义）；
 * - 两点样本也拟合（过两点必精确），单点拒绝（拟合需要至少两点）。
 * 返回系数与拟合质量（R²），质量低时 honest 报告——不给「看起来对」的曲线。
 */
export function fitGainCurve(samples: PaceSample[]): { k: number; b: number; r2: number; ok: boolean; note: string } {
  if (samples.length < 2) {
    return { k: 1, b: 0, r2: 0, ok: false, note: "至少两档节奏样本才能拟合（向导会再问一次）" };
  }
  const xs = samples.map((s) => Math.log(Math.max(0.1, s.notchesPerSec)));
  const ys = samples.map((s) => Math.log(Math.max(0.1, s.lines)));
  const n = xs.length;
  const mx = xs.reduce((a, b) => a + b, 0) / n;
  const my = ys.reduce((a, b) => a + b, 0) / n;
  let sxy = 0;
  let sxx = 0;
  for (let i = 0; i < n; i++) {
    sxy += (xs[i]! - mx) * (ys[i]! - my);
    sxx += (xs[i]! - mx) ** 2;
  }
  const b = sxx === 0 ? 0 : sxy / sxx;
  const k = Math.exp(my - b * mx);
  // R²：拟合值对样本的对数方差解释率。
  let ssRes = 0;
  let ssTot = 0;
  for (let i = 0; i < n; i++) {
    const pred = Math.log(k) + b * xs[i]!;
    ssRes += (ys[i]! - pred) ** 2;
    ssTot += (ys[i]! - my) ** 2;
  }
  const r2 = ssTot === 0 ? 1 : Math.max(0, 1 - ssRes / ssTot);
  const ok = r2 >= 0.7;
  return {
    k: Math.round(k * 100) / 100,
    b: Math.round(b * 100) / 100,
    r2: Math.round(r2 * 1000) / 1000,
    ok,
    note: ok ? `幂律曲线拟合良好（R²=${r2.toFixed(2)}）` : "样本分散，拟合可信度低——建议多采几档节奏",
  };
}

/** 拟合曲线求值：节奏 → 行数（钳制 1..24，出厂线性即 k=3,b=0 特例）。 */
export function evalGainCurve(k: number, b: number, notchesPerSec: number): number {
  return Math.max(1, Math.min(24, k * Math.pow(Math.max(0.1, notchesPerSec), b)));
}

/** 预览表：6 档节奏采样（1/2/3/6/10/15 档每秒）——向导第三步的对照表。 */
export function previewTable(k: number, b: number): { pace: number; lines: number }[] {
  return [1, 2, 3, 6, 10, 15].map((pace) => ({ pace, lines: Math.round(evalGainCurve(k, b, pace) * 10) / 10 }));
}

/* ------------------------------- 标定向导状态机 ------------------------------- */

export type CalibStep = "idle" | "collect" | "fit" | "preview" | "done";

export interface CalibState {
  step: CalibStep;
  lineEst: number | null;
  paceSamples: PaceSample[];
  fit: { k: number; b: number; r2: number; ok: boolean; note: string } | null;
}

const CALIB_SAMPLE_CAP = 32;

/**
 * 标定向导（不可变状态机——每步返回新状态，面板纯渲染）：
 * collect（行高估计+节奏问答）→ fit（拟合）→ preview（预览表确认）→
 * done（采纳）。abort 随时回 idle——中断可弃（章三：状态机完整出路）。
 */
export class WheelCalibrationWizard {
  private st: CalibState = { step: "idle", lineEst: null, paceSamples: [], fit: null };

  private estimator = new LineHeightEstimator();

  get state(): CalibState {
    return { ...this.st, paceSamples: [...this.st.paceSamples] };
  }

  begin(): void {
    this.estimator = new LineHeightEstimator();
    this.st = { step: "collect", lineEst: null, paceSamples: [], fit: null };
  }

  /** 采一条行高对样本（运行时像素/行双口径事件喂入）。 */
  feedLinePair(s: WheelPairSample): void {
    if (this.st.step !== "collect") return;
    const est = this.estimator.feed(s);
    if (est !== null) this.st = { ...this.st, lineEst: est };
  }

  /** 记一档节奏问答（pace 档/秒，lines 用户偏好行数）。 */
  recordPace(pace: number, lines: number): void {
    if (this.st.step !== "collect") return;
    const next = [...this.st.paceSamples, { notchesPerSec: pace, lines }];
    if (next.length > CALIB_SAMPLE_CAP) next.splice(0, next.length - CALIB_SAMPLE_CAP);
    this.st = { ...this.st, paceSamples: next };
  }

  /** 采集完 → 拟合（样本不足显性拒绝，不静默给坏曲线）。 */
  fitNow(): CalibState {
    if (this.st.step !== "collect" || this.st.paceSamples.length < 2) {
      return { ...this.state, fit: { k: 1, b: 0, r2: 0, ok: false, note: "样本不足两档，无法拟合" } };
    }
    const fit = fitGainCurve(this.st.paceSamples);
    this.st = { ...this.st, fit, step: "preview" };
    return this.state;
  }

  /** 预览确认 → 采纳（返回生效参数；调用方写回 wheelGain 分节扩展字段）。 */
  accept(): { k: number; b: number; lineEst: number | null } | null {
    const fit = this.st.step === "preview" ? this.st.fit : null;
    if (!fit?.ok) return null;
    this.st = { ...this.st, step: "done" };
    return { k: fit.k, b: fit.b, lineEst: this.st.lineEst };
  }

  abort(): void {
    this.st = { step: "idle", lineEst: null, paceSamples: [], fit: null };
  }
}

/**
 * 与 F612 出厂线性的一致性对账：标定曲线在 3 与 8 档/秒两锚点行数与
 * 出厂线性（3→3 行、8→12 行）偏差 <35% 判「贴近出厂手感」——
 * 向导采纳前的最后一道提示（用户可无视，但数字要摆出来）。
 */
export function compareWithFactory(fit: { k: number; b: number }): { at3: number; at8: number; nearFactory: boolean } {
  const at3 = Math.round(evalGainCurve(fit.k, fit.b, 3) * 10) / 10;
  const at8 = Math.round(evalGainCurve(fit.k, fit.b, 8) * 10) / 10;
  const near = Math.abs(at3 - 3) <= 3 * 0.35 && Math.abs(at8 - 12) <= 12 * 0.35;
  return { at3, at8, nearFactory: near };
}
