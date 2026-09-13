/**
 * UNREAL-X AI-02 · 族0019 启动降噪（X00426 档 · 启动期通知/动画降噪档位）。
 *
 * 四档降噪（off=现状）：启动窗口期内对通知/动效/声音做抑制决策；
 * 窗口时长钳制，逐项决策可解释。纯逻辑模块。
 */

export const QUIET_TIERS = [
  { id: "off", name: "关闭（现状）", level: 0, suppressNotify: false, suppressMotion: false, suppressSound: false },
  { id: "gentle", name: "轻柔", level: 1, suppressNotify: false, suppressMotion: false, suppressSound: true },
  { id: "hushed", name: "静谧", level: 2, suppressNotify: true, suppressMotion: false, suppressSound: true },
  { id: "mute", name: "全静", level: 3, suppressNotify: true, suppressMotion: true, suppressSound: true },
] as const;
export type QuietTierId = (typeof QUIET_TIERS)[number]["id"];
export const DEFAULT_QUIET_ID: QuietTierId = "off";

export function findQuiet(id: string): (typeof QUIET_TIERS)[number] {
  return QUIET_TIERS.find((t) => t.id === id) ?? QUIET_TIERS[0]!;
}

/** 启动降噪窗口时长（秒）钳制 0~300，默认 60。 */
export const QUIET_WINDOW_MIN = 0;
export const QUIET_WINDOW_MAX = 300;
export const QUIET_WINDOW_DEFAULT = 60;

export function clampQuietWindow(sec: number): number {
  if (!Number.isFinite(sec)) return QUIET_WINDOW_DEFAULT;
  return Math.min(QUIET_WINDOW_MAX, Math.max(QUIET_WINDOW_MIN, Math.round(sec)));
}

/** 降噪控制器。 */
export class BootQuiet {
  tierId: QuietTierId;
  windowSec: number;
  clamped = 0;
  /** 启动已流逝秒数（tick 累加）。 */
  elapsedSec = 0;

  constructor(tierId: string = DEFAULT_QUIET_ID, windowSec = QUIET_WINDOW_DEFAULT) {
    const t = findQuiet(tierId);
    if (t.id !== tierId) this.clamped += 1;
    this.tierId = t.id as QuietTierId;
    this.windowSec = clampQuietWindow(windowSec);
  }

  get tier(): (typeof QUIET_TIERS)[number] {
    return findQuiet(this.tierId);
  }

  /** 是否仍在启动窗口内。 */
  inWindow(): boolean {
    return this.elapsedSec < this.windowSec;
  }

  tick(sec = 1): void {
    this.elapsedSec += Math.max(0, sec);
  }

  /** 抑制决策（窗口外一律放行；每项决策可解释）。 */
  decision(kind: "notify" | "motion" | "sound"): { suppressed: boolean; reason: string } {
    const t = this.tier;
    if (!this.inWindow()) return { suppressed: false, reason: "启动窗口已结束，正常放行" };
    if (t.level === 0) return { suppressed: false, reason: "降噪关闭（现状）" };
    const flag = kind === "notify" ? t.suppressNotify : kind === "motion" ? t.suppressMotion : t.suppressSound;
    return {
      suppressed: flag,
      reason: flag ? `${t.name}档在启动窗口内抑制${kind === "notify" ? "通知" : kind === "motion" ? "动效" : "提示音"}` : `${t.name}档不抑制该项`,
    };
  }

  /** 摘要（设置页直接展示）。 */
  caption(): string {
    return `${this.tier.name} · 窗口 ${this.windowSec}s`;
  }

  reset(): void {
    this.elapsedSec = 0;
    this.clamped = 0;
  }
}
