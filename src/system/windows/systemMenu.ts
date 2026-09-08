/**
 * V-21 经典系统菜单复刻（化境 · AI-2 窗口与键位路）：
 * 标题栏图标右键 / Alt+Space 唤出经典系统菜单：
 * 还原 / 移动 / 大小 / 最小化 / 最大化 / 关闭；
 * 「移动」「大小」进入键盘微调模式（方向键 1px、Shift+方向 10px、Esc 退出、Enter 确认）。
 * - 菜单项按当前窗口态动态禁用（最大化的窗口禁用「移动/大小」）；
 * - 不做菜单项增删自定义；不做系统窗口菜单注入（只管 VWM 窗口）；
 * - Alt+Space 对系统语义让位由键位注册表层负责，本层只提供菜单模型。
 */

export type WindowState = "normal" | "max" | "minimized";

export type SystemMenuItemId = "restore" | "move" | "size" | "minimize" | "maximize" | "close";

export interface SystemMenuItem {
  id: SystemMenuItemId;
  label: string;
  disabled: boolean;
}

/** 三态禁用逻辑（验收：正常/最大化/最小化三态禁用逻辑正确）。 */
export function systemMenuItems(state: WindowState): SystemMenuItem[] {
  const isMax = state === "max";
  const isMin = state === "minimized";
  return [
    { id: "restore", label: "还原", disabled: !isMax && !isMin },
    { id: "move", label: "移动", disabled: isMax || isMin },
    { id: "size", label: "大小", disabled: isMax || isMin },
    { id: "minimize", label: "最小化", disabled: isMin },
    { id: "maximize", label: "最大化", disabled: isMax },
    { id: "close", label: "关闭", disabled: false },
  ];
}

// ---------- 键盘微调模式（V-21「移动/大小」） ----------

export type NudgeMode = "move" | "size";

export interface NudgeState {
  mode: NudgeMode;
  rect: { x: number; y: number; w: number; h: number };
}

/** 进入键盘微调（方向键 1px、Shift+方向 10px）。 */
export function nudgeBegin(mode: NudgeMode, rect: { x: number; y: number; w: number; h: number }): NudgeState {
  return { mode, rect: { ...rect } };
}

/**
 * 微调一步：dir 四向；shift=10px 精度。size 模式调整宽高（最小 300×280
 * 与 VWM 工具窗口最小几何一致）；move 模式调整位置。
 * 返回 null 表示 Esc/无效输入（调用方退出微调模式）。
 */
export function nudgeStep(
  st: NudgeState,
  dir: "left" | "right" | "up" | "down",
  shift: boolean,
): NudgeState {
  const d = shift ? 10 : 1;
  const r = { ...st.rect };
  if (st.mode === "move") {
    if (dir === "left") r.x -= d;
    if (dir === "right") r.x += d;
    if (dir === "up") r.y -= d;
    if (dir === "down") r.y += d;
  } else {
    if (dir === "left") r.w = Math.max(300, r.w - d);
    if (dir === "right") r.w += d;
    if (dir === "up") r.h = Math.max(280, r.h - d);
    if (dir === "down") r.h += d;
  }
  return { ...st, rect: r };
}
