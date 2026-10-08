// AURORA-10000: AI-27 批次（族0131~0135 · 回收恢复/磁盘空间/同步备份/数据流/组织哲学），勿删。
import { Vfs, FsNode, baseName, extOf, joinPath, parentOf } from './fsModel';
import { fnv1a } from './groupA';

/* ============ 族0131 回收站与恢复（F03251~F03275） ============ */

export interface RecycleEntry {
  path: string; // 原路径
  deletedAt: number;
  size: number;
  content?: string;
  hash?: string;
  locked?: boolean; // 黑名单二次确认
}

/** 每盘独立回收站（F03272）：盘符 -> 条目表。 */
export class RecycleBin {
  private bins = new Map<string, RecycleEntry[]>();
  private blacklist = new Set<string>();
  private capacityWarnBytes = 1024 * 1024 * 1024;

  driveOf(path: string): string {
    const m = /^\/?([A-Za-z]):/.exec(path.replace(/\\/g, '/'));
    return m ? m[1]!.toUpperCase() : 'C';
  }

  delete(v: Vfs, path: string): RecycleEntry | undefined {
    const n = v.get(path);
    if (!n) return undefined;
    const entry: RecycleEntry = { path, deletedAt: Date.now(), size: n.size, content: n.content, hash: n.hash, locked: this.blacklist.has(path) };
    const d = this.driveOf(path);
    if (!this.bins.has(d)) this.bins.set(d, []);
    this.bins.get(d)!.push(entry);
    v.remove(path);
    return entry;
  }

  /** F03254 还原（单/批）。 */
  restore(v: Vfs, paths: string[] | 'all'): number {
    let n = 0;
    for (const [, list] of this.bins) {
      for (let i = list.length - 1; i >= 0; i--) {
        const e = list[i]!;
        if (paths !== 'all' && !paths.includes(e.path)) continue;
        const dir = parentOf(e.path);
        if (v.has(dir) && !v.has(e.path)) {
          v.add({ path: e.path, kind: 'file', size: e.size, content: e.content, hash: e.hash });
          list.splice(i, 1);
          n++;
        }
      }
    }
    return n;
  }

  /** F03251 回收站内搜索。 */
  search(q: string): RecycleEntry[] {
    return [...this.bins.values()].flat().filter((e) => e.path.toLowerCase().includes(q.toLowerCase()));
  }

  /** F03253 按删除时间分组。 */
  groupByTime(): Map<string, RecycleEntry[]> {
    const m = new Map<string, RecycleEntry[]>();
    for (const e of [...this.bins.values()].flat()) {
      const k = new Date(e.deletedAt).toISOString().slice(0, 10);
      if (!m.has(k)) m.set(k, []);
      m.get(k)!.push(e);
    }
    return m;
  }

  /** F03256 永久删除二次确认。 */
  purge(paths: string[], confirm: boolean): number {
    if (!confirm) return 0;
    let n = 0;
    for (const [, list] of this.bins) {
      for (let i = list.length - 1; i >= 0; i--) {
        const e = list[i]!;
        if (paths.includes(e.path)) {
          list.splice(i, 1);
          n++;
        }
      }
    }
    return n;
  }

  /** F03257 定时清空计划（返回到期的清理项）。 */
  autoEmpty(cutoff: number): RecycleEntry[] {
    const out: RecycleEntry[] = [];
    for (const [, list] of this.bins) {
      for (let i = list.length - 1; i >= 0; i--) {
        const e = list[i]!;
        if (e.deletedAt < cutoff) out.push(list.splice(i, 1)[0]!);
      }
    }
    return out;
  }

  /** F03258 容量提醒。 */
  overCapacity(): boolean {
    return [...this.bins.values()].flat().reduce((s, e) => s + e.size, 0) > this.capacityWarnBytes;
  }

  /** F03259 类型筛选。 */
  filterType(ext: string): RecycleEntry[] {
    return [...this.bins.values()].flat().filter((e) => extOf(e.path) === ext);
  }

  /** F03260 省空间统计。 */
  stats(): { entries: number; bytes: number } {
    const all = [...this.bins.values()].flat();
    return { entries: all.length, bytes: all.reduce((s, e) => s + e.size, 0) };
  }

  /** F03270 删除黑名单。 */
  addBlacklist(path: string): boolean {
    if (this.blacklist.has(path)) return false;
    this.blacklist.add(path);
    return true;
  }

  /** F03271 回收站加密开关。 */
  encryption = false;
  setEncryption(on: boolean): void {
    this.encryption = on;
  }

  /** F03273 回收站位置迁移。 */
  relocateDrive(path: string, toDrive: string): boolean {
    const d = this.driveOf(path);
    const list = this.bins.get(d);
    if (!list) return false;
    const i = list.findIndex((e) => e.path === path);
    if (i < 0) return false;
    const [e] = list.splice(i, 1);
    if (!this.bins.has(toDrive)) this.bins.set(toDrive, []);
    this.bins.get(toDrive)!.push(e!);
    return true;
  }
}

/** F03262~F03265 文件版本快照体系。 */
export class VersionStore {
  private snaps = new Map<string, { time: number; size: number; note?: string; content?: string }[]>();

  /** 登记新版本（去重：内容相同不重复登记）。 */
  snapshot(path: string, content: string, time: number, note?: string): boolean {
    if (!this.snaps.has(path)) this.snaps.set(path, []);
    const list = this.snaps.get(path)!;
    const h = fnv1a(content);
    if (list.some((s) => fnv1a(s.content ?? '') === h)) return false;
    list.push({ time, size: content.length, note, content });
    return true;
  }

  timeline(path: string): { time: number; size: number; note?: string }[] {
    return [...(this.snaps.get(path) ?? [])].sort((a, b) => a.time - b.time);
  }

  /** F03263 两版对比。 */
  diff(path: string, i: number, j: number): 'same' | 'different' | 'missing' {
    const list = this.snaps.get(path);
    if (!list || !list[i] || !list[j]) return 'missing';
    return fnv1a(list[i].content ?? '') === fnv1a(list[j].content ?? '') ? 'same' : 'different';
  }

  /** F03264 回滚到版本。 */
  restore(v: Vfs, path: string, time: number): boolean {
    const s = this.snaps.get(path)?.find((x) => x.time === time);
    if (!s || !v.has(path)) return false;
    v.update(path, { content: s.content, size: s.size });
    return true;
  }
}

/* ============ 族0132 磁盘与空间（F03276~F03300） ============ */

export interface SizeItem {
  name: string;
  size: number;
  children?: SizeItem[];
}

/** F03276 矩形树图：slice-dice 布局（面积严格守恒）。 */
export function treemap(items: SizeItem[], x: number, y: number, w: number, h: number): { name: string; x: number; y: number; w: number; h: number }[] {
  const out: { name: string; x: number; y: number; w: number; h: number }[] = [];
  let cx = x;
  let cy = y;
  let rw = w;
  let rh = h;
  const rest = [...items];
  let remaining = rest.reduce((s, i) => s + i.size, 0) || 1;
  while (rest.length) {
    const it = rest.shift()!;
    const frac = it.size / remaining;
    remaining = Math.max(0, remaining - it.size);
    if (rw >= rh) {
      const cw = rw * frac;
      out.push({ name: it.name, x: cx, y: cy, w: cw, h: rh });
      cx += cw;
      rw -= cw;
    } else {
      const ch = rh * frac;
      out.push({ name: it.name, x: cx, y: cy, w: rw, h: ch });
      cy += ch;
      rh -= ch;
    }
  }
  return out;
}

/** F03277 旭日图数据。 */
export function sunburst(item: SizeItem, depth = 0, start = 0, span = 360): { name: string; depth: number; start: number; span: number }[] {
  const out: { name: string; depth: number; start: number; span: number }[] = [{ name: item.name, depth, start, span }];
  const total = item.children?.reduce((s, c) => s + c.size, 0) || 0;
  if (item.children && total > 0) {
    let a = start;
    for (const c of item.children) {
      const s = (c.size / total) * span;
      out.push(...sunburst(c, depth + 1, a, s));
      a += s;
    }
  }
  return out;
}

/** F03278~F03280 占比/排行/时间分布。 */
export function byType(files: FsNode[]): Map<string, number> {
  const m = new Map<string, number>();
  for (const n of files) {
    const k = extOf(n.path) || '无类型';
    m.set(k, (m.get(k) ?? 0) + n.size);
  }
  return m;
}
export function dirRanking(v: Vfs, root: string): { path: string; bytes: number }[] {
  return v
    .all()
    .filter((n) => n.kind === 'dir' && n.path.startsWith(root === '/' ? '/' : root + '/'))
    .map((d) => ({ path: d.path, bytes: v.children(d.path).reduce((s, c) => s + c.size, 0) }))
    .sort((a, b) => b.bytes - a.bytes);
}
export function byTime(files: FsNode[], bucketMs = 86400_000): Map<number, number> {
  const m = new Map<number, number>();
  for (const n of files) {
    const k = Math.floor(n.mtime / bucketMs);
    m.set(k, (m.get(k) ?? 0) + n.size);
  }
  return m;
}

/** F03284/F03285 垃圾/临时扫描。 */
export function junkScan(v: Vfs): FsNode[] {
  return v.all().filter((n) => n.kind === 'file' && /\.(tmp|temp|log|cache|bak|old|dmp)$/i.test(n.path));
}

/** F03287 卸载残留：目录名匹配已卸载应用名单。 */
export function leftoverScan(v: Vfs, uninstalled: string[]): string[] {
  return v.all().filter((n) => n.kind === 'dir' && uninstalled.some((u) => baseName(n.path).toLowerCase().includes(u.toLowerCase()))).map((n) => n.path);
}

/** F03288 30 天空间趋势。 */
export function spaceTrend(samples: number[]): { day: number; bytes: number }[] {
  return samples.map((bytes, day) => ({ day, bytes }));
}

/** F03289 限额告警。 */
export function budgetAlert(used: number, quota: number): { level: 'ok' | 'warn' | 'over'; pct: number } {
  const pct = used / Math.max(1, quota);
  return { level: pct >= 1 ? 'over' : pct >= 0.8 ? 'warn' : 'ok', pct };
}

/** F03291~F03293 一键清理（白名单 + 预演 + 撤销）。 */
export function cleanPlan(v: Vfs, whitelist: string[]): string[] {
  return junkScan(v).filter((n) => !whitelist.some((w) => n.path.includes(w))).map((n) => n.path);
}
export function undoClean(v: Vfs, snapshot: FsNode[]): number {
  return snapshot.filter((n) => !v.has(n.path) && v.has(parentOf(n.path)) && v.add({ ...n })).length;
}

/* ============ 族0133 数据同步备份（F03301~F03325） ============ */

export interface BackupPlan {
  name: string;
  src: string;
  dst: string;
  every: 'day' | 'week';
  keep: number;
  encrypted: boolean;
  compressed: boolean;
  excludes: string[];
}

/** F03301 增量快照：仅收录自上次以来变化的文件。 */
export function incremental(v: Vfs, lastRun: number, excludes: string[] = []): FsNode[] {
  return v.all().filter((n) => n.kind === 'file' && n.mtime > lastRun && !excludes.some((e) => n.path.includes(e)));
}

/** F03303 保留策略：超出的最旧快照剔除。 */
export function retention(list: { time: number }[], keep: number): { time: number }[] {
  const sorted = [...list].sort((a, b) => b.time - a.time);
  return sorted.slice(0, keep);
}

/** F03314 双向同步：返回两侧差异与冲突。 */
export function twoWaySync(v: Vfs, aDir: string, bDir: string): { copyToB: string[]; copyToA: string[]; conflicts: string[] } {
  const a = v.children(aDir);
  const b = v.children(bDir);
  const bNames = new Set(b.map((n) => baseName(n.path)));
  const aNames = new Set(a.map((n) => baseName(n.path)));
  const conflicts: string[] = [];
  for (const na of a) {
    const nb = b.find((x) => baseName(x.path) === baseName(na.path));
    if (nb && nb.size !== na.size) conflicts.push(baseName(na.path));
  }
  return {
    copyToB: a.filter((n) => !bNames.has(baseName(n.path))).map((n) => n.path),
    copyToA: b.filter((n) => !aNames.has(baseName(n.path))).map((n) => n.path),
    conflicts,
  };
}

/** F03319 块级增量：变化块计算。 */
export function blockDiff(oldText: string, newText: string, block = 4096): { changedBlocks: number; totalBlocks: number } {
  const n = Math.max(Math.ceil(newText.length / block), 1);
  let changed = 0;
  for (let i = 0; i < n; i++) {
    if (oldText.slice(i * block, (i + 1) * block) !== newText.slice(i * block, (i + 1) * block)) changed++;
  }
  return { changedBlocks: changed, totalBlocks: n };
}

/** F03320 去重存储：按内容哈希存储块。 */
export class DedupStore {
  private blocks = new Map<string, string>();
  private saved = 0;

  put(text: string, block = 4096): number {
    for (let i = 0; i < text.length; i += block) {
      const b = text.slice(i, i + block);
      const h = fnv1a(b);
      if (this.blocks.has(h)) this.saved += b.length;
      else this.blocks.set(h, b);
    }
    return this.saved;
  }
  get dedupSaved(): number {
    return this.saved;
  }
}

/* ============ 族0134 剪贴与拖拽数据（F03326~F03350） ============ */

export type DragAction = 'copy' | 'move' | 'link';

/** F03327~F03329 动作提示与修饰键切换。 */
export function dragAction(modifier: 'none' | 'ctrl' | 'alt', sameDrive: boolean): DragAction {
  if (modifier === 'ctrl') return 'copy';
  if (modifier === 'alt') return 'link';
  return sameDrive ? 'move' : 'copy';
}

/** F03335 计数角标。 */
export function dragBadge(paths: string[]): number {
  return paths.length;
}

/** F03337 终端路径转换。 */
export function toTerminalPath(path: string): string {
  return `"${path.replace(/\//g, '\\')}"`;
}

/** F03339~F03341 剪贴文件列表 / 复制路径 / 路径粘贴成文件。 */
export class FileClipboard {
  private items: string[] = [];
  private op: DragAction = 'copy';

  set(paths: string[], op: DragAction): void {
    this.items = [...paths];
    this.op = op;
  }
  get list(): string[] {
    return [...this.items];
  }
  get action(): DragAction {
    return this.op;
  }
  copyPathsAsText(): string {
    return this.items.join('\n');
  }
  /** 把剪贴板里的文本路径粘贴成文件登记。 */
  pasteAsFiles(v: Vfs, dir: string): number {
    let n = 0;
    for (const p of this.items) {
      if (v.add({ path: joinPath(dir, baseName(p)), kind: 'file' })) n++;
    }
    return n;
  }
}

/** F03343 敏感拖出确认。 */
export function sensitiveDragConfirm(paths: string[], sensitiveDir: string, confirmed: boolean): { blocked: boolean; needsConfirm: boolean } {
  const touches = paths.some((p) => p.startsWith(sensitiveDir === '/' ? '/' : sensitiveDir + '/'));
  return { blocked: touches && !confirmed, needsConfirm: touches };
}

/* ============ 族0135 文件组织哲学（F03351~F03375） ============ */

/** F03352 智能快访：按访问频次推荐。 */
export function quickAccess(accessCount: Map<string, number>, top = 5): string[] {
  return [...accessCount.entries()].sort((a, b) => b[1] - a[1]).slice(0, top).map(([p]) => p);
}

/** F03356 多目录聚合库。 */
export class Library {
  private dirs = new Set<string>();
  add(dir: string): boolean {
    if (this.dirs.has(dir)) return false;
    this.dirs.add(dir);
    return true;
  }
  aggregate(v: Vfs): FsNode[] {
    return [...this.dirs].flatMap((d) => v.children(d));
  }
}

/** F03360 命名规范检查。 */
export function namingCheck(name: string): { ok: boolean; issues: string[] } {
  const issues: string[] = [];
  if (/[\\/:*?"<>|]/.test(name)) issues.push('含非法字符');
  if (/\s{2,}/.test(name)) issues.push('连续空格');
  if (/[A-Z]/.test(name) && /\.[a-z]+$/.test(name)) issues.push('大小写混用');
  if (/^(新|副本|未命名)/.test(name)) issues.push('默认命名残留');
  return { ok: issues.length === 0, issues };
}

/** F03361/F03362 日期命名/序号命名建议。 */
export function dateName(prefix: string, time: number): string {
  const d = new Date(time);
  const p = (x: number) => String(x).padStart(2, '0');
  return `${prefix}_${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}`;
}
export function seqName(prefix: string, i: number, pad = 3): string {
  return `${prefix}_${String(i).padStart(pad, '0')}`;
}

/** F03364 引用网络：内容互引图谱（文件内容中含路径即视为引用）。 */
export function referenceGraph(v: Vfs): Map<string, string[]> {
  const g = new Map<string, string[]>();
  for (const n of v.all()) {
    if (n.kind !== 'file' || !n.content) continue;
    for (const other of v.all()) {
      if (other.path !== n.path && n.content.includes(baseName(other.path))) {
        if (!g.has(n.path)) g.set(n.path, []);
        g.get(n.path)!.push(other.path);
      }
    }
  }
  return g;
}

/** F03365 孤儿检测：未被任何文件引用且无标签。 */
export function orphans(v: Vfs): string[] {
  const g = referenceGraph(v);
  const referenced = new Set([...g.values()].flat());
  return v.all().filter((n) => n.kind === 'file' && !referenced.has(n.path) && !(n.tags?.length ?? n.project)).map((n) => n.path);
}

/** F03369 目录健康评分。 */
export function dirHealth(v: Vfs, dir: string): { score: number; issues: string[] } {
  const issues: string[] = [];
  const kids = v.children(dir);
  const empty = kids.length === 0;
  if (empty) issues.push('空目录');
  const junk = kids.filter((k) => /\.(tmp|log|bak)$/i.test(k.path));
  if (junk.length) issues.push(`含 ${junk.length} 个临时/日志文件`);
  const dupNames = new Map<string, number>();
  for (const k of kids) dupNames.set(baseName(k.path), (dupNames.get(baseName(k.path)) ?? 0) + 1);
  const score = Math.max(0, 100 - issues.length * 20);
  return { score, issues };
}

/** F03370 目录清单导出。 */
export function treePrint(v: Vfs, root: string, indent = ''): string {
  const lines: string[] = [];
  const walk = (dir: string, pad: string) => {
    for (const c of v.children(dir)) {
      lines.push(`${pad}${baseName(c.path)}${c.kind === 'dir' ? '/' : ''}`);
      if (c.kind === 'dir') walk(c.path, pad + '  ');
    }
  };
  lines.push(`${indent}${baseName(root) || '/'}`);
  walk(root, indent + '  ');
  return lines.join('\n');
}

/** F03371 两目录对比。 */
export function dirCompare(v: Vfs, a: string, b: string): { onlyA: string[]; onlyB: string[]; changed: string[] } {
  const ca = v.children(a);
  const cb = v.children(b);
  const ma = new Map(ca.map((n) => [baseName(n.path), n]));
  const mb = new Map(cb.map((n) => [baseName(n.path), n]));
  return {
    onlyA: [...ma.keys()].filter((k) => !mb.has(k)),
    onlyB: [...mb.keys()].filter((k) => !ma.has(k)),
    changed: [...ma.keys()].filter((k) => mb.has(k) && ma.get(k)!.size !== mb.get(k)!.size),
  };
}
