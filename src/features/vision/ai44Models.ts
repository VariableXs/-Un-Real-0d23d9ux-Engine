/**
 * UNREAL-X-15000 · AI-44 视觉系统（领域12 · 族0431~0440 · X10751~X11000）模型层，勿删。
 * 十域：主题引擎深2.0 / 多屏艺术2.0 / 微动效细节2.0 / 季节环境系统2.0 / 界面密度2.0 /
 *       强调色系统2.0 / 特效层2.0 / 声画联动2.0 / 视觉守卫2.0 / 视觉性能预算。
 * 纯 TypeScript 零依赖；五档矩阵 + 越界钳制 + 快照迁移 + 降级链，供 ai44Checks.ts 断言。
 */

/* ================= 公共基建 ================= */

/** 通用五档矩阵：默认档=balanced（现状手感），off=低配降级终档。 */
export const TIER_MATRIX = ['off', 'light', 'balanced', 'rich', 'max'] as const;
export type Tier = (typeof TIER_MATRIX)[number];
export const DEFAULT_TIER: Tier = 'balanced';

export function clampTier(v: unknown): Tier {
  return TIER_MATRIX.includes(v as Tier) ? (v as Tier) : DEFAULT_TIER;
}

/** 错误码体系：禁裸报错，每个码带可读文案与下一步建议。 */
export const ERROR_CODES = {
  E4401: { text: '主题变量引用成环', next: '展开引用链定位环点后解环' },
  E4402: { text: '多屏布局编号越界', next: '钳制到最近屏幕后重排' },
  E4403: { text: '微动效时长溢出', next: '钳制到令牌上限并统一曲线' },
  E4404: { text: '季节配置无法解析', next: '回退当前自然季节默认档' },
  E4405: { text: '密度缩放超出安全区', next: '钳制到 0.8~1.4 并提示重启生效' },
  E4406: { text: '强调色对比度不足', next: '自动加深或改选达标候选色' },
  E4407: { text: '特效层数超预算', next: '按优先级裁剪到预算内' },
  E4408: { text: '声画绑定的音源不存在', next: '解除绑定并回退静默曲线' },
  E4409: { text: '视觉守卫规则冲突', next: '按 HC 红线优先仲裁后重载' },
  E4410: { text: '性能预算超标', next: '查看超标项并按建议降档' },
} as const;
export type ErrorCode = keyof typeof ERROR_CODES;

export function explainError(code: string): { text: string; next: string } {
  return (ERROR_CODES as Record<string, { text: string; next: string }>)[code] ?? ERROR_CODES.E4401;
}

/** 动效令牌：曲线/时长/缩放三对齐；off 档退化为纯淡入淡出（reduce-motion 等价）。 */
export const MOTION_TOKENS = { curve: 'ease-standard', durationMs: 180, scale: 1 } as const;
export function motionFor(tier: Tier): { curve: string; durationMs: number; scale: number } {
  if (tier === 'off') return { curve: 'linear-fade', durationMs: 120, scale: 0 };
  return { ...MOTION_TOKENS };
}

/* ================= 族0431 主题引擎深 2.0 ================= */

const THEME_MATRIX = ['mono', 'duotone', 'standard', 'vivid', 'editorial'] as const;
export type ThemeTier = (typeof THEME_MATRIX)[number];

export class ThemeEngineDeep2 {
  tier: ThemeTier;
  clamped = 0;
  private vars = new Map<string, string>();

  constructor(tier: unknown = 'standard') {
    this.tier = THEME_MATRIX.includes(tier as ThemeTier) ? (tier as ThemeTier) : 'standard';
    if (this.tier !== tier) this.clamped += 1;
  }

  setVar(name: string, value: string): boolean {
    if (this.vars.has(name) && this.vars.get(name) === value) return false; // 去重
    this.vars.set(name, value);
    return true;
  }

  getVar(name: string): string | null {
    return this.vars.get(name) ?? null;
  }

  /** 变量引用展开：--a: var(--b)，检测环。 */
  resolve(name: string, seen = new Set<string>()): string | null {
    if (seen.has(name)) return null; // 引用成环
    const raw = this.vars.get(name);
    if (raw === undefined) return null;
    const m = /^var\((--[\w-]+)\)$/.exec(raw);
    if (!m) return raw;
    seen.add(name);
    return this.resolve(m[1]!, seen);
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, vars: [...this.vars.entries()] });
  }

  static deserialize(raw: string): ThemeEngineDeep2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; vars?: [string, string][] };
      const t = new ThemeEngineDeep2(o.tier ?? 'standard');
      for (const [k, v] of o.vars ?? []) t.vars.set(k, v);
      return t;
    } catch {
      return new ThemeEngineDeep2();
    }
  }

  rollback(): boolean {
    this.vars.clear();
    return this.vars.size === 0;
  }

  suggest(contrastDark: boolean): { target: ThemeTier; reason: string } | null {
    if (contrastDark && this.tier === 'standard') return { target: 'editorial', reason: '深色环境建议编辑排版主题' };
    return null;
  }
}

/* ================= 族0432 多屏艺术 2.0 ================= */

const SCREEN_MATRIX = ['mirror', 'extend', 'panorama', 'independent', 'gallery'] as const;
export type ScreenTier = (typeof SCREEN_MATRIX)[number];

export class MultiScreenArt2 {
  tier: ScreenTier;
  clamped = 0;
  private arts = new Map<number, string>();
  screenCount: number;

  constructor(tier: unknown = 'extend', screenCount = 2) {
    this.tier = SCREEN_MATRIX.includes(tier as ScreenTier) ? (tier as ScreenTier) : 'extend';
    if (this.tier !== tier) this.clamped += 1;
    this.screenCount = Math.max(1, Math.min(8, screenCount));
    if (this.screenCount !== screenCount) this.clamped += 1;
  }

  /** 屏号越界钳制到最近屏幕。 */
  assignArt(screen: number, art: string): number {
    const idx = Math.max(0, Math.min(this.screenCount - 1, screen));
    if (idx !== screen) this.clamped += 1;
    this.arts.set(idx, art);
    return idx;
  }

  artOf(screen: number): string | null {
    return this.arts.get(Math.max(0, Math.min(this.screenCount - 1, screen))) ?? null;
  }

  get assignedCount(): number {
    return this.arts.size;
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, n: this.screenCount, arts: [...this.arts.entries()] });
  }

  static deserialize(raw: string): MultiScreenArt2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; n?: number; arts?: [number, string][] };
      const m = new MultiScreenArt2(o.tier ?? 'extend', o.n ?? 2);
      for (const [s, a] of o.arts ?? []) m.arts.set(s, a);
      return m;
    } catch {
      return new MultiScreenArt2();
    }
  }

  rollback(): boolean {
    this.arts.clear();
    return this.assignedCount === 0;
  }

  suggest(count: number): { target: ScreenTier; reason: string } | null {
    if (count >= 3 && this.tier === 'mirror') return { target: 'gallery', reason: '三屏以上建议画廊布局' };
    return null;
  }
}

/* ================= 族0433 微动效细节 2.0 ================= */

const MICRO_MATRIX = ['off', 'reduced', 'standard', 'expressive', 'playful'] as const;
export type MicroTier = (typeof MICRO_MATRIX)[number];
export const MICRO_SLOTS = ['hover', 'press', 'focus', 'enter', 'exit'] as const;
export type MicroSlot = (typeof MICRO_SLOTS)[number];

export class MicroMotion2 {
  tier: MicroTier;
  clamped = 0;
  private durations = new Map<MicroSlot, number>();

  constructor(tier: unknown = 'standard') {
    this.tier = MICRO_MATRIX.includes(tier as MicroTier) ? (tier as MicroTier) : 'standard';
    if (this.tier !== tier) this.clamped += 1;
  }

  /** 时长钳制 0~600ms；off/reduced 档强制走淡入淡出。 */
  setDuration(slot: MicroSlot, ms: number): number {
    const cap = this.tier === 'off' || this.tier === 'reduced' ? 120 : 600;
    const v = Math.max(0, Math.min(cap, ms));
    if (v !== ms) this.clamped += 1;
    this.durations.set(slot, v);
    return v;
  }

  durationOf(slot: MicroSlot): number {
    return this.durations.get(slot) ?? 180;
  }

  curveOf(slot: MicroSlot): string {
    if (this.tier === 'off' || this.tier === 'reduced') return 'linear-fade';
    return slot === 'press' ? 'ease-spring' : 'ease-standard';
  }

  /** 键盘 roving 焦点序：focus 槽时长短于 enter（即时反馈优先）。 */
  static rovingOk(h: MicroMotion2): boolean {
    return h.durationOf('focus') <= h.durationOf('enter');
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, d: [...this.durations.entries()] });
  }

  static deserialize(raw: string): MicroMotion2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; d?: [MicroSlot, number][] };
      const m = new MicroMotion2(o.tier ?? 'standard');
      for (const [s, v] of o.d ?? []) m.durations.set(s, v);
      return m;
    } catch {
      return new MicroMotion2();
    }
  }

  rollback(): boolean {
    this.durations.clear();
    return this.durations.size === 0;
  }

  suggest(batteryPct: number): { target: MicroTier; reason: string } | null {
    if (batteryPct < 20 && this.tier === 'playful') return { target: 'reduced', reason: '低电量建议收敛微动效' };
    return null;
  }
}

/* ================= 族0434 季节环境系统 2.0 ================= */

const SEASON_MATRIX = ['off', 'subtle', 'balanced', 'immersive', 'festival'] as const;
export type SeasonTier = (typeof SEASON_MATRIX)[number];
export const SEASONS = ['spring', 'summer', 'autumn', 'winter'] as const;
export type Season = (typeof SEASONS)[number];

export class SeasonEnv2 {
  tier: SeasonTier;
  clamped = 0;
  private manual: Season | null = null;

  constructor(tier: unknown = 'balanced') {
    this.tier = SEASON_MATRIX.includes(tier as SeasonTier) ? (tier as SeasonTier) : 'balanced';
    if (this.tier !== tier) this.clamped += 1;
  }

  setManualSeason(s: Season | null): void {
    this.manual = s;
  }

  /** 自然季节按月份推断；手动覆盖优先。 */
  seasonFor(month: number): Season {
    if (this.manual) return this.manual;
    const m = ((month % 12) + 12) % 12;
    if (m <= 1 || m === 11) return 'winter';
    if (m <= 4) return 'spring';
    if (m <= 7) return 'summer';
    return 'autumn';
  }

  static paletteFor(s: Season): [number, number, number] {
    return { spring: [120, 200, 120], summer: [90, 170, 255], autumn: [230, 150, 60], winter: [170, 190, 220] }[s];
  }

  /** 越界月份钳制：负数/超界回当前年循环。 */
  static clampMonth(m: number): number {
    return ((m % 12) + 12) % 12;
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, manual: this.manual });
  }

  static deserialize(raw: string): SeasonEnv2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; manual?: Season | null };
      const s = new SeasonEnv2(o.tier ?? 'balanced');
      if (o.manual && SEASONS.includes(o.manual)) s.setManualSeason(o.manual);
      return s;
    } catch {
      return new SeasonEnv2();
    }
  }

  rollback(): boolean {
    this.setManualSeason(null);
    this.tier = 'balanced';
    return this.manual === null;
  }

  suggest(month: number): { target: SeasonTier; reason: string } | null {
    if (this.seasonFor(month) === 'winter' && this.tier === 'subtle') return { target: 'immersive', reason: '冬季建议沉浸氛围' };
    return null;
  }
}

/* ================= 族0435 界面密度 2.0 ================= */

const DENSITY_MATRIX = ['compact', 'cozy', 'comfortable', 'spacious', 'presentation'] as const;
export type DensityTier = (typeof DENSITY_MATRIX)[number];
/** 密度档 → 缩放系数（安全区 0.8~1.4）。 */
export const DENSITY_SCALE: Record<DensityTier, number> = { compact: 0.8, cozy: 0.9, comfortable: 1, spacious: 1.2, presentation: 1.4 };

export class UiDensity2 {
  tier: DensityTier;
  clamped = 0;
  private rowHeightPx = 32;

  constructor(tier: unknown = 'comfortable') {
    this.tier = DENSITY_MATRIX.includes(tier as DensityTier) ? (tier as DensityTier) : 'comfortable';
    if (this.tier !== tier) this.clamped += 1;
  }

  setRowHeight(px: number): number {
    const scaled = Math.round(px * DENSITY_SCALE[this.tier]);
    const v = Math.max(20, Math.min(64, scaled));
    if (v !== Math.round(px * DENSITY_SCALE[this.tier])) this.clamped += 1;
    this.rowHeightPx = v;
    return this.rowHeightPx;
  }

  get rowHeight(): number {
    return this.rowHeightPx;
  }

  static safeScale(v: number): number {
    return Math.max(0.8, Math.min(1.4, v));
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, row: this.rowHeightPx });
  }

  static deserialize(raw: string): UiDensity2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; row?: number };
      const d = new UiDensity2(o.tier ?? 'comfortable');
      if (typeof o.row === 'number') d.rowHeightPx = o.row;
      return d;
    } catch {
      return new UiDensity2();
    }
  }

  rollback(): boolean {
    this.rowHeightPx = 32;
    this.tier = 'comfortable';
    return this.rowHeight === 32;
  }

  suggest(visionLow: boolean): { target: DensityTier; reason: string } | null {
    if (visionLow && DENSITY_SCALE[this.tier] < 1.2) return { target: 'spacious', reason: '建议更大间距易读' };
    return null;
  }
}

/* ================= 族0436 强调色系统 2.0 ================= */

const ACCENT_MATRIX = ['blue', 'teal', 'violet', 'amber', 'rose'] as const;
export type AccentTier = (typeof ACCENT_MATRIX)[number];
export const ACCENT_RGB: Record<AccentTier, [number, number, number]> = {
  blue: [59, 130, 246],
  teal: [20, 184, 166],
  violet: [139, 92, 246],
  amber: [245, 158, 11],
  rose: [244, 63, 94],
};

export class AccentColor2 {
  tier: AccentTier;
  clamped = 0;
  private lightBg = true;

  constructor(tier: unknown = 'blue') {
    this.tier = ACCENT_MATRIX.includes(tier as AccentTier) ? (tier as AccentTier) : 'blue';
    if (this.tier !== tier) this.clamped += 1;
  }

  setBackground(light: boolean): void {
    this.lightBg = light;
  }

  /** WCAG 相对亮度 + 对比度（真实算法，非近似）。 */
  static luminance(rgb: [number, number, number]): number {
    const lin = (c: number) => {
      const s = c / 255;
      return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * lin(rgb[0]) + 0.7152 * lin(rgb[1]) + 0.0722 * lin(rgb[2]);
  }

  static contrast(a: [number, number, number], b: [number, number, number]): number {
    const l1 = AccentColor2.luminance(a);
    const l2 = AccentColor2.luminance(b);
    const [hi, lo] = l1 > l2 ? [l1, l2] : [l2, l1];
    return (hi + 0.05) / (lo + 0.05);
  }

  /** HC 红线：正文对比 ≥ 4.5。深底配亮色/浅底配深色。 */
  contrastOnBg(): number {
    const accent = ACCENT_RGB[this.tier];
    const bg: [number, number, number] = this.lightBg ? [255, 255, 255] : [17, 17, 17];
    return AccentColor2.contrast(accent, bg);
  }

  /** 通道值越界钳制。 */
  static clampChannel(c: number): number {
    return Math.max(0, Math.min(255, Math.round(c)));
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, light: this.lightBg });
  }

  static deserialize(raw: string): AccentColor2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; light?: boolean };
      const a = new AccentColor2(o.tier ?? 'blue');
      a.setBackground(o.light !== false);
      return a;
    } catch {
      return new AccentColor2();
    }
  }

  rollback(): boolean {
    this.tier = 'blue';
    this.setBackground(true);
    return this.tier === 'blue' && this.lightBg;
  }

  /** 对比不足时给出达标候选（可解释）。 */
  suggestAccessible(): { target: AccentTier; reason: string } | null {
    const best = ACCENT_MATRIX.reduce((acc, t) => (AccentColor2.contrast(ACCENT_RGB[t], this.lightBg ? [255, 255, 255] : [17, 17, 17]) > AccentColor2.contrast(ACCENT_RGB[acc], this.lightBg ? [255, 255, 255] : [17, 17, 17]) ? t : acc), 'blue' as AccentTier);
    if (this.contrastOnBg() < 4.5) return { target: best, reason: `当前对比 ${this.contrastOnBg().toFixed(2)} 低于 4.5，建议 ${best}` };
    return null;
  }
}

/* ================= 族0437 特效层 2.0 ================= */

const FX_MATRIX = ['none', 'blur', 'shadow', 'glass', 'bloom'] as const;
export type FxTier = (typeof FX_MATRIX)[number];
/** 各特效相对成本（预算单位）。 */
export const FX_COST: Record<FxTier, number> = { none: 0, blur: 1, shadow: 1, glass: 2, bloom: 3 };

export class FxLayer2 {
  tier: FxTier;
  clamped = 0;
  private enabled = new Set<FxTier>();
  budget = 4;

  constructor(tier: unknown = 'glass') {
    this.tier = FX_MATRIX.includes(tier as FxTier) ? (tier as FxTier) : 'glass';
    if (this.tier !== tier) this.clamped += 1;
  }

  enable(fx: FxTier): boolean {
    if (this.enabled.has(fx)) return false; // 去重
    if (FX_COST[fx] === 0) return false;
    const total = [...this.enabled].reduce((s, f) => s + FX_COST[f], 0);
    if (total + FX_COST[fx] > this.budget) return false; // 层数超预算
    this.enabled.add(fx);
    return true;
  }

  disable(fx: FxTier): boolean {
    return this.enabled.delete(fx);
  }

  get enabledList(): FxTier[] {
    return [...this.enabled];
  }

  get usedBudget(): number {
    return [...this.enabled].reduce((s, f) => s + FX_COST[f], 0);
  }

  /** 低配降级链：按成本从高到低裁剪。 */
  degrade(): FxTier[] {
    const order = (['bloom', 'glass', 'blur', 'shadow'] as const).filter((f) => this.enabled.has(f));
    for (const f of order) this.enabled.delete(f);
    return this.enabledList;
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, fx: this.enabledList, budget: this.budget });
  }

  static deserialize(raw: string): FxLayer2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; fx?: FxTier[]; budget?: number };
      const f = new FxLayer2(o.tier ?? 'glass');
      if (typeof o.budget === 'number') f.budget = o.budget;
      for (const x of o.fx ?? []) f.enabled.add(x);
      return f;
    } catch {
      return new FxLayer2();
    }
  }

  rollback(): boolean {
    this.enabled.clear();
    this.budget = 4;
    return this.usedBudget === 0;
  }

  suggest(gpuLow: boolean): { drop: FxTier | null; reason: string } | null {
    if (gpuLow && this.enabled.has('bloom')) return { drop: 'bloom', reason: 'GPU 紧张，建议关闭泛光' };
    return null;
  }
}

/* ================= 族0438 声画联动 2.0 ================= */

const AV_MATRIX = ['off', 'beat', 'level', 'spectrum', 'cinematic'] as const;
export type AvTier = (typeof AV_MATRIX)[number];

export class AudioVisualLink2 {
  tier: AvTier;
  clamped = 0;
  private boundSource: string | null = null;
  private lastLevel = 0;

  constructor(tier: unknown = 'level') {
    this.tier = AV_MATRIX.includes(tier as AvTier) ? (tier as AvTier) : 'level';
    if (this.tier !== tier) this.clamped += 1;
  }

  /** 绑定音源：重复绑定同源去重，off 档拒绝绑定。 */
  bind(source: string): boolean {
    if (this.tier === 'off') return false;
    if (this.boundSource === source) return false;
    this.boundSource = source;
    return true;
  }

  unbind(): boolean {
    const had = this.boundSource !== null;
    this.boundSource = null;
    return had;
  }

  get source(): string | null {
    return this.boundSource;
  }

  /** 电平 → 视觉强度：平滑（指数滑动平均），钳制 0~1。 */
  pushLevel(v: number): number {
    const x = Math.max(0, Math.min(1, v));
    if (x !== v) this.clamped += 1;
    const smooth = this.lastLevel + 0.5 * (x - this.lastLevel);
    this.lastLevel = smooth;
    if (this.tier === 'off' || this.boundSource === null) return 0;
    return smooth;
  }

  get currentLevel(): number {
    return this.tier === 'off' || this.boundSource === null ? 0 : this.lastLevel;
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, src: this.boundSource, lvl: this.lastLevel });
  }

  static deserialize(raw: string): AudioVisualLink2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; src?: string | null; lvl?: number };
      const a = new AudioVisualLink2(o.tier ?? 'level');
      if (typeof o.src === 'string') a.bind(o.src);
      a.lastLevel = Math.max(0, Math.min(1, o.lvl ?? 0));
      return a;
    } catch {
      return new AudioVisualLink2();
    }
  }

  rollback(): boolean {
    this.unbind();
    this.lastLevel = 0;
    return this.source === null && this.currentLevel === 0;
  }

  suggest(meeting: boolean): { target: AvTier; reason: string } | null {
    if (meeting && this.tier === 'cinematic') return { target: 'off', reason: '会议中建议静默声画联动' };
    return null;
  }
}

/* ================= 族0439 视觉守卫 2.0 ================= */

const GUARD_MATRIX = ['off', 'basic', 'standard', 'strict', 'hc'] as const;
export type GuardTier = (typeof GUARD_MATRIX)[number];

export interface GuardViolation {
  rule: 'contrast' | 'touch-target' | 'focus-ring' | 'motion-cap';
  detail: string;
}

export class VisionGuard2 {
  tier: GuardTier;
  clamped = 0;
  private rules = new Set<GuardViolation['rule']>(['contrast']);

  constructor(tier: unknown = 'standard') {
    this.tier = GUARD_MATRIX.includes(tier as GuardTier) ? (tier as GuardTier) : 'standard';
    if (this.tier !== tier) this.clamped += 1;
    if (this.tier === 'hc') {
      this.rules.add('touch-target');
      this.rules.add('focus-ring');
      this.rules.add('motion-cap');
    }
  }

  addRule(rule: GuardViolation['rule']): boolean {
    if (this.rules.has(rule)) return false; // 去重
    this.rules.add(rule);
    return true;
  }

  get ruleList(): GuardViolation['rule'][] {
    return [...this.rules];
  }

  /** HC 红线：对比 < 4.5 即违规。 */
  static checkContrast(fg: [number, number, number], bg: [number, number, number]): GuardViolation | null {
    return AccentColor2.contrast(fg, bg) < 4.5 ? { rule: 'contrast', detail: '前景对比低于 4.5' } : null;
  }

  /** HC 档触点 ≥ 44px。 */
  static checkTouchTarget(px: number): GuardViolation | null {
    return px < 44 ? { rule: 'touch-target', detail: `触点 ${px}px 低于 44px` } : null;
  }

  audit(issues: Array<GuardViolation | null>): GuardViolation[] {
    if (this.tier === 'off') return [];
    return issues.filter((i): i is GuardViolation => i !== null);
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, rules: this.ruleList });
  }

  static deserialize(raw: string): VisionGuard2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; rules?: GuardViolation['rule'][] };
      const g = new VisionGuard2(o.tier ?? 'standard');
      for (const r of o.rules ?? []) g.rules.add(r);
      return g;
    } catch {
      return new VisionGuard2();
    }
  }

  rollback(): boolean {
    this.rules = new Set(['contrast']);
    this.tier = 'standard';
    return this.ruleList.length === 1;
  }

  suggest(): { target: GuardTier; reason: string } | null {
    if (this.tier === 'basic') return { target: 'strict', reason: '建议启用严格守卫防线' };
    return null;
  }
}

/* ================= 族0440 视觉性能预算 ================= */

const BUDGET_MATRIX = ['loose', 'normal', 'tight', 'strict', 'ci'] as const;
export type BudgetTier = (typeof BUDGET_MATRIX)[number];
/** 默认预算表（ms / 次），指标入 CI 基线防劣化。 */
export const BUDGET_TABLE: Record<string, number> = {
  'theme.compile': 8,
  'motion.frame': 4,
  'accent.contrast': 1,
  'fx.composite': 12,
  'season.switch': 20,
};

export class VisionBudget2 {
  tier: BudgetTier;
  clamped = 0;
  private budgets = new Map<string, number>(Object.entries(BUDGET_TABLE));
  private samples = new Map<string, number[]>();
  /** 防劣化守卫：预算只增不删（收紧），破坏即红。 */
  private violations = 0;

  constructor(tier: unknown = 'normal') {
    this.tier = BUDGET_MATRIX.includes(tier as BudgetTier) ? (tier as BudgetTier) : 'normal';
    if (this.tier !== tier) this.clamped += 1;
  }

  /** 预算收紧（只减不增），放宽视为破坏计数。 */
  tighten(metric: string, ms: number): boolean {
    const cur = this.budgets.get(metric);
    if (cur === undefined) return false;
    if (ms >= cur) {
      this.violations += 1;
      return false;
    }
    this.budgets.set(metric, ms);
    return true;
  }

  budgetOf(metric: string): number | null {
    return this.budgets.get(metric) ?? null;
  }

  get violationCount(): number {
    return this.violations;
  }

  sample(metric: string, ms: number): void {
    const arr = this.samples.get(metric) ?? [];
    arr.push(ms);
    this.samples.set(metric, arr);
  }

  /** p95 分位。 */
  p95(metric: string): number | null {
    const arr = this.samples.get(metric);
    if (!arr || arr.length === 0) return null;
    const sorted = [...arr].sort((a, b) => a - b);
    const idx = Math.min(arr.length - 1, Math.ceil(arr.length * 0.95) - 1);
    return sorted[idx]!;
  }

  /** 基线判定：p95 是否超预算。 */
  withinBudget(metric: string): boolean {
    const p = this.p95(metric);
    const b = this.budgetOf(metric);
    if (p === null || b === null) return true;
    return p <= b;
  }

  /** 低配降级链：预算档回退。 */
  degrade(): BudgetTier {
    const order = BUDGET_MATRIX as readonly BudgetTier[];
    const idx = order.indexOf(this.tier);
    this.tier = order[Math.max(0, idx - 1)]!;
    return this.tier;
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, b: [...this.budgets.entries()] });
  }

  static deserialize(raw: string): VisionBudget2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; b?: [string, number][] };
      const v = new VisionBudget2(o.tier ?? 'normal');
      for (const [k, ms] of o.b ?? []) v.budgets.set(k, ms);
      return v;
    } catch {
      return new VisionBudget2();
    }
  }

  rollback(): boolean {
    this.budgets = new Map(Object.entries(BUDGET_TABLE));
    this.samples.clear();
    this.violations = 0;
    return this.budgetOf('theme.compile') === BUDGET_TABLE['theme.compile'];
  }

  suggest(): { metric: string; reason: string } | null {
    const bad = [...this.budgets.keys()].find((m) => !this.withinBudget(m));
    return bad ? { metric: bad, reason: `${bad} p95 超预算，建议按守卫口径收紧或降档` } : null;
  }
}
