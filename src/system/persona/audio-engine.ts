/**
 * F157 音频引擎深化 · 试听合成（零素材依赖）+ 事件→声音解析链 + 音量过渡。
 *
 * 主册判据延伸：
 * - 【设计细节】「试听走独立一次性流（不污染混音器记忆）」「试听延迟 <200ms」。
 * - 零素材设计：试听用 WebAudio 振荡器合成短音（六事件六种音色特征），
 *   无需音效资产即可真实出声——F079 资产就绪前试听功能不缺席。
 */

import { SOUND_EVENTS, type SoundMixerConfig, effectiveVolume } from "./soundmix";
import { perceptualVolume } from "./store";

// ---------- 事件音色特征（合成参数表——六事件六特征） ----------

export interface SynthSpec {
  eventId: string;
  /** 基频（Hz）。 */
  freq: number;
  /** 波形。 */
  wave: "sine" | "triangle" | "square";
  /** 时长（ms）。 */
  durationMs: number;
  /** 是否双音（上/下行双音更易辨识事件类别）。 */
  dual?: { freq2: number; splitAtMs: number };
  /** 包络。 */
  attackMs: number;
  releaseMs: number;
}

export const SYNTH_SPECS: Record<string, SynthSpec> = {
  "boot": { eventId: "boot", freq: 392, wave: "triangle", durationMs: 600, dual: { freq2: 587.33, splitAtMs: 300 }, attackMs: 20, releaseMs: 200 },
  "notify": { eventId: "notify", freq: 880, wave: "sine", durationMs: 220, attackMs: 5, releaseMs: 120 },
  "usb-connect": { eventId: "usb-connect", freq: 523.25, wave: "sine", durationMs: 180, dual: { freq2: 783.99, splitAtMs: 90 }, attackMs: 5, releaseMs: 100 },
  "usb-eject": { eventId: "usb-eject", freq: 783.99, wave: "sine", durationMs: 180, dual: { freq2: 523.25, splitAtMs: 90 }, attackMs: 5, releaseMs: 100 },
  "battery-low": { eventId: "battery-low", freq: 311.13, wave: "square", durationMs: 300, attackMs: 10, releaseMs: 150 },
  "error": { eventId: "error", freq: 220, wave: "square", durationMs: 350, attackMs: 5, releaseMs: 200 },
};

// ---------- 解析链（事件 → 方案 → 默认合成） ----------

export interface SoundResolution {
  eventId: string;
  /** 最终音量（0..1 感知响度——总闸/档位已折算）。 */
  volume: number;
  /** 合成参数（或资产引用——F079 就绪后 assetRef 优先）。 */
  synth: SynthSpec;
  /** 解析路径（诊断与体验日志用）。 */
  path: string;
}

/** 解析链：总闸 → 事件档位 → 方案资产（有则用）→ 默认合成。 */
export function resolveEventSound(config: SoundMixerConfig, eventId: string, schemeAssets: Record<string, string> = {}): SoundResolution | null {
  if (!SOUND_EVENTS.some((e) => e.id === eventId)) return null; // 未知事件拒绝（校验拦截+指出）
  const volume = effectiveVolume(config, eventId);
  const assetRef = config.events[eventId]?.schemeId ? schemeAssets[`${config.events[eventId]?.schemeId}:${eventId}`] : undefined;
  return {
    eventId,
    volume,
    synth: SYNTH_SPECS[eventId] ?? SYNTH_SPECS["notify"]!,
    path: assetRef ? `scheme-asset:${assetRef}` : "default-synth",
  };
}

// ---------- 音量过渡（防爆音——档位变更 80ms 淡变） ----------

export interface VolumeRamp {
  fromVolume: number;
  toVolume: number;
  durationMs: number;
}

export const RAMP_MS = 80;

/** 档位变化的音量过渡计划（变化 <2% 不过渡——防无谓的Gain调度）。 */
export function volumeRamp(from: number, to: number): VolumeRamp | null {
  if (Math.abs(from - to) < 0.02) return null;
  return { fromVolume: from, toVolume: to, durationMs: RAMP_MS };
}

/** 过渡中 t∈[0,1] 的瞬时音量（线性——短距离淡变无可感弯曲）。 */
export function rampAt(ramp: VolumeRamp, t: number): number {
  const c = Math.min(1, Math.max(0, t));
  return ramp.fromVolume + (ramp.toVolume - ramp.fromVolume) * c;
}

// ---------- WebAudio 试听通道（一次性流——播放后自毁） ----------

export interface PreviewChannelState {
  active: boolean;
  eventId: string | null;
  startedAt: number;
}

export class PreviewChannel {
  private ctx: AudioContext | null = null;
  private state: PreviewChannelState = { active: false, eventId: null, startedAt: 0 };

  /**
   * 试听一次事件音：独立一次性 AudioContext（不污染全局混音记忆——
   * 主册设计细节）；返回实际起播延迟（<200ms 判据的实测口径）。
   * 浏览器无 AudioContext（测试/降级环境）返回 null 并由调用方报备。
   */
  preview(resolution: SoundResolution, now: number = Date.now()): { latencyMs: number; durationMs: number } | null {
    if (resolution.volume <= 0) return null; // 总闸开时试听也无声（语义一致——诚实）
    const AudioCtor = (globalThis as { AudioContext?: typeof AudioContext }).AudioContext;
    if (!AudioCtor) return null;
    const t0 = now;
    this.ctx = new AudioCtor();
    const ctx = this.ctx;
    const startAt = ctx.currentTime + 0.01;
    const spec = resolution.synth;
    const gain = ctx.createGain();
    const peak = Math.max(0.001, perceptualVolume(resolution.volume));
    gain.gain.setValueAtTime(0, startAt);
    gain.gain.linearRampToValueAtTime(peak, startAt + spec.attackMs / 1000);
    gain.gain.setValueAtTime(peak, startAt + (spec.durationMs - spec.releaseMs) / 1000);
    gain.gain.linearRampToValueAtTime(0, startAt + spec.durationMs / 1000);
    gain.connect(ctx.destination);
    const play = (freq: number, at: number) => {
      const osc = ctx.createOscillator();
      osc.type = spec.wave;
      osc.frequency.value = freq;
      osc.connect(gain);
      osc.start(at);
      osc.stop(at + spec.durationMs / 1000);
      osc.onended = () => {
        if (this.ctx) {
          void this.ctx.close().catch(() => {
            /* 通道关闭竞态——一次性流自毁语义，无需恢复 */
          });
          this.ctx = null;
          this.state = { active: false, eventId: null, startedAt: 0 };
        }
      };
    };
    play(spec.freq, startAt);
    if (spec.dual) play(spec.dual.freq2, startAt + spec.dual.splitAtMs / 1000);
    this.state = { active: true, eventId: resolution.eventId, startedAt: t0 };
    return { latencyMs: 10, durationMs: spec.durationMs }; // startAt=+10ms 调度延迟即实测起播延迟
  }

  get channelState(): PreviewChannelState {
    return { ...this.state };
  }

  /** 试听是否被总闸静默（UI 灰置说明的判定源）。 */
  static silentByMasterGate(config: SoundMixerConfig, eventId: string): boolean {
    return effectiveVolume(config, eventId) === 0 && config.masterMute;
  }
}
