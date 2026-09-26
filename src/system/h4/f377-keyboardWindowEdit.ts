/**
 * F377 键盘移动与调整窗口（H 域 · AI-H4）：
 * 系统菜单选「移动/大小」后窗口进入键盘编排模式：方向键移窗（大小模式改边界）、
 * Enter 落定、Esc 还原原地——无鼠标环境编排窗口的最后手段（F206 纪律的窗口层兑现）；
 * 键盘模式下窗口半透明 80% 提示「正在编排」，边界到达屏幕边缘时窗口贴边不再越界。
 * 判据（主册 F377）：移动/大小两模式方向键行为；Enter 落定精度（<1px）；Esc 还原；
 * 半透明提示；边界贴边。
 * 依赖锚点：F206 焦点与键盘导航 / F376 菜单衔接。
 */

export type KeyboardEditMode = "move" | "size";

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** 方向键步进（px）：普通 1；Shift 加速 10（键盘编排的效率档）。 */
export const STEP_PX = 1;
export const STEP_FAST_PX = 10;

export interface KeyboardEditSession {
  mode: KeyboardEditMode;
  /** 进入时的原地几何（Esc 还原的落点）。 */
  origin: Rect;
  /** 当前几何（编排中实时变化）。 */
  current: Rect;
  /** 屏幕工作区（边界贴边判据的钳制域）。 */
  workArea: { x: number; y: number; w: number; h: number };
}

export function beginEdit(mode: KeyboardEditMode, rect: Rect, workArea: { x: number; y: number; w: number; h: number }): KeyboardEditSession {
  return { mode, origin: { ...rect }, current: { ...rect }, workArea };
}

/** 方向键：move 改 x/y；size 改 w/h（右/下增大，左/上减小、最小 40 钳制）。 */
export function arrowKey(s: KeyboardEditSession, dir: "up" | "down" | "left" | "right", fast: boolean): KeyboardEditSession {
  const step = fast ? STEP_FAST_PX : STEP_PX;
  const clampW = Math.max(40, Math.min(s.current.w, s.workArea.w));
  const clampH = Math.max(40, Math.min(s.current.h, s.workArea.h));
  if (s.mode === "move") {
    const dx = dir === "left" ? -step : dir === "right" ? step : 0;
    const dy = dir === "up" ? -step : dir === "down" ? step : 0;
    const x = Math.min(Math.max(s.current.x + dx, s.workArea.x), s.workArea.x + s.workArea.w - clampW);
    const y = Math.min(Math.max(s.current.y + dy, s.workArea.y), s.workArea.y + s.workArea.h - clampH);
    return { ...s, current: { ...s.current, x, y } };
  }
  const dw = dir === "left" ? -step : dir === "right" ? step : 0;
  const dh = dir === "up" ? -step : dir === "down" ? step : 0;
  const w = Math.min(Math.max(s.current.w + dw, 40), s.workArea.w);
  const h = Math.min(Math.max(s.current.h + dh, 40), s.workArea.h);
  return { ...s, current: { ...s.current, w, h } };
}

/** Enter 落定：返回当前几何（精度判据：返回值与 current 逐字段恒等——<1px 的实现是零变换）。 */
export function commit(s: KeyboardEditSession): { committed: Rect; driftPx: number } {
  return { committed: { ...s.current }, driftPx: 0 };
}

/** Esc 还原原地（判据）：回到进入时的 origin。 */
export function cancelEdit(s: KeyboardEditSession): Rect {
  return { ...s.origin };
}

/** 半透明提示（判据）：编排中窗口 80% 不透明度提示「正在编排」。 */
export function editVisual(s: KeyboardEditSession): { opacity: number; hint: string } {
  void s;
  return { opacity: 0.8, hint: "正在编排——方向键调整，Enter 落定，Esc 取消" };
}

/** 边界贴边自证：连续 N 步越界方向键后，几何恒在工作区内（判据「贴边不再越界」）。 */
export function auditEdgeClamp(s0: KeyboardEditSession, dir: "up" | "down" | "left" | "right", steps = 50): { pass: boolean; final: Rect } {
  let s = s0;
  for (let i = 0; i < steps; i++) s = arrowKey(s, dir, true);
  const r = s.current;
  const within = r.x >= s.workArea.x && r.y >= s.workArea.y && r.x + r.w <= s.workArea.x + s.workArea.w && r.y + r.h <= s.workArea.y + s.workArea.h;
  return { pass: within, final: r };
}
