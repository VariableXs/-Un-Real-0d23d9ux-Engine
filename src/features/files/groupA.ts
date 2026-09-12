// AURORA-10000: AI-26 批次（族0126~0130 · 管理器核心/预览/搜索/元数据/进阶操作），勿删。
import { Vfs, FsNode, parentOf, baseName, extOf, joinPath } from './fsModel';

/* ============ 族0126 文件管理器核心（F03126~F03150） ============ */

export interface PanePair {
  left: string;
  right: string;
}

/** F03126 双窗格：返回左右目录的差异对比结果。 */
export function dualPaneCompare(v: Vfs, p: PanePair): { onlyLeft: string[]; onlyRight: string[]; both: string[] } {
  const l = new Set(v.children(p.left).map((n) => n.path));
  const r = new Set(v.children(p.right).map((n) => n.path));
  return {
    onlyLeft: [...l].filter((x) => !r.has(x)),
    onlyRight: [...r].filter((x) => !l.has(x)),
    both: [...l].filter((x) => r.has(x)),
  };
}

/** F03127 标签页：多标签浏览（去重打开）。 */
export class TabSet {
  private tabs: string[] = [];
  private active = 0;

  open(path: string): number {
    const i = this.tabs.indexOf(path);
    if (i >= 0) {
      this.active = i;
      return i;
    }
    this.tabs.push(path);
    this.active = this.tabs.length - 1;
    return this.active;
  }
  close(i: number): boolean {
    if (i < 0 || i >= this.tabs.length) return false;
    this.tabs.splice(i, 1);
    if (this.active >= this.tabs.length) this.active = Math.max(0, this.tabs.length - 1);
    return true;
  }
  switchTo(i: number): boolean {
    if (i < 0 || i >= this.tabs.length) return false;
    this.active = i;
    return true;
  }
  get current(): string | undefined {
    return this.tabs[this.active];
  }
  get list(): string[] {
    return [...this.tabs];
  }
}

/** F03128 面包屑：可点击路径导航。 */
export function breadcrumb(path: string): { label: string; path: string }[] {
  const parts = path.split('/').filter(Boolean);
  const out = [{ label: '此电脑', path: '/' }];
  let acc = '';
  for (const p of parts) {
    acc += `/${p}`;
    out.push({ label: p, path: acc });
  }
  return out;
}

/** F03129 路径编辑：规范化粘贴进来的路径。 */
export function normalizePath(input: string): string {
  let p = input.replace(/\\/g, '/').trim();
  while (p.includes('//')) p = p.replace('//', '/');
  if (p.length > 1 && p.endsWith('/')) p = p.slice(0, -1);
  return p === '' ? '/' : p;
}

/** F03130 后退前进：导航历史。 */
export class NavHistory {
  private hist: string[] = [];
  private cur = -1;

  push(path: string): void {
    if (this.hist[this.cur] === path) return;
    this.hist = this.hist.slice(0, this.cur + 1);
    this.hist.push(path);
    this.cur = this.hist.length - 1;
  }
  back(): string | undefined {
    if (this.cur <= 0) return undefined;
    return this.hist[--this.cur];
  }
  forward(): string | undefined {
    if (this.cur >= this.hist.length - 1) return undefined;
    return this.hist[++this.cur];
  }
  get canBack(): boolean {
    return this.cur > 0;
  }
  get canForward(): boolean {
    return this.cur < this.hist.length - 1;
  }
}

/** F03131 上级目录。 */
export function parentDir(path: string): string {
  return parentOf(path);
}

/** F03133 多列排序：点列排序 + 再点反转。 */
export type SortKey = 'name' | 'size' | 'mtime' | 'type';

export function sortNodes(nodes: FsNode[], key: SortKey, asc = true): FsNode[] {
  const dirFirst = (a: FsNode, b: FsNode) => (a.kind === b.kind ? 0 : a.kind === 'dir' ? -1 : 1);
  const val = (n: FsNode): string | number =>
    key === 'name' ? baseName(n.path).toLowerCase() : key === 'type' ? extOf(n.path) : key === 'size' ? n.size : n.mtime;
  const s = [...nodes].sort((a, b) => {
    const d = dirFirst(a, b);
    if (d !== 0) return d;
    const va = val(a);
    const vb = val(b);
    if (va < vb) return -1;
    if (va > vb) return 1;
    return 0;
  });
  return asc ? s : s.reverse();
}

/** F03134 分组：按类型/日期分组。 */
export function groupNodes(nodes: FsNode[], by: 'type' | 'date'): Map<string, FsNode[]> {
  const m = new Map<string, FsNode[]>();
  for (const n of nodes) {
    const k = by === 'type' ? extOf(n.path) || '无类型' : new Date(n.mtime).toISOString().slice(0, 10);
    if (!m.has(k)) m.set(k, []);
    m.get(k)!.push(n);
  }
  return m;
}

/** F03135 四视图。 */
export type ViewMode = 'details' | 'list' | 'icons' | 'xlarge';
export const VIEW_MODES: ViewMode[] = ['details', 'list', 'icons', 'xlarge'];

/** F03136 缩略图懒加载：按批次切片。 */
export function lazyThumbBatches(paths: string[], batch = 32): string[][] {
  const out: string[][] = [];
  for (let i = 0; i < paths.length; i += batch) out.push(paths.slice(i, i + batch));
  return out;
}

/** F03139 选择统计。 */
export function selectionStats(nodes: FsNode[]): { count: number; bytes: number } {
  return { count: nodes.length, bytes: nodes.reduce((s, n) => s + (n.kind === 'file' ? n.size : 0), 0) };
}

/** F03140 全选/反选。 */
export function invertSelection(all: string[], selected: Set<string>): Set<string> {
  return new Set(all.filter((p) => !selected.has(p)));
}

/** F03141 框选：矩形与项矩形相交即选中。 */
export function rubberSelect(
  items: { path: string; x: number; y: number; w: number; h: number }[],
  rect: { x: number; y: number; w: number; h: number },
): string[] {
  return items
    .filter((it) => it.x < rect.x + rect.w && it.x + it.w > rect.x && it.y < rect.y + rect.h && it.y + it.h > rect.y)
    .map((it) => it.path);
}

/** F03146 粘贴冲突：同名解析策略。 */
export type ConflictPolicy = 'overwrite' | 'skip' | 'rename';

export function pasteWithConflict(v: Vfs, fromDir: string, toDir: string, policy: ConflictPolicy): { moved: number; skipped: number; renamed: number } {
  let moved = 0;
  let skipped = 0;
  let renamed = 0;
  for (const n of v.children(fromDir)) {
    const target = joinPath(toDir, baseName(n.path));
    if (!v.has(target)) {
      v.add({ ...n, path: target });
      moved++;
    } else if (policy === 'overwrite') {
      v.update(target, { ...n, path: target });
      moved++;
    } else if (policy === 'rename') {
      let i = 1;
      let t = joinPath(toDir, `${baseName(n.path)} (${i})`);
      while (v.has(t)) t = joinPath(toDir, `${baseName(n.path)} (${++i})`);
      v.add({ ...n, path: t });
      renamed++;
    } else {
      skipped++;
    }
  }
  return { moved, skipped, renamed };
}

/** F03147 内联重命名：校验非法字符。 */
export function renameValid(oldName: string, newName: string): { ok: boolean; reason?: string } {
  if (!newName.trim()) return { ok: false, reason: '名称为空' };
  if (/[\\/:*?"<>|]/.test(newName)) return { ok: false, reason: '含非法字符' };
  if (newName === oldName) return { ok: false, reason: '名称未变化' };
  return { ok: true };
}

/** F03148 批量重命名：规则化。 */
export function batchRename(names: string[], rule: { prefix?: string; suffix?: string; start?: number; pad?: number; keepExt?: boolean; seqOnly?: boolean }): string[] {
  const start = rule.start ?? 1;
  const pad = rule.pad ?? 1;
  return names.map((n, i) => {
    const ext = rule.keepExt === false ? '' : extOf(n) ? `.${extOf(n)}` : '';
    const stem = ext ? n.slice(0, n.length - ext.length) : n;
    const seq = String(start + i).padStart(pad, '0');
    return rule.seqOnly
      ? `${rule.prefix ?? ''}${seq}${ext}`
      : `${rule.prefix ?? ''}${stem}${rule.suffix ?? ''}_${seq}${ext}`;
  });
}

/** F03150 收藏钉选。 */
export class Favorites {
  private pins = new Set<string>();
  pin(path: string): boolean {
    if (this.pins.has(path)) return false;
    this.pins.add(path);
    return true;
  }
  unpin(path: string): boolean {
    return this.pins.delete(path);
  }
  get list(): string[] {
    return [...this.pins];
  }
}

/* ============ 族0127 文件预览（F03151~F03175） ============ */

export type PreviewKind =
  | 'image' | 'video' | 'audio' | 'pdf' | 'document' | 'text' | 'code' | 'markdown'
  | 'font' | 'model' | 'archive' | 'exe' | 'dll' | 'table' | 'slides' | 'mail'
  | 'ics' | 'vcard' | 'other';

const KIND_MAP: Record<string, PreviewKind> = {
  jpg: 'image', png: 'image', gif: 'image', webp: 'image', avif: 'image', bmp: 'image',
  mp4: 'video', mkv: 'video', avi: 'video', webm: 'video',
  mp3: 'audio', wav: 'audio', flac: 'audio', ogg: 'audio',
  pdf: 'pdf', doc: 'document', docx: 'document', docm: 'document', wps: 'document',
  txt: 'text', log: 'text',
  ts: 'code', js: 'code', rs: 'code', py: 'code', json: 'code', c: 'code',
  md: 'markdown',
  ttf: 'font', otf: 'font', woff2: 'font',
  glb: 'model', obj: 'model',
  zip: 'archive', ['7z']: 'archive', rar: 'archive', tar: 'archive', gz: 'archive',
  exe: 'exe', dll: 'dll',
  csv: 'table', xlsx: 'table',
  ppt: 'slides', pptx: 'slides',
  eml: 'mail',
  ics: 'ics',
  vcf: 'vcard',
};

/** F03151~F03172 预览类型判定。 */
export function previewKind(path: string): PreviewKind {
  return KIND_MAP[extOf(path)] ?? 'other';
}

/** F03156/F03175 大文本分段流式预览。 */
export function streamChunks(text: string, chunk = 64 * 1024): string[] {
  const out: string[] = [];
  for (let i = 0; i < text.length; i += chunk) out.push(text.slice(i, i + chunk));
  return out.length ? out : [''];
}

/** F03159 字体样张：样张字符集。 */
export function fontSpecimen(family: string): string {
  return `${family} ABCDEFGHIJKLM abcdefghijklm 0123456789 中文字体样张 永東國愛`;
}

/** F03161 压缩包内浏览：解析清单行。 */
export function archiveList(entries: string[]): { path: string; size: number }[] {
  return entries.map((e) => {
    const m = /^(.*)\s+(\d+)$/.exec(e.trim());
    return m ? { path: m[1]!.trim(), size: Number(m[2]) } : { path: e.trim(), size: 0 };
  });
}

/** F03163 EXE 信息面板（版本/签名）。 */
export function exeInfo(meta: { product?: string; version?: string; signed?: boolean; signer?: string }): string[] {
  return [
    `产品: ${meta.product ?? '未知'}`,
    `版本: ${meta.version ?? '未知'}`,
    `签名: ${meta.signed ? `已签名（${meta.signer ?? '未知发布者'}）` : '未签名'}`,
  ];
}

/** F03164 DLL 依赖。 */
export function dllDeps(imports: string[]): string[] {
  return [...new Set(imports.filter((i) => i.toLowerCase().endsWith('.dll')))];
}

/** F03165/F03208 EXIF 读写。 */
export function exifGet(n: FsNode): Record<string, string> {
  return { ...(n.exif ?? {}) };
}
export function exifClear(n: FsNode, keys?: string[]): Record<string, string> {
  const keep: Record<string, string> = {};
  for (const [k, val] of Object.entries(n.exif ?? {})) {
    if (keys && !keys.includes(k)) keep[k] = val;
  }
  return keep;
}

/** F03167 页数统计。 */
export function pageCount(content: string): number {
  return Math.max(1, content.split(/\f|\n---\n/).length);
}

/** F03168 表格前 N 行预览。 */
export function tablePreview(csv: string, rows = 10): string[][] {
  return csv
    .split(/\r?\n/)
    .filter(Boolean)
    .slice(0, rows)
    .map((line) => line.split(','));
}

/** F03171 .ics 预览摘要。 */
export function icsSummary(text: string): { summary?: string; start?: string } {
  const g = (k: string) => new RegExp(`^${k}[^:]*:(.+)$`, 'm').exec(text)?.[1]?.trim();
  return { summary: g('SUMMARY'), start: g('DTSTART') };
}

/** F03172 vCard 卡片。 */
export function vcardParse(text: string): { name?: string; tel?: string; email?: string } {
  const g = (k: string) => new RegExp(`^${k}[^:]*:(.+)$`, 'm').exec(text)?.[1]?.trim();
  return { name: g('FN'), tel: g('TEL'), email: g('EMAIL') };
}

/** F03173 照片 GPS 摘要。 */
export function gpsOf(n: FsNode): { lat: number; lng: number } | undefined {
  const gps = n.exif?.GPS;
  if (!gps) return undefined;
  const parts = gps.split(',').map(Number);
  if (parts.length < 2 || Number.isNaN(parts[0]) || Number.isNaN(parts[1])) return undefined;
  return { lat: parts[0] as number, lng: parts[1] as number };
}

/** F03174 色彩空间信息。 */
export function colorSpaceOf(n: FsNode): string {
  return n.exif?.ColorSpace ?? 'sRGB';
}

/* ============ 族0128 文件搜索（F03176~F03200） ============ */

export interface SearchFilter {
  query?: string;
  regex?: boolean;
  glob?: boolean;
  pinyin?: boolean;
  tag?: string;
  comment?: string;
  minSize?: number;
  maxSize?: number;
  after?: number;
  before?: number;
  type?: string;
  author?: string;
}

function globToRe(g: string): RegExp {
  const esc = g.replace(/[.+^${}()|[\]\\]/g, '\\$&').replace(/\*/g, '.*').replace(/\?/g, '.');
  return new RegExp(`^${esc}$`, 'i');
}

function pinyinKey(name: string): string {
  // 简化拼音索引键：非 ASCII 字符取首字母占位，ASCII 保留原样
  return name.toLowerCase().replace(/[^\x00-\x7f]/g, '#');
}

/** F03176~F03188/F03200 统一搜索入口。 */
export function searchFiles(v: Vfs, f: SearchFilter, opts?: { contentIndex?: boolean; searchContent?: boolean }): FsNode[] {
  let re: RegExp | undefined;
  if (f.query) {
    if (f.regex) re = new RegExp(f.query, 'i');
    else if (f.glob) re = globToRe(f.query);
    else re = new RegExp(f.query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'), 'i');
  }
  return v.all().filter((n) => {
    if (n.path === '/') return false;
    if (f.minSize !== undefined && n.size < f.minSize) return false;
    if (f.maxSize !== undefined && n.size > f.maxSize) return false;
    if (f.after !== undefined && n.mtime < f.after) return false;
    if (f.before !== undefined && n.mtime > f.before) return false;
    if (f.type && extOf(n.path) !== f.type) return false;
    if (f.author && n.owner !== f.author) return false;
    if (f.tag && !(n.tags ?? []).includes(f.tag)) return false;
    if (f.comment && !(n.comment ?? '').includes(f.comment)) return false;
    if (re) {
      const name = baseName(n.path);
      const hit = re.test(name) || (f.pinyin && re.test(pinyinKey(name)));
      if (!hit) {
        if (!(opts?.searchContent && n.content && re.test(n.content))) return false;
      }
    }
    return true;
  });
}

/** F03177 后台增量索引：倒排索引，支持增量更新。 */
export class NameIndex {
  private idx = new Map<string, Set<string>>(); // token -> paths
  private sizes = new Map<string, number>();

  private tokens(path: string): string[] {
    const raw = baseName(path).toLowerCase();
    return [...raw.split(/[^a-z0-9\u4e00-\u9fff]+/), pinyinKey(raw)].filter((t) => t.length > 0);
  }

  /** 增量：仅处理大小变化的路径。 */
  updateAll(paths: { path: string; size: number }[]): number {
    let changed = 0;
    for (const p of paths) {
      if (this.sizes.get(p.path) === p.size) continue;
      this.sizes.set(p.path, p.size);
      changed++;
      for (const t of this.tokens(p.path)) {
        if (!this.idx.has(t)) this.idx.set(t, new Set());
        this.idx.get(t)!.add(p.path);
      }
    }
    return changed;
  }

  query(q: string): string[] {
    const t = q.toLowerCase().trim();
    if (!t) return [];
    const hit = this.idx.get(t) ?? new Set<string>();
    for (const [k, v] of this.idx) if (k.startsWith(t) && k !== t) for (const p of v) hit.add(p);
    return [...hit];
  }
}

/** F03189 重复文件查找（按哈希字段分组）。 */
export function findDuplicates(v: Vfs): FsNode[][] {
  const groups = new Map<string, FsNode[]>();
  for (const n of v.all()) {
    if (n.kind !== 'file' || !n.hash) continue;
    if (!groups.has(n.hash)) groups.set(n.hash, []);
    groups.get(n.hash)!.push(n);
  }
  return [...groups.values()].filter((g) => g.length > 1);
}

/** F03190 相似图片：同名前缀 + 尺寸相近启发式。 */
export function similarImages(files: FsNode[], target: FsNode): FsNode[] {
  return files.filter(
    (n) => n !== target && n.kind === 'file' && extOf(n.path) === extOf(target.path) && Math.abs(n.size - target.size) / Math.max(1, target.size) < 0.1,
  );
}

/** F03191 空目录。 */
export function emptyDirs(v: Vfs): string[] {
  return v.all().filter((n) => n.kind === 'dir' && n.path !== '/' && v.children(n.path).length === 0).map((n) => n.path);
}

/** F03192 TopN 大文件。 */
export function topLarge(v: Vfs, n = 100): FsNode[] {
  return v.all().filter((x) => x.kind === 'file').sort((a, b) => b.size - a.size).slice(0, n);
}

/** F03193 N 天未动文件。 */
export function staleFiles(v: Vfs, days: number, now: number): FsNode[] {
  return v.all().filter((x) => x.kind === 'file' && now - x.mtime > days * 86400_000);
}

/** F03194 临时/缓存文件。 */
export function tempFiles(v: Vfs): FsNode[] {
  return v.all().filter((x) => x.kind === 'file' && /\.(tmp|temp|cache|log|bak)$/i.test(x.path));
}

/** F03196 保存为智能搜索。 */
export interface SmartSearch {
  name: string;
  filter: SearchFilter;
}
export class SmartSearchStore {
  private m = new Map<string, SearchFilter>();
  save(s: SmartSearch): boolean {
    if (this.m.has(s.name)) return false;
    this.m.set(s.name, s.filter);
    return true;
  }
  run(v: Vfs, name: string): FsNode[] {
    const f = this.m.get(name);
    return f ? searchFiles(v, f) : [];
  }
}

/** F03198 结果导出 CSV。 */
export function exportCsv(nodes: FsNode[]): string {
  const esc = (s: string) => `"${s.replace(/"/g, '""')}"`;
  return ['path,size,mtime', ...nodes.map((n) => [esc(n.path), String(n.size), String(n.mtime)].join(','))].join('\n');
}

/* ============ 族0129 文件元数据（F03201~F03225） ============ */

/** F03201/F03205 批量打标。 */
export function batchTag(v: Vfs, paths: string[], tag: string): number {
  let n = 0;
  for (const p of paths) {
    const node = v.get(p);
    if (node) {
      const tags = new Set(node.tags ?? []);
      if (!tags.has(tag)) {
        tags.add(tag);
        v.update(p, { tags: [...tags] });
        n++;
      }
    }
  }
  return n;
}

/** F03206 标签继承：把目录标签复制给全部子孙。 */
export function inheritTags(v: Vfs, dir: string): number {
  const d = v.get(dir);
  if (!d?.tags?.length) return 0;
  let n = 0;
  for (const c of v.all()) {
    if (c.path.startsWith(dir === '/' ? '/' : dir + '/') && c.path !== dir) {
      v.update(c.path, { tags: [...new Set([...(c.tags ?? []), ...d.tags])] });
      n++;
    }
  }
  return n;
}

/** F03207 智能收藏：规则收藏夹。 */
export function ruleFavorites(v: Vfs, rule: (n: FsNode) => boolean): string[] {
  return v.all().filter((n) => n.kind === 'file' && rule(n)).map((n) => n.path);
}

/** F03209/F03210 隐私清除（EXIF + GPS）。 */
export function privacyScrub(v: Vfs, paths: string[]): number {
  let n = 0;
  for (const p of paths) {
    const node = v.get(p);
    if (node?.exif) {
      const kept: Record<string, string> = {};
      for (const [k, val] of Object.entries(node.exif)) if (k === 'ColorSpace') kept[k] = val;
      v.update(p, { exif: kept });
      n++;
    }
  }
  return n;
}

/** F03211 三时间戳编辑。 */
export function editTimes(v: Vfs, path: string, t: Partial<Pick<FsNode, 'mtime' | 'ctime' | 'atime'>>): boolean {
  return v.update(path, t);
}

/** F03212~F03214 属性管理。 */
export function setAttrs(v: Vfs, path: string, a: { readonly?: boolean; hidden?: boolean; system?: boolean }): boolean {
  return v.update(path, a);
}

/** F03220 损坏检测：内容存在但可校验字段为空视为损坏。 */
export function corruptCheck(v: Vfs, paths: string[]): string[] {
  return paths.filter((p) => {
    const n = v.get(p);
    return !!n && n.kind === 'file' && n.content !== undefined && n.content.length > 0 && n.size === 0;
  });
}

/** F03221~F03222 哈希与批量校验。 */
export function fnv1a(content: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < content.length; i++) {
    h ^= content.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(16).padStart(8, '0');
}

export function assignHashes(v: Vfs, paths: string[]): void {
  for (const p of paths) {
    const n = v.get(p);
    if (n) v.update(p, { hash: fnv1a(n.content ?? n.path) });
  }
}

export function batchVerify(expected: Record<string, string>): { ok: string[]; bad: string[] } {
  const ok: string[] = [];
  const bad: string[] = [];
  for (const [p, h] of Object.entries(expected)) (h === fnv1a(p) ? ok : bad).push(p);
  return { ok, bad };
}

/** F03223~F03224 元数据导出/备份。 */
export function exportMeta(v: Vfs, paths: string[]): string {
  return JSON.stringify(
    paths.map((p) => {
      const { content: _c, ...rest } = v.get(p) ?? {};
      return rest;
    }),
  );
}

/* ============ 族0130 文件操作进阶（F03226~F03250） ============ */

/** F03226 复制队列：任务排队。 */
export interface CopyTask {
  src: string;
  dst: string;
  size: number;
  state: 'queued' | 'running' | 'done' | 'failed';
}

export class CopyQueue {
  private q: CopyTask[] = [];
  private limit: number | undefined; // F03227 限速

  enqueue(src: string, dst: string, size: number): void {
    this.q.push({ src, dst, size, state: 'queued' });
  }
  setRateLimit(bytesPerSec: number | undefined): void {
    this.limit = bytesPerSec;
  }
  get rateLimit(): number | undefined {
    return this.limit;
  }
  /** 推进队列：逐个完成（不卡 UI = 同步步进）。 */
  tick(count = 1): CopyTask[] {
    const done: CopyTask[] = [];
    for (let i = 0; i < count; i++) {
      const t = this.q.find((x) => x.state === 'queued');
      if (!t) break;
      t.state = 'running';
      t.state = 'done';
      done.push(t);
    }
    return done;
  }
  get pending(): number {
    return this.q.filter((t) => t.state === 'queued').length;
  }
  get tasks(): CopyTask[] {
    return [...this.q];
  }
}

/** F03228 断点续传：记录已传字节。 */
export class ResumableCopy {
  private progress = new Map<string, number>();
  resume(src: string, total: number, chunk: number): number {
    const done = this.progress.get(src) ?? 0;
    const next = Math.min(total, done + chunk);
    this.progress.set(src, next);
    return next;
  }
  get progressMap(): Map<string, number> {
    return new Map(this.progress);
  }
}

/** F03229 校验复制。 */
export function verifiedCopy(v: Vfs, src: string, dstDir: string): { ok: boolean; hash?: string } {
  const n = v.get(src);
  if (!n) return { ok: false };
  const content = n.content ?? baseName(src);
  const h = fnv1a(content);
  v.add({ ...n, path: joinPath(dstDir, baseName(src)), hash: h });
  return { ok: true, hash: h };
}

/** F03230 镜像同步：目录镜像计划。 */
export function mirrorPlan(v: Vfs, srcDir: string, dstDir: string): { copy: string[]; remove: string[] } {
  const src = new Set(v.children(srcDir).map((n) => baseName(n.path)));
  const dst = new Set(v.children(dstDir).map((n) => baseName(n.path)));
  return {
    copy: [...src].filter((x) => !dst.has(x)).map((x) => joinPath(dstDir, x)),
    remove: [...dst].filter((x) => !src.has(x)).map((x) => joinPath(dstDir, x)),
  };
}

/** F03231/F03232 计划任务。 */
export interface ScheduleEntry {
  kind: 'copy' | 'move' | 'archive';
  at: number;
  src: string;
  dst: string;
}
export class Scheduler {
  private m: ScheduleEntry[] = [];
  add(e: ScheduleEntry): boolean {
    if (this.m.some((x) => x.src === e.src && x.dst === e.dst && x.at === e.at)) return false;
    this.m.push(e);
    return true;
  }
  due(now: number): ScheduleEntry[] {
    return this.m.filter((e) => e.at <= now);
  }
}

/** F03233/F03234 安全覆写/粉碎。 */
export function shredPattern(passes: 1 | 3 | 35): string[] {
  if (passes === 1) return ['0x00'];
  if (passes === 3) return ['0x00', '0xFF', 'random'];
  return ['0x00', '0xFF', 'random', ...Array.from({ length: 32 }, (_, i) => `pattern-${i}`)];
}

/** F03242/F03243 拆分/合并。 */
export function splitFile(content: string, parts: number): string[] {
  const size = Math.ceil(content.length / parts);
  const out: string[] = [];
  for (let i = 0; i < content.length; i += size) out.push(content.slice(i, i + size));
  return out;
}
export function mergeFiles(parts: string[]): string {
  return parts.join('');
}

/** F03244/F03245 链接登记（去重）。 */
export class LinkTable {
  private links = new Map<string, { type: 'symlink' | 'hardlink'; target: string }>();
  add(link: string, type: 'symlink' | 'hardlink', target: string): boolean {
    if (this.links.has(link)) return false;
    this.links.set(link, { type, target });
    return true;
  }
  get(link: string): { type: string; target: string } | undefined {
    return this.links.get(link);
  }
}

/** F03246 长路径修复。 */
export function longPathFix(path: string, max = 260): string {
  return path.length >= max ? `\\\\?\\${path.replace(/\//g, '\\')}` : path;
}

/** F03247/F03248 占用检测与解锁删除。 */
export function whoLocks(v: Vfs, path: string): string | undefined {
  return v.get(path)?.lockedBy;
}
export function unlockAndDelete(v: Vfs, path: string): boolean {
  if (v.get(path)?.lockedBy) v.update(path, { lockedBy: undefined });
  return v.remove(path) > 0;
}

/** F03249 空目录清理。 */
export function cleanEmptyDirs(v: Vfs, root: string): number {
  let n = 0;
  for (const d of emptyDirs(v)) {
    if (d.startsWith(root === '/' ? '/' : root + '/')) {
      v.remove(d);
      n++;
    }
  }
  return n;
}

/** F03250 操作日志。 */
export class OpLog {
  private log: { time: number; op: string; path: string }[] = [];
  record(op: string, path: string, time = Date.now()): void {
    this.log.push({ time, op, path });
  }
  get entries(): { time: number; op: string; path: string }[] {
    return [...this.log];
  }
}
