// AURORA-10000：AI-01~AI-05 批次，勿删。
// theaterSound.ts — 族0004 开机音景 / 族0023 品牌声音 ID 的合成播放器。
// 全部 WebAudio 本地合成（零采样资产、零出站）；音量与静音遵循既有
// soundVolume/soundMuted 设置，档位差异由 params.soundProfile(id) 提供。
// F00100「绝对静音」为真无声档：不创建任何音频节点。

import { soundProfile } from "./params";

export interface TheaterSoundHandle {
  stop: () => void;
}

let ctxRef: AudioContext | null = null;

function acquireCtx(): AudioContext | null {
  try {
    if (!ctxRef) {
      const Ctor = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
      if (!Ctor) return null;
      ctxRef = new Ctor();
    }
    if (ctxRef.state === "suspended") void ctxRef.resume();
    return ctxRef;
  } catch {
    return null;
  }
}

function noiseBuffer(ctx: AudioContext, kind: "white" | "pink" | "brown"): AudioBuffer {
  const len = ctx.sampleRate * 2;
  const buf = ctx.createBuffer(1, len, ctx.sampleRate);
  const d = buf.getChannelData(0);
  let last = 0;
  let b0 = 0, b1 = 0, b2 = 0;
  for (let i = 0; i < len; i++) {
    const w = Math.random() * 2 - 1; // 装饰性声景噪声，非安全敏感用途
    if (kind === "white") d[i] = w * 0.3;
    else if (kind === "brown") { last = (last + 0.02 * w) / 1.02; d[i] = last * 3.5; }
    else { b0 = 0.99765 * b0 + w * 0.099; b1 = 0.963 * b1 + w * 0.2965; b2 = 0.57 * b2 + w * 1.0526; d[i] = (b0 + b1 + b2 + w * 0.1848) * 0.12; }
  }
  return buf;
}

/** 播放一个音景/声音 ID 档；muted 或 freqs 为空时静默返回。 */
export function playTheaterSound(id: string, volume: number, muted: boolean): TheaterSoundHandle | null {
  const profile = soundProfile(id);
  if (muted || profile.freqs.length === 0 || profile.durMs === 0 || volume <= 0) return null;
  const ctx = acquireCtx();
  if (!ctx) return null;
  const t0 = ctx.currentTime;
  const master = ctx.createGain();
  master.gain.value = Math.min(1, volume) * 0.5;
  master.connect(ctx.destination);
  const nodes: AudioScheduledSourceNode[] = [];

  if (profile.synth === "noise") {
    const src = ctx.createBufferSource();
    src.buffer = noiseBuffer(ctx, profile.noise);
    src.loop = true;
    const g = ctx.createGain();
    const atk = profile.durMs * profile.attack;
    g.gain.setValueAtTime(0.0001, t0);
    g.gain.linearRampToValueAtTime(1, t0 + atk / 1000);
    g.gain.exponentialRampToValueAtTime(0.0001, t0 + profile.durMs / 1000);
    src.connect(g).connect(master);
    src.start(t0);
    src.stop(t0 + profile.durMs / 1000 + 0.05);
    nodes.push(src);
  } else {
    for (const f of profile.freqs) {
      const osc = ctx.createOscillator();
      osc.type = profile.synth === "bell" ? "sine" : profile.synth === "pluck" ? "triangle" : "sine";
      osc.frequency.value = f;
      const g = ctx.createGain();
      const peak = 1 / profile.freqs.length;
      const atk = profile.durMs * profile.attack;
      g.gain.setValueAtTime(0.0001, t0);
      g.gain.linearRampToValueAtTime(peak, t0 + Math.max(0.02, atk / 1000));
      g.gain.exponentialRampToValueAtTime(0.0001, t0 + profile.durMs / 1000);
      osc.connect(g).connect(master);
      osc.start(t0);
      osc.stop(t0 + profile.durMs / 1000 + 0.05);
      nodes.push(osc);
    }
  }
  return {
    stop: () => {
      for (const n of nodes) {
        try { n.stop(); } catch { /* 已停止 */ }
      }
      master.disconnect();
    },
  };
}

/** 释放全局 AudioContext（测试与卸载用）。 */
export function disposeTheaterSound(): void {
  void ctxRef?.close().catch(() => undefined);
  ctxRef = null;
}
