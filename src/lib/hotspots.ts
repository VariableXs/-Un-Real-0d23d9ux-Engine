/**
 * AI-17 · Z-69 边缘热区自定义（Edge Hotspots）
 * 四角（单击/悬停可配）+ 顶边 + 左右边；默认仅左下角=开始菜单（最保守），
 * 其余热区未配置时完全无行为。悬停 300ms 防误触；全屏应用内全部禁用。
 */

export type EdgePosition = "tl" | "tr" | "bl" | "br" | "top" | "left" | "right";
export type HotspotAction = "none" | "start-menu" | "quick-panel" | "desktop" | "vwm";

export interface HotspotConfig {
  positions: Partial<Record<EdgePosition, HotspotAction>>;
  /** 悬停触发防误触 ms（默认 300） */
  hoverDwellMs: number;
  /** 热区命中像素宽（默认 6px） */
  hitPx: number;
  enabled: boolean;
}

export const DEFAULT_HOTSPOT_CONFIG: HotspotConfig = {
  positions: { bl: "start-menu" }, // 默认仅开始菜单左下角一处
  hoverDwellMs: 300,
  hitPx: 6,
  enabled: true,
};

const STORAGE_KEY = "vision.edgeHotspots.v1";

export function loadHotspotConfig(): HotspotConfig {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULT_HOTSPOT_CONFIG, positions: { ...DEFAULT_HOTSPOT_CONFIG.positions } };
    const parsed = JSON.parse(raw) as Partial<HotspotConfig>;
    return {
      positions: { ...parsed.positions },
      hoverDwellMs: parsed.hoverDwellMs ?? DEFAULT_HOTSPOT_CONFIG.hoverDwellMs,
      hitPx: parsed.hitPx ?? DEFAULT_HOTSPOT_CONFIG.hitPx,
      enabled: parsed.enabled ?? true,
    };
  } catch {
    return { ...DEFAULT_HOTSPOT_CONFIG, positions: { ...DEFAULT_HOTSPOT_CONFIG.positions } };
  }
}

export function saveHotspotConfig(cfg: HotspotConfig): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(cfg));
  } catch {
    /* 隐私模式等：静默 */
  }
}

export function resetHotspotConfig(): HotspotConfig {
  const cfg = { ...DEFAULT_HOTSPOT_CONFIG, positions: { ...DEFAULT_HOTSPOT_CONFIG.positions } };
  saveHotspotConfig(cfg);
  return cfg;
}

/** 命中判定：给定指针坐标与视口尺寸，返回命中的边缘位（不在任何热区返回 null）。 */
export function hitEdge(
  x: number,
  y: number,
  vw: number,
  vh: number,
  hitPx: number,
): EdgePosition | null {
  const inLeft = x <= hitPx;
  const inRight = x >= vw - hitPx;
  const inTop = y <= hitPx;
  const inBottom = y >= vh - hitPx;
  if (inTop && inLeft) return "tl";
  if (inTop && inRight) return "tr";
  if (inBottom && inLeft) return "bl";
  if (inBottom && inRight) return "br";
  if (inTop) return "top";
  if (inLeft) return "left";
  if (inRight) return "right";
  return null;
}

/** 全屏应用内热区全部禁用（联动 Z-18 全屏协议）。 */
export function isFullscreenActive(): boolean {
  if (typeof document === "undefined") return false;
  return Boolean(document.fullscreenElement);
}

/** 动作分发：返回真正有行为的位置（none 视为未配置——完全无响应）。 */
export function actionFor(cfg: HotspotConfig, edge: EdgePosition): HotspotAction {
  if (!cfg.enabled) return "none";
  return cfg.positions[edge] ?? "none";
}
