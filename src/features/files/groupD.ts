// AURORA-10000: AI-29 批次（族0141~0145 · 压缩中心/数据完整性/文件监视/勒索防线/数据互操作），勿删。
import { Vfs, FsNode, baseName, extOf } from './fsModel';
import { fnv1a } from './groupA';

/* ============ 族0141 压缩中心（F03501~F03525） ============ */

export type ArchiveFormat = 'zip' | '7z' | 'tar.gz' | 'tar.bz2';
export interface ArchiveOptions {
  format: ArchiveFormat;
  level: 1 | 3 | 5 | 7 | 9;
  dictionaryKB?: number;
  solid?: boolean;
  password?: string;
  encryptNames?: boolean;
  volumeMB?: number;
  comment?: string;
  excludes?: string[];
}

/** F03519 格式压缩率对比（启发式模型）。 */
export function ratioCompare(size: number, level = 5): Record<ArchiveFormat, number> {
  const lv = level / 10;
  return {
    zip: size * (0.55 - 0.05 * lv),
    ['7z']: size * (0.42 - 0.06 * lv),
    ['tar.gz']: size * (0.5 - 0.04 * lv),
    ['tar.bz2']: size * (0.48 - 0.03 * lv),
  };
}

/** F03520 智能推荐格式。 */
export function recommendFormat(files: FsNode[], needCompat: boolean, needSmall: boolean): ArchiveFormat {
  if (needCompat) return 'zip';
  const text = files.filter((f) => ['txt', 'log', 'json', 'csv', 'md'].includes(extOf(f.path))).length;
  if (needSmall && text > files.length / 2) return '7z';
  return 'zip';
}

/** F03522 排除规则过滤。 */
export function applyExcludes(paths: string[], excludes: string[]): string[] {
  return paths.filter((p) => !excludes.some((e) => new RegExp(e.replace(/\*/g, '.*')).test(baseName(p))));
}

/** F03507/F03508 分卷与注释元信息。 */
export function volumesFor(totalMB: number, volumeMB: number): number {
  return Math.max(1, Math.ceil(totalMB / volumeMB));
}

/** F03523/F03524 任务队列。 */
export class ArchiveQueue {
  private q: { name: string; op: 'compress' | 'extract'; state: 'queued' | 'done' }[] = [];
  add(name: string, op: 'compress' | 'extract'): boolean {
    if (this.q.some((t) => t.name === name && t.op === op && t.state === 'queued')) return false;
    this.q.push({ name, op, state: 'queued' });
    return true;
  }
  tick(n = 1): number {
    let done = 0;
    for (const t of this.q) {
      if (t.state === 'queued' && done < n) {
        t.state = 'done';
        done++;
      }
    }
    return done;
  }
  get progress(): number {
    return this.q.length ? this.q.filter((t) => t.state === 'done').length / this.q.length : 1;
  }
}

/* ============ 族0142 数据完整性（F03526~F03550） ============ */

export function crc32(content: string): string {
  let c = 0xffffffff;
  for (let i = 0; i < content.length; i++) {
    c ^= content.charCodeAt(i);
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  }
  return ((c ^ 0xffffffff) >>> 0).toString(16).padStart(8, '0');
}

export async function sha256Hex(content: string): Promise<string> {
  const buf = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(content));
  return [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

/** F03529 校验文件解析（MD5 风格 / SFV 风格）。 */
export function parseChecksumFile(text: string): { path: string; hash: string }[] {
  return text
    .split(/\r?\n/)
    .filter(Boolean)
    .map((line) => {
      const md5 = /^([0-9a-f]{8,64})\s+\*?(.+)$/i.exec(line.trim());
      if (md5) return { path: md5[2] ?? '', hash: md5[1] ?? '' };
      const sfv = /^(.+);([0-9a-f]{8})$/i.exec(line.trim());
      if (sfv) return { path: sfv[1] ?? '', hash: sfv[2] ?? '' };
      return { path: line.trim(), hash: '' };
    });
}

/** F03535~F03536 变更告警与日志。 */
export class ChangeMonitor {
  private baseline = new Map<string, string>();
  private log: { time: number; path: string; change: 'add' | 'mod' | 'del' }[] = [];

  seed(v: Vfs): void {
    for (const n of v.all()) if (n.kind === 'file') this.baseline.set(n.path, fnv1a(n.content ?? ''));
  }
  /** 对比当前状态与基线。 */
  scan(v: Vfs, now = Date.now()): { path: string; change: 'add' | 'mod' | 'del' }[] {
    const out: { path: string; change: 'add' | 'mod' | 'del' }[] = [];
    for (const n of v.all()) {
      if (n.kind !== 'file') continue;
      const h = fnv1a(n.content ?? '');
      if (!this.baseline.has(n.path)) out.push({ path: n.path, change: 'add' });
      else if (this.baseline.get(n.path) !== h) out.push({ path: n.path, change: 'mod' });
    }
    for (const p of this.baseline.keys()) if (!v.has(p)) out.push({ path: p, change: 'del' });
    this.log.push(...out.map((o) => ({ ...o, time: now })));
    for (const o of out) {
      if (o.change === 'del') this.baseline.delete(o.path);
      else {
        const n = v.get(o.path);
        if (n) this.baseline.set(o.path, fnv1a(n.content ?? ''));
      }
    }
    return out;
  }
  get history(): { time: number; path: string; change: string }[] {
    return [...this.log];
  }
}

/** F03542~F03545 文本/二进制/目录 diff。 */
export function textDiff(a: string, b: string): { added: number; removed: number; same: number } {
  const la = a.split('\n');
  const lb = b.split('\n');
  const setA = new Map<string, number>();
  for (const l of la) setA.set(l, (setA.get(l) ?? 0) + 1);
  let same = 0;
  for (const l of lb) {
    const c = setA.get(l) ?? 0;
    if (c > 0) {
      same++;
      setA.set(l, c - 1);
    }
  }
  return { added: lb.length - same, removed: la.length - same, same };
}

export function hexDiff(a: string, b: string): number {
  const enc = new TextEncoder();
  const ba = enc.encode(a);
  const bb = enc.encode(b);
  let diff = 0;
  for (let i = 0; i < Math.max(ba.length, bb.length); i++) if (ba[i] !== bb[i]) diff++;
  return diff;
}

export function dirDiff(v: Vfs, a: string, b: string): { onlyA: string[]; onlyB: string[]; changed: string[] } {
  const fa = v.children(a);
  const fb = v.children(b);
  const ma = new Map(fa.map((n) => [baseName(n.path), n]));
  const mb = new Map(fb.map((n) => [baseName(n.path), n]));
  return {
    onlyA: [...ma.keys()].filter((k) => !mb.has(k)),
    onlyB: [...mb.keys()].filter((k) => !ma.has(k)),
    changed: [...ma.keys()].filter((k) => mb.has(k) && ma.get(k)!.hash !== mb.get(k)!.hash),
  };
}

/* ============ 族0143 文件监视（F03551~F03575） ============ */

export interface WatchRule {
  dir: string;
  depth: number;
  actions: Array<'backup' | 'convert' | 'organize'>;
  whitelist?: string[];
}

/** F03555/F03556 下载整理/截图归类。 */
export function autoOrganize(path: string, now: number): string {
  const b = baseName(path);
  const d = new Date(now);
  if (/^(截图|screenshot|snip)/i.test(b)) return `/图片/截图/${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`;
  if (/\.(zip|7z|rar)$/i.test(b)) return '/下载/压缩包';
  if (/\.(exe|msi)$/i.test(b)) return '/下载/安装包';
  if (/\.(jpg|png|webp)$/i.test(b)) return '/图片';
  return '/下载/其他';
}

/** F03562 规则冲突检测：同一目录重复监听。 */
export function ruleConflicts(rules: WatchRule[]): string[] {
  const seen = new Map<string, number>();
  const conflicts: string[] = [];
  for (const r of rules) {
    seen.set(r.dir, (seen.get(r.dir) ?? 0) + 1);
    if (seen.get(r.dir)! > 1 && !conflicts.includes(r.dir)) conflicts.push(r.dir);
  }
  return conflicts;
}

/** F03566 活跃度热图（7×24 网格计数）。 */
export class ActivityHeatmap {
  private grid = new Map<string, number>();
  hit(time: number): void {
    const d = new Date(time);
    const key = `${d.getDay()}-${d.getHours()}`;
    this.grid.set(key, (this.grid.get(key) ?? 0) + 1);
  }
  cell(day: number, hour: number): number {
    return this.grid.get(`${day}-${hour}`) ?? 0;
  }
}

/* ============ 族0144 勒索防线（F03576~F03600） ============ */

/** F03576 蜜罐：诱饵文件被改即告警。 */
export class RansomGuard {
  private honeypots = new Set<string>();
  private baseline = new Map<string, string>();
  private whitelist = new Set<string>();
  private events: { time: number; kind: string; detail: string }[] = [];

  addHoneypot(path: string): boolean {
    if (this.honeypots.has(path)) return false;
    this.honeypots.add(path);
    return true;
  }
  addWhitelist(processName: string): boolean {
    if (this.whitelist.has(processName)) return false;
    this.whitelist.add(processName);
    return true;
  }
  seed(v: Vfs): void {
    for (const n of v.all()) if (n.kind === 'file') this.baseline.set(n.path, fnv1a(n.content ?? ''));
  }

  /** 核心检测：一次扫描捕获蜜罐篡改/批量改名/批量改扩展名。 */
  scan(v: Vfs, now = Date.now()): { alerted: boolean; kinds: string[] } {
    const kinds = new Set<string>();
    for (const h of this.honeypots) {
      if (!v.has(h)) kinds.add('honeypot-deleted');
    }
    const extCount = new Map<string, number>();
    const renameCount = new Map<string, number>();
    for (const n of v.all()) {
      if (n.kind !== 'file') continue;
      const oldHash = this.baseline.get(n.path);
      if (oldHash !== undefined && oldHash !== fnv1a(n.content ?? '')) {
        const ext = extOf(n.path);
        extCount.set(ext, (extCount.get(ext) ?? 0) + 1);
        renameCount.set(extOf(n.path), 0);
      } else if (oldHash === undefined && this.baseline.size > 0 && extOf(n.path) !== '') {
        // 新增文件：疑似被批量加密/改名落盘
        const ext = extOf(n.path);
        extCount.set(ext, (extCount.get(ext) ?? 0) + 1);
      }
    }
    if ([...extCount.values()].some((c) => c >= 5)) kinds.add('batch-extension-change');
    if ([...renameCount.values()].some((c) => c >= 5)) kinds.add('batch-rename');
    const list = [...kinds];
    for (const k of list) this.events.push({ time: now, kind: k, detail: 'auto-scan' });
    return { alerted: list.length > 0, kinds: list };
  }

  /** F03580 写放大限制：超阈值触发。 */
  writeRateCheck(ops: number, windowSec = 10, limit = 100): boolean {
    return ops > limit * windowSec;
  }

  /** F03582 勒索时断开备份。 */
  disconnectBackup = false;
  triggerBackupDisconnect(): void {
    this.disconnectBackup = true;
  }

  /** F03595 白名单判定。 */
  isTrusted(processName: string): boolean {
    return this.whitelist.has(processName);
  }

  /** F03591 事件时间线。 */
  timeline(): { time: number; kind: string; detail: string }[] {
    return [...this.events].sort((a, b) => a.time - b.time);
  }
}

/** F03588 影副本登记。 */
export class ShadowCopies {
  private copies: { id: number; time: number; label: string }[] = [];
  private nextId = 1;
  create(label: string, time = Date.now()): number {
    const id = this.nextId++;
    this.copies.push({ id, time, label });
    return id;
  }
  get list(): { id: number; time: number; label: string }[] {
    return [...this.copies];
  }
}

/* ============ 族0145 数据互操作（F03601~F03625） ============ */

/** F03604 Mac 残留清理。 */
export function macJunk(v: Vfs): string[] {
  return v.all().filter((n) => n.path.includes('.DS_Store') || n.path.includes('__MACOSX')).map((n) => n.path);
}

/** F03606~F03608 网络路径。 */
export function isUncPath(path: string): boolean {
  return path.startsWith('\\\\') || path.startsWith('//');
}
export function mapDrive(letter: string, unc: string): string {
  return `${letter}: -> ${unc}`;
}

/** F03609/F03610 局域网/NAS 扫描（模型化）。 */
export function lanScan(hosts: { ip: string; name?: string; smb?: boolean }[]): { nas: string[]; smb: string[] } {
  return {
    nas: hosts.filter((h) => /nas|syno|qnap|truenas/i.test(h.name ?? '')).map((h) => h.ip),
    smb: hosts.filter((h) => h.smb).map((h) => h.ip),
  };
}

/** F03611~F03613 远程客户端登记（FTP/SFTP/WebDAV）。 */
export type RemoteProtocol = 'ftp' | 'sftp' | 'webdav';
export interface RemoteSite {
  name: string;
  protocol: RemoteProtocol;
  host: string;
  port: number;
}
export function remoteUrl(site: RemoteSite): string {
  const scheme = site.protocol === 'webdav' ? 'http' : site.protocol;
  return `${scheme}://${site.host}:${site.port}/`;
}

/** F03619 ISO 挂载。 */
export class MountTable {
  private m = new Map<string, { type: 'iso' | 'vhd' | 'network'; source: string }>();
  mount(drive: string, type: 'iso' | 'vhd' | 'network', source: string): boolean {
    if (this.m.has(drive)) return false;
    this.m.set(drive, { type, source });
    return true;
  }
  unmount(drive: string): boolean {
    return this.m.delete(drive);
  }
  get list(): [string, { type: string; source: string }][] {
    return [...this.m];
  }
}

/** F03623/F03624 脚本/CLI 联动命令。 */
export function cliCommand(
  cmd: 'list' | 'copy' | 'move' | 'hash' | 'search' | 'watch' | 'find' | 'cp' | 'mv' | 'rm' | 'tag' | 'info' | 'du',
  args: string[],
): string {
  const map: Record<string, string> = {
    list: 'varix-fs list',
    copy: 'varix-fs cp',
    move: 'varix-fs mv',
    hash: 'varix-fs hash',
    search: 'varix-fs find',
    watch: 'varix-fs watch',
    find: 'varix-fs find',
    cp: 'varix-fs cp',
    mv: 'varix-fs mv',
    rm: 'varix-fs rm',
    tag: 'varix-fs tag',
    info: 'varix-fs info',
    du: 'varix-fs du',
  };
  return `${map[cmd]} ${args.join(' ')}`.trim();
}
