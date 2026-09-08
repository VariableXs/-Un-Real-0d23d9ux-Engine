/**
 * AI-18 M-65 昼夜壁纸组 — 纯逻辑：时段判定 + 下一边界。
 *
 * 口径：
 * - 边界默认 6/12/18/21（小时；schema 已保证严格递增 0..23）；
 * - slot: [b0,b1) 早 / [b1,b2) 日 / [b2,b3) 暮 / 其余 夜（夜跨午夜环绕）；
 * - 切换交叉淡入由 WallpaperLayer 用 CSS 过渡实现（300ms，复用 Z-68 技术）。
 */

import type { DaySlot } from "./schema";

export const DAY_SLOTS: readonly DaySlot[] = ["morning", "day", "dusk", "night"];

/** 当前小时所属时段。boundaries 必须严格递增（schema coerce 已保证）。 */
export function slotOfHour(hour: number, b: readonly [number, number, number, number]): DaySlot {
  const h = ((Math.floor(hour) % 24) + 24) % 24;
  if (h >= b[0] && h < b[1]) return "morning";
  if (h >= b[1] && h < b[2]) return "day";
  if (h >= b[2] && h < b[3]) return "dusk";
  return "night";
}

/** 距下一时段边界的分钟数（用于调度下次切换检查）。 */
export function minutesToNextBoundary(hour: number, minute: number, b: readonly [number, number, number, number]): number {
  const h = ((Math.floor(hour) % 24) + 24) % 24;
  const m = h * 60 + Math.max(0, Math.min(59, Math.floor(minute)));
  for (const bh of b) {
    const target = bh * 60;
    if (target > m) return target - m;
  }
  // 已过最后一个边界 → 明天第一个边界
  return 24 * 60 - m + b[0] * 60;
}

/** 某时段目录/列表是否配置了可用壁纸（空串 = 未配置，跳过该时段沿用当前）。 */
export function slotConfigured(dirs: Record<DaySlot, string>, slot: DaySlot): boolean {
  return typeof dirs[slot] === "string" && dirs[slot].trim() !== "";
}
