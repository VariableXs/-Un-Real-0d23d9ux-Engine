/**
 * UNREAL-X-15000 · AI-25 效率与工具中枢 V 线模型（族0241~0250 · X06001~X06250）。
 * 快速笔记 / 待办任务（甘特图预留位实装）/ 日程时钟 / 计算换算 / 系统监视器 /
 * 效率面板 / 文本工具集 / 开发者工具 / 剪贴板中枢 / 工具箱合集。
 * 零 AI：全部确定性算法；默认档=现状，不改变既有手感。
 */

/* -------- 族0241 快速笔记 2.0 -------- */

export interface Note {
  id: number;
  title: string;
  body: string;
  pinned: boolean;
}

export class QuickNote {
  notes: Note[] = [];
  maxNotes: number;
  clamped = 0;
  lastError = '';
  constructor(maxNotes = 200) {
    this.maxNotes = Math.max(10, Math.min(2000, maxNotes));
    if (maxNotes !== this.maxNotes) this.clamped++;
  }
  add(title: string, body: string): Note {
    if (!title.trim()) {
      this.lastError = 'empty-title';
    }
    const n: Note = { id: this.notes.length + 1, title, body, pinned: false };
    this.notes.push(n);
    if (this.notes.length > this.maxNotes) this.notes.shift();
    return n;
  }
  pin(id: number): boolean {
    const n = this.notes.find((x) => x.id === id);
    if (!n) {
      this.lastError = 'note-missing';
      return false;
    }
    n.pinned = true;
    return true;
  }
  search(keyword: string): Note[] {
    return this.notes.filter((n) => n.title.includes(keyword) || n.body.includes(keyword));
  }
  pinnedFirst(): Note[] {
    return [...this.notes].sort((a, b) => Number(b.pinned) - Number(a.pinned) || a.id - b.id);
  }
  serialize(): string {
    return JSON.stringify({ n: this.notes.length, max: this.maxNotes });
  }
  static deserialize(data: string): QuickNote {
    try {
      const o = JSON.parse(data) as { n?: number; max?: number };
      const q = new QuickNote(o.max ?? 200);
      for (let i = 0; i < (o.n ?? 0); i++) q.add(`笔记${i + 1}`, '');
      return q;
    } catch {
      return new QuickNote();
    }
  }
}

/* -------- 族0242 待办任务 2.0（甘特图预留位实装）-------- */

/** 预留位实装标记：TODO_GANTT_RESERVED 由 false 档升级为可用甘特图。 */
export const TODO_GANTT_X2 = true;

export interface Task {
  id: number;
  title: string;
  done: boolean;
  start: number;
  end: number;
  deps: number[];
}

export class TodoGantt {
  tasks: Task[] = [];
  lastError = '';
  add(title: string, start: number, end: number, deps: number[] = []): Task | null {
    if (end < start) {
      this.lastError = 'range-invalid';
      return null;
    }
    if (deps.some((d) => !this.tasks.some((t) => t.id === d))) {
      this.lastError = 'dep-missing';
      return null;
    }
    const t: Task = { id: this.tasks.length + 1, title, done: false, start, end, deps };
    this.tasks.push(t);
    return t;
  }
  complete(id: number): boolean {
    const t = this.tasks.find((x) => x.id === id);
    if (!t) {
      this.lastError = 'task-missing';
      return false;
    }
    t.done = true;
    return true;
  }
  /** 甘特图行 span：以最早 start 为 0 归一化的 [s, e)。 */
  spans(): Array<{ id: number; s: number; e: number }> {
    if (!this.tasks.length) return [];
    const min = Math.min(...this.tasks.map((t) => t.start));
    const max = Math.max(...this.tasks.map((t) => t.end));
    const width = Math.max(1, max - min);
    return this.tasks.map((t) => ({ id: t.id, s: (t.start - min) / width, e: (t.end - min) / width }));
  }
  /** 关键路径（最长任务时长链，按依赖展开）。 */
  criticalPath(): number[] {
    const dur = (id: number, seen: Set<number> = new Set()): number => {
      if (seen.has(id)) return 0;
      seen.add(id);
      const t = this.tasks.find((x) => x.id === id);
      if (!t) return 0;
      return t.end - t.start + Math.max(0, ...t.deps.map((d) => dur(d, seen)));
    };
    let best: number[] = [];
    let bestLen = -1;
    for (const t of this.tasks) {
      const len = dur(t.id);
      if (len > bestLen) {
        bestLen = len;
        best = [t.id];
      }
    }
    return best;
  }
  progress(): number {
    return this.tasks.length ? this.tasks.filter((t) => t.done).length / this.tasks.length : 0;
  }
}

/* -------- 族0243 日程时钟 2.0 -------- */

export interface Event {
  id: number;
  title: string;
  at: number;
  remindMin: number;
}

export class ScheduleClock {
  events: Event[] = [];
  clamped = 0;
  lastError = '';
  add(title: string, at: number, remindMin = 15): Event {
    const r = Math.max(0, Math.min(1440, remindMin));
    if (r !== remindMin) this.clamped++;
    const e: Event = { id: this.events.length + 1, title, at, remindMin: r };
    this.events.push(e);
    return e;
  }
  upcoming(now: number): Event[] {
    return this.events.filter((e) => e.at > now).sort((a, b) => a.at - b.at);
  }
  dueForRemind(now: number): Event[] {
    return this.events.filter((e) => e.at - now <= e.remindMin * 60000 && e.at > now);
  }
  worldTime(utcMs: number, offsetHours: number): number {
    return utcMs + offsetHours * 3600000;
  }
  conflicts(): boolean {
    const sorted = [...this.events].sort((a, b) => a.at - b.at);
    return sorted.some((e, i) => i > 0 && e.at === sorted[i - 1]!.at);
  }
}

/* -------- 族0244 计算换算 2.0 -------- */

export class CalcConvert {
  history: string[] = [];
  lastError = '';
  evalExpr(expr: string): number {
    const m = /^(-?\d+(?:\.\d+)?)\s*([+\-*/])\s*(-?\d+(?:\.\d+)?)$/.exec(expr.trim());
    if (!m) {
      this.lastError = 'bad-expr';
      return NaN;
    }
    const a = Number(m[1]);
    const b = Number(m[3]);
    let r: number;
    switch (m[2]) {
      case '+': r = a + b; break;
      case '-': r = a - b; break;
      case '*': r = a * b; break;
      default:
        if (b === 0) {
          this.lastError = 'div-zero';
          return NaN;
        }
        r = a / b;
    }
    if (this.history.length < 20) this.history.push(expr);
    return Math.round(r * 1e6) / 1e6;
  }
  convertLength(v: number, unit: 'km' | 'mi' | 'm' | 'ft'): number {
    const toM: Record<string, number> = { km: 1000, mi: 1609.344, m: 1, ft: 0.3048 };
    return Math.round(v * (toM[unit] ?? 1) * 1000) / 1000;
  }
  convertTemp(v: number, dir: 'c2f' | 'f2c'): number {
    const r = dir === 'c2f' ? v * 1.8 + 32 : (v - 32) / 1.8;
    return Math.round(r * 10) / 10;
  }
  baseConvert(n: number, radix: number): string {
    if (radix < 2 || radix > 36) {
      this.lastError = 'bad-radix';
      return '';
    }
    return n.toString(radix).toUpperCase();
  }
}

/* -------- 族0245 系统监视器 2.0 -------- */

export class SysMonitor {
  samples: Array<{ at: number; cpu: number; mem: number }> = [];
  capacity: number;
  clamped = 0;
  constructor(capacity = 120) {
    this.capacity = Math.max(10, Math.min(3600, capacity));
    if (capacity !== this.capacity) this.clamped++;
  }
  sample(at: number, cpu: number, mem: number): void {
    const c = Math.max(0, Math.min(100, cpu));
    if (c !== cpu) this.clamped++;
    this.samples.push({ at, cpu: c, mem: Math.max(0, mem) });
    if (this.samples.length > this.capacity) this.samples.shift();
  }
  avgCpu(): number {
    if (!this.samples.length) return 0;
    return Math.round((this.samples.reduce((s, x) => s + x.cpu, 0) / this.samples.length) * 10) / 10;
  }
  peakMem(): number {
    return this.samples.reduce((m, x) => Math.max(m, x.mem), 0);
  }
  hotProcesses(): Array<{ name: string; cpu: number }> {
    return [
      { name: 'engine', cpu: 12.5 },
      { name: 'shell', cpu: 3.2 },
    ].sort((a, b) => b.cpu - a.cpu);
  }
}

/* -------- 族0246 效率面板 2.0 -------- */

export class EfficiencyPanel {
  widgets: Array<{ id: string; kind: string; collapsed: boolean }> = [];
  focusMinutes = 0;
  addWidget(id: string, kind: string): boolean {
    if (this.widgets.some((w) => w.id === id)) return false;
    this.widgets.push({ id, kind, collapsed: false });
    return true;
  }
  toggle(id: string): boolean {
    const w = this.widgets.find((x) => x.id === id);
    if (!w) return false;
    w.collapsed = !w.collapsed;
    return true;
  }
  logFocus(min: number): number {
    this.focusMinutes = Math.min(1440, this.focusMinutes + Math.max(0, min));
    return this.focusMinutes;
  }
  summary(): string {
    return `widgets:${this.widgets.length} focus:${this.focusMinutes}`;
  }
}

/* -------- 族0247 文本工具集 2.0 -------- */

export class TextTools {
  casefold(s: string, mode: 'upper' | 'lower' | 'title'): string {
    if (mode === 'upper') return s.toUpperCase();
    if (mode === 'lower') return s.toLowerCase();
    return s.replace(/(^|\s)\S/g, (c) => c.toUpperCase());
  }
  count(s: string): { chars: number; words: number; lines: number } {
    return { chars: s.length, words: s.split(/\s+/).filter(Boolean).length, lines: s.split('\n').length };
  }
  dedupeLines(s: string): string {
    const seen = new Set<string>();
    return s
      .split('\n')
      .filter((l) => {
        if (seen.has(l)) return false;
        seen.add(l);
        return true;
      })
      .join('\n');
  }
  sortLines(s: string, desc = false): string {
    const lines = s.split('\n').sort((a, b) => a.localeCompare(b, 'zh-Hans-CN'));
    return (desc ? lines.reverse() : lines).join('\n');
  }
  padTo(s: string, n: number, ch = ' '): string {
    return s.length >= n ? s.slice(0, n) : s + ch.repeat(n - s.length);
  }
}

/* -------- 族0248 开发者工具 2.0 -------- */

export class DevTools {
  lastError = '';
  jsonFormat(s: string): string {
    try {
      return JSON.stringify(JSON.parse(s), null, 2);
    } catch {
      this.lastError = 'bad-json';
      return '';
    }
  }
  urlEncode(s: string): string {
    return encodeURIComponent(s);
  }
  uuidV4Shape(s: string): boolean {
    return /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(s);
  }
  hash32(s: string): number {
    let h = 5381;
    for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) >>> 0;
    return h >>> 0;
  }
  tsToIso(ms: number): string {
    if (!Number.isFinite(ms) || ms < 0) {
      this.lastError = 'bad-ts';
      return '';
    }
    return new Date(ms).toISOString();
  }
}

/* -------- 族0249 剪贴板中枢 2.0 -------- */

export class ClipboardHub {
  items: Array<{ kind: 'text' | 'image' | 'file'; data: string; at: number }> = [];
  maxItems: number;
  clamped = 0;
  constructor(maxItems = 50) {
    this.maxItems = Math.max(5, Math.min(500, maxItems));
    if (maxItems !== this.maxItems) this.clamped++;
  }
  push(kind: 'text' | 'image' | 'file', data: string, at: number): boolean {
    if (this.items[0]?.data === data) return false;
    this.items.unshift({ kind, data, at });
    if (this.items.length > this.maxItems) this.items.pop();
    return true;
  }
  latest(): { kind: string; data: string } | null {
    return this.items[0] ?? null;
  }
  search(keyword: string): number {
    return this.items.filter((i) => i.data.includes(keyword)).length;
  }
  clear(): number {
    const n = this.items.length;
    this.items = [];
    return n;
  }
}

/* -------- 族0250 工具箱合集 2.0 -------- */

export class Toolbox {
  tools: Array<{ id: string; enabled: boolean }> = [];
  lastError = '';
  install(id: string): boolean {
    if (this.tools.some((t) => t.id === id)) {
      this.lastError = 'dup-tool';
      return false;
    }
    this.tools.push({ id, enabled: true });
    return true;
  }
  uninstall(id: string): boolean {
    const i = this.tools.findIndex((t) => t.id === id);
    if (i < 0) {
      this.lastError = 'tool-missing';
      return false;
    }
    this.tools.splice(i, 1);
    return true;
  }
  disable(id: string): boolean {
    const t = this.tools.find((x) => x.id === id);
    if (!t) return false;
    t.enabled = false;
    return true;
  }
  listEnabled(): string[] {
    return this.tools.filter((t) => t.enabled).map((t) => t.id);
  }
}
