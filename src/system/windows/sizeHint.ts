/**
 * V-23 调整大小实时几何提示（化境 · AI-2 窗口与键位路）：
 * 拖动窗口边/角时，光标旁显示实时 W×H（等宽字体、80ms 跟随由 CSS transition 承担）；
 * Shift 按住 = 等比缩放（锁定拖动起点比例）。
 * - 数值显示可关（默认开，视觉极轻）；不做数值输入框；不做预设尺寸吸附。
 */

export interface SizeHint {
  w: number;
  h: number;
  /** 等比缩放生效（Shift 按住）。 */
  aspect: boolean;
}

/** 格式化提示文本（等宽场景展示 `1280×720`）。 */
export function formatSizeHint(w: number, h: number): string {
  return `${Math.round(w)}×${Math.round(h)}`;
}

/**
 * 等比缩放：以起点宽高比锁定，由新的主维度推导另一维度。
 * ratio = 起点 w/h；给定拖动后的 w → h = w / ratio（误差 <0.5% 由整数取整承担）。
 */
export function aspectSize(startW: number, startH: number, newW: number, minH = 280): { w: number; h: number } {
  const ratio = startW / startH;
  const w = Math.max(1, Math.round(newW));
  const h = Math.max(minH, Math.round(w / ratio));
  return { w, h };
}

/** 计算提示：拖动中（newW/newH）+ Shift 等比 → 返回应显示的宽高。 */
export function sizeHintFor(
  start: { w: number; h: number },
  current: { w: number; h: number },
  shiftHeld: boolean,
): SizeHint {
  if (shiftHeld) {
    const { w, h } = aspectSize(start.w, start.h, current.w);
    return { w, h, aspect: true };
  }
  return { w: current.w, h: current.h, aspect: false };
}
