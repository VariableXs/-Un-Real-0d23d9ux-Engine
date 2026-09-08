/**
 * A-4 声音设计 + AI-16 U-05 启动交响 / U-52 声景反馈 / Z-45 系统声音方案：
 * Web Audio 程序化合成（零资产文件、永不报错、无网络）。全部短促柔和 ≤ 400ms；
 * 音量独立可控 + 全局静音；勿扰模式自动静音（由调用方以 dnd 入参表达）。
 *
 * 10 音清单（A-4 六音 + U-52 四件）：
 *   boot 启动落定   notify 通知横幅   alarm 闹钟     error 错误
 *   snap 贴靠吸附   trash 回收站清空  open 打开      close 关闭
 *   window 窗口呈现 minimize 最小化
 *
 * U-52 声景主题：同一 RECIPES 在三套主题下做参数化变奏（freq/gain/decay 缩放），
 * 深夜（22:00–6:00）自动整体 ×0.5（soundNightDamp=false 可关）。
 */

import type { SoundThemeId } from "./settings";

export type SoundName =
  | "boot"
  | "notify"
  | "alarm"
  | "error"
  | "snap"
  | "trash"
  | "open"
  | "close"
  | "window"
  | "minimize";

export const SOUND_NAMES: SoundName[] = [
  "boot", "notify", "alarm", "error", "snap", "trash", "open", "close", "window", "minimize",
];

/** U-52 主题变奏参数（乘到基础配方上；0.5..2 的克制区间）。 */
export interface ThemeTuning {
  freqScale: number;
  gainScale: number;
  decayScale: number;
}

export type ThemeTunings = Partial<Record<SoundName, ThemeTuning>>;

/** U-52 三套预置 + Z-45「默认方案」（与后端 sound_scheme_validate 白名单对齐）。 */
export const SOUND_THEMES: Record<SoundThemeId, { label: string; tunings: ThemeTunings }> = {
  default: { label: "玻璃", tunings: {} },
  wood: {
    label: "木质",
    tunings: {
      snap: { freqScale: 0.72, gainScale: 1.15, decayScale: 0.6 }, // 实木吸附：低频短促
      trash: { freqScale: 0.6, gainScale: 1.1, decayScale: 0.75 }, // 纸张感
      notify: { freqScale: 0.82, gainScale: 1.0, decayScale: 1.1 },
      boot: { freqScale: 0.78, gainScale: 1.0, decayScale: 1.15 },
      open: { freqScale: 0.8, gainScale: 1.0, decayScale: 1.05 },
      close: { freqScale: 0.76, gainScale: 1.0, decayScale: 0.85 },
      window: { freqScale: 0.85, gainScale: 0.95, decayScale: 1.0 },
      minimize: { freqScale: 0.7, gainScale: 0.95, decayScale: 0.8 },
      alarm: { freqScale: 0.85, gainScale: 1.0, decayScale: 1.0 },
      error: { freqScale: 0.8, gainScale: 1.0, decayScale: 1.1 },
    },
  },
  midnight: {
    label: "暗夜",
    tunings: {
      snap: { freqScale: 0.6, gainScale: 0.6, decayScale: 1.2 },
      trash: { freqScale: 0.5, gainScale: 0.6, decayScale: 1.3 },
      notify: { freqScale: 0.65, gainScale: 0.7, decayScale: 1.25 },
      boot: { freqScale: 0.6, gainScale: 0.65, decayScale: 1.35 },
      open: { freqScale: 0.65, gainScale: 0.7, decayScale: 1.2 },
      close: { freqScale: 0.6, gainScale: 0.7, decayScale: 1.15 },
      window: { freqScale: 0.7, gainScale: 0.65, decayScale: 1.2 },
      minimize: { freqScale: 0.55, gainScale: 0.65, decayScale: 1.1 },
      alarm: { freqScale: 0.75, gainScale: 0.75, decayScale: 1.1 },
      error: { freqScale: 0.65, gainScale: 0.7, decayScale: 1.2 },
    },
  },
};

let ctx: AudioContext | null = null;

function audio(): AudioContext | null {
  try {
    if (!ctx) {
      const AC = window.AudioContext || (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
      if (!AC) return null;
      ctx = new AC();
    }
    if (ctx.state === "suspended") void ctx.resume().catch(() => {});
    return ctx;
  } catch {
    return null;
  }
}

/** 单音合成：柔和正弦 + 指数衰减包络；freq 起止、时长 ≤ 400ms。主题参数在此变奏。 */
function tone(
  ac: AudioContext,
  out: GainNode,
  f0: number,
  f1: number,
  dur: number,
  delay: number,
  gain: number,
  t?: ThemeTuning,
): void {
  const fs = t?.freqScale ?? 1;
  const gs = t?.gainScale ?? 1;
  const ds = t?.decayScale ?? 1;
  const t0 = ac.currentTime + delay;
  const osc = ac.createOscillator();
  const env = ac.createGain();
  osc.type = "sine";
  osc.frequency.setValueAtTime(Math.max(40, f0 * fs), t0);
  osc.frequency.exponentialRampToValueAtTime(Math.max(40, f1 * fs), t0 + dur * ds);
  env.gain.setValueAtTime(0.0001, t0);
  env.gain.exponentialRampToValueAtTime(Math.max(0.0002, gain * gs), t0 + 0.015);
  env.gain.exponentialRampToValueAtTime(0.0001, t0 + dur * ds);
  osc.connect(env).connect(out);
  osc.start(t0);
  osc.stop(t0 + dur * ds + 0.02);
}

const RECIPES: Record<SoundName, (ac: AudioContext, out: GainNode, t?: ThemeTuning) => void> = {
  // 启动落定：上行两音，温润收束
  boot: (ac, out, t) => {
    tone(ac, out, 330, 330, 0.16, 0, 0.10, t);
    tone(ac, out, 495, 470, 0.24, 0.10, 0.10, t);
  },
  // 通知横幅：清亮双击
  notify: (ac, out, t) => {
    tone(ac, out, 880, 900, 0.09, 0, 0.09, t);
    tone(ac, out, 1175, 1150, 0.14, 0.09, 0.08, t);
  },
  // 闹钟：三连短促同音
  alarm: (ac, out, t) => {
    for (let i = 0; i < 3; i++) tone(ac, out, 740, 700, 0.09, i * 0.12, 0.11, t);
  },
  // 错误：下行小二度，克制
  error: (ac, out, t) => {
    tone(ac, out, 392, 370, 0.16, 0, 0.10, t);
    tone(ac, out, 311, 300, 0.22, 0.12, 0.10, t);
  },
  // 贴靠吸附：极短上滑（木质主题下即"实木"）
  snap: (ac, out, t) => {
    tone(ac, out, 520, 780, 0.07, 0, 0.09, t);
  },
  // 回收站清空：下扫噪音感（正弦簇近似；木质主题下更"纸张"）
  trash: (ac, out, t) => {
    tone(ac, out, 620, 180, 0.30, 0, 0.09, t);
    tone(ac, out, 470, 140, 0.30, 0.05, 0.06, t);
  },
  // U-52 open：开启 — 轻盈上行
  open: (ac, out, t) => {
    tone(ac, out, 440, 660, 0.12, 0, 0.08, t);
  },
  // U-52 close：关闭 — 收拢下行
  close: (ac, out, t) => {
    tone(ac, out, 620, 420, 0.12, 0, 0.08, t);
  },
  // U-52 window：窗口呈现 — 双音立起
  window: (ac, out, t) => {
    tone(ac, out, 520, 560, 0.08, 0, 0.07, t);
    tone(ac, out, 700, 740, 0.10, 0.06, 0.06, t);
  },
  // U-52 minimize：最小化 — 快速滑落
  minimize: (ac, out, t) => {
    tone(ac, out, 700, 380, 0.14, 0, 0.07, t);
  },
};

const MAX_MS = 400;
let lastPlay = 0;

/** U-52 深夜（22:00–6:00）自动整体音量 ×0.5（hour 可注入以便单测）。 */
export function nightFactor(hour: number, damp: boolean): number {
  if (!damp) return 1;
  return hour >= 22 || hour < 6 ? 0.5 : 1;
}

/**
 * 播放系统音。muted = 全局静音；dnd = 勿扰（自动静音，闹钟除外——
 * 闹钟属用户主动约定，勿扰下仍响，与宿主行为一致）。
 * volume 0-1（settings.soundVolume）；同音 80ms 内去抖；
 * theme = U-52 声景主题（default 时用基础配方）。
 */
export function playSound(
  name: SoundName,
  opts?: { volume?: number; muted?: boolean; dnd?: boolean; theme?: SoundThemeId; nightDamp?: boolean },
): void {
  const now = Date.now();
  if (now - lastPlay < 80) return;
  if (opts?.muted) return;
  if (opts?.dnd && name !== "alarm") return;
  const ac = audio();
  if (!ac) return;
  lastPlay = now;

  const theme = opts?.theme ?? "default";
  const tuning = SOUND_THEMES[theme]?.tunings[name];
  const night = nightFactor(new Date().getHours(), opts?.nightDamp ?? true);
  const vol = Math.min(1, Math.max(0, (opts?.volume ?? 0.5) * night));
  if (vol <= 0) return;

  const guard = ac.createGain();
  guard.gain.value = vol * 0.5; // 全局安全系数
  guard.connect(ac.destination);
  RECIPES[name](ac, guard, tuning);
  // 保险：400ms 后断开，杜绝悬挂节点
  window.setTimeout(() => guard.disconnect(), MAX_MS);
}

// ---------------------------------------------------------------------------
// U-05 启动交响（三层：低频铺底 → 节拍脉冲 → 就绪双音）
// ---------------------------------------------------------------------------

export interface BootSymphony {
  /** 30%/80% 进度跨越时调用：柔和 tick（与胶囊端点呼吸同相位）。 */
  pulse(): void;
  /** ready 事件到达时调用：上行双音 C5→G5。 */
  ready(): void;
  stop(): void;
}

/** 获取主题化启动音量（0 剥离出纯逻辑便于复用/测试的参数化包装）。 */
function symphonyVolume(base: number, theme: SoundThemeId, hour: number, nightDamp: boolean): number {
  return Math.min(1, Math.max(0, base * nightFactor(hour, nightDamp) * (theme === "midnight" ? 0.7 : 1)));
}

/**
 * U-05 启动交响：随真实进度递进的三层启动音效（全部实时合成，无音频文件资产）。
 * - 0–30% 低频铺底：55Hz 正弦缓升（音量 20%），enter 时开始；
 * - 30–80% 节拍脉冲：每 2s 一次柔和 tick（pulse() 由进度驱动调用）；
 * - 100% 就绪瞬间：上行双音 C5→G5（各 180ms，60ms 释放）。
 * 启动音永远尊重静音（soundMuted / bootSoundMode=mute 时零声音）。
 */
export function startBootSymphony(opts: {
  volume: number;
  muted: boolean;
  mode: "full" | "mute" | "chime-only";
  theme?: SoundThemeId;
  nightDamp?: boolean;
}): BootSymphony | null {
  if (opts.muted || opts.mode === "mute") return null;
  const ac = audio();
  if (!ac) return null;

  const theme = opts.theme ?? "default";
  const tuning = SOUND_THEMES[theme].tunings.boot;
  const vol = symphonyVolume(opts.volume, theme, new Date().getHours(), opts.nightDamp ?? true);
  if (vol <= 0) return null;

  const nodes: { stop: () => void }[] = [];
  let stopped = false;

  if (opts.mode === "full") {
    // 低频铺底：55Hz 正弦缓升（音量 20%），直到 ready/stop
    const osc = ac.createOscillator();
    const env = ac.createGain();
    osc.type = "sine";
    osc.frequency.setValueAtTime(55, ac.currentTime);
    osc.frequency.linearRampToValueAtTime(66, ac.currentTime + 12);
    env.gain.setValueAtTime(0.0001, ac.currentTime);
    env.gain.exponentialRampToValueAtTime(0.2 * vol, ac.currentTime + 0.4);
    osc.connect(env).connect(ac.destination);
    osc.start();
    nodes.push({ stop: () => { try { osc.stop(); } catch { /* 已停 */ } env.disconnect(); } });
  }

  const chime = (): void => {
    if (stopped) return;
    const guard = ac.createGain();
    guard.gain.value = vol * 0.5;
    guard.connect(ac.destination);
    tone(ac, guard, 523, 523, 0.18, 0, 0.12, tuning); // C5
    tone(ac, guard, 784, 760, 0.18, 0.14, 0.12, tuning); // G5（60ms 释放交叠）
    window.setTimeout(() => guard.disconnect(), MAX_MS);
  };

  return {
    pulse(): void {
      if (stopped) return;
      const guard = ac.createGain();
      guard.gain.value = vol * 0.5 * 0.6;
      guard.connect(ac.destination);
      tone(ac, guard, 110, 104, 0.06, 0, 0.06, tuning);
      window.setTimeout(() => guard.disconnect(), MAX_MS);
    },
    ready(): void {
      if (stopped) return;
      chime();
    },
    stop(): void {
      if (stopped) return;
      stopped = true;
      for (const n of nodes) n.stop();
    },
  };
}
