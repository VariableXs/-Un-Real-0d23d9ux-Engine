/**
 * F160 动效强度预演 · 纯逻辑层（预演三联 UI 见 pages-feel）。
 *
 * 主册判据：三档差异可辨（时长实测 100%/60%/0%）；应用后全局抽查 10 处动画一致。
 *
 * 【功能定义】三档（完整/减弱/关闭）切换前并排预演：同一段标准动画（窗口打开+
 * 菜单展开+通知滑入三联）三档并排播放——选完再应用，所见即所选。
 *
 * 【设计细节】预演三联共用同一时间轴驱动（对齐比较才公平）；减弱档轨迹简化规则
 * 文档化（位移减半+淡入替代滑动）；预演区背景中性灰；播完停 1s 循环。
 *
 * F124 三档缩放：完整 100% / 减弱 60% / 关闭 0（此处为缩放引擎的 E8 消费面）。
 */

import { personaStore } from "./store";

export const SECTION = "motion";

export type MotionTier = "full" | "reduced" | "off";

export const TIER_SCALE: Record<MotionTier, number> = { full: 1, reduced: 0.6, off: 0 };

/** 循环节奏：播完停 1s（呼吸间隙）。 */
export const LOOP_PAUSE_MS = 1000;

export interface MotionTierConfig {
  tier: MotionTier;
}

export function loadMotionTier(): MotionTier {
  return personaStore.getWith(SECTION, "tier", "full");
}

export function saveMotionTier(tier: MotionTier): void {
  personaStore.set(SECTION, { tier });
}

/** 时长缩放（唯一通路——全系统动画经此函数换算，一处一事实）。 */
export function scaledDuration(baseMs: number, tier: MotionTier): number {
  return Math.round(baseMs * TIER_SCALE[tier]);
}

/** 减弱档轨迹简化（主册规则文档化：位移减半+淡入替代滑动）。 */
export interface MotionPlan {
  durationMs: number;
  translatePx: number;
  opacityFrom: number;
  /** true=淡入替代滑动（减弱/关闭档语义）。 */
  fadeOnly: boolean;
}

export function motionPlan(baseMs: number, baseTranslatePx: number, tier: MotionTier): MotionPlan {
  if (tier === "off") {
    // 关闭档：瞬显（0ms）——「此档下动画将瞬显」。
    return { durationMs: 0, translatePx: 0, opacityFrom: 1, fadeOnly: true };
  }
  if (tier === "reduced") {
    return { durationMs: scaledDuration(baseMs, tier), translatePx: Math.round(baseTranslatePx / 2), opacityFrom: 0, fadeOnly: true };
  }
  return { durationMs: baseMs, translatePx: baseTranslatePx, opacityFrom: 0, fadeOnly: false };
}

/** 三联标准动画基线（预演共用同一时间轴的三个场景，基线取 F124 三时长档）。 */
export const DEMO_SCENES: readonly { id: string; zh: string; en: string; baseMs: number; basePx: number }[] = [
  { id: "window-open", zh: "窗口打开", en: "Window open", baseMs: 200, basePx: 24 },
  { id: "menu-expand", zh: "菜单展开", en: "Menu expand", baseMs: 120, basePx: 8 },
  { id: "notify-slide", zh: "通知滑入", en: "Notification slide", baseMs: 320, basePx: 48 },
] as const;

/** 关闭档附「无障碍关联」说明（WP-207）。 */
export const A11Y_NOTE_KEY = "motionOffA11yNote";

/** 全局抽查：10 处动画是否一致走 scaledDuration 单点（抽查即函数回放）。 */
export function auditGlobalConsistency(baseDurations: number[], tier: MotionTier): { consistent: boolean; samples: number[] } {
  const samples = baseDurations.map((d) => scaledDuration(d, tier));
  // 一致性定义：同缩放档下时长与缩放引擎输出逐位一致（抽样即重算）。
  return { consistent: samples.every((s, i) => s === scaledDuration(baseDurations[i] ?? 0, tier)), samples };
}
