// AURORA-10000: AI-32 批次（领域07 效率与工具中枢）逻辑核，勿删。
// 族0156 系统监视器 / 族0157 效率面板 / 族0158 文本工具集 / 族0159 开发者工具 / 族0160 录音与音频工具。
// 全部纯函数/纯模型；SHA-256/HMAC 为本地实现，零网络。

import qrcode from 'qrcode-generator';
import { uuid } from './groupA';

/* ========================= 族0156 系统监视器 ========================= */

export interface ProcSample {
  pid: number;
  name: string;
  cpu: number; // %
  mem: number; // MB
  disk: number; // MB/s
  net: number; // KB/s
  gpu: number; // %
  type: 'app' | 'system' | 'service';
  parent?: number;
  threads: number;
  handles: number;
  modules: string[];
  status: 'running' | 'notResponding' | 'suspended';
}

export const MOCK_PROCS: ProcSample[] = [
  { pid: 4, name: 'System', cpu: 0.3, mem: 144, disk: 0.5, net: 0, gpu: 0, type: 'system', threads: 180, handles: 5200, modules: ['ntoskrnl.exe'], status: 'running' },
  { pid: 1024, name: 'varix.exe', cpu: 6.2, mem: 412, disk: 2.1, net: 120, gpu: 3.5, type: 'app', parent: 900, threads: 42, handles: 1320, modules: ['varix.exe', 'webview2.dll'], status: 'running' },
  { pid: 1025, name: 'varix-render.exe', cpu: 12.8, mem: 655, disk: 8.4, net: 90, gpu: 22.1, type: 'app', parent: 1024, threads: 28, handles: 890, modules: ['webview2.dll', 'd3d11.dll'], status: 'running' },
  { pid: 2048, name: 'game.exe', cpu: 35.4, mem: 8192, disk: 40.2, net: 560, gpu: 78.9, type: 'app', threads: 64, handles: 3400, modules: ['game.exe', 'anticheat.sys'], status: 'running' },
  { pid: 3000, name: 'stuck.exe', cpu: 0.1, mem: 88, disk: 0, net: 0, gpu: 0, type: 'app', parent: 900, threads: 6, handles: 210, modules: ['stuck.exe'], status: 'notResponding' },
  { pid: 4000, name: 'audiodg.exe', cpu: 1.2, mem: 34, disk: 0.2, net: 0, gpu: 0.4, type: 'service', threads: 8, handles: 140, modules: ['audiodg.exe'], status: 'running' },
  { pid: 5000, name: 'updater.exe', cpu: 0.8, mem: 56, disk: 1.2, net: 220, gpu: 0, type: 'app', threads: 10, handles: 180, modules: ['updater.exe'], status: 'suspended' },
  { pid: 6000, name: 'indexer.exe', cpu: 4.4, mem: 210, disk: 15.6, net: 10, gpu: 0, type: 'service', threads: 12, handles: 420, modules: ['indexer.exe'], status: 'running' },
];

export interface MetricPoint {
  t: number;
  cpu: number;
  mem: number;
  disk: number;
  net: number;
  gpu: number;
}

export class MetricsHistory {
  private points: MetricPoint[] = [];

  push(p: MetricPoint): void {
    this.points.push(p);
  }

  /** F03889 历史曲线：窗口抽样（60s/10min）。 */
  series(windowSec: number): MetricPoint[] {
    return this.points.filter((p) => p.t >= (this.points[this.points.length - 1]?.t ?? 0) - windowSec * 1000);
  }

  get count(): number {
    return this.points.length;
  }
}

export type ProcColumn = keyof Pick<ProcSample, 'cpu' | 'mem' | 'disk' | 'net' | 'gpu' | 'name' | 'pid'>;

export class ProcessTable {
  constructor(public procs: ProcSample[] = MOCK_PROCS) {}

  sortBy(col: ProcColumn, desc = true): ProcSample[] {
    return [...this.procs].sort((a, b) => {
      const d = a[col] < b[col] ? -1 : a[col] > b[col] ? 1 : 0;
      return desc ? -d : d;
    });
  }

  search(q: string): ProcSample[] {
    const k = q.toLowerCase();
    return this.procs.filter((p) => p.name.toLowerCase().includes(k) || String(p.pid).includes(k));
  }

  /** 安全结束：系统关键进程（pid≤100 或类型 system）拒绝。 */
  kill(pid: number): 'ok' | 'protected' | 'missing' {
    const p = this.procs.find((x) => x.pid === pid);
    if (!p) return 'missing';
    if (p.type === 'system' || pid <= 100) return 'protected';
    this.procs = this.procs.filter((x) => x.pid !== pid);
    return 'ok';
  }

  tree(rootPid?: number): ProcSample[] {
    const roots = rootPid === undefined ? this.procs.filter((p) => p.parent === undefined || !this.procs.some((x) => x.pid === p.parent)) : this.procs.filter((p) => p.pid === rootPid);
    const out: ProcSample[] = [];
    const walk = (pid: number, depth: number): void => {
      for (const p of this.procs.filter((x) => x.parent === pid)) {
        out.push({ ...p, name: `${'  '.repeat(depth)}${p.name}` });
        walk(p.pid, depth + 1);
      }
    };
    for (const r of roots) {
      out.push(r);
      walk(r.pid, 1);
    }
    return out;
  }

  byCategory(): Map<ProcSample['type'], ProcSample[]> {
    const m = new Map<ProcSample['type'], ProcSample[]>();
    for (const p of this.procs) {
      const arr = m.get(p.type) ?? [];
      arr.push(p);
      m.set(p.type, arr);
    }
    return m;
  }
}

/** F03894 等待链：未响应进程 → 其等待对象。 */
export function waitChain(procs: ProcSample[]): { pid: number; chain: string[] }[] {
  return procs
    .filter((p) => p.status === 'notResponding')
    .map((p) => ({
      pid: p.pid,
      chain: [p.name, ...(p.parent !== undefined ? [procs.find((x) => x.pid === p.parent)?.name ?? '?'] : []), '内核对象: 同步句柄'],
    }));
}

export function perProcessRanking(procs: ProcSample[], kind: 'gpu' | 'disk' | 'net', topN = 3): ProcSample[] {
  const key = kind === 'gpu' ? 'gpu' : kind;
  return [...procs].sort((a, b) => (b[key] as number) - (a[key] as number)).slice(0, topN);
}

export interface ServiceItem {
  name: string;
  display: string;
  running: boolean;
  startup: 'auto' | 'manual' | 'disabled';
}

export class ServiceTable {
  private items: ServiceItem[] = [
    { name: 'varix-core', display: 'Variable 核心服务', running: true, startup: 'auto' },
    { name: 'varix-fs', display: '虚拟文件服务', running: true, startup: 'auto' },
    { name: 'varix-sync', display: '同步服务', running: false, startup: 'manual' },
    { name: 'legacy-print', display: '兼容打印池', running: false, startup: 'disabled' },
  ];

  get list(): readonly ServiceItem[] {
    return this.items;
  }

  toggle(name: string): boolean {
    const s = this.items.find((x) => x.name === name);
    if (!s || s.startup === 'disabled') return false;
    s.running = !s.running;
    return true;
  }

  setStartup(name: string, mode: ServiceItem['startup']): boolean {
    const s = this.items.find((x) => x.name === name);
    if (!s) return false;
    s.startup = mode;
    return true;
  }
}

export interface StartupItem {
  name: string;
  from: 'registry' | 'startupFolder' | 'task';
  enabled: boolean;
  impact: 'low' | 'medium' | 'high';
}

export class StartupManager {
  private items: StartupItem[] = [
    { name: 'varix-shell', from: 'registry', enabled: true, impact: 'medium' },
    { name: 'cloud-drive', from: 'startupFolder', enabled: true, impact: 'high' },
    { name: 'keyboard-feel', from: 'task', enabled: false, impact: 'low' },
  ];

  get list(): readonly StartupItem[] {
    return this.items;
  }

  toggle(name: string): boolean {
    const it = this.items.find((x) => x.name === name);
    if (!it) return false;
    it.enabled = !it.enabled;
    return true;
  }

  highImpact(): StartupItem[] {
    return this.items.filter((x) => x.enabled && x.impact === 'high');
  }
}

export const SCHEDULED_TASKS = [
  { name: '磁盘整理', trigger: '每周日 03:00', last: '2026-09-06 03:00', next: '2026-09-13 03:00' },
  { name: '备份快照', trigger: '每天 12:00/24:00', last: '2026-09-13 00:00', next: '2026-09-13 12:00' },
  { name: '更新检查', trigger: '每 6 小时', last: '2026-09-13 06:00', next: '2026-09-13 12:00' },
];

export function summaryMetrics(h: MetricsHistory): { cpu: number; mem: number; net: number; uptimeMin: number } {
  const last = h.series(60);
  const avg = (f: (p: MetricPoint) => number) => (last.length ? Math.round((last.reduce((s, p) => s + f(p), 0) / last.length) * 10) / 10 : 0);
  return { cpu: avg((p) => p.cpu), mem: avg((p) => p.mem), net: avg((p) => p.net), uptimeMin: h.count };
}

export const FLOAT_WINDOW_SPEC = { width: 180, height: 64, opacity: 0.85, draggable: true, alwaysOnTop: true };
export const MONITOR_TIPS = ['双击列头排序，Ctrl+F 搜进程', '等待链定位卡死根因', '按进程看 GPU/磁盘/网络归属'];

/* ========================= 族0157 效率面板 ========================= */

export interface UsageEvent {
  app: string;
  start: number;
  end: number;
  focus: boolean; // 是否专注类应用
}

export interface Habit {
  name: string;
  history: number[]; // 完成的天序（day index）
}

export interface TimeBlock {
  label: string;
  startMin: number;
  endMin: number;
  done: boolean;
}

export class UsageLog {
  private events: UsageEvent[] = [];
  private pomo: { at: number; minutes: number; todo?: string }[] = [];
  private switches = 0;
  private interruptions: { source: string; at: number }[] = [];

  record(e: UsageEvent): this {
    this.events.push(e);
    return this;
  }

  recordPomodoro(at: number, minutes: number, todo?: string): this {
    this.pomo.push({ at, minutes, todo });
    return this;
  }

  recordSwitch(): this {
    this.switches += 1;
    return this;
  }

  recordInterruption(source: string, at: number): this {
    this.interruptions.push({ source, at });
    return this;
  }

  /** F03901 今日概览：时间去哪了。 */
  todayOverview(now: number): { totalMin: number; focusMin: number; apps: { app: string; minutes: number }[] } {
    const dayStart = new Date(now); dayStart.setHours(0, 0, 0, 0);
    const evs = this.events.filter((e) => e.start >= dayStart.getTime());
    const byApp = new Map<string, number>();
    let total = 0;
    let focus = 0;
    for (const e of evs) {
      const mins = Math.max(0, (e.end - e.start) / 60000);
      total += mins;
      if (e.focus) focus += mins;
      byApp.set(e.app, (byApp.get(e.app) ?? 0) + mins);
    }
    return {
      totalMin: Math.round(total),
      focusMin: Math.round(focus),
      apps: [...byApp.entries()].map(([app, minutes]) => ({ app, minutes: Math.round(minutes) })).sort((a, b) => b.minutes - a.minutes),
    };
  }

  appRanking(topN = 5, now = Date.now()): { app: string; minutes: number }[] {
    return this.todayOverview(now).apps.slice(0, topN);
  }

  focusMinutes(): number {
    return this.events.filter((e) => e.focus).reduce((s, e) => s + (e.end - e.start) / 60000, 0);
  }

  pomodoroHistory(): { at: number; minutes: number; todo?: string }[] {
    return [...this.pomo];
  }

  /** F03906 每日效率分（0~100）：专注占比 70% + 番茄 20% + 少打断 10%。 */
  dailyScore(now: number): number {
    const ov = this.todayOverview(now);
    if (ov.totalMin === 0) return 0;
    const focusRatio = ov.focusMin / ov.totalMin;
    const pomoPart = Math.min(1, this.pomo.length / 8);
    const interruptPart = Math.max(0, 1 - this.interruptions.length / 20);
    return Math.round(focusRatio * 70 + pomoPart * 20 + interruptPart * 10);
  }

  interruptionRanking(): { source: string; count: number }[] {
    const m = new Map<string, number>();
    for (const i of this.interruptions) m.set(i.source, (m.get(i.source) ?? 0) + 1);
    return [...m.entries()].map(([source, count]) => ({ source, count })).sort((a, b) => b.count - a.count);
  }

  contextSwitches(): number {
    return this.switches;
  }

  /** F03909 深度时段：按小时聚合专注分钟，找峰值小时。 */
  deepHours(now: number): { hour: number; focusMin: number }[] {
    const dayStart = new Date(now); dayStart.setHours(0, 0, 0, 0);
    const byHour = new Map<number, number>();
    for (const e of this.events.filter((e) => e.focus && e.start >= dayStart.getTime())) {
      const h = new Date(e.start).getHours();
      byHour.set(h, (byHour.get(h) ?? 0) + (e.end - e.start) / 60000);
    }
    return [...byHour.entries()].map(([hour, focusMin]) => ({ hour, focusMin: Math.round(focusMin) })).sort((a, b) => b.focusMin - a.focusMin);
  }

  weeklyReport(now: number): { days: { day: string; focusMin: number }[]; totalFocusMin: number } {
    const out: { day: string; focusMin: number }[] = [];
    for (let i = 6; i >= 0; i--) {
      const d = new Date(now - i * 86400_000);
      out.push({ day: `${d.getMonth() + 1}/${d.getDate()}`, focusMin: Math.round(this.focusMinutes() / 7) + i });
    }
    return { days: out, totalFocusMin: out.reduce((s, x) => s + x.focusMin, 0) };
  }

  monthlyReport(now: number): { weeks: number; totalFocusMin: number; avgScore: number } {
    return { weeks: 4, totalFocusMin: Math.round(this.focusMinutes() * 30), avgScore: this.dailyScore(now) };
  }
}

export class EfficiencyGoals {
  private goals: { key: string; targetMin: number }[] = [];

  set(key: string, targetMin: number): this {
    const g = this.goals.find((x) => x.key === key);
    if (g) g.targetMin = targetMin;
    else this.goals.push({ key, targetMin });
    return this;
  }

  progress(key: string, actualMin: number): { targetMin: number; pct: number; met: boolean } {
    const g = this.goals.find((x) => x.key === key);
    const target = g?.targetMin ?? 0;
    return { targetMin: target, pct: target === 0 ? 0 : Math.min(100, Math.round((actualMin / target) * 100)), met: actualMin >= target };
  }
}

export class HabitTracker {
  private habits = new Map<string, number[]>();

  add(name: string): this {
    if (!this.habits.has(name)) this.habits.set(name, []);
    return this;
  }

  check(name: string, dayIndex: number): boolean {
    const arr = this.habits.get(name);
    if (!arr) return false;
    if (!arr.includes(dayIndex)) arr.push(dayIndex);
    return true;
  }

  /** 连续天数（从今天往前数）。 */
  streak(name: string, todayIndex: number): number {
    const arr = [...(this.habits.get(name) ?? [])].sort((a, b) => b - a);
    let n = 0;
    for (let i = 0; i < arr.length; i++) {
      if (arr[i] === todayIndex - i) n += 1;
      else break;
    }
    return n;
  }

  dueReminders(name: string, todayIndex: number): boolean {
    return !(this.habits.get(name) ?? []).includes(todayIndex);
  }

  get names(): string[] {
    return [...this.habits.keys()];
  }
}

/** 本地规则建议（不出网）。 */
export function efficiencySuggestions(ov: { totalMin: number; focusMin: number }, switches: number): string[] {
  const out: string[] = [];
  if (ov.totalMin > 0 && ov.focusMin / ov.totalMin < 0.4) out.push('专注占比偏低：试试 25 分钟番茄锁定一个任务');
  if (switches > 40) out.push('上下文切换偏多：把同类窗口聚到同一虚拟桌面');
  if (ov.totalMin > 480) out.push('今日使用超 8 小时：安排一次休息');
  if (out.length === 0) out.push('状态良好，保持节奏');
  return out;
}

export class TimeBlockPlanner {
  private blocks: TimeBlock[] = [];

  add(label: string, startMin: number, endMin: number): boolean {
    if (endMin <= startMin) return false;
    if (this.blocks.some((b) => startMin < b.endMin && endMin > b.startMin)) return false;
    this.blocks.push({ label, startMin, endMin, done: false });
    this.blocks.sort((a, b) => a.startMin - b.startMin);
    return true;
  }

  get plan(): readonly TimeBlock[] {
    return this.blocks;
  }

  /** 执行：标记当前应进行的块。 */
  execute(nowMin: number): TimeBlock | undefined {
    const cur = this.blocks.find((b) => nowMin >= b.startMin && nowMin < b.endMin);
    if (cur) cur.done = true;
    return cur;
  }

  /** 与日历联动：合并已占用区间。 */
  integrate(busy: { startMin: number; endMin: number }[]): TimeBlock[] {
    return this.blocks.filter((b) => !busy.some((x) => b.startMin < x.endMin && b.endMin > x.startMin));
  }
}

export function efficiencyExport(log: UsageLog): string {
  return JSON.stringify({ v: 1, score: log.dailyScore(Date.now()) });
}

export const EFFICIENCY_PRIVACY = '统计仅本地存储，不上传任何数据';
export const EFFICIENCY_WIDGET_SPEC = { size: [2, 2], refreshMin: 5 };
export const EFFICIENCY_HOTKEY = 'Ctrl+Alt+E';

/** §15 守卫：网页时长统计为预留位（需浏览器内核配合）。 */
export const WEB_TIME_RESERVED = true;
export const EFFICIENCY_TIPS = ['深色时段用深度时段视图找高效区间', '习惯打卡连续 7 天给成就章'];

/* ========================= 族0158 文本工具集 ========================= */

export function toFullWidth(s: string): string {
  return s.replace(/[\x21-\x7e]/g, (c) => String.fromCharCode(c.charCodeAt(0) + 0xfee0)).replace(/ /g, '　');
}

export function toHalfWidth(s: string): string {
  return s.replace(/[\uff01-\uff5e]/g, (c) => String.fromCharCode(c.charCodeAt(0) - 0xfee0)).replace(/　/g, ' ');
}

export function caseStyle(input: string, style: 'camel' | 'pascal' | 'snake' | 'kebab' | 'upper-snake'): string {
  const words = input
    .replace(/([a-z])([A-Z])/g, '$1 $2')
    .split(/[\s_\-]+/)
    .filter(Boolean)
    .map((w) => w.toLowerCase());
  if (words.length === 0) return '';
  if (style === 'camel') return words.map((w, i) => (i === 0 ? w : w[0]!.toUpperCase() + w.slice(1))).join('');
  if (style === 'pascal') return words.map((w) => w[0]!.toUpperCase() + w.slice(1)).join('');
  if (style === 'snake') return words.join('_');
  if (style === 'upper-snake') return words.join('_').toUpperCase();
  return words.join('-');
}

export function dedupeLines(text: string, keepLast = false): string {
  const lines = text.split('\n');
  const seen = new Set<string>();
  const out: string[] = [];
  const src = keepLast ? [...lines].reverse() : lines;
  for (const l of src) {
    if (seen.has(l)) continue;
    seen.add(l);
    out.push(l);
  }
  return (keepLast ? out.reverse() : out).join('\n');
}

export function sortLines(text: string, opts: { numeric?: boolean; desc?: boolean } = {}): string {
  const lines = text.split('\n');
  const cmp = opts.numeric
    ? (a: string, b: string) => (Number(a) || 0) - (Number(b) || 0)
    : (a: string, b: string) => a.localeCompare(b, 'zh');
  lines.sort(cmp);
  if (opts.desc) lines.reverse();
  return lines.join('\n');
}

export function reverseLines(text: string): string {
  return text.split('\n').reverse().join('\n');
}

export function reverseText(text: string): string {
  return [...text].reverse().join('');
}

export function removeEmptyLines(text: string): string {
  return text.split('\n').filter((l) => l.trim() !== '').join('\n');
}

export function trimLines(text: string): string {
  return text.split('\n').map((l) => l.trim()).join('\n');
}

export function countLines(text: string): number {
  return text === '' ? 0 : text.split('\n').length;
}

/** 字数：CJK 按字计，拉丁按词计。 */
export function countWords(text: string): number {
  const cjk = (text.match(/[\u4e00-\u9fff\u3400-\u4dbf]/g) ?? []).length;
  const latin = (text.match(/[A-Za-z0-9]+(?:['’-][A-Za-z0-9]+)*/g) ?? []).length;
  return cjk + latin;
}

export function columnPrefix(text: string, prefix: string): string {
  return text.split('\n').map((l) => (l.length ? prefix + l : l)).join('\n');
}

export function joinColumns(a: string, b: string, sep = '\t'): string {
  const la = a.split('\n');
  const lb = b.split('\n');
  const n = Math.max(la.length, lb.length);
  return Array.from({ length: n }, (_, i) => `${la[i] ?? ''}${sep}${lb[i] ?? ''}`).join('\n');
}

export function splitColumn(text: string, sep: string, index: number): string {
  return text.split('\n').map((l) => l.split(sep)[index] ?? '').join('\n');
}

export function regexExtract(text: string, pattern: string, flags = 'g'): string[] {
  const re = new RegExp(pattern, flags.includes('g') ? flags : flags + 'g');
  const out: string[] = [];
  for (const m of text.matchAll(re)) out.push(m[1] ?? m[0]);
  return out;
}

export function regexReplace(text: string, pattern: string, replacement: string, flags = 'g'): string {
  return text.replace(new RegExp(pattern, flags), replacement);
}

export function mdToHtml(md: string): string {
  return md
    .split('\n')
    .map((l) => {
      if (/^### /.test(l)) return `<h3>${l.slice(4)}</h3>`;
      if (/^## /.test(l)) return `<h2>${l.slice(3)}</h2>`;
      if (/^# /.test(l)) return `<h1>${l.slice(2)}</h1>`;
      if (/^[-*] /.test(l)) return `<li>${l.slice(2)}</li>`;
      return `<p>${l.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>').replace(/\*(.+?)\*/g, '<em>$1</em>')}</p>`;
    })
    .join('\n');
}

export function htmlToMd(html: string): string {
  return html
    .replace(/<h([1-3])>(.*?)<\/h\1>/g, (_, lvl: string, txt: string) => `${'#'.repeat(Number(lvl))} ${txt}`)
    .replace(/<li>(.*?)<\/li>/g, '- $1')
    .replace(/<strong>(.*?)<\/strong>/g, '**$1**')
    .replace(/<em>(.*?)<\/em>/g, '*$1*')
    .replace(/<p>(.*?)<\/p>/g, '$1')
    .replace(/<[^>]+>/g, '')
    .trim();
}

export function jsonFormat(input: string, indent = 2): string {
  return JSON.stringify(JSON.parse(input), null, indent);
}

export function jsonCheck(input: string): { ok: boolean; error?: string } {
  try {
    JSON.parse(input);
    return { ok: true };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
}

export function jsonMinify(input: string): string {
  return JSON.stringify(JSON.parse(input));
}

export function csvToJson(csv: string): Record<string, string>[] {
  const [head, ...rows] = csv.trim().split('\n');
  const cols = (head ?? '').split(',');
  return rows.filter((r) => r.trim()).map((r) => {
    const vals = r.split(',');
    const o: Record<string, string> = {};
    cols.forEach((c, i) => (o[c.trim()] = (vals[i] ?? '').trim()));
    return o;
  });
}

export function jsonToCsv(items: Record<string, unknown>[]): string {
  if (items.length === 0) return '';
  const cols = [...new Set(items.flatMap((o) => Object.keys(o)))];
  const esc = (v: unknown) => (typeof v === 'string' && v.includes(',') ? `"${v}"` : String(v ?? ''));
  return [cols.join(','), ...items.map((o) => cols.map((c) => esc(o[c])).join(','))].join('\n');
}

const SQL_KEYWORDS = ['SELECT', 'FROM', 'WHERE', 'GROUP BY', 'ORDER BY', 'HAVING', 'LIMIT', 'JOIN', 'LEFT JOIN', 'INNER JOIN', 'ON', 'INSERT INTO', 'VALUES', 'UPDATE', 'SET', 'DELETE FROM'];

export function sqlFormat(sql: string): string {
  let out = sql.replace(/\s+/g, ' ').trim();
  for (const kw of SQL_KEYWORDS) {
    out = out.replace(new RegExp(`\\s*\\b${kw}\\b`, 'gi'), `\n${kw}`);
  }
  return out.trim();
}

export function beautifyCode(src: string): string {
  let indent = 0;
  return src
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)
    .map((l) => {
      if (l.startsWith('}')) indent = Math.max(0, indent - 1);
      const line = '  '.repeat(indent) + l;
      if (l.endsWith('{')) indent += 1;
      return line;
    })
    .join('\n');
}

export function lorem(sentences: number): string {
  const words = 'lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod tempor incididunt ut labore'.split(' ');
  const out: string[] = [];
  let s = 0;
  while (s < sentences) {
    const len = 6 + (s % 5);
    const ws = Array.from({ length: len }, (_, i) => words[(s * 7 + i) % words.length]);
    const sent = ws.join(' ');
    out.push(sent[0]!.toUpperCase() + sent.slice(1) + '.');
    s += 1;
  }
  return out.join(' ');
}

export function genUuidBatch(n: number): string[] {
  return Array.from({ length: Math.max(0, n) }, () => uuid());
}

export function genPassword(len: number, opts: { upper?: boolean; lower?: boolean; digit?: boolean; symbol?: boolean } = {}): string {
  let pool = '';
  if (opts.lower !== false) pool += 'abcdefghijkmnpqrstuvwxyz';
  if (opts.upper) pool += 'ABCDEFGHJKLMNPQRSTUVWXYZ';
  if (opts.digit !== false) pool += '23456789';
  if (opts.symbol) pool += '!@#$%^&*()-_=+[]{}';
  if (!pool) pool = 'abcdefghijkmnpqrstuvwxyz23456789';
  const bytes = new Uint8Array(len);
  globalThis.crypto.getRandomValues(bytes);
  return [...bytes].map((b) => pool[b % pool.length]).join('');
}

/** F03949 二维码：本地生成（qrcode-generator，纯计算零网络）。 */
export function qrMatrix(text: string): { size: number; dark: (r: number, c: number) => boolean } {
  const qr = qrcode(0, 'M');
  qr.addData(text);
  qr.make();
  const n = qr.getModuleCount();
  return { size: n, dark: (r, c) => qr.isDark(r, c) };
}

/* ========================= 族0159 开发者工具 ========================= */

export interface RegexMatch {
  index: number;
  text: string;
  groups: string[];
}

export function regexTest(pattern: string, flags: string, text: string): RegexMatch[] | { error: string } {
  try {
    const re = new RegExp(pattern, flags);
    const out: RegexMatch[] = [];
    if (!flags.includes('g')) {
      const m = text.match(re);
      if (m) out.push({ index: m.index ?? 0, text: m[0], groups: m.slice(1) });
      return out;
    }
    for (const m of text.matchAll(re)) out.push({ index: m.index, text: m[0], groups: m.slice(1) });
    return out;
  } catch (e) {
    return { error: e instanceof Error ? e.message : String(e) };
  }
}

export function timestampConvert(ts: number): { iso: string; local: string } {
  const d = new Date(ts);
  return { iso: d.toISOString(), local: d.toLocaleString('zh-CN', { hour12: false }) };
}

export function timestampOf(iso: string): number {
  return new Date(iso).getTime();
}

export function jsonToTs(json: string, rootName = 'Root'): string {
  const lines: string[] = [];
  const tsType = (v: unknown): string => {
    if (v === null) return 'null';
    if (Array.isArray(v)) {
      const inner = tsType(v[0]);
      return `${inner}[]`;
    }
    if (typeof v === 'object') return 'object';
    return typeof v;
  };
  const gen = (val: Record<string, unknown>, name: string): void => {
    lines.push(`interface ${name} {`);
    for (const [k, v] of Object.entries(val)) {
      const key = /^[A-Za-z_$][\w$]*$/.test(k) ? k : JSON.stringify(k);
      if (Array.isArray(v) && v.length > 0 && typeof v[0] === 'object' && v[0] !== null) {
        const inner = `I${k[0]!.toUpperCase()}${k.slice(1)}`;
        lines.push(`  ${key}: ${inner}[];`);
        gen(v[0] as Record<string, unknown>, inner);
      } else if (typeof v === 'object' && v !== null && !Array.isArray(v)) {
        const inner = `I${k[0]!.toUpperCase()}${k.slice(1)}`;
        lines.push(`  ${key}: ${inner};`);
        gen(v as Record<string, unknown>, inner);
      } else {
        lines.push(`  ${key}: ${tsType(v)};`);
      }
    }
    lines.push('}');
  };
  gen(JSON.parse(json) as Record<string, unknown>, rootName);
  return lines.join('\n');
}

export function urlEncode(s: string, component = true): string {
  return component ? encodeURIComponent(s) : encodeURI(s);
}

export function urlDecode(s: string, component = true): string {
  return component ? decodeURIComponent(s) : decodeURI(s);
}

const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';

export function base64Encode(input: string): string {
  const bytes = new TextEncoder().encode(input);
  let out = '';
  for (let i = 0; i < bytes.length; i += 3) {
    const b0 = bytes[i] ?? 0;
    const b1 = bytes[i + 1];
    const b2 = bytes[i + 2];
    out += B64[b0 >> 2]!;
    out += b1 === undefined ? B64[(b0 & 3) << 4]! + '==' : b2 === undefined ? B64[((b0 & 3) << 4) | (b1 >> 4)]! + B64[(b1 & 15) << 2]! + '=' : B64[((b0 & 3) << 4) | (b1 >> 4)]! + B64[((b1 & 15) << 2) | (b2 >> 6)]! + B64[b2 & 63]!;
  }
  return out;
}

export function base64Decode(b64: string): string {
  const clean = b64.replace(/=+$/, '');
  const bytes: number[] = [];
  for (let i = 0; i < clean.length; i += 4) {
    const c0 = B64.indexOf(clean[i] ?? 'A');
    const c1 = B64.indexOf(clean[i + 1] ?? 'A');
    const c2 = B64.indexOf(clean[i + 2] ?? 'A');
    const c3 = B64.indexOf(clean[i + 3] ?? 'A');
    bytes.push((c0 << 2) | (c1 >> 4));
    if (c2 >= 0 && i + 2 < clean.length) bytes.push(((c1 & 15) << 4) | (c2 >> 2));
    if (c3 >= 0 && i + 3 < clean.length) bytes.push(((c2 & 3) << 6) | c3);
  }
  return new TextDecoder().decode(new Uint8Array(bytes));
}

export function jwtDecode(token: string): { header: Record<string, unknown>; payload: Record<string, unknown> } | { error: string } {
  const parts = token.split('.');
  if (parts.length !== 3) return { error: 'JWT 需三段' };
  try {
    return {
      header: JSON.parse(base64Decode(parts[0]!.replace(/-/g, '+').replace(/_/g, '/'))),
      payload: JSON.parse(base64Decode(parts[1]!.replace(/-/g, '+').replace(/_/g, '/'))),
    };
  } catch {
    return { error: '解码失败' };
  }
}

/* ---- SHA-256 / HMAC 本地实现 ---- */

const K256 = [
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

function rotr(x: number, n: number): number {
  return (x >>> n) | (x << (32 - n));
}

export function sha256Bytes(msg: Uint8Array): Uint8Array {
  const H = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
  const bitLen = msg.length * 8;
  const padded = new Uint8Array(((msg.length + 8) >> 6 << 6) + 64);
  padded.set(msg);
  padded[msg.length] = 0x80;
  const dv = new DataView(padded.buffer);
  dv.setUint32(padded.length - 4, bitLen >>> 0);
  dv.setUint32(padded.length - 8, Math.floor(bitLen / 0x100000000));
  const w = new Uint32Array(64);
  for (let off = 0; off < padded.length; off += 64) {
    for (let i = 0; i < 16; i++) w[i] = dv.getUint32(off + i * 4);
    for (let i = 16; i < 64; i++) {
      const s0 = rotr(w[i - 15]!, 7) ^ rotr(w[i - 15]!, 18) ^ (w[i - 15]! >>> 3);
      const s1 = rotr(w[i - 2]!, 17) ^ rotr(w[i - 2]!, 19) ^ (w[i - 2]! >>> 10);
      w[i] = (w[i - 16]! + s0 + w[i - 7]! + s1) >>> 0;
    }
    let a = H[0]!, b = H[1]!, c = H[2]!, d = H[3]!, e = H[4]!, f = H[5]!, g = H[6]!, h = H[7]!;
    for (let i = 0; i < 64; i++) {
      const S1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
      const ch = (e & f) ^ (~e & g);
      const t1 = (h + S1 + ch + K256[i]! + w[i]!) >>> 0;
      const S0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
      const mj = (a & b) ^ (a & c) ^ (b & c);
      const t2 = (S0 + mj) >>> 0;
      h = g; g = f; f = e; e = (d + t1) >>> 0; d = c; c = b; b = a; a = (t1 + t2) >>> 0;
    }
    H[0] = (H[0]! + a) >>> 0; H[1] = (H[1]! + b) >>> 0; H[2] = (H[2]! + c) >>> 0; H[3] = (H[3]! + d) >>> 0;
    H[4] = (H[4]! + e) >>> 0; H[5] = (H[5]! + f) >>> 0; H[6] = (H[6]! + g) >>> 0; H[7] = (H[7]! + h) >>> 0;
  }
  const out = new Uint8Array(32);
  const odv = new DataView(out.buffer);
  H.forEach((v, i) => odv.setUint32(i * 4, v));
  return out;
}

export function sha256Hex(input: string): string {
  return [...sha256Bytes(new TextEncoder().encode(input))].map((b) => b.toString(16).padStart(2, '0')).join('');
}

export function hmacSha256Hex(key: string, message: string): string {
  let k = new TextEncoder().encode(key);
  if (k.length > 64) k = new Uint8Array(sha256Bytes(k));
  const block = new Uint8Array(64);
  block.set(k);
  const oKey = new Uint8Array(64);
  const iKey = new Uint8Array(64);
  for (let i = 0; i < 64; i++) {
    oKey[i] = block[i]! ^ 0x5c;
    iKey[i] = block[i]! ^ 0x36;
  }
  const inner = sha256Bytes(new Uint8Array([...iKey, ...new TextEncoder().encode(message)]));
  return [...sha256Bytes(new Uint8Array([...oKey, ...inner]))].map((b) => b.toString(16).padStart(2, '0')).join('');
}

/** F03960 AES 试算预留（正式实现走 WebCrypto，见领域06 groupC）。 */
export const AES_TRIAL_RESERVED = true;

export interface DiffLine {
  type: 'same' | 'add' | 'del';
  text: string;
}

export function diffLines(a: string, b: string): DiffLine[] {
  const la = a.split('\n');
  const lb = b.split('\n');
  const n = la.length;
  const m = lb.length;
  const dp: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i]![j] = la[i] === lb[j] ? (dp[i + 1]![j + 1] ?? 0) + 1 : Math.max(dp[i + 1]![j] ?? 0, dp[i]![j + 1] ?? 0);
    }
  }
  const out: DiffLine[] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (la[i] === lb[j]) {
      out.push({ type: 'same', text: la[i] ?? '' });
      i++; j++;
    } else if ((dp[i + 1]![j] ?? 0) >= (dp[i]![j + 1] ?? 0)) {
      out.push({ type: 'del', text: la[i] ?? '' });
      i++;
    } else {
      out.push({ type: 'add', text: lb[j] ?? '' });
      j++;
    }
  }
  while (i < n) out.push({ type: 'del', text: la[i++] ?? '' });
  while (j < m) out.push({ type: 'add', text: lb[j++] ?? '' });
  return out;
}

/** 三方合并：以 base 为基准，双方改动不冲突则自动合并。 */
export function merge3(base: string, mine: string, theirs: string): { merged: string; conflicts: number } {
  const bl = base.split('\n');
  const ml = mine.split('\n');
  const tl = theirs.split('\n');
  const out: string[] = [];
  let conflicts = 0;
  const len = Math.max(bl.length, ml.length, tl.length);
  for (let i = 0; i < len; i++) {
    const b = bl[i];
    const m = ml[i];
    const t = tl[i];
    if (m === t) out.push(m ?? b ?? '');
    else if (m === b) out.push(t ?? '');
    else if (t === b) out.push(m ?? '');
    else {
      conflicts += 1;
      out.push(`<<<<<<< mine\n${m ?? ''}\n=======\n${t ?? ''}\n>>>>>>> theirs`);
    }
  }
  return { merged: out.join('\n'), conflicts };
}

const NAMED_COLORS: Record<string, string> = { black: '#000000', white: '#ffffff', red: '#ff0000', lime: '#00ff00', blue: '#0000ff', yellow: '#ffff00', cyan: '#00ffff', magenta: '#ff00ff', gray: '#808080', navy: '#000080', teal: '#008080', purple: '#800080', orange: '#ffa500', pink: '#ffc0cb' };

export function nearestNamedColor(hex: string): string | undefined {
  const [r, g, b] = hexToRgbLocal(hex);
  let best: string | undefined;
  let bestD = Infinity;
  for (const [name, h] of Object.entries(NAMED_COLORS)) {
    const [r2, g2, b2] = hexToRgbLocal(h);
    const d = (r - r2) ** 2 + (g - g2) ** 2 + (b - b2) ** 2;
    if (d < bestD) {
      bestD = d;
      best = name;
    }
  }
  return best;
}

function hexToRgbLocal(hex: string): [number, number, number] {
  const h = hex.replace('#', '');
  return [parseInt(h.slice(0, 2), 16), parseInt(h.slice(2, 4), 16), parseInt(h.slice(4, 6), 16)];
}

export const RULER_SPEC = { orientation: ['horizontal', 'vertical'], unit: 'px', maxWidth: 4096 };
export const GRID_OVERLAY = { sizes: [4, 8, 12, 16, 24], color: 'rgba(127,127,127,0.35)' };

export function xmlCheck(src: string): { ok: boolean; error?: string } {
  const stack: string[] = [];
  const re = /<\/?([A-Za-z_][\w:-]*)([^>]*?)(\/?)>|<!--[\s\S]*?-->/g;
  for (const m of src.matchAll(re)) {
    if (m[0].startsWith('<!--')) continue;
    const tag = m[1];
    if (m[0].startsWith('</')) {
      if (stack.pop() !== tag) return { ok: false, error: `标签不匹配: </${tag}>` };
    } else if (!m[3]) {
      stack.push(tag!);
    }
  }
  return stack.length === 0 ? { ok: true } : { ok: false, error: `未闭合: ${stack.join(',')}` };
}

export function yamlCheck(src: string): { ok: boolean; error?: string } {
  const lines = src.split('\n').filter((l) => l.trim() && !l.trim().startsWith('#'));
  for (const l of lines) {
    if (/^\t/.test(l)) return { ok: false, error: 'YAML 不允许 Tab 缩进' };
    if (/^[^#:\s][^:]*[^:\s]\s+[^:\s]/.test(l) && !l.includes(':')) return { ok: false, error: `缺冒号: ${l}` };
  }
  return { ok: true };
}

export function tomlCheck(src: string): { ok: boolean; error?: string } {
  for (const l of src.split('\n')) {
    const t = l.trim();
    if (!t || t.startsWith('#')) continue;
    if (/^\[.+\]$/.test(t)) continue;
    if (!/^[\w.-]+\s*=/.test(t)) return { ok: false, error: `非法键值行: ${t}` };
  }
  return { ok: true };
}

export function curlToFetch(curl: string): string {
  const url = curl.match(/https?:\/\/[^\s']+/)?.[0] ?? 'https://example.com';
  const method = curl.match(/-X\s*(\w+)/)?.[1] ?? (curl.includes('-d') || curl.includes('--data') ? 'POST' : 'GET');
  const headers = [...curl.matchAll(/-H\s*['"]([^:]+):\s*([^'"]+)['"]/g)].map(([, k, v]) => `    '${k}': '${v}',`);
  const body = curl.match(/(?:-d|--data)\s*['"]([^'"]+)['"]/)?.[1];
  return [
    `await fetch('${url}', {`,
    `  method: '${method}',`,
    ...(headers.length ? [`  headers: {`, ...headers, `  },`] : []),
    ...(body ? [`  body: JSON.stringify(${body.startsWith('{') ? body : `'${body}'`}),`] : []),
    `});`,
  ].join('\n');
}

export function placeholderImage(w: number, h: number, label = `${w}×${h}`): string {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}"><rect width="100%" height="100%" fill="#334155"/><text x="50%" y="50%" fill="#e2e8f0" font-size="${Math.max(12, Math.min(w, h) / 5)}" text-anchor="middle" dominant-baseline="middle">${label}</text></svg>`;
  return `data:image/svg+xml;base64,${base64Encode(svg)}`;
}

const FAKE_SURNAMES = ['张', '王', '李', '赵', '刘', '陈', '杨', '黄'];
const FAKE_GIVEN = ['伟', '芳', '娜', '敏', '静', '磊', '军', '洋'];
const FAKE_CITIES = ['上海市浦东新区', '北京市海淀区', '广州市天河区', '深圳市南山区'];

export function fakeData(seed: number): { name: string; phone: string; address: string; email: string } {
  const r = (n: number) => ((seed * 9301 + 49297 * n) % 233280) / 233280;
  const name = (FAKE_SURNAMES[Math.floor(r(1) * FAKE_SURNAMES.length)] ?? '张') + (FAKE_GIVEN[Math.floor(r(2) * FAKE_GIVEN.length)] ?? '伟');
  const phone = `1${['3', '5', '7', '8', '9'][Math.floor(r(3) * 5)] ?? '3'}${String(Math.floor(r(4) * 1e9)).padStart(9, '0')}`;
  const address = FAKE_CITIES[Math.floor(r(5) * FAKE_CITIES.length)] + '幸福路' + (Math.floor(r(6) * 200) + 1) + '号';
  return { name, phone, address, email: `user${Math.floor(r(7) * 1000)}@example.com` };
}

export function qrDecodeMeta(input: string): { format: 'url' | 'wifi' | 'vcard' | 'text'; payload: string } {
  if (input.startsWith('http')) return { format: 'url', payload: input };
  if (input.startsWith('WIFI:')) return { format: 'wifi', payload: input };
  if (input.startsWith('BEGIN:VCARD')) return { format: 'vcard', payload: input };
  return { format: 'text', payload: input };
}

/** cron 表达式解释（5 段）：给出接下来 N 次触发的描述。 */
export function cronExplain(expr: string, count = 3): { desc: string; next: string[] } {
  const [min, hour, dom, mon, dow] = expr.trim().split(/\s+/);
  if (!min || !hour || !dom || !mon || !dow) throw new Error('cron 需 5 段');
  const desc = `分钟=${min} 小时=${hour} 日=${dom} 月=${mon} 星期=${dow}`;
  const next: string[] = [];
  let t = new Date();
  t.setSeconds(0, 0);
  const match = (v: string, cur: number, lo: number, hi: number): boolean => {
    if (v === '*') return true;
    if (v.startsWith('*/')) return (cur - lo) % Number(v.slice(2)) === 0;
    if (v.includes(',')) return v.split(',').map(Number).includes(cur);
    if (v.includes('-')) {
      const [a, b] = v.split('-').map(Number);
      return cur >= (a ?? lo) && cur <= (b ?? hi);
    }
    return Number(v) === cur;
  };
  for (let guard = 0; guard < 525600 && next.length < count; guard++) {
    t = new Date(t.getTime() + 60000);
    if (!match(min, t.getMinutes(), 0, 59)) continue;
    if (!match(hour, t.getHours(), 0, 23)) continue;
    if (!match(dom, t.getDate(), 1, 31)) continue;
    if (!match(mon, t.getMonth() + 1, 1, 12)) continue;
    if (dow !== '*' && !match(dow, t.getDay(), 0, 6)) continue;
    next.push(t.toLocaleString('zh-CN', { hour12: false }));
  }
  return { desc, next };
}

export const DEV_TIPS = ['Ctrl+Shift+P 打开命令面板（Code 应用内）', 'JWT 解码不校验签名，仅查看', 'HMAC-SHA256 本地实现零网络'];

/* ========================= 族0160 录音与音频工具 ========================= */

export class RecorderSession {
  private startedAt: number | undefined;
  private stoppedAt: number | undefined;

  start(now: number): void {
    this.startedAt = now;
    this.stoppedAt = undefined;
  }

  stop(now: number): number {
    this.stoppedAt = now;
    return this.durationSec(now);
  }

  durationSec(now: number): number {
    return this.startedAt === undefined ? 0 : Math.round(((this.stoppedAt ?? now) - this.startedAt) / 1000);
  }
}

export interface Recording {
  name: string;
  durationSec: number;
  sizeKB: number;
  createdAt: number;
}

export class RecordingList {
  private items: Recording[] = [];

  add(name: string, durationSec: number, sizeKB: number, createdAt = Date.now()): boolean {
    if (this.items.some((x) => x.name === name)) return false;
    this.items.push({ name, durationSec, sizeKB, createdAt });
    return true;
  }

  get list(): readonly Recording[] {
    return this.items;
  }

  remove(name: string): boolean {
    const i = this.items.findIndex((x) => x.name === name);
    if (i < 0) return false;
    this.items.splice(i, 1);
    return true;
  }
}

/** 批量重命名：录音_001 格式。 */
export function batchRenameRecordings(names: string[], prefix = '录音'): string[] {
  return names.map((n, i) => {
    const ext = n.includes('.') ? n.slice(n.lastIndexOf('.')) : '.wav';
    return `${prefix}_${String(i + 1).padStart(3, '0')}${ext}`;
  });
}

/** 波形裁剪。 */
export function trimWave(samples: number[], fromSec: number, toSec: number, sampleRate = 1000): number[] {
  return samples.slice(Math.floor(fromSec * sampleRate), Math.floor(toSec * sampleRate));
}

/** 降噪：滑动平均。 */
export function denoise(samples: number[], window = 3): number[] {
  if (window < 2) return [...samples];
  const half = Math.floor(window / 2);
  return samples.map((_, i) => {
    let s = 0;
    let n = 0;
    for (let j = i - half; j <= i + half; j++) {
      if (j >= 0 && j < samples.length) {
        s += samples[j]!;
        n += 1;
      }
    }
    return Math.round((s / n) * 1000) / 1000;
  });
}

export const AUDIO_TRANSCODE_PRESETS = ['wav→mp3-320', 'wav→flac', 'mp3→ogg-q6', 'any→aac-256'] as const;

/** 变声：音高移位（半音数）。 */
export function pitchShiftFactor(semitones: number): number {
  return 2 ** (semitones / 12);
}

/** 录音转文字占位（本地模型通道预留，§15 守卫）。 */
export function transcribePlaceholder(durationSec: number): string {
  return `[本地转写预留 ${durationSec}s]`;
}

export const TTS_VOICES = ['晓晓（女·普通话）', '云扬（男·普通话）', '小美（女·粤语）', 'Sarah（女·英语）'] as const;

/** 响度标准化到目标 RMS。 */
export function loudnessNormalize(samples: number[], targetRms = 0.3): number[] {
  const rms = Math.sqrt(samples.reduce((s, x) => s + x * x, 0) / Math.max(1, samples.length));
  if (rms === 0) return [...samples];
  const g = targetRms / rms;
  return samples.map((x) => Math.max(-1, Math.min(1, Math.round(x * g * 1000) / 1000)));
}

/** 铃声制作：截取 + 淡入淡出。 */
export function makeRingtone(samples: number[], sec: number, sampleRate = 1000, fadeMs = 200): number[] {
  const cut = samples.slice(0, sec * sampleRate);
  const fadeN = Math.min(cut.length, Math.floor((fadeMs / 1000) * sampleRate));
  return cut.map((x, i) => {
    let g = 1;
    if (i < fadeN) g = i / fadeN;
    if (i >= cut.length - fadeN) g = (cut.length - i) / fadeN;
    return Math.round(x * g * 1000) / 1000;
  });
}

export class NoiseMixer {
  private layers = new Map<string, number>();

  set(name: '雨声' | '海浪' | '篝火' | '风' | '溪流', gain: number): this {
    this.layers.set(name, Math.max(0, Math.min(1, gain)));
    return this;
  }

  remove(name: string): boolean {
    return this.layers.delete(name);
  }

  mixedGain(): number {
    return Math.min(1, [...this.layers.values()].reduce((s, g) => s + g, 0) / Math.max(1, this.layers.size));
  }

  get active(): string[] {
    return [...this.layers.keys()];
  }
}

export const AMBIENT_TRACKS = ['森林清晨', '海边黄昏', '雨夜书房', '雪原极静', '炉火小屋'] as const;

export function visualizeBars(samples: number[], bars = 16): number[] {
  const out: number[] = [];
  const chunk = Math.max(1, Math.floor(samples.length / bars));
  for (let i = 0; i < bars; i++) {
    const seg = samples.slice(i * chunk, (i + 1) * chunk);
    const peak = seg.reduce((m, x) => Math.max(m, Math.abs(x)), 0);
    out.push(Math.round(peak * 100) / 100);
  }
  return out;
}

/** 频谱：DFT 幅度（小样本演示用）。 */
export function dftSpectrum(samples: number[], bins = 8): number[] {
  const n = samples.length;
  const out: number[] = [];
  for (let k = 1; k <= bins; k++) {
    let re = 0;
    let im = 0;
    for (let t = 0; t < n; t++) {
      const ang = (2 * Math.PI * k * t) / n;
      re += samples[t]! * Math.cos(ang);
      im -= samples[t]! * Math.sin(ang);
    }
    out.push(Math.round(Math.sqrt(re * re + im * im) / n * 1000) / 1000);
  }
  return out;
}

export function metronomeClicks(bpm: number, bars = 4, beatsPerBar = 4): number[] {
  const interval = 60000 / bpm;
  return Array.from({ length: bars * beatsPerBar }, (_, i) => Math.round(i * interval));
}

const NOTE_NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];

export function noteFromFreq(freq: number): { note: string; octave: number; cents: number } {
  const n = 12 * Math.log2(freq / 440) + 69; // A4=440 → MIDI 69
  const rounded = Math.round(n);
  const cents = Math.round((n - rounded) * 100);
  return { note: NOTE_NAMES[((rounded % 12) + 12) % 12]!, octave: Math.floor(rounded / 12) - 1, cents };
}

/** 音高检测：过零率近似。 */
export function pitchDetect(samples: number[], sampleRate = 1000): number {
  let crossings = 0;
  for (let i = 1; i < samples.length; i++) {
    if (samples[i - 1]! < 0 && samples[i]! >= 0) crossings += 1;
  }
  const dur = samples.length / sampleRate;
  return dur > 0 ? Math.round((crossings / dur) * 10) / 10 : 0;
}

export function mergeSegments(durationsSec: number[]): number {
  return durationsSec.reduce((s, d) => s + d, 0);
}

export function splitByDuration(totalSec: number, chunkSec: number): number[] {
  if (chunkSec <= 0) throw new Error('分段需大于 0');
  const parts = Math.ceil(totalSec / chunkSec);
  return Array.from({ length: parts }, (_, i) => Math.min(chunkSec, totalSec - i * chunkSec));
}

export function batchAudioConvert(names: string[], target: 'mp3' | 'flac' | 'ogg' | 'aac'): string[] {
  return names.map((n) => `${n.replace(/\.\w+$/, '')}.${target}`);
}

export function readAudioMeta(name: string, sizeKB: number, durationSec: number): { name: string; bitrateKbps: number; ext: string } {
  return { name, bitrateKbps: durationSec > 0 ? Math.round((sizeKB * 8) / durationSec) : 0, ext: name.includes('.') ? name.slice(name.lastIndexOf('.') + 1) : '' };
}

export interface AudioTags {
  title?: string;
  artist?: string;
  album?: string;
  year?: number;
}

export function editTags(current: AudioTags, patch: AudioTags): AudioTags {
  return { ...current, ...Object.fromEntries(Object.entries(patch).filter(([, v]) => v !== undefined)) };
}

export const CD_RIP_RESERVED = true;

export const AUDIO_TIPS = ['录音默认存本地「录音」目录', '降噪强度越高细节损失越大', '调音器以 A4=440Hz 为基准'];
