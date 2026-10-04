/**
 * F165 开机动画个性化 · 完整设计。
 *
 * 主册判据：改色后开机录屏与预览一致；三档密度实际帧资产数对拍；结构时长零变化
 * （8.0s±0.2s）。
 *
 * 【功能定义】四幕动画（C-2）可定制项：配色（星徽主色/粒子色随 E1 强调色）/
 * 粒子密度三档/幕三「点亮」过渡底图（可选用户壁纸暗化版）；结构与时长不可改
 * （8 秒线是性能契约）——标准不可改，皮肤可以。
 *
 * 【状态与异常】资产烘焙失败 → 回退官方默认动画+报备；用户壁纸过暗/过亮 → 自动
 * 压暗/提亮至可读区间（说明条）；密度档与低端机自动匹配（F060 弱机降档建议）。
 *
 * 【设计细节】粒子色=强调色令牌映射表（主色→粒子基色/辅色→高光）；密度三档粒子
 * 数（1200/600/200）；过渡底图暗化 65% 固定（保证幕四交接可读）；烘帧在关机时
 * 安装更新同窗口执行；跳过键（Esc）语义不受定制影响。
 */

import { BOOT_DURATION_MS, personaStore, withinBootDuration } from "./store";

export const SECTION = "boot";

export type ParticleDensity = "dense" | "standard" | "minimal";

export const PARTICLE_COUNTS: Record<ParticleDensity, number> = { dense: 1200, standard: 600, minimal: 200 };
export const BACKDROP_DIM = 0.65; // 暗化 65% 固定
export const SKIP_KEY = "Escape"; // 跳过键语义不受定制影响

export interface BootSkinConfig {
  /** 强调色覆盖（hex）；null=随 E1 强调色令牌。 */
  accentOverride: string | null;
  density: ParticleDensity;
  /** 幕三过渡底图（用户壁纸暗化版）。 */
  useWallpaperBackdrop: boolean;
  wallpaperPath: string | null;
  /** 烘焙状态：pending/baked/failed（失败回退官方默认+报备）。 */
  bakeState: "pending" | "baked" | "failed";
}

export function defaultBootSkinConfig(): BootSkinConfig {
  return { accentOverride: null, density: "standard", useWallpaperBackdrop: false, wallpaperPath: null, bakeState: "pending" };
}

export function loadBootSkinConfig(): BootSkinConfig {
  const stored = personaStore.getWith(SECTION, "boot", undefined) as Partial<BootSkinConfig> | undefined;
  return { ...defaultBootSkinConfig(), ...(stored ?? {}) };
}

export function saveBootSkinConfig(c: BootSkinConfig): void {
  personaStore.set(SECTION, { boot: c });
}

const HEX_RE = /^#[0-9a-fA-F]{6}$/;

export interface BootSkinValidation {
  ok: boolean;
  reason: string;
}

export function validateBootSkinConfig(c: BootSkinConfig): BootSkinValidation {
  if (c.accentOverride !== null && !HEX_RE.test(c.accentOverride)) {
    return { ok: false, reason: "强调色须为 #rrggbb 格式" };
  }
  if (!(c.density in PARTICLE_COUNTS)) return { ok: false, reason: "密度档非法" };
  return { ok: true, reason: "校验通过" };
}

// ---------- 粒子色映射表（主色→粒子基色/辅色→高光） ----------

export interface ParticlePalette {
  base: string;
  highlight: string;
  /** 星徽主色（幕一/logo 着色）。 */
  emblem: string;
}

function hexToRgb(hex: string): [number, number, number] {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

function rgbToHex(r: number, g: number, b: number): string {
  const h = (v: number) => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, "0");
  return `#${h(r)}${h(g)}${h(b)}`;
}

/** 粒子色映射：基色=强调色 80% 亮度；高光=提亮 1.25 倍（纯函数，预览与真播同源）。 */
export function particlePalette(accentHex: string): ParticlePalette {
  const [r, g, b] = hexToRgb(accentHex);
  return {
    base: rgbToHex(r * 0.8, g * 0.8, b * 0.8),
    highlight: rgbToHex(r * 1.25, g * 1.25, b * 1.25),
    emblem: accentHex,
  };
}

// ---------- 三档密度帧资产对拍 ----------

export interface BakePlan {
  density: ParticleDensity;
  particleCount: number;
  /** 每粒子帧位姿记录数（四幕 8s × 60fps 采样=480 帧/粒）。 */
  framesPerParticle: number;
  /** 结构时长校验（8.0s±0.2s——不可改契约）。 */
  durationMs: number;
  durationOk: boolean;
}

export function bakePlan(density: ParticleDensity): BakePlan {
  return {
    density,
    particleCount: PARTICLE_COUNTS[density],
    framesPerParticle: 480,
    durationMs: BOOT_DURATION_MS,
    durationOk: withinBootDuration(BOOT_DURATION_MS),
  };
}

/** 三档密度资产规模对拍表（验收「三档密度实际帧资产数对拍」的机械口径）。 */
export function densityAuditTable(): BakePlan[] {
  return (Object.keys(PARTICLE_COUNTS) as ParticleDensity[]).map(bakePlan);
}

// ---------- 底图亮度治理（过暗/过亮自动压暗/提亮） ----------

/** 亮度 0..1（YIQ 近似）。可读区间 [0.12, 0.88]。 */
export function luminance(hex: string): number {
  const [r, g, b] = hexToRgb(hex.length === 7 ? hex : "#808080");
  return (0.299 * r + 0.587 * g + 0.114 * b) / 255;
}

export const READABLE_LUMA_MIN = 0.12;
export const READABLE_LUMA_MAX = 0.88;

export function normalizeBackdropLuma(hex: string): { hex: string; adjusted: boolean; note: string } {
  const l = luminance(hex);
  if (l < READABLE_LUMA_MIN) {
    return { hex: rgbToHex(...(scaleRgb(hex, 1 / Math.max(l, 0.02)) as [number, number, number])), adjusted: true, note: "壁纸过暗，已自动提亮至可读区间" };
  }
  if (l > READABLE_LUMA_MAX) {
    return { hex: rgbToHex(...(scaleRgb(hex, READABLE_LUMA_MAX / l) as [number, number, number])), adjusted: true, note: "壁纸过亮，已自动压暗至可读区间" };
  }
  return { hex, adjusted: false, note: "亮度在可读区间，未调整" };
}

function scaleRgb(hex: string, factor: number): [number, number, number] {
  const [r, g, b] = hexToRgb(hex);
  return [r * factor, g * factor, b * factor];
}

/** 弱机降档建议（F060 电量账本联动）：电量档低 → 建议极简档。 */
export function suggestDensity(batteryTier: "high" | "medium" | "low"): ParticleDensity | null {
  return batteryTier === "low" ? "minimal" : null;
}
