/**
 * V-06 桌面图标密度：四档（32/48/64/96）+ Ctrl+滚轮连续缩放（24–128px，步进 4）。
 * - px 持久化在 localStorage（variable:desktop:iconPx），档位/连续值共用一个真源；
 * - tier 几何按现有三档（32/48/64）比例线性插值，96 档按 48→64 斜率外推；
 * - 诚实边界：settings.iconSize 类型为 32|48|64 且 src/lib/settings.ts 禁改，
 *   96 档与连续缩放值仅 localStorage 记忆，选择 32–64 时同步回写设置。
 */
import type { IconSize } from "../../lib/settings";

/** 桌面档位（本车道扩展类型；IconSize 是设置页的 32|48|64）。 */
export type DesktopIconSize = IconSize | 96;

export interface IconTier {
  w: number;
  h: number;
  tile: number;
  icon: number;
}

/** 四档几何（32/48/64 与历史 SIZE_TIERS 逐值一致；96 为线性外推）。 */
export const SIZE_TIERS: Record<DesktopIconSize, IconTier> = {
  32: { w: 84, h: 98, tile: 44, icon: 30 },
  48: { w: 100, h: 114, tile: 58, icon: 46 },
  64: { w: 118, h: 132, tile: 76, icon: 62 },
  96: { w: 154, h: 168, tile: 112, icon: 94 },
};

export const ICON_PX_MIN = 24;
export const ICON_PX_MAX = 128;
export const ICON_PX_STEP = 4;

const LS_KEY = "variable:desktop:iconPx";

export function clampIconPx(px: number): number {
  return Math.min(ICON_PX_MAX, Math.max(ICON_PX_MIN, px));
}

export function loadIconPx(): number | null {
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (raw === null) return null;
    const n = Number(raw);
    return Number.isFinite(n) ? clampIconPx(n) : null;
  } catch {
    return null;
  }
}

export function saveIconPx(px: number): void {
  try {
    localStorage.setItem(LS_KEY, String(clampIconPx(Math.round(px))));
  } catch {
    /* storage blocked → 不持久化 */
  }
}

/** 锚点（按 px 升序），段内线性插值；端点外沿最近段斜率延伸后钳制。 */
const ANCHORS: { px: number; tier: IconTier }[] = [
  { px: 32, tier: SIZE_TIERS[32] },
  { px: 48, tier: SIZE_TIERS[48] },
  { px: 64, tier: SIZE_TIERS[64] },
  { px: 96, tier: SIZE_TIERS[96] },
];

/** 任意 px（24–128）→ 图标几何：三段线性插值，四舍五入到整像素。 */
export function tierForPx(px: number): IconTier {
  const v = clampIconPx(px);
  if (v <= ANCHORS[0]!.px) {
    // 下端延伸：以 32 档为基准按 px/32 缩放
    const k = v / ANCHORS[0]!.px;
    const t = ANCHORS[0]!.tier;
    return { w: Math.round(t.w * k), h: Math.round(t.h * k), tile: Math.round(t.tile * k), icon: Math.round(t.icon * k) };
  }
  const last = ANCHORS[ANCHORS.length - 1]!;
  if (v >= last.px) {
    // 上端延伸（96→128）：沿 64→96 段斜率外推
    const a = ANCHORS[ANCHORS.length - 2]!;
    const b = last;
    const k = (v - a.px) / (b.px - a.px);
    return {
      w: Math.round(a.tier.w + (b.tier.w - a.tier.w) * k),
      h: Math.round(a.tier.h + (b.tier.h - a.tier.h) * k),
      tile: Math.round(a.tier.tile + (b.tier.tile - a.tier.tile) * k),
      icon: Math.round(a.tier.icon + (b.tier.icon - a.tier.icon) * k),
    };
  }
  for (let i = 0; i < ANCHORS.length - 1; i++) {
    const a = ANCHORS[i]!;
    const b = ANCHORS[i + 1]!;
    if (v >= a.px && v <= b.px) {
      const k = (v - a.px) / (b.px - a.px);
      return {
        w: Math.round(a.tier.w + (b.tier.w - a.tier.w) * k),
        h: Math.round(a.tier.h + (b.tier.h - a.tier.h) * k),
        tile: Math.round(a.tier.tile + (b.tier.tile - a.tier.tile) * k),
        icon: Math.round(a.tier.icon + (b.tier.icon - a.tier.icon) * k),
      };
    }
  }
  return SIZE_TIERS[48];
}

/**
 * 动效偏好：设置页 reduceMotion（App.tsx 写入 html[data-reduce-motion]）
 * 或系统级 prefers-reduced-motion 任一命中即视为需要降低动效。
 */
export function prefersReduceMotion(): boolean {
  try {
    if (typeof document !== "undefined" && document.documentElement.dataset.reduceMotion === "true") return true;
  } catch {
    /* non-DOM 环境 */
  }
  try {
    return typeof window !== "undefined" && typeof window.matchMedia === "function"
      ? window.matchMedia("(prefers-reduced-motion: reduce)").matches
      : false;
  } catch {
    return false;
  }
}