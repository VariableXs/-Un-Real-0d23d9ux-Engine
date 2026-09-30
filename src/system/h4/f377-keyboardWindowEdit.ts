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

/* ================= v4 深化批次四：边缘吸附 / 步进撤销栈 / 300 步自检走查 ================= */

/** 吸附阈值（判据「边界贴边」的量化口径）：距工作区边缘 ≤8px 即吸齐——手抖差 3px 也算到边。 */
export const SNAP_PX = 8;

/** 全量钳制：几何强制落回工作区（宽高超工作区时先缩再移）。 */
export function clampRect(rect: Rect, workArea: { x: number; y: number; w: number; h: number }): Rect {
  const w = Math.min(rect.w, workArea.w);
  const h = Math.min(rect.h, workArea.h);
  const x = Math.min(Math.max(rect.x, workArea.x), workArea.x + workArea.w - w);
  const y = Math.min(Math.max(rect.y, workArea.y), workArea.y + workArea.h - h);
  return { x, y, w, h };
}

/** 边缘吸附：四边 8px 内吸齐工作区边缘（键盘编排的「到位感」）。 */
export function snapToEdges(rect: Rect, workArea: { x: number; y: number; w: number; h: number }): Rect {
  const c = clampRect(rect, workArea);
  const snap = (v: number, target: number) => (Math.abs(v - target) <= SNAP_PX ? target : v);
  const x = snap(c.x, workArea.x);
  const y = snap(c.y, workArea.y);
  const w = Math.abs(c.x + c.w - (workArea.x + workArea.w)) <= SNAP_PX ? workArea.x + workArea.w - x : c.w;
  const h = Math.abs(c.y + c.h - (workArea.y + workArea.h)) <= SNAP_PX ? workArea.y + workArea.h - y : c.h;
  return { x, y, w, h };
}

/** 编排内步进历史：Esc 是「还原全部」（origin）；Ctrl+Z 是「退一步」（本栈）——两层撤销。 */
export interface EditHistory {
  past: Rect[];
}

export function pushStep(h: EditHistory, rect: Rect): EditHistory {
  return { past: [...h.past.slice(-49), { ...rect }] };
}

export function undoStep(h: EditHistory): { history: EditHistory; rect: Rect | null } {
  if (h.past.length === 0) return { history: h, rect: null };
  const past = [...h.past];
  const rect = past.pop()!;
  return { history: { past }, rect };
}

/** 300 步自检走查（判据「边界贴边」加强版）：四方向+快慢循环 300 步，每步钳制不变量全查。 */
export function selfCheckWalk(s0: KeyboardEditSession, steps = 300): { pass: boolean; violations: number; final: Rect } {
  const dirs = ["up", "right", "down", "left"] as const;
  let s = s0;
  let violations = 0;
  for (let i = 0; i < steps; i++) {
    s = arrowKey(s, dirs[i % 4]!, i % 7 === 0);
    const r = s.current;
    const within = r.x >= s.workArea.x && r.y >= s.workArea.y && r.x + r.w <= s.workArea.x + s.workArea.w && r.y + r.h <= s.workArea.y + s.workArea.h;
    if (!within) violations++;
  }
  return { pass: violations === 0, violations, final: s.current };
}
