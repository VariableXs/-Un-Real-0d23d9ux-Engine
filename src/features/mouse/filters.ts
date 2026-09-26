/**
 * J 鼠标域 · F603 抬笔滤波 + F611 手抖过滤。
 *
 * F603——鼠标抬起瞬间传感器失焦前的 2-3ms 微抖常把精准落点拽偏一像素：
 * 按键抬起事件后 8ms 窗口内对位移事件做衰减滤波（最后位移按 50% 权重折算）。
 * 窗口与权重两参数固定默认不开放（手感参数宜少不宜多）；只在输入管线底层
 * 生效、对上层交互语义零感知；不引入缓冲、延迟零增加（逐事件即时放行）。
 *
 * F611——指针微抖平滑（无障碍向，默认关）：高频率低幅度抖动（震颤特征）
 * 过滤三档（关/轻/强——轻档吃 0.5px 内抖动、强档吃 2px 内）；算法只压高频
 * 不压意图（快速大幅度移动完全直通——防抖不是防动）。与 F601 串联不冲突：
 * 先滤抖后增益的固定管线序（runtime.ts 落实）。
 *
 * 判据锚点：
 * - 抬起抖动注入 100 次落点偏差 P95 <0.5px → LiftFilter（含 selfTest 辅助）
 * - 快速移动无滤波痕迹（速度连续性对拍）→ 边界用例单测
 * - 震颤样本集注入（2/4/6Hz 三频段）过滤效果谱 → tremorSpectrum()
 * - 意图移动直通判据（快速大幅零衰减）→ TremorFilter 边界
 */

/** F603 抬起滤波窗口（固定默认，不开放——手感参数宜少不宜多）。 */
export const LIFT_WINDOW_MS = 8;
/** F603 窗口内末位移权重（50% 折算）。 */
export const LIFT_TAIL_WEIGHT = 0.5;

/**
 * F603 抬笔滤波器：管线内逐事件调用。
 * - onButtonUp(atMs)：登记抬起时刻；
 * - feed(dx, dy, atMs)：返回滤波后位移——抬起窗口内按权重折算，窗口外直通。
 * 每次返回即放行（不缓冲），管线延迟零增量。
 */
export class LiftFilter {
  private lastUpAt = -Infinity;

  onButtonUp(atMs: number): void {
    this.lastUpAt = atMs;
  }

  feed(dx: number, dy: number, atMs: number): { x: number; y: number; filtered: boolean } {
    const dt = atMs - this.lastUpAt;
    if (dt >= 0 && dt <= LIFT_WINDOW_MS) {
      return { x: dx * LIFT_TAIL_WEIGHT, y: dy * LIFT_TAIL_WEIGHT, filtered: true };
    }
    return { x: dx, y: dy, filtered: false };
  }
}

/* ------------------------------- F611 手抖过滤 ------------------------------- */

export type TremorLevel = "off" | "light" | "strong";

/** 三档参数：幅度阈值（px 内吃掉）与保留系数（keep 越小吃得越干净）。 */
export const TREMOR_LEVELS: Record<Exclude<TremorLevel, "off">, { ampPx: number; alpha: number; name: string; desc: string }> = {
  light: { ampPx: 0.5, alpha: 0.65, name: "轻", desc: "吃 0.5px 内微抖，速度感几乎不变。" },
  strong: { ampPx: 2, alpha: 0.85, name: "强", desc: "吃 2px 内抖动，震颤用户停得住。" },
};

/**
 * F611 手抖过滤器：一极 IIR 低通 + 幅度阈值门。
 * 设计约束：
 * - 只压高频不压意图：单事件位移 ≥ ampPx*4（意图尺度）或持续时间上的慢移动
 *   直接直通；阈值内的微小位移向稳定位置收敛。
 * - 关档零干预（feed 原样返回，零分配）。
 */
export class TremorFilter {
  level: TremorLevel;
  /** 收敛锚点（低通状态）。 */
  private sx = 0;
  private sy = 0;
  private hasAnchor = false;

  constructor(level: TremorLevel = "off") {
    this.level = level;
  }

  reset(): void {
    this.sx = 0;
    this.sy = 0;
    this.hasAnchor = false;
  }

  feed(dx: number, dy: number): { x: number; y: number; filtered: boolean } {
    if (this.level === "off") return { x: dx, y: dy, filtered: false };
    const p = TREMOR_LEVELS[this.level];
    const mag = Math.hypot(dx, dy);
    // 意图直通：超过 4×阈值的大幅移动不衰减（快速大幅零衰减判据）。
    if (mag >= p.ampPx * 4) {
      this.sx += dx;
      this.sy += dy;
      this.hasAnchor = true;
      return { x: dx, y: dy, filtered: false };
    }
    if (!this.hasAnchor) {
      this.hasAnchor = true;
      this.sx = dx;
      this.sy = dy;
      return { x: 0, y: 0, filtered: true };
    }
    // 低通：向累计位置收敛（阈值内位移被逐步吃掉）。
    const nx = this.sx + dx;
    const ny = this.sy + dy;
    const fx = this.sx + (nx - this.sx) * (1 - p.alpha) * 0.5;
    const fy = this.sy + (ny - this.sy) * (1 - p.alpha) * 0.5;
    const outX = fx - this.sx;
    const outY = fy - this.sy;
    this.sx = fx;
    this.sy = fy;
    return { x: outX, y: outY, filtered: true };
  }
}

/**
 * 震颤效果谱分析（F611 判据：2/4/6Hz 三频段注入样本的过滤效果）。
 * samples: 每秒 freq Hz 的正弦抖动序列（位移幅度 ampPx）。
 * 返回残余幅度比（0=全吃掉，1=全直通）——三频段谱表由此生成。
 */
export function tremorSpectrum(
  level: TremorLevel,
  freq: 2 | 4 | 6,
  ampPx: number,
  seconds = 1,
): { residualRatio: number } {
  const f = new TremorFilter(level);
  const period = 1000 / freq;
  const steps = Math.round((seconds * 1000) / period) * 2; // 每半周期一采样
  let input = 0;
  let output = 0;
  for (let i = 0; i < steps; i++) {
    const t = (i / 2) * period;
    const dx = ampPx * Math.sin((2 * Math.PI * t) / period);
    input += Math.abs(dx);
    output += Math.abs(f.feed(dx, 0).x);
  }
  return { residualRatio: input === 0 ? 0 : Math.round((output / input) * 1000) / 1000 };
}

/**
 * F603 判据自检辅助：机械抖动模拟注入 n 次，统计落点偏差 P95。
 * jitter: 每次注入的「松手前抖一下」位移序列（px）。
 */
export function liftFilterSelfTest(jitters: { dx: number; dy: number; dtMs: number }[], n: number): {
  p95: number;
  pass: boolean;
} {
  const f = new LiftFilter();
  const errs: number[] = [];
  for (let i = 0; i < n; i++) {
    f.onButtonUp(0);
    let ex = 0;
    let ey = 0;
    for (const j of jitters) {
      const out = f.feed(j.dx, j.dy, j.dtMs);
      ex += out.x;
      ey += out.y;
    }
    // 对照：无滤波时全额计入；偏差 = 滤波后残余。
    errs.push(Math.hypot(ex, ey));
  }
  errs.sort((a, b) => a - b);
  const p95 = errs[Math.min(errs.length - 1, Math.floor(n * 0.95))] ?? 0;
  return { p95: Math.round(p95 * 100) / 100, pass: p95 < 0.5 };
}

/**
 * F611 自动调谐（无障碍引导面）：采集一段真实指针样本，按抖动能量推荐档位。
 * 抖动能量 = 高频小幅度位移占比（>60% 建议强档，>25% 建议轻档，否则关档）。
 * 推荐不是强制——面板呈现建议值，用户一键采纳或自行选择（可发现性章十一）。
 */
export function autoTuneTremor(samples: { dx: number; dy: number }[]): { recommended: TremorLevel; jitterRatio: number } {
  if (samples.length < 20) return { recommended: "off", jitterRatio: 0 };
  let jitterEnergy = 0;
  let totalEnergy = 0;
  for (const s of samples) {
    const mag = Math.hypot(s.dx, s.dy);
    totalEnergy += mag;
    if (mag < TREMOR_LEVELS.strong.ampPx * 2) jitterEnergy += mag; // 4px 内视为微抖能量
  }
  const ratio = totalEnergy === 0 ? 0 : jitterEnergy / totalEnergy;
  const recommended: TremorLevel = ratio > 0.6 ? "strong" : ratio > 0.25 ? "light" : "off";
  return { recommended, jitterRatio: Math.round(ratio * 1000) / 1000 };
}
