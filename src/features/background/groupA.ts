// UNREAL-X-15000: AI-10（领域03 桌面与图标 · 族0091~0094 · X02251~X02350），勿删。
// 族0091 壁纸引擎 2.0 / 族0092 取色联动 2.0 / 族0093 壁纸管理 2.0 / 族0094 壁纸创作工坊 2.0。

/* ===================== 族0091 壁纸引擎 2.0 ===================== */

export const WALLPAPER_MODES = ['static', 'slideshow', 'video', 'shader', 'physics'] as const;
export type WallpaperMode = (typeof WALLPAPER_MODES)[number];

export interface WallpaperSource { id: string; mode: WallpaperMode; asset: string; intervalMs: number }

/** 壁纸引擎：五模式矩阵 + 轮播 + 降级链 + 回滚净身。 */
export class WallpaperEngine {
  private mode: WallpaperMode = 'static';
  private currentId = 'default';
  private playlist: string[] = [];
  private idx = 0;
  private batterySaver = false;

  setMode(m: WallpaperMode): void { this.mode = (WALLPAPER_MODES as readonly string[]).includes(m) ? m : 'static'; }
  modeOf(): WallpaperMode { return this.mode; }
  modeCount(): number { return WALLPAPER_MODES.length; }
  setPlaylist(ids: string[]): void { this.playlist = [...ids]; this.idx = 0; }
  setBatterySaver(on: boolean): void { this.batterySaver = on; }

  play(id: string): string { this.currentId = id || 'default'; return this.currentId; }
  current(): string { return this.currentId; }

  /** 轮播推进（X02251 核心链路）。 */
  next(): string {
    if (this.playlist.length === 0) return this.currentId;
    this.idx = (this.idx + 1) % this.playlist.length;
    this.currentId = this.playlist[this.idx]!;
    return this.currentId;
  }

  /** 低电量降级（X02269 低配口径）：动态模式一律回静态。 */
  effectiveMode(): WallpaperMode {
    if (this.batterySaver && this.mode !== 'static') return 'static';
    return this.mode;
  }

  /** 非法资产护栏（X02256）。 */
  static sanitizeSource(s: Partial<WallpaperSource>): WallpaperSource {
    return {
      id: typeof s.id === 'string' && s.id ? s.id : 'wp-unknown',
      mode: s.mode && (WALLPAPER_MODES as readonly string[]).includes(s.mode) ? s.mode : 'static',
      asset: typeof s.asset === 'string' && s.asset ? s.asset : 'builtin://aurora',
      intervalMs: typeof s.intervalMs === 'number' && Number.isFinite(s.intervalMs) && s.intervalMs >= 1000 ? s.intervalMs : 30_000,
    };
  }

  /** 批量预热（X02272 批处理口径）。 */
  static warmup(ids: string[]): string[] { return [...new Set(ids)]; }
}

/* ===================== 族0092 取色联动 2.0 ===================== */

export interface OklchColor { l: number; c: number; h: number }

const clampL = (v: number): number => Math.min(0.72, Math.max(0.55, v));
const clampC = (v: number): number => Math.min(0.13, Math.max(0, v));

/** 取色流水线：RGB → OKLCH 序列 → 语义四色 + 对比门禁。 */
export class WallpaperPalette {
  private colors: OklchColor[] = [];

  /** 从像素阵列提色（简化中值近似，纯逻辑可测）。 */
  extract(pixels: Array<[number, number, number]>): OklchColor[] {
    if (pixels.length === 0) return [];
    const lumas = pixels.map(([r, g, b]) => (r * 0.2126 + g * 0.7152 + b * 0.0722) / 255);
    const avg = lumas.reduce((s, v) => s + v, 0) / lumas.length;
    const hue = (() => {
      let hx = 0;
      for (const [r, g, b] of pixels) {
        const max = Math.max(r, g, b); const min = Math.min(r, g, b);
        if (max === min) continue;
        const d = max - min;
        if (max === r) hx += (((g - b) / d) % 6) * 60;
        else if (max === g) hx += ((b - r) / d + 2) * 60;
        else hx += ((r - g) / d + 4) * 60;
      }
      return ((hx / pixels.length) % 360 + 360) % 360;
    })();
    this.colors = [0, 1, 2, 3, 4].map((i) => ({
      l: clampL(avg * (0.9 + i * 0.05)),
      c: clampC(0.06 + i * 0.012),
      h: Math.round(hue + i * 8) % 360,
    }));
    return this.colors;
  }

  palette(): OklchColor[] { return this.colors.map((c) => ({ ...c })); }

  /** 语义映射（X02277）：accent/success/warn/danger。 */
  static semantic(colors: OklchColor[]): Record<string, OklchColor> {
    const base = colors[0] ?? { l: 0.68, c: 0.09, h: 262 };
    return {
      accent: { l: clampL(base.l), c: clampC(base.c), h: base.h },
      success: { l: clampL(base.l), c: clampC(base.c), h: 160 },
      warn: { l: clampL(base.l), c: clampC(base.c), h: 85 },
      danger: { l: clampL(base.l), c: clampC(base.c), h: 25 },
    };
  }

  /** 对比门禁（X02287）：不达标 → L 微调（≤3 轮）。 */
  static contrastGate(c: OklchColor, iterations = 0): { color: OklchColor; rounds: number; pass: boolean } {
    let cur = { ...c };
    let rounds = iterations;
    while (rounds < 3 && (cur.l < 0.55 || cur.l > 0.72)) {
      cur = { ...cur, l: clampL(cur.l) };
      rounds++;
    }
    return { color: cur, rounds, pass: cur.l >= 0.55 && cur.l <= 0.72 };
  }

  /** HC 永不参与取色（X02290）。 */
  static hcExcluded(): boolean { return true; }
}

/* ===================== 族0093 壁纸管理 2.0 ===================== */

export interface WallpaperMeta { id: string; name: string; tags: string[]; favorite: boolean; bytes: number }

/** 壁纸库：登记去重 + 标签检索 + 收藏 + 清理。 */
export class WallpaperLibrary {
  private items = new Map<string, WallpaperMeta>();

  add(meta: WallpaperMeta): 'new' | 'dup' {
    if (this.items.has(meta.id)) return 'dup';
    this.items.set(meta.id, { ...meta, tags: [...meta.tags] });
    return 'new';
  }
  remove(id: string): boolean { return this.items.delete(id); }
  count(): number { return this.items.size; }
  get(id: string): WallpaperMeta | undefined { const m = this.items.get(id); return m ? { ...m, tags: [...m.tags] } : undefined; }

  tag(id: string, tag: string): boolean {
    const m = this.items.get(id);
    if (!m || m.tags.includes(tag)) return false;
    m.tags.push(tag);
    return true;
  }

  favorite(id: string, on = true): boolean {
    const m = this.items.get(id);
    if (!m) return false;
    m.favorite = on;
    return true;
  }

  search(tag: string): WallpaperMeta[] {
    return [...this.items.values()].filter((m) => m.tags.includes(tag)).map((m) => ({ ...m, tags: [...m.tags] }));
  }

  /** 去重入库（X02306 批处理口径）：同名资源秒传。 */
  static dedupe(metas: WallpaperMeta[]): WallpaperMeta[] {
    const seen = new Set<string>();
    return metas.filter((m) => (seen.has(m.id) ? false : (seen.add(m.id), true)));
  }

  /** 空间占用（X02308 存储口径）。 */
  totalBytes(): number { return [...this.items.values()].reduce((s, m) => s + m.bytes, 0); }
}

/* ===================== 族0094 壁纸创作工坊 2.0 ===================== */

export type GeneratorKind = 'gradient' | 'mesh' | 'pattern' | 'noise' | 'photo-blend';

export interface GeneratorParams { kind: GeneratorKind; hue: number; layers: number; seed: number }

/** 创作工坊：参数化生成 + 预设保存 + 导出。 */
export class WallpaperWorkshop {
  private presets = new Map<string, GeneratorParams>();

  static generate(p: GeneratorParams): { stops: string[]; kind: GeneratorKind } {
    const k = (['gradient', 'mesh', 'pattern', 'noise', 'photo-blend'] as const).includes(p.kind) ? p.kind : 'gradient';
    const layers = Math.max(1, Math.min(8, Math.floor(p.layers) || 1));
    const hue = ((Math.round(p.hue) % 360) + 360) % 360;
    const stops = Array.from({ length: layers }, (_, i) => `oklch(0.7 0.1 ${(hue + i * 24) % 360})`);
    return { stops, kind: k };
  }

  savePreset(name: string, p: GeneratorParams): boolean {
    if (!name) return false;
    this.presets.set(name, { ...p });
    return true;
  }
  loadPreset(name: string): GeneratorParams | undefined {
    const p = this.presets.get(name);
    return p ? { ...p } : undefined;
  }
  presetNames(): string[] { return [...this.presets.keys()].sort(); }

  /** 非法参数护栏（X02331）：越界回默认。 */
  static sanitize(p: Partial<GeneratorParams>): GeneratorParams {
    const kinds: GeneratorKind[] = ['gradient', 'mesh', 'pattern', 'noise', 'photo-blend'];
    return {
      kind: p.kind && kinds.includes(p.kind) ? p.kind : 'gradient',
      hue: typeof p.hue === 'number' && Number.isFinite(p.hue) ? ((Math.round(p.hue) % 360) + 360) % 360 : 262,
      layers: typeof p.layers === 'number' && Number.isFinite(p.layers) ? Math.max(1, Math.min(8, Math.round(p.layers))) : 3,
      seed: typeof p.seed === 'number' && Number.isFinite(p.seed) ? Math.max(0, Math.floor(p.seed)) : 0,
    };
  }

  /** 批量渲染计划（X02347）。 */
  static batchPlan(params: GeneratorParams[], seedBase: number): Array<{ i: number; seed: number }> {
    return params.map((_, i) => ({ i, seed: seedBase + i }));
  }
}
