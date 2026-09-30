/**
 * V-04 双击桌面空白动作（localStorage 持久化，默认 none = 等于现状）。
 * 动作本身由 DesktopShell 监听 window 事件 "ai04:desktop-action" 执行（动作桥）。
 */
export type DoubleClickAction = "none" | "show-desktop" | "minimize-all" | "palette" | "lock";

export const DOUBLE_CLICK_ACTIONS: readonly DoubleClickAction[] = [
  "none",
  "show-desktop",
  "minimize-all",
  "palette",
  "lock",
];

const LS_KEY = "variable:desktop:doubleClickAction";

export function loadDoubleClickAction(): DoubleClickAction {
  try {
    const raw = localStorage.getItem(LS_KEY);
    return DOUBLE_CLICK_ACTIONS.includes(raw as DoubleClickAction) ? (raw as DoubleClickAction) : "none";
  } catch {
    return "none";
  }
}

export function saveDoubleClickAction(a: DoubleClickAction): void {
  try {
    localStorage.setItem(LS_KEY, a);
  } catch {
    /* storage blocked → 不持久化 */
  }
}