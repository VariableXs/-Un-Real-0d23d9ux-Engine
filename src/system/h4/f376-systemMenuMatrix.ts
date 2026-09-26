/**
 * F376 标题栏系统菜单（H 域 · AI-H4）：
 * 标题栏右键（或 Alt+空格）呼出标准系统菜单六项：还原/移动/大小/最小化/最大化/关闭——
 * 顺序与 Windows 逐字对齐（交互词典），前四项在对应状态置灰；「移动/大小」进入键盘
 * 模式（F377）。菜单出现位置=鼠标点下方 2px（F215 同规）。
 * 判据（主册 F376）：六项顺序对照；置灰状态机（四状态×六项矩阵）；键盘模式衔接；
 * 菜单几何偏移。
 * 说明：V-21 批已有经典系统菜单模型（systemMenu.ts，单窗态）；本模块补齐 F376 判据的
 * 完整四状态×六项置灰矩阵与 2px 几何口径，供 V-21 消费（复用不重写）。
 */

import type { SystemMenuItemId } from "../windows/systemMenu";

/** 四窗口状态（判据「四状态×六项矩阵」）。 */
export type WindowStatus = "normal" | "maximized" | "minimized" | "snapped";

/** F376 菜单六项（顺序与 Windows 逐字对齐——交互词典）。 */
export const MENU_ORDER: SystemMenuItemId[] = ["restore", "move", "size", "minimize", "maximize", "close"];

export const MENU_LABELS: Record<SystemMenuItemId, string> = {
  restore: "还原",
  move: "移动",
  size: "大小",
  minimize: "最小化",
  maximize: "最大化",
  close: "关闭",
};

/** 菜单出现几何：鼠标点下方 2px（判据）。 */
export const MENU_OFFSET_PX = 2;

export function menuOrigin(cursor: { x: number; y: number }): { x: number; y: number } {
  return { x: cursor.x, y: cursor.y + MENU_OFFSET_PX };
}

/**
 * 置灰状态机（判据核心——四状态×六项矩阵，一处一事实）：
 * - normal：还原灰；其余全亮；
 * - maximized：最大化灰、移动灰、大小灰（最大化态不可移/调）；还原/最小化/关闭亮；
 * - minimized：最小化灰、移动灰、大小灰；还原/最大化/关闭亮；
 * - snapped：最大化灰（贴靠态以还原代替最大化）；其余同 normal。
 */
export function disabledMatrix(status: WindowStatus): Record<SystemMenuItemId, boolean> {
  switch (status) {
    case "normal":
      return { restore: true, move: false, size: false, minimize: false, maximize: false, close: false };
    case "maximized":
      return { restore: false, move: true, size: true, minimize: false, maximize: true, close: false };
    case "minimized":
      return { restore: false, move: true, size: true, minimize: true, maximize: false, close: false };
    case "snapped":
      return { restore: false, move: false, size: false, minimize: false, maximize: true, close: false };
  }
}

export interface F376MenuItem {
  id: SystemMenuItemId;
  label: string;
  disabled: boolean;
}

/** 按序构建菜单（顺序判据 + 置灰矩阵一次成型）。 */
export function buildMenu(status: WindowStatus): F376MenuItem[] {
  const matrix = disabledMatrix(status);
  return MENU_ORDER.map((id) => ({ id, label: MENU_LABELS[id], disabled: matrix[id] }));
}

/** 键盘模式衔接（判据）：「移动/大小」命中后返回应进入的 F377 模式。 */
export function keyboardModeFor(id: SystemMenuItemId): "move" | "size" | null {
  return id === "move" ? "move" : id === "size" ? "size" : null;
}

/** 状态机完整性审计：四状态全矩阵可生成且六项齐（判据走查面）。 */
export function auditMatrix(): { pass: boolean; missing: string[] } {
  const missing: string[] = [];
  for (const status of ["normal", "maximized", "minimized", "snapped"] as const) {
    const menu = buildMenu(status);
    if (menu.length !== 6) missing.push(`${status}: 项数 ${menu.length}`);
    if (menu.some((m) => m.disabled && m.id === "close")) missing.push(`${status}: 关闭不可置灰`);
  }
  return { pass: missing.length === 0, missing };
}
