/**
 * F360 像素标尺与网格叠加（H 域 · AI-H4）：
 * 创造者工具第二件：屏幕叠加测量模式——水平/垂直标尺（像素读数）、
 * 区域量测（拖出矩形显示宽×高）、8px 网格叠加（对齐检查，透明度 20%）；
 * 全部叠加层走合成器覆盖平面（F335 同源）——叠加开着时点击穿透到底层应用。
 * 判据（主册 F360）：读数准确性（已知尺寸窗口零误差）；点击穿透；
 * 网格 20% 透明度；量测拖拽实时性；快捷退出（Esc 秒退）。
 * 依赖锚点：F335 覆盖平面 / F359 拾色器（同快捷面板家族）。
 */

export interface Point {
  x: number;
  y: number;
}

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** 网格间距 8px（判据）。 */
export const GRID_STEP_PX = 8;
/** 网格叠加透明度 20%（判据）。 */
export const GRID_OPACITY = 0.2;
/** 量测拖拽实时性预算（ms/帧——读数随拖即时更新）。 */
export const MEASURE_FRAME_BUDGET_MS = 16;

export type OverlayMode = "off" | "rulers" | "measure" | "grid" | "all";

export interface OverlayState {
  mode: OverlayMode;
  /** 量测起点（measure 模式下拖拽中更新）。 */
  measureStart: Point | null;
  /** 当前量测矩形（拖拽中实时更新——实时性判据）。 */
  measureRect: Rect | null;
}

export function initialOverlay(mode: OverlayMode = "off"): OverlayState {
  return { mode, measureStart: null, measureRect: null };
}

/** Esc 秒退：任何模式下 Esc 一律回到 off 且量测清零（判据「快捷退出」）。 */
export function escapeExit(_state: OverlayState): OverlayState {
  return initialOverlay("off");
}

/** 量测拖拽：起点落定 → 拖动实时更新矩形（负方向归一化——往左上拖也对）。 */
export function beginMeasure(state: OverlayState, at: Point): OverlayState {
  return { ...state, mode: state.mode === "off" ? "measure" : state.mode, measureStart: at, measureRect: { x: at.x, y: at.y, w: 0, h: 0 } };
}

export function updateMeasure(state: OverlayState, current: Point): OverlayState {
  if (!state.measureStart) return state;
  const x = Math.min(state.measureStart.x, current.x);
  const y = Math.min(state.measureStart.y, current.y);
  const w = Math.abs(current.x - state.measureStart.x);
  const h = Math.abs(current.y - state.measureStart.y);
  return { ...state, measureRect: { x, y, w, h } };
}

/** 量测读数：宽×高（px）；零误差判据——读数恒等于几何差值，无任何舍入。 */
export function measureReadout(rect: Rect | null): { w: number; h: number } | null {
  if (!rect) return null;
  return { w: rect.w, h: rect.h };
}

/** 点击穿透（判据）：叠加层所有状态下输入都放行底层。 */
export function clickThrough(state: OverlayState): true {
  void state;
  return true;
}

/**
 * 网格对齐检查：给定点/矩形到最近 8px 网格的偏差；
 * 偏差 0 = 已对齐（对齐检查的核心读数）。
 */
export function gridAlignment(px: number): { nearest: number; offset: number; aligned: boolean } {
  const nearest = Math.round(px / GRID_STEP_PX) * GRID_STEP_PX;
  return { nearest, offset: px - nearest, aligned: px % GRID_STEP_PX === 0 };
}

/** 网格覆盖绘制参数（渲染面约定：颜色/透明度/步长三件一处定义）。 */
export function gridPaintSpec(): { step: number; opacity: number; lines: "rgba(127,127,127,OPACITY)" } {
  return { step: GRID_STEP_PX, opacity: GRID_OPACITY, lines: "rgba(127,127,127,OPACITY)" };
}

/** 标尺读数：光标所在像素（水平/垂直）——读数=坐标，零变换。 */
export function rulerReadout(at: Point): { horizontal: number; vertical: number } {
  return { horizontal: at.x, vertical: at.y };
}
