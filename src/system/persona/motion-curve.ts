/**
 * F124/F160 动效曲线深化 · 五曲线解析求值 + spring 解析解 + 打断可逆进度。
 *
 * 主册判据延伸：
 * - F124「全系统动画抽查 30 处曲线/时长全部落在总谱表内」——总谱表的
 *   数学面：cubic-bezier 求解器（牛顿迭代 + 二分兜底）、spring 解析解
 *   （阻尼比 <1 的欠阻尼振荡——F124 spring 曲线的物理意义）；
 * - F160「打断恢复」——动画打断后的可逆进度模型：从当前速度续跑而非
 *   从头播放（大作过场级的打断品质）；
 * - 三档强度缩放（100%/60%/0%）联动 F160 档位。
 */

// ---------- cubic-bezier 求解（CSS 缓动的数学真身） ----------

export interface Bezier {
  x1: number;
  y1: number;
  x2: number;
  y2: number;
}

/** 参数 t 下的 x/y（Bernstein 形式）。 */
function bezierAt(p1: number, p2: number, t: number): number {
  const omt = 1 - t;
  return 3 * omt * omt * t * p1 + 3 * omt * t * t * p2 + t * t * t;
}

/** 给定目标 x 反解参数 t（牛顿 8 步 + 二分兜底——CSS 规范同款算法）。 */
export function bezierXtoT(b: Bezier, x: number): number {
  if (x <= 0) return 0;
  if (x >= 1) return 1;
  let t = x;
  for (let i = 0; i < 8; i++) {
    const bx = bezierAt(b.x1, b.x2, t) - x;
    if (Math.abs(bx) < 1e-6) return t;
    const dx = 3 * (1 - t) * (1 - t) * b.x1 + 6 * (1 - t) * t * (b.x2 - b.x1) + 3 * t * t * (1 - b.x2);
    if (Math.abs(dx) < 1e-6) break;
    t -= bx / dx;
  }
  let lo = 0;
  let hi = 1;
  t = x;
  for (let i = 0; i < 24; i++) {
    const bx = bezierAt(b.x1, b.x2, t);
    if (Math.abs(bx - x) < 1e-6) return t;
    if (bx < x) lo = t;
    else hi = t;
    t = (lo + hi) / 2;
  }
  return t;
}

/** 进度 x∈[0,1] → 缓动值 y∈[0,1]（enter/exit/emphasized/linear 共用）。 */
export function bezierEase(b: Bezier, x: number): number {
  return bezierAt(b.y1, b.y2, bezierXtoT(b, x));
}

/** F124 五曲线（与 tokens.ts MOTION_CURVES 的 bezier 字符串同参数——一处一事实的解析形态）。 */
export const CURVES: Record<string, Bezier & { name: string; zh: string }> = {
  enter: { x1: 0.16, y1: 1, x2: 0.3, y2: 1, name: "enter", zh: "进入" },
  exit: { x1: 0.7, y1: 0, x2: 0.84, y2: 0, name: "exit", zh: "退出" },
  emphasized: { x1: 0.65, y1: 0, x2: 0.35, y2: 1, name: "emphasized", zh: "强调" },
  spring: { x1: 0.34, y1: 1.56, x2: 0.64, y2: 1, name: "spring", zh: "弹性 105% 过冲" },
  linear: { x1: 0, y1: 0, x2: 1, y2: 1, name: "linear", zh: "线性（仅进度条）" },
};

// ---------- spring 解析解（欠阻尼——真实弹性的时域形状） ----------

export interface SpringParams {
  /** 初始速度（打断续跑的接口面）。 */
  v0: number;
  /** 无阻尼固有频率（rad/s）。 */
  omega0: number;
  /** 阻尼比（<1 欠阻尼振荡；F124 spring ≈ 0.62——105% 过冲对应值）。 */
  zeta: number;
}

/**
 * 欠阻尼 spring 在时刻 t 的归一位置（目标 1）：
 * x(t) = 1 - e^(-ζωt) (cos(ωd t) + (ζω/ωd) sin(ωd t))，ωd = ω√(1-ζ²)。
 */
export function springAt(p: SpringParams, tSec: number): number {
  if (p.zeta >= 1) return 1; // 过阻尼/临界——解析式退化为单调（本域不使用）。
  const wd = p.omega0 * Math.sqrt(1 - p.zeta * p.zeta);
  const env = Math.exp(-p.zeta * p.omega0 * tSec);
  return 1 - env * (Math.cos(wd * tSec) + ((p.zeta * p.omega0) / wd) * Math.sin(wd * tSec));
}

/** 过冲峰值与发生时刻（105% 契约的验证：过冲率 = e^(-ζπ/√(1-ζ²)) = 0.05 → ζ = 0.6901）。 */
export const SPRING_OVERSHOOT_ZETA = 0.6901;

export function springOvershoot(p: SpringParams): { peak: number; atSec: number } {
  const wd = p.omega0 * Math.sqrt(1 - p.zeta * p.zeta);
  const tPeak = Math.PI / wd; // 欠阻尼首峰时刻（解析）。
  return { peak: springAt(p, tPeak), atSec: tPeak };
}

// ---------- 打断可逆进度（大作级打断品质的数学面） ----------

export interface InterruptibleState {
  /** 线性时间位 0..1（推进的横轴——与缓动值严格分离，混用即发散）。 */
  pos: number;
  /** 缓动后的当前值（渲染读这个）。 */
  value: number;
  /** 当前速度（缓动值/s——打断续跑的一阶信息）。 */
  velocity: number;
  /** 方向（正向播放/反向回退）。 */
  direction: 1 | -1;
}

/**
 * 可逆动画推进：时间位沿方向线性推进、值 = 曲线(pos)——位置与值分离
 * （把缓动值当时间位推进是发散的：两端曲线平坦，永远到不了端点）。
 * exit 是 enter 的时间反演——打断接续无缝。
 */
export function advance(cur: InterruptibleState, dtMs: number, curve: Bezier, durationMs: number): InterruptibleState {
  const rawPos = cur.pos + cur.direction * (dtMs / durationMs);
  const pos = Math.max(0, Math.min(1, rawPos));
  const hitEnd = rawPos >= 1 || rawPos <= 0;
  const value = bezierEase(curve, pos);
  const eps = 1e-4;
  const slope = (bezierEase(curve, Math.min(1, pos + eps)) - value) / eps;
  return {
    pos,
    value,
    velocity: cur.direction * slope * (1000 / durationMs) * (hitEnd ? 0 : 1),
    direction: hitEnd && rawPos >= 1 ? 1 : hitEnd && rawPos <= 0 ? -1 : cur.direction,
  };
}

/** 反向（打断回退）：方向翻转，位置与速度保持（连续性——不跳变）。 */
export function reverse(cur: InterruptibleState): InterruptibleState {
  return { ...cur, direction: cur.direction === 1 ? -1 : 1 };
}

// ---------- 强度档缩放（F160 三档联动） ----------

export type MotionTier = "full" | "reduced" | "off";

/** 档位缩放：full=1、reduced=0.6、off=0（off=瞬显，动画时长归零）。 */
export function tierScale(tier: MotionTier): number {
  return tier === "full" ? 1 : tier === "reduced" ? 0.6 : 0;
}

/** 时长按档缩放（毫秒取整——F160 契约"时长实测 60%"按值成立，不做帧栅格近似）。 */
export function scaledDurationMs(durationMs: number, tier: MotionTier): number {
  return Math.round(durationMs * tierScale(tier));
}

/** 逐帧采样表（LUT——运行期查表不求解，帧内零数学开销）。 */
export function sampleLut(curve: Bezier, samples = 64): Float32Array {
  const lut = new Float32Array(samples + 1);
  for (let i = 0; i <= samples; i++) {
    lut[i] = bezierEase(curve, i / samples);
  }
  return lut;
}
