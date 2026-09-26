/**
 * F168/F164/F237 联动深化 · 多屏与窗口几何（DPI 四档/贴边分屏/记忆恢复）。
 *
 * 主册判据延伸：
 * - F168「三选项独立生效」「自动隐藏五态」的窗口侧：贴边分屏落点、
 *   最小尺寸钳制（F214 联动）；
 * - F237「恢复精度 <1px」「显示器变更拉回」——记忆恢复的几何契约；
 * - 七章「多显示器、不同 DPI/缩放比例下布局都要成立」——DPI 四档
 *   （1:1/125%/150%/200%）适配计算。
 */

// ---------- DPI 四档 ----------

export type DpiTier = "100%" | "125%" | "150%" | "200%";

export const DPI_SCALES: Record<DpiTier, number> = { "100%": 1, "125%": 1.25, "150%": 1.5, "200%": 2 };

/** 逻辑 px → 物理 px（四舍五入到物理像素网格——4K 不糊的第一性）。 */
export function toPhysical(logicalPx: number, tier: DpiTier): number {
  const s = DPI_SCALES[tier];
  return Math.round(logicalPx * s);
}

/** UI 尺寸档适配：图标/间距在 150%+ 档位补足物理清晰度（等比不等于等清）。 */
export function adaptForDpi(sizePx: number, tier: DpiTier): { logical: number; physical: number; spriteTier: 1 | 2 } {
  const s = DPI_SCALES[tier];
  const physical = toPhysical(sizePx, tier);
  return { logical: sizePx, physical, spriteTier: physical > sizePx ? 2 : 1 };
}

// ---------- 贴边分屏（F080 同族手感的窗口侧落点） ----------

export interface ScreenRect {
  x: number;
  y: number;
  w: number;
  h: number;
  /** 工作区（扣任务栏）。 */
  workTop: number;
  workBottom: number;
}

export type SnapZone = "left" | "right" | "top-max" | "top-left" | "top-right" | "bottom-left" | "bottom-right" | "none";

/** 指针落点 → 贴边区（边距 8px 触发带——F080 八落点同源）。 */
export function snapZoneFor(px: number, py: number, screen: ScreenRect, edgePx = 8): SnapZone {
  const nearL = px <= screen.x + edgePx;
  const nearR = px >= screen.x + screen.w - edgePx;
  const nearT = py <= screen.workTop + edgePx;
  const nearB = py >= screen.workBottom - edgePx;
  if (nearT && nearL) return "top-left";
  if (nearT && nearR) return "top-right";
  if (nearT) return "top-max";
  if (nearB && nearL) return "bottom-left";
  if (nearB && nearR) return "bottom-right";
  if (nearL) return "left";
  if (nearR) return "right";
  return "none";
}

/** 贴边落点矩形（半屏/四分/最大化——工作区口径，任务栏永不遮）。 */
export function snapRect(zone: SnapZone, screen: ScreenRect): ScreenRect | null {
  const top = screen.workTop;
  const bottom = screen.workBottom;
  const h = bottom - top;
  const halfW = Math.floor(screen.w / 2);
  const halfH = Math.floor(h / 2);
  switch (zone) {
    case "left":
      return { ...screen, x: screen.x, y: top, w: halfW, h };
    case "right":
      return { ...screen, x: screen.x + screen.w - halfW, y: top, w: halfW, h };
    case "top-max":
      return { ...screen, y: top, h };
    case "top-left":
      return { ...screen, x: screen.x, y: top, w: halfW, h: halfH };
    case "top-right":
      return { ...screen, x: screen.x + screen.w - halfW, y: top, w: halfW, h: halfH };
    case "bottom-left":
      return { ...screen, x: screen.x, y: top + h - halfH, w: halfW, h: halfH };
    case "bottom-right":
      return { ...screen, x: screen.x + screen.w - halfW, y: top + h - halfH, w: halfW, h: halfH };
    case "none":
      return null;
  }
}

// ---------- 最小尺寸钳制与记忆恢复（F214/F237） ----------

export interface WindowMemory {
  id: string;
  rect: ScreenRect;
  dpiTier: DpiTier;
}

/** 最小尺寸钳制（三档降级：尺寸 → 标题栏只保 → 内容滚动）。 */
export function clampToMinimum(rect: { w: number; h: number }, min: { w: number; h: number }): { w: number; h: number; degraded: "none" | "compact" | "scroll" } {
  const w = Math.max(min.w, rect.w);
  const h = Math.max(min.h, rect.h);
  const degraded = w > rect.w + 40 || h > rect.h + 40 ? "scroll" : w > rect.w || h > rect.h ? "compact" : "none";
  return { w, h, degraded };
}

/** 显示器热切换拉回：记忆矩形超出新屏 → 夹回可视区（精度 <1px 的恢复契约）。 */
export function pullBackToScreen(mem: WindowMemory, newScreen: ScreenRect): { rect: ScreenRect; adjusted: boolean; note: string } {
  const maxX = newScreen.x + newScreen.w - Math.min(200, mem.rect.w);
  const maxY = newScreen.workBottom - 80;
  const x = Math.max(newScreen.x, Math.min(maxX, mem.rect.x));
  const y = Math.max(newScreen.workTop, Math.min(maxY, mem.rect.y));
  const adjusted = x !== mem.rect.x || y !== mem.rect.y;
  return {
    rect: { ...mem.rect, x, y },
    adjusted,
    note: adjusted ? `显示器变更——窗口从 (${mem.rect.x},${mem.rect.y}) 拉回 (${x},${y})（可视区钳制，尺寸不变）` : "显示器布局未变——记忆位置精确恢复",
  };
}

/** DPI 变更时的窗口重排：逻辑尺寸不变、物理随档（WM_DPICHANGED 无闪烁口径）。 */
export function rescaleForDpi(mem: WindowMemory, newTier: DpiTier): { rect: ScreenRect; note: string } {
  if (mem.dpiTier === newTier) return { rect: mem.rect, note: "DPI 档位未变" };
  const oldS = DPI_SCALES[mem.dpiTier];
  const newS = DPI_SCALES[newTier];
  const ratio = newS / oldS;
  return {
    rect: { ...mem.rect, w: Math.round(mem.rect.w * ratio), h: Math.round(mem.rect.h * ratio) },
    note: `DPI ${mem.dpiTier} → ${newTier}：逻辑内容不变，物理尺寸 ×${ratio.toFixed(2)}（重排一次完成，无逐帧闪烁）`,
  };
}
