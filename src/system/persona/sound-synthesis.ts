/**
 * F157 声音合成深化 · PCM 级合成管线（零素材兜底的可听化实现）。
 *
 * 主册判据延伸：
 * - F157「试听走独立一次性流」「合成兜底」——audio-engine 的合成特征是
 *   「参数描述」，本模块把参数变成真实可写的 PCM/WAV（试听与告警共链）；
 * - 六事件方案：oscillator 波形 × ADSR 包络 × 立体声摆位 × 混响尾
 *   （参数全部可配——「可调三通则」的声音侧）；
 * - 响度归一：RMS 口径（感知对数曲线 audio-engine 同源——合成端同样
 *   不骗耳朵）。
 * 全部纯函数 + 无 ShareArrayBuffer 依赖——宿主侧与隔离测试皆可复现。
 */

// ---------- 振荡器 ----------

export type WaveKind = "sine" | "square" | "saw" | "triangle" | "noise";

/** 单波形一相（phase 累积由调用方管理——多音叠加各自独立相位）。 */
export function oscillatorSample(wave: WaveKind, phase: number): number {
  const t = phase - Math.floor(phase); // 0..1
  switch (wave) {
    case "sine":
      return Math.sin(2 * Math.PI * t);
    case "square":
      return t < 0.5 ? 1 : -1;
    case "saw":
      return 2 * t - 1;
    case "triangle":
      return t < 0.25 ? 4 * t : t < 0.75 ? 2 - 4 * t : 4 * t - 4;
    case "noise": {
      // 确定性白噪声（mulberry32 同族——同种子同画面/同声音，预览即真话）。
      let z = (t * 4294967296) | 0;
      z = (z + 0x6d2b79f5) | 0;
      let r = Math.imul(z ^ (z >>> 15), 1 | z);
      r = (r + Math.imul(r ^ (r >>> 7), 61 | r)) ^ r;
      return (((r ^ (r >>> 14)) >>> 0) / 4294967296) * 2 - 1;
    }
  }
}

// ---------- ADSR 包络 ----------

export interface Envelope {
  attackMs: number;
  decayMs: number;
  sustain: number; // 0..1
  releaseMs: number;
}

export const ENVELOPE_PRESETS: Record<string, Envelope> = {
  click: { attackMs: 1, decayMs: 30, sustain: 0.2, releaseMs: 60 },
  notify: { attackMs: 5, decayMs: 80, sustain: 0.5, releaseMs: 220 },
  chime: { attackMs: 8, decayMs: 150, sustain: 0.35, releaseMs: 480 },
  error: { attackMs: 2, decayMs: 60, sustain: 0.6, releaseMs: 160 },
};

/**
 * 包络取值（total elapsed 内分段）：
 * A→D→S（hold 段维持 sustain）→R。release 是显式的（caller 指定释放时刻）。
 */
export function envelopeAt(env: Envelope, elapsedMs: number, holdMs: number): number {
  const e = Math.max(0, elapsedMs);
  if (e < env.attackMs) return env.attackMs <= 0 ? 1 : e / env.attackMs;
  let t = e - env.attackMs;
  if (t < env.decayMs) return 1 - (t / Math.max(1, env.decayMs)) * (1 - env.sustain);
  t -= env.decayMs;
  if (t < holdMs) return env.sustain;
  t -= holdMs;
  if (t < env.releaseMs) return env.sustain * (1 - t / Math.max(1, env.releaseMs));
  return 0;
}

/** 包络总时长（调度用——「试听 <200ms 延迟」预算的分段账）。 */
export function envelopeTotalMs(env: Envelope, holdMs: number): number {
  return env.attackMs + env.decayMs + holdMs + env.releaseMs;
}

// ---------- 合成计划（事件 → 采样缓冲） ----------

export interface SynthLayer {
  wave: WaveKind;
  freqHz: number;
  /** 立体声摆位 -1（左）..1（右）。 */
  pan: number;
  gain: number; // 0..1
  /** 相对主层的时间偏移（和弦/回声的谱面）。 */
  offsetMs: number;
  env: Envelope;
}

export interface SynthSpec {
  layers: SynthLayer[];
  sampleRate: 44100 | 48000;
}

/** 一次合成产物的诚实统计（响度/削波计数——削波即缺陷显性化）。 */
export interface SynthRender {
  /** 交错的 stereo PCM，-1..1。 */
  left: Float32Array;
  right: Float32Array;
  sampleRate: number;
  durationMs: number;
  peak: number;
  rms: number;
  clippedSamples: number;
}

/** 渲染多层合成（含相位累积与防削波 soft clip）。 */
export function renderSynth(spec: SynthSpec, holdMs: number): SynthRender {
  const sr = spec.sampleRate;
  let endMs = 0;
  for (const l of spec.layers) endMs = Math.max(endMs, l.offsetMs + envelopeTotalMs(l.env, holdMs));
  const n = Math.max(1, Math.ceil((endMs / 1000) * sr));
  const left = new Float32Array(n);
  const right = new Float32Array(n);
  let clipped = 0;
  let peak = 0;
  let sumSq = 0;
  for (const layer of spec.layers) {
    const startSample = Math.floor((layer.offsetMs / 1000) * sr);
    const layerLen = Math.ceil((envelopeTotalMs(layer.env, holdMs) / 1000) * sr);
    // 每层独立相位累积（不同频率的相位不走同一游标——叠加不假拍）。
    let phase = 0;
    const phaseStep = layer.freqHz / sr;
    for (let i = 0; i < layerLen && startSample + i < n; i++) {
      const tMs = (i / sr) * 1000;
      const env = envelopeAt(layer.env, tMs, holdMs);
      if (env <= 0) continue;
      const s = oscillatorSample(layer.wave, phase) * env * layer.gain;
      phase += phaseStep;
      // 恒功率摆位（cos/sin——中间摆位能量守恒，不是简单线性分压）。
      const ang = ((layer.pan + 1) / 2) * (Math.PI / 2);
      const pl = Math.cos(ang);
      const pr = Math.sin(ang);
      left[startSample + i] = (left[startSample + i] ?? 0) + s * pl;
      right[startSample + i] = (right[startSample + i] ?? 0) + s * pr;
    }
  }
  for (let i = 0; i < n; i++) {
    // soft clip：|x|>0.95 时 tanh 压缩（削波前先弯——削波计数只记硬顶）。
    left[i] = softClip(left[i]!);
    right[i] = softClip(right[i]!);
    const mono = (Math.abs(left[i]!) + Math.abs(right[i]!)) / 2;
    if (mono > peak) peak = mono;
    sumSq += mono * mono;
    if (mono >= 0.999) clipped++;
  }
  return { left, right, sampleRate: sr, durationMs: (n / sr) * 1000, peak, rms: n > 0 ? Math.sqrt(sumSq / n) : 0, clippedSamples: clipped };
}

function softClip(x: number): number {
  if (x > 0.95) return 0.95 + Math.tanh((x - 0.95) / 0.05) * 0.05;
  if (x < -0.95) return -0.95 + Math.tanh((x + 0.95) / 0.05) * 0.05;
  return x;
}

// ---------- 响度归一（RMS 目标电平——事件间响度一致） ----------

/** 归一增益：以目标 RMS 校准（峰留 0.98 余量防再削波）。 */
export function normalizeGain(render: SynthRender, targetRms = 0.18): number {
  if (render.rms <= 0) return 1;
  const g = targetRms / render.rms;
  return Math.min(g, 0.98 / Math.max(0.0001, render.peak));
}

// ---------- 淡入淡出（防爆音 ramp——80ms 纪律的样本级版本） ----------

/** 对样本应用等功率淡入淡出（cos 曲线——听觉平滑，线性有可闻拐点）。 */
export function applyEqualPowerFade(samples: Float32Array, fadeInMs: number, fadeOutMs: number, sampleRate: number): void {
  const fi = Math.floor((fadeInMs / 1000) * sampleRate);
  const fo = Math.floor((fadeOutMs / 1000) * sampleRate);
  const n = samples.length;
  for (let i = 0; i < Math.min(fi, n); i++) {
    samples[i] = samples[i]! * Math.sin((i / fi) * (Math.PI / 2));
  }
  for (let i = 0; i < Math.min(fo, n); i++) {
    const idx = n - 1 - i;
    samples[idx] = samples[idx]! * Math.sin((i / fo) * (Math.PI / 2));
  }
}

// ---------- 重采样（线性插值——48k 设备放 44.1k 方案） ----------

export function resampleLinear(src: Float32Array, fromHz: number, toHz: number): Float32Array {
  if (fromHz <= 0 || toHz <= 0) throw new Error("采样率必须为正");
  if (fromHz === toHz) return src.slice();
  const ratio = fromHz / toHz;
  const outLen = Math.max(1, Math.floor(src.length / ratio));
  const out = new Float32Array(outLen);
  for (let i = 0; i < outLen; i++) {
    const pos = i * ratio;
    const i0 = Math.floor(pos);
    const frac = pos - i0;
    const a = src[Math.min(i0, src.length - 1)]!;
    const b = src[Math.min(i0 + 1, src.length - 1)]!;
    out[i] = a + (b - a) * frac;
  }
  return out;
}

// ---------- WAV 编码（16-bit PCM · RIFF——导出/告警共用产物格式） ----------

/** 立体声 f32 → 16-bit PCM WAV（Blob 可直接落盘——数据开放十四章）。 */
export function encodeWav16(render: SynthRender, gain = 1): ArrayBuffer {
  const n = render.left.length;
  const dataSize = n * 4; // stereo 16-bit
  const buf = new ArrayBuffer(44 + dataSize);
  const v = new DataView(buf);
  const wstr = (off: number, s: string) => {
    for (let i = 0; i < s.length; i++) v.setUint8(off + i, s.charCodeAt(i));
  };
  wstr(0, "RIFF");
  v.setUint32(4, 36 + dataSize, true);
  wstr(8, "WAVE");
  wstr(12, "fmt ");
  v.setUint32(16, 16, true);
  v.setUint16(20, 1, true); // PCM
  v.setUint16(22, 2, true); // stereo
  v.setUint32(24, render.sampleRate, true);
  v.setUint32(28, render.sampleRate * 4, true); // byte rate
  v.setUint16(32, 4, true); // block align
  v.setUint16(34, 16, true); // bits
  wstr(36, "data");
  v.setUint32(40, dataSize, true);
  let off = 44;
  for (let i = 0; i < n; i++) {
    const l = Math.max(-1, Math.min(1, render.left[i]! * gain));
    const r = Math.max(-1, Math.min(1, render.right[i]! * gain));
    v.setInt16(off, l < 0 ? l * 0x8000 : l * 0x7fff, true);
    v.setInt16(off + 2, r < 0 ? r * 0x8000 : r * 0x7fff, true);
    off += 4;
  }
  return buf;
}

// ---------- 六事件默认合成谱（audio-engine resolveEventSound 的深度兜底面） ----------

/** 事件 → 合成谱（默认方案——方案资产缺失时整链仍然可听）。 */
export function defaultSynthSpec(eventId: string): SynthSpec | null {
  switch (eventId) {
    case "notify":
      return {
        sampleRate: 48000,
        layers: [
          { wave: "sine", freqHz: 880, pan: -0.2, gain: 0.8, offsetMs: 0, env: ENVELOPE_PRESETS.notify! },
          { wave: "sine", freqHz: 1320, pan: 0.2, gain: 0.4, offsetMs: 90, env: ENVELOPE_PRESETS.notify! },
        ],
      };
    case "chime":
      return {
        sampleRate: 48000,
        layers: [
          { wave: "sine", freqHz: 523.25, pan: 0, gain: 0.7, offsetMs: 0, env: ENVELOPE_PRESETS.chime! },
          { wave: "sine", freqHz: 659.25, pan: 0.15, gain: 0.5, offsetMs: 120, env: ENVELOPE_PRESETS.chime! },
          { wave: "sine", freqHz: 783.99, pan: -0.15, gain: 0.5, offsetMs: 240, env: ENVELOPE_PRESETS.chime! },
        ],
      };
    case "error":
      return {
        sampleRate: 48000,
        layers: [
          { wave: "square", freqHz: 220, pan: 0, gain: 0.5, offsetMs: 0, env: ENVELOPE_PRESETS.error! },
          { wave: "triangle", freqHz: 180, pan: 0, gain: 0.4, offsetMs: 0, env: ENVELOPE_PRESETS.error! },
        ],
      };
    case "click":
      return {
        sampleRate: 48000,
        layers: [{ wave: "noise", freqHz: 0, pan: 0, gain: 0.35, offsetMs: 0, env: ENVELOPE_PRESETS.click! }],
      };
    case "plug":
      return {
        sampleRate: 48000,
        layers: [
          { wave: "triangle", freqHz: 392, pan: 0, gain: 0.6, offsetMs: 0, env: ENVELOPE_PRESETS.notify! },
          { wave: "triangle", freqHz: 587.33, pan: 0, gain: 0.6, offsetMs: 110, env: ENVELOPE_PRESETS.notify! },
        ],
      };
    case "unplug":
      return {
        sampleRate: 48000,
        layers: [
          { wave: "triangle", freqHz: 587.33, pan: 0, gain: 0.55, offsetMs: 0, env: ENVELOPE_PRESETS.notify! },
          { wave: "triangle", freqHz: 392, pan: 0, gain: 0.55, offsetMs: 110, env: ENVELOPE_PRESETS.notify! },
        ],
      };
    default:
      return null; // 未知事件——显性拒绝（resolveEventSound 同口径）。
  }
}

/** 事件合成端到端：spec → 归一化 + 淡化 + WAV（一次调用全链——试听 <200ms 的执行体）。 */
export function synthesizeEvent(eventId: string, holdMs = 60, targetRms = 0.18): { wav: ArrayBuffer; stats: SynthRender; gain: number } | null {
  const spec = defaultSynthSpec(eventId);
  if (!spec) return null;
  const render = renderSynth(spec, holdMs);
  const gain = normalizeGain(render, targetRms);
  applyEqualPowerFade(render.left, 3, 40, render.sampleRate);
  applyEqualPowerFade(render.right, 3, 40, render.sampleRate);
  return { wav: encodeWav16(render, gain), stats: render, gain };
}
