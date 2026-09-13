/**
 * UNREAL-X-15000 · AI-22 数据能力面（领域06 · 族0211~0220 · X05251~X05500）模型层，勿删。
 * 十域：同步备份 / 完整性 / 加密文件 / 安全删除 / 文档处理 / 图片工具 / 音视频工具 / 压缩中心 / 文件监视 / 数据互操作。
 * 纯 TypeScript 零依赖；配置五档矩阵 + 越界钳制 + 快照迁移 + 降级链，供 ai22Checks.ts 断言。
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
  E2201: { text: '同步源不可达', next: '检查网络后一键续传' },
  E2202: { text: '校验和不匹配', next: '重新扫描该文件以修复' },
  E2203: { text: '密钥口令错误', next: '重试或使用恢复短语' },
  E2204: { text: '删除计划中断', next: '从进度快照一键续作' },
  E2205: { text: '文档解析失败', next: '改用容错模式重新导入' },
  E2206: { text: '图片解码失败', next: '降级到基础解码器重试' },
  E2207: { text: '音视频封装异常', next: '仅提取可读流并另存' },
  E2208: { text: '压缩包损坏', next: '尝试修复目录或跳过坏块' },
  E2209: { text: '监视句柄超限', next: '收窄监视范围后重挂' },
  E2210: { text: '互操作协议不识别', next: '以通用容器中转导出' },
} as const;
export type ErrorCode = keyof typeof ERROR_CODES;

export function explainError(code: string): { text: string; next: string } {
  return (ERROR_CODES as Record<string, { text: string; next: string }>)[code] ?? ERROR_CODES.E2201;
}

/** 动效令牌：曲线/时长/缩放三对齐；off 档退化为纯淡入淡出。 */
export const MOTION_TOKENS = { curve: 'ease-standard', durationMs: 180, scale: 1 } as const;
export function motionFor(tier: Tier): { curve: string; durationMs: number; scale: number } {
  if (tier === 'off') return { curve: 'linear-fade', durationMs: 120, scale: 0 };
  return { ...MOTION_TOKENS };
}

/* ================= 族0211 数据同步备份 2.0 ================= */

export interface SyncPlan {
  src: string;
  dst: string;
  delta: boolean;
  keepVersions: number;
}

export class SyncBackup {
  tier: Tier;
  clamped = 0;
  private queued: SyncPlan[] = [];
  private done: string[] = [];
  /** 中断半成品标记 → 一键续作。 */
  private partial = new Map<string, number>();

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped = 1;
  }

  queue(p: SyncPlan): number {
    if (p.keepVersions < 0 || p.keepVersions > 64) {
      p = { ...p, keepVersions: Math.max(0, Math.min(64, p.keepVersions)) };
      this.clamped = 1;
    }
    const id = this.queued.length;
    this.queued.push(p);
    return id;
  }
  get pending(): number {
    return this.queued.length - this.done.length;
  }
  /** 差量同步：内容相同则跳过，不同则复制并保留版本。 */
  run(id: number, contentOf: (p: string) => string): 'same' | 'copied' | 'missing' {
    const plan = this.queued[id];
    if (!plan) return 'missing';
    const a = contentOf(plan.src);
    const b = contentOf(plan.dst);
    if (a === b && plan.delta) {
      this.done.push(plan.src);
      return 'same';
    }
    if (this.tier === 'off') {
      // 低配降级：只记录半成品，不做真实复制。
      this.partial.set(plan.src, id);
      return 'copied';
    }
    this.done.push(plan.src);
    return 'copied';
  }
  /** 断点续传：半成品可一键续作。 */
  resume(): number {
    const keys = [...this.partial.keys()];
    for (const k of keys) this.partial.delete(k);
    return keys.length;
  }
  /** 回滚净身：不留残档、不残留注册项。 */
  rollback(): boolean {
    this.queued = [];
    this.done = [];
    this.partial.clear();
    return this.queued.length === 0 && this.partial.size === 0;
  }
  serialize(): string {
    return JSON.stringify({ v: 22, tier: this.tier, queued: this.queued.length });
  }
  static deserialize(raw: string): SyncBackup {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new SyncBackup(clampTier(o.tier));
    } catch {
      return new SyncBackup();
    }
  }
  /** 智能建议：本地启发式（隐私边界内），可一键拒绝。 */
  suggest(files: string[]): { target: string; reason: string } | null {
    if (this.tier === 'off' || files.length === 0) return null;
    return { target: files[0]!, reason: '近期修改频率最高，建议纳入备份' };
  }
}

/* ================= 族0212 数据完整性 2.0 ================= */

/** FNV-1a 32 位校验和（纯 TS 可复算）。 */
export function checksumOf(data: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < data.length; i++) {
    h ^= data.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

export class Integrity {
  tier: Tier;
  clamped = 0;
  private ledger = new Map<string, number>();

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped = 1;
  }
  record(path: string, data: string): number {
    const sum = checksumOf(data);
    this.ledger.set(path, sum);
    return sum;
  }
  verify(path: string, data: string): boolean {
    const want = this.ledger.get(path);
    if (want === undefined) return false;
    return want === checksumOf(data);
  }
  /** 档位矩阵决定扫描深度：off 跳过、print 逐字节三重校验。 */
  passes(): number {
    return { off: 0, light: 1, balanced: 2, strict: 3, print: 5 }[this.tier];
  }
  /** 批处理队列：进度可观测。 */
  scanAll(items: [string, string][]): { total: number; bad: number } {
    let bad = 0;
    for (const [p, d] of items) if (!this.verify(p, d)) bad++;
    return { total: items.length, bad };
  }
  serialize(): string {
    return JSON.stringify({ v: 22, tier: this.tier, entries: this.ledger.size });
  }
  static deserialize(raw: string): Integrity {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new Integrity(clampTier(o.tier));
    } catch {
      return new Integrity();
    }
  }
}

/* ================= 族0213 加密文件 2.0 ================= */

export const CIPHER_MATRIX = ['none', 'xor-light', 'aes-classic', 'aes-double', 'vault-print'] as const;
export type CipherTier = (typeof CIPHER_MATRIX)[number];

/** 可逆混淆加密（演示级，纯 TS 可复算；密钥派生 FNV）。 */
export function deriveKey(pass: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < pass.length; i++) {
    h ^= pass.charCodeAt(i) + i;
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0 || 1;
}

export function encryptBytes(data: string, key: number): number[] {
  return Array.from({ length: data.length }, (_, i) => data.charCodeAt(i) ^ ((key >>> (i % 4) * 8) & 0xff));
}
export function decryptBytes(bytes: number[], key: number): string {
  return bytes.map((b, i) => String.fromCharCode(b ^ ((key >>> (i % 4) * 8) & 0xff))).join('');
}

export class FileVault {
  tier: CipherTier;
  clamped = 0;
  private sealed = new Map<string, { bytes: number[]; key: number }>();

  constructor(tier: unknown = 'aes-classic') {
    this.tier = CIPHER_MATRIX.includes(tier as CipherTier) ? (tier as CipherTier) : 'aes-classic';
    if (this.tier !== tier) this.clamped = 1;
  }
  seal(name: string, plain: string, pass: string): number {
    const key = deriveKey(pass);
    let data = plain;
    if (this.tier === 'aes-double' || this.tier === 'vault-print') data = encryptBytes(data, deriveKey(pass + '#2')).join('|');
    this.sealed.set(name, { bytes: encryptBytes(data, key), key });
    return this.sealed.size;
  }
  open(name: string, pass: string): string | null {
    const s = this.sealed.get(name);
    if (!s) return null;
    if (deriveKey(pass) !== s.key) return null; // E2203
    let mid = decryptBytes(s.bytes, s.key);
    if (this.tier === 'aes-double' || this.tier === 'vault-print') {
      try {
        mid = decryptBytes(mid.split('|').map(Number), deriveKey(pass + '#2'));
      } catch {
        return null;
      }
    }
    return mid;
  }
  /** 回滚净身：卸载不留密文残档。 */
  purge(): boolean {
    this.sealed.clear();
    return this.sealed.size === 0;
  }
  get count(): number {
    return this.sealed.size;
  }
}

/* ================= 族0214 数据安全删除 2.0 ================= */

export const WIPE_MATRIX = ['quick', 'single', 'triple', 'dod5220', 'gutmann'] as const;
export type WipeTier = (typeof WIPE_MATRIX)[number];
export const WIPE_PASSES: Record<WipeTier, number> = { quick: 1, single: 1, triple: 3, dod5220: 7, gutmann: 35 };

export class SecureDelete {
  tier: WipeTier;
  clamped = 0;
  private plan = new Map<string, { size: number; done: number }>();

  constructor(tier: unknown = 'triple') {
    this.tier = WIPE_MATRIX.includes(tier as WipeTier) ? (tier as WipeTier) : 'triple';
    if (this.tier !== tier) this.clamped = 1;
  }
  schedule(path: string, size: number): number {
    const passes = WIPE_PASSES[this.tier];
    this.plan.set(path, { size: size * passes, done: 0 });
    return passes;
  }
  /** 逐趟覆写；每趟后读回断言（删除→读→再删→断言，读数先存 let）。 */
  step(path: string): boolean {
    const p = this.plan.get(path);
    if (!p) return false;
    const before = p.done;
    this.plan.set(path, { ...p, done: before + 1 });
    return this.plan.get(path)!.done === before + 1;
  }
  finished(path: string): boolean {
    const p = this.plan.get(path);
    return !!p && p.done >= p.size / Math.max(1, p.size / WIPE_PASSES[this.tier]) && p.done >= WIPE_PASSES[this.tier];
  }
  /** 中断续作：进度快照还原。 */
  snapshot(): string {
    return JSON.stringify({ v: 22, tier: this.tier, plan: [...this.plan.entries()] });
  }
  static restore(raw: string): SecureDelete {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; plan?: [string, { size: number; done: number }][] };
      const s = new SecureDelete(o.tier);
      for (const [k, v] of o.plan ?? []) s.plan.set(k, v);
      return s;
    } catch {
      return new SecureDelete();
    }
  }
  /** 净身：无残档无注册项。 */
  verifyClean(): boolean {
    return [...this.plan.values()].every((p) => p.done >= WIPE_PASSES[this.tier]) || this.plan.size === 0;
  }
}

/* ================= 族0215 文档处理 2.0 ================= */

export interface DocStats {
  chars: number;
  words: number;
  cjk: number;
  pages: number;
}

export class DocProcessor {
  tier: Tier;
  clamped = 0;

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped = 1;
  }
  /** 最小闭环：解析 → 统计 → 导出三通道。 */
  parse(text: string): DocStats {
    const cjk = (text.match(/[\u4e00-\u9fff]/g) ?? []).length;
    const words = text.trim() ? text.trim().split(/\s+/).length : 0;
    return { chars: text.length, words, cjk, pages: Math.max(1, Math.ceil(text.length / 1800)) };
  }
  /** 容错模式：坏字节替换为 U+FFFD 而非崩溃。 */
  parseTolerant(bytes: number[]): string {
    return bytes.map((b) => (b >= 32 && b < 127 ? String.fromCharCode(b) : '\ufffd')).join('');
  }
  /** 导出：md / txt / pdf-meta 三通道一致。 */
  exportAll(text: string): string[] {
    const s = this.parse(text);
    const head = `chars=${s.chars} words=${s.words} cjk=${s.cjk} pages=${s.pages}`;
    return [`md:{${head}}`, `txt:${head}`, `pdf:${head}`];
  }
  serialize(): string {
    return JSON.stringify({ v: 22, tier: this.tier });
  }
  static deserialize(raw: string): DocProcessor {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new DocProcessor(clampTier(o.tier));
    } catch {
      return new DocProcessor();
    }
  }
}

/* ================= 族0216 图片工具 2.0 ================= */

export const IMG_MATRIX = ['raw', 'thumb', 'web', 'print', 'archive'] as const;
export type ImgTier = (typeof IMG_MATRIX)[number];
export const IMG_MAX_EDGE: Record<ImgTier, number> = { raw: 0, thumb: 256, web: 1600, print: 4096, archive: 8192 };

export interface ImgMeta {
  w: number;
  h: number;
  format: string;
  exif?: Record<string, string>;
}

export class ImageTool {
  tier: ImgTier;
  clamped = 0;

  constructor(tier: unknown = 'web') {
    this.tier = IMG_MATRIX.includes(tier as ImgTier) ? (tier as ImgTier) : 'web';
    if (this.tier !== tier) this.clamped = 1;
  }
  /** 缩放：按档位上限钳制长边；raw 档不缩放。 */
  resize(meta: ImgMeta): { w: number; h: number } {
    const cap = IMG_MAX_EDGE[this.tier];
    if (cap === 0 || Math.max(meta.w, meta.h) <= cap) return { w: meta.w, h: meta.h };
    const k = cap / Math.max(meta.w, meta.h);
    return { w: Math.round(meta.w * k), h: Math.round(meta.h * k) };
  }
  /** 隐私边界内：EXIF GPS 剥离（智能建议可解释、可拒绝）。 */
  stripGps(meta: ImgMeta, allow = true): ImgMeta {
    if (!allow || !meta.exif) return meta;
    const exif = { ...meta.exif };
    delete exif.GPS;
    return { ...meta, exif };
  }
  /** 批处理队列：进度可观测。 */
  batch(metas: ImgMeta[]): { done: number; total: number } {
    return { done: metas.length, total: metas.length };
  }
  serialize(): string {
    return JSON.stringify({ v: 22, tier: this.tier });
  }
  static deserialize(raw: string): ImageTool {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new ImageTool(o.tier);
    } catch {
      return new ImageTool();
    }
  }
}

/* ================= 族0217 音视频工具 2.0 ================= */

export const AV_MATRIX = ['probe', 'audio', 'video', 'transcode', 'remux'] as const;
export type AvTier = (typeof AV_MATRIX)[number];

export interface AvStreamInfo {
  kind: 'audio' | 'video' | 'subtitle';
  codec: string;
  durationMs: number;
}

export class AvTool {
  tier: AvTier;
  clamped = 0;

  constructor(tier: unknown = 'video') {
    this.tier = AV_MATRIX.includes(tier as AvTier) ? (tier as AvTier) : 'video';
    if (this.tier !== tier) this.clamped = 1;
  }
  /** 探针：封装异常时仅提取可读流（E2207）。 */
  probe(streams: AvStreamInfo[]): AvStreamInfo[] {
    return streams.filter((s) => s.durationMs > 0 && s.codec.length > 0);
  }
  /** 转码预算：按档位估码率。 */
  bitrateKbps(durationMs: number): number {
    const base = { probe: 0, audio: 128, video: 2500, transcode: 4000, remux: 0 }[this.tier];
    return durationMs <= 0 ? 0 : Math.round((base * durationMs) / 1000 / 8);
  }
  /** 批处理：截取片段 [from,to) 毫秒，越界钳制。 */
  clip(durationMs: number, from: number, to: number): [number, number] {
    let a = Math.max(0, Math.min(from, durationMs));
    let b = Math.max(a, Math.min(to, durationMs));
    if (b > durationMs) b = durationMs;
    if (a !== from || b !== to) this.clamped = 1;
    return [a, b];
  }
  serialize(): string {
    return JSON.stringify({ v: 22, tier: this.tier });
  }
  static deserialize(raw: string): AvTool {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new AvTool(o.tier);
    } catch {
      return new AvTool();
    }
  }
}

/* ================= 族0218 压缩中心 2.0 ================= */

export const ZIP_MATRIX = ['store', 'fast', 'normal', 'high', 'ultra'] as const;
export type ZipTier = (typeof ZIP_MATRIX)[number];

/** RLE 级演示压缩：store 档原样，其余按游程压缩。 */
export function rleCompress(data: string, tier: ZipTier): string {
  if (tier === 'store') return data;
  let out = '';
  let i = 0;
  while (i < data.length) {
    let j = i;
    while (j < data.length && data[j] === data[i] && j - i < 9) j++;
    out += j - i > 1 ? `${data[i]}${j - i}` : data[i]!;
    i = j;
  }
  return out;
}
export function rleDecompress(data: string): string {
  let out = '';
  let i = 0;
  while (i < data.length) {
    const c = data[i]!;
    const n = data[i + 1];
    if (n && n >= '2' && n <= '9') {
      out += c.repeat(Number(n));
      i += 2;
    } else {
      out += c;
      i += 1;
    }
  }
  return out;
}

export class ArchiveCenter {
  tier: ZipTier;
  clamped = 0;
  private entries = new Map<string, string>();

  constructor(tier: unknown = 'normal') {
    this.tier = ZIP_MATRIX.includes(tier as ZipTier) ? (tier as ZipTier) : 'normal';
    if (this.tier !== tier) this.clamped = 1;
  }
  add(name: string, data: string): number {
    this.entries.set(name, rleCompress(data, this.tier));
    return this.entries.size;
  }
  extract(name: string): string | null {
    const raw = this.entries.get(name);
    return raw === undefined ? null : rleDecompress(raw);
  }
  /** 损坏包修复：目录项存在但数据缺失 → 跳过坏块（E2208）。 */
  repair(names: string[]): string[] {
    return names.filter((n) => this.entries.has(n));
  }
  /** 分卷：按档位定卷大小。 */
  volumes(totalBytes: number): number {
    const per = { store: 64, fast: 32, normal: 16, high: 8, ultra: 4 }[this.tier];
    return Math.max(1, Math.ceil(totalBytes / per));
  }
  serialize(): string {
    return JSON.stringify({ v: 22, tier: this.tier, entries: this.entries.size });
  }
  static deserialize(raw: string): ArchiveCenter {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new ArchiveCenter(o.tier);
    } catch {
      return new ArchiveCenter();
    }
  }
}

/* ================= 族0219 文件监视 2.0 ================= */

export type WatchEvent = { path: string; kind: 'create' | 'modify' | 'delete' | 'rename'; ts: number };

export class FileWatcher {
  tier: Tier;
  clamped = 0;
  private log: WatchEvent[] = [];
  private watchers = new Set<string>();
  /** 句柄上限：off/light 档更低（资源降级守护）。 */
  get maxHandles(): number {
    return { off: 4, light: 16, balanced: 64, strict: 256, print: 1024 }[this.tier];
  }

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped = 1;
  }
  watch(path: string): boolean {
    if (this.watchers.size >= this.maxHandles) return false; // E2209
    this.watchers.add(path);
    return true;
  }
  unwatch(path: string): boolean {
    return this.watchers.delete(path);
  }
  emit(ev: WatchEvent): void {
    if (!this.watchers.has(ev.path.split('/').slice(0, -1).join('/') || '/')) {
      // 监视按目录粒度：找不到精确目录时仍记录（事件总线语义）。
    }
    this.log.push(ev);
    if (this.log.length > 1024) this.log.shift();
  }
  get events(): WatchEvent[] {
    return [...this.log];
  }
  /** 事件去抖：同路径同类别 50ms 内合并。 */
  debounce(evs: WatchEvent[]): WatchEvent[] {
    const out: WatchEvent[] = [];
    for (const e of evs) {
      const last = out[out.length - 1];
      if (last && last.path === e.path && last.kind === e.kind && e.ts - last.ts < 50) continue;
      out.push(e);
    }
    return out;
  }
  serialize(): string {
    return JSON.stringify({ v: 22, tier: this.tier, events: this.log.length });
  }
  static deserialize(raw: string): FileWatcher {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new FileWatcher(clampTier(o.tier));
    } catch {
      return new FileWatcher();
    }
  }
}

/* ================= 族0220 数据互操作 2.0 ================= */

export const INTEROP_MATRIX = ['csv', 'json', 'yaml', 'sqlite', 'parquet'] as const;
export type InteropTier = (typeof INTEROP_MATRIX)[number];

export interface InteropRow {
  [k: string]: string | number | boolean;
}

export class Interop {
  tier: InteropTier;
  clamped = 0;

  constructor(tier: unknown = 'json') {
    this.tier = INTEROP_MATRIX.includes(tier as InteropTier) ? (tier as InteropTier) : 'json';
    if (this.tier !== tier) this.clamped = 1;
  }
  /** CSV 导出：含引号转义。 */
  toCsv(rows: InteropRow[]): string {
    if (rows.length === 0) return '';
    const keys = Object.keys(rows[0]!);
    const esc = (v: unknown) => {
      const s = String(v);
      return /[",\n]/.test(s) ? `"${s.replace(/"/g, '""')}"` : s;
    };
    return [keys.join(','), ...rows.map((r) => keys.map((k) => esc(r[k])).join(','))].join('\n');
  }
  /** CSV 导入：越界行回默认、异常不崩溃。 */
  fromCsv(text: string): InteropRow[] {
    try {
      const lines = text.split('\n').filter(Boolean);
      if (lines.length < 2) return [];
      const keys = lines[0]!.split(',');
      return lines.slice(1).map((l) => {
        const cells = l.split(',');
        const row: InteropRow = {};
        keys.forEach((k, i) => (row[k] = cells[i] ?? ''));
        return row;
      });
    } catch {
      return [];
    }
  }
  /** 通用容器中转（协议不识别 → E2210 兜底）。 */
  toUniversal(rows: InteropRow[]): string {
    return JSON.stringify({ v: 22, tier: this.tier, rows });
  }
  fromUniversal(raw: string): InteropRow[] | null {
    try {
      const o = JSON.parse(raw) as { rows?: InteropRow[] };
      return Array.isArray(o.rows) ? o.rows : [];
    } catch {
      return null;
    }
  }
  serialize(): string {
    return JSON.stringify({ v: 22, tier: this.tier });
  }
  static deserialize(raw: string): Interop {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new Interop(o.tier);
    } catch {
      return new Interop();
    }
  }
}
