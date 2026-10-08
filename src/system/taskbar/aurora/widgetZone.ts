/**
 * AURORA-10000 领域04 · 族0079 任务栏小组件区（AI-16 批次，勿删）。
 * 小组件注册/排序/轮播/免打扰/帧预算，纯模型。
 */
import { getD4 } from "./prefs";

export type WidgetId =
  | "weather" | "clock" | "calendar" | "system" | "media" | "quick"
  | "notes" | "todo" | "stocks" | "countdown";

export interface WidgetState {
  id: WidgetId;
  enabled: boolean;
  order: number;
  /** 展开态：弹出详情（月视图/曲线/封面）。 */
  expanded: boolean;
}

/** 默认组件集与顺序（F01951~F01958）。 */
export const DEFAULT_WIDGETS: readonly WidgetState[] = [
  { id: "weather", enabled: true, order: 0, expanded: false },
  { id: "clock", enabled: true, order: 1, expanded: false },
  { id: "calendar", enabled: true, order: 2, expanded: false },
  { id: "system", enabled: false, order: 3, expanded: false },
  { id: "media", enabled: true, order: 4, expanded: false },
  { id: "quick", enabled: false, order: 5, expanded: false },
  { id: "notes", enabled: false, order: 6, expanded: false },
  { id: "todo", enabled: false, order: 7, expanded: false },
  { id: "stocks", enabled: false, order: 8, expanded: false },
  { id: "countdown", enabled: false, order: 9, expanded: false },
];

/** 拖拽重排（F01968）：把 from 移到 to 位置。 */
export function reorderWidgets(widgets: readonly WidgetState[], from: number, to: number): WidgetState[] {
  const arr = [...widgets];
  const moved = arr.splice(from, 1)[0];
  if (!moved) return [...widgets];
  arr.splice(Math.max(0, Math.min(arr.length, to)), 0, moved);
  return arr.map((w, i) => ({ ...w, order: i }));
}

/** 免打扰时段（F01972）："22:00-07:00" 格式；不匹配返回 false。 */
export function inDndWindow(hhmm: string, now: Date): boolean {
  const m = /^(\d{2}):(\d{2})-(\d{2}):(\d{2})$/.exec(hhmm.trim());
  if (!m || m[1] == null || m[2] == null || m[3] == null || m[4] == null) return false;
  const mins = now.getHours() * 60 + now.getMinutes();
  const a = parseInt(m[1], 10) * 60 + parseInt(m[2], 10);
  const b = parseInt(m[3], 10) * 60 + parseInt(m[4], 10);
  return a <= b ? mins >= a && mins < b : mins >= a || mins < b;
}

/** 当前应渲染的组件（enabled + 免打扰过滤 + 排序）。 */
export function visibleWidgets(widgets: readonly WidgetState[], now = new Date()): WidgetState[] {
  const dnd = getD4<string>("F01972") ?? "";
  return widgets
    .filter((w) => w.enabled && !(dnd && inDndWindow(dnd, now)))
    .sort((a, b) => a.order - b.order);
}

/** 轮播（F01971）：间隔秒，0=关闭；返回当前帧组件子集。 */
export function carouselSlice(widgets: readonly WidgetState[], pageSize: number, tick: number): WidgetState[] {
  const interval = getD4<number>("F01971") ?? 0;
  const vis = visibleWidgets(widgets);
  if (interval <= 0 || vis.length <= pageSize) return vis;
  const start = (tick * pageSize) % vis.length;
  return Array.from({ length: Math.min(pageSize, vis.length) }, (_, i) => vis[(start + i) % vis.length] as WidgetState);
}

/** 帧预算（F01974）：组件区动画帧上限（ms/帧）。 */
export function widgetFrameBudgetMs(): number { return 16; }
