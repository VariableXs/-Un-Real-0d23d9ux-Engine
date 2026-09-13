// UNREAL-X：AI-12 族0119「桌面彩蛋学」（X02951~X02975）。
// 桌面彩蛋的触发编排、冷却与总开关：不损主线体验、可一键关闭、有品牌记忆点。

export type EggTrigger = 'sequence' | 'date' | 'count' | 'idle' | 'weather';
export const EGG_TRIGGERS: EggTrigger[] = ['sequence', 'date', 'count', 'idle', 'weather'];

export type EggRarity = 'common' | 'rare' | 'epic' | 'legendary';

export interface EggDef {
  id: string;
  trigger: EggTrigger;
  payload: string;
  rarity: EggRarity;
  /** 触发所需次数或毫秒阈值。 */
  threshold: number;
}

export const EGG_CATALOG: EggDef[] = [
  { id: 'konami', trigger: 'sequence', payload: 'prism-sweep', rarity: 'rare', threshold: 8 },
  { id: 'newyear', trigger: 'date', payload: 'fireworks', rarity: 'common', threshold: 101 },
  { id: 'hundred', trigger: 'count', payload: 'confetti', rarity: 'common', threshold: 100 },
  { id: 'daydream', trigger: 'idle', payload: 'aurora-drift', rarity: 'epic', threshold: 300_000 },
  { id: 'snowline', trigger: 'weather', payload: 'snowfall', rarity: 'legendary', threshold: 0 },
];

export const RARITY_WEIGHT: Record<EggRarity, number> = {
  common: 1,
  rare: 3,
  epic: 6,
  legendary: 10,
};

export interface EggOptions {
  masterOff: boolean;
  cooldownMs: number;
  soundless: boolean;
  reduceMotion: boolean;
}

export const DEFAULT_EGG: EggOptions = {
  masterOff: false,
  cooldownMs: 60_000,
  soundless: false,
  reduceMotion: false,
};

export interface EggEvent {
  id: string;
  at: number;
  payload: string;
  rarity: EggRarity;
}

const clamp = (v: number, lo: number, hi: number): number => (v < lo ? lo : v > hi ? hi : v);

/** 稀有度分（用于成就排序）。 */
export function rarityScore(e: EggDef): number {
  return RARITY_WEIGHT[e.rarity];
}

export class EasterEggLayer {
  private opts: EggOptions = { ...DEFAULT_EGG };
  private fired: EggEvent[] = [];
  private counters = new Map<string, number>();
  private lastAt = new Map<string, number>();

  constructor(patch?: Partial<EggOptions>) {
    if (patch) this.configure(patch);
  }

  get options(): EggOptions {
    return { ...this.opts };
  }

  configure(patch: Partial<EggOptions>): boolean {
    let ok = true;
    if (typeof patch.masterOff === 'boolean') this.opts.masterOff = patch.masterOff;
    if (patch.cooldownMs !== undefined) {
      if (patch.cooldownMs >= 0 && patch.cooldownMs <= 3_600_000) this.opts.cooldownMs = Math.round(patch.cooldownMs);
      else {
        this.opts.cooldownMs = DEFAULT_EGG.cooldownMs;
        ok = false;
      }
    }
    if (typeof patch.soundless === 'boolean') this.opts.soundless = patch.soundless;
    if (typeof patch.reduceMotion === 'boolean') this.opts.reduceMotion = patch.reduceMotion;
    return ok;
  }

  /** 累计触发计数（登记去重：每 id 一条计数）。 */
  bump(id: string, step = 1): number {
    const next = (this.counters.get(id) ?? 0) + clamp(Math.round(step), 0, 1000);
    this.counters.set(id, next);
    return next;
  }

  countOf(id: string): number {
    return this.counters.get(id) ?? 0;
  }

  /** 冷却判定。 */
  inCooldown(id: string, now: number): boolean {
    const last = this.lastAt.get(id);
    if (last === undefined) return false;
    return now - last < this.opts.cooldownMs;
  }

  /** 是否达到阈值（不同触发器语义不同）。 */
  ready(def: EggDef, now: number): boolean {
    if (this.opts.masterOff) return false;
    if (this.inCooldown(def.id, now)) return false;
    switch (def.trigger) {
      case 'count':
        return this.countOf(def.id) >= def.threshold;
      case 'sequence':
        return this.countOf(def.id) >= def.threshold;
      case 'idle':
        return this.countOf(def.id) >= def.threshold;
      case 'date': {
        const md = Number(`${now}`.slice(-6, -2));
        return md === def.threshold;
      }
      case 'weather':
        return this.countOf(def.id) > def.threshold;
      default:
        return false;
    }
  }

  /** 触发：记录事件；已触发过的同 id 在冷却内不重复。 */
  fire(def: EggDef, now: number): { ok: boolean; event?: EggEvent; reason?: string } {
    if (this.opts.masterOff) return { ok: false, reason: 'E02 · 彩蛋总开关已关闭，可在设置页重新开启' };
    if (this.inCooldown(def.id, now)) return { ok: false, reason: 'E04 · 彩蛋仍在冷却中，请稍后再试' };
    if (!this.ready(def, now)) return { ok: false, reason: 'E01 · 尚未达到触发条件' };
    const event: EggEvent = { id: def.id, at: now, payload: def.payload, rarity: def.rarity };
    this.fired.push(event);
    this.lastAt.set(def.id, now);
    return { ok: true, event };
  }

  history(): EggEvent[] {
    return this.fired.map((e) => ({ ...e }));
  }

  get count(): number {
    return this.fired.length;
  }

  /** 成就总分。 */
  achievementScore(): number {
    return this.fired.reduce((a, e) => a + RARITY_WEIGHT[e.rarity], 0);
  }

  /** 动效时长：reduce-motion 下归零，不损主线体验。 */
  duration(ms: number): number {
    return this.opts.reduceMotion ? 0 : ms;
  }

  hint(): string {
    return this.opts.masterOff ? '彩蛋已全部关闭' : `已触发 ${this.count} 个彩蛋`;
  }

  /** 一键关闭：清空记录并不再触发。 */
  disableAll(): boolean {
    this.opts.masterOff = true;
    this.fired = [];
    this.counters.clear();
    return this.fired.length === 0 && this.opts.masterOff;
  }

  uninstall(): boolean {
    this.fired = [];
    this.counters.clear();
    this.lastAt.clear();
    this.opts = { ...DEFAULT_EGG };
    return this.count === 0 && this.counters.size === 0;
  }
}
