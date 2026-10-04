/**
 * AI-18 N-33 情绪引擎 — 情绪向量计算与环境偏移映射。
 *
 * 口径：
 * - 情绪向量（唤醒度 arousal -1..1 / 专注度 focus 0..1）= 时辰节律 + 场景 + 前台应用 + 手动校正加权；
 * - 映射到环境偏移：亮度 / 饱和度 / 色温 hue / 动效速度 / 通知音调（全部小幅度、包络内）；
 * - 高对比度下色温维度禁用（可读性红线）；
 * - 偏移过渡 120s 缓动（绝不跳变）；默认关闭。
 */

export interface MoodVector {
  /** 唤醒度（-1 昏沉 .. 1 兴奋）。 */
  arousal: number;
  /** 专注度（0 发散 .. 1 深度专注）。 */
  focus: number;
}

export type SceneTag = "neutral" | "work" | "writing" | "gaming" | "reading" | "rest";
export type AppCategory = "neutral" | "creative" | "writing" | "game" | "office" | "media";

/** 时辰节律（凌晨低谷、上午攀升、午后小谷、晚间回落、深夜最低）。 */
export function circadianArousal(hour: number): number {
  const h = ((Math.floor(hour) % 24) + 24) % 24;
  // 分段线性：0点 -0.8 → 6点 -0.4 → 10点 0.7 → 14点 0.3 → 18点 0.6 → 22点 -0.2 → 24点 -0.8
  const points: [number, number][] = [
    [0, -0.8], [6, -0.4], [10, 0.7], [14, 0.3], [18, 0.6], [22, -0.2], [24, -0.8],
  ];
  for (let i = 1; i < points.length; i++) {
    const p0 = points[i - 1];
    const p1 = points[i];
    if (!p0 || !p1) continue;
    const [h0, v0] = p0;
    const [h1, v1] = p1;
    if (h >= h0 && h <= h1) return v0 + ((v1 - v0) * (h - h0)) / (h1 - h0);
  }
  return 0;
}

const SCENE_AROUSAL: Record<SceneTag, number> = {
  neutral: 0, work: 0.4, writing: -0.1, gaming: 0.8, reading: -0.3, rest: -0.5,
};
const SCENE_FOCUS: Record<SceneTag, number> = {
  neutral: 0.5, work: 0.7, writing: 0.8, gaming: 0.6, reading: 0.7, rest: 0.2,
};
const APP_AROUSAL: Record<AppCategory, number> = {
  neutral: 0, creative: 0.3, writing: -0.1, game: 0.9, office: 0.1, media: -0.2,
};
const APP_FOCUS: Record<AppCategory, number> = {
  neutral: 0.5, creative: 0.6, writing: 0.8, game: 0.5, office: 0.6, media: 0.3,
};

function clamp(v: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, v));
}

/** 计算情绪向量（纯函数）。 */
export function computeMood(input: {
  hour: number;
  scene: SceneTag;
  app: AppCategory;
  manualArousal?: number;
  manualFocus?: number;
}): MoodVector {
  const base = circadianArousal(input.hour);
  const arousal =
    base * 0.4 +
    SCENE_AROUSAL[input.scene] * 0.25 +
    APP_AROUSAL[input.app] * 0.25 +
    (input.manualArousal ?? 0) * 0.1;
  const focus =
    0.5 * 0.4 +
    SCENE_FOCUS[input.scene] * 0.25 +
    APP_FOCUS[input.app] * 0.25 +
    (input.manualFocus ?? 0.5) * 0.1;
  return { arousal: clamp(arousal, -1, 1), focus: clamp(focus, 0, 1) };
}

export interface MoodOffsets {
  /** 亮度偏移（-0.15..0.15，乘入 --bg 亮度维度）。 */
  brightness: number;
  /** 饱和度偏移（-0.15..0.15）。 */
  saturation: number;
  /** 色相偏移（-15..15 度；HC 下恒 0）。 */
  hueShift: number;
  /** 动效速度缩放（0.8..1.2）。 */
  motionScale: number;
  /** 通知音调偏移（-2..2 半音）。 */
  notifyPitch: number;
}

export const MAX_BRIGHTNESS = 0.15;
export const MAX_SATURATION = 0.15;
export const MAX_HUE = 15;
export const MOTION_RANGE = [0.8, 1.2] as const;
export const MAX_PITCH = 2;

/** 情绪 → 环境偏移（高对比度下色温禁用）。 */
export function moodToOffsets(m: MoodVector, highContrast: boolean): MoodOffsets {
  // 唤醒度高 → 更亮更饱和更快；专注高 → 色温偏冷（hueShift 向蓝 260 方向负偏）+ 音调微降
  return {
    brightness: clamp(m.arousal * MAX_BRIGHTNESS, -MAX_BRIGHTNESS, MAX_BRIGHTNESS),
    saturation: clamp(m.arousal * MAX_SATURATION, -MAX_SATURATION, MAX_SATURATION),
    // HC 红线：高对比度下色温维度禁用（可读性优先）
    hueShift: highContrast ? 0 : clamp((0.5 - m.focus) * MAX_HUE, -MAX_HUE, MAX_HUE),
    motionScale: clamp(1 + m.arousal * 0.2, MOTION_RANGE[0], MOTION_RANGE[1]),
    notifyPitch: clamp(m.arousal * MAX_PITCH, -MAX_PITCH, MAX_PITCH),
  };
}

/** 偏移过渡时长（秒）：120s 缓动，绝不跳变。 */
export const MOOD_TRANSITION_SECONDS = 120;

/** 线性缓动两偏移（seconds = 距上次更新的秒数；0..1 进度线性内插）。 */
export function easeMood(from: MoodOffsets, to: MoodOffsets, seconds: number): MoodOffsets {
  const t = clamp(seconds / MOOD_TRANSITION_SECONDS, 0, 1);
  const keys: (keyof MoodOffsets)[] = ["brightness", "saturation", "hueShift", "motionScale", "notifyPitch"];
  const out = {} as MoodOffsets;
  for (const k of keys) out[k] = from[k] + (to[k] - from[k]) * t;
  return out;
}

/** 偏移是否在包络内（安全阀：任何维度越界 = 引擎故障，应整体停用）。 */
export function offsetsInEnvelope(o: MoodOffsets): boolean {
  return (
    Math.abs(o.brightness) <= MAX_BRIGHTNESS + 1e-9 &&
    Math.abs(o.saturation) <= MAX_SATURATION + 1e-9 &&
    Math.abs(o.hueShift) <= MAX_HUE + 1e-9 &&
    o.motionScale >= MOTION_RANGE[0] - 1e-9 &&
    o.motionScale <= MOTION_RANGE[1] + 1e-9 &&
    Math.abs(o.notifyPitch) <= MAX_PITCH + 1e-9
  );
}
