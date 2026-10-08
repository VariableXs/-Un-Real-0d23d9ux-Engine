/**
 * Z-09 系统组合键让位协议 — 内置系统组合表（40+）。
 *
 * 语义档位：
 * - passthrough：永不拦截，直接让给系统/应用（注册表禁止登记）；
 * - yield：全屏或有前台输入焦点时让位；非全屏桌面层可消费；
 * - translate：翻译为本引擎动作（登记进 Z-08 注册表）。
 *
 * 全屏让位：Z-18 全屏检测接口未落地前，先以 `fullscreenActive` 标志位对接
 * （setFullscreenActive()，由窗口管理侧在检测到全屏前台时置位）。
 *
 * 与 M-36 系统保留键库同源共用本常量（SYSTEM_RESERVED combos 集合）。
 */

export type ComboDisposition = "passthrough" | "yield" | "translate";

export interface SystemCombo {
  combo: string;
  disposition: ComboDisposition;
  /** 让位/翻译条件说明（行为表 docs/SYSTEM-COMBOS.md 同步）。 */
  note: string;
}

export const SYSTEM_COMBOS: SystemCombo[] = [
  // —— Win 系（系统保留，passthrough）——
  { combo: "super+e", disposition: "passthrough", note: "资源管理器（Win11 保留）" },
  { combo: "super+d", disposition: "passthrough", note: "显示桌面" },
  { combo: "super+m", disposition: "passthrough", note: "最小化全部" },
  { combo: "super+n", disposition: "passthrough", note: "通知中心" },
  { combo: "super+i", disposition: "passthrough", note: "系统设置" },
  { combo: "super+v", disposition: "passthrough", note: "剪贴板历史（Win11）" },
  { combo: "super+s", disposition: "passthrough", note: "搜索" },
  { combo: "super+l", disposition: "passthrough", note: "锁定" },
  { combo: "super+tab", disposition: "translate", note: "任务视图；winTabSwitcher 开启时翻译为 wintab（既有契约）" },
  { combo: "super+left", disposition: "yield", note: "贴靠；全屏让位" },
  { combo: "super+right", disposition: "yield", note: "贴靠；全屏让位" },
  { combo: "super+up", disposition: "yield", note: "最大化；全屏让位" },
  { combo: "super+down", disposition: "yield", note: "还原/最小化；全屏让位" },
  { combo: "super+1", disposition: "passthrough", note: "任务栏槽位（Win 保留）" },
  { combo: "super+2", disposition: "passthrough", note: "任务栏槽位" },
  { combo: "super+3", disposition: "passthrough", note: "任务栏槽位" },
  { combo: "super+4", disposition: "passthrough", note: "任务栏槽位" },
  { combo: "super+5", disposition: "passthrough", note: "任务栏槽位" },
  { combo: "super+6", disposition: "passthrough", note: "任务栏槽位" },
  { combo: "super+7", disposition: "passthrough", note: "任务栏槽位" },
  { combo: "super+8", disposition: "passthrough", note: "任务栏槽位" },
  { combo: "super+9", disposition: "passthrough", note: "任务栏槽位" },
  // —— Alt 系 ——
  { combo: "alt+tab", disposition: "passthrough", note: "窗口切换（永不拦截）" },
  { combo: "alt+f4", disposition: "passthrough", note: "关闭窗口" },
  { combo: "alt+escape", disposition: "passthrough", note: "窗口循环" },
  { combo: "alt+space", disposition: "yield", note: "系统菜单；桌面层可消费" },
  // —— Ctrl+Alt 系（登录安全级，passthrough）——
  { combo: "ctrl+alt+delete", disposition: "passthrough", note: "安全屏（SAS，不可拦截）" },
  { combo: "ctrl+alt+arrowleft", disposition: "yield", note: "虚拟桌面切换（Win11）" },
  { combo: "ctrl+alt+arrowright", disposition: "yield", note: "虚拟桌面切换" },
  // —— Ctrl+Shift 系（输入法/系统）——
  { combo: "ctrl+shift+escape", disposition: "passthrough", note: "任务管理器" },
  { combo: "ctrl+shift+arrowleft", disposition: "yield", note: "IME 选择文字" },
  { combo: "ctrl+shift+arrowright", disposition: "yield", note: "IME 选择文字" },
  // —— 功能键 ——
  { combo: "f11", disposition: "yield", note: "全屏切换；全屏时让位给应用" },
  { combo: "f12", disposition: "yield", note: "开发者工具（调试构建）" },
  { combo: "f5", disposition: "yield", note: "刷新；编辑焦点内让位" },
  { combo: "printscreen", disposition: "passthrough", note: "截图（系统）" },
  { combo: "f1", disposition: "yield", note: "帮助；应用优先" },
  { combo: "f6", disposition: "passthrough", note: "焦点循环" },
  { combo: "f10", disposition: "passthrough", note: "菜单栏" },
  // —— IME / 辅助 ——
  { combo: "shift", disposition: "passthrough", note: "中英切换（IME）" },
  { combo: "ctrl+space", disposition: "passthrough", note: "IME 开关" },
  { combo: "ctrl+alt+shift", disposition: "passthrough", note: "IME 保留" },
  { combo: "escape", disposition: "yield", note: "M-34 浮层栈消费；双击 Esc 切环境独立判定" },
];

/** M-36 系统保留键库：passthrough/yield 全部视为保留（用户自定义不得占用）。 */
export const SYSTEM_RESERVED: ReadonlySet<string> = new Set(
  SYSTEM_COMBOS.filter((c) => c.disposition !== "translate").map((c) => c.combo),
);

/** Z-18 未落地前的全屏标志位。 */
let fullscreenActive = false;

export function setFullscreenActive(active: boolean): void {
  fullscreenActive = active;
}

export function isFullscreenActive(): boolean {
  return fullscreenActive;
}

export interface YieldDecision {
  /** true = 本引擎不得消费，事件让给系统/应用。 */
  yield: boolean;
  reason: string;
}

/** 让位判定：全屏自动全让位（yield/passthrough 均）；passthrough 永让。 */
export function shouldYield(combo: string): YieldDecision {
  const entry = SYSTEM_COMBOS.find((c) => c.combo === combo);
  if (!entry) return { yield: false, reason: "not-system-combo" };
  if (entry.disposition === "passthrough") return { yield: true, reason: `passthrough:${entry.note}` };
  if (fullscreenActive) return { yield: true, reason: `fullscreen:${entry.note}` };
  return { yield: false, reason: `consume:${entry.note}` };
}
