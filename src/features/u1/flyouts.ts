/**
 * 指示器与浮层五件（AI-U1 · F421 输入法指示器 / F422 音量浮层 /
 * F423 电池浮层 / F448 麦克风电平 / F449 摄像头预览）——前端生效面。
 *
 * 判据唯一源（主册摘文，与 kernel/varix/src/uni1/ 同参数）：
 * - F421「循环顺序=F373 设置序；右键直选；徽标形态（含双拼角标）；三处
 *   同步；点击响应 <100ms」。
 * - F422「拖动实时生效判据（<50ms 音频响应）；设备名与 F241 一致；下拉
 *   切换；混音器跳转；浮层几何（图标上方居中）」。
 * - F423「续航估算准确性（±15% 对实际）；充电预计；开关即时；数据同源
 *   审计（三处 F366/F423/F291 同数）；浮层几何」。
 * - F448「电平实时性（<100ms）；回放保真；建议触发阈值（电平分档）；
 *   设备切换；与 F322 隐私指示联动（测试中指示亮）」。
 * - F449「预览延迟 <200ms；参数两路（硬件/标注）行为；镜像开关；指示
 *   常亮判据；预览关闭即释放摄像头（指示灭）」。
 */

import { u1Store } from "./u1store";

/* ------------------------------- F421 输入法指示器 ------------------------------- */

export const IME_CLICK_BUDGET_MS = 100;

export type ImeBadge = "zh" | "en" | "zh-shuangpin";

/** 徽标形态（判据：含双拼角标）。 */
export function imeBadge(layout: string, shuangpin: string | null): ImeBadge {
  if (layout.startsWith("en")) return "en";
  return shuangpin === layout ? "zh-shuangpin" : "zh";
}

/** 点击循环（顺序 = F373 设置序注入——环回）。 */
export function imeCycle(order: string[], current: number): number {
  if (order.length === 0) return current;
  return (current + 1) % order.length;
}

/** 三处同步对拍（任务栏徽标/候选窗/设置页三方同值）。 */
export function imeThreeWaySync(current: string, candidateView: string, settingsView: string): boolean {
  return current === candidateView && current === settingsView;
}

/* ------------------------------- F422 音量浮层 ------------------------------- */

export const VOLUME_APPLY_BUDGET_MS = 50;

/** 浮层几何：任务栏图标上方居中（判据原文），底缘安全钳制。 */
export function volumeFlyoutGeometry(icon: { x: number; w: number }, fly: { w: number; h: number }, screen: { w: number; h: number }, gap = 8): { x: number; y: number } {
  return {
    x: Math.max(8, Math.min(icon.x + icon.w / 2 - fly.w / 2, screen.w - fly.w - 8)),
    y: screen.h - gap - fly.h,
  };
}

/** 音量钳制 0-100（步进 2——F436 滑杆表同源）。 */
export function clampVolume(v: number): number {
  return Math.min(100, Math.max(0, Math.round(v / 2) * 2));
}

/* ------------------------------- F423 电池浮层 ------------------------------- */

/** 续航估算：线性模型 + ±15% 诚实标注（判据：±15% 对实际）。 */
export function batteryRemainMinutes(batteryPct: number, drainPctPerHour: number): { minutes: number; tolerancePct: number } {
  const safe = Math.max(drainPctPerHour, 1);
  return { minutes: Math.round((batteryPct / safe) * 60), tolerancePct: 15 };
}

/** 充电预计（判据：充电预计）。 */
export function batteryChargeMinutes(batteryPct: number, chargePctPerHour: number): number {
  const safe = Math.max(chargePctPerHour, 1);
  return Math.round(((100 - batteryPct) / safe) * 60);
}

/* ------------------------------- F448 麦克风电平 ------------------------------- */

export const METER_LATENCY_BUDGET_MS = 100;

/** 电平分档 → 人话建议（与内核 level_advice 同表）。 */
export function micAdvice(levelPermille: number, inputVolume: number): string {
  if (levelPermille < 200) return "电平过低——离麦克风近一点，或调高输入音量";
  if (levelPermille > 950) return inputVolume > 50 ? "电平过高——把输入音量降到 60 以内再试" : "电平过高——离麦克风远一点";
  return "电平正常——就用这个设置";
}

/** 采样 → 电平（千分比，绝对值映射——与内核 meter_tick 同式）。 */
export function micLevel(sample: number): number {
  return Math.min(1000, Math.round((Math.abs(sample) / 32767) * 1000));
}

/* ------------------------------- F449 摄像头预览 ------------------------------- */

export const CAM_PREVIEW_BUDGET_MS = 200;

export type CamParam = "brightness" | "contrast" | "saturation";

/** 参数两路：硬件支持 → 生效；不支持 → 诚实标注（不假装）。 */
export function camParamApply(supported: Record<CamParam, boolean>, p: CamParam, v: number): { ok: true; value: number } | { ok: false; note: string } {
  if (!supported[p]) return { ok: false, note: "该摄像头不支持此调节——参数仅作展示" };
  return { ok: true, value: Math.min(100, Math.max(0, Math.round(v))) };
}

/* ------------------------------- 面板读数 ------------------------------- */

/** 指示器开关读数（消费 u1Store）。 */
export function indicatorsEnabled(): { ime: boolean; volume: boolean; battery: boolean } {
  const cfg = u1Store.get<{ imeBadge?: boolean; volumeFly?: boolean; batteryFly?: boolean }>("indicators");
  return { ime: cfg.imeBadge ?? true, volume: cfg.volumeFly ?? true, battery: cfg.batteryFly ?? true };
}
