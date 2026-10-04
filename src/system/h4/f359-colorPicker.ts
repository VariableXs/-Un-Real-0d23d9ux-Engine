/**
 * F359 全局屏幕拾色器（H 域 · AI-H4）：
 * 创造者工具第一件：系统级吸管——取屏幕任意像素颜色（放大镜环辅助定位、
 * 显示 HEX/RGB 双读数），取到即复制入剪贴板并进 F234 最近使用色；
 * 连续取色模式（按住 Shift 连续取）。
 * 判据（主册 F359）：取色准确性（对已知色板 20 点零误差）；放大环倍率；
 * HEX/RGB 双格式复制；进 F234 联动；连续模式。
 * 依赖锚点：F234 颜色选择器（最近使用色接口）。
 */

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

/** 放大镜环：环内显示的采样边长（像素）与显示放大倍率。 */
export const LOUPE_SAMPLING_PX = 15;
export const LOUPE_ZOOM = 8;

/** 通道值钳制到 [0,255]（取样源异常时显式钳制而非产出非法色）。 */
export function clampChannel(v: number): number {
  if (!Number.isFinite(v)) return 0;
  return Math.min(255, Math.max(0, Math.round(v)));
}

/** HEX 双读数之一：#RRGGBB 大写。 */
export function toHex(c: Rgb): string {
  const h = (v: number) => clampChannel(v).toString(16).toUpperCase().padStart(2, "0");
  return `#${h(c.r)}${h(c.g)}${h(c.b)}`;
}

/** RGB 双读数之二：rgb(r, g, b)。 */
export function toRgbString(c: Rgb): string {
  return `rgb(${clampChannel(c.r)}, ${clampChannel(c.g)}, ${clampChannel(c.b)})`;
}

/** 剪贴板载荷：双格式同时给出（判据「双格式复制」）。 */
export function clipboardPayload(c: Rgb): { hex: string; rgb: string } {
  return { hex: toHex(c), rgb: toRgbString(c) };
}

export interface ColorPick {
  x: number;
  y: number;
  color: Rgb;
  at: number;
  /** Shift 连续模式标记。 */
  continuous: boolean;
}

/** 最近使用色容量（F234 判据同源：最近 8 色）。 */
export const RECENT_COLORS_CAP = 8;

/**
 * 进 F234 最近使用色：新色插队首，重复色（HEX 相等）提至队首不重复占位。
 * 返回新数组（不改入参）。
 */
export function pushRecentColor(recent: Rgb[], c: Rgb): Rgb[] {
  const hex = toHex(c);
  const rest = recent.filter((x) => toHex(x) !== hex);
  return [c, ...rest].slice(0, RECENT_COLORS_CAP);
}

/**
 * 取色准确性审计：对已知色板逐点零误差（判据「对已知色板 20 点零误差」）。
 * 拾取结果与色板比对：HEX 相等即零误差；返回错点列表。
 */
export function auditColorAccuracy(samples: Array<{ expected: Rgb; picked: Rgb }>): Array<{ index: number; expected: string; picked: string }> {
  return samples
    .map((s, index) => ({ index, expected: toHex(s.expected), picked: toHex(s.picked) }))
    .filter((r) => r.expected !== r.picked);
}

/** 连续模式状态机：普通模式取一色即退出；Shift 按住则保持。 */
export interface PickerSessionState {
  active: boolean;
  shiftHeld: boolean;
  picks: number;
}

export function pickOnce(state: PickerSessionState): PickerSessionState {
  return { ...state, picks: state.picks + 1, active: state.shiftHeld };
}

/** 放大环几何：环心即采样点，环半径 = sampling×zoom/2（诊断与绘制共用）。 */
export function loupeGeometry(): { radiusPx: number; samplingPx: number; zoom: number } {
  return { radiusPx: (LOUPE_SAMPLING_PX * LOUPE_ZOOM) / 2, samplingPx: LOUPE_SAMPLING_PX, zoom: LOUPE_ZOOM };
}

/* ================= v5 深化批次五：色值解析互转 / 对比度 / 抗噪取样 ================= */

/** HEX 解析：#RGB/#RRGGBB 大小写兼容；非法如实 null（不猜）。 */
export function parseHex(text: string): Rgb | null {
  const t = text.trim().replace(/^#/, "");
  if (/^[0-9a-fA-F]{6}$/.test(t)) {
    return { r: parseInt(t.slice(0, 2), 16), g: parseInt(t.slice(2, 4), 16), b: parseInt(t.slice(4, 6), 16) };
  }
  if (/^[0-9a-fA-F]{3}$/.test(t)) {
    return { r: parseInt(t[0]! + t[0]!, 16), g: parseInt(t[1]! + t[1]!, 16), b: parseInt(t[2]! + t[2]!, 16) };
  }
  return null;
}

/** rgb()/rgba() 解析：越界钳制；格式错如实 null。 */
export function parseRgbString(text: string): Rgb | null {
  const m = /rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)(?:\s*,\s*[\d.]+)?\s*\)/.exec(text.trim());
  if (!m) return null;
  return { r: clampChannel(Number(m[1])), g: clampChannel(Number(m[2])), b: clampChannel(Number(m[3])) };
}

/** 互转 round-trip：HEX → RGB 串 → 解析 → HEX 恒等（复制保真的机检面）。 */
export function hexRoundTrip(c: Rgb): boolean {
  const parsed = parseRgbString(toRgbString(c));
  return parsed !== null && toHex(parsed) === toHex(c);
}

/** WCAG 相对亮度（sRGB 线性化——对比度计算的标准口径）。 */
export function relativeLuminance(c: Rgb): number {
  const lin = (v: number) => {
    const s = clampChannel(v) / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b);
}

/** WCAG 对比度（取色 → 检查文字可读性的延伸工具）。 */
export function contrastRatio(a: Rgb, b: Rgb): number {
  const la = relativeLuminance(a);
  const lb = relativeLuminance(b);
  const ratio = (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
  return Math.round(ratio * 100) / 100;
}

/** 抗噪取样：环内 3×3 中心加权平均（中心 5、边 1——单像素噪点不再带歪取色）。 */
export function loupeAverage(samples: Rgb[]): Rgb {
  if (samples.length === 0) return { r: 0, g: 0, b: 0 };
  const w = samples.map((_, i) => (i === Math.floor(samples.length / 2) ? 5 : 1));
  const total = w.reduce((a, b) => a + b, 0);
  const mix = (get: (c: Rgb) => number) => Math.round(samples.reduce((s, c, i) => s + get(c) * w[i]!, 0) / total);
  return { r: mix((c) => c.r), g: mix((c) => c.g), b: mix((c) => c.b) };
}
