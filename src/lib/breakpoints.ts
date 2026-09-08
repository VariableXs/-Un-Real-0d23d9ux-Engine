/**
 * AI-17 · U-12 布局响应式重构（Responsive Reflow）
 * 环境窗口 1280 → 4K 连续自适应。
 * 断点三档：compact（<1440）/ standard（1440–2559）/ wide（≥2560）。
 * 大字模式：单一超大显示器 + 高缩放比时全局字号 +2 档、点击目标 ≥48px
 * （html[data-ui-scale="large"] 由 CSS 消费）。
 */

export type LayoutTier = "compact" | "standard" | "wide";

export interface ViewportInfo {
  width: number;
  height: number;
  dpr: number;
}

/** 按视口宽度判定布局档。 */
export function layoutTier(width: number): LayoutTier {
  if (width < 1440) return "compact";
  if (width < 2560) return "standard";
  return "wide";
}

/**
 * 大字模式判定：投影/外接 TV 场景（有效物理像素宽 ≥ 2560 且 DPR ≥ 1.5，
 * 或超宽 ≥ 3200）时建议进入。仅判定，不自动改布局——由设置消费。
 */
export function suggestLargeType(info: ViewportInfo): boolean {
  const effective = info.width / info.dpr; // CSS 像素
  if (info.width >= 3200 && info.dpr >= 1.25) return true;
  return effective >= 2560 && info.dpr >= 1.5;
}

/** 当前视口信息（SSR/测试安全：window 缺失或值无效时返回 1920 标准）。 */
export function viewportInfo(): ViewportInfo {
  if (typeof window === "undefined") return { width: 1920, height: 1080, dpr: 1 };
  const width = Number((window as { innerWidth?: unknown }).innerWidth);
  const height = Number((window as { innerHeight?: unknown }).innerHeight);
  const dpr = Number((window as { devicePixelRatio?: unknown }).devicePixelRatio);
  return {
    width: Number.isFinite(width) && width > 0 ? width : 1920,
    height: Number.isFinite(height) && height > 0 ? height : 1080,
    dpr: Number.isFinite(dpr) && dpr > 0 ? dpr : 1,
  };
}

/**
 * 把布局档写进 html dataset（data-layout-tier / data-ui-scale），
 * CSS 容器查询/选择器按档生效。返回反初始化函数。
 */
export function initResponsiveContext(): () => void {
  if (typeof window === "undefined") return () => {};
  const apply = (): void => {
    const info = viewportInfo();
    document.documentElement.dataset.layoutTier = layoutTier(info.width);
    document.documentElement.dataset.uiScale = suggestLargeType(info) ? "large" : "normal";
  };
  apply();
  window.addEventListener("resize", apply, { passive: true });
  return () => {
    window.removeEventListener("resize", apply);
  };
}
