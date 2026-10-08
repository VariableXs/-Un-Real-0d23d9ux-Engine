/**
 * F151/F154 调色板引擎深化 · 壁纸色彩提取 → 令牌表派生。
 *
 * 主册判据延伸：
 * - F154「构图保护 / 壁纸与主题和谐」——从壁纸像素做中位切分量化，提取
 *   主色簇与亮度分布，判「这张壁纸适不适合当前主题」；
 * - F151「第三方主题作者改 24 个颜色就能做出完整主题」——提供
 *   「从一张图派生整套令牌」的作者工具面（img→tokens 一键底稿）；
 * - 三铁律「随时可退」：派生表与默认表 diff 可见（theme-engine diff 共用口径）。
 * 全部纯函数——采样管线（canvas 读像素）由壳层喂入，引擎不碰 DOM。
 */

// ---------- 色彩基础（域内独立实现——不依赖 theme-engine 私有函数，一处一事实） ----------

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

export function hexToRgb(hex: string): Rgb | null {
  const m = /^#?([0-9a-fA-F]{6})$/.exec(hex.trim());
  if (!m) return null;
  const n = parseInt(m[1] ?? "000000", 16);
  return { r: (n >> 16) & 0xff, g: (n >> 8) & 0xff, b: n & 0xff };
}

export function rgbToHex(c: Rgb): string {
  const h = (v: number) => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, "0");
  return `#${h(c.r)}${h(c.g)}${h(c.b)}`;
}

/** 相对亮度（WCAG 口径——与 theme-studio/tokens 公式同源，此处为像素簇批量场景的内联实现）。 */
export function luminance(c: Rgb): number {
  const lin = (v: number) => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b);
}

export function rgbToHsl(c: Rgb): { h: number; s: number; l: number } {
  const r = c.r / 255;
  const g = c.g / 255;
  const b = c.b / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return { h: 0, s: 0, l };
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h: number;
  if (max === r) h = ((g - b) / d + (g < b ? 6 : 0)) / 6;
  else if (max === g) h = ((b - r) / d + 2) / 6;
  else h = ((r - g) / d + 4) / 6;
  return { h, s, l };
}

export function hslToRgb(h: number, s: number, l: number): Rgb {
  const hue2rgb = (p: number, q: number, t: number): number => {
    let x = t;
    if (x < 0) x += 1;
    if (x > 1) x -= 1;
    if (x < 1 / 6) return p + (q - p) * 6 * x;
    if (x < 1 / 2) return q;
    if (x < 2 / 3) return p + (q - p) * (2 / 3 - x) * 6;
    return p;
  };
  if (s === 0) {
    const v = Math.round(l * 255);
    return { r: v, g: v, b: v };
  }
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  return {
    r: Math.round(hue2rgb(p, q, h + 1 / 3) * 255),
    g: Math.round(hue2rgb(p, q, h) * 255),
    b: Math.round(hue2rgb(p, q, h - 1 / 3) * 255),
  };
}

// ---------- 中位切分量化（壁纸主色簇提取） ----------

export interface PaletteSwatch {
  hex: string;
  /** 簇内像素占比 0..1。 */
  population: number;
  luminance: number;
  hsl: { h: number; s: number; l: number };
}

export interface PaletteResult {
  swatches: PaletteSwatch[];
  /** 全图平均亮度 0..1。 */
  meanLuminance: number;
  /** 最亮簇与最暗簇的亮度比（>8 视为高对比壁纸）。 */
  contrastSpan: number;
  /** 提取耗时（供 F160 同源的诚实性能标注）。 */
  elapsedMs: number;
}

const QUANT_CHANNELS = 3;

function boxVolume(pixels: Rgb[]): number {
  let min = [255, 255, 255];
  let max = [0, 0, 0];
  for (const p of pixels) {
    const arr = [p.r, p.g, p.b];
    for (let i = 0; i < QUANT_CHANNELS; i++) {
      if (arr[i]! < min[i]!) min[i] = arr[i]!;
      if (arr[i]! > max[i]!) max[i] = arr[i]!;
    }
  }
  return (max[0]! - min[0]!) * (max[1]! - min[1]!) * (max[2]! - min[2]!);
}

function widestChannel(pixels: Rgb[]): number {
  let min = [255, 255, 255];
  let max = [0, 0, 0];
  for (const p of pixels) {
    const arr = [p.r, p.g, p.b];
    for (let i = 0; i < QUANT_CHANNELS; i++) {
      if (arr[i]! < min[i]!) min[i] = arr[i]!;
      if (arr[i]! > max[i]!) max[i] = arr[i]!;
    }
  }
  let best = 0;
  let bestSpan = -1;
  for (let i = 0; i < QUANT_CHANNELS; i++) {
    const span = max[i]! - min[i]!;
    if (span > bestSpan) {
      bestSpan = span;
      best = i;
    }
  }
  return best;
}

/**
 * 中位切分（median cut）：把像素递归切到 k 簇，每簇取均值色。
 * 预算保护：输入超过 maxSamples 先等距抽样（4K 全量 8.3M 像素不能硬算——
 * F160 同源纪律：引擎自身开销可预期）。
 */
export function medianCut(pixels: Rgb[], k: number, maxSamples = 24000): PaletteSwatch[] {
  if (pixels.length === 0 || k <= 0) return [];
  let sample = pixels;
  if (sample.length > maxSamples) {
    const stride = sample.length / maxSamples;
    sample = [];
    for (let i = 0; i < maxSamples; i++) sample.push(pixels[Math.floor(i * stride)]!);
  }
  let boxes: Rgb[][] = [sample];
  while (boxes.length < k) {
    // 每轮切「体积最大」的箱（中位切分标准变体——比固定顺序收敛更均匀）。
    let bi = -1;
    let bv = -1;
    for (let i = 0; i < boxes.length; i++) {
      const v = boxVolume(boxes[i]!);
      if (boxes[i]!.length > 1 && v > bv) {
        bv = v;
        bi = i;
      }
    }
    if (bi < 0) break; // 全部单色箱——已到自然簇数。
    const box = boxes[bi]!;
    const ch = widestChannel(box);
    const sorted = [...box].sort((a, b) => {
      const av = ch === 0 ? a.r : ch === 1 ? a.g : a.b;
      const bv2 = ch === 0 ? b.r : ch === 1 ? b.g : b.b;
      return av - bv2;
    });
    const mid = sorted.length >> 1;
    boxes = [...boxes.slice(0, bi), sorted.slice(0, mid), sorted.slice(mid), ...boxes.slice(bi + 1)];
  }
  const total = sample.length;
  const swatches: PaletteSwatch[] = [];
  for (const box of boxes) {
    if (box.length === 0) continue;
    let r = 0;
    let g = 0;
    let b = 0;
    for (const p of box) {
      r += p.r;
      g += p.g;
      b += p.b;
    }
    const mean: Rgb = { r: r / box.length, g: g / box.length, b: b / box.length };
    swatches.push({
      hex: rgbToHex(mean),
      population: box.length / total,
      luminance: luminance(mean),
      hsl: rgbToHsl(mean),
    });
  }
  return swatches.sort((a, b) => b.population - a.population);
}

/** 从 RGBA 像素数组（canvas getImageData 的 data）提取调色板。 */
export function extractPalette(rgba: Uint8ClampedArray | Uint8Array, width: number, height: number, k = 6, step = 4): PaletteResult {
  const t0 = typeof performance !== "undefined" ? performance.now() : Date.now();
  const pixels: Rgb[] = [];
  let lumSum = 0;
  let lumCount = 0;
  // step 采样：横向每 step 像素取 1（4K 宽 3840 / 4 ≈ 960 列 × 2160 行仍大，
  // 再乘纵向步进由调用方控制——引擎侧只保证给定数组的开销线性）。
  for (let y = 0; y < height; y += step) {
    const rowBase = y * width * 4; // y 已按 step 步进——行基址按真实行宽算（步进只作用于采样密度，不作用于寻址）。
    for (let x = 0; x < width; x += step) {
      const o = rowBase + x * 4;
      const a = rgba[o + 3] ?? 255;
      if (a < 128) continue; // 半透明像素不参与（合成后的观感才是壁纸观感）。
      const c: Rgb = { r: rgba[o] ?? 0, g: rgba[o + 1] ?? 0, b: rgba[o + 2] ?? 0 };
      pixels.push(c);
      lumSum += luminance(c);
      lumCount++;
    }
  }
  const swatches = medianCut(pixels, k);
  const meanLuminance = lumCount > 0 ? lumSum / lumCount : 0.5;
  let minL = 1;
  let maxL = 0;
  for (const s of swatches) {
    if (s.luminance < minL) minL = s.luminance;
    if (s.luminance > maxL) maxL = s.luminance;
  }
  const t1 = typeof performance !== "undefined" ? performance.now() : Date.now();
  return {
    swatches,
    meanLuminance,
    contrastSpan: swatches.length >= 2 ? (maxL + 0.01) / (minL + 0.01) : 1,
    elapsedMs: Math.max(0, t1 - t0),
  };
}

// ---------- 亮度判类（壁纸 × 深浅主题和谐判定） ----------

export type WallpaperTone = "dark" | "mid" | "light";

/** 壁纸亮度判类：深色主题配暗壁纸才和谐（F153 联动——错配时建议切换主题）。 */
export function classifyTone(meanLuminance: number): WallpaperTone {
  if (meanLuminance < 0.28) return "dark";
  if (meanLuminance > 0.62) return "light";
  return "mid";
}

export interface HarmonyVerdict {
  harmonious: boolean;
  tone: WallpaperTone;
  /** 人话结论（三要素：现状/原因/下一步——第十三章异常显性化同源）。 */
  message: string;
}

export function judgeWallpaperHarmony(meanLuminance: number, themeDark: boolean): HarmonyVerdict {
  const tone = classifyTone(meanLuminance);
  if (themeDark && tone === "dark") return { harmonious: true, tone, message: "壁纸偏暗 × 深色主题——和谐。" };
  if (!themeDark && tone === "light") return { harmonious: true, tone, message: "壁纸偏亮 × 浅色主题——和谐。" };
  if (tone === "mid") return { harmonious: true, tone, message: "壁纸为中调——深浅主题皆可。" };
  return {
    harmonious: false,
    tone,
    message: themeDark
      ? "壁纸偏亮 × 深色主题——任务栏文字对比度可能受损；建议换深色壁纸或切浅色主题。"
      : "壁纸偏暗 × 浅色主题——图标轮廓可能发闷；建议换亮色壁纸或切深色主题。",
  };
}

// ---------- 主色 → 令牌派生（img→tokens 作者工具面） ----------

/** 语义令牌键（与 tokens.ts 的 24 色键一致的子集——派生只动可安全派生的键）。 */
const DERIVABLE_KEYS = [
  "--p-bg-canvas",
  "--p-bg-surface",
  "--p-bg-raised",
  "--p-fg-primary",
  "--p-fg-secondary",
  "--p-accent",
  "--p-accent-soft",
  "--p-border-subtle",
  "--p-border-regular",
] as const;

export type DerivableKey = (typeof DERIVABLE_KEYS)[number];

export function derivableKeys(): readonly DerivableKey[] {
  return DERIVABLE_KEYS;
}

/** 从主色（HSL 空间）生成深色或浅色的三层面板——亮度锚点固定（保证可读性底线）。 */
export function surfaceFamilyFrom(base: { h: number; s: number }, dark: boolean): { canvas: Rgb; surface: Rgb; raised: Rgb } {
  if (dark) {
    return {
      canvas: hslToRgb(base.h, base.s * 0.35, 0.09),
      surface: hslToRgb(base.h, base.s * 0.32, 0.125),
      raised: hslToRgb(base.h, base.s * 0.3, 0.16),
    };
  }
  return {
    canvas: hslToRgb(base.h, base.s * 0.22, 0.96),
    surface: hslToRgb(base.h, base.s * 0.24, 0.92),
    raised: hslToRgb(base.h, base.s * 0.26, 0.88),
  };
}

/** 前景推导：底色亮度远离 0.5 的方向取对侧（WCAG 4.5:1 底线，theme-engine 自修可再细调）。 */
export function foregroundFor(bgLuminance: number, hue: number, sat: number): { primary: Rgb; secondary: Rgb } {
  const dark = bgLuminance < 0.5;
  return {
    primary: dark ? hslToRgb(hue, sat * 0.12, 0.93) : hslToRgb(hue, sat * 0.25, 0.16),
    secondary: dark ? hslToRgb(hue, sat * 0.1, 0.68) : hslToRgb(hue, sat * 0.18, 0.42),
  };
}

export interface DerivationResult {
  colors: Partial<Record<DerivableKey, string>>;
  /** 派生自哪个簇（索引——来源可溯）。 */
  sourceSwatch: number;
  /** 未派生键数（诚实覆盖度——派生不是全量，剩余走默认）。 */
  untouched: number;
}

/**
 * 从调色板派生令牌表底稿：
 * - 主色取「饱和度 × 占比」加权最高的彩色簇（占比高但灰的簇不适合当强调色）；
 * - 找不到彩色簇（黑白照片）时保留默认强调色（诚实：派生不了的不硬造）。
 */
export function deriveTokensFromPalette(palette: PaletteResult, dark: boolean): DerivationResult | null {
  if (palette.swatches.length === 0) return null;
  let bestIdx = -1;
  let bestScore = -1;
  palette.swatches.forEach((s, i) => {
    const score = s.hsl.s * 0.75 + s.population * 0.25;
    if (s.hsl.s > 0.08 && score > bestScore) {
      bestScore = score;
      bestIdx = i;
    }
  });
  if (bestIdx < 0) return null; // 全灰壁纸——强调色不硬造（诚实覆盖度）。
  const src = palette.swatches[bestIdx]!;
  const family = surfaceFamilyFrom({ h: src.hsl.h, s: src.hsl.s }, dark);
  const fg = foregroundFor(src.luminance, src.hsl.h, src.hsl.s);
  const accent = hslToRgb(src.hsl.h, Math.min(0.72, Math.max(0.42, src.hsl.s)), dark ? 0.64 : 0.46);
  const border = dark
    ? { subtle: hslToRgb(src.hsl.h, src.hsl.s * 0.2, 0.32), regular: hslToRgb(src.hsl.h, src.hsl.s * 0.2, 0.42) }
    : { subtle: hslToRgb(src.hsl.h, src.hsl.s * 0.2, 0.82), regular: hslToRgb(src.hsl.h, src.hsl.s * 0.2, 0.7) };
  return {
    colors: {
      "--p-bg-canvas": rgbToHex(family.canvas),
      "--p-bg-surface": rgbToHex(family.surface),
      "--p-bg-raised": rgbToHex(family.raised),
      "--p-fg-primary": rgbToHex(fg.primary),
      "--p-fg-secondary": rgbToHex(fg.secondary),
      "--p-accent": rgbToHex(accent),
      "--p-accent-soft": rgbToHex(hslToRgb(src.hsl.h, src.hsl.s * 0.6, dark ? 0.3 : 0.85)),
      "--p-border-subtle": rgbToHex(border.subtle),
      "--p-border-regular": rgbToHex(border.regular),
    },
    sourceSwatch: bestIdx,
    untouched: 24 - DERIVABLE_KEYS.length,
  };
}

// ---------- 色彩命名（可读性：调色板面板里每簇有人话名） ----------

const HUE_NAMES: readonly { maxH: number; zh: string; en: string }[] = [
  { maxH: 15, zh: "红", en: "red" },
  { maxH: 40, zh: "橙", en: "orange" },
  { maxH: 65, zh: "黄", en: "yellow" },
  { maxH: 95, zh: "黄绿", en: "chartreuse" },
  { maxH: 150, zh: "绿", en: "green" },
  { maxH: 185, zh: "青", en: "teal" },
  { maxH: 215, zh: "天蓝", en: "sky" },
  { maxH: 255, zh: "蓝", en: "blue" },
  { maxH: 290, zh: "紫", en: "violet" },
  { maxH: 330, zh: "品红", en: "magenta" },
  { maxH: 360, zh: "红", en: "red" },
];

export interface ColorName {
  hue: string;
  shade: "极暗" | "暗" | "中" | "亮" | "极亮";
  gray: boolean;
  label: string;
}

/** HSL → 人话名（调色板面板/导出注释用——创作工具的可读性底线）。 */
export function nameColor(hex: string, lang: "zh" | "en" = "zh"): ColorName | null {
  const rgb = hexToRgb(hex);
  if (!rgb) return null;
  const { h, s, l } = rgbToHsl(rgb);
  const gray = s < 0.08;
  const shade: ColorName["shade"] = l < 0.15 ? "极暗" : l < 0.35 ? "暗" : l < 0.65 ? "中" : l < 0.85 ? "亮" : "极亮";
  const shadeEn: Record<ColorName["shade"], string> = { 极暗: "near-black", 暗: "dark", 中: "mid", 亮: "light", 极亮: "near-white" };
  if (gray) {
    const label = lang === "zh" ? `${shade}灰` : `${shadeEn[shade]} gray`;
    return { hue: "灰", shade, gray: true, label };
  }
  const deg = h * 360;
  const entry = HUE_NAMES.find((e) => deg < e.maxH) ?? HUE_NAMES[HUE_NAMES.length - 1]!;
  const label = lang === "zh" ? `${shade}${entry.zh}` : `${shadeEn[shade]} ${entry.en}`;
  return { hue: entry.zh, shade, gray: false, label };
}
