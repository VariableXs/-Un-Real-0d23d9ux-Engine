/**
 * A-4 声音设计：精选 6 音（宁缺毋滥），Web Audio 程序化合成（零资产文件、
 * 永不报错、无网络）。全部短促柔和 ≤ 400ms；音量独立可控 + 全局静音；
 * 勿扰模式自动静音（由调用方以 dnd 入参表达）。
 *
 * 6 音清单（蓝图 23.4）：
 *   boot   启动落定   notify  通知横幅   alarm  闹钟
 *   error  错误       snap    贴靠吸附   trash  回收站清空
 */

export type SoundName = "boot" | "notify" | "alarm" | "error" | "snap" | "trash";

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

/** 单音合成：柔和正弦 + 指数衰减包络；freq 起止、时长 ≤ 400ms。 */
function tone(ac: AudioContext, out: GainNode, f0: number, f1: number, dur: number, delay = 0, gain = 0.12): void {
  const t0 = ac.currentTime + delay;
  const osc = ac.createOscillator();
  const env = ac.createGain();
  osc.type = "sine";
  osc.frequency.setValueAtTime(f0, t0);
  osc.frequency.exponentialRampToValueAtTime(Math.max(40, f1), t0 + dur);
  env.gain.setValueAtTime(0.0001, t0);
  env.gain.exponentialRampToValueAtTime(gain, t0 + 0.015);
  env.gain.exponentialRampToValueAtTime(0.0001, t0 + dur);
  osc.connect(env).connect(out);
  osc.start(t0);
  osc.stop(t0 + dur + 0.02);
}

const RECIPES: Record<SoundName, (ac: AudioContext, out: GainNode) => void> = {
  // 启动落定：上行两音，温润收束
  boot: (ac, out) => {
    tone(ac, out, 330, 330, 0.16, 0, 0.10);
    tone(ac, out, 495, 470, 0.24, 0.10, 0.10);
  },
  // 通知横幅：清亮双击
  notify: (ac, out) => {
    tone(ac, out, 880, 900, 0.09, 0, 0.09);
    tone(ac, out, 1175, 1150, 0.14, 0.09, 0.08);
  },
  // 闹钟：三连短促同音
  alarm: (ac, out) => {
    for (let i = 0; i < 3; i++) tone(ac, out, 740, 700, 0.09, i * 0.12, 0.11);
  },
  // 错误：下行小二度，克制
  error: (ac, out) => {
    tone(ac, out, 392, 370, 0.16, 0, 0.10);
    tone(ac, out, 311, 300, 0.22, 0.12, 0.10);
  },
  // 贴靠吸附：极短上滑
  snap: (ac, out) => {
    tone(ac, out, 520, 780, 0.07, 0, 0.09);
  },
  // 回收站清空：下扫噪音感（正弦簇近似）
  trash: (ac, out) => {
    tone(ac, out, 620, 180, 0.30, 0, 0.09);
    tone(ac, out, 470, 140, 0.30, 0.05, 0.06);
  },
};

const MAX_MS = 400;
let lastPlay = 0;

/**
 * 播放系统音。muted = 全局静音；dnd = 勿扰（自动静音，闹钟除外——
 * 闹钟属用户主动约定，勿扰下仍响，与宿主行为一致）。
 * volume 0-1（settings.soundVolume）；同音 80ms 内去抖。
 */
export function playSound(name: SoundName, opts?: { volume?: number; muted?: boolean; dnd?: boolean }): void {
  const now = Date.now();
  if (now - lastPlay < 80) return;
  if (opts?.muted) return;
  if (opts?.dnd && name !== "alarm") return;
  const ac = audio();
  if (!ac) return;
  lastPlay = now;

  const vol = Math.min(1, Math.max(0, opts?.volume ?? 0.5));
  if (vol <= 0) return;

  const guard = ac.createGain();
  guard.gain.value = vol * 0.5; // 全局安全系数
  guard.connect(ac.destination);
  RECIPES[name](ac, guard);
  // 保险：400ms 后断开，杜绝悬挂节点
  window.setTimeout(() => guard.disconnect(), MAX_MS);
}
