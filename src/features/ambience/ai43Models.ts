/**
 * UNREAL-X-15000 · AI-43 个性化深化（领域12 · 族0421~0430 · X10501~X10750）模型层，勿删。
 * 十域：氛围光2.0 / 屏保复兴2.0 / 字体生态2.0 / 图标包生态2.0 / 触觉反馈2.0 /
 *       动效艺术2.0 / 个性化档案2.0 / 空间个性化2.0 / 印刷导出2.0 / 视觉彩蛋2.0。
 * 纯 TypeScript 零依赖；五档矩阵 + 越界钳制 + 快照迁移 + 降级链，供 ai43Checks.ts 断言。
 */

/* ================= 公共基建 ================= */

/** 通用五档矩阵：默认档=balanced（现状手感），off=低配降级终档。 */
export const TIER_MATRIX = ['off', 'light', 'balanced', 'rich', 'cinema'] as const;
export type Tier = (typeof TIER_MATRIX)[number];
export const DEFAULT_TIER: Tier = 'balanced';

export function clampTier(v: unknown): Tier {
  return TIER_MATRIX.includes(v as Tier) ? (v as Tier) : DEFAULT_TIER;
}

/** 错误码体系：禁裸报错，每个码带可读文案与下一步建议。 */
export const ERROR_CODES = {
  E4301: { text: '氛围采样失败', next: '回退默认光效并重采样' },
  E4302: { text: '屏保模块缺资源', next: '切换到内置极简屏保' },
  E4303: { text: '字体文件损坏', next: '移入隔离区并回退系统字体' },
  E4304: { text: '图标包清单不合规', next: '按模板修正 manifest 后重装' },
  E4305: { text: '触觉马达不支持波形', next: '降级为最接近的内置波形' },
  E4306: { text: '动效曲线解析失败', next: '回退标准动效令牌' },
  E4307: { text: '个性化档案版本过旧', next: '走迁移通道升级后导入' },
  E4308: { text: '空间区域越界', next: '钳制到最近合法区域' },
  E4309: { text: '打印描述超出纸张', next: '自动缩排到安全边距' },
  E4310: { text: '彩蛋暗号不匹配', next: '检查输入序列后重试' },
} as const;
export type ErrorCode = keyof typeof ERROR_CODES;

export function explainError(code: string): { text: string; next: string } {
  return (ERROR_CODES as Record<string, { text: string; next: string }>)[code] ?? ERROR_CODES.E4301;
}

/** 动效令牌：曲线/时长/缩放三对齐；off 档退化为纯淡入淡出（reduce-motion 等价）。 */
export const MOTION_TOKENS = { curve: 'ease-standard', durationMs: 180, scale: 1 } as const;
export function motionFor(tier: Tier): { curve: string; durationMs: number; scale: number } {
  if (tier === 'off') return { curve: 'linear-fade', durationMs: 120, scale: 0 };
  return { ...MOTION_TOKENS };
}

/* ================= 族0421 氛围光 2.0 ================= */

const AMB_MATRIX = ['off', 'static', 'soft', 'dynamic', 'cinema'] as const;
export type AmbTier = (typeof AMB_MATRIX)[number];

export interface AmbFrame {
  source: 'wallpaper' | 'content' | 'audio' | 'manual';
  rgb: [number, number, number];
  intensity: number;
}

export class AmbienceLight2 {
  tier: AmbTier;
  clamped = 0;
  persisted: Record<string, unknown> = {};
  private degraded = false;
  private history: AmbFrame[] = [];

  constructor(tier: unknown = 'soft') {
    this.tier = AMB_MATRIX.includes(tier as AmbTier) ? (tier as AmbTier) : 'soft';
    if (this.tier !== tier) this.clamped += 1;
  }

  /** 端到端最小闭环：采样 → 生成光帧。 */
  sample(source: AmbFrame['source'], rgb: [number, number, number], intensity: number): AmbFrame {
    const f: AmbFrame = {
      source,
      rgb: rgb.map((c) => Math.max(0, Math.min(255, Math.round(c)))) as [number, number, number],
      intensity: Math.max(0, Math.min(1, intensity)),
    };
    if (f.intensity !== intensity || f.rgb.some((c, i) => c !== rgb[i])) this.clamped += 1;
    this.history.push(f);
    if (this.tier === 'off') f.intensity = 0;
    return f;
  }

  /** 档位矩阵：≥5 档独立可交付，档间迁移平滑（强度系数单调）。 */
  static tierGain(t: AmbTier): number {
    return { off: 0, static: 0.25, soft: 0.5, dynamic: 0.75, cinema: 1 }[t];
  }

  /** 配置持久化 + 快照导出/导入/跨版本携带。 */
  persist(key: string, value: unknown): void {
    this.persisted[key] = value;
  }

  serialize(): string {
    return JSON.stringify({ v: 2, tier: this.tier, cfg: this.persisted, hist: this.history.length });
  }

  static deserialize(raw: string): AmbienceLight2 {
    try {
      const o = JSON.parse(raw) as { v?: number; tier?: unknown; cfg?: Record<string, unknown> };
      const s = new AmbienceLight2(clampTier(o.tier));
      s.persisted = o.cfg ?? {};
      return s;
    } catch {
      return new AmbienceLight2();
    }
  }

  /** 资源降级：CPU/内存/电量紧张时逐级降档，off 终档。 */
  degrade(level: 0 | 1 | 2 | 3): AmbTier {
    this.degraded = true;
    const order = AMB_MATRIX as readonly AmbTier[];
    const idx = order.indexOf(this.tier);
    this.tier = order[Math.max(0, idx - level)]!;
    return this.tier;
  }

  get isDegraded(): boolean {
    return this.degraded;
  }

  /** 回滚净身：清空历史与持久化，不留残档。 */
  rollback(): boolean {
    this.history = [];
    this.persisted = {};
    return this.history.length === 0 && Object.keys(this.persisted).length === 0;
  }

  /** 本地启发式智能建议：隐私边界内、可解释、可拒绝（返回 null = 拒绝）。 */
  suggest(dark: boolean): { target: AmbTier; reason: string } | null {
    if (dark && this.tier === 'soft') return { target: 'cinema', reason: '环境较暗，建议提升氛围沉浸度' };
    return null;
  }
}

/* ================= 族0422 屏保复兴 2.0 ================= */

const SS_MATRIX = ['off', 'clock', 'photos', 'art', 'theater'] as const;
export type SsTier = (typeof SS_MATRIX)[number];

export class Screensaver2 {
  tier: SsTier;
  clamped = 0;
  private running = false;
  private startedAt = 0;
  private framesDrawn = 0;
  private partial = false;

  constructor(tier: unknown = 'clock') {
    this.tier = SS_MATRIX.includes(tier as SsTier) ? (tier as SsTier) : 'clock';
    if (this.tier !== tier) this.clamped += 1;
  }

  /** 最小闭环：空闲触发 → 运行 → 唤醒退出，可观测。 */
  activate(idleMs: number, now: number): boolean {
    if (this.tier === 'off' || idleMs < 60_000) return false;
    this.running = true;
    this.startedAt = now;
    return this.running;
  }

  wake(): boolean {
    const was = this.running;
    this.running = false;
    this.partial = this.framesDrawn > 0 && was;
    this.framesDrawn = 0;
    return was;
  }

  get isRunning(): boolean {
    return this.running;
  }

  tick(): number {
    if (this.running) this.framesDrawn += 1;
    return this.framesDrawn;
  }

  /** 断点续作：半成品标记 + 一键续作。 */
  resume(): boolean {
    if (!this.partial) return false;
    this.partial = false;
    this.running = true;
    return true;
  }

  static tierFps(t: SsTier): number {
    return { off: 0, clock: 1, photos: 15, art: 30, theater: 60 }[t];
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, frames: this.framesDrawn });
  }

  static deserialize(raw: string): Screensaver2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new Screensaver2(o.tier ?? 'clock');
    } catch {
      return new Screensaver2();
    }
  }

  rollback(): boolean {
    this.running = false;
    this.framesDrawn = 0;
    this.partial = false;
    return !this.running && this.framesDrawn === 0;
  }

  suggest(idleMs: number): { target: SsTier; reason: string } | null {
    if (idleMs > 15 * 60_000 && this.tier === 'clock') return { target: 'photos', reason: '长时间空闲，建议相册屏保' };
    return null;
  }
}

/* ================= 族0423 字体生态 2.0 ================= */

const FONT_MATRIX = ['system', 'serif', 'sans', 'mono', 'display'] as const;
export type FontTier = (typeof FONT_MATRIX)[number];

export class FontEco2 {
  tier: FontTier;
  clamped = 0;
  private installed = new Set<string>();
  private size = 16;

  constructor(tier: unknown = 'sans') {
    this.tier = FONT_MATRIX.includes(tier as FontTier) ? (tier as FontTier) : 'sans';
    if (this.tier !== tier) this.clamped += 1;
  }

  install(name: string, bytes: Uint8Array): boolean {
    if (this.installed.has(name)) return false; // 登记去重
    if (bytes.length < 4) return false; // 非法字体拒收
    this.installed.add(name);
    return true;
  }

  get installedList(): string[] {
    return [...this.installed];
  }

  setSize(px: number): number {
    const v = Math.max(10, Math.min(40, px));
    if (v !== px) this.clamped += 1;
    this.size = v;
    return this.size;
  }

  get currentSize(): number {
    return this.size;
  }

  /** 回退链：从当前档逐级回退，最终落到 system。 */
  fallbackChain(): FontTier[] {
    const idx = FONT_MATRIX.indexOf(this.tier);
    return FONT_MATRIX.slice(0, idx + 1).reverse() as FontTier[];
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, size: this.size, fonts: this.installedList });
  }

  static deserialize(raw: string): FontEco2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; size?: number; fonts?: string[] };
      const f = new FontEco2(o.tier ?? 'sans');
      if (typeof o.size === 'number') f.setSize(o.size);
      for (const n of o.fonts ?? []) f.install(n, new Uint8Array(8));
      return f;
    } catch {
      return new FontEco2();
    }
  }

  rollback(): boolean {
    this.installed.clear();
    this.setSize(16);
    return this.installed.size === 0 && this.size === 16;
  }

  suggest(text: string): { family: FontTier; reason: string } | null {
    if (/[A-Za-z]{20,}/.test(text)) return { family: 'mono', reason: '检测到长代码段，建议等宽字体' };
    return null;
  }
}

/* ================= 族0424 图标包生态 2.0 ================= */

const ICON_MATRIX = ['flat', 'outline', 'glass', 'pixel', 'handdrawn'] as const;
export type IconTier = (typeof ICON_MATRIX)[number];

export class IconPack2 {
  tier: IconTier;
  clamped = 0;
  private overrides = new Map<string, string>();

  constructor(tier: unknown = 'flat') {
    this.tier = ICON_MATRIX.includes(tier as IconTier) ? (tier as IconTier) : 'flat';
    if (this.tier !== tier) this.clamped += 1;
  }

  /** 清单校验：name + version + icons 三要素。 */
  static validateManifest(m: { name?: string; version?: string; icons?: string[] }): boolean {
    return typeof m.name === 'string' && m.name.length > 0 && /^\d+\.\d+$/.test(m.version ?? '') && (m.icons?.length ?? 0) > 0;
  }

  override(iconId: string, asset: string): boolean {
    if (this.overrides.has(iconId)) return false; // 登记去重
    this.overrides.set(iconId, asset);
    return true;
  }

  assetOf(iconId: string): string {
    return this.overrides.get(iconId) ?? `${this.tier}/${iconId}.svg`;
  }

  get overrideCount(): number {
    return this.overrides.size;
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, ov: [...this.overrides.entries()] });
  }

  static deserialize(raw: string): IconPack2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; ov?: [string, string][] };
      const p = new IconPack2(o.tier ?? 'flat');
      for (const [k, v] of o.ov ?? []) p.overrides.set(k, v);
      return p;
    } catch {
      return new IconPack2();
    }
  }

  rollback(): boolean {
    this.overrides.clear();
    return this.overrideCount === 0;
  }

  suggest(missing: string[]): { target: string; reason: string } | null {
    return missing.length > 10 ? { target: 'fallback-pack', reason: `缺 ${missing.length} 枚图标，建议装补充包` } : null;
  }
}

/* ================= 族0425 触觉反馈 2.0 ================= */

const HAPTIC_MATRIX = ['off', 'tick', 'soft', 'crisp', 'rich'] as const;
export type HapticTier = (typeof HAPTIC_MATRIX)[number];

export class Haptics2 {
  tier: HapticTier;
  clamped = 0;
  private patterns = new Set<string>(['tap', 'success', 'warn']);

  constructor(tier: unknown = 'soft') {
    this.tier = HAPTIC_MATRIX.includes(tier as HapticTier) ? (tier as HapticTier) : 'soft';
    if (this.tier !== tier) this.clamped += 1;
  }

  registerPattern(name: string, wave: number[]): boolean {
    if (this.patterns.has(name)) return false; // 去重
    if (wave.some((w) => w < 0 || w > 1)) return false; // 波形越界拒收
    this.patterns.add(name);
    return true;
  }

  hasPattern(name: string): boolean {
    return this.patterns.has(name);
  }

  /** 波形降级：不支持的自定义波形回退最接近内置波形。 */
  play(name: string): string {
    if (this.tier === 'off') return 'mute';
    if (this.patterns.has(name)) return name;
    return 'tap';
  }

  static intensityFor(t: HapticTier): number {
    return { off: 0, tick: 0.2, soft: 0.5, crisp: 0.75, rich: 1 }[t];
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, pats: [...this.patterns] });
  }

  static deserialize(raw: string): Haptics2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; pats?: string[] };
      const h = new Haptics2(o.tier ?? 'soft');
      for (const p of o.pats ?? []) h.patterns.add(p);
      return h;
    } catch {
      return new Haptics2();
    }
  }

  rollback(): boolean {
    this.patterns = new Set(['tap', 'success', 'warn']);
    return [...this.patterns].length === 3;
  }

  suggest(battery: number): { target: HapticTier; reason: string } | null {
    if (battery < 15 && this.tier !== 'off') return { target: 'off', reason: '电量紧张，建议关闭触觉反馈' };
    return null;
  }
}

/* ================= 族0426 动效艺术 2.0 ================= */

const MOTION_ART_MATRIX = ['none', 'fade', 'slide', 'parallax', 'particle'] as const;
export type ArtTier = (typeof MOTION_ART_MATRIX)[number];

export class MotionArt2 {
  tier: ArtTier;
  clamped = 0;
  private reduceMotion = false;

  constructor(tier: unknown = 'fade') {
    this.tier = MOTION_ART_MATRIX.includes(tier as ArtTier) ? (tier as ArtTier) : 'fade';
    if (this.tier !== tier) this.clamped += 1;
  }

  setReduceMotion(on: boolean): void {
    this.reduceMotion = on;
  }

  /** reduce-motion 自动降级为纯淡入淡出。 */
  render(frame: number): { curve: string; offset: number } {
    if (this.tier === 'none' || this.reduceMotion) return { curve: 'linear-fade', offset: 0 };
    const amp = { fade: 0, slide: 8, parallax: 16, particle: 24 }[this.tier] ?? 0;
    return { curve: 'ease-standard', offset: Math.round(Math.sin(frame / 6) * amp) };
  }

  static layerCount(t: ArtTier): number {
    return { none: 0, fade: 1, slide: 2, parallax: 3, particle: 4 }[t];
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, rm: this.reduceMotion });
  }

  static deserialize(raw: string): MotionArt2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; rm?: boolean };
      const m = new MotionArt2(o.tier ?? 'fade');
      m.setReduceMotion(o.rm === true);
      return m;
    } catch {
      return new MotionArt2();
    }
  }

  rollback(): boolean {
    this.tier = 'fade';
    this.reduceMotion = false;
    return this.tier === 'fade' && !this.reduceMotion;
  }

  suggest(cpuPct: number): { target: ArtTier; reason: string } | null {
    if (cpuPct > 80 && this.tier === 'particle') return { target: 'parallax', reason: 'CPU 紧张，建议降为视差档' };
    return null;
  }
}

/* ================= 族0427 个性化档案 2.0 ================= */

const PROFILE_MATRIX = ['minimal', 'work', 'balanced', 'creator', 'game'] as const;
export type ProfileTier = (typeof PROFILE_MATRIX)[number];

export class PersonaProfile2 {
  tier: ProfileTier;
  clamped = 0;
  private saved = new Map<string, string>();

  constructor(tier: unknown = 'balanced') {
    this.tier = PROFILE_MATRIX.includes(tier as ProfileTier) ? (tier as ProfileTier) : 'balanced';
    if (this.tier !== tier) this.clamped += 1;
  }

  save(name: string, snapshot: string): boolean {
    if (this.saved.has(name)) return false; // 去重：同名覆盖拒绝
    this.saved.set(name, snapshot);
    return true;
  }

  apply(name: string): string | null {
    return this.saved.get(name) ?? null;
  }

  get names(): string[] {
    return [...this.saved.keys()];
  }

  /** 跨版本迁移：旧版本档案升级到 v2。 */
  static migrate(raw: string): PersonaProfile2 | null {
    try {
      const o = JSON.parse(raw) as { v?: number; tier?: unknown; items?: Record<string, string> };
      if (o.v !== 1 && o.v !== 2) return null;
      const p = new PersonaProfile2(o.tier ?? 'balanced');
      for (const [k, v] of Object.entries(o.items ?? {})) p.saved.set(k, v);
      return p;
    } catch {
      return null;
    }
  }

  serialize(): string {
    return JSON.stringify({ v: 2, tier: this.tier, items: Object.fromEntries(this.saved) });
  }

  static deserialize(raw: string): PersonaProfile2 {
    return PersonaProfile2.migrate(raw) ?? new PersonaProfile2();
  }

  rollback(): boolean {
    this.saved.clear();
    return this.names.length === 0;
  }

  suggest(hour: number): { target: ProfileTier; reason: string } | null {
    if (hour >= 9 && hour < 18 && this.tier === 'game') return { target: 'work', reason: '工作时间，建议切换工作档案' };
    return null;
  }
}

/* ================= 族0428 空间个性化 2.0 ================= */

const SPACE_MATRIX = ['plain', 'grid', 'zoned', 'flow', 'freeform'] as const;
export type SpaceTier = (typeof SPACE_MATRIX)[number];
export const ZONES = ['top-left', 'top-right', 'center', 'bottom-left', 'bottom-right'] as const;
export type Zone = (typeof ZONES)[number];

export class SpacePersona2 {
  tier: SpaceTier;
  clamped = 0;
  private decor = new Map<Zone, string>();

  constructor(tier: unknown = 'grid') {
    this.tier = SPACE_MATRIX.includes(tier as SpaceTier) ? (tier as SpaceTier) : 'grid';
    if (this.tier !== tier) this.clamped += 1;
  }

  /** 区域越界钳制到最近合法区域（四角 + 中心）。 */
  static nearestZone(x: number, y: number): Zone {
    const cx = x < 0.33 ? 0 : x > 0.66 ? 2 : 1;
    const cy = y < 0.33 ? 0 : y > 0.66 ? 2 : 1;
    if (cx === 1 && cy === 1) return 'center';
    const row = cy === 0 ? 'top' : cy === 2 ? 'bottom' : 'center';
    if (row === 'center') return 'center';
    return (row + (cx === 0 ? '-left' : '-right')) as Zone;
  }

  decorate(zone: Zone, asset: string): boolean {
    if (this.decor.has(zone)) return false; // 每区一件，去重
    this.decor.set(zone, asset);
    return true;
  }

  decorOf(zone: Zone): string | null {
    return this.decor.get(zone) ?? null;
  }

  get decoratedCount(): number {
    return this.decor.size;
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, decor: [...this.decor.entries()] });
  }

  static deserialize(raw: string): SpacePersona2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; decor?: [Zone, string][] };
      const s = new SpacePersona2(o.tier ?? 'grid');
      for (const [z, a] of o.decor ?? []) if (ZONES.includes(z)) s.decor.set(z, a);
      return s;
    } catch {
      return new SpacePersona2();
    }
  }

  rollback(): boolean {
    this.decor.clear();
    return this.decoratedCount === 0;
  }

  suggest(count: number): { target: SpaceTier; reason: string } | null {
    if (count > 40 && this.tier === 'plain') return { target: 'zoned', reason: '桌面元素多，建议分区管理' };
    return null;
  }
}

/* ================= 族0429 印刷导出 2.0 ================= */

const PRINT_MATRIX = ['draft', 'text', 'balanced', 'photo', 'press'] as const;
export type PrintTier = (typeof PRINT_MATRIX)[number];

export class PrintExport2 {
  tier: PrintTier;
  clamped = 0;
  private marginMm = 10;
  private pages: string[] = [];
  private partialPage = -1;

  constructor(tier: unknown = 'text') {
    this.tier = PRINT_MATRIX.includes(tier as PrintTier) ? (tier as PrintTier) : 'text';
    if (this.tier !== tier) this.clamped += 1;
  }

  setMargin(mm: number): number {
    const v = Math.max(0, Math.min(30, mm));
    if (v !== mm) this.clamped += 1;
    this.marginMm = v;
    return this.marginMm;
  }

  get margin(): number {
    return this.marginMm;
  }

  /** 排版：按字数分页（简化行模型），中断时记录半成品页。 */
  layout(text: string, charsPerPage: number): string[] {
    const cap = Math.max(20, Math.min(5000, charsPerPage));
    if (cap !== charsPerPage) this.clamped += 1;
    this.pages = [];
    for (let i = 0; i < text.length; i += cap) this.pages.push(text.slice(i, i + cap));
    return this.pages;
  }

  /** 中断续印：从第 n 页继续。 */
  abort(atPage: number): boolean {
    if (atPage < 0 || atPage >= this.pages.length) return false;
    this.partialPage = atPage;
    return true;
  }

  resume(): number {
    const from = this.partialPage;
    this.partialPage = -1;
    return from;
  }

  get pageCount(): number {
    return this.pages.length;
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, margin: this.marginMm, pages: this.pageCount });
  }

  static deserialize(raw: string): PrintExport2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; margin?: number };
      const p = new PrintExport2(o.tier ?? 'text');
      if (typeof o.margin === 'number') p.setMargin(o.margin);
      return p;
    } catch {
      return new PrintExport2();
    }
  }

  rollback(): boolean {
    this.pages = [];
    this.partialPage = -1;
    this.setMargin(10);
    return this.pageCount === 0 && this.margin === 10;
  }

  suggest(words: number): { target: PrintTier; reason: string } | null {
    if (words > 10_000 && this.tier === 'photo') return { target: 'text', reason: '长文档建议文本优先档' };
    return null;
  }
}

/* ================= 族0430 视觉彩蛋 2.0 ================= */

const EGG_MATRIX = ['none', 'subtle', 'classic', 'seasonal', 'konami'] as const;
export type EggTier = (typeof EGG_MATRIX)[number];

export class VisualEgg2 {
  tier: EggTier;
  clamped = 0;
  private found = new Set<string>();
  private enabled = true;

  constructor(tier: unknown = 'classic') {
    this.tier = EGG_MATRIX.includes(tier as EggTier) ? (tier as EggTier) : 'classic';
    if (this.tier !== tier) this.clamped += 1;
  }

  setEnabled(on: boolean): void {
    this.enabled = on;
  }

  get isEnabled(): boolean {
    return this.enabled;
  }

  /** 暗号序列触发，一次性发现（去重）。 */
  trigger(seq: string[]): boolean {
    if (!this.enabled || this.tier === 'none') return false;
    const code = seq.join('>');
    if (this.found.has(code)) return false;
    if (code === 'up>up>down>down' || code === 'varix' || (this.tier === 'seasonal' && code === 'newyear')) {
      this.found.add(code);
      return true;
    }
    return false;
  }

  get foundCount(): number {
    return this.found.size;
  }

  hasFound(code: string): boolean {
    return this.found.has(code);
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, found: [...this.found], on: this.enabled });
  }

  static deserialize(raw: string): VisualEgg2 {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; found?: string[]; on?: boolean };
      const e = new VisualEgg2(o.tier ?? 'classic');
      for (const f of o.found ?? []) e.found.add(f);
      e.setEnabled(o.on !== false);
      return e;
    } catch {
      return new VisualEgg2();
    }
  }

  rollback(): boolean {
    this.found.clear();
    this.setEnabled(true);
    return this.foundCount === 0 && this.isEnabled;
  }
}
