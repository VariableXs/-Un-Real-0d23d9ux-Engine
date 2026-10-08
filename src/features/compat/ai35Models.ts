/**
 * UNREAL-X-15000 · AI-35 兼容深化与收官（领域09 · 族0341~0350 · X08501~X08750）模型层，勿删。
 * V 线五族：文档库 / 社区反馈 / 认证 / 无障碍 / 收官。
 * （K 线三族见 kernel/varix/src/compatshim.rs，C 线两族见 code-analysis/core/src/ai35.rs。）
 * 纯 TypeScript 零依赖；五档矩阵 + 越界钳制 + 快照迁移 + 降级链，供 ai35Checks.ts 断言。
 */

/* ================= 公共基建 ================= */

/** 五档通用矩阵：默认档=balanced，off=低配降级终档。 */
export const TIER_MATRIX = ['off', 'light', 'balanced', 'strict', 'print'] as const;
export type Tier = (typeof TIER_MATRIX)[number];
export const DEFAULT_TIER: Tier = 'balanced';

export function clampTier(v: unknown): Tier {
  return TIER_MATRIX.includes(v as Tier) ? (v as Tier) : DEFAULT_TIER;
}

/** 错误码体系：禁裸报错，每个码带可读文案与下一步建议。 */
export const ERROR_CODES = {
  E3501: { text: '文档版本漂移', next: '以锚点版本重建索引' },
  E3502: { text: '反馈缺复现步', next: '以引导清单补采' },
  E3503: { text: '认证证据过期', next: '重跑认证套件换签' },
  E3504: { text: '无障碍对比不足', next: '切换 HC 令牌复检' },
  E3505: { text: '收官门禁未齐', next: '按核对单补齐后重审' },
} as const;
export type ErrorCode = keyof typeof ERROR_CODES;

export function explainError(code: string): { text: string; next: string } {
  return (ERROR_CODES as Record<string, { text: string; next: string }>)[code] ?? ERROR_CODES.E3501;
}

/** 动效令牌：曲线/时长/缩放三对齐；off 档退化为纯淡入淡出。 */
export const MOTION_TOKENS = { curve: 'ease-standard', durationMs: 180, scale: 1 } as const;
export function motionFor(tier: Tier): { curve: string; durationMs: number; scale: number } {
  if (tier === 'off') return { curve: 'linear-fade', durationMs: 120, scale: 0 };
  return { ...MOTION_TOKENS };
}

/* ================= 族0344 兼容文档库 ================= */

export class CompatDocLibrary {
  tier: Tier;
  clamped = 0;
  private docs = new Map<string, { ver: number; body: string }>();

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped += 1;
  }

  /** 收录文档：同键版本递增，条数钳 64。 */
  put(key: string, ver: number, body: string): number {
    if (this.docs.size >= 64 && !this.docs.has(key)) return this.docs.size;
    const old = this.docs.get(key);
    this.docs.set(key, { ver: Math.max(ver, old?.ver ?? 0), body });
    return this.docs.size;
  }

  /** 按键检索：版本回退保护（只升不降）。 */
  get(key: string): { ver: number; body: string } | undefined {
    return this.docs.get(key);
  }

  /** 前缀检索：键名前缀命中清单（字典序）。 */
  search(prefix: string): string[] {
    return [...this.docs.keys()].filter((k) => k.startsWith(prefix)).sort();
  }

  /** 锚点版本：全部文档最大版本号。 */
  anchorVer(): number {
    return [...this.docs.values()].reduce((m, d) => Math.max(m, d.ver), 0);
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, docs: [...this.docs] });
  }

  static deserialize(raw: string): CompatDocLibrary {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; docs?: [string, { ver: number; body: string }][] };
      const l = new CompatDocLibrary(clampTier(o.tier));
      for (const [k, d] of o.docs ?? []) l.docs.set(k, d);
      return l;
    } catch {
      return new CompatDocLibrary();
    }
  }
}

/* ================= 族0345 社区反馈 ================= */

export interface Feedback {
  title: string;
  reproSteps: number; // 复现步数量
  votes: number;
}

export class CommunityFeedback {
  tier: Tier;
  clamped = 0;
  private items: Feedback[] = [];

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped += 1;
  }

  /** 提交反馈：复现步 <2 判为待补采。 */
  submit(f: Feedback): 'accepted' | 'needs-repro' {
    if (f.reproSteps < 2) return 'needs-repro';
    this.items.push(f);
    return 'accepted';
  }

  /** 热度榜：按票数降序，同票按提交序，取前 k（≤10）。 */
  top(k: number): Feedback[] {
    return [...this.items]
      .map((f, i) => ({ f, i }))
      .sort((a, b) => b.f.votes - a.f.votes || a.i - b.i)
      .slice(0, Math.min(Math.max(k, 0), 10))
      .map((x) => x.f);
  }

  /** 去重归并：同名标题合并累票。 */
  dedup(): number {
    const seen = new Map<string, Feedback>();
    for (const f of this.items) {
      const old = seen.get(f.title);
      seen.set(f.title, old ? { ...old, votes: old.votes + f.votes } : f);
    }
    this.items = [...seen.values()];
    return this.items.length;
  }

  get count(): number {
    return this.items.length;
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, items: this.items });
  }

  static deserialize(raw: string): CommunityFeedback {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; items?: Feedback[] };
      const c = new CommunityFeedback(clampTier(o.tier));
      c.items = o.items ?? [];
      return c;
    } catch {
      return new CommunityFeedback();
    }
  }
}

/* ================= 族0346 兼容认证 ================= */

export type CertVerdict = 'gold' | 'silver' | 'pass' | 'fail';

export class CertSuite {
  tier: Tier;
  clamped = 0;
  private results: boolean[] = [];

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped += 1;
  }

  /** 记录用例结果：计入套件。 */
  record(ok: boolean): number {
    this.results.push(ok);
    return this.results.length;
  }

  /** 认证评级：全过且 ≥20 例 gold / 全过且 ≥10 例 silver / 通过率 ≥80% pass / 其余 fail。 */
  verdict(): CertVerdict {
    const n = this.results.length;
    const pass = this.results.filter(Boolean).length;
    if (n > 0 && pass === n && n >= 20) return 'gold';
    if (n > 0 && pass === n && n >= 10) return 'silver';
    if (n > 0 && pass / n >= 0.8) return 'pass';
    return 'fail';
  }

  /** 证书有效期：按评级给天数。 */
  validDays(): number {
    return { gold: 730, silver: 365, pass: 180, fail: 0 }[this.verdict()];
  }

  /** 重跑清零：证据过期后换签。 */
  reset(): void {
    this.results = [];
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, results: this.results });
  }

  static deserialize(raw: string): CertSuite {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; results?: boolean[] };
      const c = new CertSuite(clampTier(o.tier));
      c.results = o.results ?? [];
      return c;
    } catch {
      return new CertSuite();
    }
  }
}

/* ================= 族0348 兼容无障碍 ================= */

export class CompatA11y {
  tier: Tier;
  clamped = 0;

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped += 1;
  }

  /** 对比度比率：RGB 灰度近似 → WCAG 比率（1~21）。 */
  contrast(fg: number, bg: number): number {
    const lum = (c: number) => {
      const s = c / 255;
      return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
    };
    const l1 = lum(fg);
    const l2 = lum(bg);
    const ratio = (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);
    return Math.round(ratio * 100) / 100;
  }

  /** AA 判定：正文 ≥4.5，大字 ≥3.0。 */
  aaOk(ratio: number, large: boolean): boolean {
    return ratio >= (large ? 3 : 4.5);
  }

  /** 焦点环：兼容模式下焦点必须 2px 可见。 */
  focusRing(): { width: number; visible: boolean } {
    if (this.tier === 'off') return { width: 0, visible: false };
    return { width: 2, visible: true };
  }

  /** 读屏标注：aria 标签缺省回退为可见文本。 */
  ariaLabel(label: string | undefined, fallback: string): string {
    return label && label.length > 0 ? label : fallback;
  }

  /** HC 映射：strict 档强制高对比令牌。 */
  isHc(): boolean {
    return this.tier === 'strict' || this.tier === 'print';
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier });
  }

  static deserialize(raw: string): CompatA11y {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new CompatA11y(clampTier(o.tier));
    } catch {
      return new CompatA11y();
    }
  }
}

/* ================= 族0350 兼容收官 ================= */

/** 收官门禁核对单：25 项占位（对应 X08726~X08750）。 */
export const FINALE_CHECKLIST: string[] = [
  '体检', '实验室', '共存', '企业', '中文', '遥测', 'Web', '格式', 'API', '回归',
  'Shim', '协商', '沙盒', '文档', '社区', '认证', '性能税', '无障碍', '档案',
  '门禁归零', '三线对齐', 'ID 连续', '测试全绿', '文档同步', '收官复核',
];

export class CompatFinale {
  tier: Tier;
  clamped = 0;
  private done = new Set<string>();

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped += 1;
  }

  /** 勾选门禁项：仅接受核对单内条目。 */
  check(item: string): boolean {
    if (!FINALE_CHECKLIST.includes(item)) return false;
    this.done.add(item);
    return true;
  }

  /** 门禁余量。 */
  remaining(): number {
    return FINALE_CHECKLIST.length - this.done.size;
  }

  /** 收官判定：全部勾齐才许收官。 */
  ready(): boolean {
    return this.remaining() === 0;
  }

  /** 收官语。 */
  banner(): string {
    return this.ready() ? `兼容防线 750/750 项全量交付（${FINALE_CHECKLIST.length} 门禁齐）` : `门禁余 ${this.remaining()} 项`;
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, done: [...this.done] });
  }

  static deserialize(raw: string): CompatFinale {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; done?: string[] };
      const f = new CompatFinale(clampTier(o.tier));
      for (const d of o.done ?? []) f.check(d);
      return f;
    } catch {
      return new CompatFinale();
    }
  }
}
