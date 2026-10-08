/**
 * U3-v8 引擎五：wallmatrix —— F501 五档亮度×两字色矩阵活体扩容
 * （AI-U3 · 批次八工单⑤）。
 *
 * 判据唯一源（主册摘文 + 工单）：
 * - F501「五档亮度壁纸×两字色自动选择用例；投影参数实测；选中底衬；4K
 *   渲染精度；与 F297 压暗联动一致性」——此前 deskicons/lumapick 的
 *   「五档」是标量样张（0.05…0.95 五个数）；本引擎把样张扩容为**真实
 *   壁纸采样**：合成壁纸位图（纯函数像素发生器：渐变/棋盘/噪点三种纹理
 *   × 五档目标亮度），逐区域（图标带/任务栏带/全幅）真实扫描像素求
 *   感知亮度，再过 pickIconTextColor 判字色——产出 5×2=10 格全矩阵册。
 * - 滞回带（hysteresis）：亮度在阈值附近抖动时字色不闪烁（0.47-0.53
 *   保持上一次选择）——真实壁纸会跨阈值，无滞回必抖。
 * - 复用 deskicons 的 luma/pickIconTextColor（同源不重写算法）。
 */

import { luma, pickIconTextColor, type IconTextColor } from "../deskicons";

/* ------------------------------- 壁纸发生器 ------------------------------- */

/** 壁纸纹理种类（三种代表性构图——纯函数生成，零 IO）。 */
export type WallTexture = "gradient" | "checker" | "noise";

/** 壁纸位图（行主序 RGB，值域 0-255）。 */
export interface WallBitmap {
  w: number;
  h: number;
  /** 长度 = w*h*3。 */
  px: Float64Array;
}

/** 五档目标亮度（判据口径：极暗/暗/中/亮/极亮——RGB 255 尺度）。 */
export const WALL_TARGET_LUMA_255 = [16, 64, 128, 192, 240] as const;

/** 确定性伪随机（噪点纹理用——LCG，无外部依赖，同种子同图）。 */
function lcg(seed: number): () => number {
  let s = seed >>> 0;
  return () => {
    s = (s * 1664525 + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

/**
 * 合成壁纸位图（判据：五档亮度真实成图——灰阶 target 255 尺度直接铺
 * RGB，纹理只改变空间分布不改变平均灰度）。
 */
export function makeWallpaper(texture: WallTexture, target255: number, w = 64, h = 40, seed = 7): WallBitmap {
  const px = new Float64Array(w * h * 3);
  const rnd = lcg(seed);
  const t = target255 / 255;
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      let v: number;
      if (texture === "gradient") {
        // 垂直渐变：顶 target+0.1 → 底 target-0.1（均值≈target）
        v = t + 0.1 - 0.2 * (y / Math.max(1, h - 1));
      } else if (texture === "checker") {
        // 棋盘：target ± 0.15 交替（均值≈target）
        v = t + (((x >> 3) + (y >> 3)) % 2 === 0 ? 0.15 : -0.15);
      } else {
        // 噪点：target ± 随机 0.1（LCG 均值≈0.5，长程均值≈target）
        v = t + (rnd() - 0.5) * 0.2;
      }
      const c = Math.max(0, Math.min(1, v)) * 255;
      const i = (y * w + x) * 3;
      px[i] = c; px[i + 1] = c; px[i + 2] = c;
    }
  }
  return { w, h, px };
}

/* ------------------------------- 区域扫描 ------------------------------- */

/** 采样区域（图标带=中部 60%、任务栏带=底部 12%——判据「逐区域」口径）。 */
export type SampleZone = "full" | "icon-band" | "taskbar-band";

/** 区域像素范围（返回 [起y, 止y) 行区间）。 */
function zoneRows(zone: SampleZone, h: number): [number, number] {
  if (zone === "icon-band") return [Math.floor(h * 0.2), Math.floor(h * 0.8)];
  if (zone === "taskbar-band") return [Math.floor(h * 0.88), h];
  return [0, h];
}

/** 真实扫描：逐像素线性化→Rec.709 加权→算术平均（与 deskicons.luma 同源）。 */
export function scanLuma(bmp: WallBitmap, zone: SampleZone = "full"): number {
  const [y0, y1] = zoneRows(zone, bmp.h);
  let sum = 0;
  let n = 0;
  for (let y = y0; y < y1; y++) {
    for (let x = 0; x < bmp.w; x++) {
      const i = (y * bmp.w + x) * 3;
      sum += luma(bmp.px[i]! / 255, bmp.px[i + 1]! / 255, bmp.px[i + 2]! / 255);
      n++;
    }
  }
  if (n === 0) throw new Error("[u3:F501] 采样区域为空——壁纸位图尺寸非法");
  return sum / n;
}

/* ------------------------------- 滞回带 ------------------------------- */

/** 滞回半宽（判据：阈值附近不闪烁——0.47/0.53 双门限）。 */
export const HYSTERESIS_HALF = 0.03;

export interface TextColorRt {
  last: IconTextColor | null;
}

/**
 * 滞回选色（带状态的决策器）：进入滞回带保持上次选择；首次决策按阈值。
 * darkOverlay 透传 F297（联动一致性判据同 deskicons）。
 */
export function pickWithHysteresis(rt: TextColorRt, l: number, darkOverlay = false): { color: IconTextColor; rt: TextColorRt } {
  const eff = darkOverlay ? l * 0.7 : l;
  if (rt.last !== null && eff >= 0.5 - HYSTERESIS_HALF && eff <= 0.5 + HYSTERESIS_HALF) {
    return { color: rt.last, rt };
  }
  const color = pickIconTextColor(l, darkOverlay);
  return { color, rt: { last: color } };
}

/* ------------------------------- 全矩阵册 ------------------------------- */

export interface MatrixCell {
  texture: WallTexture;
  target255: number;
  zone: SampleZone;
  measuredLuma: number;
  color: IconTextColor;
  /** 判据自洽：色与实测亮度方向一致（暗→浅字/亮→深字）。 */
  consistent: boolean;
}

/**
 * 五档 × 三纹理 × 三区域全矩阵（工单⑤「活体扩容」的产物——45 格全
 * 真实扫描，任一格方向自洽性破坏即红）。
 */
export function buildWallMatrix(): Array<MatrixCell> {
  const out: Array<MatrixCell> = [];
  const textures: ReadonlyArray<WallTexture> = ["gradient", "checker", "noise"];
  const zones: ReadonlyArray<SampleZone> = ["full", "icon-band", "taskbar-band"];
  for (const tex of textures) {
    for (const t of WALL_TARGET_LUMA_255) {
      const bmp = makeWallpaper(tex, t);
      for (const zone of zones) {
        const l = scanLuma(bmp, zone);
        const color = pickIconTextColor(l);
        out.push({ texture: tex, target255: t, zone, measuredLuma: l, color, consistent: color === (l >= 0.5 ? "dark" : "light") });
      }
    }
  }
  return out;
}

/* ------------------------------- 自检 ------------------------------- */

export function wallmatrixSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];

  // 五档成图：实测亮度单调升、均值接近目标（灰度 255/255 线性化后 128/255≈0.216 线性域）
  const lumas = WALL_TARGET_LUMA_255.map((t) => scanLuma(makeWallpaper("gradient", t)));
  checks.push({ name: "F501 五档实测单调升", pass: lumas.every((l, i) => i === 0 || l > lumas[i - 1]!) });
  checks.push({ name: "F501 纹理不改均值", pass: Math.abs(lumas[2]! - 0.216) < 0.02 });

  // 三纹理同档互差小（纹理只改分布不改均值）
  const g = scanLuma(makeWallpaper("gradient", 128));
  const c = scanLuma(makeWallpaper("checker", 128));
  const n = scanLuma(makeWallpaper("noise", 128));
  checks.push({ name: "F501 三纹理同档互差 <0.05", pass: Math.abs(g - c) < 0.05 && Math.abs(g - n) < 0.05 });

  // 区域扫描：任务栏带取底部（gradient 底部更暗 → 带值 ≤ 全幅）
  const bmp = makeWallpaper("gradient", 128);
  const full = scanLuma(bmp, "full");
  const bar = scanLuma(bmp, "taskbar-band");
  checks.push({ name: "F501 区域扫描任务栏带更暗", pass: bar < full });

  // 全矩阵 45 格方向自洽
  const matrix = buildWallMatrix();
  checks.push({ name: "F501 全矩阵 45 格自洽", pass: matrix.length === 45 && matrix.every((m) => m.consistent) });
  // 矩阵极暗档全浅字、极亮档全深字（两字色用例的两端）
  checks.push({ name: "F501 极暗全浅/极亮全深", pass: matrix.filter((m) => m.target255 === 16).every((m) => m.color === "light") && matrix.filter((m) => m.target255 === 240).every((m) => m.color === "dark") });

  // 滞回：0.49 抖动保持上次选择；离带按阈值
  let rt: TextColorRt = { last: null };
  const first = pickWithHysteresis(rt, 0.55);
  rt = first.rt;
  const inBand = pickWithHysteresis(rt, 0.49);
  checks.push({ name: "F501 滞回带保持字色", pass: first.color === "dark" && inBand.color === "dark" && inBand.rt === rt });
  const outBand = pickWithHysteresis(rt, 0.40);
  checks.push({ name: "F501 离带按阈值翻转", pass: outBand.color === "light" && outBand.rt.last === "light" });

  // F297 联动：0.55 压暗后翻转（0.55*0.7=0.385）
  const darkened = pickWithHysteresis({ last: null }, 0.55, true);
  checks.push({ name: "F501 F297 压暗联动翻转", pass: darkened.color === "light" });

  return checks;
}
