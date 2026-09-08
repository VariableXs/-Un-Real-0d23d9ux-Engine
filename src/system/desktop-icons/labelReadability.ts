/**
 * V-05 桌面图标标签可读性：按壁纸亮度自动选择标签字色。
 * - light = 白字（暗底）/ dark = 黑字（亮底）；仅 image 壁纸模式参与采样，
 *   其余模式 auto → 维持现状默认（白字+暗晕，由现有 CSS 提供）。
 * - 采样：壁纸图缩到 ~64px 宽画布，对每个图标标签落点取邻域平均亮度；
 *   亮度阈值跟随感知亮度（Rec.601 YIQ）。
 * - 用户三选 auto/black/white 存 localStorage（variable:desktop:labelShade）。
 * - 高对比度语义：theme === "high-contrast" 时 HC 主题自身以 CSS token 保证
 *   对比度，本模块在其上强制白字（light）——与 HC「最大化可读」语义一致。
 * 诚实边界：视频/网页/着色器壁纸无法安全逐帧采样（跨域/性能），一律走默认样式。
 */
import type { WallpaperMode } from "../../lib/settings";

export type LabelShade = "light" | "dark";
export type ShadePref = "auto" | "black" | "white";

const LS_KEY = "variable:desktop:labelShade";

export const SHADE_PREFS: readonly ShadePref[] = ["auto", "black", "white"];

/** 亮度阈值（0-1）：高于此值视为亮底 → 黑字。 */
export const SHADE_THRESHOLD = 0.55;

/** 采样画布宽（px），高度等比缩放。 */
export const SAMPLE_WIDTH = 64;

/** 标签落点采样半径（采样画布像素）。 */
export const SAMPLE_RADIUS = 2;

export function loadShadePref(): ShadePref {
  try {
    const raw = localStorage.getItem(LS_KEY);
    return SHADE_PREFS.includes(raw as ShadePref) ? (raw as ShadePref) : "auto";
  } catch {
    return "auto";
  }
}

export function saveShadePref(p: ShadePref): void {
  try {
    localStorage.setItem(LS_KEY, p);
  } catch {
    /* storage blocked → 不持久化 */
  }
}

/** 归一化感知亮度（Rec.601 YIQ），0 = 黑，1 = 白。 */
export function luminance(r: number, g: number, b: number): number {
  return (0.299 * r + 0.587 * g + 0.114 * b) / 255;
}

/**
 * 单点邻域平均亮度（data 为 RGBA 像素；x/y 越界时钳制在画布内）。
 * 独立成纯函数便于单测（不依赖真实 canvas）。
 */
export function sampleLuminance(
  data: Uint8ClampedArray,
  w: number,
  h: number,
  x: number,
  y: number,
  radius: number = SAMPLE_RADIUS,
): number {
  const cx = Math.min(w - 1, Math.max(0, Math.round(x)));
  const cy = Math.min(h - 1, Math.max(0, Math.round(y)));
  let sum = 0;
  let n = 0;
  for (let dy = -radius; dy <= radius; dy++) {
    const yy = Math.min(h - 1, Math.max(0, cy + dy));
    for (let dx = -radius; dx <= radius; dx++) {
      const xx = Math.min(w - 1, Math.max(0, cx + dx));
      const i = (yy * w + xx) * 4;
      const r = data[i];
      const g = data[i + 1];
      const b = data[i + 2];
      if (r === undefined || g === undefined || b === undefined) continue;
      sum += luminance(r, g, b);
      n++;
    }
  }
  return n === 0 ? 0 : sum / n;
}

/** 由亮度判字色：亮底 → 黑字（dark），暗底 → 白字（light）。 */
export function shadeForLuminance(lum: number, threshold: number = SHADE_THRESHOLD): LabelShade {
  return lum > threshold ? "dark" : "light";
}

export interface ShadePoint {
  id: string;
  /** 归一化坐标（0-1，相对壁纸/视口）。 */
  x: number;
  y: number;
}

export interface ShadeSample {
  data: Uint8ClampedArray;
  w: number;
  h: number;
}

/** 从已缩放的画布像素中批量取字色（与 canvas 解耦，纯函数可测）。 */
export function shadesFromPixels(
  sample: ShadeSample,
  points: ShadePoint[],
  threshold: number = SHADE_THRESHOLD,
): Record<string, LabelShade> {
  const out: Record<string, LabelShade> = {};
  for (const p of points) {
    out[p.id] = shadeForLuminance(
      sampleLuminance(sample.data, sample.w, sample.h, p.x * sample.w, p.y * sample.h),
      threshold,
    );
  }
  return out;
}

/**
 * 组件入口：img 为已加载完成的壁纸图。把图缩到 SAMPLE_WIDTH 宽后按点采样。
 * 非 DOM 环境（vitest/SSR）或画布不可用 → 返回 null（调用方回落默认样式）。
 */
export function computeLabelShades(
  img: HTMLImageElement,
  points: ShadePoint[],
): Record<string, LabelShade> | null {
  try {
    if (typeof document === "undefined" || !img.naturalWidth || !img.naturalHeight) return null;
    const w = SAMPLE_WIDTH;
    const h = Math.max(1, Math.round((img.naturalHeight / img.naturalWidth) * SAMPLE_WIDTH));
    const canvas = document.createElement("canvas");
    canvas.width = w;
    canvas.height = h;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    if (!ctx) return null;
    ctx.drawImage(img, 0, 0, w, h);
    const data = ctx.getImageData(0, 0, w, h).data;
    return shadesFromPixels({ data, w, h }, points);
  } catch {
    // 跨域污染等 drawImage/getImageData 异常 → 如实回落默认样式
    return null;
  }
}

/**
 * 组件侧最终决策：
 * - black/white → 用户强制；
 * - auto + image + 有采样 → 采样结果；
 * - auto + image + 无采样（加载失败等） → null（维持默认样式，不装懂）；
 * - auto + 非 image → null（现状默认：白字+暗晕）；
 * - 高对比度主题 → 强制白字（HC 语义：最大化可读，见文件头注释）。
 */
export function resolveShade(
  pref: ShadePref,
  mode: WallpaperMode,
  sampled: Record<string, LabelShade> | null,
  id: string,
  highContrast: boolean,
): LabelShade | null {
  if (pref === "black") return "dark";
  if (pref === "white") return "light";
  if (highContrast) return "light";
  if (mode !== "image") return null;
  return sampled?.[id] ?? null;
}