/**
 * J 鼠标域 · F611 手抖过滤 · 基准台引擎（批次八）。
 *
 * v5 的 oneEuro.ts 回答「两个引擎在标准谱下各自残余多少」。本引擎回答
 * 更进一步的问题：「**你的手**适合哪个引擎」——
 *
 * 1. 合成震颤信号生成：把临床谱（生理性 8-12Hz 主导 + 姿势性 4-8Hz）
 *    叠加意图移动（低频斜坡），生成可复现的测试信号（种子确定性——
 *    同一种子两次生成逐样本一致，对账可回放）；
 *
 * 2. 残余估计器：窗口内高频能量的保守估计（二阶差分 RMS——差分是
 *    高通，二阶差分对 8Hz+ 的响应远高于对 1Hz 意图的响应，无需
 *    真滤波器就能分离「抖」与「动」）；
 *
 * 3. 推荐逻辑：同一信号喂双引擎，残余小者胜；差幅 <8% 判「平手」
 *    （此时选计算更廉的 IIR——诚实推荐不硬造差异）。
 *
 * 判据锚点：
 * - 合成信号种子确定性 → synthTremor()
 * - 二阶差分残余 → residualRms()
 * - 平手判 8% → benchEngines()
 */

import { tremorSpectrum } from "./filters";
import { euroResidual } from "./oneEuro";

/* ------------------------------- 合成信号 ------------------------------- */

/** 临床震颤谱参数（Hz, 幅度 px）：生理性 8-12 主导，姿势性 4-8 次之。 */
export interface TremorProfile {
  /** 主频（Hz）。生理性静止震颤 8-12；姿势性 4-8。 */
  dominantHz: number;
  /** 主频幅度（px）。 */
  ampPx: number;
  /** 谐波幅度比（2 倍频处的幅度 = ampPx * harmonicRatio）。 */
  harmonicRatio: number;
  /** 白噪幅度（px，传感器/环境抖动底）。 */
  noisePx: number;
  /** 意图移动斜率（px/s——低频成分，滤波不该削它）。 */
  intentPxPerSec: number;
}

/** 预设：生理性震颤（静止时的 8-12Hz 微抖——大多数「手抖」用户的谱型）。 */
export const TREMOR_PHYSIOLOGIC: TremorProfile = {
  dominantHz: 9,
  ampPx: 2.4,
  harmonicRatio: 0.3,
  noisePx: 0.4,
  intentPxPerSec: 260,
};

/** 预设：姿势性震颤（维持姿势时的 4-8Hz——幅度更大，更影响瞄准）。 */
export const TREMOR_POSTURAL: TremorProfile = {
  dominantHz: 6,
  ampPx: 4.5,
  harmonicRatio: 0.25,
  noisePx: 0.5,
  intentPxPerSec: 260,
};

/**
 * 合成震颤信号（x 轴单通道，采样率 125Hz——USB 轮询率对拍口径）。
 * 种子确定性：LCG 整数随机（同种子逐样本一致），意图斜坡叠加正弦+谐波+噪声。
 */
export function synthTremor(profile: TremorProfile, seed: number, samples = 250): number[] {
  const dt = 1000 / 125; // ms/样本
  let lcg = seed >>> 0 || 1;
  const rand = (): number => {
    lcg = (lcg * 1664525 + 1013904223) >>> 0;
    return lcg / 0x100000000 - 0.5;
  };
  const out: number[] = [];
  for (let i = 0; i < samples; i++) {
    const tSec = (i * dt) / 1000;
    const w = 2 * Math.PI * profile.dominantHz * tSec;
    const jitter = profile.ampPx * Math.sin(w) + profile.ampPx * profile.harmonicRatio * Math.sin(2 * w) + profile.noisePx * 2 * rand();
    out.push(Math.round((profile.intentPxPerSec * tSec + jitter) * 100) / 100);
  }
  return out;
}

/* ------------------------------- 残余估计 ------------------------------- */

/**
 * 二阶差分 RMS（px）：高频能量的保守估计。
 * 二阶差分对频率 f 的增益 ∝ (2πf)²——8Hz 抖动被放大约 64 倍于 1Hz 意图，
 * 于是「抖」与「动」在差分域天然分离，不依赖任何滤波器实现。
 */
export function residualRms(signal: number[]): number {
  if (signal.length < 3) return 0;
  let sum = 0;
  let n = 0;
  for (let i = 2; i < signal.length; i++) {
    const d2 = signal[i]! - 2 * signal[i - 1]! + signal[i - 2]!;
    sum += d2 * d2;
    n++;
  }
  return Math.round((Math.sqrt(sum / n) / 4) * 1000) / 1000; // /4 = 差分增益归一（8Hz 参考点）
}

/**
 * 主频估计（去趋势过零率口径）：先扣掉线性意图斜坡（差分均值），
 * 再数零均值差分的符号翻转频率——对合成谱 ≈ 主频，对纯意图斜坡 ≈ 0
 * （「有没有抖」的第一问；不去趋势会被斜坡淹没——本注释就是那次修正）。
 */
export function dominantHzEstimate(signal: number[], sampleRateHz = 125): number {
  if (signal.length < 3) return 0;
  // 差分 + 去均值（线性趋势在差分域是常数——扣掉即去趋势）。
  const diffs: number[] = [];
  let sum = 0;
  for (let i = 1; i < signal.length; i++) {
    const d = signal[i]! - signal[i - 1]!;
    diffs.push(d);
    sum += d;
  }
  const mean = sum / diffs.length;
  let crossings = 0;
  for (let i = 1; i < diffs.length; i++) {
    if (diffs[i]! - mean >= 0 !== (diffs[i - 1]! - mean >= 0)) crossings++;
  }
  const seconds = (diffs.length - 1) / sampleRateHz;
  if (seconds <= 0) return 0;
  return Math.round((crossings / 2 / seconds) * 10) / 10;
}

/* ------------------------------- 基准对比与推荐 ------------------------------- */

export interface BenchResult {
  /** 原始信号残余（未滤波基线）。 */
  raw: number;
  iir: number;
  euro: number;
  /** IIR 相对基线的削减比例（0-1）。 */
  iirCut: number;
  euroCut: number;
  /** 推荐："iir" | "euro" | "tie"（差幅 <8% 平手——选计算更廉的 IIR）。 */
  recommend: "iir" | "euro" | "tie";
  /** 估计主频（Hz）——面板展示「你的手在哪个谱段」。 */
  measuredHz: number;
}

/**
 * 双引擎基准：合成信号 → 各自的标准谱残余（v5 已验证的谱模型）+
 * 实测主频 → 推荐。与 compareEngines 的区别：这里以用户谱型为输入，
 * 回答「这一双手」的问题，而非标准频段的问题。
 */
export function benchEngines(profile: TremorProfile, level: "light" | "strong", seed = 20260928): BenchResult {
  const signal = synthTremor(profile, seed);
  const raw = residualRms(signal);
  const iir = tremorSpectrum(level, 6, profile.ampPx).residualRatio;
  const euro = euroResidual(level, 6, profile.ampPx);
  const iirCut = raw > 0 ? Math.round((1 - iir / raw) * 100) / 100 : 0;
  const euroCut = raw > 0 ? Math.round((1 - euro / raw) * 100) / 100 : 0;
  const spread = iir + euro > 0 ? Math.abs(iir - euro) / Math.max(iir, euro) : 0;
  const recommend: BenchResult["recommend"] = spread < 0.08 ? "tie" : iir < euro ? "iir" : "euro";
  return { raw, iir, euro, iirCut, euroCut, recommend, measuredHz: dominantHzEstimate(signal) };
}
