/**
 * V-30 拖拽中断与回弹（化境 · AI-2 窗口与键位路）：
 * VWM 窗口拖拽进行中按 Esc = 取消本次拖拽，窗口回弹到拖拽起点（150ms，
 * 复用 spring 令牌由 CSS 层承担），无任何残留状态。
 * - Esc 只在拖拽进行中生效（调用方在拖拽分支先于全局 Esc 语义消费按键）；
 * - 与贴靠预览共存：取消同时清预览（返回值携带 clearPreview 指令）；
 * - 不做「拖拽中右键取消」；不做多级撤销。
 */

import type { VwmRect } from "./vwm";

/** 回弹时长（ms）——CSS transition 由调用方套 spring 令牌。 */
export const DRAG_CANCEL_MS = 150;

export interface DragSession {
  winId: string;
  /** 拖拽起点几何（Esc 回弹目标）。 */
  start: VwmRect;
}

let session: DragSession | null = null;

/** 拖拽开始（记录起点）。 */
export function dragBegin(winId: string, start: VwmRect): void {
  session = { winId, start: { ...start } };
}

/** 拖拽正常结束（落位/贴靠）：清会话，Esc 不再生效。 */
export function dragSettle(): void {
  session = null;
}

export function dragActive(): DragSession | null {
  return session;
}

export interface DragCancelResult {
  winId: string;
  /** 回弹目标（= 拖拽起点）。 */
  rect: VwmRect;
  /** 同时清贴靠预览。 */
  clearPreview: true;
}

/**
 * Esc 取消：返回回弹计划；无进行中拖拽返回 null（Esc 语义继续向上传递，
 * 不影响 M-34 Esc 层级）。
 */
export function dragCancelByEsc(): DragCancelResult | null {
  const s = session;
  if (!s) return null;
  session = null;
  return { winId: s.winId, rect: { ...s.start }, clearPreview: true };
}
