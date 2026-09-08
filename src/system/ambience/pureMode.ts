/**
 * AI-18 M-67 桌面纯净模式 — 纯逻辑状态机。
 *
 * 口径：
 * - 三层（图标/任务栏/横幅）独立记忆，退出恢复原状；
 * - 进入/退出 200ms 淡出淡入（CSS 类 pure-mode-hidden，tokens.css 已定义）；
 * - 通知仍入中心不打扰；任务栏隐藏期间角标移到屏幕边缘一枚小点；
 * - 不自动屏蔽任何功能（纯净 ≠ 禁用，只是不显示）。
 */

export type PureLayer = "icons" | "taskbar" | "banners";

export const PURE_LAYERS: readonly PureLayer[] = ["icons", "taskbar", "banners"];

export interface PureModeState {
  /** 是否在纯净模式中。 */
  active: boolean;
  /** 进入前各层原状（退出精确还原）。 */
  before: Record<PureLayer, boolean>;
}

export const PURE_MODE_OFF: PureModeState = {
  active: false,
  before: { icons: true, taskbar: true, banners: true },
};

/**
 * 进入纯净模式：记住当前各层可见性，全部隐藏。
 * @param current 各层当前可见性
 */
export function enterPureMode(current: Record<PureLayer, boolean>): PureModeState {
  return { active: true, before: { ...current } };
}

/** 退出纯净模式：返回各层原状映射（精确还原）。 */
export function exitPureMode(s: PureModeState): Record<PureLayer, boolean> {
  return { ...s.before };
}

/** 纯净模式下各层应为的可见性。 */
export function pureLayerVisibility(s: PureModeState): Record<PureLayer, boolean> {
  return s.active ? { icons: false, taskbar: false, banners: false } : { ...s.before };
}
