/**
 * Z-25 放大镜与取色器 —— 纯逻辑核心（可测试，零 DOM / 零 IPC 依赖）：
 * - HEX / RGB / HSL 三格式互转
 * - 像素网格绘制参数计算（≥8× 才叠加网格线；格宽 = 倍率）
 * - 取色历史管理（16 色上限，localStorage 键 variable:magnifier:history:v1）
 * - 放大源矩形计算（以光标物理坐标为中心，钳制在屏幕快照范围内）
 *
 * 红线：本文件不碰 IPC / DOM；抓屏与绘制在 MagnifierApp.tsx。
 * 不做截图标注、打印等与本任务无关的功能。
 */

// ---------- 颜色互转 ----------

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

/** HSL：h ∈ [0,360)，s / l ∈ [0,100]（百分比，一位小数）。 */
export interface Hsl {
  h: number;
  s: number;
  l: number;
}

function clampByte(n: number): number {
  return Math.min(255, Math.max(0, Math.round(n)));
}

/** "#RGB" / "#RRGGBB" / "RRGGBB" → RGB；非法输入返回 null（如实失败）。 */
export function hexToRgb(hex: string): Rgb | null {
  let s = hex.trim().replace(/^#/, "");
  if (/^[0-9a-fA-F]{3}$/.test(s)) s = s.split("").map((c) => c + c).join("");
  if (!/^[0-9a-fA-F]{6}$/.test(s)) return null;
  return {
    r: parseInt(s.slice(0, 2), 16),
    g: parseInt(s.slice(2, 4), 16),
    b: parseInt(s.slice(4, 6), 16),
  };
}

/** RGB → "#rrggbb"（小写）。 */
export function rgbToHex(c: Rgb): string {
  const h = (n: number): string => clampByte(n).toString(16).padStart(2, "0");
  return `#${h(c.r)}${h(c.g)}${h(c.b)}`;
}

/** RGB → HSL（标准公式；黑白灰 s=0）。 */
export function rgbToHsl(c: Rgb): Hsl {
  const r = clampByte(c.r) / 255;
  const g = clampByte(c.g) / 255;
  const b = clampByte(c.b) / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  let h = 0;
  let s = 0;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    if (max === r) h = (g - b) / d + (g < b ? 6 : 0);
    else if (max === g) h = (b - r) / d + 2;
    else h = (r - g) / d + 4;
    h *= 60;
  }
  const r1 = (n: number): number => Math.round(n * 10) / 10;
  return { h: r1(h), s: r1(s * 100), l: r1(l * 100) };
}

/** HSL → RGB（h 超过 360 取模；饱和度/亮度百分比钳制 0..100）。 */
export function hslToRgb(c: Hsl): Rgb {
  const h = ((c.h % 360) + 360) % 360;
  const s = Math.min(100, Math.max(0, c.s)) / 100;
  const l = Math.min(100, Math.max(0, c.l)) / 100;
  const chroma = (1 - Math.abs(2 * l - 1)) * s;
  const hp = h / 60;
  const x = chroma * (1 - Math.abs((hp % 2) - 1));
  let r = 0;
  let g = 0;
  let b = 0;
  if (hp < 1) [r, g, b] = [chroma, x, 0];
  else if (hp < 2) [r, g, b] = [x, chroma, 0];
  else if (hp < 3) [r, g, b] = [0, chroma, x];
  else if (hp < 4) [r, g, b] = [0, x, chroma];
  else if (hp < 5) [r, g, b] = [x, 0, chroma];
  else [r, g, b] = [chroma, 0, x];
  const m = l - chroma / 2;
  return { r: clampByte((r + m) * 255), g: clampByte((g + m) * 255), b: clampByte((b + m) * 255) };
}

// ---------- 放大镜参数 ----------

/** 倍率范围：2×–16×。 */
export const ZOOM_MIN = 2;
export const ZOOM_MAX = 16;

/** 钳制倍率（非法/越界 → 就近取整钳制）。 */
export function clampZoom(z: number): number {
  if (!Number.isFinite(z)) return ZOOM_MIN;
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(z)));
}

/** 像素网格绘制参数。 */
export interface MagnifierGrid {
  /** 是否叠加网格线（倍率 ≥ 8 才显示）。 */
  show: boolean;
  /** 每个源图像素放大后的格宽（= 倍率，画布像素）。 */
  step: number;
  /** 视口覆盖的源图像素列数。 */
  cols: number;
  /** 视口覆盖的源图像素行数。 */
  rows: number;
}

/** 网格绘制参数：viewW/viewH 为画布尺寸（CSS 像素 ≈ 物理像素）。 */
export function gridLayout(zoom: number, viewW: number, viewH: number): MagnifierGrid {
  const z = clampZoom(zoom);
  const cols = Math.max(1, Math.floor(Math.max(1, viewW) / z));
  const rows = Math.max(1, Math.floor(Math.max(1, viewH) / z));
  return { show: z >= 8, step: z, cols, rows };
}

/** 网格线坐标（画布像素；不显示网格或视口过小时返回空数组）。 */
export function gridLines(zoom: number, viewW: number, viewH: number): { vxs: number[]; hys: number[] } {
  const g = gridLayout(zoom, viewW, viewH);
  if (!g.show) return { vxs: [], hys: [] };
  const vxs: number[] = [];
  for (let i = 1; i < g.cols; i++) vxs.push(i * g.step);
  const hys: number[] = [];
  for (let i = 1; i < g.rows; i++) hys.push(i * g.step);
  return { vxs, hys };
}

/** 放大源矩形（屏幕快照坐标系）：以光标为中心的 sw×sh 区域，钳制在快照内。 */
export interface SourceRect {
  sx: number;
  sy: number;
  sw: number;
  sh: number;
}

export function sourceRect(
  cursor: { x: number; y: number },
  zoom: number,
  viewW: number,
  viewH: number,
  imgW: number,
  imgH: number,
): SourceRect {
  const z = clampZoom(zoom);
  const sw = Math.max(1, Math.ceil(Math.max(1, viewW) / z));
  const sh = Math.max(1, Math.ceil(Math.max(1, viewH) / z));
  let sx = Math.round(cursor.x - sw / 2);
  let sy = Math.round(cursor.y - sh / 2);
  if (imgW > 0) sx = Math.min(Math.max(sx, 0), Math.max(0, imgW - sw));
  if (imgH > 0) sy = Math.min(Math.max(sy, 0), Math.max(0, imgH - sh));
  return { sx, sy, sw, sh };
}

/** 从 ImageData.data 读取 (x, y) 像素；越界返回 null。 */
export function pickPixel(data: Uint8ClampedArray, w: number, x: number, y: number): Rgb | null {
  if (x < 0 || y < 0 || x >= w) return null;
  const i = (y * w + x) * 4;
  if (i < 0 || i + 2 >= data.length) return null;
  return { r: data[i] ?? 0, g: data[i + 1] ?? 0, b: data[i + 2] ?? 0 };
}

// ---------- 取色历史（16 色上限） ----------

export const HISTORY_KEY = "variable:magnifier:history:v1";
export const HISTORY_CAP = 16;

const HEX_RE = /^#[0-9a-fA-F]{6}$/;

/** 解析历史 JSON（坏数据 → 空表，绝不抛错）。 */
export function parseHistory(raw: string | null): string[] {
  if (!raw) return [];
  try {
    const v = JSON.parse(raw) as unknown;
    if (!Array.isArray(v)) return [];
    return v.filter((x): x is string => typeof x === "string" && HEX_RE.test(x));
  } catch {
    return [];
  }
}

/** 纯函数：新取色置顶；重复色去重上移；超过 16 色淘汰最旧（尾部）。 */
export function addColorHistory(list: string[], hex: string): string[] {
  if (!HEX_RE.test(hex)) return list;
  const h = hex.toLowerCase();
  return [h, ...list.filter((c) => c.toLowerCase() !== h)].slice(0, HISTORY_CAP);
}

/** 读取历史（localStorage 不可用时返回空表）。 */
export function loadHistory(): string[] {
  try {
    return parseHistory(localStorage.getItem(HISTORY_KEY));
  } catch {
    return [];
  }
}

/** 写回历史（存储受限时静默跳过，不影响取色主流程）。 */
export function saveHistory(list: string[]): void {
  try {
    localStorage.setItem(HISTORY_KEY, JSON.stringify(list.slice(0, HISTORY_CAP)));
  } catch {
    /* 存储满/被禁用 → 本次不持久化 */
  }
}
