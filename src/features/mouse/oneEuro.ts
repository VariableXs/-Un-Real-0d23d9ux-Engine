/**
 * J 鼠标域 · F603/F611 One Euro 自适应滤波引擎（v5 · 深化批次五）。
 *
 * v4 最丑角落（checklist F611 自记）：「IIR 一阶低通对 6Hz 以上强档的相位
 * 滞后约 80ms——2-4px 的中等意图移动处于灰区」。本模块引入业界正宗的
 * One Euro Filter（Casiez et al. CHI 2012）：截止频率随速度自适应——
 * 慢（震颤区）低截止吃抖动，快（意图区）高截止直通，灰区消失在数学上。
 *
 * 与 IIR 引擎并存不替换（默认仍 iir——换引擎是手感变更，必须用户显性选）；
 * compareEngines() 出两引擎在 2/4/6Hz 三频段的残余比对拍谱，面板并排渲染，
 * 用户看着数据选引擎（章九：错误与引导——选择基于证据不基于口号）。
 *
 * One Euro 实现要点（信号口径：管线喂相对位移，引擎内部积分成位置信号、
 * 滤波后差分输出——标准 One Euro 语义，逐事件无缓冲零延迟）：
 * - 速度估计：对位置信号再套一个 1€ 低通（dCutoff=1Hz 固定）取导数；
 * - 自适应截止：fc = minCutoff + beta·|速度|；
 * - alpha = 1/(1 + tau·2π·fc)，tau 为帧间隔（秒，逐事件实测）。
 */

/** 单通道 1€ 低通（One Euro 的内积木）。 */
class OneEuroChannel {
  private hatX: number | null = null;
  private hatDf = 0;

  constructor(
    private readonly minCutoff: number,
    private readonly beta: number,
    private readonly dCutoff: number,
  ) {}

  private static alpha(cutoff: number, dtSec: number): number {
    const tau = 1 / (2 * Math.PI * cutoff);
    return 1 / (1 + tau / dtSec);
  }

  reset(): void {
    this.hatX = null;
    this.hatDf = 0;
  }

  feed(x: number, dtSec: number): number {
    if (this.hatX === null) {
      this.hatX = x;
      return x;
    }
    const dx = (x - this.hatX) / Math.max(dtSec, 1e-4);
    const aD = OneEuroChannel.alpha(this.dCutoff, dtSec);
    this.hatDf += aD * (dx - this.hatDf);
    const cutoff = this.minCutoff + this.beta * Math.abs(this.hatDf);
    const a = OneEuroChannel.alpha(cutoff, dtSec);
    this.hatX += a * (x - this.hatX);
    return this.hatX;
  }
}

export interface OneEuroParams {
  /** 慢速截止（Hz）——越低震颤吃得越干净。 */
  minCutoff: number;
  /** 速度系数——越大快速移动越直通。 */
  beta: number;
  /** 导数通道截止（Hz，固定默认 1）。 */
  dCutoff: number;
}

/** 档位 → 引擎参数（tuneOneEuro 的映射表，一处一事实）。 */
export const ONE_EURO_LEVELS: Record<"light" | "strong", OneEuroParams> = {
  light: { minCutoff: 1.2, beta: 0.04, dCutoff: 1 },
  strong: { minCutoff: 0.5, beta: 0.02, dCutoff: 1 },
};

/**
 * One Euro 手抖滤波引擎（F611 第二引擎）：
 * 接口与 TremorFilter 对齐（feed(dx,dy) → {x,y,filtered}），意图直通门
 * （≥4×幅度阈值的位移零衰减）两引擎同规——「只压高频不压意图」是引擎
 * 无关的铁律，直通由外门裁决、滤波只管门内的平滑。
 */
export class TremorFilterEuro implements TremorEngine {
  level: "off" | "light" | "strong";
  private cx: OneEuroChannel | null = null;
  private cy: OneEuroChannel | null = null;
  private lastMs: number | null = null;
  /** 积分位置（相对位移 → 绝对信号的标准做法）。 */
  private ix = 0;
  private iy = 0;
  private ox = 0;
  private oy = 0;

  constructor(level: "off" | "light" | "strong" = "off") {
    this.level = level;
  }

  reset(): void {
    this.cx = null;
    this.cy = null;
    this.lastMs = null;
    this.ix = 0;
    this.iy = 0;
    this.ox = 0;
    this.oy = 0;
  }

  private channels(): { cx: OneEuroChannel; cy: OneEuroChannel } {
    const p = ONE_EURO_LEVELS[this.level === "strong" ? "strong" : "light"];
    this.cx ??= new OneEuroChannel(p.minCutoff, p.beta, p.dCutoff);
    this.cy ??= new OneEuroChannel(p.minCutoff, p.beta, p.dCutoff);
    return { cx: this.cx!, cy: this.cy! };
  }

  /**
   * @param atMs 事件时刻（毫秒，缺省 performance.now）——One Euro 需要真实 dt 做帧率无关滤波。
   */
  feed(dx: number, dy: number, atMs: number = performance.now()): { x: number; y: number; filtered: boolean } {
    if (this.level === "off") return { x: dx, y: dy, filtered: false };
    const amp = this.level === "strong" ? 2 : 0.5;
    // 意图直通门（与 IIR 同规）：4×幅度阈值以上零衰减。
    if (Math.hypot(dx, dy) >= amp * 4) {
      this.ix += dx;
      this.iy += dy;
      this.ox += dx;
      this.oy += dy;
      this.lastMs = atMs;
      return { x: dx, y: dy, filtered: false };
    }
    const dtSec = this.lastMs === null ? 1 / 125 : Math.max(1e-4, (atMs - this.lastMs) / 1000);
    this.lastMs = atMs;
    this.ix += dx;
    this.iy += dy;
    const { cx, cy } = this.channels();
    const nx = cx.feed(this.ix, dtSec);
    const ny = cy.feed(this.iy, dtSec);
    const outX = nx - this.ox;
    const outY = ny - this.oy;
    this.ox = nx;
    this.oy = ny;
    return { x: outX, y: outY, filtered: Math.hypot(outX, outY) > 0 };
  }
}

/**
 * 档位自动推荐 → One Euro 参数（自动调谐的引擎侧延伸）：
 * 与 filters.autoTuneTremor 同一判据（抖动能量占比），推荐档位后再给参数——
 * 用户改 beta/minCutoff 是专家捷径（章十一），默认参数不进面板。
 */
export function tuneOneEuro(level: "light" | "strong"): OneEuroParams {
  return { ...ONE_EURO_LEVELS[level] };
}

/**
 * 震颤样本注入（对拍谱的样本器）：freq Hz 正弦抖动，每半周期一采样，
 * 与 filters.tremorSpectrum 同一注入口径——两引擎吃同一份样本才叫对拍。
 */
function inject(freq: 2 | 4 | 6, ampPx: number, seconds: number): { dx: number; t: number }[] {
  const period = 1000 / freq;
  const steps = Math.round((seconds * 1000) / period) * 2;
  const out: { dx: number; t: number }[] = [];
  for (let i = 0; i < steps; i++) {
    const t = (i / 2) * period;
    out.push({ dx: ampPx * Math.sin((2 * Math.PI * t) / period), t });
  }
  return out;
}

/** 单引擎残余比（同 tremorSpectrum 口径：Σ|out|/Σ|in|，四舍五入 3 位）。 */
export function euroResidual(
  level: "light" | "strong",
  freq: 2 | 4 | 6,
  ampPx: number,
  seconds = 1,
): number {
  const f = new TremorFilterEuro(level);
  let input = 0;
  let output = 0;
  for (const s of inject(freq, ampPx, seconds)) {
    input += Math.abs(s.dx);
    output += Math.abs(f.feed(s.dx, 0, s.t).x);
  }
  return input === 0 ? 0 : Math.round((output / input) * 1000) / 1000;
}

/**
 * 双引擎对拍谱（F611 判据「三频段过滤效果谱」的引擎对照版）：
 * IIR 与 One Euro 在 2/4/6Hz 三频段的残余比并排——判据期待的「更好」不在
 * 嘴上在谱上：One Euro 慢频段应≤IIR（吃得更干净），快频段残余相近
 * （意图同样直通）。面板与单测共用此函数。
 */
export function compareEngines(ampPx = 1): {
  rows: { freq: 2 | 4 | 6; iir: { residualRatio: number }; euro: number }[];
  euroBetterAt: (2 | 4 | 6)[];
} {
  const freqs: (2 | 4 | 6)[] = [2, 4, 6];
  const rows = freqs.map((freq) => ({
    freq,
    iir: iirSpectrum("strong", freq, ampPx),
    euro: euroResidual("strong", freq, ampPx),
  }));
  return { rows, euroBetterAt: rows.filter((r) => r.euro <= r.iir.residualRatio).map((r) => r.freq) };
}

// filters.ts 与本文件单向依赖（filters 不 import oneEuro）——无环。
import { tremorSpectrum as iirSpectrum, type TremorEngine } from "./filters";
