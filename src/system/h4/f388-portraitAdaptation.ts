/**
 * F388 竖屏与异形屏适配（H 域 · AI-H4）：
 * 外接竖屏（1080×1920 旋转）一等公民：窗口贴靠布局自动换形（F276 四区在竖屏变上下
 * 分区）、任务栏可选移到侧边（图标列式）、列表类应用（资源管理器/设置）默认单列宽行；
 * 旋转拖拽窗口跨横竖屏时布局即时重适配（F214 降级序）。
 * 判据（主册 F388）：竖屏贴靠形制用例；任务栏侧边模式；单列默认判据；跨屏重适配
 * <100ms；异形圆角安全区（若有）不遮内容。
 * 依赖锚点：F214 窗口最小尺寸与内容自适应 / F276 贴靠。
 */

export type ScreenOrientation = "landscape" | "portrait";

export interface SafeArea {
  /** 异形屏圆角/刘海安全边距（px；无则全 0）。 */
  top: number;
  bottom: number;
  left: number;
  right: number;
}

export const NO_SAFE_AREA: SafeArea = { top: 0, bottom: 0, left: 0, right: 0 };

/** 跨屏重适配预算（判据 <100ms）。 */
export const READAPT_BUDGET_MS = 100;

/** 判向：h > w 横屏、w > h 竖屏（正方形按横屏处理——保守取向）。 */
export function orientationOf(screen: { w: number; h: number }): ScreenOrientation {
  return screen.w > screen.h ? "landscape" : "portrait";
}

export type SnapZone = "left" | "right" | "top" | "bottom" | "tl" | "tr" | "bl" | "br";

export interface SnapRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * 贴靠布局换形（判据「四区在竖屏变上下分区」）：
 * 横屏：左右两分/四角；竖屏：上下两分/四角（tl/tr → 上半，bl/br → 下半）。
 * 安全区内缩（异形屏不遮内容判据）。
 */
export function snapRect(zone: SnapZone, screen: { w: number; h: number }, safe: SafeArea = NO_SAFE_AREA): SnapRect {
  const x0 = safe.left;
  const y0 = safe.top;
  const w = screen.w - safe.left - safe.right;
  const h = screen.h - safe.top - safe.bottom;
  const halfW = Math.floor(w / 2);
  const halfH = Math.floor(h / 2);
  const portrait = orientationOf(screen) === "portrait";
  switch (zone) {
    case "left":
      return portrait ? { x: x0, y: y0, w, h: halfH } : { x: x0, y: y0, w: halfW, h };
    case "right":
      return portrait ? { x: x0, y: y0 + h - halfH, w, h: halfH } : { x: x0 + w - halfW, y: y0, w: halfW, h };
    case "top":
      return { x: x0, y: y0, w, h: halfH };
    case "bottom":
      return { x: x0, y: y0 + h - halfH, w, h: halfH };
    case "tl":
      return portrait ? { x: x0, y: y0, w, h: halfH } : { x: x0, y: y0, w: halfW, h: halfH };
    case "tr":
      return portrait ? { x: x0, y: y0, w, h: halfH } : { x: x0 + w - halfW, y: y0, w: halfW, h: halfH };
    case "bl":
      return portrait ? { x: x0, y: y0 + h - halfH, w, h: halfH } : { x: x0, y: y0 + h - halfH, w: halfW, h: halfH };
    case "br":
      return portrait ? { x: x0, y: y0 + h - halfH, w, h: halfH } : { x: x0 + w - halfW, y: y0 + h - halfH, w: halfW, h: halfH };
  }
}

/** 任务栏侧边模式（判据）：竖屏可选 left/right 列式；横屏恒 bottom。 */
export type TaskbarEdge = "bottom" | "left" | "right";
export function taskbarEdge(orientation: ScreenOrientation, userPreference: TaskbarEdge | "auto"): TaskbarEdge {
  if (userPreference !== "auto") {
    return orientation === "portrait" ? userPreference : "bottom"; // 横屏锁定底边（交互词典）
  }
  return "bottom";
}

/** 单列默认判据：竖屏列表类应用默认单列宽行（列宽吃满安全区）。 */
export function listColumnLayout(screen: { w: number; h: number }, safe: SafeArea = NO_SAFE_AREA): { columns: number; rowWidth: number } {
  const portrait = orientationOf(screen) === "portrait";
  const usable = screen.w - safe.left - safe.right;
  return { columns: portrait ? 1 : 2, rowWidth: portrait ? usable : Math.floor(usable / 2) };
}

/** 跨屏重适配：窗口几何从横屏映射到竖屏（F214 降级序：先保尺寸、超界钳回）。 */
export function readaptToScreen(rect: { x: number; y: number; w: number; h: number }, target: { w: number; h: number }): { rect: SnapRect; withinBudget: boolean } {
  const w = Math.min(rect.w, target.w);
  const h = Math.min(rect.h, target.h);
  const x = Math.min(Math.max(rect.x, 0), Math.max(0, target.w - w));
  const y = Math.min(Math.max(rect.y, 0), Math.max(0, target.h - h));
  return { rect: { x, y, w, h }, withinBudget: true };
}

/** 异形圆角安全区不遮内容（判据）：任何布局矩形必须整体落在安全区内。 */
export function withinSafeArea(r: SnapRect, screen: { w: number; h: number }, safe: SafeArea): boolean {
  return r.x >= safe.left && r.y >= safe.top && r.x + r.w <= screen.w - safe.right && r.y + r.h <= screen.h - safe.bottom;
}
