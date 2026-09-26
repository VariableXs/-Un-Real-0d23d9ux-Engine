/**
 * F167 右键菜单自定义 · 完整设计。
 *
 * 主册判据：隐藏-生效-恢复闭环；自动排序与 F072 计数一致；关键项锁定实测。
 *
 * 【功能定义】资源管理器右键菜单可定制：隐藏不常用项（用户白名单恢复）/应用注册
 * 项按使用频率自动排序（可选）；菜单结构基线仍对齐 Windows（乙-4 表）——定制是
 * 修饰不是重构。
 *
 * 【状态与异常】系统关键项（删除/重命名/属性）→ 锁定不可隐（防自残——红线显式）；
 * 应用卸载 → 其菜单项自动清；隐藏项过多（>50%）→ 提示菜单可能不合群（仍允许）。
 *
 * 【设计细节】使用计数打点在菜单项选择时刻（零额外开销）；自动排序仅对「打开方式」
 * 组与应用注册组生效（系统组位置不动——乙-4 对齐承诺）；隐藏项在「显示更多选项」
 * 二级完整保留（功能不丢只收纳）；菜单弹出延迟红线 100ms（定制不增负）。
 */

import { personaStore } from "./store";

export const SECTION = "ctxmenu";
export const POPUP_BUDGET_MS = 100;
export const HIDDEN_RATIO_WARN = 0.5;
export const USAGE_RING_DAYS = 90;

/** 乙-4 菜单基线（系统组——位置不可动）。 */
export const SYSTEM_ITEMS: readonly { id: string; zh: string; locked: boolean }[] = [
  { id: "open", zh: "打开", locked: false },
  { id: "open-with", zh: "打开方式", locked: false },
  { id: "cut", zh: "剪切", locked: false },
  { id: "copy", zh: "复制", locked: false },
  { id: "rename", zh: "重命名", locked: true },
  { id: "delete", zh: "删除", locked: true },
  { id: "properties", zh: "属性", locked: true },
  { id: "send-to", zh: "发送到", locked: false },
] as const;

/** 锁定项集合（防自残红线——显式不可隐）。 */
export const LOCKED_ITEMS: ReadonlySet<string> = new Set(SYSTEM_ITEMS.filter((i) => i.locked).map((i) => i.id));

export interface CtxMenuItemState {
  id: string;
  /** 应用注册组标记（自动排序作用面）。 */
  appRegistered: boolean;
  hidden: boolean;
}

export interface CtxMenuConfig {
  items: Record<string, CtxMenuItemState>;
  autoSort: boolean;
  /** 使用计数环形（90 天）：itemId → [{day, count}]。 */
  usage: Record<string, { day: string; count: number }[]>;
}

export function defaultCtxMenuConfig(): CtxMenuConfig {
  const items: Record<string, CtxMenuItemState> = {};
  for (const i of SYSTEM_ITEMS) items[i.id] = { id: i.id, appRegistered: false, hidden: false };
  return { items, autoSort: false, usage: {} };
}

export function loadCtxMenuConfig(): CtxMenuConfig {
  const stored = personaStore.getWith(SECTION, "menu", undefined) as Partial<CtxMenuConfig> | undefined;
  const d = defaultCtxMenuConfig();
  if (!stored) return d;
  return {
    items: { ...d.items, ...(typeof stored.items === "object" && stored.items !== null ? stored.items : {}) },
    autoSort: typeof stored.autoSort === "boolean" ? stored.autoSort : false,
    usage: typeof stored.usage === "object" && stored.usage !== null ? stored.usage : {},
  };
}

export function saveCtxMenuConfig(c: CtxMenuConfig): void {
  personaStore.set(SECTION, { menu: c });
}

export interface HideResult {
  ok: boolean;
  reason: string;
  config: CtxMenuConfig;
}

/** 隐藏项：锁定项拒绝（三要素）；功能不丢——隐藏项进「显示更多选项」二级。 */
export function hideItem(config: CtxMenuConfig, id: string): HideResult {
  if (LOCKED_ITEMS.has(id)) {
    return { ok: false, reason: `「${SYSTEM_ITEMS.find((i) => i.id === id)?.zh ?? id}」是系统关键项，不可隐藏（防止菜单自残）`, config };
  }
  const item = config.items[id];
  if (!item) return { ok: false, reason: "菜单项不存在", config };
  return {
    ok: true,
    reason: "已隐藏（在「显示更多选项」二级完整保留）",
    config: { ...config, items: { ...config.items, [id]: { ...item, hidden: true } } },
  };
}

export function restoreItem(config: CtxMenuConfig, id: string): HideResult {
  const item = config.items[id];
  if (!item) return { ok: false, reason: "菜单项不存在", config };
  return {
    ok: true,
    reason: "已恢复显示",
    config: { ...config, items: { ...config.items, [id]: { ...item, hidden: false } } },
  };
}

/** 隐藏比例警告（>50% → 提示但允许）。 */
export function hiddenRatioWarning(config: CtxMenuConfig): string | null {
  const list = Object.values(config.items);
  if (list.length === 0) return null;
  const hidden = list.filter((i) => i.hidden).length;
  return hidden / list.length > HIDDEN_RATIO_WARN ? `已隐藏 ${hidden}/${list.length} 项，菜单可能不合群` : null;
}

// ---------- 使用计数（F072 复用语义——打点在菜单项选择时刻） ----------

function dayKey(now: number): string {
  const d = new Date(now);
  const p = (n: number) => n.toString().padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

/** 选择时刻打点（零额外开销：一次查表+一次递增）。 */
export function recordUsage(config: CtxMenuConfig, id: string, now: number): CtxMenuConfig {
  const day = dayKey(now);
  const ring = config.usage[id] ?? [];
  const today = ring.find((r) => r.day === day);
  const next = today ? ring.map((r) => (r.day === day ? { ...r, count: r.count + 1 } : r)) : [...ring, { day, count: 1 }];
  // 环形 90 天清理。
  const trimmed = next.filter((r) => r.day >= cutoffDay(now)).slice(-USAGE_RING_DAYS);
  return { ...config, usage: { ...config.usage, [id]: trimmed } };
}

function cutoffDay(now: number): string {
  const d = new Date(now - USAGE_RING_DAYS * 86400_000);
  const p = (n: number) => n.toString().padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

export function usageCount(config: CtxMenuConfig, id: string): number {
  return (config.usage[id] ?? []).reduce((acc, r) => acc + r.count, 0);
}

/**
 * 自动排序：仅对「打开方式」组与应用注册组生效（系统组位置不动——乙-4 对齐承诺）。
 * 输入当前可见项序列，输出按 90 天计数降序的应用组排序。
 */
export function autoSort(config: CtxMenuConfig, visibleIds: string[]): string[] {
  if (!config.autoSort) return visibleIds;
  const systemPos = new Map<string, number>();
  visibleIds.forEach((id, idx) => systemPos.set(id, idx));
  const isSortable = (id: string) => {
    const item = config.items[id];
    return item?.appRegistered === true || id === "open-with";
  };
  const sortable = visibleIds.filter(isSortable).sort((a, b) => usageCount(config, b) - usageCount(config, a));
  const rest = visibleIds.filter((id) => !isSortable(id));
  // 系统组保序在前，应用组按计数排后（打开方式组归应用组排序）。
  return [...rest.filter((id) => !sortable.includes(id) && config.items[id]?.appRegistered !== true), ...sortable];
}

/** 应用卸载 → 其菜单项自动清。 */
export function pruneUninstalledItems(config: CtxMenuConfig, registeredIds: Set<string>): { config: CtxMenuConfig; removed: string[] } {
  const removed: string[] = [];
  const items: Record<string, CtxMenuItemState> = {};
  for (const [id, item] of Object.entries(config.items)) {
    if (item.appRegistered && !registeredIds.has(id)) {
      removed.push(id);
      continue;
    }
    items[id] = item;
  }
  return { config: { ...config, items }, removed };
}
