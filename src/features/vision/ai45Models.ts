/**
 * UNREAL-X-15000 · AI-45 视觉内核与质量 逻辑核（族0441~0450 · X11001~X11250），勿删。
 * 五层 × 五档模板：基础实装/边界与恢复/手感与细节/性能与优化/创新拓展。全部确定性算法，零 AI。
 */

/* -------- 族0441 GPU 合成艺术（X11001~X11025 · K 线口径）-------- */

export const COMPOSITE_TIERS = ['solid', 'flat', 'depth', 'glass', 'aurora'] as const;
export type CompositeTier = (typeof COMPOSITE_TIERS)[number];

/** GPU 合成艺术：档位矩阵 + 参数钳制 + 快照迁移。 */
export class GpuComposite {
  tier: CompositeTier = 'solid';
  blur = 0;
  saturation = 1;
  clamped = 0;
  setTier(t: string): CompositeTier {
    const ok = (COMPOSITE_TIERS as readonly string[]).includes(t);
    this.tier = ok ? (t as CompositeTier) : 'solid';
    if (!ok) this.clamped++;
    return this.tier;
  }
  /** 参数钳制：blur 0~28px，saturation 0.5~2.0。 */
  setParams(blur: number, sat: number): { blur: number; saturation: number } {
    const b = Number.isFinite(blur) ? Math.min(28, Math.max(0, blur)) : 0;
    const s = Number.isFinite(sat) ? Math.min(2, Math.max(0.5, sat)) : 1;
    if (b !== blur || s !== sat || !Number.isFinite(blur) || !Number.isFinite(sat)) this.clamped++;
    this.blur = b;
    this.saturation = s;
    return { blur: b, saturation: s };
  }
  /** 低配降级链：aurora→glass→flat→solid。 */
  degrade(): CompositeTier {
    const i = COMPOSITE_TIERS.indexOf(this.tier);
    const next = COMPOSITE_TIERS[Math.max(0, i - 1)] as CompositeTier;
    this.tier = next;
    return next;
  }
  snapshot(): string {
    return JSON.stringify({ tier: this.tier, blur: this.blur, saturation: this.saturation });
  }
  restore(raw: string): boolean {
    try {
      const o = JSON.parse(raw) as { tier?: string; blur?: number; saturation?: number };
      this.setTier(String(o.tier ?? 'solid'));
      this.setParams(Number(o.blur ?? 0), Number(o.saturation ?? 1));
      return true;
    } catch {
      return false;
    }
  }
  static tierLabel(t: CompositeTier): string {
    return { solid: '纯色', flat: '平面', depth: '景深', glass: '玻璃', aurora: '极光' }[t];
  }
}

/* -------- 族0442 着色器效果库（X11026~X11050 · K 线口径）-------- */

export const SHADER_KINDS = ['bloom', 'grain', 'ca', 'vignette', 'chromakey'] as const;
export type ShaderKind = (typeof SHADER_KINDS)[number];

export interface ShaderEntry {
  name: string;
  kind: ShaderKind;
  intensity: number;
}

/** 着色器效果库：登记去重 + 强度钳制 + 编译校验。 */
export class ShaderLib {
  effects: ShaderEntry[] = [];
  clamped = 0;
  /** 登记自带去重：同名拒绝。 */
  add(name: string, kind: string, intensity: number): boolean {
    if (!name || this.effects.some((e) => e.name === name)) {
      this.clamped++;
      return false;
    }
    if (!(SHADER_KINDS as readonly string[]).includes(kind)) {
      this.clamped++;
      return false;
    }
    const i = Number.isFinite(intensity) ? Math.min(1, Math.max(0, intensity)) : 0;
    this.effects.push({ name, kind: kind as ShaderKind, intensity: i });
    return true;
  }
  /** 编译校验：GLSL 片段必须含 main 且大括号配对。 */
  static validate(src: string): boolean {
    if (!src.includes('main')) return false;
    let depth = 0;
    for (const ch of src) {
      if (ch === '{') depth++;
      if (ch === '}') depth--;
      if (depth < 0) return false;
    }
    return depth === 0;
  }
  byKind(kind: ShaderKind): ShaderEntry[] {
    return this.effects.filter((e) => e.kind === kind);
  }
  /** 渲染链：按登记序组合。 */
  chain(): string[] {
    return this.effects.map((e) => e.name);
  }
  static kindLabel(k: ShaderKind): string {
    return { bloom: '辉光', grain: '噪点', ca: '色差', vignette: '暗角', chromakey: '色键' }[k];
  }
}

/* -------- 族0443 色彩管理内核侧（X11051~X11075 · K 线口径）-------- */

export const COLOR_INTENTS = ['perceptual', 'relative', 'saturation', 'absolute'] as const;
export type ColorIntent = (typeof COLOR_INTENTS)[number];
export const GAMUT_CHAIN = ['p3', 'adobergb', 'srgb'] as const;
export type Gamut = (typeof GAMUT_CHAIN)[number];

/** 色彩管理：OKLCH 钳制 + 色域回退 + 渲染意图。 */
export class ColorMgmt {
  intent: ColorIntent = 'relative';
  gamut: Gamut = 'srgb';
  clamped = 0;
  setIntent(i: string): ColorIntent {
    const ok = (COLOR_INTENTS as readonly string[]).includes(i);
    this.intent = ok ? (i as ColorIntent) : 'relative';
    if (!ok) this.clamped++;
    return this.intent;
  }
  /** 色域协商：设备能力列表按链回退。 */
  negotiate(device: string[]): Gamut {
    if (!Array.isArray(device)) {
      this.clamped++;
      this.gamut = 'srgb';
      return this.gamut;
    }
    for (const g of GAMUT_CHAIN) {
      if (device.includes(g)) {
        this.gamut = g;
        return g;
      }
    }
    this.gamut = 'srgb';
    return this.gamut;
  }
  /** OKLCH 钳制：L 0~1、C ≤0.13（取色纪律）、H 归一 0~360。 */
  static clampOklch(l: number, c: number, h: number): { l: number; c: number; h: number } {
    const cl = Number.isFinite(l) ? Math.min(1, Math.max(0, l)) : 0.5;
    const cc = Number.isFinite(c) ? Math.min(0.13, Math.max(0, c)) : 0;
    const ch = Number.isFinite(h) ? ((h % 360) + 360) % 360 : 0;
    return { l: cl, c: cc, h: ch };
  }
  /** 配置文件序列化：坏输入回默认。 */
  static parse(raw: string): { intent: ColorIntent; gamut: Gamut } {
    try {
      const o = JSON.parse(raw) as { intent?: string; gamut?: string };
      return {
        intent: (COLOR_INTENTS as readonly string[]).includes(String(o.intent)) ? (o.intent as ColorIntent) : 'relative',
        gamut: (GAMUT_CHAIN as readonly string[]).includes(String(o.gamut)) ? (o.gamut as Gamut) : 'srgb',
      };
    } catch {
      return { intent: 'relative', gamut: 'srgb' };
    }
  }
}

/* -------- 族0444 HDR 管线（X11076~X11100 · K 线口径）-------- */

export const HDR_MODE_CHAIN = ['hdr10', 'hlg', 'sdr-high', 'sdr'] as const;
export type HdrMode = (typeof HDR_MODE_CHAIN)[number];

/** HDR 管线：模式协商 + 亮度钳制 + 色调映射。 */
export class HdrPipe {
  mode: HdrMode = 'sdr';
  maxNits = 300;
  clamped = 0;
  /** 模式协商：按面板能力回退。 */
  negotiate(panel: string[]): HdrMode {
    if (!Array.isArray(panel)) {
      this.clamped++;
      this.mode = 'sdr';
      return this.mode;
    }
    for (const m of HDR_MODE_CHAIN) {
      if (panel.includes(m)) {
        this.mode = m;
        return m;
      }
    }
    this.mode = 'sdr';
    return this.mode;
  }
  /** 峰值亮度钳制（100~10000 nits），HDR 最低 600。 */
  setMaxNits(n: number): number {
    if (!Number.isFinite(n)) {
      this.clamped++;
      return this.maxNits;
    }
    const floor = this.mode === 'sdr' ? 100 : 600;
    this.maxNits = Math.min(10000, Math.max(floor, Math.round(n)));
    return this.maxNits;
  }
  /** 色调映射：SDR 显示时 HDR 亮度压缩到 0~1。 */
  tonemap(nits: number): number {
    if (!Number.isFinite(nits) || nits <= 0) {
      this.clamped++;
      return 0;
    }
    if (this.mode === 'sdr' || this.mode === 'sdr-high') return Math.min(1, nits / this.maxNits);
    return Math.min(1, nits / 10000);
  }
  isHdr(): boolean {
    return this.mode === 'hdr10' || this.mode === 'hlg';
  }
  static modeLabel(m: HdrMode): string {
    return { hdr10: 'HDR10', hlg: 'HLG', 'sdr-high': '高亮 SDR', sdr: '标准 SDR' }[m];
  }
}

/* -------- 族0445 视觉基准（X11101~X11125 · C 线口径）-------- */

export interface BenchRecord {
  name: string;
  ms: number;
}

/** 视觉基准：用例登记去重 + 预算表 + 超标判定。 */
export class VisionBench {
  budget: Record<string, number> = { firstFrame: 300, openLayer: 240, tabSwitch: 120, themeSwitch: 200 };
  records: BenchRecord[] = [];
  clamped = 0;
  record(name: string, ms: number): boolean {
    if (!name || !(name in this.budget) || !Number.isFinite(ms) || ms < 0) {
      this.clamped++;
      return false;
    }
    if (this.records.some((r) => r.name === name && r.ms === ms)) return false;
    this.records.push({ name, ms });
    return true;
  }
  /** 超预算项：记录毫秒 > 预算即红。 */
  violations(): string[] {
    return this.records.filter((r) => r.ms > this.budget[r.name]!).map((r) => r.name);
  }
  /** 批量跑 N 轮取 P95（简化：排序取 95 分位）。 */
  static p95(samples: number[]): number {
    const s = samples.filter((n) => Number.isFinite(n) && n >= 0).sort((a, b) => a - b);
    if (s.length === 0) return 0;
    const idx = Math.min(s.length - 1, Math.ceil(s.length * 0.95) - 1);
    return s[idx]!;
  }
  setBudget(name: string, ms: number): boolean {
    if (!name || !Number.isFinite(ms) || ms <= 0) {
      this.clamped++;
      return false;
    }
    this.budget[name] = ms;
    return true;
  }
}

/* -------- 族0446 视觉回归（X11126~X11150 · C 线口径）-------- */

/** 视觉回归：像素 diff 比率 + 0.1% 阈值 + 基线更新纪律。 */
export class VisionDiff {
  baselines = new Map<string, number[]>();
  threshold = 0.001;
  clamped = 0;
  setBaseline(page: string, pixels: number[]): boolean {
    if (!page || pixels.length === 0) {
      this.clamped++;
      return false;
    }
    this.baselines.set(page, [...pixels]);
    return true;
  }
  /** 归一化差异比率：逐像素比，返回差异占比 0~1。 */
  static ratio(a: number[], b: number[]): number {
    if (a.length !== b.length || a.length === 0) return 1;
    let diff = 0;
    for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) diff++;
    return diff / a.length;
  }
  /** 判定：比率 > 阈值即红。 */
  judge(page: string, current: number[]): { pass: boolean; ratio: number } {
    const base = this.baselines.get(page);
    if (!base) {
      this.clamped++;
      return { pass: false, ratio: 1 };
    }
    const r = VisionDiff.ratio(base, current);
    return { pass: r <= this.threshold, ratio: r };
  }
  /** 有意的视觉变更才允许更新基线（防只增不审）。 */
  updateBaseline(page: string, pixels: number[], acknowledged: boolean): boolean {
    if (!acknowledged) {
      this.clamped++;
      return false;
    }
    return this.setBaseline(page, pixels);
  }
}

/* -------- 族0447 主题兼容矩阵（X11151~X11175 · C 线口径）-------- */

export const THEME_IDS = ['dark', 'light', 'high-contrast'] as const;
export type ThemeId = (typeof THEME_IDS)[number];
export const DENSITY_IDS = ['compact', 'standard', 'comfortable'] as const;
export type DensityId = (typeof DENSITY_IDS)[number];
export const MATERIALS = ['m-solid', 'm-frosted', 'm-acrylic', 'm-mica', 'm-glow'] as const;
export type MaterialId = (typeof MATERIALS)[number];

/** 主题兼容矩阵：3 主题 × 3 密度 × 5 材质，HC 材质一律降级 solid。 */
export class ThemeMatrix {
  clamped = 0;
  cell(theme: string, density: string, mat: string): { theme: ThemeId; density: DensityId; material: MaterialId } {
    const t = (THEME_IDS as readonly string[]).includes(theme) ? (theme as ThemeId) : 'dark';
    const d = (DENSITY_IDS as readonly string[]).includes(density) ? (density as DensityId) : 'standard';
    const m = (MATERIALS as readonly string[]).includes(mat) ? (mat as MaterialId) : 'm-solid';
    if (t !== theme || d !== density || m !== mat) this.clamped++;
    return { theme: t, density: d, material: t === 'high-contrast' ? 'm-solid' : m };
  }
  /** 矩阵全量：HC 行强制 solid，共 3×3×5=45 格。 */
  all(): { theme: ThemeId; density: DensityId; material: MaterialId }[] {
    const out: { theme: ThemeId; density: DensityId; material: MaterialId }[] = [];
    for (const t of THEME_IDS) for (const d of DENSITY_IDS) for (const m of MATERIALS) out.push(this.cell(t, d, m));
    return out;
  }
  /** 矩阵一致性：HC 行 15 格全 solid。 */
  hcSolidCount(): number {
    return this.all().filter((c) => c.theme === 'high-contrast' && c.material === 'm-solid').length;
  }
}

/* -------- 族0448 视觉遥测（X11176~X11200 · C 线口径）-------- */

export interface VisionEvent {
  kind: string;
  ms: number;
  at: number;
}

/** 视觉遥测：环形缓冲 + 脱敏 + 采样钳制。 */
export class VisionTelem {
  private buf: VisionEvent[] = [];
  capacity: number;
  sampled = 1;
  clamped = 0;
  constructor(capacity = 128) {
    this.capacity = capacity > 0 ? capacity : 128;
  }
  push(kind: string, ms: number, at: number): boolean {
    if (!kind || !Number.isFinite(ms) || !Number.isFinite(at)) {
      this.clamped++;
      return false;
    }
    if (this.sampled <= 0) return false;
    this.buf.push({ kind, ms, at });
    if (this.buf.length > this.capacity) this.buf.shift();
    return true;
  }
  /** 脱敏：事件名不携带路径/URL/用户片段。 */
  static sanitize(kind: string): string {
    return kind.replace(/[\\/]/g, '_').slice(0, 32);
  }
  byKind(kind: string): VisionEvent[] {
    return this.buf.filter((e) => e.kind === kind);
  }
  /** 平均耗时：空集回 0。 */
  avgMs(kind: string): number {
    const evs = this.byKind(kind);
    if (evs.length === 0) return 0;
    return evs.reduce((s, e) => s + e.ms, 0) / evs.length;
  }
  setSampled(rate: number): number {
    if (!Number.isFinite(rate)) {
      this.clamped++;
      return this.sampled;
    }
    this.sampled = Math.min(1, Math.max(0, rate));
    return this.sampled;
  }
}

/* -------- 族0449 第一印象打磨 2.0（X11201~X11225 · V 线）-------- */

export const IMPRESSION_LAYERS = ['logo', 'wordmark', 'countdown', 'hero', 'welcome'] as const;
export type ImpressionLayer = (typeof IMPRESSION_LAYERS)[number];

/** 第一印象打磨：呼吸缩放钳制 + 欢迎页配置 + reduce-motion。 */
export class FirstImpression {
  layers = new Map<ImpressionLayer, { visible: boolean; breathe: number }>();
  reducedMotion = false;
  clamped = 0;
  constructor() {
    for (const l of IMPRESSION_LAYERS) this.layers.set(l, { visible: true, breathe: 1 });
  }
  setLayer(l: string, visible: boolean, breathe: number): boolean {
    if (!(IMPRESSION_LAYERS as readonly string[]).includes(l)) {
      this.clamped++;
      return false;
    }
    const b = Number.isFinite(breathe) ? Math.min(1.08, Math.max(1, breathe)) : 1;
    this.layers.set(l as ImpressionLayer, { visible, breathe: b });
    return true;
  }
  /** reduce-motion：全部呼吸归 1、动效降纯淡入淡出。 */
  applyReduceMotion(): void {
    this.reducedMotion = true;
    for (const [k, v] of this.layers) this.layers.set(k, { ...v, breathe: 1 });
  }
  visibleLayers(): ImpressionLayer[] {
    return IMPRESSION_LAYERS.filter((l) => this.layers.get(l)!.visible);
  }
  /** 首帧编排：logo→wordmark→hero 依次可见。 */
  static choreography(): ImpressionLayer[] {
    return ['logo', 'wordmark', 'hero'];
  }
}

/* -------- 族0450 微文案 2.0（X11226~X11250 · V 线）-------- */

export const COPY_TONES = ['plain', 'warm', 'pro'] as const;
export type CopyTone = (typeof COPY_TONES)[number];

/** 微文案：语气登记 + 长度钳制 + 中文标点纪律。 */
export class MicroCopy {
  tone: CopyTone = 'plain';
  dict = new Map<string, string>();
  clamped = 0;
  setTone(t: string): CopyTone {
    const ok = (COPY_TONES as readonly string[]).includes(t);
    this.tone = ok ? (t as CopyTone) : 'plain';
    if (!ok) this.clamped++;
    return this.tone;
  }
  /** 登记：去重覆盖 + 长度钳制 48 字。 */
  set(key: string, text: string): string {
    if (!key) {
      this.clamped++;
      return '';
    }
    const t = text.length > 48 ? `${text.slice(0, 47)}…` : text;
    this.dict.set(key, t);
    return t;
  }
  get(key: string): string {
    return this.dict.get(key) ?? '';
  }
  /** 中文文案纪律：不以半角逗号/句号结尾、不含裸英文省略号。 */
  static zhOk(text: string): boolean {
    if (/[,;.?!]$/.test(text)) return false;
    if (text.includes('...')) return false;
    return text.length > 0;
  }
  /** 错误叙事模板：失败 + 原因 + 下一步。 */
  static failure(what: string, why: string, next: string): string {
    const w = what || '操作';
    return `「${w}」未完成：${why}。${next ? `试试：${next}。` : '可稍后重试。'}`;
  }
}
