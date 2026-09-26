/**
 * F164 锁屏定制 · 完整设计。
 *
 * 主册判据：三式切换即时预览；唤醒到可见 ≤2s 实测；零循环动画验证（功耗挂账 F060）。
 *
 * 【功能定义】锁屏界面定制：壁纸（独立于桌面可选）/时间样式（三式：大数字/模拟/
 * 极简）/状态信息行（电量/日期可选开关）；锁屏动画复用 C-2 四幕语汇静版（淡入即
 * 静，无循环动画——省电纪律）。
 *
 * 【状态与异常】锁屏期间通知 → 不显示内容只计数（隐私默认）；壁纸加载失败 → 深色
 * 纯色兜底；唤醒延迟预算 2s 内可见时间。
 *
 * 【设计细节】大数字式 96px；模拟式钟面 4K 资产（F100 同素材）；状态行图标走 E1
 * 令牌色；密码输入框聚焦时时间缩小上移（让位布局预演）。
 */

import { personaStore } from "./store";

export const SECTION = "lock";
export const WAKE_BUDGET_MS = 2000;
export const BIG_DISPLAY_PX = 96;

export type LockTimeStyle = "big-number" | "analog" | "minimal";

export interface LockScreenConfig {
  /** 壁纸来源：独立 / 跟随桌面 / 纯色兜底。 */
  wallpaperMode: "independent" | "follow-desktop";
  wallpaperPath: string | null;
  timeStyle: LockTimeStyle;
  showBattery: boolean;
  showDate: boolean;
  /** 锁屏期间通知只计数不显内容（隐私默认，可关——关闭时也仅显示应用名）。 */
  notifyPrivacy: boolean;
}

export function defaultLockScreenConfig(): LockScreenConfig {
  return {
    wallpaperMode: "follow-desktop",
    wallpaperPath: null,
    timeStyle: "big-number",
    showBattery: true,
    showDate: true,
    notifyPrivacy: true,
  };
}

export function loadLockScreenConfig(): LockScreenConfig {
  const stored = personaStore.getWith(SECTION, "lock", undefined) as Partial<LockScreenConfig> | undefined;
  return { ...defaultLockScreenConfig(), ...(stored ?? {}) };
}

export function saveLockScreenConfig(c: LockScreenConfig): void {
  personaStore.set(SECTION, { lock: c });
}

export const LOCK_TIME_STYLES: readonly { id: LockTimeStyle; zh: string; en: string }[] = [
  { id: "big-number", zh: "大数字", en: "Big number" },
  { id: "analog", zh: "模拟钟面", en: "Analog" },
  { id: "minimal", zh: "极简", en: "Minimal" },
] as const;

/** 零循环动画校验：锁屏层在无交互 5 秒内不得有任何重绘请求（功耗红线）。 */
export function isZeroLoopAnimation(redrawTimestamps: number[], windowMs = 5000): boolean {
  if (redrawTimestamps.length < 2) return true;
  const sorted = [...redrawTimestamps].sort((a, b) => a - b);
  const last = sorted.at(-1) ?? 0;
  const withinWindow = sorted.filter((t) => t > last - windowMs);
  return withinWindow.length <= 1;
}

/** 唤醒路径时间线分解（F053 启动时间线新节点——各段预算）。 */
export const WAKE_TIMELINE: readonly { stage: string; budgetMs: number }[] = [
  { stage: "ACPI 唤醒", budgetMs: 600 },
  { stage: "合成器恢复", budgetMs: 800 },
  { stage: "锁屏层首帧", budgetMs: 600 },
] as const;

export function wakeWithinBudget(stageElapsed: { stage: string; elapsedMs: number }[]): { ok: boolean; over: string[] } {
  const over: string[] = [];
  for (const s of stageElapsed) {
    const budget = WAKE_TIMELINE.find((w) => w.stage === s.stage)?.budgetMs;
    if (budget !== undefined && s.elapsedMs > budget) over.push(s.stage);
  }
  return { ok: over.length === 0, over };
}

/** 密码框聚焦时的时间让位布局（缩小上移——预演参数）。 */
export const FOCUS_RETREAT = { scale: 0.7, translateY: -48 };

/** 壁纸加载失败 → 深色纯色兜底色。 */
export const FALLBACK_COLOR = "#101018";
