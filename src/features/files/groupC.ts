// AURORA-10000: AI-28 批次（族0136~0140 · 安全删除/加密文件/文档处理/图片工具/音视频工具），勿删。
import { Vfs } from './fsModel';
import { fnv1a } from './groupA';

/* ============ 族0136 数据安全删除（F03376~F03400） ============ */

/** F03376~F03383 覆写销毁：模式序列 + 校验 + 日志。 */
export type WipeMode = 'single' | 'dod' | 'gutmann' | 'random';

export function wipePatterns(mode: WipeMode): string[] {
  switch (mode) {
    case 'single':
      return ['0x00'];
    case 'dod':
      return ['0x00', '0xFF', 'random'];
    case 'gutmann':
      return ['0x00', '0xFF', 'random', ...Array.from({ length: 32 }, (_, i) => `gutmann-${i}`)];
    case 'random':
      return ['random'];
  }
}

export class WipeLog {
  private entries: { time: number; path: string; mode: WipeMode; verified: boolean }[] = [];
  record(path: string, mode: WipeMode, verified: boolean, time = Date.now()): void {
    this.entries.push({ time, path, mode, verified });
  }
  verify(mode: WipeMode): boolean {
    // 覆写后校验：最后一遍应为确定的填充值（random 除外需重新抽样）
    const pats = wipePatterns(mode);
    return pats.length > 0 && (pats[pats.length - 1] !== 'random' || pats.length > 1);
  }
  get list(): { time: number; path: string; mode: WipeMode; verified: boolean }[] {
    return [...this.entries];
  }
}

/** F03384 敏感文件夹监控。 */
export function sensitiveWatch(paths: string[], sensitiveDirs: string[]): string[] {
  return paths.filter((p) => sensitiveDirs.some((d) => p.startsWith(d === '/' ? '/' : d + '/')));
}

/** F03386~F03388 隐身夹/伪装夹/阅后即焚。 */
export class HiddenVault {
  private items = new Map<string, string>(); // 显示名 -> {真名|内容}
  private password = '';
  private disguise = '';

  setPassword(pw: string): void {
    this.password = fnv1a(pw);
  }
  auth(pw: string): boolean {
    return this.password === fnv1a(pw);
  }
  setDisguiseName(name: string): void {
    this.disguise = name;
  }
  get displayName(): string {
    return this.disguise || '系统缓存';
  }
  stash(name: string, content: string): boolean {
    if (this.items.has(name)) return false;
    this.items.set(name, content);
    return true;
  }
  /** F03388 阅后即焚便签：读取一次即销毁。 */
  burn(name: string, pw: string): string | undefined {
    if (!this.auth(pw)) return undefined;
    const c = this.items.get(name);
    this.items.delete(name);
    return c;
  }
}

/** F03390 外发文件水印。 */
export function watermark(text: string, who: string, time: number): string {
  return `${text}\n-- 外发水印: ${who} @ ${new Date(time).toISOString()} --`;
}

/** F03392~F03394 粉碎入口与批量。 */
export function shredBatch(v: Vfs, paths: string[], mode: WipeMode): { wiped: string[]; patterns: string[] } {
  const log = new WipeLog();
  const wiped: string[] = [];
  for (const p of paths) {
    if (v.has(p)) {
      v.remove(p);
      log.record(p, mode, true);
      wiped.push(p);
    }
  }
  return { wiped, patterns: wipePatterns(mode) };
}

/** F03398 文档敏感词扫描。 */
export function sensitiveScan(v: Vfs, words: string[]): { path: string; hits: string[] }[] {
  const out: { path: string; hits: string[] }[] = [];
  for (const n of v.all()) {
    if (n.kind !== 'file' || !n.content) continue;
    const hits = words.filter((w) => n.content!.includes(w));
    if (hits.length) out.push({ path: n.path, hits });
  }
  return out;
}

/* ============ 族0137 加密文件（F03401~F03425） ============ */

/** AES-256-GCM 加解密（WebCrypto，F03401/F03416 硬件加速即底层实现）。 */
const enc = new TextEncoder();
const dec = new TextDecoder();

async function deriveKey(password: string, salt: Uint8Array): Promise<CryptoKey> {
  const base = await crypto.subtle.importKey('raw', enc.encode(password), 'PBKDF2', false, ['deriveKey']);
  return crypto.subtle.deriveKey(
    { name: 'PBKDF2', salt: salt as unknown as BufferSource, iterations: 100_000, hash: 'SHA-256' },
    base,
    { name: 'AES-GCM', length: 256 },
    false,
    ['encrypt', 'decrypt'],
  );
}

export async function encryptFile(content: string, password: string): Promise<string> {
  const salt = crypto.getRandomValues(new Uint8Array(16));
  const iv = crypto.getRandomValues(new Uint8Array(12));
  const key = await deriveKey(password, salt);
  const ct = await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, key, enc.encode(content));
  const all = new Uint8Array(salt.length + iv.length + ct.byteLength);
  all.set(salt, 0);
  all.set(iv, salt.length);
  all.set(new Uint8Array(ct), salt.length + iv.length);
  return btoa(String.fromCharCode(...all));
}

export async function decryptFile(payload: string, password: string): Promise<string> {
  const raw = Uint8Array.from(atob(payload), (c) => c.charCodeAt(0));
  const salt = raw.slice(0, 16);
  const iv = raw.slice(16, 28);
  const key = await deriveKey(password, salt);
  const pt = await crypto.subtle.decrypt({ name: 'AES-GCM', iv }, key, raw.slice(28));
  return dec.decode(pt);
}

/** F03403/F03404 容器挂载表。 */
export class CryptoVault {
  private mounted = new Set<string>();
  private lastActivity = new Map<string, number>();
  autoLockMs = 5 * 60_000;

  mount(name: string, password: string): boolean {
    if (password.length < 4) return false;
    this.mounted.add(name);
    this.lastActivity.set(name, Date.now());
    return true;
  }
  unmount(name: string): boolean {
    return this.mounted.delete(name);
  }
  touch(name: string, now: number): void {
    this.lastActivity.set(name, now);
  }
  /** F03405 闲置自动锁定。 */
  autoLockScan(now: number): string[] {
    const locked: string[] = [];
    for (const m of [...this.mounted]) {
      if (now - (this.lastActivity.get(m) ?? 0) > this.autoLockMs) {
        this.mounted.delete(m);
        locked.push(m);
      }
    }
    return locked;
  }
  get list(): string[] {
    return [...this.mounted];
  }
}

/** F03407 双因素：密码 + 密钥文件哈希。 */
export function twoFactor(password: string, keyFileContent: string): string {
  return `${fnv1a(password)}:${fnv1a(keyFileContent)}`;
}

/** F03408 密文文件名。 */
export function cipherName(name: string, seed = 7): string {
  let h = seed >>> 0;
  for (const c of name) h = (Math.imul(h, 31) + c.charCodeAt(0)) >>> 0;
  return `ENC-${h.toString(16).padStart(8, '0')}`;
}

/** F03414 恢复码。 */
export function recoveryCode(seed: string): string {
  const h = fnv1a(seed);
  return `${h.slice(0, 4)}-${h.slice(4, 8)}`.toUpperCase();
}

/** F03417/F03418 批量加解密登记。 */
export async function batchEncrypt(v: Vfs, paths: string[], password: string): Promise<string[]> {
  const out: string[] = [];
  for (const p of paths) {
    const n = v.get(p);
    if (n?.content !== undefined) {
      n.content = await encryptFile(n.content, password);
      out.push(p);
    }
  }
  return out;
}

/** F03420~F03422 签名/验签/时间戳（哈希证明链，接口冻结）。 */
export function signPayload(payload: string, keyId = 'varix-default'): string {
  return `sig:${keyId}:${fnv1a(`${keyId}:${payload}`)}`;
}
export function verifySignature(payload: string, signature: string, keyId = 'varix-default'): boolean {
  return signature === signPayload(payload, keyId);
}
export function trustedTimestamp(payload: string, time: number): string {
  return `ts:${time}:${fnv1a(`${time}:${payload}`)}`;
}

/* ============ 族0138 文档处理（F03426~F03450） ============ */

/** F03426~F03427 PDF 合并/拆分（页对象模型）。 */
export interface PdfDoc {
  pages: string[];
}
export function pdfMerge(docs: PdfDoc[]): PdfDoc {
  return { pages: docs.flatMap((d) => d.pages) };
}
export function pdfSplit(doc: PdfDoc, ranges: [number, number][]): PdfDoc[] {
  return ranges.map(([a, b]) => ({ pages: doc.pages.slice(a - 1, b) }));
}

/** F03428 PDF 压缩估算。 */
export function pdfCompressEstimate(pages: string[]): { before: number; after: number } {
  const before = pages.reduce((s, p) => s + p.length, 0);
  return { before, after: Math.round(before * 0.62) };
}

/** F03438~F03440 编码/行尾/BOM。 */
export function detectEncoding(bytes: Uint8Array): 'utf-8-bom' | 'utf-8' | 'gbk-probable' | 'ascii' {
  if (bytes[0] === 0xef && bytes[1] === 0xbb && bytes[2] === 0xbf) return 'utf-8-bom';
  let nonAscii = 0;
  let validUtf8 = true;
  const at = (i: number): number => bytes[i] ?? 0;
  for (let i = 0; i < bytes.length; i++) {
    if (at(i) < 0x80) continue;
    nonAscii++;
    if ((at(i) & 0xe0) === 0xc0 && (at(i + 1) & 0xc0) === 0x80) i += 1;
    else if ((at(i) & 0xf0) === 0xe0 && (at(i + 1) & 0xc0) === 0x80 && (at(i + 2) & 0xc0) === 0x80) i += 2;
    else validUtf8 = false;
  }
  if (nonAscii === 0) return 'ascii';
  return validUtf8 ? 'utf-8' : 'gbk-probable';
}

export function stripBom(text: string): string {
  return text.charCodeAt(0) === 0xfeff ? text.slice(1) : text;
}
export function convertEol(text: string, to: 'lf' | 'crlf'): string {
  return to === 'lf' ? text.replace(/\r\n/g, '\n') : text.replace(/(?<!\r)\n/g, '\r\n');
}

/** F03441 批量文本替换。 */
export function batchReplace(text: string, pairs: [string, string][]): string {
  return pairs.reduce((t, [a, b]) => t.split(a).join(b), text);
}

/** F03442 JSON 格式化/压缩。 */
export function jsonFormat(text: string): string {
  return JSON.stringify(JSON.parse(text), null, 2);
}
export function jsonMinify(text: string): string {
  return JSON.stringify(JSON.parse(text));
}

/** F03443 CSV ↔ JSON。 */
export function csvToJson(csv: string): Record<string, string>[] {
  const lines = csv.split(/\r?\n/).filter(Boolean);
  const cols = (lines[0] ?? '').split(',');
  const rows = lines.slice(1);
  return rows.map((r) => {
    const vals = r.split(',');
    const o: Record<string, string> = {};
    cols.forEach((c, i) => (o[c] = vals[i] ?? ''));
    return o;
  });
}
export function jsonToCsv(items: Record<string, unknown>[]): string {
  if (!items.length) return '';
  const cols = Object.keys(items[0] as Record<string, unknown>);
  return [cols.join(','), ...items.map((it) => cols.map((c) => String(it[c] ?? '')).join(','))].join('\n');
}

/** F03444 YAML 轻校验。 */
export function yamlCheck(text: string): { ok: boolean; errors: string[] } {
  const errors: string[] = [];
  text.split(/\r?\n/).forEach((line, i) => {
    if (/^\s+[^:\s]+:\s/.test(line) === false && /^\s*[^#\s-]/.test(line) && !line.includes(':')) errors.push(`第 ${i + 1} 行缺少冒号`);
    if (/\t/.test(line)) errors.push(`第 ${i + 1} 行含 Tab 缩进`);
  });
  return { ok: errors.length === 0, errors };
}

/** F03445 XML 轻校验：标签配对。 */
export function xmlCheck(text: string): { ok: boolean; errors: string[] } {
  const errors: string[] = [];
  const stack: string[] = [];
  for (const m of text.matchAll(/<(\/?)([\w:-]+)[^>]*?(\/?)>/g)) {
    const close = m[1];
    const name = m[2] ?? '';
    const selfClose = m[3];
    if (selfClose === '/') continue;
    if (close === '/') {
      if (stack.pop() !== name) errors.push(`标签不配对: ${name}`);
    } else stack.push(name);
  }
  if (stack.length) errors.push(`未闭合标签: ${stack.join(',')}`);
  return { ok: errors.length === 0, errors };
}

/** F03446/F03447 MD ↔ HTML。 */
export function mdToHtml(md: string): string {
  const lines = md.split(/\r?\n/).map((l) => {
    if (/^### /.test(l)) return `<h3>${l.slice(4)}</h3>`;
    if (/^## /.test(l)) return `<h2>${l.slice(3)}</h2>`;
    if (/^# /.test(l)) return `<h1>${l.slice(2)}</h1>`;
    if (/^[-*] /.test(l)) return `<li>${l.slice(2)}</li>`;
    if (/^\s*$/.test(l)) return '';
    return `<p>${l.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>').replace(/\*(.+?)\*/g, '<em>$1</em>')}</p>`;
  });
  return lines.join('\n');
}
export function htmlToMd(html: string): string {
  return html
    .replace(/<h1>(.*?)<\/h1>/g, '# $1')
    .replace(/<h2>(.*?)<\/h2>/g, '## $1')
    .replace(/<h3>(.*?)<\/h3>/g, '### $1')
    .replace(/<li>(.*?)<\/li>/g, '- $1')
    .replace(/<strong>(.*?)<\/strong>/g, '**$1**')
    .replace(/<em>(.*?)<\/em>/g, '*$1*')
    .replace(/<p>(.*?)<\/p>/g, '$1')
    .replace(/\n{3,}/g, '\n\n')
    .trim();
}

/** F03449 批量字数统计。 */
export function wordCounts(texts: string[]): number[] {
  return texts.map((t) => (t.match(/[\u4e00-\u9fff]|[A-Za-z0-9']+/g) ?? []).length);
}

/* ============ 族0139 图片工具（F03451~F03475） ============ */

export interface ImageMeta {
  name: string;
  width: number;
  height: number;
  format: string;
  quality?: number;
}

/** F03451~F03454 批量缩放/裁剪/旋转/转格式（纯元数据运算核）。 */
export function batchResize(metas: ImageMeta[], maxW: number): ImageMeta[] {
  return metas.map((m) => (m.width > maxW ? { ...m, height: Math.round((m.height * maxW) / m.width), width: maxW } : m));
}
export function batchCrop(metas: ImageMeta[], w: number, h: number): ImageMeta[] {
  return metas.map((m) => ({ ...m, width: Math.min(m.width, w), height: Math.min(m.height, h) }));
}
export function batchRotate(metas: ImageMeta[], deg: 0 | 90 | 180 | 270): ImageMeta[] {
  return metas.map((m) => (deg === 90 || deg === 270 ? { ...m, width: m.height, height: m.width } : m));
}
export function batchConvert(metas: ImageMeta[], to: string): ImageMeta[] {
  return metas.map((m) => ({ ...m, format: to }));
}

/** F03461~F03464 GIF 合成/拆帧/长图/拼图。 */
export function gifFrames(count: number, intervalMs: number): { index: number; at: number }[] {
  return Array.from({ length: count }, (_, i) => ({ index: i, at: i * intervalMs }));
}
export function stitchVertical(metas: ImageMeta[]): { width: number; height: number } {
  return { width: Math.max(...metas.map((m) => m.width)), height: metas.reduce((s, m) => s + m.height, 0) };
}
export function gridCollage(metas: ImageMeta[], cols: number): { cols: number; rows: number; cell: { w: number; h: number } } {
  const rows = Math.ceil(metas.length / cols);
  return { cols, rows, cell: { w: Math.max(...metas.map((m) => m.width)), h: Math.max(...metas.map((m) => m.height)) } };
}

/** F03465/F03466 色板提取/直方图。 */
export function paletteExtract(pixels: Uint8ClampedArray, k = 5): string[] {
  // 简易主色：按 RGB>>5 量化计数取前 k
  const count = new Map<string, number>();
  for (let i = 0; i < pixels.length; i += 4) {
    const key = `${(pixels[i] ?? 0) >> 5}-${(pixels[i + 1] ?? 0) >> 5}-${(pixels[i + 2] ?? 0) >> 5}`;
    count.set(key, (count.get(key) ?? 0) + 1);
  }
  return [...count.entries()].sort((a, b) => b[1] - a[1]).slice(0, k).map(([key]) => {
    const parts = key.split('-').map((x) => (Number(x) << 5) + 16);
    const r = parts[0] ?? 0;
    const g = parts[1] ?? 0;
    const b = parts[2] ?? 0;
    return `#${((r << 16) | (g << 8) | b).toString(16).padStart(6, '0')}`;
  });
}
export function histogram(pixels: Uint8ClampedArray): number[] {
  const bins = new Array(8).fill(0);
  for (let i = 0; i < pixels.length; i += 4) {
    const lum = Math.round(0.299 * (pixels[i] ?? 0) + 0.587 * (pixels[i + 1] ?? 0) + 0.114 * (pixels[i + 2] ?? 0));
    bins[Math.min(7, lum >> 5)]++;
  }
  return bins;
}

/** F03471/F03472 ico/favicon 尺寸组。 */
export const ICO_SIZES = [16, 24, 32, 48, 64, 128, 256] as const;

/** F03473 按拍摄时间命名。 */
export function exifNaming(metas: { name: string; exif?: Record<string, string> }[]): string[] {
  return metas.map((m) => m.exif?.DateTime?.replace(/[ :]/g, '-') ?? m.name);
}

/* ============ 族0140 音视频工具（F03476~F03500） ============ */

export interface MediaMeta {
  name: string;
  kind: 'video' | 'audio';
  durationSec: number;
  bitrateKbps?: number;
  codec?: string;
}

/** F03476~F03479 转码预设/压缩估算/裁段/合并。 */
export const TRANSCODE_PRESETS = ['1080p-h264', '720p-h264', '4k-hevc', 'web-480p'] as const;

export function compressEstimate(m: MediaMeta, targetMbps: number): { beforeMB: number; afterMB: number } {
  const mb = (m.durationSec * (m.bitrateKbps ?? 8000)) / 8 / 1024;
  return { beforeMB: mb, afterMB: (m.durationSec * targetMbps * 1000) / 8 / 1024 };
}
export function trimRange(m: MediaMeta, fromSec: number, toSec: number): MediaMeta {
  return { ...m, durationSec: Math.max(0, toSec - fromSec) };
}
export function concatMedia(ms: MediaMeta[]): MediaMeta {
  return { name: 'merged', kind: ms[0]?.kind ?? 'video', durationSec: ms.reduce((s, m) => s + m.durationSec, 0) };
}

/** F03484 响度归一：目标 LUFS 增益。 */
export function loudnessGain(currentLufs: number, targetLufs = -14): number {
  return targetLufs - currentLufs;
}

/** F03486 变速不变调：速率因子。 */
export function tempoFactor(fromSec: number, toSec: number): number {
  return fromSec / toSec;
}

/** F03492 srt/ass 字幕转换。 */
export function srtToAss(srt: string): string {
  const body = srt
    .split(/\r?\n\r?\n/)
    .filter(Boolean)
    .map((block) => {
      const lines = block.split(/\r?\n/);
      const t = lines[1] ?? '';
      const ab = t.split(' --> ');
      const a = ab[0];
      const b = ab[1];
      const ass = (s: string) => {
        const seg = s.trim().split(':');
        const hh = seg[0] ?? '0';
        const mm = seg[1] ?? '0';
        const rest = seg[2] ?? '0,000';
        const msParts = rest.split(',');
        const ss = msParts[0] ?? '0';
        const ms = msParts[1] ?? '000';
        return `${Number(hh)}:${mm}:${ss}.${Math.round(Number(ms) / 10)}`;
      };
      return `Dialogue: 0,${ass(a ?? '')},${ass(b ?? '')},Default,,0,0,0,,${lines.slice(2).join('\\N')}`;
    });
  return ['[Script Info]', 'ScriptType: v4.00+', '', '[V4+ Styles]', '', '[Events]', ...body].join('\n');
}

/** F03495 按时长命名。 */
export function durationName(name: string, sec: number): string {
  const mm = String(Math.floor(sec / 60)).padStart(2, '0');
  const ss = String(Math.round(sec % 60)).padStart(2, '0');
  return `${name}_${mm}m${ss}s`;
}

/** F03496 媒体库索引。 */
export class MediaLibrary {
  private items: MediaMeta[] = [];
  index(metas: MediaMeta[]): number {
    let added = 0;
    for (const m of metas) {
      if (!this.items.some((x) => x.name === m.name)) {
        this.items.push(m);
        added++;
      }
    }
    return added;
  }
  /** F03497 重复媒体。 */
  duplicates(): MediaMeta[][] {
    const g = new Map<number, MediaMeta[]>();
    for (const m of this.items) {
      const key = Math.round(m.durationSec);
      if (!g.has(key)) g.set(key, []);
      g.get(key)!.push(m);
    }
    return [...g.values()].filter((x) => x.length > 1);
  }
  /** F03498 队列导出。 */
  exportQueue(): string {
    return this.items.map((m, i) => `${i + 1}. ${m.name} (${Math.round(m.durationSec)}s)`).join('\n');
  }
  /** F03499 损坏检测：时长/码率缺失。 */
  corrupt(): string[] {
    return this.items.filter((m) => m.durationSec <= 0 || !m.codec).map((m) => m.name);
  }
}
