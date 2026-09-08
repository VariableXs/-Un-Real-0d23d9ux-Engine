/**
 * N-08 主题工坊 —— 语义 token 定义 + 对比度 / OKLCH 纯函数。
 *
 * token 清单以 src/design/tokens.css（A-1 单一事实源）真实存在的变量为准：
 * - 任务书里写的 --border 在现有体系中是 --stroke 的兼容别名
 *   （tokens.css L105: --border: var(--stroke)），工坊编辑正身 --stroke。
 * - --bg-surface / --bg-raised / --accent-soft / --stroke 是带 alpha 的半透明色：
 *   编辑器只改 RGB 分量、保留原 alpha（input[type=color] 只能输出 6 位 hex），
 *   对比度校验时按 WCAG 合成规则先叠加到 bg-canvas 再算（见 compositeOver）。
 * - vtheme 存储统一用 hex（#rrggbb / #rrggbbaa），OKLCH 仅作显示换算；
 *   换算用标准 OKLab 矩阵（Björn Ottosson），非近似库。
 */

export type VariantKind = "light" | "dark" | "hc";

export interface ColorTokenDef {
  key: string;
  zh: string;
  en: string;
  group: "bg" | "text" | "brand" | "state";
  /** 半透明 token：编辑时保留原 alpha；作为底色参与校验时先合成。 */
  translucent?: boolean;
}

export const COLOR_TOKENS: ColorTokenDef[] = [
  { key: "--bg-canvas", zh: "画布底色", en: "Canvas", group: "bg" },
  { key: "--bg-surface", zh: "面板底色", en: "Surface", group: "bg", translucent: true },
  { key: "--bg-raised", zh: "浮层底色", en: "Raised", group: "bg", translucent: true },
  { key: "--text-primary", zh: "主文字", en: "Text primary", group: "text" },
  { key: "--text-secondary", zh: "次要文字", en: "Text secondary", group: "text" },
  { key: "--accent", zh: "强调色", en: "Accent", group: "brand" },
  { key: "--accent-soft", zh: "强调色·弱", en: "Accent soft", group: "brand", translucent: true },
  { key: "--stroke", zh: "描边", en: "Stroke", group: "brand", translucent: true },
  { key: "--success", zh: "成功", en: "Success", group: "state" },
  { key: "--warn", zh: "警告", en: "Warn", group: "state" },
  { key: "--danger", zh: "危险", en: "Danger", group: "state" },
];

/** 圆角档：作用于 tokens.css 的 --r-window/card/control/chip。 */
export type RadiusKind = "sharp" | "soft" | "round";
export const RADIUS_PRESETS: Record<RadiusKind, Record<string, number>> = {
  sharp: { "--r-window": 2, "--r-card": 2, "--r-control": 2, "--r-chip": 0 },
  soft: { "--r-window": 16, "--r-card": 12, "--r-control": 8, "--r-chip": 4 },
  round: { "--r-window": 22, "--r-card": 18, "--r-control": 12, "--r-chip": 8 },
};
export const RADIUS_KINDS: RadiusKind[] = ["sharp", "soft", "round"];

/** 密度档：等比缩放 --sp-1..8（基准 4/8/12/16/24/32/48/64px）。 */
export type DensityKind = "compact" | "regular" | "relaxed";
export const DENSITY_FACTORS: Record<DensityKind, number> = { compact: 0.85, regular: 1, relaxed: 1.18 };
export const DENSITY_KINDS: DensityKind[] = ["compact", "regular", "relaxed"];
export const SP_BASE: Record<string, number> = {
  "--sp-1": 4, "--sp-2": 8, "--sp-3": 12, "--sp-4": 16,
  "--sp-5": 24, "--sp-6": 32, "--sp-7": 48, "--sp-8": 64,
};

/** 字阶档：等比缩放 --fs-12..32。 */
export type FontScaleKind = "small" | "standard" | "large";
export const FONT_SCALE_FACTORS: Record<FontScaleKind, number> = { small: 0.92, standard: 1, large: 1.1 };
export const FONT_SCALE_KINDS: FontScaleKind[] = ["small", "standard", "large"];
export const FS_BASE: Record<string, number> = {
  "--fs-12": 12, "--fs-13": 13, "--fs-15": 15, "--fs-17": 17,
  "--fs-20": 20, "--fs-24": 24, "--fs-32": 32,
};

// ---------- 颜色解析（#rrggbb / #rrggbbaa） ----------

export interface Rgba { r: number; g: number; b: number; a: number }

export function hexToRgba(hex: string): Rgba | null {
  const m = /^#([0-9a-fA-F]{6})([0-9a-fA-F]{2})?$/.exec(hex.trim());
  if (!m) return null;
  const int = parseInt(m[1] ?? "000000", 16);
  const aHex = m[2];
  return {
    r: (int >> 16) & 0xff,
    g: (int >> 8) & 0xff,
    b: int & 0xff,
    a: aHex ? parseInt(aHex, 16) / 255 : 1,
  };
}

export function rgbaToHex(c: Rgba): string {
  const h = (n: number) => Math.round(Math.min(255, Math.max(0, n))).toString(16).padStart(2, "0");
  const base = `#${h(c.r)}${h(c.g)}${h(c.b)}`;
  return c.a >= 1 ? base : `${base}${h(c.a * 255)}`;
}

/** WCAG 合成：半透明前景叠加到不透明底色上。 */
export function compositeOver(fg: string, bg: string): string | null {
  const f = hexToRgba(fg);
  const b = hexToRgba(bg);
  if (!f || !b) return null;
  const a = f.a;
  return rgbaToHex({
    r: f.r * a + b.r * (1 - a),
    g: f.g * a + b.g * (1 - a),
    b: f.b * a + b.b * (1 - a),
    a: 1,
  });
}

// ---------- WCAG 对比度（相对亮度，纯函数） ----------

function srgbToLinear(c: number): number {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
}

/** WCAG 相对亮度；无效 hex 返回 -1。 */
export function relativeLuminance(hex: string): number {
  const c = hexToRgba(hex);
  if (!c) return -1;
  return 0.2126 * srgbToLinear(c.r) + 0.7152 * srgbToLinear(c.g) + 0.0722 * srgbToLinear(c.b);
}

/**
 * WCAG 对比度（1..21）。任一色无效返回 0（必然不达标，编辑器显示红牌）。
 * 半透明色请先 compositeOver；本函数不做合成。
 */
export function contrastRatio(a: string, b: string): number {
  const la = relativeLuminance(a);
  const lb = relativeLuminance(b);
  if (la < 0 || lb < 0) return 0;
  const hi = Math.max(la, lb);
  const lo = Math.min(la, lb);
  return (hi + 0.05) / (lo + 0.05);
}

export const AA_TEXT_RATIO = 4.5;

export interface ContrastPairResult {
  id: string;
  fgKey: string;
  bgKey: string;
  ratio: number;
  ok: boolean;
}

/** 一个变体的对比度校验对（文本/状态色 vs 画布；raised 按 alpha 合成后另列一组）。 */
export function validateVariantContrast(colors: Record<string, string>): ContrastPairResult[] {
  const canvas = colors["--bg-canvas"] ?? "#000000";
  const fgPairs: [string, string][] = [
    ["--text-primary", "--bg-canvas"],
    ["--text-secondary", "--bg-canvas"],
    ["--accent", "--bg-canvas"],
    ["--success", "--bg-canvas"],
    ["--warn", "--bg-canvas"],
    ["--danger", "--bg-canvas"],
    ["--text-primary", "--bg-raised"],
  ];
  return fgPairs.map(([fgKey, bgKey]) => {
    const rawFg = colors[fgKey] ?? "";
    const rawBg = colors[bgKey] ?? "#000000";
    const fgA = hexToRgba(rawFg)?.a ?? 1;
    const bgA = hexToRgba(rawBg)?.a ?? 1;
    const fg = fgA < 1 ? compositeOver(rawFg, canvas) ?? rawFg : rawFg;
    const bg = bgA < 1 ? compositeOver(rawBg, canvas) ?? rawBg : rawBg;
    const ratio = contrastRatio(fg, bg);
    return { id: `${fgKey}|${bgKey}`, fgKey, bgKey, ratio, ok: ratio >= AA_TEXT_RATIO };
  });
}

// ---------- OKLCH ↔ hex（标准 OKLab 矩阵；gamut 外收敛 chroma） ----------

export interface Oklch { l: number; c: number; h: number }

export function hexToOklch(hex: string): Oklch | null {
  const c = hexToRgba(hex);
  if (!c) return null;
  const r = srgbToLinear(c.r), g = srgbToLinear(c.g), b = srgbToLinear(c.b);
  const l_ = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m_ = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s_ = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const L = 0.2104542553 * l_ + 0.793617785 * m_ - 0.0040720468 * s_;
  const A = 1.9779984951 * l_ - 2.428592205 * m_ + 0.4505937099 * s_;
  const B = 0.0259040371 * l_ + 0.7827717662 * m_ - 0.808675766 * s_;
  const C = Math.sqrt(A * A + B * B);
  let H = (Math.atan2(B, A) * 180) / Math.PI;
  if (H < 0) H += 360;
  return { l: L, c: C, h: H };
}

function oklabToLinearRgb(L: number, A: number, B: number): [number, number, number] {
  const l_ = L + 0.3963377774 * A + 0.2158037573 * B;
  const m_ = L - 0.1055613458 * A - 0.0638541728 * B;
  const s_ = L - 0.0894841775 * A - 1.291485548 * B;
  const l = l_ ** 3, m = m_ ** 3, s = s_ ** 3;
  return [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
}

const linearToSrgb = (x: number): number => {
  const v = x <= 0.0031308 ? x * 12.92 : 1.055 * Math.pow(x, 1 / 2.4) - 0.055;
  return Math.round(Math.min(1, Math.max(0, v)) * 255);
};

function inGamut(rgb: [number, number, number]): boolean {
  return rgb.every((v) => v >= -1e-4 && v <= 1 + 1e-4);
}

/** OKLCH → hex；超出 sRGB gamut 时按比例收敛 chroma（保持亮度/色相）。 */
export function oklchToHex(l: number, c: number, h: number): string {
  const rad = (h * Math.PI) / 180;
  const clamp01 = (x: number) => Math.min(1, Math.max(0, x));
  let chroma = Math.max(0, c);
  for (let i = 0; i < 24; i++) {
    const A = Math.cos(rad) * chroma;
    const B = Math.sin(rad) * chroma;
    const rgb = oklabToLinearRgb(clamp01(l), A, B);
    if (inGamut(rgb) || chroma === 0) {
      const [r, g, b] = rgb;
      return rgbaToHex({ r: linearToSrgb(r), g: linearToSrgb(g), b: linearToSrgb(b), a: 1 });
    }
    chroma *= 0.96;
  }
  return l >= 0.5 ? "#ffffff" : "#000000";
}

/** oklch() CSS 字符串（仅展示用；存储一律 hex）。 */
export function formatOklch(o: Oklch | null): string {
  if (!o) return "—";
  return `oklch(${o.l.toFixed(3)} ${o.c.toFixed(3)} ${o.h.toFixed(1)})`;
}

// ---------- 默认三变体（数值取自 tokens.css 三亮度层，OKLCH→hex 运行时换算） ----------

type OklchSeed = [l: number, c: number, h: number];

interface VariantSeed {
  colors: Record<string, OklchSeed>;
  alpha: Record<string, number>;
}

const SEEDS: Record<VariantKind, VariantSeed> = {
  dark: {
    colors: {
      "--bg-canvas": [0.14, 0.02, 262], "--bg-surface": [0.18, 0.025, 262], "--bg-raised": [0.2, 0.025, 262],
      "--text-primary": [0.9, 0.02, 262], "--text-secondary": [0.65, 0.03, 262],
      "--accent": [0.68, 0.09, 262], "--accent-soft": [0.68, 0.09, 262],
      "--success": [0.78, 0.12, 160], "--warn": [0.82, 0.13, 85], "--danger": [0.68, 0.15, 25],
      "--stroke": [0.7, 0.02, 262],
    },
    alpha: { "--bg-surface": 0.72, "--bg-raised": 0.92, "--accent-soft": 0.16, "--stroke": 0.14 },
  },
  light: {
    colors: {
      "--bg-canvas": [0.93, 0.02, 90], "--bg-surface": [0.97, 0.015, 90], "--bg-raised": [0.98, 0.01, 90],
      "--text-primary": [0.28, 0.02, 90], "--text-secondary": [0.52, 0.03, 90],
      "--accent": [0.52, 0.08, 75], "--accent-soft": [0.52, 0.08, 75],
      "--success": [0.78, 0.12, 160], "--warn": [0.82, 0.13, 85], "--danger": [0.68, 0.15, 25],
      "--stroke": [0.35, 0.02, 90],
    },
    alpha: { "--bg-surface": 0.86, "--bg-raised": 0.94, "--accent-soft": 0.14, "--stroke": 0.18 },
  },
  hc: {
    colors: {
      "--bg-canvas": [0, 0, 0], "--bg-surface": [0, 0, 0], "--bg-raised": [0, 0, 0],
      "--text-primary": [1, 0, 0], "--text-secondary": [0.85, 0, 0],
      "--accent": [0.87, 0.16, 95], "--accent-soft": [0.87, 0.16, 95],
      "--success": [0.82, 0.2, 150], "--warn": [0.85, 0.16, 90], "--danger": [0.66, 0.21, 25],
      "--stroke": [0.65, 0, 0],
    },
    alpha: { "--bg-surface": 0.94, "--bg-raised": 1, "--accent-soft": 0.22, "--stroke": 1 },
  },
};

export interface VthemeShape {
  radius: RadiusKind;
  density: DensityKind;
  fontScale: FontScaleKind;
}

export function defaultVariant(kind: VariantKind): { colors: Record<string, string>; shape: VthemeShape } {
  const seed = SEEDS[kind];
  const colors: Record<string, string> = {};
  for (const [key, [l, c, h]] of Object.entries(seed.colors)) {
    const hex = oklchToHex(l, c, h);
    const a = seed.alpha[key];
    colors[key] = a === undefined ? hex : `${hex}${Math.round(a * 255).toString(16).padStart(2, "0")}`;
  }
  return { colors, shape: { radius: "soft", density: "regular", fontScale: "standard" } };
}

// ---------- 变体信号（跟随现有主题设置：documentElement[data-theme]） ----------

/** ThemeId ∈ deep-space | paper | minimal-black | high-contrast | custom（settings.ts）。 */
export function detectVariant(themeId: string | undefined): VariantKind {
  if (themeId === "high-contrast") return "hc";
  if (themeId === "paper") return "light";
  return "dark";
}

export function currentVariant(): VariantKind {
  if (typeof document === "undefined") return "dark";
  return detectVariant(document.documentElement.dataset.theme);
}

// ---------- 应用 / 清除（documentElement 内联变量优先于 tokens.css 亮度层） ----------

const SHAPE_VAR_KEYS = [...Object.keys(RADIUS_PRESETS.soft), ...Object.keys(SP_BASE), ...Object.keys(FS_BASE)];

export function shapeVarValues(shape: VthemeShape): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [k, v] of Object.entries(RADIUS_PRESETS[shape.radius])) out[k] = `${v}px`;
  const df = DENSITY_FACTORS[shape.density];
  for (const [k, v] of Object.entries(SP_BASE)) out[k] = `calc(${v}px * ${df})`;
  const ff = FONT_SCALE_FACTORS[shape.fontScale];
  for (const [k, v] of Object.entries(FS_BASE)) out[k] = `calc(${v}px * ${ff})`;
  return out;
}

export function applyVariantToDOM(colors: Record<string, string>, shape: VthemeShape): void {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  for (const def of COLOR_TOKENS) {
    const v = colors[def.key];
    if (v) root.style.setProperty(def.key, v);
  }
  for (const [k, v] of Object.entries(shapeVarValues(shape))) root.style.setProperty(k, v);
}

/** 清除工坊写入的全部内联变量（还原内建主题层）。 */
export function clearVariantFromDOM(): void {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  for (const def of COLOR_TOKENS) root.style.removeProperty(def.key);
  for (const k of SHAPE_VAR_KEYS) root.style.removeProperty(k);
}
