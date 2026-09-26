/**
 * 视图三件（AI-U1 · F429 F11 全屏 / F430 Ctrl+滚轮缩放 / F432 列表翻页）。
 *
 * 判据唯一源（主册摘文，与 kernel/varix/src/uni1/ v1 同参数——内核
 * zoomwheel 修复版语义为准）：
 * - F429「全屏渲染边界（零边缝）；顶缘任务栏滑出与自动收回；退出双键
 *   （F11/Esc）；提示一次性；进入/退出动画（F124 强调档 200ms）」。
 * - F430「三场景缩放用例；五档图标切换阈值；锚点缩放准确性（放大后鼠标
 *   下的内容仍在鼠标下）；边界行为；记忆联动」。
 * - F432「相对位置保持判据；四键行为；扩选组合；万项列表翻页帧就绪；
 *   滚动条同步」。
 */

import { u1Store } from "./u1store";

/* ------------------------------- F429 F11 全屏 ------------------------------- */

/** 进入/退出动画档（F124 强调档 200ms）。 */
export const FULLSCREEN_ANIM_MS = 200;

/** 顶缘滑出：光标距顶 ≤2px 持续 300ms → 任务栏滑出；移开 3s 自动收回。 */
export const TOP_EDGE_PX = 2;
export const TOP_EDGE_HOLD_MS = 300;
export const TOP_EDGE_RETRACT_MS = 3000;

export type TopEdgeState = "hidden" | "peeking" | "shown";

export function topEdgeTick(cursorY: number, holdMs: number, state: TopEdgeState, sinceShownMs: number): TopEdgeState {
  if (state === "shown" && sinceShownMs >= TOP_EDGE_RETRACT_MS) return "hidden";
  if (cursorY <= TOP_EDGE_PX && holdMs >= TOP_EDGE_HOLD_MS) return "shown";
  if (cursorY <= TOP_EDGE_PX) return "peeking";
  return "hidden";
}

/** 退出双键：F11 或 Esc（提示一次性——一次性提示账由调用方持有）。 */
export function fullscreenExitKey(key: string): boolean {
  return key === "F11" || key === "Escape";
}

/* ------------------------------- F430 Ctrl+滚轮缩放 ------------------------------- */

/** 图标五档（与内核 ICON_MODES 同表）。 */
export const ICON_MODES = ["超大图标", "大图标", "中图标", "小图标", "列表"] as const;

/** 缩放参数（默认 = 内核常量；u1Store 可视但边界钳制同内核）。 */
export const ZOOM_MIN = 100;
export const ZOOM_MAX = 8000;
export const ZOOM_STEP = 250;

/** 档位步进（与内核 StepZoom 同语义：+1 向大档=索引减；边界停住）。 */
export function iconModeStep(mode: number, dir: number, levels = ICON_MODES.length): number | null {
  const next = mode - dir;
  if (next < 0 || next >= levels) return null;
  return next;
}

export interface ZoomState { permille: number; offset: { x: number; y: number }; bounceHints: number }

/**
 * 锚点缩放（内核修复版语义）：以鼠标点为不动点；
 * new_offset = mouse - (mouse - offset) * new/old。
 * 边界：越界请求记微弹提示，末段半步贴边钳到边界值；已贴边再越界停住。
 */
export function anchorZoom(s: ZoomState, mouse: { x: number; y: number }, dir: number): ZoomState & { applied: boolean } {
  const old = s.permille;
  const raw = old + dir * ZOOM_STEP;
  let next = raw;
  let applied = true;
  if (raw < ZOOM_MIN || raw > ZOOM_MAX) {
    s.bounceHints += 1;
    next = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, raw));
    if (next === old) applied = false;
  }
  if (applied) {
    s.permille = next;
    s.offset = {
      x: mouse.x - Math.round(((mouse.x - s.offset.x) * next) / old),
      y: mouse.y - Math.round(((mouse.y - s.offset.y) * next) / old),
    };
  }
  return { ...s, applied };
}

/** 锚点不变量：缩放前后鼠标点下的内容坐标不变（内核判据同式——除以倍率）。 */
export function contentUnderMouse(s: ZoomState, mouse: { x: number; y: number }): { x: number; y: number } {
  return {
    x: Math.round(((mouse.x - s.offset.x) * 1000) / s.permille),
    y: Math.round(((mouse.y - s.offset.y) * 1000) / s.permille),
  };
}

/* ------------------------------- F432 列表翻页 ------------------------------- */

export interface ListNavState { count: number; pageRows: number; selected: number; scrollTop: number }

/**
 * 相对位置保持翻页（内核修复版语义）：选中项随页同步移动——翻页前在屏上
 * 第 N 行，翻页后仍在第 N 行；两端到界才无动作。
 */
export function pageDown(s: ListNavState): boolean {
  const maxTop = Math.max(0, s.count - s.pageRows);
  const nextSel = Math.min(s.selected + s.pageRows, s.count - 1);
  const nextTop = Math.min(s.scrollTop + s.pageRows, maxTop);
  if (nextSel === s.selected && nextTop === s.scrollTop) return false;
  s.selected = nextSel;
  s.scrollTop = nextTop;
  syncScroll(s);
  return true;
}

export function pageUp(s: ListNavState): boolean {
  const nextSel = Math.max(0, s.selected - s.pageRows);
  const nextTop = Math.max(0, s.scrollTop - s.pageRows);
  if (nextSel === s.selected && nextTop === s.scrollTop) return false;
  s.selected = nextSel;
  s.scrollTop = nextTop;
  syncScroll(s);
  return true;
}

/** 滚动条同步：选中项必须在可视区内。 */
export function syncScroll(s: ListNavState): void {
  if (s.selected < s.scrollTop) s.scrollTop = s.selected;
  else if (s.selected >= s.scrollTop + s.pageRows) s.scrollTop = s.selected + 1 - s.pageRows;
}

/** 相对行号（相对位置保持判据的读数）。 */
export function relativeRow(s: ListNavState): number {
  return s.selected - Math.min(s.scrollTop, s.selected);
}

/** 万项翻页帧就绪：页首尾行在数据界内（虚拟化无白帧的结构性证据）。 */
export function pageReady(count: number, pageRows: number, pageIndex: number): boolean {
  const start = pageIndex * pageRows;
  return start < count && start + pageRows <= count;
}

/* ------------------------------- 面板读数 ------------------------------- */

export function zoomStepPref(): number {
  const cfg = u1Store.get<{ zoomStep?: number }>("viewFx");
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, cfg.zoomStep ?? ZOOM_STEP));
}
