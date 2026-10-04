/**
 * UNREAL-X AI-01 · 族0007 启动配速学（X00151~X00175）。
 *
 * 节奏档配置：五档启动配速（稳妥/均衡/疾速/竞速/静默），每档给出
 * 倒计时秒数、跳过时机、动效等级；档间迁移平滑、选择可记忆、
 * 非法档位与越界数值全部钳制回默认。
 */

export interface PacingProfile {
  id: string;
  name: string;
  /** 引导菜单倒计时（秒，0 = 不等待）。 */
  countdownSec: number;
  /** 该秒数后可跳过剩余动效。 */
  skipAfterSec: number;
  /** 动效等级 0~3。 */
  motionLevel: number;
  /** 静默启动（无品牌演出）。 */
  silent: boolean;
}

/** 五档配速矩阵（≥5 档独立可交付，默认 = balanced）。 */
export const PACING_PROFILES: readonly PacingProfile[] = [
  { id: "steady", name: "稳妥", countdownSec: 8, skipAfterSec: 8, motionLevel: 3, silent: false },
  { id: "balanced", name: "均衡", countdownSec: 3, skipAfterSec: 3, motionLevel: 2, silent: false },
  { id: "swift", name: "疾速", countdownSec: 1, skipAfterSec: 1, motionLevel: 1, silent: false },
  { id: "sprint", name: "竞速", countdownSec: 0, skipAfterSec: 0, motionLevel: 0, silent: false },
  { id: "mute", name: "静默", countdownSec: 0, skipAfterSec: 0, motionLevel: 0, silent: true },
] as const;

export const DEFAULT_PACING_ID = "balanced";

export function findPacing(id: string): PacingProfile {
  return PACING_PROFILES.find((p) => p.id === id) ?? PACING_PROFILES[1]!;
}

/** 倒计时安全边界：0~30 秒，非法回默认档值。 */
export const COUNTDOWN_MIN = 0;
export const COUNTDOWN_MAX = 30;

export function clampCountdown(sec: number): number {
  if (!Number.isFinite(sec)) return findPacing(DEFAULT_PACING_ID).countdownSec;
  return Math.min(COUNTDOWN_MAX, Math.max(COUNTDOWN_MIN, Math.round(sec)));
}

/** 动效等级钳制：0~3；reduce-motion 时强制 ≤1。 */
export function clampMotion(level: number, reduceMotion = false): number {
  const v = Math.min(3, Math.max(0, Math.round(level) || 0));
  return reduceMotion ? Math.min(v, 1) : v;
}

/** 配速编辑器：在档位基础上微调并记忆（越界钳制）。 */
export class PacingMixer {
  profileId: string;
  countdownSec: number;
  motionLevel: number;
  clamped = 0;

  constructor(profileId: string = DEFAULT_PACING_ID) {
    const p = findPacing(profileId);
    if (p.id !== profileId) this.clamped += 1;
    this.profileId = p.id;
    this.countdownSec = p.countdownSec;
    this.motionLevel = p.motionLevel;
  }

  /** 切档：档位全量参数落地（迁移平滑），微调值重置为档位默认。 */
  switchTo(profileId: string): PacingProfile {
    const p = findPacing(profileId);
    if (p.id !== profileId) this.clamped += 1;
    this.profileId = p.id;
    this.countdownSec = p.countdownSec;
    this.motionLevel = p.motionLevel;
    return p;
  }

  setCountdown(sec: number): number {
    const c = clampCountdown(sec);
    if (c !== sec) this.clamped += 1;
    this.countdownSec = c;
    return c;
  }

  setMotion(level: number, reduceMotion = false): number {
    const m = clampMotion(level, reduceMotion);
    if (m !== level) this.clamped += 1;
    this.motionLevel = m;
    return m;
  }

  /** 渲染摘要（设置页直接展示）。 */
  caption(): string {
    const p = findPacing(this.profileId);
    const tail = this.countdownSec === p.countdownSec ? "" : `（自定义 ${this.countdownSec}s）`;
    return `${p.name} · 倒计时 ${this.countdownSec}s · 动效 L${this.motionLevel}${tail}`;
  }
}
