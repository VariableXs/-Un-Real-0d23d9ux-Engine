/**
 * F151/F162 强调色深化 · 亮度阶梯生成 + 色觉模拟 + 可辨识性审计。
 *
 * 主册判据延伸：
 * - F151「24 色令牌全系统覆盖」的派生面：一个强调色要撑起 hover/active/
 *   disabled/soft 全状态族——亮度阶梯（ramp）是工程化的派生法（一套
 *   数学，不做逐态手调）；
 * - 二十维度 14「色弱可辨」：色觉模拟（Viénot 1999 近似矩阵）+
 *   可辨识性矩阵——状态色在三类色觉下两两可分是验收判据的机械化；
 * - 三铁律「随时可退」：所有派生纯函数，输入删掉派生即消失。
 */

import { hexToRgb, hslToRgb, luminance, rgbToHex, rgbToHsl, type Rgb } from "./palette-engine";

// ---------- 强调色亮度阶梯（11 档工程惯例） ----------

export interface RampStep {
  /** 档位号 0..10（0 最暗）。 */
  step: number;
  hex: string;
  luminance: number;
  /** 对白/黑的对比度（选文字色的依据）。 */
  contrastOnWhite: number;
  contrastOnBlack: number;
}

/** 在 HSL 色相/饱和度守恒下生成亮度阶梯（L 从 0.12 到 0.97 等距）。 */
export function accentRamp(accentHex: string, steps = 11): RampStep[] {
  const rgb = hexToRgb(accentHex);
  if (!rgb) throw new Error(`非法强调色 ${accentHex}`);
  const { h, s } = rgbToHsl(rgb);
  const out: RampStep[] = [];
  for (let i = 0; i < steps; i++) {
    const l = 0.12 + (0.97 - 0.12) * (i / (steps - 1));
    const c = hslToRgb(h, s, l);
    const lum = luminance(c);
    out.push({
      step: i,
      hex: rgbToHex(c),
      luminance: Math.round(lum * 1000) / 1000,
      contrastOnWhite: Math.round(((1.05) / (lum + 0.05)) * 100) / 100,
      contrastOnBlack: Math.round(((lum + 0.05) / 0.05) * 100) / 100,
    });
  }
  return out;
}

/** 语义槽位推荐：按对比度需求从阶梯选档（不是拍脑袋定 hover 色）。 */
export type AccentSlot = "hover" | "active" | "disabled" | "soft" | "text" | "onAccent";

export function pickRampSlot(ramp: RampStep[], baseHex: string): Record<AccentSlot, string> {
  const base = hexToRgb(baseHex);
  if (!base) throw new Error(`非法强调色 ${baseHex}`);
  const baseLum = luminance(base);
  const byStep = (s: number): RampStep => ramp[Math.max(0, Math.min(ramp.length - 1, s))]!;
  const baseStep = ramp.reduce((best, r) => (Math.abs(r.luminance - baseLum) < Math.abs(byStep(best).luminance - baseLum) ? r.step : best), 0);
  const text = ramp.find((r) => r.contrastOnWhite >= 4.5) ?? byStep(ramp.length - 1); // 浅色底可读的第一档。
  const onAccent = [...ramp].reverse().find((r) => r.contrastOnBlack >= 4.5) ?? byStep(0); // 深色底可读。
  return {
    hover: byStep(baseStep - 1).hex,
    active: byStep(baseStep - 2).hex,
    disabled: byStep(baseStep + 3).hex,
    soft: byStep(baseStep + 5).hex,
    text: text.hex,
    onAccent: onAccent.hex,
  };
}

// ---------- 色觉模拟（Viénot 1999 近似 · 线性 RGB 域） ----------

export type VisionType = "normal" | "protanopia" | "deuteranopia" | "tritanopia";

/** sRGB→线性（模拟矩阵在线性域工作——色觉科学口径）。 */
function toLinear(c: Rgb): [number, number, number] {
  const lin = (v: number): number => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return [lin(c.r), lin(c.g), lin(c.b)];
}

function fromLinear(l: [number, number, number]): Rgb {
  const enc = (v: number): number => Math.round(255 * (v <= 0.0031308 ? v * 12.92 : 1.055 * v ** (1 / 2.4) - 0.055));
  return { r: enc(l[0]), g: enc(l[1]), b: enc(l[2]) };
}

/** Viénot 近似模拟矩阵（行随机频率归一）。 */
const VISION_MATRICES: Record<Exclude<VisionType, "normal">, number[][]> = {
  protanopia: [
    [0.11238, 0.88762, 0],
    [0.11238, 0.88762, 0],
    [0.00405, -0.00405, 1],
  ],
  deuteranopia: [
    [0.29275, 0.70725, 0],
    [0.29275, 0.70725, 0],
    [-0.02234, 0.02234, 1],
  ],
  tritanopia: [
    [1, 0.14461, -0.14461],
    [0, 0.85924, 0.14076],
    [0, 0.85924, 0.14076],
  ],
};

/** 单色色觉模拟（normal 原样返回——矩阵口径统一）。 */
export function simulateVision(hex: string, vision: VisionType): string {
  const rgb = hexToRgb(hex);
  if (!rgb) throw new Error(`非法颜色 ${hex}`);
  if (vision === "normal") return rgbToHex(rgb);
  const l = toLinear(rgb);
  const m = VISION_MATRICES[vision]!;
  const out: [number, number, number] = [
    m[0]![0]! * l[0] + m[0]![1]! * l[1] + m[0]![2]! * l[2],
    m[1]![0]! * l[0] + m[1]![1]! * l[1] + m[1]![2]! * l[2],
    m[2]![0]! * l[0] + m[2]![1]! * l[1] + m[2]![2]! * l[2],
  ];
  return rgbToHex(fromLinear(out));
}

// ---------- 可辨识性审计（状态色对在色觉下的 ΔE 近似） ----------

/** CIE76 ΔE（Lab 域——可辨识性的量化口径，ΔE ≥ 20 视为可分）。 */
export function deltaE(hexA: string, hexB: string): number {
  const labA = toLab(hexA);
  const labB = toLab(hexB);
  return Math.hypot(labA[0] - labB[0], labA[1] - labB[1], labA[2] - labB[2]);
}

function toLab(hex: string): [number, number, number] {
  const l = toLinear(hexToRgb(hex)!);
  // sRGB D65 → XYZ。
  const x = l[0] * 0.4124 + l[1] * 0.3576 + l[2] * 0.1805;
  const y = l[0] * 0.2126 + l[1] * 0.7152 + l[2] * 0.0722;
  const z = l[0] * 0.0193 + l[1] * 0.1192 + l[2] * 0.9505;
  const f = (t: number): number => (t > 0.008856 ? Math.cbrt(t) : 7.787 * t + 16 / 116);
  const fx = f(x / 0.95047);
  const fy = f(y / 1.0);
  const fz = f(z / 1.08883);
  return [116 * fy - 16, 500 * (fx - fy), 200 * (fy - fz)];
}

export interface DistinctionVerdict {
  vision: VisionType;
  /** 两两 ΔE 矩阵（状态色对：success/warn/danger/accent）。 */
  pairs: Array<{ a: string; b: string; deltaE: number; distinguishable: boolean }>;
  /** 全部可分 = 该色觉下状态可辨。 */
  allDistinguishable: boolean;
}

const STATE_KEYS = ["--p-success", "--p-warn", "--p-danger", "--p-accent"] as const;
const DE_THRESHOLD = 20;

/** 状态色可辨识性矩阵（正常 + 三类色觉全跑——20 维度 14 的机械化）。 */
export function auditStateDistinction(colors: Record<string, string>): DistinctionVerdict[] {
  const hexes = STATE_KEYS.map((k) => colors[k]).filter((v): v is string => typeof v === "string");
  const visions: VisionType[] = ["normal", "protanopia", "deuteranopia", "tritanopia"];
  return visions.map((vision) => {
    const simmed = hexes.map((h) => simulateVision(h, vision));
    const pairs: DistinctionVerdict["pairs"] = [];
    for (let i = 0; i < simmed.length; i++) {
      for (let j = i + 1; j < simmed.length; j++) {
        const de = deltaE(simmed[i]!, simmed[j]!);
        pairs.push({ a: STATE_KEYS[i]!, b: STATE_KEYS[j]!, deltaE: Math.round(de * 10) / 10, distinguishable: de >= DE_THRESHOLD });
      }
    }
    return { vision, pairs, allDistinguishable: pairs.every((p) => p.distinguishable) };
  });
}

/** 色觉自适应修复建议：不可分的对 → 调亮度间隔（改明度不改色相——保持语义）。 */
export function suggestDistinctionFix(colors: Record<string, string>, vision: Exclude<VisionType, "normal">): Array<{ key: string; from: string; to: string; reason: string }> {
  const verdict = auditStateDistinction(colors).find((v) => v.vision === vision)!;
  const fixes: Array<{ key: string; from: string; to: string; reason: string }> = [];
  for (const p of verdict.pairs.filter((x) => !x.distinguishable)) {
    const hex = colors[p.b]!;
    const rgb = hexToRgb(hex)!;
    const { h, s, l } = rgbToHsl(rgb);
    const targetL = l > 0.5 ? l - 0.18 : l + 0.18; // 向远离对侧的方向拉开明度。
    const to = rgbToHex(hslToRgb(h, s, Math.max(0.05, Math.min(0.95, targetL))));
    fixes.push({ key: p.b, from: hex, to, reason: `${p.a} 与 ${p.b} 在 ${vision} 下 ΔE=${p.deltaE}（<${DE_THRESHOLD}）——拉开明度保持色相` });
  }
  return fixes;
}
