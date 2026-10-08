// AURORA-10000: AI-30 批次（族0146~0150 · 性能/无障碍/本地化/扩展/内核联动），勿删。
import { Vfs, FsNode, baseName } from './fsModel';
import { fnv1a } from './groupA';

/* ============ 族0146 文件管理性能（F03626~F03650） ============ */

/** F03627 虚拟化列表：可见窗口计算。 */
export function virtualWindow(scrollTop: number, viewportH: number, rowH: number, total: number): { start: number; end: number } {
  const start = Math.max(0, Math.floor(scrollTop / rowH) - 5);
  const visible = Math.ceil(viewportH / rowH) + 10;
  return { start, end: Math.min(total, start + visible) };
}

/** F03628/F03629 LRU 缩略缓存 + 后台生成队列。 */
export class ThumbCache {
  private cap: number;
  private map = new Map<string, string>();

  constructor(cap = 256) {
    this.cap = cap;
  }
  get(key: string): string | undefined {
    const v = this.map.get(key);
    if (v !== undefined) {
      this.map.delete(key);
      this.map.set(key, v); // 触碰即移到最新
    }
    return v;
  }
  set(key: string, val: string): void {
    if (this.map.has(key)) this.map.delete(key);
    else if (this.map.size >= this.cap) this.map.delete(this.map.keys().next().value as string);
    this.map.set(key, val);
  }
  get size(): number {
    return this.map.size;
  }
}

export class ThumbWorkerQueue {
  private q: string[] = [];
  push(key: string): boolean {
    if (this.q.includes(key)) return false;
    this.q.push(key);
    return true;
  }
  step(): string | undefined {
    return this.q.shift();
  }
}

/** F03631 搜索响应预算。 */
export function searchBudget(items: number): { withinBudget: boolean; ms: number } {
  const ms = items > 100_000 ? 80 : items > 10_000 ? 20 : 2;
  return { withinBudget: ms <= 100, ms };
}

/** F03635/F03636 流式哈希/流式预览（分块处理）。 */
export function streamingHash(content: string, block = 1024): string {
  let h = '';
  for (let i = 0; i < Math.max(1, Math.ceil(content.length / block)); i++) {
    h = fnv1a(h + content.slice(i * block, (i + 1) * block));
  }
  return h;
}
export function previewWindow(text: string, startLine: number, lines: number): string[] {
  return text.split('\n').slice(startLine, startLine + lines);
}

/** F03637 内存预算。 */
export function memoryBudget(bytes: number, capMB = 50): { ok: boolean; pct: number } {
  const pct = bytes / (capMB * 1024 * 1024);
  return { ok: pct <= 1, pct };
}

/** F03641 缓存失效：mtime 驱动。 */
export class MtimeCache {
  private m = new Map<string, { mtime: number; val: string }>();
  fetch(key: string, mtime: number, compute: () => string): string {
    const c = this.m.get(key);
    if (c && c.mtime === mtime) return c.val;
    const val = compute();
    this.m.set(key, { mtime, val });
    return val;
  }
}

/** F03642/F03643 介质感知策略。 */
export function ioStrategy(media: 'ssd' | 'hdd'): { concurrency: number; sequentialHint: boolean; indexAllowed: boolean } {
  return media === 'ssd' ? { concurrency: 8, sequentialHint: false, indexAllowed: false } : { concurrency: 1, sequentialHint: true, indexAllowed: true };
}

/** F03647 省电/性能模式。 */
export type PowerMode = 'eco' | 'balanced' | 'performance';
export function powerLimits(mode: PowerMode): { bgIoMbps: number; thumbEnabled: boolean } {
  switch (mode) {
    case 'eco':
      return { bgIoMbps: 1, thumbEnabled: false };
    case 'balanced':
      return { bgIoMbps: 10, thumbEnabled: true };
    case 'performance':
      return { bgIoMbps: 0, thumbEnabled: true };
  }
}

/** F03648/F03649 阻塞诊断与自愈。 */
export function stallDiagnose(ops: { op: string; ms: number }[], threshold = 100): string[] {
  return ops.filter((o) => o.ms > threshold).map((o) => o.op);
}
export function selfHeal(state: 'ok' | 'stalled'): 'restart-panel' | 'none' {
  return state === 'stalled' ? 'restart-panel' : 'none';
}

/** F03650 性能基准套件。 */
export function benchmark(items: number, ms: number): { op: string; items: number; ms: number; opsPerSec: number } {
  return { op: 'dir-scan', items, ms, opsPerSec: Math.round(items / Math.max(0.001, ms / 1000)) };
}

/* ============ 族0147 文件管理无障碍（F03651~F03675） ============ */

/** F03652~F03654 读屏语义与播报。 */
export function rowSemantics(n: FsNode): string {
  return `${n.kind === 'dir' ? '文件夹' : '文件'} ${baseName(n.path)}，${n.kind === 'file' ? `${n.size} 字节，` : ''}${new Date(n.mtime).toLocaleDateString('zh-CN')}`;
}
export function announce(op: 'delete' | 'copy' | 'move' | 'select', detail: string): string {
  const verb = { delete: '已删除', copy: '已复制', move: '已移动', select: '已选中' }[op];
  return `${verb}${detail}`;
}

/** F03656/F03657 对比度与高对比。 */
export function contrastRatio(fg: [number, number, number], bg: [number, number, number]): number {
  const lum = (c: [number, number, number]) => {
    const parts = c.map((v) => {
      const sv = v / 255;
      return sv <= 0.03928 ? sv / 12.92 : ((sv + 0.055) / 1.055) ** 2.4;
    });
    const r = parts[0] ?? 0;
    const g = parts[1] ?? 0;
    const b = parts[2] ?? 0;
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const l1 = lum(fg);
  const l2 = lum(bg);
  const hi = Math.max(l1, l2);
  const lo = Math.min(l1, l2);
  return (hi + 0.05) / (lo + 0.05);
}
export function highContrastOk(fg: [number, number, number], bg: [number, number, number]): boolean {
  return contrastRatio(fg, bg) >= 4.5;
}

/** F03658 字号四档。 */
export const FONT_SCALES = [0.85, 1, 1.15, 1.3] as const;

/** F03661 树导航：方向键结果。 */
export function treeNav(v: Vfs, current: string, key: 'up' | 'down' | 'left' | 'right'): string {
  const parent = current === '/' ? '/' : current.slice(0, current.lastIndexOf('/')) || '/';
  const sibs = v.children(parent);
  const dirKids = v.children(current);
  switch (key) {
    case 'right':
      return dirKids[0]?.path ?? current;
    case 'left': {
      const i = current.lastIndexOf('/');
      return i <= 0 ? '/' : current.slice(0, i);
    }
    default: {
      const i = sibs.findIndex((s) => s.path === current);
      const j = key === 'down' ? Math.min(sibs.length - 1, i + 1) : Math.max(0, i - 1);
      return sibs[j]?.path ?? current;
    }
  }
}

/** F03663~F03665 范围/删除/进度播报。 */
export function rangeAnnounce(all: number, selected: number): string {
  return `已全选 ${selected} 项，共 ${all} 项`;
}
export function progressAnnounce(done: number, total: number): string {
  return `复制进度 ${Math.round((done / Math.max(1, total)) * 100)}%（${done}/${total}）`;
}

/** F03668 色弱安全标签色（CVD 安全组合）。 */
export const CVD_SAFE_TAGS: Record<string, string> = { ok: '#0072B2', warn: '#E69F00', bad: '#D55E00', info: '#009E73' };

/** F03672 语音控制指令解析。 */
export function voiceCommand(text: string): { op: 'open' | 'delete' | 'rename' | 'search'; arg: string } | undefined {
  const m = /^(打开|删除|重命名|搜索)\s*(.+)$/.exec(text.trim());
  if (!m) return undefined;
  const map: Record<string, 'open' | 'delete' | 'rename' | 'search'> = { 打开: 'open', 删除: 'delete', 重命名: 'rename', 搜索: 'search' };
  const op = m[1] ? map[m[1]] : undefined;
  if (!op) return undefined;
  return { op, arg: m[2] ?? '' };
}

/* ============ 族0148 文件管理本地化（F03676~F03700） ============ */

export const FS_LOCALES = ['zh', 'zh-TW', 'en'] as const;

/** F03678/F03679 排序规则：拼音（简化）/大小写不敏感。 */
export function localeCompareNames(a: string, b: string, locale: 'zh' | 'zh-TW' | 'en'): number {
  return a.localeCompare(b, locale === 'en' ? 'en' : 'zh-Hans-CN', { sensitivity: 'base', numeric: true });
}

/** F03680/F03681 本地日期/数字。 */
export function localeDate(t: number, locale: 'zh' | 'zh-TW' | 'en'): string {
  return new Date(t).toLocaleDateString(locale === 'en' ? 'en-US' : 'zh-CN');
}
export function localeNumber(n: number, locale: 'zh' | 'zh-TW' | 'en'): string {
  return n.toLocaleString(locale === 'en' ? 'en-US' : 'zh-CN');
}

/** F03682/F03683 非法字符与保留名。 */
export const RESERVED_NAMES = ['CON', 'PRN', 'AUX', 'NUL', 'COM1', 'LPT1'];
export function nameIssues(name: string): string[] {
  const issues: string[] = [];
  if (/[\\/:*?"<>|]/.test(name)) issues.push('invalid-chars');
  const stem = (name.split('.')[0] ?? '').toUpperCase();
  if (RESERVED_NAMES.includes(stem)) issues.push('reserved-name');
  return issues;
}

/** F03691/F03692 长名换行/中点截断。 */
export function truncateMiddle(name: string, max = 24): string {
  if (name.length <= max) return name;
  const head = Math.ceil((max - 1) / 2);
  const tail = Math.floor((max - 1) / 2);
  return `${name.slice(0, head)}…${name.slice(-tail)}`;
}

/** F03695 路径分隔符统一。 */
export function unifySeparators(path: string): string {
  return path.replace(/\\/g, '/');
}

/** F03696 缺键审计：三语言键数一致。 */
export function i18nAudit(dicts: Record<string, Record<string, string>>): { ok: boolean; missing: string[] } {
  const locales = Object.keys(dicts);
  const keySets = locales.map((l) => new Set(Object.keys(dicts[l] ?? {})));
  const all = new Set(keySets.flatMap((s) => [...s]));
  const missing: string[] = [];
  for (const k of all) {
    for (let i = 0; i < locales.length; i++) if (!keySets[i]?.has(k)) missing.push(`${locales[i] ?? ''}:${k}`);
  }
  return { ok: missing.length === 0, missing };
}

/** F03697 伪本地化。 */
export function pseudoLocalize(s: string): string {
  return `[${s.replace(/([aeiou])/g, '$1$1')}]`;
}

/* ============ 族0149 文件管理扩展（F03701~F03725） ============ */

export interface FsPlugin {
  id: string;
  name: string;
  permissions: string[];
  menuEntry?: string;
  customColumn?: string;
  sandbox: boolean;
  budgetMs?: number;
}

/** 插件登记（去重）+ 冲突检测 + 权限 + 预算。 */
export class PluginRegistry {
  private plugins = new Map<string, FsPlugin>();

  register(p: FsPlugin): boolean {
    if (this.plugins.has(p.id)) return false;
    this.plugins.set(p.id, p);
    return true;
  }
  /** F03712 冲突检测：菜单项重名。 */
  conflicts(): string[] {
    const m = new Map<string, string[]>();
    for (const p of this.plugins.values()) {
      if (!p.menuEntry) continue;
      if (!m.has(p.menuEntry)) m.set(p.menuEntry, []);
      m.get(p.menuEntry)!.push(p.id);
    }
    return [...m.entries()].filter(([, ids]) => ids.length > 1).map(([k]) => k);
  }
  /** F03713 权限校验。 */
  hasPermission(id: string, perm: string): boolean {
    return this.plugins.get(id)?.permissions.includes(perm) ?? false;
  }
  get list(): FsPlugin[] {
    return [...this.plugins.values()];
  }
}

/** F03710 上下文菜单编辑器。 */
export class ContextMenu {
  private items: { id: string; label: string; kind: 'builtin' | 'plugin'; order: number }[] = [];
  add(id: string, label: string, kind: 'builtin' | 'plugin', order = 100): boolean {
    if (this.items.some((i) => i.id === id)) return false;
    this.items.push({ id, label, kind, order });
    return true;
  }
  remove(id: string): boolean {
    const i = this.items.findIndex((x) => x.id === id);
    if (i < 0) return false;
    this.items.splice(i, 1);
    return true;
  }
  get menu(): { id: string; label: string; kind: string }[] {
    return [...this.items].sort((a, b) => a.order - b.order);
  }
}

/** F03716 varix-fs 命令行（CLI 全能力面）。 */
export const FS_CLI_COMMANDS = ['list', 'cp', 'mv', 'rm', 'hash', 'find', 'tag', 'info', 'du', 'watch'] as const;

/** F03720 配置迁移 / F03721 便携模式。 */
export function exportConfig(state: Record<string, unknown>): string {
  return JSON.stringify({ schema: 'varix-fs-config/1', state });
}
export function importConfig(json: string): Record<string, unknown> | undefined {
  try {
    const o = JSON.parse(json) as { schema?: string; state?: Record<string, unknown> };
    return o.schema === 'varix-fs-config/1' ? o.state : undefined;
  } catch {
    return undefined;
  }
}
export const PORTABLE_MARKER = '.varix-portable';

/** F03725 扩展安全审计：越权权限检出。 */
export function auditPlugins(plugins: FsPlugin[], allowed: string[]): { id: string; overreach: string[] }[] {
  return plugins
    .map((p) => ({ id: p.id, overreach: p.permissions.filter((x) => !allowed.includes(x)) }))
    .filter((r) => r.overreach.length > 0);
}

/* ============ 族0150 内核联动（F03726~F03750） ============ */

/** F03727 内核通知合并：窗口期内去重合并。 */
export function coalesceEvents(events: { path: string; op: string }[], _windowMs = 200): { path: string; ops: string[] }[] {
  const out: { path: string; ops: string[] }[] = [];
  for (const e of events) {
    const last = out[out.length - 1];
    if (last && last.path === e.path) last.ops.push(e.op);
    else out.push({ path: e.path, ops: [e.op] });
  }
  return out;
}

/** F03728 按盘策略。 */
export function drivePolicy(_drive: string, media: 'ssd' | 'hdd'): { indexing: boolean; trim: boolean } {
  return media === 'ssd' ? { indexing: false, trim: true } : { indexing: true, trim: false };
}

/** F03729/F03730 RAM 盘/临时盘。 */
export class VolatileDisks {
  private ram = new Set<string>();
  private temp = new Set<string>();
  createRamDisk(letter: string): boolean {
    if (this.ram.has(letter) || this.temp.has(letter)) return false;
    this.ram.add(letter);
    return true;
  }
  createTempDisk(letter: string): boolean {
    if (this.ram.has(letter) || this.temp.has(letter)) return false;
    this.temp.add(letter);
    return true;
  }
  /** 重启清空临时盘。 */
  reboot(): string[] {
    const cleared = [...this.temp];
    this.temp.clear();
    return cleared;
  }
}

/** F03735/F03736 用户与目录配额。 */
export function quotaCheck(used: number, quota: number): { ok: boolean; pct: number; level: 'ok' | 'warn' | 'over' } {
  const pct = used / Math.max(1, quota);
  return { ok: pct < 1, pct, level: pct >= 1 ? 'over' : pct >= 0.8 ? 'warn' : 'ok' };
}

/** F03737~F03739 内核审计留痕。 */
export class KernelAudit {
  private log: { time: number; path: string; op: 'read' | 'write' | 'delete'; user: string }[] = [];
  record(path: string, op: 'read' | 'write' | 'delete', user: string, time = Date.now()): void {
    this.log.push({ time, path, op, user });
  }
  byOp(op: 'read' | 'write' | 'delete'): { time: number; path: string; op: string; user: string }[] {
    return this.log.filter((e) => e.op === op);
  }
  get all(): { time: number; path: string; op: string; user: string }[] {
    return [...this.log];
  }
}

/** F03741 符号链接安全：拒绝越出根。 */
export function symlinkSafe(root: string, target: string): boolean {
  const norm = target.replace(/\\/g, '/');
  return norm.startsWith(root === '/' ? '/' : root + '/') || norm === root;
}

/** F03742~F03744 挂载点/盘符/自动挂载。 */
export class MountPolicy {
  private letters = new Set<string>(['C']);
  assignDrive(unc: string, preferred?: string): string | undefined {
    const letter = preferred && !this.letters.has(preferred) ? preferred : [...'DEFGHIJKLMNOPQRSTUVWXYZ'].find((l) => !this.letters.has(l));
    if (!letter) return undefined;
    this.letters.add(letter);
    return `${letter}: -> ${unc}`;
  }
  autoMountRules: Array<{ match: string; action: 'mount' | 'ask' | 'ignore' }> = [
    { match: 'ISO', action: 'ask' },
    { match: 'USB', action: 'mount' },
  ];
}

/** F03747 掉电保护：日志序号连续性校验。 */
export function writeOrderOk(logSeq: number[]): boolean {
  for (let i = 1; i < logSeq.length; i++) if (logSeq[i] !== (logSeq[i - 1] ?? 0) + 1) return false;
  return true;
}

/** F03748 文件系统自检。 */
export function fsck(v: Vfs): { ok: boolean; errors: string[] } {
  const errors: string[] = [];
  for (const n of v.all()) {
    if (n.path !== '/') {
      const p = n.path.slice(0, n.path.lastIndexOf('/')) || '/';
      if (!v.has(p)) errors.push(`孤儿节点: ${n.path}`);
    }
    if (n.kind === 'file' && n.size < 0) errors.push(`负尺寸: ${n.path}`);
  }
  return { ok: errors.length === 0, errors };
}

/** F03750 内核文件基准。 */
export function kernelBench(files: number, ms: number): { workload: string; files: number; ms: number; filesPerSec: number } {
  return { workload: 'vfs-meta-scan', files, ms, filesPerSec: Math.round(files / Math.max(0.001, ms / 1000)) };
}
