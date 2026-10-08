/**
 * UNREAL-X-15000 · AI-50 无障碍五域 逻辑核（族0491~0500 · X12251~X12500），勿删。
 * 五层 × 五档模板：基础实装/边界与恢复/手感与细节/性能与优化/创新拓展。全部确定性算法，零 AI。
 */

/* -------- 族0491 视觉无障碍 2.0（X12251~X12275 · V 线口径）-------- */

export const TEXT_SCALES = [1, 1.1, 1.25, 1.5, 2] as const;

/** 视觉无障碍：对比度计算 + 缩放档 + HC 映射。 */
export class VisualA11y {
  scale: number = 1;
  hc = false;
  clamped = 0;

  /** WCAG 相对亮度。 */
  static lum(r: number, g: number, b: number): number {
    const f = (c: number) => {
      const v = c / 255;
      return v <= 0.03928 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
  }
  /** 对比度 1~21。 */
  static contrast(a: [number, number, number], b: [number, number, number]): number {
    const l1 = VisualA11y.lum(...a);
    const l2 = VisualA11y.lum(...b);
    const hi = Math.max(l1, l2);
    const lo = Math.min(l1, l2);
    return Math.round(((hi + 0.05) / (lo + 0.05)) * 100) / 100;
  }
  /** 正文 ≥4.5 达标。 */
  static aaText(a: [number, number, number], b: [number, number, number]): boolean {
    return VisualA11y.contrast(a, b) >= 4.5;
  }
  /** 缩放档钳制（仅允许五档）。 */
  setScale(s: number): number {
    const hit = (TEXT_SCALES as readonly number[]).includes(s);
    this.scale = hit ? s : 1;
    if (!hit) this.clamped++;
    return this.scale;
  }
  toggleHc(): boolean {
    this.hc = !this.hc;
    return this.hc;
  }
  /** HC 映射：材质→纯色。 */
  materialMap(m: string): string {
    if (!this.hc) return m;
    return m === 'solid' ? 'solid' : 'solid';
  }
  snapshot(): string {
    return JSON.stringify({ scale: this.scale, hc: this.hc });
  }
  restore(raw: string): boolean {
    try {
      const o = JSON.parse(raw) as { scale?: number; hc?: boolean };
      this.setScale(Number(o.scale ?? 1));
      this.hc = o.hc === true;
      return true;
    } catch {
      return false;
    }
  }
}

/* -------- 族0492 听觉无障碍 2.0（X12276~X12300 · V 线口径）-------- */

export const ALERT_KINDS = ['flash', 'banner', 'vibrate', 'caption', 'legend'] as const;
export type AlertKind = (typeof ALERT_KINDS)[number];

/** 听觉无障碍：单声道混音 + 字幕延迟钳制 + 视觉告警登记。 */
export class HearA11y {
  mono = false;
  captionDelayMs = 0;
  alerts: string[] = [];
  clamped = 0;

  toggleMono(): boolean {
    this.mono = !this.mono;
    return this.mono;
  }
  /** 单声道混音：左右平均。 */
  mix(l: number, r: number): number {
    if (!this.mono) return l;
    const v = (Number.isFinite(l) ? l : 0) / 2 + (Number.isFinite(r) ? r : 0) / 2;
    return Math.round(v * 100) / 100;
  }
  /** 字幕延迟钳制 0~5000ms。 */
  setCaptionDelay(ms: number): number {
    const v = Number.isFinite(ms) ? Math.min(5000, Math.max(0, ms)) : 0;
    if (v !== ms || !Number.isFinite(ms)) this.clamped++;
    this.captionDelayMs = v;
    return v;
  }
  /** 视觉告警登记（去重）。 */
  addAlert(kind: string): boolean {
    if (!(ALERT_KINDS as readonly string[]).includes(kind)) { this.clamped++; return false; }
    if (this.alerts.includes(kind)) return false;
    this.alerts.push(kind);
    return true;
  }
  /** 双通道保障：声音之外至少一种视觉等价通道。 */
  hasEquivalent(): boolean {
    return this.alerts.length > 0;
  }
  reset(): void {
    this.alerts = [];
    this.mono = false;
  }
}

/* -------- 族0493 运动无障碍 2.0（X12301~X12325 · V 线口径）-------- */

export const TARGET_MIN = 44;
export const TARGET_COMPACT_EXEMPT = 40;

/** 运动无障碍：停留点击 + 粘滞键序列 + 触达红线。 */
export class MotorA11y {
  dwellMs = 800;
  stickySeq: string[] = [];
  clamped = 0;

  /** 停留时长钳制 200~3000ms。 */
  setDwell(ms: number): number {
    const v = Number.isFinite(ms) ? Math.min(3000, Math.max(200, ms)) : 800;
    if (v !== ms || !Number.isFinite(ms)) this.clamped++;
    this.dwellMs = v;
    return v;
  }
  /** 停留达标判定。 */
  dwellOk(held: number): boolean {
    return held >= this.dwellMs;
  }
  /** 粘滞键：修饰键入序列，5 键封顶成组合。 */
  pressModifier(key: string): number {
    if (!['ctrl', 'shift', 'alt', 'win'].includes(key)) { this.clamped++; return this.stickySeq.length; }
    if (this.stickySeq.length < 5) this.stickySeq.push(key);
    return this.stickySeq.length;
  }
  comboReady(): boolean {
    return this.stickySeq.length >= 2;
  }
  clearSticky(): void {
    this.stickySeq = [];
  }
  /** 触达红线：标准 44，compact 豁免 40。 */
  static targetOk(size: number, compact: boolean): boolean {
    return compact ? size >= TARGET_COMPACT_EXEMPT : size >= TARGET_MIN;
  }
}

/* -------- 族0494 认知无障碍 2.0（X12326~X12350 · V 线口径）-------- */

export const READ_LEVELS = ['plain', 'simple', 'guided'] as const;
export type ReadLevel = (typeof READ_LEVELS)[number];

/** 认知无障碍：易读档位 + 分心守护 + 专注计时。 */
export class CogA11y {
  level: ReadLevel = 'plain';
  distractions = 0;
  focusMin = 0;
  clamped = 0;

  setLevel(l: string): ReadLevel {
    const ok = (READ_LEVELS as readonly string[]).includes(l);
    this.level = ok ? (l as ReadLevel) : 'plain';
    if (!ok) this.clamped++;
    return this.level;
  }
  /** 分心守护：屏蔽名单登记去重。 */
  private blocked: string[] = [];
  block(app: string): boolean {
    if (!app || this.blocked.includes(app)) return false;
    this.blocked.push(app);
    this.distractions++;
    return true;
  }
  isBlocked(app: string): boolean {
    return this.blocked.includes(app);
  }
  /** 专注计时钳制 1~120 分钟。 */
  setFocus(min: number): number {
    const v = Number.isFinite(min) ? Math.min(120, Math.max(1, min)) : 25;
    if (v !== min || !Number.isFinite(min)) this.clamped++;
    this.focusMin = v;
    return v;
  }
  /** 简化措辞映射（确定性表）。 */
  simplify(term: string): string {
    const map: Record<string, string> = {
      '初始化': '开始设置', '同步': '更新一致', '渲染': '画出来', '校验': '检查', '遥测': '使用记录',
    };
    return map[term] ?? term;
  }
  reset(): void {
    this.blocked = [];
    this.distractions = 0;
    this.focusMin = 0;
  }
}

/* -------- 族0495 语音无障碍 2.0（X12351~X12375 · V 线口径）-------- */

export const VOICE_ROLES = ['tts', 'stt', 'command', 'dictation', 'screen-reader'] as const;
export type VoiceRole = (typeof VOICE_ROLES)[number];

/** 语音无障碍：语速钳制 + 语音登记去重 + SSML 校验。 */
export class VoiceA11y {
  rate = 1;
  voices = new Map<string, VoiceRole>();
  clamped = 0;

  /** 语速钳制 0.5~3。 */
  setRate(r: number): number {
    const v = Number.isFinite(r) ? Math.min(3, Math.max(0.5, r)) : 1;
    if (v !== r || !Number.isFinite(r)) this.clamped++;
    this.rate = v;
    return v;
  }
  /** 语音登记去重。 */
  register(id: string, role: string): boolean {
    if (!id || this.voices.has(id)) return false;
    const ok = (VOICE_ROLES as readonly string[]).includes(role);
    if (!ok) { this.clamped++; return false; }
    this.voices.set(id, role as VoiceRole);
    return true;
  }
  byRole(role: string): string[] {
    return [...this.voices.entries()].filter(([, r]) => r === role).map(([id]) => id);
  }
  /** SSML 轻校验：标签配对。 */
  static ssmlOk(s: string): boolean {
    const open = (s.match(/<speak>/g) ?? []).length;
    const close = (s.match(/<\/speak>/g) ?? []).length;
    return open === 1 && close === 1 && s.startsWith('<speak>') && s.endsWith('</speak>');
  }
  revoke(id: string): boolean {
    return this.voices.delete(id);
  }
}

/* -------- 族0496 本地化核心 2.0（X12376~X12400 · V 线口径）-------- */

export const PLURAL_CATEGORIES = ['zero', 'one', 'two', 'few', 'many', 'other'] as const;
export type PluralCategory = (typeof PLURAL_CATEGORIES)[number];

/** 本地化核心：语言标签校验 + 复数规则 + 回退链。 */
export class L10nCore {
  fallbacks: string[] = [];
  clamped = 0;

  /** BCP-47 轻校验：language[-Script][-REGION]。 */
  static tagOk(tag: string): boolean {
    if (!/^[a-z]{2,3}(-[A-Z][a-z]{3})?(-[A-Z]{2})?$/.test(tag)) return false;
    return true;
  }
  /** 复数类别（英文简化规则）。 */
  static pluralEn(n: number): PluralCategory {
    if (n === 1) return 'one';
    return 'other';
  }
  /** 复数类别（中文恒 other）。 */
  static pluralZh(): PluralCategory {
    return 'other';
  }
  /** 回退链登记去重。 */
  addFallback(tag: string): boolean {
    if (!L10nCore.tagOk(tag)) { this.clamped++; return false; }
    if (this.fallbacks.includes(tag)) return false;
    this.fallbacks.push(tag);
    return true;
  }
  /** 解析：沿回退链找第一个命中。 */
  resolve(tag: string, available: string[]): string {
    const chain = [tag, ...this.fallbacks];
    for (const t of chain) {
      if (available.includes(t)) return t;
    }
    return available[0] ?? '';
  }
  isRtl(tag: string): boolean {
    return ['ar', 'he', 'fa', 'ur'].includes(tag.split('-')[0] ?? '');
  }
  reset(): void {
    this.fallbacks = [];
  }
}

/* -------- 族0497 中文本地化深化 2.0（X12401~X12425 · V 线口径）-------- */

/** 中文深化：标点规范 + 数字缩写 + 日期中文。 */
export class ZhL10n {
  clamped = 0;

  /** 半角标点转全角。 */
  static punct(s: string): string {
    const map: Record<string, string> = { ',': '，', '.': '。', '?': '？', '!': '！', ':': '：', ';': '；' };
    return s.replace(/[,.?!:;]/g, (c) => map[c] ?? c);
  }
  /** 中文语境禁省略号三点（用六点 ……）。 */
  static ellipsisOk(s: string): boolean {
    return !s.includes('...') || s.includes('……');
  }
  /** 大数缩写：万/亿。 */
  static abbr(n: number): string {
    const v = Number.isFinite(n) ? Math.max(0, n) : 0;
    if (v >= 1e8) return `${Math.round(v / 1e8 * 100) / 100}亿`;
    if (v >= 1e4) return `${Math.round(v / 1e4 * 100) / 100}万`;
    return String(Math.round(v));
  }
  /** 中文数字日期。 */
  static dateCn(y: number, m: number, d: number): string {
    const mm = Math.min(12, Math.max(1, Math.trunc(m)));
    const dd = Math.min(31, Math.max(1, Math.trunc(d)));
    return `${Math.trunc(y)}年${mm}月${dd}日`;
  }
  /** 引号规范：中文用「」。 */
  static quote(s: string): string {
    return `「${s}」`;
  }
  /** 长度克制：超 48 字截断加省略。 */
  static trim(s: string, max = 48): string {
    if (s.length <= max) return s;
    return `${s.slice(0, max)}……`;
  }
}

/* -------- 族0498 多语言字体 2.0（X12426~X12450 · V 线口径）-------- */

export const FONT_SCRIPTS = ['latin', 'cjk', 'arabic', 'devanagari', 'cyrillic'] as const;
export type FontScript = (typeof FONT_SCRIPTS)[number];

/** 多语言字体：字族登记去重 + 覆盖检查 + 回退链。 */
export class FontL10n {
  stacks = new Map<string, FontScript[]>();
  clamped = 0;

  /** 字族登记（去重，脚本过滤）。 */
  add(id: string, scripts: string[]): boolean {
    if (!id || this.stacks.has(id)) return false;
    const clean = [...new Set(scripts)].filter((s): s is FontScript => {
      const ok = (FONT_SCRIPTS as readonly string[]).includes(s);
      if (!ok) this.clamped++;
      return ok;
    });
    this.stacks.set(id, clean);
    return true;
  }
  /** 覆盖检查：所需脚本全部在列。 */
  covers(id: string, need: FontScript[]): boolean {
    const s = this.stacks.get(id);
    if (!s) return false;
    return need.every((n) => s.includes(n));
  }
  /** 回退链：按脚本找第一个覆盖字族。 */
  resolve(need: FontScript): string {
    for (const [id, s] of this.stacks) {
      if (s.includes(need)) return id;
    }
    return '';
  }
  remove(id: string): boolean {
    return this.stacks.delete(id);
  }
  /** 字号钳制 9~72。 */
  static sizeOk(px: number): number {
    const v = Number.isFinite(px) ? Math.min(72, Math.max(9, px)) : 12;
    return v;
  }
}

/* -------- 族0499 语音本地化 2.0（X12451~X12475 · V 线口径）-------- */

/** 语音本地化：区域语音映射 + 语速档 + 口音标签。 */
export class VoiceL10n {
  private map = new Map<string, string>();
  clamped = 0;

  /** 区域语音登记（locale → voiceId，去重覆盖）。 */
  bind(locale: string, voiceId: string): boolean {
    if (!L10nCore.tagOk(locale) || !voiceId) { this.clamped++; return false; }
    this.map.set(locale, voiceId);
    return true;
  }
  /** 精确命中。 */
  exact(locale: string): string {
    return this.map.get(locale) ?? '';
  }
  /** 语言级回退：zh-CN → zh。 */
  resolve(locale: string): string {
    const hit = this.map.get(locale);
    if (hit) return hit;
    const lang = locale.split('-')[0] ?? '';
    for (const [k, v] of this.map) {
      if (k.split('-')[0] === lang) return v;
    }
    return '';
  }
  /** 语速档五档。 */
  static RATES = [0.75, 1, 1.25, 1.5, 2] as const;
  static rateOk(r: number): boolean {
    return (VoiceL10n.RATES as readonly number[]).includes(r);
  }
  unbind(locale: string): boolean {
    return this.map.delete(locale);
  }
}

/* -------- 族0500 翻译质量 2.0（X12476~X12500 · C 线口径）-------- */

export const QUALITY_GRADES = ['reject', 'passable', 'good', 'excellent', 'publish'] as const;
export type QualityGrade = (typeof QUALITY_GRADES)[number];

/** 翻译质量：术语表去重 + 占位符完整性 + 评分五档。 */
export class TranslateQuality {
  glossary = new Map<string, string>();
  clamped = 0;

  /** 术语登记去重。 */
  addTerm(src: string, dst: string): boolean {
    if (!src || !dst || this.glossary.has(src)) return false;
    this.glossary.set(src, dst);
    return true;
  }
  /** 术语命中。 */
  term(src: string): string {
    return this.glossary.get(src) ?? '';
  }
  /** 占位符完整性：源与译的 {n} 集合一致。 */
  static placeholdersOk(src: string, dst: string): boolean {
    const pick = (s: string) => (s.match(/\{\w+\}/g) ?? []).sort().join(',');
    return pick(src) === pick(dst);
  }
  /** 标签配对（<b>…</b> 轻校验）。 */
  static tagsOk(s: string): boolean {
    const open = (s.match(/<b>/g) ?? []).length;
    const close = (s.match(/<\/b>/g) ?? []).length;
    return open === close;
  }
  /** 评分 0~100 → 五档。 */
  static grade(score: number): QualityGrade {
    const v = Number.isFinite(score) ? Math.min(100, Math.max(0, score)) : 0;
    if (v < 40) return 'reject';
    if (v < 60) return 'passable';
    if (v < 75) return 'good';
    if (v < 90) return 'excellent';
    return 'publish';
  }
  /** 应用术语替换。 */
  apply(src: string): string {
    let out = src;
    for (const [k, v] of this.glossary) out = out.split(k).join(v);
    return out;
  }
  clear(): void {
    this.glossary.clear();
  }
}
