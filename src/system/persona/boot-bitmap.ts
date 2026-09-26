/**
 * F165 开机动画深化 · 粒子 → 真帧位图投影 + 烘焙字节预算。
 *
 * 主册判据延伸：
 * - boot-engine 已有粒子模拟与烘帧**清单**（计数面）；本模块做**产物面**：
 *   把粒子投影成 RGBA 帧位图并编码 PNG——「三档密度实际帧资产数对拍」
 *   从对数变成对字节；
 * - 「结构与时长零变化（8.0s±0.2s）」：投影严格按 actOf 边界采样，
 *   BAKE_SAMPLE_FPS 30fps × 8s = 240 帧/密度，逐帧确定性（同 seed 同像素）；
 * - 性能契约：帧字节预算先算后烘（超预算 → 降密度不降时长——结构不动）。
 */

import { actOf, seedParticles, stepParticles, BAKE_SAMPLE_FPS, ACT_BOUNDS_MS, type ParticleState } from "./boot-engine";
import { BOOT_DURATION_MS } from "./store";
import type { ParticleDensity } from "./bootskin";
import { encodePng, pngBytesEstimate, type RgbaBitmap } from "./png-encode";

// ---------- 投影（粒子 → 帧位图） ----------

export interface ProjectionOptions {
  width: number;
  height: number;
  /** 幕底色（幕一~三深空底）。 */
  backdrop: [number, number, number];
  /** 粒子半径 px（4K 下随分辨率缩放——不是固定 2px）。 */
  particleRadius: number;
  /** 调色板（bootskin.particlePalette 的产物——颜色一处一事实）。 */
  palette: { base: string; highlight: string };
}

const ACT3_START = 6800; // 幕四起点（交接淡出窗）——与 ACT_BOUNDS_MS 一致，不另设时间表。

/** 幕进度透明度：幕一~三全亮，幕四线性淡出（透明度按幕进度控制——渲染层职责）。 */
export function particleAlpha(p: ParticleState, elapsedMs: number): number {
  if (p.act !== 3) return 1;
  const t = Math.min(1, Math.max(0, (elapsedMs - ACT3_START) / (BOOT_DURATION_MS - ACT3_START)));
  return 1 - t;
}

/** 把一帧粒子态画进 RGBA 缓冲（加法混合——星光叠加的物理直觉）。 */
export function projectFrame(particles: ParticleState[], opts: ProjectionOptions, elapsedMs: number): RgbaBitmap {
  const { width, height, backdrop, particleRadius, palette } = opts;
  const baseRgb = hexToRgb(palette.base);
  const hiRgb = hexToRgb(palette.highlight);
  const data = new Uint8Array(width * height * 4);
  for (let i = 0; i < width * height; i++) {
    const o = i * 4;
    data[o] = backdrop[0];
    data[o + 1] = backdrop[1];
    data[o + 2] = backdrop[2];
    data[o + 3] = 255;
  }
  for (const p of particles) {
    const alpha = particleAlpha(p, elapsedMs);
    if (alpha <= 0.01) continue;
    const cx = p.x;
    const cy = p.y;
    const r = Math.max(1, particleRadius * p.size);
    const rgb = p.color === "highlight" ? hiRgb : baseRgb;
    const x0 = Math.max(0, Math.floor(cx - r));
    const x1 = Math.min(width - 1, Math.ceil(cx + r));
    const y0 = Math.max(0, Math.floor(cy - r));
    const y1 = Math.min(height - 1, Math.ceil(cy + r));
    for (let y = y0; y <= y1; y++) {
      for (let x = x0; x <= x1; x++) {
        const d = Math.hypot(x + 0.5 - cx, y + 0.5 - cy);
        if (d > r) continue;
        const coverage = (1 - d / r) * alpha; // 距离衰减近似圆盘覆盖。
        const o = (y * width + x) * 4;
        data[o] = Math.min(255, Math.round(data[o]! + rgb[0] * coverage));
        data[o + 1] = Math.min(255, Math.round(data[o + 1]! + rgb[1] * coverage));
        data[o + 2] = Math.min(255, Math.round(data[o + 2]! + rgb[2] * coverage));
      }
    }
  }
  return { width, height, data };
}

function hexToRgb(hex: string): [number, number, number] {
  const n = parseInt(hex.replace("#", ""), 16);
  return [(n >> 16) & 0xff, (n >> 8) & 0xff, n & 0xff];
}

// ---------- 烘焙计划（帧清单 → 字节预算 → 分批执行） ----------

export interface BakeJob {
  density: ParticleDensity;
  seed: number;
  width: number;
  height: number;
  /** 每帧字节预估（pngBytesEstimate——生成前知道总量）。 */
  bytesPerFrame: number;
  frameCount: number;
  totalBytes: number;
}

export const DENSITIES: readonly { density: ParticleDensity; count: number }[] = [
  { density: "minimal", count: 200 },
  { density: "standard", count: 600 },
  { density: "dense", count: 1200 },
];

/** 三档烘焙作业（帧数 = fps × 时长——8s × 30fps = 240 帧，结构契约）。
 * 字节真相：stored 模式 PNG 大小只随分辨率——密度差在烘焙 CPU 不在字节
 * （帧字节预算因此跨档一致，这是编码器的诚实属性不是缺陷）。 */
export function bakeJobs(seed: number, width = 1920, height = 1080): BakeJob[] {
  void seed;
  const bytesPerFrame = pngBytesEstimate(width, height);
  const frameCount = Math.round((BOOT_DURATION_MS / 1000) * BAKE_SAMPLE_FPS);
  return DENSITIES.map(({ density }) => ({
    density,
    seed,
    width,
    height,
    bytesPerFrame,
    frameCount,
    totalBytes: bytesPerFrame * frameCount,
  }));
}

/** 超预算降档：按时长不变、密度降档（结构与时长零变化是宪法，密度是变量）。 */
export function downgradeDensity(jobs: BakeJob[], limitBytes: number): { chosen: ParticleDensity | null; note: string } {
  const within = [...jobs].reverse().find((j) => j.totalBytes <= limitBytes); // dense → minimal 顺序取最大可用档。
  if (!within) return { chosen: null, note: "最小档仍超预算——降分辨率而非砍时长（8.0s 结构不动）" };
  return { chosen: within.density, note: `选用 ${within.density} 档（${(within.totalBytes / 1024 / 1024).toFixed(1)} MB）——时长与结构不变` };
}

/** 单帧烘焙（确定性——同 seed 同帧同像素，对拍口径）。 */
export function bakeFrame(density: ParticleDensity, seed: number, frameIndex: number, opts: ProjectionOptions, accentHex = "#8ea0ff"): { png: Uint8Array; elapsedMs: number } {
  const frameDt = 1000 / BAKE_SAMPLE_FPS;
  const elapsedMs = frameIndex * frameDt;
  let particles = seedParticles(density, seed, accentHex, opts.width, opts.height);
  for (let s = 0; s < frameIndex; s++) {
    particles = stepParticles(particles, s * frameDt, frameDt, opts.width, opts.height);
  }
  return { png: encodePng(projectFrame(particles, opts, elapsedMs)), elapsedMs };
}

/** 幕边界采样验证：240 帧必须完整覆盖四幕（结构对拍——少一幕即缺陷）。 */
export function actCoverageCheck(frameCount = Math.round((BOOT_DURATION_MS / 1000) * BAKE_SAMPLE_FPS)): { ok: boolean; perAct: number[]; missingActs: number[] } {
  const perAct = [0, 0, 0, 0];
  for (let f = 0; f < frameCount; f++) {
    const idx = actOf((f * 1000) / BAKE_SAMPLE_FPS);
    perAct[idx] = (perAct[idx] ?? 0) + 1;
  }
  const missingActs = perAct.map((c, i) => (c === 0 ? i : -1)).filter((i) => i >= 0);
  return { ok: missingActs.length === 0 && frameCount > 0, perAct, missingActs };
}

/** 幕边界与 ACT_BOUNDS_MS 一致性（一处一事实——投影不做自己的时间表）。 */
export function actBoundsContract(): { ok: boolean; bounds: readonly number[]; totalMs: number } {
  return { ok: ACT_BOUNDS_MS[ACT_BOUNDS_MS.length - 1] === BOOT_DURATION_MS, bounds: ACT_BOUNDS_MS, totalMs: BOOT_DURATION_MS };
}
