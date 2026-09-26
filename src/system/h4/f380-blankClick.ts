/**
 * F380 空行点击清除选择（H 域 · AI-H4）：
 * 列表空白区单击=清除全部选择（拖拽选择框误选 200 项后的救命单击）；
 * Ctrl+单击空白=保留选择（手滑保护）；右键空白=列表背景菜单（粘贴/刷新/排序/属性——
 * 与项上右键菜单明确区分，不给「删除」这类无对象操作）。
 * 判据（主册 F380）：三点击行为（单击/Ctrl 单击/右键）；空白菜单项清单审计（无对象操作=0）；
 * 清除后焦点处理；与 F203 框选衔接。
 * 依赖锚点：F203 拖拽选择框。
 */

export interface ListSelection {
  selectedIds: string[];
  /** F203 框选锚点（清除选择后锚点一并清——衔接判据）。 */
  anchorId: string | null;
}

/** 空白菜单允许项（判据：无对象操作=0——本清单即审计口径）。 */
export const BACKGROUND_MENU_ITEMS = ["paste", "refresh", "sortBy", "properties", "newFolder"] as const;
export type BackgroundMenuItem = (typeof BACKGROUND_MENU_ITEMS)[number];

/** 禁止出现在空白菜单的无对象操作（判据红线清单）。 */
const FORBIDDEN_ITEMS: ReadonlySet<string> = new Set(["delete", "rename", "cut", "copy", "open"]);

export interface ClickResult {
  selection: ListSelection;
  /** 焦点落点（判据「清除后焦点处理」）：清除后焦点回列表容器本身。 */
  focusGoesTo: "list-container" | "item";
  focusedItemId: string | null;
  /** 菜单（仅右键空白时）。 */
  menu: BackgroundMenuItem[] | null;
}

/** 单击空白：清除全部选择 + 框选锚点（判据「救命单击」）。 */
export function clickBlank(_selection: ListSelection): ClickResult {
  return { selection: { selectedIds: [], anchorId: null }, focusGoesTo: "list-container", focusedItemId: null, menu: null };
}

/** Ctrl+单击空白：保留选择（手滑保护判据），焦点回容器。 */
export function ctrlClickBlank(selection: ListSelection): ClickResult {
  return {
    selection: { selectedIds: [...selection.selectedIds], anchorId: selection.anchorId },
    focusGoesTo: "list-container",
    focusedItemId: null,
    menu: null,
  };
}

/** 右键空白：选择处理同单击（无对象上下文），弹背景菜单（明确区分项上菜单）。 */
export function rightClickBlank(selection: ListSelection): ClickResult {
  return { ...clickBlank(selection), menu: [...BACKGROUND_MENU_ITEMS] };
}

/** 空白菜单项审计（判据）：白名单内且不触红线清单（无对象操作=0）。 */
export function auditBackgroundMenu(items: string[]): { pass: boolean; forbidden: string[]; unknown: string[] } {
  const allowed = new Set<string>(BACKGROUND_MENU_ITEMS);
  return {
    pass: items.every((i) => allowed.has(i)),
    forbidden: items.filter((i) => FORBIDDEN_ITEMS.has(i)),
    unknown: items.filter((i) => !allowed.has(i) && !FORBIDDEN_ITEMS.has(i)),
  };
}

/** 与 F203 框选衔接：框选结束松手在空白区 → 等价单击空白（清除）。 */
export function rubberBandReleaseOnBlank(selection: ListSelection, releasedOnBlank: boolean): ListSelection {
  return releasedOnBlank ? clickBlank(selection).selection : selection;
}

/** 多选 200 项后单击空白的性能面：清空为 O(1) 新引用（不逐项遍历）。 */
export function bulkClearPerformance(selection: ListSelection): { cleared: boolean; complexity: "O(1)" } {
  const r = clickBlank(selection);
  return { cleared: r.selection.selectedIds.length === 0 && r.selection.anchorId === null, complexity: "O(1)" };
}
