/**
 * AI-18 V-80 强调色对比度守护 — WCAG 对比度计算 + 近似达标色推荐。
 *
 * 口径：
 * - 本地即时计算（WCAG 2.x 相对亮度公式）；
 * - 阈值 4.5:1（正文 AA）；只建议不强制（「仍然使用」允许，用户主权）；
 * - 推荐色 = HSV 邻域搜索最近达标色（视觉邻近：明度距离权重更高）。
 */

export type Rgb = [number, number, number];

function parseHex(hex: string): Rgb | null {
  const m = /^#?([0-9a-fA-F]{6})$/.exec(hex.trim());
  const g = m?.[1];
  if (!g) return null;
  const n = parseInt(g, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

function toHex([r, g, b]: Rgb): string {
  const h = (v: number): string => Math.round(Math.min(255, Math.max(0, v))).toString(16).padStart(2, "0");
  return `#${h(r)}${h(g)}${h(b)}`;
}

function luminance([r, g, b]: Rgb): number {
  const lin = (v: number): number => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/** WCAG 对比度（无效输入 = null）。 */
export function contrastRatio(fg: string, bg: string): number | null {
  const f = parseHex(fg);
  const b = parseHex(bg);
  if (!f || !b) return null;
  const l1 = luminance(f);
  const l2 = luminance(b);
  const [hi, lo] = l1 >= l2 ? [l1, l2] : [l2, l1];
  return (hi + 0.05) / (lo + 0.05);
}

export const WCAG_AA = 4.5;

export interface GuardVerdict {
  /** 与文字色对比度（null = 输入无效）。 */
  ratioText: number | null;
  /** 与背景色对比度。 */
  ratioBg: number | null;
  /** 是否达标（AA 4.5:1）。 */
  pass: boolean;
}

/**
 * 判定强调色是否达标：
 * - 对文字色 ≥ 4.5:1（WCAG 2.x 正文 AA，强调色作为文字承载底色时）；
 * - 对背景色 ≥ 3:1（WCAG 1.4.11 非文本组件 AA，强调色作为界面元素置于背景上时）。
 * （文字为浅、背景为深时，任一色不可能同时对两者 4.5:1，故按用途分别定阈值。）
 */
export const WCAG_AA_UI = 3.0;

export function guardAccent(accent: string, text: string, bg: string): GuardVerdict {
  const ratioText = contrastRatio(accent, text);
  const ratioBg = contrastRatio(accent, bg);
  const pass = ratioText !== null && ratioBg !== null && ratioText >= WCAG_AA && ratioBg >= WCAG_AA_UI;
  return { ratioText, ratioBg, pass };
}

// ---- HSV 邻域搜索 ----

function rgbToHsv([r, g, b]: Rgb): Rgb {
  const rr = r / 255, gg = g / 255, bb = b / 255;
  const max = Math.max(rr, gg, bb), min = Math.min(rr, gg, bb);
  const d = max - min;
  let h = 0;
  if (d > 0) {
    if (max === rr) h = ((gg - bb) / d + (gg < bb ? 6 : 0)) * 60;
    else if (max === gg) h = ((bb - rr) / d + 2) * 60;
    else h = ((rr - gg) / d + 4) * 60;
  }
  return [h, max === 0 ? 0 : d / max, max];
}

function hsvToRgb([h, s, v]: Rgb): Rgb {
  const c = v * s;
  const hh = (((h % 360) + 360) % 360) / 60;
  const x = c * (1 - Math.abs((hh % 2) - 1));
  const [r1, g1, b1] =
    hh < 1 ? [c, x, 0] : hh < 2 ? [x, c, 0] : hh < 3 ? [0, c, x] : hh < 4 ? [0, x, c] : hh < 5 ? [x, 0, c] : [c, 0, x];
  const m = v - c;
  return [(r1 + m) * 255, (g1 + m) * 255, (b1 + m) * 255];
}

/**
 * 推荐 count 个视觉邻近的达标色（明度 ±0.45 / 饱和 ±0.15 邻域全搜索）。
 * 明度距离权重更高（邻近感主要来自明度）；HSV 相近候选去重。
 */
export function recommendAccents(accent: string, text: string, bg: string, count = 3): string[] {
  const base = parseHex(accent);
  if (!base) return [];
  const [h0, s0, v0] = rgbToHsv(base);
  const found: { hex: string; dist: number }[] = [];
  // 邻域：明度 ±(0.05..0.45) 步进、饱和 ±0.15 —— 共约 200 候选，全部本地即时
  for (let dv = -0.45; dv <= 0.45; dv += 0.05) {
    for (let ds = -0.15; ds <= 0.15; ds += 0.05) {
      const v = Math.max(0.1, Math.min(1, v0 + dv));
      const s = Math.max(0.05, Math.min(1, s0 + ds));
      const hex = toHex(hsvToRgb([h0, s, v]));
      if (hex.toLowerCase() === accent.toLowerCase()) continue;
      const g = guardAccent(hex, text, bg);
      if (!g.pass) continue;
      const dist = Math.abs(dv) * 2 + Math.abs(ds); // 明度距离权重更高（视觉邻近）
      found.push({ hex, dist });
    }
  }
  found.sort((a, b) => a.dist - b.dist);
  const out: string[] = [];
  for (const f of found) {
    if (out.length >= count) break;
    // 视觉邻近去重：与已选候选 HSV 明度差 <0.05 的跳过
    const [h1, s1, v1] = rgbToHsv(parseHex(f.hex) as Rgb);
    const dup = out.some((o) => {
      const [h2, s2, v2] = rgbToHsv(parseHex(o) as Rgb);
      return Math.abs(v1 - v2) < 0.05 && Math.abs(s1 - s2) < 0.06 && Math.abs(h1 - h2) < 5;
    });
    if (!dup) out.push(f.hex);
  }
  return out;
}
