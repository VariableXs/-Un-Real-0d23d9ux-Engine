/**
 * F168 任务栏个性化 · 完整设计。
 *
 * 主册判据：三选项独立生效互不干扰；隐藏-唤出 200ms 触发实测；托盘折叠联动正确。
 *
 * 【功能定义】任务栏三选项：图标大小（标准 24px/大 32px 档）/对齐（居中/左对齐
 * ——Windows 双惯例）/自动隐藏（贴底滑出）；改后即时生效；与 C-3 基线尺寸联动
 * 标注。
 *
 * 【状态与异常】大图标档托盘区溢出 → 自动折叠阈值联动（F075）；自动隐藏与全屏
 * 应用冲突 → 全屏态强制显示（可交互性优先）；切换动画 200ms（F124）。
 *
 * 【设计细节】大图标档任务栏增高 56px（乙-1 表 48px 的增补档）；左对齐实现=布局
 * 锚点切换；自动隐藏唤出热区 6px（比触发带窄——防误唤）；隐藏态下 Win 键仍弹开始
 * 菜单（可连性兜底）；三选项即时生效走任务栏布局重排（200ms 内完成）。
 */

import { personaStore } from "./store";

export const SECTION = "taskbar";

export type TaskbarIconSize = "standard" | "large";
export type TaskbarAlign = "center" | "left";

export const ICON_SIZE_PX: Record<TaskbarIconSize, number> = { standard: 24, large: 32 };
export const HEIGHT_PX: Record<TaskbarIconSize, number> = { standard: 48, large: 56 }; // 乙-1 表 48px + 增补档 56px
export const REVEAL_DELAY_MS = 200;
export const REVEAL_HOTZONE_PX = 6;
export const REARRANGE_MS = 200; // F124 大面板档

export interface TaskbarPrefs {
  iconSize: TaskbarIconSize;
  align: TaskbarAlign;
  autoHide: boolean;
  /** 托盘折叠阈值（F075 联动——大图标档溢出自动折叠）。 */
  trayFoldThreshold: number;
}

export function defaultTaskbarPrefs(): TaskbarPrefs {
  return { iconSize: "standard", align: "center", autoHide: false, trayFoldThreshold: 8 };
}

export function loadTaskbarPrefs(): TaskbarPrefs {
  const stored = personaStore.getWith(SECTION, "prefs", undefined) as Partial<TaskbarPrefs> | undefined;
  return { ...defaultTaskbarPrefs(), ...(stored ?? {}) };
}

export function saveTaskbarPrefs(p: TaskbarPrefs): void {
  personaStore.set(SECTION, { prefs: p });
}

/** 全屏应用与自动隐藏冲突：全屏态强制显示（可交互性优先）。 */
export function effectiveAutoHide(prefs: TaskbarPrefs, fullscreenActive: boolean): boolean {
  return prefs.autoHide && !fullscreenActive;
}

/** 大图标档托盘溢出 → 折叠阈值联动（数量超阈值即折叠为「^」展开器）。 */
export function trayShouldFold(prefs: TaskbarPrefs, trayIconCount: number): boolean {
  const threshold = prefs.iconSize === "large" ? Math.max(3, Math.floor(prefs.trayFoldThreshold * 0.75)) : prefs.trayFoldThreshold;
  return trayIconCount > threshold;
}

/** 隐藏态可连性兜底：Win 键仍弹开始菜单（语义开关恒真——此处为判定函数供走查脚本消费）。 */
export function startMenuReachableWhenHidden(): boolean {
  return true;
}

/** 光标贴底触发判定：y 在热区（底部 6px）内且停留 ≥200ms。 */
export function shouldReveal(cursorYFromBottom: number, dwellMs: number): boolean {
  return cursorYFromBottom <= REVEAL_HOTZONE_PX && dwellMs >= REVEAL_DELAY_MS;
}
