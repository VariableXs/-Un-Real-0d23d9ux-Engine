/**
 * F160 动效监控深化 · 帧时采样器 + 档位自动建议 + WP-207 进度数字替代引擎。
 *
 * 主册判据延伸：
 * - F124 设计细节「60fps 底线 80fps 目标下超预算动画自动降级直线（保帧率不保
 *   花活）」——帧时采样器检测超预算，输出降级建议。
 * - F160【状态与异常】「预演自身掉帧 → 提示性能受限（诚实）」。
 * - WP-207：关闭档下必达动画（进度环）转显性进度数字（可感知冗余）。
 */

import type { MotionTier } from "./motiontier";

// ---------- 帧时采样器（环形缓冲 + P95/P99） ----------

export interface FrameSample {
  /** 本帧合成耗时（ms）。 */
  ms: number;
  at: number;
}

export class FrameTimeSampler {
  private samples: FrameSample[] = [];

  constructor(private capacity = 300) {}

  record(ms: number, at: number = Date.now()): void {
    this.samples.push({ ms, at });
    if (this.samples.length > this.capacity) this.samples.splice(0, this.samples.length - this.capacity);
  }

  percentile(p: number): number {
    if (this.samples.length === 0) return 0;
    const sorted = this.samples.map((s) => s.ms).sort((a, b) => a - b);
    const idx = Math.min(sorted.length - 1, Math.max(0, Math.floor((sorted.length * p) / 100)));
    return sorted[idx] ?? 0;
  }

  /** 预算判定：80fps 目标 = 帧预算 12.5ms；60fps 底线 = 16.6ms。 */
  verdict(): { within80fps: boolean; within60fps: boolean; p95: number; p99: number; samples: number } {
    return {
      within80fps: this.percentile(95) <= 12.5,
      within60fps: this.percentile(99) <= 16.6,
      p95: this.percentile(95),
      p99: this.percentile(99),
      samples: this.samples.length,
    };
  }

  clear(): void {
    this.samples = [];
  }
}

// ---------- 档位自动建议（掉帧 → 建议降档；恢复 → 建议升档，迟滞防抖） ----------

export interface TierSuggestion {
  suggest: MotionTier | null;
  reason: string;
  /** 迟滞冷却：建议变更后 N 秒内不再改口（防档位振荡）。 */
  cooldownRemainMs: number;
}

export const SUGGEST_COOLDOWN_MS = 5000;
const DROP_P95_MS = 20;      // P95 >20ms（<50fps）→ 建议降档
const RECOVER_P95_MS = 13.5; // P95 <13.5ms（>74fps）→ 建议升档

export class TierAdvisor {
  private lastChangeAt = 0;
  private lastSuggested: MotionTier | null = null;

  constructor(private sampler: FrameTimeSampler) {}

  advise(currentTier: MotionTier, now: number): TierSuggestion {
    const cooldownRemainMs = Math.max(0, SUGGEST_COOLDOWN_MS - (now - this.lastChangeAt));
    const v = this.sampler.verdict();
    if (cooldownRemainMs > 0) {
      return { suggest: null, reason: "冷却中（防档位振荡）", cooldownRemainMs };
    }
    const order: MotionTier[] = ["off", "reduced", "full"];
    const idx = order.indexOf(currentTier);
    if (!v.within60fps && v.p95 > DROP_P95_MS && idx > 0) {
      const next = order[idx - 1]!;
      this.lastChangeAt = now;
      this.lastSuggested = next;
      return { suggest: next, reason: `P95 帧时 ${v.p95.toFixed(1)}ms 超 20ms 线——建议降档保帧率（保帧率不保花活）`, cooldownRemainMs: SUGGEST_COOLDOWN_MS };
    }
    if (v.within80fps && v.p95 < RECOVER_P95_MS && idx < order.length - 1) {
      const next = order[idx + 1]!;
      this.lastChangeAt = now;
      this.lastSuggested = next;
      return { suggest: next, reason: `P95 帧时 ${v.p95.toFixed(1)}ms 恢复预算——建议升档`, cooldownRemainMs: SUGGEST_COOLDOWN_MS };
    }
    return { suggest: null, reason: "帧时在预算内", cooldownRemainMs };
  }

  get lastSuggestion(): MotionTier | null {
    return this.lastSuggested;
  }
}

// ---------- 超预算动画自动降级（直线替代） ----------

export interface AnimationBudgetInput {
  /** 动画注册时长（缩放后）。 */
  durationMs: number;
  /** 是否「必达动画」（进度环类——WP-207 转数字而非降直线）。 */
  missionCritical: boolean;
}

export type AnimationFallback = "keep" | "linear" | "progress-number";

/** 单动画超预算判定：帧预算内保曲线；超预算非必达 → 降直线；必达 → 转进度数字。 */
export function animationFallback(input: AnimationBudgetInput, sampler: FrameTimeSampler): AnimationFallback {
  const v = sampler.verdict();
  if (v.within80fps) return "keep";
  if (input.missionCritical) return "progress-number";
  return "linear";
}

// ---------- WP-207 进度数字替代（进度环 → 显性数字） ----------

export interface ProgressNumberSpec {
  /** 0..1 进度。 */
  progress: number;
  /** 呈现格式（百分比/剩余时间）。 */
  format: "percent" | "seconds-remaining";
  totalSeconds?: number;
}

export function progressNumberSpec(progress: number, totalSeconds?: number): ProgressNumberSpec {
  const p = Math.min(1, Math.max(0, progress));
  if (totalSeconds !== undefined) {
    return { progress: p, format: "seconds-remaining", totalSeconds };
  }
  return { progress: p, format: "percent" };
}

/** 渲染文案（三要素——用户看到的人话）。 */
export function progressNumberText(spec: ProgressNumberSpec): string {
  if (spec.format === "percent") return `${Math.round(spec.progress * 100)}%`;
  const remain = Math.max(0, Math.round(((spec.totalSeconds ?? 0) * (1 - spec.progress))));
  return `剩余约 ${remain} 秒`;
}

// ---------- 预演诚实提示（预演自身掉帧 → 提示性能受限） ----------

export function previewHonestyNote(sampler: FrameTimeSampler): string | null {
  const v = sampler.verdict();
  if (v.samples === 0) return null;
  return v.within60fps ? null : `预演自身帧时 P99 ${v.p99.toFixed(1)}ms 超预算——当前性能受限，预演节奏可能与实际有偏差`;
}
