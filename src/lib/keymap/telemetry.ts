/**
 * M-28 键位使用统计（本地）。
 *
 * 隐私口径（红线）：只记 action id + 次数，不记任何内容/时间明细；
 * 数据仅存本地 settings KV `keyStats`，永不出站。
 *
 * 注册失败（降级）单独计数成「占用榜」，复用 M-36 的空闲槽建议函数。
 */

export type KeyStats = Record<string, number>;

export interface OccupancyRecord {
  combo: string;
  /** 挤占来源（"system-reserved" | 注册失败的既有 binding id）。 */
  holder: string;
}

export function recordAction(stats: KeyStats, actionId: string): KeyStats {
  return { ...stats, [actionId]: (stats[actionId] ?? 0) + 1 };
}

/** 使用榜：按次数降序。 */
export function topActions(stats: KeyStats, limit = 10): { id: string; count: number }[] {
  return Object.entries(stats)
    .map(([id, count]) => ({ id, count }))
    .sort((a, b) => b.count - a.count || a.id.localeCompare(b.id))
    .slice(0, limit);
}

/** 占用榜：注册失败 combo 计数（调用方每收到一次降级失败 +1）。 */
export function recordOccupancy(occupancy: KeyStats, combo: string): KeyStats {
  return recordAction(occupancy, combo);
}

/** 每 5 分钟批量落盘阈值（由调用方定时 flush 到 settings KV）。 */
export const KEYSTATS_FLUSH_MS = 5 * 60 * 1000;
