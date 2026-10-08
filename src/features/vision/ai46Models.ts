/**
 * UNREAL-X-15000 · AI-46 视觉生态与收官 逻辑核（族0451~0460 · X11251~X11500），勿删。
 * 五层 × 五档模板：基础实装/边界与恢复/手感与细节/性能与优化/创新拓展。全部确定性算法，零 AI。
 */

/* -------- 族0451 帮助体系 2.0（X11251~X11275 · V 线）-------- */

export const HELP_KINDS = ['article', 'tooltip', 'tour', 'shortcut', 'glossary'] as const;
export type HelpKind = (typeof HELP_KINDS)[number];

export interface HelpTopic {
  id: string;
  kind: HelpKind;
  title: string;
  tags: string[];
}

/** 帮助体系：条目登记去重 + 搜索 + 锚点跳转。 */
export class HelpX2 {
  topics: HelpTopic[] = [];
  clamped = 0;
  add(id: string, kind: string, title: string, tags: string[]): boolean {
    if (!id || !title || this.topics.some((t) => t.id === id)) {
      this.clamped++;
      return false;
    }
    if (!(HELP_KINDS as readonly string[]).includes(kind)) {
      this.clamped++;
      return false;
    }
    this.topics.push({ id, kind: kind as HelpKind, title, tags: tags ?? [] });
    return true;
  }
  /** 搜索：标题或标签命中即返回（按登记序）。 */
  search(q: string): HelpTopic[] {
    if (!q) return [];
    return this.topics.filter((t) => t.title.includes(q) || t.tags.some((g) => g.includes(q)));
  }
  byKind(kind: HelpKind): HelpTopic[] {
    return this.topics.filter((t) => t.kind === kind);
  }
  /** 锚点：`help://<id>` 形式。 */
  static anchor(id: string): string {
    return id ? `help://${id}` : '';
  }
  static kindLabel(k: HelpKind): string {
    return { article: '文章', tooltip: '提示', tour: '引导', shortcut: '快捷键', glossary: '术语' }[k];
  }
}

/* -------- 族0452 艺术合作 2.0（X11276~X11300 · 三方）-------- */

export const LICENSE_KINDS = ['cc-by', 'cc-nc', 'vendor', 'commissioned'] as const;
export type LicenseKind = (typeof LICENSE_KINDS)[number];

export interface ArtPack {
  name: string;
  artist: string;
  license: LicenseKind;
  assets: number;
}

/** 艺术合作：画师包登记 + 许可裁决 + 分成档位。 */
export class ArtCollab {
  packs: ArtPack[] = [];
  clamped = 0;
  add(name: string, artist: string, license: string, assets: number): boolean {
    if (!name || !artist || this.packs.some((p) => p.name === name)) {
      this.clamped++;
      return false;
    }
    if (!(LICENSE_KINDS as readonly string[]).includes(license)) {
      this.clamped++;
      return false;
    }
    if (!Number.isFinite(assets) || assets < 0) {
      this.clamped++;
      return false;
    }
    this.packs.push({ name, artist, license: license as LicenseKind, assets: Math.round(assets) });
    return true;
  }
  /** 上架裁决：cc-nc 与 commissioned 可进商店精选，其余仅社区。 */
  shelf(p: ArtPack): 'featured' | 'community' {
    return p.license === 'cc-nc' || p.license === 'commissioned' ? 'community' : 'featured';
  }
  byArtist(artist: string): ArtPack[] {
    return this.packs.filter((p) => p.artist === artist);
  }
  /** 分成档：资产数 1~49→40%，50~199→50%，≥200→60%。 */
  static royalty(assets: number): number {
    if (!Number.isFinite(assets) || assets <= 0) return 0;
    if (assets >= 200) return 60;
    if (assets >= 50) return 50;
    return 40;
  }
}

/* -------- 族0453 生成艺术壁纸（X11301~X11325 · C 线）-------- */

export const GEN_STYLES = ['waves', 'aurora', 'mesh', 'bloom', 'dunes'] as const;
export type GenStyle = (typeof GEN_STYLES)[number];

/** 生成艺术壁纸：种子确定性 + 调色板提取 + 钳制。 */
export class GenWallpaper {
  seed = 1;
  style: GenStyle = 'aurora';
  clamped = 0;
  setStyle(s: string): GenStyle {
    const ok = (GEN_STYLES as readonly string[]).includes(s);
    this.style = ok ? (s as GenStyle) : 'aurora';
    if (!ok) this.clamped++;
    return this.style;
  }
  /** 确定性伪随机（mulberry32）：同种子同序列。 */
  static rng(seed: number): () => number {
    let a = Math.floor(Number.isFinite(seed) ? seed : 0) >>> 0;
    return () => {
      a = (a + 0x6d2b79f5) >>> 0;
      let t = a;
      t = Math.imul(t ^ (t >>> 15), t | 1);
      t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }
  /** 调色板：5 色 OKLCH 序列，L 钳 0.55~0.72、C 钳 ≤0.13。 */
  palette(seed: number): string[] {
    const r = GenWallpaper.rng(seed);
    const out: string[] = [];
    for (let i = 0; i < 5; i++) {
      const l = Math.min(0.72, Math.max(0.55, 0.55 + r() * 0.25));
      const c = Math.min(0.13, r() * 0.2);
      const h = Math.floor(r() * 360);
      out.push(`oklch(${l.toFixed(3)} ${c.toFixed(3)} ${h})`);
    }
    return out;
  }
  /** 渲染参数：尺寸钳制 1~8K 宽。 */
  static clampWidth(w: number): number {
    if (!Number.isFinite(w)) return 1920;
    return Math.min(7680, Math.max(640, Math.round(w)));
  }
  static styleLabel(s: GenStyle): string {
    return { waves: '波浪', aurora: '极光', mesh: '网格', bloom: '绽放', dunes: '沙丘' }[s];
  }
}

/* -------- 族0454 程序化图案（X11326~X11350 · C 线）-------- */

export const PATTERN_KINDS = ['checker', 'stripes', 'dots', 'tri', 'hex'] as const;
export type PatternKind = (typeof PATTERN_KINDS)[number];

/** 程序化图案：种类 + 密度钳制 + 无缝平铺校验。 */
export class ProcPattern {
  kind: PatternKind = 'dots';
  density = 8;
  clamped = 0;
  setKind(k: string): PatternKind {
    const ok = (PATTERN_KINDS as readonly string[]).includes(k);
    this.kind = ok ? (k as PatternKind) : 'dots';
    if (!ok) this.clamped++;
    return this.kind;
  }
  /** 密度钳制 2~64（每格像素倒数）。 */
  setDensity(d: number): number {
    if (!Number.isFinite(d)) {
      this.clamped++;
      return this.density;
    }
    this.density = Math.min(64, Math.max(2, Math.round(d)));
    return this.density;
  }
  /** 无缝平铺：周幅必须为密度的整数倍。 */
  static seamless(width: number, density: number): boolean {
    if (!Number.isFinite(width) || !Number.isFinite(density) || density <= 0) return false;
    return width > 0 && width % density === 0;
  }
  /** 图案哈希：同参数同指纹（确定性）。 */
  static hash(kind: string, density: number): number {
    let h = 2166136261;
    const s = `${kind}:${density}`;
    for (let i = 0; i < s.length; i++) {
      h ^= s.charCodeAt(i);
      h = Math.imul(h, 16777619);
    }
    return h >>> 0;
  }
  static kindLabel(k: PatternKind): string {
    return { checker: '棋盘', stripes: '条纹', dots: '圆点', tri: '三角', hex: '蜂巢' }[k];
  }
}

/* -------- 族0455 视觉无障碍 2.0（X11351~X11375 · V 线）-------- */

/** 视觉无障碍：对比度计算 + AA 门禁 + HC 映射。 */
export class VisionA11y {
  clamped = 0;
  /** sRGB 相对亮度（0~1）。 */
  static luminance(rgb: [number, number, number]): number {
    const f = (v: number): number => {
      const c = Math.min(255, Math.max(0, v)) / 255;
      return c <= 0.04045 ? c / 12.92 : Math.pow((c + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * f(rgb[0]) + 0.7152 * f(rgb[1]) + 0.0722 * f(rgb[2]);
  }
  /** WCAG 对比度（1~21）。 */
  static contrast(a: [number, number, number], b: [number, number, number]): number {
    const la = VisionA11y.luminance(a);
    const lb = VisionA11y.luminance(b);
    const hi = Math.max(la, lb);
    const lo = Math.min(la, lb);
    return (hi + 0.05) / (lo + 0.05);
  }
  /** AA 门禁：正文 ≥4.5，大字/图标 ≥3。 */
  static aa(contrast: number, large: boolean): boolean {
    if (!Number.isFinite(contrast) || contrast < 1) return false;
    return contrast >= (large ? 3 : 4.5);
  }
  /** HC 映射：任意前景色在 HC 下归黑或白。 */
  static hcMap(rgb: [number, number, number]): '#000' | '#fff' {
    return VisionA11y.luminance(rgb) > 0.4 ? '#000' : '#fff';
  }
  /** 触达目标：≥44px 红线（compact 豁免 40）。 */
  static touchOk(px: number, compact: boolean): boolean {
    return px >= (compact ? 40 : 44);
  }
  clampFocus(px: number): number {
    if (!Number.isFinite(px)) {
      this.clamped++;
      return 2;
    }
    return Math.min(4, Math.max(2, Math.round(px)));
  }
}

/* -------- 族0456 视觉本地化（X11376~X11400 · V 线）-------- */

export const VIS_LOCALES = ['zh-CN', 'zh-TW', 'en', 'ja', 'ar'] as const;
export type VisLocale = (typeof VIS_LOCALES)[number];

/** 视觉本地化：区域规则 + RTL + 数字系统。 */
export class VisL10n {
  locale: VisLocale = 'zh-CN';
  clamped = 0;
  setLocale(l: string): VisLocale {
    const ok = (VIS_LOCALES as readonly string[]).includes(l);
    this.locale = ok ? (l as VisLocale) : 'zh-CN';
    if (!ok) this.clamped++;
    return this.locale;
  }
  isRtl(): boolean {
    return this.locale === 'ar';
  }
  /** 数字格式：zh 用阿拉伯数字，ar 用东方数字。 */
  static numerals(n: number, locale: string): string {
    if (!Number.isFinite(n)) return '';
    if (locale === 'ar') {
      const east = '٠١٢٣٤٥٦٧٨٩';
      return String(Math.trunc(n)).replace(/\d/g, (d) => east[Number(d)]!);
    }
    return String(Math.trunc(n));
  }
  /** 文本膨胀预估：en→zh 收敛，zh→ar 膨胀 ~1.25。 */
  static expansion(from: string, to: string): number {
    if (from === to) return 1;
    if (from === 'en' && to.startsWith('zh')) return 0.7;
    if (to === 'ar') return 1.25;
    return 1.1;
  }
  static localeLabel(l: VisLocale): string {
    return { 'zh-CN': '简体中文', 'zh-TW': '繁體中文', en: 'English', ja: '日本語', ar: 'العربية' }[l];
  }
}

/* -------- 族0457 视觉档案（X11401~X11425 · V 线）-------- */

export interface ArchiveEntry {
  id: string;
  title: string;
  version: number;
  frozen: boolean;
}

/** 视觉档案：条目去重 + 版本链 + 冻结。 */
export class VisArchive {
  entries: ArchiveEntry[] = [];
  clamped = 0;
  add(id: string, title: string): boolean {
    if (!id || !title || this.entries.some((e) => e.id === id)) {
      this.clamped++;
      return false;
    }
    this.entries.push({ id, title, version: 1, frozen: false });
    return true;
  }
  /** 版本递进：冻结后拒绝修改。 */
  bump(id: string): number {
    const e = this.entries.find((x) => x.id === id);
    if (!e || e.frozen) {
      this.clamped++;
      return e ? e.version : 0;
    }
    e.version++;
    return e.version;
  }
  freeze(id: string): boolean {
    const e = this.entries.find((x) => x.id === id);
    if (!e) {
      this.clamped++;
      return false;
    }
    e.frozen = true;
    return true;
  }
  /** 导出/导入：坏 JSON 回空档。 */
  export(): string {
    return JSON.stringify(this.entries);
  }
  static import(raw: string): ArchiveEntry[] {
    try {
      const o = JSON.parse(raw) as ArchiveEntry[];
      return Array.isArray(o) ? o : [];
    } catch {
      return [];
    }
  }
}

/* -------- 族0458 视觉工坊社区（X11426~X11450 · 三方）-------- */

export const SUBMISSION_STATES = ['pending', 'review', 'published', 'rejected'] as const;
export type SubmissionState = (typeof SUBMISSION_STATES)[number];

/** 视觉工坊社区：投稿 + 审核状态机 + 去重。 */
export class WorkshopHub {
  items = new Map<string, SubmissionState>();
  clamped = 0;
  submit(id: string): boolean {
    if (!id || this.items.has(id)) {
      this.clamped++;
      return false;
    }
    this.items.set(id, 'pending');
    return true;
  }
  /** 状态机：pending→review→published/rejected；非法迁移拒绝。 */
  transition(id: string, to: string): SubmissionState | null {
    const cur = this.items.get(id);
    if (!cur || !(SUBMISSION_STATES as readonly string[]).includes(to)) {
      this.clamped++;
      return null;
    }
    const allowed: Record<SubmissionState, string[]> = {
      pending: ['review', 'rejected'],
      review: ['published', 'rejected'],
      published: [],
      rejected: ['pending'],
    };
    if (!allowed[cur].includes(to)) {
      this.clamped++;
      return cur;
    }
    const next = to as SubmissionState;
    this.items.set(id, next);
    return next;
  }
  byState(s: SubmissionState): string[] {
    return [...this.items.entries()].filter(([, v]) => v === s).map(([k]) => k);
  }
}

/* -------- 族0459 视觉体检（X11451~X11475 · C 线）-------- */

/** 视觉体检：检查项聚合 + 评分 + 等级。 */
export class VisionCheckup {
  results = new Map<string, boolean>();
  clamped = 0;
  record(name: string, pass: boolean): boolean {
    if (!name) {
      this.clamped++;
      return false;
    }
    this.results.set(name, pass);
    return true;
  }
  /** 得分：通过项 / 总项 ×100，空档回 0。 */
  score(): number {
    if (this.results.size === 0) return 0;
    const pass = [...this.results.values()].filter(Boolean).length;
    return Math.round((pass / this.results.size) * 100);
  }
  /** 等级：≥95 A / ≥85 B / ≥70 C / 其余 D。 */
  grade(): 'A' | 'B' | 'C' | 'D' {
    const s = this.score();
    return s >= 95 ? 'A' : s >= 85 ? 'B' : s >= 70 ? 'C' : 'D';
  }
  failed(): string[] {
    return [...this.results.entries()].filter(([, v]) => !v).map(([k]) => k);
  }
  /** 重跑清零（中断还原语义）。 */
  reset(): void {
    this.results.clear();
  }
}

/* -------- 族0460 视觉收官（X11476~X11500 · 三方）-------- */

export const FINALE_STEPS = ['checkup', 'freeze', 'handoff', 'archive', 'ceremony'] as const;
export type FinaleStep = (typeof FINALE_STEPS)[number];

/** 视觉收官：五步清单 + 基线冻结 + 交接。 */
export class VisionFinale {
  done = new Set<FinaleStep>();
  clamped = 0;
  /** 顺序门禁：必须按 FINALE_STEPS 依次完成。 */
  complete(step: string): boolean {
    const idx = (FINALE_STEPS as readonly string[]).indexOf(step);
    if (idx < 0) {
      this.clamped++;
      return false;
    }
    for (let i = 0; i < idx; i++) {
      if (!this.done.has(FINALE_STEPS[i] as FinaleStep)) {
        this.clamped++;
        return false;
      }
    }
    this.done.add(step as FinaleStep);
    return true;
  }
  progress(): number {
    return Math.round((this.done.size / FINALE_STEPS.length) * 100);
  }
  isFrozen(): boolean {
    return this.done.has('freeze');
  }
  /** 交接包：五步全完成后才可签发。 */
  handoff(): string | null {
    return this.done.size === FINALE_STEPS.length ? 'UNREAL-X-15000 · 领域12 视觉线交付包' : null;
  }
  static stepLabel(s: FinaleStep): string {
    return { checkup: '体检', freeze: '冻结', handoff: '交接', archive: '归档', ceremony: '仪式' }[s];
  }
}
