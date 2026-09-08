/**
 * V-29 窗口色带标记（化境 · AI-2 窗口与键位路）：
 * 按应用给 VWM 窗口标题栏顶部加 2px 色带（工作=蓝、通讯=绿、监控=橙），
 * 设置面板集中管理，默认关闭。
 * - 颜色走语义色令牌（accent/info/success/warning/danger——不做任意色，防视觉失控）；
 * - 色带不占标题栏布局高度（覆盖绘制，position:absolute）；
 * - 高对比度模式加粗至 3px（UI 层）；不做每窗口独立色；不做色带动效；
 * - 默认关闭时逐像素等于现状（调用方开关不渲染即零差异）。
 */

import type { CSSProperties } from "react";

export type BandColor = "accent" | "info" | "success" | "warning" | "danger";

/** 应用 → 色带映射（应用级粒度即止；键为 VwmApp 标识或 tp:<id>）。 */
export type BandMap = Record<string, BandColor>;

export const BAND_TOKEN: Record<BandColor, string> = {
  accent: "var(--accent, #4cc2ff)",
  info: "var(--info, #60cdff)",
  success: "var(--success, #6ccb5f)",
  warning: "var(--warning, #fcc66d)",
  danger: "var(--danger, #ff99a4)",
};

export const BAND_LABEL: Record<BandColor, string> = {
  accent: "强调",
  info: "工作（蓝）",
  success: "通讯（绿）",
  warning: "监控（橙）",
  danger: "警示（红）",
};

const KEY = "variable:vwm:colorband";

/** 色带总开关（默认关）。 */
export function colorBandEnabled(): boolean {
  try {
    return localStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}

export function setColorBandEnabled(v: boolean): void {
  try {
    localStorage.setItem(KEY, v ? "1" : "0");
  } catch {
    /* storage blocked */
  }
}

export function loadBandMap(): BandMap {
  try {
    const raw = JSON.parse(localStorage.getItem(`${KEY}:map`) ?? "{}") as BandMap;
    return raw && typeof raw === "object" ? raw : {};
  } catch {
    return {};
  }
}

export function saveBandMap(map: BandMap): void {
  try {
    localStorage.setItem(`${KEY}:map`, JSON.stringify(map));
  } catch {
    /* storage blocked */
  }
}

/** 查询某应用的色带（未映射或总开关关闭 → null = 不渲染，逐像素等于现状）。 */
export function bandFor(app: string, map: BandMap = loadBandMap(), enabled = colorBandEnabled()): BandColor | null {
  if (!enabled) return null;
  return map[app] ?? null;
}

/** 色带内联样式（覆盖绘制：absolute、高度不占布局；高对比度由 CSS 类加粗到 3px）。 */
export function bandStyle(color: BandColor, highContrast = false): CSSProperties {
  return {
    position: "absolute",
    top: 0,
    left: 0,
    right: 0,
    height: highContrast ? 3 : 2,
    background: BAND_TOKEN[color],
    pointerEvents: "none",
  };
}
