/**
 * AI-18 M-66 音量淡变防爆音 — 30ms 线性淡变（纯逻辑 + Web Audio 应用）。
 *
 * 口径：
 * - 常数 30ms 写死（听感无延迟、无爆音）；
 * - 仅对环境内发起的调节生效（系统侧音量键直达系统，不拦截不改写）；
 * - 环境内音频（音景 U-49 / UI 音效）走 Web Audio GainNode 线性 ramp；
 * - 拖动滑杆：跟随目标值持续重定 ramp（跟手感 = 每次目标变化重设锚点，30ms 内到达）。
 */

import type { AmbienceSettings } from "./schema";

/** 淡变常数（毫秒）。写死，不做设置项（全景书口径）。 */
export const VOLUME_FADE_MS = 30;

/** 纯逻辑：淡变曲线采样（单测用；线性 from→to）。 */
export function fadeCurve(from: number, to: number, steps = 6): number[] {
  const out: number[] = [];
  for (let i = 0; i < steps; i++) {
    const t = i / (steps - 1);
    out.push(from + (to - from) * t);
  }
  return out;
}

/** 环境内音量调节入口：对目标 GainNode 施加 30ms 线性 ramp（防爆音）。 */
export function fadeGainTo(gain: AudioParam, target: number, audioContext: AudioContext, nowSec?: number): void {
  const t = nowSec ?? audioContext.currentTime;
  const value = Math.max(0.0001, Math.min(1, target));
  gain.cancelScheduledValues(t);
  gain.setValueAtTime(Math.max(0.0001, gain.value), t);
  gain.linearRampToValueAtTime(value, t + VOLUME_FADE_MS / 1000);
}

/** 静音切换同样走淡变（半夜静音「咔」声消除）。 */
export function fadeMuteToggle(gain: AudioParam, muted: boolean, audioContext: AudioContext): void {
  fadeGainTo(gain, muted ? 0 : 1, audioContext);
}

/** 设置音景主音量（M-66 淡变 + U-49 音量表折算）。 */
export function sceneTargetVolume(scene: keyof AmbienceSettings["soundscapeVolumes"], settings: AmbienceSettings): number {
  const v = settings.soundscapeVolumes[scene] ?? 0.5;
  return Math.min(1, Math.max(0, v)) * 0.5;
}
