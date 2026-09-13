/**
 * UNREAL-X-15000 · AI-34 兼容工程（领域09 · 族0331~0340 · X08251~X08500）模型层，勿删。
 * V 线五族：多系统共存 / 企业环境 / 中文深度兼容 / Web 兼容 / 格式兼容。
 * （C 线四族见 code-analysis/core/src/ai34.rs，K 线族0339 见 kernel/varix/src/compatapi.rs。）
 * 纯 TypeScript 零依赖；五档矩阵 + 越界钳制 + 快照迁移 + 降级链，供 ai34Checks.ts 断言。
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
  E3401: { text: '多系统引导项冲突', next: '按最近成功启动序重排' },
  E3402: { text: '域策略拒绝挂载', next: '以只读探针重试' },
  E3403: { text: '中文路径编码歧义', next: '切换 NFC/NFKC 归一再打开' },
  E3404: { text: 'Web 视图内核过旧', next: '回退静态渲染模式' },
  E3405: { text: '格式嗅探不确定', next: '以通用容器中转导出' },
} as const;
export type ErrorCode = keyof typeof ERROR_CODES;

export function explainError(code: string): { text: string; next: string } {
  return (ERROR_CODES as Record<string, { text: string; next: string }>)[code] ?? ERROR_CODES.E3401;
}

/** 动效令牌：曲线/时长/缩放三对齐；off 档退化为纯淡入淡出。 */
export const MOTION_TOKENS = { curve: 'ease-standard', durationMs: 180, scale: 1 } as const;
export function motionFor(tier: Tier): { curve: string; durationMs: number; scale: number } {
  if (tier === 'off') return { curve: 'linear-fade', durationMs: 120, scale: 0 };
  return { ...MOTION_TOKENS };
}

/* ================= 族0333 多系统共存 ================= */

/** 引导项：系统名 + 默认序 + 是否受管。 */
export interface BootEntry {
  name: string;
  order: number;
  managed: boolean;
}

export class CoexistMatrix {
  tier: Tier;
  clamped = 0;
  private entries: BootEntry[] = [];

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped += 1;
  }

  /** 登记引导项：同名去重（保留首序），数量钳 12。 */
  add(name: string, order: number, managed = true): number {
    if (this.entries.some((e) => e.name === name)) return this.entries.length;
    if (this.entries.length >= 12) return this.entries.length;
    this.entries.push({ name, order, managed });
    return this.entries.length;
  }

  /** 启动菜单序：受管项按 order 升序，未受管沉底。 */
  menu(): string[] {
    const managed = this.entries.filter((e) => e.managed).sort((a, b) => a.order - b.order).map((e) => e.name);
    const other = this.entries.filter((e) => !e.managed).map((e) => e.name);
    return [...managed, ...other];
  }

  /** 共存风险：未受管项越多风险越高（0~3 档）。 */
  risk(): number {
    const n = this.entries.filter((e) => !e.managed).length;
    return n === 0 ? 0 : n <= 2 ? 1 : n <= 5 ? 2 : 3;
  }

  /** 分区互斥：同一盘符不可被两项同时占用。 */
  static conflict(a: string[], b: string[]): boolean {
    return a.some((x) => b.includes(x));
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, entries: this.entries });
  }

  static deserialize(raw: string): CoexistMatrix {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; entries?: BootEntry[] };
      const m = new CoexistMatrix(clampTier(o.tier));
      for (const e of o.entries ?? []) m.add(e.name, e.order, e.managed);
      return m;
    } catch {
      return new CoexistMatrix();
    }
  }
}

/* ================= 族0334 企业环境 ================= */

/** 域策略：策略键 + 值 + 是否强制。 */
export type PolicyMap = Map<string, { value: string; enforced: boolean }>;

export class EnterpriseEnv {
  tier: Tier;
  clamped = 0;
  policies: PolicyMap = new Map();

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped += 1;
  }

  /** 下发策略：enforced 项不可被本地覆盖。 */
  push(key: string, value: string, enforced: boolean): void {
    const old = this.policies.get(key);
    if (old?.enforced && !enforced) return;
    this.policies.set(key, { value, enforced });
  }

  /** 生效值：强制项恒为域值，其余可本地改。 */
  effective(key: string, local?: string): string {
    const p = this.policies.get(key);
    if (!p) return local ?? '';
    return p.enforced ? p.value : (local ?? p.value);
  }

  /** 合规快照：强制项清单（按字典序）。 */
  enforcedList(): string[] {
    return [...this.policies.entries()].filter(([, p]) => p.enforced).map(([k]) => k).sort();
  }

  /** 部署配额：按档位给装机并发数，off 档为 0。 */
  deployQuota(): number {
    return { off: 0, light: 5, balanced: 20, strict: 50, print: 50 }[this.tier];
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, policies: [...this.policies] });
  }

  static deserialize(raw: string): EnterpriseEnv {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; policies?: [string, { value: string; enforced: boolean }][] };
      const e = new EnterpriseEnv(clampTier(o.tier));
      for (const [k, p] of o.policies ?? []) e.policies.set(k, p);
      return e;
    } catch {
      return new EnterpriseEnv();
    }
  }
}

/* ================= 族0335 中文深度兼容 ================= */

const CJK_RANGE = /[\u4e00-\u9fff\u3400-\u4dbf]/;

export class CjkCompat {
  tier: Tier;
  clamped = 0;

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped += 1;
  }

  /** 路径长度：中文按 2 计，超 260 钳制截断（MAX_PATH 兼容）。 */
  pathLen(path: string): number {
    let n = 0;
    let out = '';
    for (const ch of path) {
      const w = CJK_RANGE.test(ch) ? 2 : 1;
      if (n + w > 260) break;
      n += w;
      out += ch;
    }
    return n;
  }

  /** Unicode 归一：NFC 合成（é → é 单码位）。 */
  normalize(text: string): string {
    return this.tier === 'off' ? text : text.normalize('NFC');
  }

  /** 全半角互换：ASCII 可见符号转全角。 */
  toFullWidth(text: string): string {
    if (this.tier === 'off') return text;
    return text.replace(/[!-~]/g, (c) => String.fromCharCode(c.charCodeAt(0) + 0xfee0));
  }

  /** 字体回退链：中文缺字时按 楷→黑→系统 顺序回退。 */
  fontFallback(requested: string): string[] {
    if (requested === '楷体') return ['楷体', '黑体', 'system'];
    if (requested === '黑体') return ['黑体', 'system'];
    return ['system'];
  }

  /** 简繁转换探针：仅判定是否含待转换汉字，不做真实转换。 */
  needsVariant(text: string): boolean {
    return CJK_RANGE.test(text);
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier });
  }

  static deserialize(raw: string): CjkCompat {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new CjkCompat(clampTier(o.tier));
    } catch {
      return new CjkCompat();
    }
  }
}

/* ================= 族0337 Web 兼容 ================= */

export type EngineId = 'modern' | 'legacy' | 'static';

export class WebCompat {
  tier: Tier;
  clamped = 0;
  engine: EngineId = 'modern';

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped += 1;
    if (this.tier === 'off') this.engine = 'static';
  }

  /** 引擎裁决：UA 特征 → 内核档。 */
  pickEngine(ua: string): EngineId {
    if (this.tier === 'off') return 'static';
    if (ua.includes('Legacy')) return 'legacy';
    return 'modern';
  }

  /** 能力降级：无 WebGL → canvas2d，无 canvas → 文本。 */
  renderPath(hasWebGl: boolean, hasCanvas: boolean): string {
    if (hasWebGl && this.tier !== 'off') return 'webgl';
    if (hasCanvas) return 'canvas2d';
    return 'text';
  }

  /** CSP 协商：允许内联脚本仅限 strict 以下档。 */
  inlineScriptAllowed(): boolean {
    return this.tier !== 'strict' && this.tier !== 'print';
  }

  /** Cookie 策略：SameSite 判定。 */
  cookiePolicy(sameSiteNone: boolean): 'accept' | 'lax' | 'reject' {
    if (sameSiteNone && this.tier === 'strict') return 'reject';
    return sameSiteNone ? 'lax' : 'accept';
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier, engine: this.engine });
  }

  static deserialize(raw: string): WebCompat {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new WebCompat(clampTier(o.tier));
    } catch {
      return new WebCompat();
    }
  }
}

/* ================= 族0338 格式兼容 ================= */

export type FmtId = 'json' | 'csv' | 'xml' | 'ini' | 'bin';

export class FormatBridge {
  tier: Tier;
  clamped = 0;

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped += 1;
  }

  /** 嗅探：魔数/首字符 → 格式 id，不确定返回 'bin'。 */
  sniff(head: string): FmtId {
    const t = head.trimStart();
    if (t.startsWith('{') || t.startsWith('[')) return 'json';
    if (t.startsWith('<?xml') || t.startsWith('<')) return 'xml';
    if (t.includes(',') && t.includes('\n')) return 'csv';
    if (t.includes('=')) return 'ini';
    return 'bin';
  }

  /** CSV 行解析：支持引号转义。 */
  fromCsv(line: string): string[] {
    const out: string[] = [];
    let cur = '';
    let inQ = false;
    for (let i = 0; i < line.length; i++) {
      const c = line[i]!;
      if (inQ) {
        if (c === '"' && line[i + 1] === '"') { cur += '"'; i++; }
        else if (c === '"') inQ = false;
        else cur += c;
      } else if (c === '"') inQ = true;
      else if (c === ',') { out.push(cur); cur = ''; }
      else cur += c;
    }
    out.push(cur);
    return out;
  }

  /** CSV 序列化：含逗号/引号字段加引号。 */
  toCsv(fields: string[]): string {
    return fields.map((f) => (/[",]/.test(f) ? `"${f.replace(/"/g, '""')}"` : f)).join(',');
  }

  /** 中转容器：任意格式 → 通用 JSON 行。 */
  toUniversal(rows: string[][]): string {
    return rows.map((r) => JSON.stringify(r)).join('\n');
  }

  fromUniversal(text: string): string[][] | null {
    try {
      return text.split('\n').filter((l) => l.length > 0).map((l) => JSON.parse(l) as string[]);
    } catch {
      return null;
    }
  }

  serialize(): string {
    return JSON.stringify({ tier: this.tier });
  }

  static deserialize(raw: string): FormatBridge {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new FormatBridge(clampTier(o.tier));
    } catch {
      return new FormatBridge();
    }
  }
}
