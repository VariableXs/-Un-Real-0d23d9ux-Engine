// UNREAL-X：AI-12 族0118「桌面本地化 2.0」（X02926~X02950）。
// 桌面文案的多语言装载、回退链、复数与排序、伪本地化与溢出检测。

export type Locale = 'zh-CN' | 'zh-TW' | 'en-US' | 'ja-JP' | 'ko-KR' | 'de-DE' | 'fr-FR' | 'ar-EG';
export const LOCALES: Locale[] = ['zh-CN', 'zh-TW', 'en-US', 'ja-JP', 'ko-KR', 'de-DE', 'fr-FR', 'ar-EG'];

/** RTL 语言集合。 */
export const RTL_LOCALES: Locale[] = ['ar-EG'];

export function isRtl(locale: Locale): boolean {
  return RTL_LOCALES.includes(locale);
}

/** 回退链：zh-TW → zh-CN → en-US。 */
export function fallbackChain(locale: Locale): Locale[] {
  const chain: Locale[] = [locale];
  if (locale.startsWith('zh')) {
    if (!chain.includes('zh-CN')) chain.push('zh-CN');
  }
  if (!chain.includes('en-US')) chain.push('en-US');
  return chain;
}

/** 文案膨胀系数：中译英按 1.6 估，德文按 1.9 估。 */
export const EXPANSION: Record<Locale, number> = {
  'zh-CN': 1,
  'zh-TW': 1,
  'en-US': 1.6,
  'ja-JP': 1.1,
  'ko-KR': 1.15,
  'de-DE': 1.9,
  'fr-FR': 1.75,
  'ar-EG': 1.5,
};

/** 溢出检测：按膨胀系数估算宽度是否超容器。 */
export function overflows(text: string, locale: Locale, maxPx: number, pxPerChar = 8): boolean {
  const width = [...text].length * pxPerChar * EXPANSION[locale];
  return width > maxPx;
}

/** 伪本地化：ASCII 转带重音的等价字符，用于发现硬编码与溢出。 */
const PSEUDO_MAP: Record<string, string> = {
  a: 'à', b: 'ḃ', c: 'ĉ', d: 'ď', e: 'é', f: 'ḟ', g: 'ĝ', h: 'ĥ', i: 'î', j: 'ĵ',
  k: 'ķ', l: 'ļ', m: 'ḿ', n: 'ń', o: 'ô', p: 'ṗ', q: 'ǫ', r: 'ř', s: 'ś', t: 'ť',
  u: 'û', v: 'ṽ', w: 'ŵ', x: 'ẋ', y: 'ŷ', z: 'ź',
  A: 'À', B: 'Ḃ', C: 'Ĉ', D: 'Ď', E: 'É', F: 'Ḟ', G: 'Ĝ', H: 'Ĥ', I: 'Î', J: 'Ĵ',
  K: 'Ķ', L: 'Ļ', M: 'Ḿ', N: 'Ń', O: 'Ô', P: 'Ṗ', Q: 'Ǫ', R: 'Ř', S: 'Ś', T: 'Ť',
  U: 'Û', V: 'Ṽ', W: 'Ŵ', X: 'Ẋ', Y: 'Ŷ', Z: 'Ź',
};

export function pseudoLocalize(text: string): string {
  return [...text].map((c) => PSEUDO_MAP[c] ?? c).join('');
}

export interface L10nOptions {
  locale: Locale;
  fallback: boolean;
  pseudo: boolean;
  strict: boolean;
}

export const DEFAULT_L10N: L10nOptions = {
  locale: 'zh-CN',
  fallback: true,
  pseudo: false,
  strict: false,
};

export class DesktopL10n {
  private opts: L10nOptions = { ...DEFAULT_L10N };
  private dict = new Map<string, Partial<Record<Locale, string>>>();

  constructor(patch?: Partial<L10nOptions>) {
    if (patch) this.configure(patch);
  }

  get options(): L10nOptions {
    return { ...this.opts };
  }

  configure(patch: Partial<L10nOptions>): boolean {
    let ok = true;
    if (patch.locale !== undefined) {
      if (LOCALES.includes(patch.locale)) this.opts.locale = patch.locale;
      else ok = false;
    }
    if (typeof patch.fallback === 'boolean') this.opts.fallback = patch.fallback;
    if (typeof patch.pseudo === 'boolean') this.opts.pseudo = patch.pseudo;
    if (typeof patch.strict === 'boolean') this.opts.strict = patch.strict;
    return ok;
  }

  /** 登记词条（去重：同 key 覆盖而非重复计数）。 */
  put(key: string, locale: Locale, value: string): boolean {
    if (!key) return false;
    const slot = this.dict.get(key) ?? {};
    const isNew = slot[locale] === undefined;
    slot[locale] = value;
    this.dict.set(key, slot);
    return isNew;
  }

  get size(): number {
    return this.dict.size;
  }

  /** 取词：当前语 → 回退链 → 键名本身。 */
  t(key: string): string {
    const slot = this.dict.get(key);
    const chain = this.opts.fallback ? fallbackChain(this.opts.locale) : [this.opts.locale];
    for (const loc of chain) {
      const v = slot?.[loc];
      if (v !== undefined) return this.opts.pseudo ? pseudoLocalize(v) : v;
    }
    return this.opts.strict ? `⟦${key}⟧` : key;
  }

  /** 缺失翻译的键清单（按当前语）。 */
  missingKeys(): string[] {
    const out: string[] = [];
    for (const [k, slot] of this.dict) {
      if (slot[this.opts.locale] === undefined) out.push(k);
    }
    return out.sort();
  }

  /** 覆盖率 0~100。 */
  coverage(): number {
    if (this.dict.size === 0) return 100;
    return Math.round(((this.dict.size - this.missingKeys().length) * 100) / this.dict.size);
  }

  /** 复数：中文只有 other，英语有 one/other。 */
  plural(count: number): Intl.LDMLPluralRule {
    try {
      return new Intl.PluralRules(this.opts.locale).select(count);
    } catch {
      return 'other';
    }
  }

  /** 排序：按语区 collation。 */
  collate(items: string[]): string[] {
    try {
      return [...items].sort(new Intl.Collator(this.opts.locale).compare);
    } catch {
      return [...items].sort();
    }
  }

  /** 数字与日期格式化（失败时退回 ISO）。 */
  formatNumber(n: number): string {
    try {
      return new Intl.NumberFormat(this.opts.locale).format(n);
    } catch {
      return String(n);
    }
  }

  formatDate(d: Date): string {
    try {
      return new Intl.DateTimeFormat(this.opts.locale).format(d);
    } catch {
      return d.toISOString().slice(0, 10);
    }
  }

  /** 溢出风险清单。 */
  overflowRisks(maxPx: number): string[] {
    const out: string[] = [];
    for (const [k, slot] of this.dict) {
      const v = slot[this.opts.locale];
      if (v !== undefined && overflows(v, this.opts.locale, maxPx)) out.push(k);
    }
    return out.sort();
  }

  hint(): string {
    return `${this.opts.locale} 覆盖率 ${this.coverage()}%${isRtl(this.opts.locale) ? '（RTL）' : ''}`;
  }

  uninstall(): boolean {
    this.dict.clear();
    this.opts = { ...DEFAULT_L10N };
    return this.dict.size === 0;
  }
}
