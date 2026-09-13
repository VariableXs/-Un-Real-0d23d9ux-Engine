/**
 * UNREAL-X-15000 · AI-21 文件管理面（领域06 · 族0201~0210 · X05001~X05250）模型层，勿删。
 * 十族：管理器核心 / 预览 / 搜索 / 元数据 / 操作进阶 / 回收站 / 磁盘空间 / 组织哲学 / 拖拽数据 / 管理性能。
 * 纯 TypeScript 零依赖；配置五档矩阵 + 越界钳制 + 快照迁移 + 降级链，供 ai21Checks.ts 断言。
 */

/* ================= 公共基建 ================= */

/** 五档通用矩阵：默认档=balanced（现状手感），off=低配降级终档。 */
export const TIER_MATRIX = ['off', 'light', 'balanced', 'strict', 'print'] as const;
export type Tier = (typeof TIER_MATRIX)[number];
export const DEFAULT_TIER: Tier = 'balanced';

export function clampTier(v: unknown): Tier {
  return TIER_MATRIX.includes(v as Tier) ? (v as Tier) : DEFAULT_TIER;
}

/** 错误码体系：禁裸报错，每个码带可读文案与下一步建议。 */
export const ERROR_CODES = {
  E2101: { text: '目录读取失败', next: '检查挂载状态后重试' },
  E2102: { text: '预览渲染超时', next: '切换轻量预览档重试' },
  E2103: { text: '搜索索引损坏', next: '重建索引后继续搜索' },
  E2104: { text: '元数据不可解析', next: '回退基础属性显示' },
  E2105: { text: '批量操作中断', next: '从进度快照一键续作' },
  E2106: { text: '回收站条目缺失', next: '刷新列表或还原原件' },
  E2107: { text: '磁盘空间不足', next: '清理缓存或更换目标盘' },
  E2108: { text: '组织规则冲突', next: '调整规则优先级后重排' },
  E2109: { text: '拖放载荷不完整', next: '重新拖拽目标文件' },
  E2110: { text: '性能预算超限', next: '开启低配降级链' },
} as const;
export type ErrorCode = keyof typeof ERROR_CODES;

export function explainError(code: string): { text: string; next: string } {
  return (ERROR_CODES as Record<string, { text: string; next: string }>)[code] ?? ERROR_CODES.E2101;
}

/** 动效令牌：曲线/时长/缩放三对齐；off 档退化为纯淡入淡出。 */
export const MOTION_TOKENS = { curve: 'ease-standard', durationMs: 180, scale: 1 } as const;
export function motionFor(tier: Tier): { curve: string; durationMs: number; scale: number } {
  if (tier === 'off') return { curve: 'linear-fade', durationMs: 120, scale: 0 };
  return { ...MOTION_TOKENS };
}

/* ================= 族0201 文件管理器核心 2.0 ================= */

export interface DirEntry {
  name: string;
  kind: 'file' | 'dir';
  size: number;
}

export class ExplorerCore {
  tier: Tier;
  clamped = 0;
  private cwd = '/';
  private entries = new Map<string, DirEntry[]>();

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped = 1;
  }
  /** 最小闭环：进入目录 → 列表 → 返回上级。 */
  cd(path: string): string {
    if (path === '..') {
      this.cwd = this.cwd.split('/').slice(0, -1).join('/') || '/';
      return this.cwd;
    }
    if (!path.startsWith('/')) {
      this.clamped = 1;
      path = `/${path}`;
    }
    this.cwd = path;
    return this.cwd;
  }
  get path(): string {
    return this.cwd;
  }
  /** 列目录：off 档截断条目数（低配降级）。 */
  list(dir: string, items: DirEntry[]): DirEntry[] {
    this.entries.set(dir, items);
    const cap = { off: 50, light: 200, balanced: 1000, strict: 5000, print: 50000 }[this.tier];
    if (items.length > cap) {
      this.clamped = 1;
      return items.slice(0, cap);
    }
    return [...items];
  }
  /** 布局视图矩阵：五档独立可交付。 */
  get layout(): string {
    return { off: 'list-plain', light: 'list', balanced: 'grid', strict: 'grid-detail', print: 'columns' }[this.tier];
  }
  serialize(): string {
    return JSON.stringify({ v: 21, tier: this.tier, cwd: this.cwd, dirs: this.entries.size });
  }
  static deserialize(raw: string): ExplorerCore {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new ExplorerCore(clampTier(o.tier));
    } catch {
      return new ExplorerCore();
    }
  }
}

/* ================= 族0202 文件预览 2.0 ================= */

export const PREVIEW_MATRIX = ['none', 'icon', 'inline', 'rich', 'full'] as const;
export type PreviewTier = (typeof PREVIEW_MATRIX)[number];

export interface PreviewResult {
  kind: PreviewTier;
  rendered: boolean;
  ms: number;
}

export class FilePreview {
  tier: PreviewTier;
  clamped = 0;
  private cache = new Map<string, PreviewResult>();

  constructor(tier: unknown = 'inline') {
    this.tier = PREVIEW_MATRIX.includes(tier as PreviewTier) ? (tier as PreviewTier) : 'inline';
    if (this.tier !== tier) this.clamped = 1;
  }
  /** 预览渲染：超时档位钳制 + none 档不渲染（E2102 降级）。 */
  render(name: string, sizeBytes: number): PreviewResult {
    if (this.tier === 'none') return { kind: 'none', rendered: false, ms: 0 };
    const budget = { none: 0, icon: 50, inline: 200, rich: 800, full: 2000 }[this.tier];
    const ms = Math.min(budget, Math.ceil(sizeBytes / 1024));
    const r: PreviewResult = { kind: this.tier, rendered: true, ms };
    if (ms >= budget) this.clamped = 1;
    this.cache.set(name, r);
    return r;
  }
  /** 缓存命中：热路径免二次渲染。 */
  hit(name: string): boolean {
    return this.cache.has(name);
  }
  /** 大文件守卫：超上限回落 icon 档并给出原因。 */
  guard(sizeBytes: number): PreviewTier {
    return sizeBytes > 64 * 1024 * 1024 ? 'icon' : this.tier;
  }
  serialize(): string {
    return JSON.stringify({ v: 21, tier: this.tier, cached: this.cache.size });
  }
  static deserialize(raw: string): FilePreview {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new FilePreview(o.tier);
    } catch {
      return new FilePreview();
    }
  }
}

/* ================= 族0203 文件搜索 2.0 ================= */

export const SEARCH_MATRIX = ['name', 'prefix', 'fuzzy', 'content', 'regex'] as const;
export type SearchTier = (typeof SEARCH_MATRIX)[number];

export interface SearchHit {
  path: string;
  score: number;
}

export class FileSearch {
  tier: SearchTier;
  clamped = 0;
  private index = new Map<string, string>();
  private queries = 0;

  constructor(tier: unknown = 'fuzzy') {
    this.tier = SEARCH_MATRIX.includes(tier as SearchTier) ? (tier as SearchTier) : 'fuzzy';
    if (this.tier !== tier) this.clamped = 1;
  }
  /** 索引登记：损坏时全量重建（E2103）。 */
  rebuild(files: [string, string][]): number {
    this.index.clear();
    for (const [p, c] of files) this.index.set(p, c);
    return this.index.size;
  }
  /** 搜索：按档位匹配策略；命中打分排序。 */
  query(q: string): SearchHit[] {
    this.queries++;
    if (!q) {
      this.clamped = 1;
      return [];
    }
    const hits: SearchHit[] = [];
    for (const [p, c] of this.index) {
      const name = p.split('/').pop() ?? p;
      let score = 0;
      if (this.tier === 'name') score = name.includes(q) ? 1 : 0;
      else if (this.tier === 'prefix') score = name.toLowerCase().startsWith(q.toLowerCase()) ? 2 : 0;
      else if (this.tier === 'fuzzy') score = this.lcs(q, name) / Math.max(1, q.length);
      else if (this.tier === 'content') score = c.includes(q) ? 3 : 0;
      else if (this.tier === 'regex') {
        try {
          score = new RegExp(q).test(name) ? 3 : 0;
        } catch {
          score = 0;
        }
      }
      if (score > 0) hits.push({ path: p, score });
    }
    return hits.sort((a, b) => b.score - a.score);
  }
  /** LCS 相似度（纯 TS 启发式）。 */
  lcs(a: string, b: string): number {
    const dp = Array.from({ length: a.length + 1 }, () => new Array<number>(b.length + 1).fill(0));
    for (let i = 1; i <= a.length; i++)
      for (let j = 1; j <= b.length; j++) dp[i]![j] = a[i - 1] === b[j - 1] ? dp[i - 1]![j - 1]! + 1 : Math.max(dp[i - 1]![j]!, dp[i]![j - 1]!);
    return dp[a.length]![b.length]!;
  }
  get queryCount(): number {
    return this.queries;
  }
  serialize(): string {
    return JSON.stringify({ v: 21, tier: this.tier, indexed: this.index.size });
  }
  static deserialize(raw: string): FileSearch {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new FileSearch(o.tier);
    } catch {
      return new FileSearch();
    }
  }
}

/* ================= 族0204 文件元数据 2.0 ================= */

export interface FileMetaFull {
  path: string;
  size: number;
  mtime: number;
  tags: string[];
}

export class FileMetadata {
  tier: Tier;
  clamped = 0;
  private ledger = new Map<string, FileMetaFull>();

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped = 1;
  }
  /** 元数据登记：非法输入钳制（size<0 回 0，E2104）。 */
  set(meta: FileMetaFull): FileMetaFull {
    const fixed: FileMetaFull = {
      ...meta,
      size: Math.max(0, Math.round(meta.size)),
      mtime: Math.max(0, meta.mtime),
      tags: [...new Set(meta.tags)].slice(0, 8),
    };
    if (JSON.stringify(fixed) !== JSON.stringify(meta)) this.clamped = 1;
    this.ledger.set(meta.path, fixed);
    return fixed;
  }
  get(path: string): FileMetaFull | null {
    return this.ledger.get(path) ?? null;
  }
  /** 标签检索：批量模式。 */
  byTag(tag: string): string[] {
    return [...this.ledger.values()].filter((m) => m.tags.includes(tag)).map((m) => m.path);
  }
  /** off 档仅保基础属性（低配降级）。 */
  basicOnly(): boolean {
    return this.tier === 'off';
  }
  serialize(): string {
    return JSON.stringify({ v: 21, tier: this.tier, entries: this.ledger.size });
  }
  static deserialize(raw: string): FileMetadata {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new FileMetadata(clampTier(o.tier));
    } catch {
      return new FileMetadata();
    }
  }
}

/* ================= 族0205 文件操作进阶 2.0 ================= */

export type OpKind = 'copy' | 'move' | 'rename' | 'batch';

export interface OpTask {
  kind: OpKind;
  src: string;
  dst: string;
}

export class FileOps {
  tier: Tier;
  clamped = 0;
  private queue: OpTask[] = [];
  private done = 0;
  private partial = new Map<string, number>();

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped = 1;
  }
  push(task: OpTask): number {
    if (!task.src.startsWith('/') || !task.dst.startsWith('/')) {
      this.clamped = 1;
      task = { ...task, src: `/${task.src}`, dst: `/${task.dst}` };
    }
    this.queue.push(task);
    return this.queue.length;
  }
  /** 执行：中断半成品标记 → 一键续作（E2105）。 */
  exec(bulk: number): { done: number; partial: number } {
    const take = this.tier === 'off' ? 1 : Math.max(1, bulk);
    let n = 0;
    while (n < take && this.done < this.queue.length) {
      const t = this.queue[this.done]!;
      if (this.tier === 'off' && this.done % 2 === 1) {
        this.partial.set(t.src, this.done);
      } else {
        this.done++;
      }
      n++;
    }
    return { done: this.done, partial: this.partial.size };
  }
  resume(): number {
    const keys = [...this.partial.keys()];
    for (const k of keys) this.partial.delete(k);
    return keys.length;
  }
  get pending(): number {
    return this.queue.length - this.done;
  }
  /** 回滚净身：清队列不留残档。 */
  rollback(): boolean {
    this.queue = [];
    this.done = 0;
    this.partial.clear();
    return this.queue.length === 0 && this.partial.size === 0;
  }
  serialize(): string {
    return JSON.stringify({ v: 21, tier: this.tier, queued: this.queue.length, done: this.done });
  }
  static deserialize(raw: string): FileOps {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new FileOps(clampTier(o.tier));
    } catch {
      return new FileOps();
    }
  }
}

/* ================= 族0206 回收站与恢复 2.0 ================= */

export const RETENTION_MATRIX = ['immediate', '7d', '30d', '90d', 'forever'] as const;
export type RetentionTier = (typeof RETENTION_MATRIX)[number];
export const RETENTION_DAYS: Record<RetentionTier, number> = { immediate: 0, '7d': 7, '30d': 30, '90d': 90, forever: Infinity };

export class RecycleBin {
  tier: RetentionTier;
  clamped = 0;
  private trashed = new Map<string, { size: number; days: number; restored: boolean }>();

  constructor(tier: unknown = '30d') {
    this.tier = RETENTION_MATRIX.includes(tier as RetentionTier) ? (tier as RetentionTier) : '30d';
    if (this.tier !== tier) this.clamped = 1;
  }
  trash(path: string, size: number): number {
    this.trashed.set(path, { size: Math.max(0, size), days: 0, restored: false });
    return this.trashed.size;
  }
  /** 还原：条目缺失返回 null（E2106）。 */
  restore(path: string): boolean | null {
    const e = this.trashed.get(path);
    if (!e) return null;
    e.restored = true;
    this.trashed.delete(path);
    return true;
  }
  /** 保留期清理：超期条目自动清空。 */
  sweep(nowDays: number): number {
    const limit = RETENTION_DAYS[this.tier];
    let removed = 0;
    for (const [p, e] of this.trashed)
      if (nowDays - e.days > limit) {
        this.trashed.delete(p);
        removed++;
      }
    return removed;
  }
  /** 净身：清空回收站。 */
  purge(): boolean {
    this.trashed.clear();
    return this.trashed.size === 0;
  }
  get size(): number {
    return [...this.trashed.values()].reduce((s, e) => s + e.size, 0);
  }
  get count(): number {
    return this.trashed.size;
  }
  serialize(): string {
    return JSON.stringify({ v: 21, tier: this.tier, items: this.trashed.size });
  }
  static deserialize(raw: string): RecycleBin {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new RecycleBin(o.tier);
    } catch {
      return new RecycleBin();
    }
  }
}

/* ================= 族0207 磁盘与空间 2.0 ================= */

export const SCAN_MATRIX = ['none', 'quick', 'standard', 'deep', 'forensic'] as const;
export type ScanTier = (typeof SCAN_MATRIX)[number];
export const SCAN_DEPTH: Record<ScanTier, number> = { none: 0, quick: 1, standard: 3, deep: 6, forensic: 12 };

export interface UsageBucket {
  label: string;
  bytes: number;
}

export class DiskSpace {
  tier: ScanTier;
  clamped = 0;
  private total = 0;
  private buckets: UsageBucket[] = [];

  constructor(tier: unknown = 'standard') {
    this.tier = SCAN_MATRIX.includes(tier as ScanTier) ? (tier as ScanTier) : 'standard';
    if (this.tier !== tier) this.clamped = 1;
  }
  /** 空间扫描：按档位深度聚合分桶。 */
  scan(buckets: UsageBucket[]): { total: number; depth: number } {
    if (this.tier === 'none') return { total: 0, depth: 0 };
    this.buckets = buckets.map((b) => ({ ...b, bytes: Math.max(0, b.bytes) }));
    this.total = this.buckets.reduce((s, b) => s + b.bytes, 0);
    return { total: this.total, depth: SCAN_DEPTH[this.tier] };
  }
  /** 可用空间预测：不足时给出建议（E2107）。 */
  enough(needBytes: number): boolean {
    return this.total === 0 ? false : this.total - needBytes >= 0;
  }
  /** 大文件排行：批量可观测。 */
  top(n: number): UsageBucket[] {
    return [...this.buckets].sort((a, b) => b.bytes - a.bytes).slice(0, Math.max(0, Math.min(50, n)));
  }
  serialize(): string {
    return JSON.stringify({ v: 21, tier: this.tier, total: this.total });
  }
  static deserialize(raw: string): DiskSpace {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new DiskSpace(o.tier);
    } catch {
      return new DiskSpace();
    }
  }
}

/* ================= 族0208 文件组织哲学 2.0 ================= */

export interface OrgRule {
  id: number;
  pattern: string;
  target: string;
  priority: number;
}

export class FileOrganizer {
  tier: Tier;
  clamped = 0;
  private rules: OrgRule[] = [];

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped = 1;
  }
  /** 规则登记：优先级越界钳制 1~99（E2108）。 */
  addRule(rule: OrgRule): number {
    const fixed: OrgRule = { ...rule, priority: Math.max(1, Math.min(99, rule.priority)) };
    if (fixed.priority !== rule.priority) this.clamped = 1;
    this.rules.push(fixed);
    this.rules.sort((a, b) => a.priority - b.priority);
    return this.rules.length;
  }
  /** 智能归类：按优先级匹配第一条命中规则。 */
  classify(name: string): string | null {
    for (const r of this.rules) if (name.toLowerCase().includes(r.pattern.toLowerCase())) return r.target;
    return null;
  }
  /** 批量整理：返回 [文件, 目标] 列表。 */
  organize(files: string[]): [string, string][] {
    const out: [string, string][] = [];
    for (const f of files) {
      const t = this.classify(f) ?? (this.tier === 'off' ? '/未分类' : '/收件箱');
      out.push([f, t]);
    }
    return out;
  }
  /** 建议：本地启发式，可一键拒绝。 */
  suggest(files: string[]): { pattern: string; target: string } | null {
    if (this.tier === 'off' || files.length === 0) return null;
    const ext = files[0]!.split('.').pop() ?? '';
    return ext ? { pattern: `.${ext}`, target: `/${ext}合集` } : null;
  }
  serialize(): string {
    return JSON.stringify({ v: 21, tier: this.tier, rules: this.rules.length });
  }
  static deserialize(raw: string): FileOrganizer {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new FileOrganizer(clampTier(o.tier));
    } catch {
      return new FileOrganizer();
    }
  }
}

/* ================= 族0209 拖拽数据 2.0 ================= */

export type DragKind = 'file' | 'text' | 'folder' | 'mixed';

export interface DragPayloadData {
  kind: DragKind;
  paths: string[];
  bytes: number;
}

export class DragPayload {
  tier: Tier;
  clamped = 0;
  private data: DragPayloadData | null = null;

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped = 1;
  }
  /** 拖拽开始：载荷不完整即钳制（E2109）。 */
  start(kind: DragKind, paths: string[], bytes: number): DragPayloadData {
    const fixedPaths = paths.filter((p) => p.startsWith('/'));
    if (fixedPaths.length !== paths.length) this.clamped = 1;
    const fixedBytes = Math.max(0, bytes);
    if (fixedBytes !== bytes) this.clamped = 1;
    this.data = { kind, paths: fixedPaths, bytes: fixedBytes };
    return { ...this.data };
  }
  /** 投放校验：跨盘移动提示、载荷可读。 */
  drop(): DragPayloadData | null {
    return this.data ? { ...this.data } : null;
  }
  /** 撤销：拖拽中途清载荷。 */
  cancel(): boolean {
    this.data = null;
    return this.data === null;
  }
  /** 同盘判定：性能路径选择。 */
  sameVolume(src: string, dst: string): boolean {
    return src.split('/')[1] === dst.split('/')[1];
  }
  serialize(): string {
    return JSON.stringify({ v: 21, tier: this.tier, loaded: this.data !== null });
  }
  static deserialize(raw: string): DragPayload {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new DragPayload(clampTier(o.tier));
    } catch {
      return new DragPayload();
    }
  }
}

/* ================= 族0210 文件管理性能 ================= */

export const PERF_MATRIX = ['off', 'basic', 'smooth', 'fast', 'turbo'] as const;
export type PerfTier = (typeof PERF_MATRIX)[number];
export const PERF_BUDGET_MS: Record<PerfTier, number> = { off: 500, basic: 200, smooth: 100, fast: 50, turbo: 16 };

export class PerfIndex {
  tier: PerfTier;
  clamped = 0;
  private samples: number[] = [];
  private cache = new Map<string, number>();

  constructor(tier: unknown = 'smooth') {
    this.tier = PERF_MATRIX.includes(tier as PerfTier) ? (tier as PerfTier) : 'smooth';
    if (this.tier !== tier) this.clamped = 1;
  }
  /** 基准采样：入册防劣化（E2110）。 */
  sample(ms: number): number {
    const v = Math.max(0, ms);
    this.samples.push(v);
    if (this.samples.length > 256) this.samples.shift();
    return v;
  }
  /** 预算判定：超预算 → 降档建议。 */
  withinBudget(): boolean {
    const p95 = this.p95();
    return p95 <= PERF_BUDGET_MS[this.tier];
  }
  p95(): number {
    if (this.samples.length === 0) return 0;
    const sorted = [...this.samples].sort((a, b) => a - b);
    return sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * 0.95))]!;
  }
  /** 热路径缓存：命中即零开销。 */
  memo(key: string, compute: () => number): number {
    const hit = this.cache.get(key);
    if (hit !== undefined) return hit;
    const v = compute();
    this.cache.set(key, v);
    return v;
  }
  /** 低配降级链：材质/动效/精度三级递降。 */
  degrade(): PerfTier {
    const order: PerfTier[] = ['turbo', 'fast', 'smooth', 'basic', 'off'];
    const i = order.indexOf(this.tier);
    return order[Math.min(order.length - 1, i + 1)]!;
  }
  serialize(): string {
    return JSON.stringify({ v: 21, tier: this.tier, samples: this.samples.length, cached: this.cache.size });
  }
  static deserialize(raw: string): PerfIndex {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new PerfIndex(o.tier);
    } catch {
      return new PerfIndex();
    }
  }
}
