/**
 * AURORA-10000 领域04 · 族0096 任务视图（AI-20 批次，勿删）。
 * 窗口墙/时间线/过滤/批量操作/隐私开关与保留期。
 */

export interface TimelineEvent {
  id: string;
  appId: string;
  title: string;
  ts: number;
  desktop: number;
}

export interface TaskViewPrefs {
  cols: number;
  /** 时间线开关（F02397 隐私）。 */
  timeline: boolean;
  /** 保留天数（F02398）。 */
  retentionDays: number;
  bgOpacity: number;
}

/** 记录活动（timeline 关闭时丢弃）。 */
export function recordEvent(events: readonly TimelineEvent[], ev: Omit<TimelineEvent, "id">, prefs: TaskViewPrefs, now = Date.now()): TimelineEvent[] {
  if (!prefs.timeline) return [...events];
  const cutoff = now - prefs.retentionDays * 86_400_000;
  return [...events.filter((e) => e.ts >= cutoff), { ...ev, id: `tv-${now}-${Math.random().toString(36).slice(2, 8) }` }];
}

/** 按应用过滤（F02380）与搜索（F02381）。 */
export function filterWall(
  events: readonly TimelineEvent[],
  opts: { appId?: string; search?: string; currentDesktop?: number },
): TimelineEvent[] {
  let list = [...events];
  if (opts.appId) list = list.filter((e) => e.appId === opts.appId);
  if (opts.search) {
    const s = opts.search.toLowerCase();
    list = list.filter((e) => e.title.toLowerCase().includes(s) || e.appId.includes(s));
  }
  return list.sort((a, b) => b.ts - a.ts);
}

/** 多选批量操作（F02386）：close / pin / move-desktop。 */
export type BatchAction = { type: "close"; ids: string[] } | { type: "pin"; ids: string[] } | { type: "move"; ids: string[]; desktop: number };

export function applyBatch<T extends { id: string }>(items: readonly T[], action: BatchAction): { keep: T[]; affected: string[] } {
  const ids = new Set(action.ids);
  return { keep: items.filter((i) => !ids.has(i.id)), affected: [...ids] };
}

/** 时间线导出（F02396）。 */
export function exportTimeline(events: readonly TimelineEvent[]): string {
  return events.map((e) => `${new Date(e.ts).toISOString()}\t${e.appId}\t${e.title}`).join("\n");
}
