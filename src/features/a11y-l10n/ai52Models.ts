/**
 * UNREAL-X-15000 · AI-52 无障碍内核与收官 V 线逻辑核（族0511~0520 · X12751~X13000），勿删。
 * 五层 × 五档模板。族0511/0512/0518 为内核 K 线（kernel/varix/src/a11y/a52k.rs），
 * 族0516/0517 为代码分析 C 线（code-analysis/core/src/ai52.rs），此处为同口径 TS 镜像；
 * V 线本责为族0513/0514/0515/0519/0520。
 */

/* -------- 族0511 内核无障碍服务（X12751~X12775 · K 镜像）-------- */

/** 服务登记：接口冻结 + 开关存在（§15 守卫口径）。 */
export class KServiceRegistry {
  private services: { id: string; enabled: boolean }[] = [];
  clamped = 0;
  register(id: string, enabled = false): boolean {
    if (this.services.some((s) => s.id === id)) return false;
    if (id.length === 0) { this.clamped += 1; return false; }
    this.services.push({ id, enabled });
    return true;
  }
  get count(): number {
    return this.services.length;
  }
  isEnabled(id: string): boolean {
    return this.services.find((s) => s.id === id)?.enabled ?? false;
  }
  toggle(id: string): boolean {
    const s = this.services.find((x) => x.id === id);
    if (!s) { this.clamped += 1; return false; }
    s.enabled = !s.enabled;
    return s.enabled;
  }
}

/* -------- 族0512 读屏协议引擎（X12776~X12800 · K 镜像）-------- */

/** 读屏语音行：role, label 序列；焦点序 = 树先序。 */
export class ScreenReaderProtocol {
  lines: { role: string; label: string }[] = [];
  clamped = 0;
  push(role: string, label: string): boolean {
    if (role.length === 0 || label.length === 0) { this.clamped += 1; return false; }
    this.lines.push({ role, label });
    return true;
  }
  speak(): string[] {
    return this.lines.map((l) => l.role + ', ' + l.label);
  }
  get complete(): boolean {
    return this.lines.every((l) => l.label.length > 0);
  }
}

/* -------- 族0513 本地化工程 2.0（X12801~X12825 · V 本责）-------- */

export const PLURAL_RULES = {
  zh: (_n: number) => 'other',
  en: (n: number) => (n === 1 ? 'one' : 'other'),
  ja: (_n: number) => 'other',
  fr: (n: number) => (n <= 1 ? 'one' : 'other'),
} as const;
export type PluralLang = keyof typeof PLURAL_RULES;

/** 复数规则查询。 */
export function pluralFor(lang: string, n: number): string {
  const fn = (PLURAL_RULES as Record<string, (n: number) => string>)[lang];
  return fn ? fn(n) : 'other';
}

/** ICU 消息格式化：{name} 与复数分支。 */
export function icuFormat(template: string, vars: Record<string, string>): string {
  return template.replace(/\{(\w+)\}/g, (_, k: string) => vars[k] ?? '{' + k + '}');
}

/** 翻译键命名纪律：小写点分层。 */
export function keyOk(k: string): boolean {
  return /^[a-z0-9]+(\.[a-z0-9_]+)+$/.test(k) && !k.endsWith('.');
}

/** 本地化工程器：语言包档位 + 缺失回退链。 */
export class L10nEngineering {
  clamped = 0;
  constructor(public packs: Record<string, Record<string, string>> = {}) {}
  get(lang: string, key: string): string {
    const chain = [lang, lang.split('-')[0] as string, 'en'];
    for (const l of chain) {
      const v = this.packs[l]?.[key];
      if (v !== undefined) return v;
    }
    this.clamped += 1;
    return key;
  }
}

/* -------- 族0514 全球发布 2.0（X12826~X12850 · V 本责）-------- */

export const RELEASE_LANGS = ['zh-CN', 'en', 'ja', 'de', 'fr', 'es', 'pt-BR', 'ko', 'ar', 'ru'] as const;

/** 发布门禁：语言覆盖率 + 完成率阈值。 */
export class GlobalRelease {
  clamped = 0;
  constructor(public coverage: Record<string, number> = {}) {}
  /** 档位：full ≥95 / beta ≥80 / preview ≥60 / 否则 blocked。 */
  gate(lang: string): 'full' | 'beta' | 'preview' | 'blocked' {
    const c = this.coverage[lang];
    if (c === undefined) { this.clamped += 1; return 'blocked'; }
    if (c >= 95) return 'full';
    if (c >= 80) return 'beta';
    if (c >= 60) return 'preview';
    return 'blocked';
  }
  get fullCount(): number {
    return RELEASE_LANGS.filter((l) => this.gate(l) === 'full').length;
  }
}

/* -------- 族0515 社区本地化 2.0（X12851~X12875 · 三方）-------- */

/** 贡献审校流：draft → review → approved / rejected（状态机）。 */
export class CommunityL10n {
  state = 'draft' as 'draft' | 'review' | 'approved' | 'rejected';
  clamped = 0;
  history: string[] = ['draft'];
  submit(): boolean {
    if (this.state !== 'draft') { this.clamped += 1; return false; }
    this.state = 'review';
    this.history.push('review');
    return true;
  }
  approve(): boolean {
    if (this.state !== 'review') { this.clamped += 1; return false; }
    this.state = 'approved';
    this.history.push('approved');
    return true;
  }
  reject(): boolean {
    if (this.state !== 'review') { this.clamped += 1; return false; }
    this.state = 'rejected';
    this.history.push('rejected');
    return true;
  }
  /** 术语表一致性：译文必须与已核准术语一致。 */
  static glossaryOk(text: string, glossary: Record<string, string>): boolean {
    return Object.entries(glossary).every(([en, zh]) => text.includes(zh) || !text.includes(en));
  }
}

/* -------- 族0516 无障碍研究 2.0（X12876~X12900 · C 镜像）-------- */

/** 使用研究：任务完成时间样本 → 中位数（去除未完成样本）。 */
export function medianTaskTime(samples: (number | null)[]): number | null {
  const xs = samples.filter((s): s is number => s !== null).sort((a, b) => a - b);
  if (xs.length === 0) return null;
  const mid = xs.length >> 1;
  return xs.length % 2 ? xs[mid] as number : ((xs[mid - 1] as number) + (xs[mid] as number)) / 2;
}

/** 研究结论分级：效应量 |d| 阈值（Cohen's d 口径简化）。 */
export function effectGrade(d: number): 'large' | 'medium' | 'small' | 'negligible' {
  const a = Math.abs(d);
  if (a >= 0.8) return 'large';
  if (a >= 0.5) return 'medium';
  if (a >= 0.2) return 'small';
  return 'negligible';
}

/* -------- 族0517 无障碍自动化审计（X12901~X12925 · C 镜像）-------- */

/** WCAG 对比度计算（sRGB 相对亮度比）。 */
export function contrastRatio(rgbA: [number, number, number], rgbB: [number, number, number]): number {
  const lum = ([r, g, b]: [number, number, number]) => {
    const f = (c: number) => {
      const s = c / 255;
      return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
    };
    return 0.2126 * f(r) + 0.7152 * f(g) + 0.0722 * f(b);
  };
  const [l1, l2] = [lum(rgbA), lum(rgbB)].sort((a, b) => b - a);
  return ((l1 as number) + 0.05) / ((l2 as number) + 0.05);
}

/** 焦点序扫描：DOM 顺序与 tabindex 序一致判过。 */
export function focusOrderOk(domOrder: string[], tabStops: string[]): boolean {
  if (tabStops.length !== domOrder.length) return false;
  return tabStops.every((t, i) => t === domOrder[i]);
}

/** 审计报告项。 */
export interface AuditIssue {
  rule: 'contrast' | 'focus' | 'label' | 'alt';
  severity: 'error' | 'warn';
  target: string;
}

/** 审计器：登记去重 + 分级统计。 */
export class AutoAudit {
  private issues: AuditIssue[] = [];
  register(i: AuditIssue): boolean {
    const key = i.rule + ':' + i.target;
    if (this.issues.some((x) => x.rule + ':' + x.target === key)) return false;
    this.issues.push(i);
    return true;
  }
  get errors(): number {
    return this.issues.filter((i) => i.severity === 'error').length;
  }
  get count(): number {
    return this.issues.length;
  }
}

/* -------- 族0518 输入无障碍引擎（X12926~X12950 · K 镜像）-------- */

/** 粘滞键：修饰键按下后保持到下一非修饰键。 */
export class StickyKeys {
  private held = new Set<string>();
  press(key: string): void {
    if (['shift', 'ctrl', 'alt'].includes(key)) {
      if (this.held.has(key)) this.held.delete(key);
      else this.held.add(key);
    } else {
      this.held.clear();
    }
  }
  get isSticky(): boolean {
    return this.held.size > 0;
  }
}

/** 慢速键：按下到确认的最短保持时间（钳制 0~2000ms）。 */
export function slowKeyDelay(ms: number): { ok: boolean; delay: number } {
  const clamped = ms < 0 || ms > 2000;
  return { ok: !clamped, delay: Math.min(2000, Math.max(0, ms)) };
}

/** 悬停点击：指针停留时长达标即触发。 */
export function dwellOk(hoverMs: number, threshold = 1000): boolean {
  return hoverMs >= threshold;
}

/* -------- 族0519 无障碍档案（X12951~X12975 · V 本责）-------- */

export const ARCHIVE_KEYS = ['theme', 'contrast', 'fontScale', 'reduceMotion', 'captions', 'screenReader', 'density'] as const;

/** 无障碍档案：六元组快照 + 版本链冻结。 */
export class A11yArchive {
  version = 1;
  clamped = 0;
  constructor(public data: Partial<Record<(typeof ARCHIVE_KEYS)[number], string | number | boolean>> = {}) {}
  set(k: (typeof ARCHIVE_KEYS)[number], v: string | number | boolean): boolean {
    if (!(ARCHIVE_KEYS as readonly string[]).includes(k)) { this.clamped += 1; return false; }
    this.data[k] = v;
    return true;
  }
  serialize(): string {
    return JSON.stringify({ version: this.version, data: this.data });
  }
  static deserialize(s: string): A11yArchive {
    try {
      const o = JSON.parse(s) as { version?: number; data?: A11yArchive['data'] };
      const q = new A11yArchive(o.data ?? {});
      q.version = o.version ?? 1;
      return q;
    } catch {
      return new A11yArchive();
    }
  }
}

/* -------- 族0520 无障碍本地化收官（X12976~X13000 · 三方）-------- */

/** 收官门禁核对单：六项一票否决。 */
export function finaleGate(results: {
  idsUnique: boolean;
  typecheckClean: boolean;
  vitestGreen: boolean;
  kernelGreen: boolean;
  cLineGreen: boolean;
  docsSynced: boolean;
}): { pass: boolean; failed: string[] } {
  const failed = Object.entries(results).filter(([, v]) => !v).map(([k]) => k);
  return { pass: failed.length === 0, failed };
}

/** 领域14 总量核对：AI-50~52 三人 750 项区间边界。 */
export function domain14Bounds(): { first: number; last: number; perAi: number; ais: number } {
  return { first: 12251, last: 13000, perAi: 250, ais: 3 };
}
