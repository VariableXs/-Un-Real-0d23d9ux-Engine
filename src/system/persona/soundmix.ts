/**
 * F157 声音混合器 · 完整设计。
 *
 * 主册判据：逐事件音量独立生效实测（六事件六档位）；总闸 100% 压制；试听延迟 <200ms。
 *
 * 【功能定义】事件音效逐项音量独立调节+试听+「全部静音」总闸；音效方案（F079
 * 三官方+社区）管理；应用混音器（F026 逐流音量）入口聚合。
 *
 * 【状态与异常】方案缺某事件音 → 该行标「沿用默认」；试听设备缺失 → 按钮灰置
 * 说明；总闸开时试听也无声（总闸语义一致——诚实）。
 *
 * 【设计细节】音量曲线用感知对数（slider 50%=实际半响——线性杆骗耳朵）；试听走
 * 独立一次性流（不污染混音器记忆）；滑杆 2% 步进；总闸关闭时整页降透明+「已全局
 * 静音」条。
 */

import { perceptualVolume, personaStore } from "./store";

export const SECTION = "sound";
export const SLIDER_STEP = 0.02; // 2% 步进（F075 同族）
export const PREVIEW_BUDGET_MS = 200;

/** 六事件（F079 映射表——一处一事实与 F079 同源）。 */
export const SOUND_EVENTS: readonly { id: string; zh: string; en: string }[] = [
  { id: "boot", zh: "开机", en: "Boot" },
  { id: "notify", zh: "通知", en: "Notification" },
  { id: "usb-connect", zh: "接入设备", en: "Device connect" },
  { id: "usb-eject", zh: "移除设备", en: "Device eject" },
  { id: "battery-low", zh: "电量不足", en: "Battery low" },
  { id: "error", zh: "错误", en: "Error" },
] as const;

export const SOUND_EVENT_COUNT = 6;

export interface SoundEventConfig {
  /** 方案 id；null=官方默认方案。 */
  schemeId: string | null;
  /** 滑杆值 0..1（2% 步进）。 */
  slider: number;
}

export interface SoundMixerConfig {
  /** 总闸（true=全部静音）。 */
  masterMute: boolean;
  events: Record<string, SoundEventConfig>;
}

export function defaultSoundMixerConfig(): SoundMixerConfig {
  const events: Record<string, SoundEventConfig> = {};
  for (const e of SOUND_EVENTS) events[e.id] = { schemeId: null, slider: 1 };
  return { masterMute: false, events };
}

export function loadSoundMixerConfig(): SoundMixerConfig {
  const stored = personaStore.getWith(SECTION, "mixer", undefined) as Partial<SoundMixerConfig> | undefined;
  const d = defaultSoundMixerConfig();
  if (!stored) return d;
  return {
    masterMute: typeof stored.masterMute === "boolean" ? stored.masterMute : false,
    events: { ...d.events, ...(typeof stored.events === "object" && stored.events !== null ? stored.events : {}) },
  };
}

export function saveSoundMixerConfig(c: SoundMixerConfig): void {
  personaStore.set(SECTION, { mixer: c });
}

/** 滑杆钳制到 2% 步进栅格（0..1）。 */
export function clampSlider(v: number): number {
  const clamped = Math.min(1, Math.max(0, v));
  return Math.round(clamped / SLIDER_STEP) * SLIDER_STEP;
}

/**
 * 最终响度：总闸=0（100% 压制）；否则感知对数曲线（slider 0.5 → perceptual 0.25）。
 * 逐事件独立——六事件六档位互不串扰。
 */
export function effectiveVolume(config: SoundMixerConfig, eventId: string): number {
  if (config.masterMute) return 0;
  const ev = config.events[eventId];
  if (!ev) return 1;
  return perceptualVolume(ev.slider);
}

/** 方案缺某事件音判定：该行标「沿用默认」。 */
export function schemeLacksEvent(schemeSounds: Record<string, boolean>, eventId: string): boolean {
  return schemeSounds[eventId] !== true;
}
