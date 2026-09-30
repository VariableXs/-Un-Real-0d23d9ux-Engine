/**
 * AI-18 U-49 氛围音景引擎 — Web Audio 程序化合成（零音频资产）。
 *
 * 口径（全景书）：
 * - 五场景：雨 / 林 / 白噪 / 粉噪 / 深夜 —— 噪声滤波 + 随机事件粒子
 *   （雨滴 = 短促带通噪声脉冲；鸟鸣 = FM 合成短句；参数化随机，非循环文件）；
 * - 场景可叠加、音量独立、渐入渐出 2s；
 * - 睡眠定时到期自动淡出；
 * - 连续播放内存无增长（节点复用；粒子用一次性节点 + 自动断开）。
 */

import type { SoundscapeScene } from "../system/ambience/schema";

export const SOUNDSCAPE_SCENES: readonly SoundscapeScene[] = ["rain", "forest", "white", "pink", "night"];
export const FADE_SECONDS = 2;

/** 纯逻辑：淡入曲线（0→1 线性，单测用）。 */
export function fadeGain(from: number, to: number, t: number): number {
  return from + (to - from) * Math.min(1, Math.max(0, t));
}

/** 纯逻辑：睡眠定时是否到期。 */
export function sleepTimerDue(startedAt: number, minutes: number, now: number): boolean {
  return minutes > 0 && now - startedAt >= minutes * 60_000;
}

// ---------------- 粒子规划（纯函数，可测） ----------------

/** 规划 count 个雨滴在 durSec 内的时刻（泊松感：均匀随机）。 */
export function planDrops(count: number, durSec: number, rnd: () => number = Math.random): number[] {
  const out: number[] = [];
  for (let i = 0; i < count; i++) out.push(rnd() * durSec);
  return out.sort((a, b) => a - b);
}

export interface Chirp {
  /** 开始时刻（秒）。 */
  at: number;
  /** FM 载波基频（Hz）。 */
  freq: number;
  /** 时长（秒）。 */
  dur: number;
}

/** 规划 count 声鸟鸣（参数化随机：基频 1.8–3.5kHz、时长 0.08–0.25s）。 */
export function planChirps(count: number, durSec: number, rnd: () => number = Math.random): Chirp[] {
  const out: Chirp[] = [];
  for (let i = 0; i < count; i++) {
    out.push({
      at: rnd() * durSec,
      freq: 1800 + rnd() * 1700,
      dur: 0.08 + rnd() * 0.17,
    });
  }
  return out.sort((a, b) => a.at - b.at);
}

// ---------------- 音频引擎 ----------------

interface SceneRecipe {
  /** 基底噪声类型（white = 白；pink 通过白噪经 -3dB/oct 滤波近似）。 */
  noise: "white" | "pink";
  filter: { type: BiquadFilterType; freq: number; q: number };
  /** 粒子类型（rain = 雨滴脉冲；forest = 鸟鸣；night = 偶发低频远声）。 */
  particles: "drops" | "chirps" | "none";
  /** 粒子频率（个/分钟）。 */
  particleRate: number;
}

const RECIPES: Record<SoundscapeScene, SceneRecipe> = {
  // 雨：粉噪基底 + 低通（雨幕）+ 雨滴带通脉冲
  rain: { noise: "pink", filter: { type: "lowpass", freq: 1400, q: 0.4 }, particles: "drops", particleRate: 90 },
  // 林：白噪极低量（叶沙沙）+ 高通 + 鸟鸣 FM
  forest: { noise: "white", filter: { type: "highpass", freq: 3000, q: 0.3 }, particles: "chirps", particleRate: 8 },
  // 白噪：平坦基底，无粒子
  white: { noise: "white", filter: { type: "lowpass", freq: 20000, q: 0.0001 }, particles: "none", particleRate: 0 },
  // 粉噪：-3dB/oct（用低通 640Hz + 低 Q 近似听感）
  pink: { noise: "white", filter: { type: "lowpass", freq: 640, q: 0.25 }, particles: "none", particleRate: 0 },
  // 深夜：粉噪更低沉 + 偶发远声（雨滴粒子超低速率 = 夜虫/远雨意趣）
  night: { noise: "pink", filter: { type: "lowpass", freq: 420, q: 0.35 }, particles: "drops", particleRate: 6 },
};

let ctx: AudioContext | null = null;

function audio(): AudioContext | null {
  if (typeof window === "undefined") return null;
  if (!ctx) {
    const AC = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!AC) return null;
    ctx = new AC();
  }
  if (ctx.state === "suspended") void ctx.resume().catch(() => {});
  return ctx;
}

function noiseBuffer(ac: AudioContext, kind: "white" | "pink"): AudioBuffer {
  // 4 秒循环缓冲（够长避免可闻重复；节点复用不重建）
  const len = ac.sampleRate * 4;
  const buf = ac.createBuffer(1, len, ac.sampleRate);
  const data = buf.getChannelData(0);
  if (kind === "white") {
    for (let i = 0; i < len; i++) data[i] = Math.random() * 2 - 1;
  } else {
    // Paul Kellet 粉噪近似
    let b0 = 0, b1 = 0, b2 = 0, b3 = 0, b4 = 0, b5 = 0, b6 = 0;
    for (let i = 0; i < len; i++) {
      const w = Math.random() * 2 - 1;
      b0 = 0.99886 * b0 + w * 0.0555179;
      b1 = 0.99332 * b1 + w * 0.0750759;
      b2 = 0.969 * b2 + w * 0.153852;
      b3 = 0.8665 * b3 + w * 0.3104856;
      b4 = 0.55 * b4 + w * 0.5329522;
      b5 = -0.7616 * b5 - w * 0.016898;
      data[i] = (b0 + b1 + b2 + b3 + b4 + b5 + b6 + w * 0.5362) * 0.11;
      b6 = w * 0.115926;
    }
  }
  return buf;
}

interface SceneLayer {
  gain: GainNode;
  base?: AudioBufferSourceNode;
  particleTimer: number | null;
}

const layers = new Map<SoundscapeScene, SceneLayer>();
const startedAt = new Map<SoundscapeScene, number>();

/** 单个雨滴粒子：短促带通噪声脉冲（一次性节点，播完自动断开 —— 无泄漏）。 */
function drop(ac: AudioContext, dest: AudioNode): void {
  const dur = 0.02 + Math.random() * 0.03;
  const src = ac.createBufferSource();
  src.buffer = noiseBuffer(ac, "white");
  const bp = ac.createBiquadFilter();
  bp.type = "bandpass";
  bp.frequency.value = 900 + Math.random() * 2200;
  bp.Q.value = 6;
  const g = ac.createGain();
  const t = ac.currentTime;
  g.gain.setValueAtTime(0.0001, t);
  g.gain.exponentialRampToValueAtTime(0.28 + Math.random() * 0.2, t + 0.004);
  g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
  src.connect(bp).connect(g).connect(dest);
  src.start(t);
  src.stop(t + dur + 0.02);
  src.onended = () => {
    try { src.disconnect(); bp.disconnect(); g.disconnect(); } catch { /* noop */ }
  };
}

/** 单声鸟鸣：FM 合成短句（载波 + 快速调制；一次性节点）。 */
function chirp(ac: AudioContext, dest: AudioNode, freq: number, dur: number): void {
  const osc = ac.createOscillator();
  osc.type = "sine";
  const mod = ac.createOscillator();
  mod.type = "sine";
  mod.frequency.value = 28 + Math.random() * 40; // 颤音调制
  const modGain = ac.createGain();
  modGain.gain.value = freq * 0.12;
  mod.connect(modGain).connect(osc.frequency);
  osc.frequency.value = freq;
  const g = ac.createGain();
  const t = ac.currentTime;
  g.gain.setValueAtTime(0.0001, t);
  g.gain.exponentialRampToValueAtTime(0.12, t + 0.015);
  g.gain.exponentialRampToValueAtTime(0.0001, t + dur);
  osc.connect(g).connect(dest);
  osc.start(t); mod.start(t);
  osc.stop(t + dur + 0.02); mod.stop(t + dur + 0.02);
  osc.onended = () => {
    try { osc.disconnect(); mod.disconnect(); modGain.disconnect(); g.disconnect(); } catch { /* noop */ }
  };
}

/** 粒子调度循环（每 2s 批量规划下一窗；一次性节点自动回收）。 */
function scheduleParticles(ac: AudioContext, scene: SoundscapeScene, dest: AudioNode): number {
  const r = RECIPES[scene];
  if (r.particles === "none" || r.particleRate <= 0) return 0;
  const fire = (): void => {
    const n = Math.max(0, Math.round((r.particleRate * 2) / 60));
    for (let i = 0; i < n; i++) {
      if (r.particles === "drops") drop(ac, dest);
      else if (r.particles === "chirps") {
        const c = planChirps(1, 2)[0];
        if (c) window.setTimeout(() => chirp(ac, dest, c.freq, c.dur), c.at * 1000);
      }
    }
  };
  fire();
  return window.setInterval(fire, 2000);
}

/** 开播场景（渐入 2s；已在播则先淡出旧层再启新层 —— 切换无爆音）。 */
export function startScene(scene: SoundscapeScene, volume: number, opts?: { muted?: boolean }): void {
  const ac = audio();
  if (!ac) return;
  stopScene(scene, volume); // 已在播：先按当前值淡出再重启
  const r = RECIPES[scene];
  const gain = ac.createGain();
  const target = opts?.muted ? 0 : Math.min(1, Math.max(0, volume)) * 0.5;
  gain.gain.setValueAtTime(0.0001, ac.currentTime);
  gain.gain.linearRampToValueAtTime(Math.max(0.0001, target), ac.currentTime + FADE_SECONDS);
  gain.connect(ac.destination);

  const base = ac.createBufferSource();
  base.buffer = noiseBuffer(ac, r.noise);
  base.loop = true;
  const filt = ac.createBiquadFilter();
  filt.type = r.filter.type;
  filt.frequency.value = r.filter.freq;
  filt.Q.value = r.filter.q;
  base.connect(filt).connect(gain);
  base.start();

  layers.set(scene, { gain, base, particleTimer: scheduleParticles(ac, scene, gain) });
  startedAt.set(scene, Date.now());
}

/** 停播场景（渐出 2s 后释放节点）。 */
export function stopScene(scene: SoundscapeScene, currentVolume: number): void {
  const layer = layers.get(scene);
  if (!layer) return;
  layers.delete(scene);
  startedAt.delete(scene);
  if (layer.particleTimer) window.clearInterval(layer.particleTimer);
  const ac = ctx;
  if (!ac) return;
  layer.gain.gain.cancelScheduledValues(ac.currentTime);
  layer.gain.gain.setValueAtTime(Math.max(0.0001, currentVolume * 0.5), ac.currentTime);
  layer.gain.gain.linearRampToValueAtTime(0.0001, ac.currentTime + FADE_SECONDS);
  try { layer.base?.stop(ac.currentTime + FADE_SECONDS + 0.1); } catch { /* already stopped */ }
  window.setTimeout(() => {
    try { layer.base?.disconnect(); } catch { /* noop */ }
    try { layer.gain.disconnect(); } catch { /* noop */ }
  }, (FADE_SECONDS + 0.2) * 1000);
}

/** 在播场景列表。 */
export function activeScenes(): SoundscapeScene[] {
  return [...layers.keys()];
}

/** 场景开播时刻（Date.now；睡眠定时计算用；未播 = null）。 */
export function sceneStartedAt(scene: SoundscapeScene): number | null {
  return startedAt.get(scene) ?? null;
}

/** 总静音开关：所有在播层音量淡变（M-66 30ms 淡变在 gain param 层由 ramp 实现）。 */
export function setSoundscapeMuted(muted: boolean, volumes: Record<SoundscapeScene, number>): void {
  const ac = ctx;
  if (!ac) return;
  for (const [scene, layer] of layers) {
    const target = muted ? 0 : Math.min(1, Math.max(0, volumes[scene] ?? 0.5)) * 0.5;
    layer.gain.gain.cancelScheduledValues(ac.currentTime);
    layer.gain.gain.setValueAtTime(Math.max(0.0001, layer.gain.gain.value), ac.currentTime);
    layer.gain.gain.linearRampToValueAtTime(Math.max(0.0001, target), ac.currentTime + 0.03);
  }
}

/** 全停（退出环境 / 睡眠定时到期）。 */
export function stopAllScenes(volumes: Record<SoundscapeScene, number>): void {
  for (const scene of [...layers.keys()]) stopScene(scene, volumes[scene] ?? 0.5);
}
