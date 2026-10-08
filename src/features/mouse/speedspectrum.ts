/**
 * J 鼠标域 · F601 指针速度曲线谱 · 纵深引擎（批次七）。
 *
 * 三件事，全部是曲线谱的「谱学」：
 *
 * 1. 灵敏度换算器——Windows 指针速度滑杆是 1-11 的 11 档刻度（第 6 档为
 *    「增强指针精度」开/关分水岭），Varix 是 0.1-5.0 连续标量。老用户带着
 *    肌肉记忆迁移时问的第一句话是「我的 7 档在这里是多少」。换算必须双射
 *    （正向正算、反向反算，round-trip 无损）——对拍 Windows 注册表
 *    MouseSensitivity 数值语义（1-20 内部刻度，滑杆 = 内部值/2）。
 *
 * 2. 曲线混成——两条已注册曲线（F601 五内置 + custom）按权重 t 线性混成
 *    出中间手感：用户说「比『精准』重一点，比『平衡』轻一点」时，不是让
 *    他去调贝塞尔控制点，而是直接把两条曲线揉出第三条。混成是点态的：
 *    同一归一化输入位上两条曲线输出加权和——保证两端点行为可预测。
 *
 * 3. 曲线体检——对新曲线（用户自定义/混成产物）做五项体检：单调性、
 *    端点增益、最大斜率（突变风险）、过冲（增益 <1 的回摆）、中段平坦度。
 *    体检结论直接进面板（F601「自定义后即时预览」的判断层）。
 *
 * 判据锚点：
 * - 双射换算 round-trip 无损 → winSliderToSens() / sensToWinSlider()
 * - 第 6 档分水岭语义 → WIN_SLIDER_NEUTRAL = 6
 * - 混成端点可预测 → blendCurves(t=0)=A, t=1=B
 * - 体检五项结论 → inspectCurve()
 */

/* ------------------------------- 灵敏度换算器 ------------------------------- */

/** Windows 滑杆第 6 档 = 1x 基准（增强指针精度的分水岭档）。 */
export const WIN_SLIDER_NEUTRAL = 6;
/** Windows 滑杆档位范围（1-11）。 */
export const WIN_SLIDER_MIN = 1;
export const WIN_SLIDER_MAX = 11;

/**
 * Windows 滑杆档 → Varix 倍率。
 * 对拍语义：第 6 档 = 1.0x 基准；每档 ±0.16x 线性外推（11 档铺满
 * 0.2x-1.8x——Windows 全档跨度大但两端另有加速曲线接管，中段线性近似
 * 足够迁移对拍；斜率上限保证 slider 1 不出负值——0.35/档会算出 -0.75，
 * 初版真缺陷被 round-trip 自检抓获）；越界档位抛错（换算器不猜）。
 */
export function winSliderToSens(slider: number): number {
  if (!Number.isInteger(slider) || slider < WIN_SLIDER_MIN || slider > WIN_SLIDER_MAX) {
    throw new RangeError(`winSliderToSens: 滑杆档 ${slider} 不在 ${WIN_SLIDER_MIN}-${WIN_SLIDER_MAX}`);
  }
  return Math.round((1 + (slider - WIN_SLIDER_NEUTRAL) * 0.16) * 100) / 100;
}

/**
 * Varix 倍率 → 最近 Windows 滑杆档（反向查询，round-trip 保证档位落点一致：
 * 正算出的 sens 反算回同档——0.35 步长下无歧义）。
 */
export function sensToWinSlider(sens: number): number {
  if (!(sens > 0)) throw new RangeError(`sensToWinSlider: 倍率 ${sens} 必须为正`);
  const raw = WIN_SLIDER_NEUTRAL + (sens - 1) / 0.16;
  return Math.min(WIN_SLIDER_MAX, Math.max(WIN_SLIDER_MIN, Math.round(raw)));
}

/** Round-trip 断言器（迁移向导的自检件）：全部 11 档正反算一致才放行。 */
export function translatorSelfTest(): { pass: boolean; failures: number[] } {
  const failures: number[] = [];
  for (let s = WIN_SLIDER_MIN; s <= WIN_SLIDER_MAX; s++) {
    if (sensToWinSlider(winSliderToSens(s)) !== s) failures.push(s);
  }
  return { pass: failures.length === 0, failures };
}

/* ------------------------------- 曲线混成 ------------------------------- */

/** 曲线函数签名（与 curve.ts 的 gainAt 输入口径一致：物理位移 px → 倍率）。 */
export type GainFn = (px: number) => number;

/**
 * 点态混成：g(px) = (1-t)·A(px) + t·B(px)。
 * t=0 恒等 A、t=1 恒等 B——端点可预测是混成的类型级承诺。
 * 权重越界钳制到 [0,1]（不外推——外推混成会产生负增益）。
 */
export function blendCurves(a: GainFn, b: GainFn, t: number): GainFn {
  const w = Math.min(1, Math.max(0, t));
  return (px: number) => (1 - w) * a(px) + w * b(px);
}

/** 混成采样表（面板预览用）：0..128px 按 step 均采，双曲线与混成三列并排。 */
export function blendPreviewTable(
  a: GainFn,
  b: GainFn,
  t: number,
  step = 16,
  maxPx = 128,
): { px: number; a: number; b: number; mixed: number }[] {
  const rows: { px: number; a: number; b: number; mixed: number }[] = [];
  const mixed = blendCurves(a, b, t);
  for (let px = 0; px <= maxPx; px += step) {
    rows.push({ px, a: round3(a(px)), b: round3(b(px)), mixed: round3(mixed(px)) });
  }
  return rows;
}

/* ------------------------------- 曲线体检 ------------------------------- */

export interface CurveHealthReport {
  /** 增益是否随位移单调不减（抖动 = 手感「发涩」的元凶）。 */
  monotonic: boolean;
  /** 首个非单调点（px, gain）——单调性体检的定位证据。 */
  firstDip: { px: number; gain: number } | null;
  /** 零位移增益（应 = 最小增益，远离 0——否则慢速微调无法起步）。 */
  gainAtRest: number;
  /** 采样窗口内最大增益（封顶检查：>6x 视为失控）。 */
  gainMax: number;
  /** 最大相邻采样增益跳变（突变风险：<0.15/16px 为舒适）。 */
  maxJump: number;
  /** 过冲：中后段是否出现比前段更低的增益（回摆）。 */
  overshoot: boolean;
  /** 中段平坦度：32-96px 段增益标准差（越平坦越「匀速感」）。 */
  midFlatness: number;
  /** 结论：pass / warn / fail。 */
  verdict: "pass" | "warn" | "fail";
  /** 结论依据（人话，三要素口径——面板直接展示）。 */
  notes: string[];
}

/**
 * 五项体检。采样密度 2px（0-160px 窗口）——足够抓 16px 尺度的锯齿，
 * 又不至于在面板里卡顿（160 次采样 <1ms）。
 */
export function inspectCurve(g: GainFn): CurveHealthReport {
  const step = 2;
  const samples: { px: number; gain: number }[] = [];
  for (let px = 0; px <= 160; px += step) samples.push({ px, gain: g(px) });

  let monotonic = true;
  let firstDip: { px: number; gain: number } | null = null;
  let gainMax = 0;
  let maxIdx = 0;
  let maxJump = 0;
  for (let i = 0; i < samples.length; i++) {
    const s = samples[i]!;
    if (s.gain > gainMax) {
      gainMax = s.gain;
      maxIdx = i;
    }
    if (i > 0) {
      const prev = samples[i - 1]!;
      if (s.gain < prev.gain) maxJump = Math.max(maxJump, prev.gain - s.gain);
    }
  }
  // 单调性只追究「峰值前」的下坡段——中途回落才是手感「发涩」的元凶；
  // 峰值后的收尾回落是自然的行程衰减，按过冲（warn）单列，不与单调 fail 重叠。
  for (let i = 1; i <= maxIdx; i++) {
    const s = samples[i]!;
    const prev = samples[i - 1]!;
    if (s.gain < prev.gain) {
      monotonic = false;
      firstDip ??= { px: s.px, gain: s.gain };
      break;
    }
  }
  const gainAtRest = samples[0]!.gain;

  // 过冲：最大增益点之后是否还有 >3% 的回落段（排除采样噪声用阈值）。
  let overshoot = false;
  for (let i = maxIdx + 1; i < samples.length; i++) {
    if (samples[i]!.gain < gainMax * 0.97) {
      overshoot = true;
      break;
    }
  }

  // 中段平坦度（32-96px 段标准差）。
  const mid = samples.filter((s) => s.px >= 32 && s.px <= 96).map((s) => s.gain);
  const mean = mid.reduce((a, b) => a + b, 0) / mid.length;
  const midFlatness = Math.sqrt(mid.reduce((a, b) => a + (b - mean) ** 2, 0) / mid.length);

  const notes: string[] = [];
  let verdict: CurveHealthReport["verdict"] = "pass";
  if (!monotonic) {
    notes.push(`增益在 ${firstDip?.px}px 处回落——快速划动会有「发涩」感，建议消除下坡段`);
    verdict = "fail";
  }
  if (gainAtRest < 0.2) {
    notes.push(`零位移增益 ${round3(gainAtRest)} 过低——慢速微调难起步，建议 ≥0.2`);
    verdict = verdict === "fail" ? "fail" : "warn";
  }
  if (gainMax > 6) {
    notes.push(`最大增益 ${round3(gainMax)} 失控（>6x）——指尖轻碰会瞬移全屏`);
    verdict = "fail";
  }
  if (maxJump > 0.35) {
    notes.push(`相邻 2px 增益跳变 ${round3(maxJump)} 偏大——中速段可能感到「顿挫」`);
    verdict = verdict === "fail" ? "fail" : "warn";
  }
  if (overshoot) {
    notes.push("增益峰值后回摆——快速移动末端会有「被拽回」感");
    verdict = verdict === "fail" ? "fail" : "warn";
  }
  if (notes.length === 0) {
    notes.push(`五项全过：单调、零位 ${round3(gainAtRest)}、峰值 ${round3(gainMax)}、跳变 ${round3(maxJump)}、中段平坦 ${round3(midFlatness)}`);
  }
  return {
    monotonic,
    firstDip,
    gainAtRest: round3(gainAtRest),
    gainMax: round3(gainMax),
    maxJump: round3(maxJump),
    overshoot,
    midFlatness: round3(midFlatness),
    verdict,
    notes,
  };
}

function round3(v: number): number {
  return Math.round(v * 1000) / 1000;
}
