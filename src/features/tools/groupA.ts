// AURORA-10000: AI-31 批次（领域07 效率与工具中枢）逻辑核，勿删。
// 族0151 剪贴板增强 / 族0152 快速笔记 / 族0153 待办与任务 / 族0154 日程与时钟 / 族0155 计算与换算。
// 全部纯函数/纯模型，不触网、不落盘，便于单测与后续接入桌面 IPC。

/* ========================= 族0151 剪贴板增强 ========================= */

export type SnippetKind = 'text' | 'rich' | 'code' | 'color' | 'image';

export interface Snippet {
  id: string;
  title: string;
  body: string;
  kind: SnippetKind;
  category: string;
  lang?: string;
  createdAt: number;
}

let snippetSeq = 0;

export class SnippetLib {
  private items: Snippet[] = [];

  add(s: { title: string; body: string; kind?: SnippetKind; category?: string; lang?: string }): Snippet | undefined {
    if (this.items.some((x) => x.title === s.title && x.body === s.body)) return undefined;
    snippetSeq += 1;
    const sn: Snippet = {
      id: `sn-${snippetSeq}`,
      title: s.title,
      body: s.body,
      kind: s.kind ?? 'text',
      category: s.category ?? '默认',
      lang: s.lang,
      createdAt: Date.now(),
    };
    this.items.push(sn);
    return sn;
  }

  get list(): readonly Snippet[] {
    return this.items;
  }

  remove(id: string): boolean {
    const i = this.items.findIndex((x) => x.id === id);
    if (i < 0) return false;
    this.items.splice(i, 1);
    return true;
  }

  categories(): string[] {
    return [...new Set(this.items.map((x) => x.category))];
  }

  byCategory(cat: string): Snippet[] {
    return this.items.filter((x) => x.category === cat);
  }

  search(q: string): Snippet[] {
    const k = q.trim().toLowerCase();
    if (!k) return [];
    return this.items.filter((x) => x.title.toLowerCase().includes(k) || x.body.toLowerCase().includes(k));
  }
}

/** 模板片段变量展开（动态片段：日期/时间/UUID）。 */
export function resolveDynamic(tpl: string, now = new Date()): string {
  const pad = (n: number, w = 2) => String(n).padStart(w, '0');
  const y = now.getFullYear();
  const m = pad(now.getMonth() + 1);
  const d = pad(now.getDate());
  const t = `${pad(now.getHours())}:${pad(now.getMinutes())}:${pad(now.getSeconds())}`;
  return tpl
    .replace(/\{\{year\}\}/g, String(y))
    .replace(/\{\{date\}\}/g, `${y}-${m}-${d}`)
    .replace(/\{\{time\}\}/g, t)
    .replace(/\{\{datetime\}\}/g, `${y}-${m}-${d} ${t}`)
    .replace(/\{\{uuid\}\}/g, uuid());
}

export function uuid(): string {
  const hex = '0123456789abcdef';
  const bytes = new Uint8Array(32);
  globalThis.crypto.getRandomValues(bytes);
  let out = '';
  let bi = 0;
  for (let i = 0; i < 36; i++) {
    if (i === 8 || i === 13 || i === 18 || i === 23) out += '-';
    else out += hex[bytes[bi++]! & 0xf];
  }
  return out;
}

/** F03751 贴图板：图片钉在桌面（位置 + 缩放）。 */
export interface PinnedImage {
  dataUrl: string;
  x: number;
  y: number;
  scale: number;
  opacity: number;
}

export function pinImage(dataUrl: string, x: number, y: number, scale = 1): PinnedImage {
  return { dataUrl, x, y, scale: scale > 0 ? scale : 1, opacity: 1 };
}

/** 跨设备同步预留（§15 守卫：接口冻结 + 开关存在，默认关）。 */
export const CLIP_SYNC_RESERVED = true;
export function setSyncEnabled(v: boolean): boolean {
  return v === true && CLIP_SYNC_RESERVED;
}

export const CROSS_DEVICE_RESERVED = true;

export function exportSnippets(lib: SnippetLib): string {
  return JSON.stringify({ v: 1, items: lib.list });
}

export function importSnippets(lib: SnippetLib, json: string): number {
  try {
    const parsed = JSON.parse(json) as { items?: Snippet[] };
    let n = 0;
    for (const it of parsed.items ?? []) {
      if (lib.add({ title: it.title, body: it.body, kind: it.kind, category: it.category, lang: it.lang })) n += 1;
    }
    return n;
  } catch {
    return 0;
  }
}

export function shareFile(lib: SnippetLib): string {
  return `VARIX-SNIPPETS:${btoaSafe(exportSnippets(lib))}`;
}

const B64_ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';

export function btoaSafe(s: string): string {
  const bytes = new TextEncoder().encode(s);
  let out = '';
  for (let i = 0; i < bytes.length; i += 3) {
    const b0 = bytes[i] ?? 0;
    const b1 = bytes[i + 1];
    const b2 = bytes[i + 2];
    out += B64_ALPHABET[b0 >> 2]!;
    out += b1 === undefined ? B64_ALPHABET[(b0 & 3) << 4]! + '==' : b2 === undefined ? B64_ALPHABET[((b0 & 3) << 4) | (b1 >> 4)]! + B64_ALPHABET[(b1 & 15) << 2]! + '=' : B64_ALPHABET[((b0 & 3) << 4) | (b1 >> 4)]! + B64_ALPHABET[((b1 & 15) << 2) | (b2 >> 6)]! + B64_ALPHABET[b2 & 63]!;
  }
  return out;
}

export function atobSafe(s: string): string {
  const clean = s.replace(/=+$/, '');
  const bytes: number[] = [];
  for (let i = 0; i < clean.length; i += 4) {
    const c0 = B64_ALPHABET.indexOf(clean[i] ?? 'A');
    const c1 = B64_ALPHABET.indexOf(clean[i + 1] ?? 'A');
    const c2 = B64_ALPHABET.indexOf(clean[i + 2] ?? 'A');
    const c3 = B64_ALPHABET.indexOf(clean[i + 3] ?? 'A');
    bytes.push((c0 << 2) | (c1 >> 4));
    if (c2 >= 0 && i + 2 < clean.length) bytes.push(((c1 & 15) << 4) | (c2 >> 2));
    if (c3 >= 0 && i + 3 < clean.length) bytes.push(((c2 & 3) << 6) | c3);
  }
  return new TextDecoder().decode(new Uint8Array(bytes));
}

/** OCR 贴：图片文字提取（本地 OCR 通道预留，入参即提取文本）。 */
export function ocrPaste(imageText: string): string {
  return imageText.replace(/\s+/g, ' ').trim();
}

/** 公式贴：LaTeX 简转纯文本位。 */
export function formulaPaste(latex: string): string {
  return latex
    .replace(/\\frac\{([^{}]*)\}\{([^{}]*)\}/g, '($1)/($2)')
    .replace(/\\sqrt\{([^{}]*)\}/g, '√($1)')
    .replace(/\\\^/g, '^')
    .replace(/[\\{}]/g, '');
}

/** 颜色贴：色值规范化（#abc / rgb() → #rrggbb）。 */
export function colorSnippet(input: string): string | undefined {
  const s = input.trim();
  if (/^#[0-9a-fA-F]{3}$/.test(s)) return `#${s.slice(1).split('').map((c) => c + c).join('')}`.toLowerCase();
  if (/^#[0-9a-fA-F]{6}$/.test(s)) return s.toLowerCase();
  const m = s.match(/^rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)/i);
  if (m) {
    const [, r, g, b] = m;
    return `#${[r, g, b].map((x) => Number(x).toString(16).padStart(2, '0')).join('')}`;
  }
  return undefined;
}

/** F03765 历史轮：快捷轮选历史（环形缓冲）。 */
export class ClipboardHistory {
  private ring: string[] = [];
  private cap: number;

  constructor(cap = 20) {
    this.cap = cap;
  }

  push(text: string): void {
    if (!text) return;
    this.ring = this.ring.filter((x) => x !== text);
    this.ring.unshift(text);
    if (this.ring.length > this.cap) this.ring.length = this.cap;
  }

  pick(i: number): string | undefined {
    return this.ring[i];
  }

  get size(): number {
    return this.ring.length;
  }
}

/** F03769/F03770 队列与批量贴。 */
export class PasteQueue {
  private q: string[] = [];

  push(text: string): void {
    if (text) this.q.push(text);
  }

  get pending(): number {
    return this.q.length;
  }

  pop(): string | undefined {
    return this.q.shift();
  }
}

export function batchPaste(texts: string[], sep = '\n'): string {
  return texts.filter((t) => t.length > 0).join(sep);
}

/** F03771 延迟贴：定时粘贴计划。 */
export function schedulePaste(text: string, at: number, now: number): { text: string; at: number; due: boolean } {
  return { text, at, due: at <= now };
}

/** F03772 贴撤销。 */
export class PasteUndo {
  private stack: string[] = [];

  push(prev: string): void {
    this.stack.push(prev);
  }

  undo(): string | undefined {
    return this.stack.pop();
  }
}

/** F03766 贴为图片：文本转图片规格。 */
export function textToImageSpec(text: string, fontSize = 16, maxCols = 40): { width: number; height: number; lines: number } {
  const rows = text.split('\n');
  const cols = Math.min(maxCols, ...rows.map((r) => [...r].length));
  return { width: Math.max(cols, 1) * fontSize, height: rows.length * (fontSize + 6), lines: rows.length };
}

/** F03767 粘贴预览。 */
export function previewPaste(text: string, maxChars = 120): string {
  return text.length <= maxChars ? text : `${text.slice(0, maxChars)}…（共 ${text.length} 字）`;
}

/** F03768 AI 整理：本地规则归类（不打网）。 */
export function aiOrganize(texts: string[]): Map<string, string[]> {
  const out = new Map<string, string[]>();
  for (const t of texts) {
    let cat = '普通文本';
    if (/https?:\/\//i.test(t)) cat = '链接';
    else if (/^[{}[\]]|^\s*(const|let|function|class|import|export)\b/m.test(t) || /;|\{\s*\w+:/m.test(t)) cat = '代码';
    else if (/^#?\w+@\w+\.\w+/.test(t)) cat = '邮箱';
    else if (/^1[3-9]\d{9}$/.test(t)) cat = '电话';
    else if (/^#[0-9a-fA-F]{3,8}$/.test(t)) cat = '颜色';
    const arr = out.get(cat) ?? [];
    arr.push(t);
    out.set(cat, arr);
  }
  return out;
}

export const CLIPBOARD_TIPS = ['Win+V 打开历史', '贴图板图片可拖动缩放', '模板片段支持 {{date}} 变量', '批量贴按队列顺序执行'];

/** F03775 彩蛋：输入上上下下触发。 */
export function clipboardEgg(input: string): string | undefined {
  return input === '上上下下' ? '🐧 剪贴板企鹅向你问好' : undefined;
}

/* ========================= 族0152 快速笔记 ========================= */

export const NOTE_COLORS = ['黄', '粉', '蓝', '绿', '紫', '灰'] as const;
export type NoteColor = (typeof NOTE_COLORS)[number];

export interface StickyNote {
  id: string;
  text: string;
  color: NoteColor;
  pinned: boolean;
  remindAt?: number;
  encrypted?: boolean;
  createdAt: number;
  versions: { text: string; time: number }[];
}

let noteSeq = 0;

function xorCipher(text: string, key: string): string {
  let h = 2166136261;
  for (const c of key) h = (h ^ c.charCodeAt(0)) >>> 0;
  const k = String(h);
  return [...text].map((c, i) => String.fromCharCode(c.charCodeAt(0) ^ k.charCodeAt(i % k.length))).join('');
}

export class StickyBoard {
  private notes: StickyNote[] = [];
  private trash: StickyNote[] = [];
  private keys = new Map<string, string>();

  add(text: string, opts: { color?: NoteColor; pinned?: boolean; remindAt?: number; template?: string } = {}): StickyNote {
    noteSeq += 1;
    const body = opts.template ? `${opts.template}\n${text}` : text;
    const n: StickyNote = {
      id: `note-${noteSeq}`,
      text: body,
      color: opts.color ?? '黄',
      pinned: opts.pinned ?? false,
      remindAt: opts.remindAt,
      createdAt: Date.now(),
      versions: [],
    };
    this.notes.push(n);
    return n;
  }

  get list(): readonly StickyNote[] {
    return this.notes;
  }

  edit(id: string, text: string, now = Date.now()): boolean {
    const n = this.notes.find((x) => x.id === id);
    if (!n) return false;
    n.versions.push({ text: n.text, time: now });
    n.text = text;
    return true;
  }

  pin(id: string): boolean {
    const n = this.notes.find((x) => x.id === id);
    if (!n) return false;
    n.pinned = !n.pinned;
    return true;
  }

  /** 便签墙：置顶优先，其余按更新时间。 */
  wall(): StickyNote[] {
    return [...this.notes].sort((a, b) => Number(b.pinned) - Number(a.pinned) || b.createdAt - a.createdAt);
  }

  setRemind(id: string, at: number): boolean {
    const n = this.notes.find((x) => x.id === id);
    if (!n) return false;
    n.remindAt = at;
    return true;
  }

  encrypt(id: string, key: string): boolean {
    const n = this.notes.find((x) => x.id === id);
    if (!n || n.encrypted) return false;
    n.text = xorCipher(n.text, key);
    n.encrypted = true;
    this.keys.set(id, key);
    return true;
  }

  decrypt(id: string, key: string): string | undefined {
    const n = this.notes.find((x) => x.id === id);
    if (!n || !n.encrypted || this.keys.get(id) !== key) return undefined;
    n.text = xorCipher(n.text, key);
    n.encrypted = false;
    this.keys.delete(id);
    return n.text;
  }

  search(q: string): StickyNote[] {
    const k = q.trim().toLowerCase();
    return this.notes.filter((n) => !n.encrypted && n.text.toLowerCase().includes(k));
  }

  exportText(): string {
    return this.notes.map((n) => `【${n.color}】${n.text}`).join('\n---\n');
  }

  toTodo(id: string): string {
    const n = this.notes.find((x) => x.id === id);
    return n ? n.text.split('\n')[0] ?? '' : '';
  }

  toSchedule(id: string): { title: string; date?: string } {
    const n = this.notes.find((x) => x.id === id);
    if (!n) return { title: '' };
    const m = n.text.match(/(\d{4}-\d{2}-\d{2})/);
    return { title: n.text.split('\n')[0] ?? '', date: m?.[1] };
  }

  history(id: string): { text: string; time: number }[] {
    return this.notes.find((x) => x.id === id)?.versions ?? [];
  }

  remove(id: string): boolean {
    const i = this.notes.findIndex((x) => x.id === id);
    if (i < 0) return false;
    const [gone] = this.notes.splice(i, 1);
    if (gone) this.trash.push(gone);
    return true;
  }

  restore(id: string): boolean {
    const i = this.trash.findIndex((x) => x.id === id);
    if (i < 0) return false;
    const [back] = this.trash.splice(i, 1);
    if (back) this.notes.push(back);
    return true;
  }

  stats(): { total: number; pinned: number; trashed: number; chars: number } {
    return {
      total: this.notes.length,
      pinned: this.notes.filter((n) => n.pinned).length,
      trashed: this.trash.length,
      chars: this.notes.reduce((s, n) => s + n.text.length, 0),
    };
  }
}

export const NOTE_TEMPLATES: Record<string, string> = {
  会议速记: '【会议速记】\n时间：\n参会：\n议题：\n结论：\n待办：',
  灵感捕捉: '【灵感】',
  读书: '【读书】书名：\n摘录：\n感想：',
};

/** F03791 倒计时贴。 */
export function countdownDays(target: number, now: number): number {
  return Math.ceil((target - now) / 86400_000);
}

/** F03792 今日焦点贴：今天到期的提醒。 */
export function todayFocus(notes: StickyNote[], now: number): StickyNote[] {
  const dayEnd = now + 86400_000;
  return notes.filter((n) => n.remindAt !== undefined && n.remindAt >= now && n.remindAt < dayEnd);
}

/** F03786 语音便签：本地转写占位（§15 守卫：接口冻结）。 */
export function voiceNotePlaceholder(durationSec: number): { durationSec: number; transcript: string } {
  return { durationSec, transcript: `[语音便签 ${durationSec}s 待本地转写]` };
}

export const NOTE_TIPS = ['全局热键一键速记', '置顶便签常驻便签墙首位', '加密便签搜索不命中'];

/** §15 守卫：手写便签/共享便签为接口冻结预留。 */
export const HANDWRITING_NOTE_RESERVED = true;
export const NOTE_SHARING_RESERVED = true;

/* ========================= 族0153 待办与任务 ========================= */

export type Priority = 0 | 1 | 2 | 3;
export const PRIORITY_LABELS: Record<Priority, string> = { 0: '无', 1: '低', 2: '中', 3: '高' };
export type RepeatRule = 'daily' | 'weekly' | 'monthly';

export interface Todo {
  id: string;
  title: string;
  project?: string;
  tags: string[];
  priority: Priority;
  due?: number;
  repeat?: RepeatRule;
  done: boolean;
  subtasks: { id: string; title: string; done: boolean }[];
  pomodoros: number;
  createdAt: number;
}

let todoSeq = 0;

/** F03801 自然语言解析：「明天3点开会」「今天下午3点半买牛奶」「重要 下周一交报告」。 */
export function parseNatural(input: string, now = new Date()): { title: string; due?: number; priority?: Priority } {
  let title = input.trim();
  let priority: Priority | undefined;
  if (/重要|紧急/.test(title)) priority = 3;
  title = title.replace(/重要|紧急/g, '').trim();

  const dayWord: Record<string, number> = { 今天: 0, 明天: 1, 后天: 2, 大后天: 3 };
  let base = new Date(now);
  let matched = false;
  for (const [w, add] of Object.entries(dayWord)) {
    if (title.includes(w)) {
      base = new Date(now.getFullYear(), now.getMonth(), now.getDate() + add);
      title = title.replace(w, '');
      matched = true;
      break;
    }
  }
  const week = title.match(/(下?)(?:周|星期)([一二三四五六日天])/);
  if (week) {
    const map: Record<string, number> = { 一: 1, 二: 2, 三: 3, 四: 4, 五: 5, 六: 6, 日: 0, 天: 0 };
    const target = map[week[2] ?? '一'] ?? 1;
    const cur = now.getDay();
    let delta = (target - cur + 7) % 7;
    if (delta === 0) delta = 7;
    if (week[1] === '下') delta += 7;
    base = new Date(now.getFullYear(), now.getMonth(), now.getDate() + delta);
    title = title.replace(/下?(?:周|星期)[一二三四五六日天]/, '');
    matched = true;
  }

  let hour = -1;
  let min = 0;
  const hm = title.match(/(\d{1,2})[:：点](\d{1,2})?分?/);
  if (hm) {
    hour = Number(hm[1]);
    min = Number(hm[2] ?? 0);
  }
  const half = title.match(/(上午|下午|晚上)?(\d{1,2})点半/);
  if (half) {
    hour = Number(half[2]) + (half[1] === '下午' || half[1] === '晚上' ? 12 : 0);
    min = 30;
  }
  if (hour >= 0) {
    title = title.replace(/(上午|下午|晚上)?\d{1,2}[:：点]\d{0,2}分?/, '').replace(/点半/, '');
  }
  title = title.replace(/\s+/g, ' ').trim() || input.trim();

  let due: number | undefined;
  if (matched || hour >= 0) {
    const d = new Date(base.getFullYear(), base.getMonth(), base.getDate(), hour >= 0 ? hour : 9, min);
    due = d.getTime();
  }
  return { title, due, priority };
}

export type Quadrant = 'doFirst' | 'schedule' | 'delegate' | 'drop';

export class TodoStore {
  private todos: Todo[] = [];

  addTodo(raw: string, now = Date.now()): Todo {
    const p = parseNatural(raw, new Date(now));
    todoSeq += 1;
    const t: Todo = {
      id: `td-${todoSeq}`,
      title: p.title,
      tags: [],
      priority: p.priority ?? 1,
      due: p.due,
      done: false,
      subtasks: [],
      pomodoros: 0,
      createdAt: now,
    };
    this.todos.push(t);
    return t;
  }

  get all(): readonly Todo[] {
    return this.todos;
  }

  get inbox(): Todo[] {
    return this.todos.filter((t) => !t.project);
  }

  promote(id: string, project: string): boolean {
    const t = this.todos.find((x) => x.id === id);
    if (!t) return false;
    t.project = project;
    return true;
  }

  setTags(id: string, tags: string[]): boolean {
    const t = this.todos.find((x) => x.id === id);
    if (!t) return false;
    t.tags = tags;
    return true;
  }

  setPriority(id: string, p: Priority): boolean {
    const t = this.todos.find((x) => x.id === id);
    if (!t) return false;
    t.priority = p;
    return true;
  }

  toggleDone(id: string, celebrate = true): boolean {
    const t = this.todos.find((x) => x.id === id);
    if (!t) return false;
    t.done = !t.done;
    return celebrate && t.done;
  }

  addSubtask(id: string, title: string): boolean {
    const t = this.todos.find((x) => x.id === id);
    if (!t) return false;
    t.subtasks.push({ id: `${t.id}-s${t.subtasks.length + 1}`, title, done: false });
    return true;
  }

  completeSubtask(id: string, subId: string): boolean {
    const sub = this.todos.find((x) => x.id === id)?.subtasks.find((s) => s.id === subId);
    if (!sub) return false;
    sub.done = true;
    return true;
  }

  /** F03811 拖拽排序。 */
  reorder(id: string, targetIndex: number): boolean {
    const i = this.todos.findIndex((x) => x.id === id);
    if (i < 0) return false;
    const [t] = this.todos.splice(i, 1);
    if (!t) return false;
    this.todos.splice(Math.max(0, Math.min(targetIndex, this.todos.length)), 0, t);
    return true;
  }

  byProject(project: string): Todo[] {
    return this.todos.filter((t) => t.project === project);
  }

  today(now: number): Todo[] {
    const end = new Date(now);
    end.setHours(23, 59, 59, 999);
    return this.todos.filter((t) => t.due !== undefined && t.due <= end.getTime() && !t.done);
  }

  planned(now: number): Todo[] {
    const start = new Date(now);
    start.setHours(23, 59, 59, 999);
    return this.todos.filter((t) => t.due !== undefined && t.due > start.getTime() && !t.done).sort((a, b) => (a.due ?? 0) - (b.due ?? 0));
  }

  /** F03812 四象限：重要=优先级≥2，紧急=24h 内到期。 */
  quadrant(id: string, now = Date.now()): Quadrant {
    const t = this.todos.find((x) => x.id === id);
    if (!t) return 'drop';
    const important = t.priority >= 2;
    const urgent = t.due !== undefined && t.due - now <= 86400_000;
    if (important && urgent) return 'doFirst';
    if (important) return 'schedule';
    if (urgent) return 'delegate';
    return 'drop';
  }

  kanban(): { todo: Todo[]; doing: Todo[]; done: Todo[] } {
    return {
      todo: this.todos.filter((t) => !t.done && t.pomodoros === 0),
      doing: this.todos.filter((t) => !t.done && t.pomodoros > 0),
      done: this.todos.filter((t) => t.done),
    };
  }

  calendar(): Map<string, Todo[]> {
    const out = new Map<string, Todo[]>();
    const key = (d: number) => {
      const x = new Date(d);
      return `${x.getFullYear()}-${String(x.getMonth() + 1).padStart(2, '0')}-${String(x.getDate()).padStart(2, '0')}`;
    };
    for (const t of this.todos) {
      if (t.due === undefined) continue;
      const k = key(t.due);
      const arr = out.get(k) ?? [];
      arr.push(t);
      out.set(k, arr);
    }
    return out;
  }

  bindPomodoro(id: string, n = 1): boolean {
    const t = this.todos.find((x) => x.id === id);
    if (!t) return false;
    t.pomodoros += n;
    return true;
  }

  focusStats(): { bound: number; pomodoros: number } {
    const bound = this.todos.filter((t) => t.pomodoros > 0).length;
    return { bound, pomodoros: this.todos.reduce((s, t) => s + t.pomodoros, 0) };
  }

  /** F03808 到期提醒（未来窗口内）。 */
  dueSoon(windowMs: number, now = Date.now()): Todo[] {
    return this.todos.filter((t) => !t.done && t.due !== undefined && t.due > now && t.due - now <= windowMs);
  }

  /** F03809 重复任务下一次到期。 */
  nextRepeat(t: Todo, now = Date.now()): number | undefined {
    if (t.due === undefined || !t.repeat) return undefined;
    const d = new Date(t.due);
    if (t.repeat === 'daily') d.setDate(d.getDate() + 1);
    else if (t.repeat === 'weekly') d.setDate(d.getDate() + 7);
    else d.setMonth(d.getMonth() + 1);
    return d.getTime() > now ? d.getTime() : this.nextRepeat({ ...t, due: d.getTime() }, now);
  }

  batchOp(ids: string[], op: 'done' | 'delete' | 'tag', tag = ''): number {
    let n = 0;
    for (const id of ids) {
      const t = this.todos.find((x) => x.id === id);
      if (!t) continue;
      if (op === 'done') t.done = true;
      else if (op === 'delete') this.todos = this.todos.filter((x) => x.id !== id);
      else t.tags = [...new Set([...t.tags, tag])].filter((x) => x);
      n += 1;
    }
    return n;
  }

  importJson(json: string): number {
    try {
      const arr = JSON.parse(json) as Partial<Todo>[];
      let n = 0;
      for (const it of arr) {
        if (!it.title) continue;
        todoSeq += 1;
        this.todos.push({
          id: `td-${todoSeq}`,
          title: it.title,
          tags: it.tags ?? [],
          priority: (it.priority ?? 1) as Priority,
          due: it.due,
          done: it.done ?? false,
          subtasks: [],
          pomodoros: 0,
          createdAt: Date.now(),
        });
        n += 1;
      }
      return n;
    } catch {
      return 0;
    }
  }

  importCsv(csv: string): number {
    return this.importJson(
      JSON.stringify(
        csv
          .trim()
          .split('\n')
          .slice(1)
          .map((line) => {
            const [title, priority] = line.split(',');
            return { title, priority: Number(priority ?? 1) };
          }),
      ),
    );
  }

  exportJson(): string {
    return JSON.stringify(this.todos.map(({ id, title, project, tags, priority, due, repeat, done }) => ({ id, title, project, tags, priority, due, repeat, done })));
  }

  exportCsv(): string {
    return ['title,priority,done', ...this.todos.map((t) => `${t.title},${t.priority},${t.done}`)].join('\n');
  }

  backup(): string {
    return JSON.stringify({ v: 1, todos: this.todos });
  }
}

export const TODO_TIPS = ['输入「明天3点开会」自动识别时间', 'Ctrl+Enter 完成并庆祝', '四象限快捷分配优先级'];

/** §15 守卫：甘特视图为预留位。 */
export const TODO_GANTT_RESERVED = true;

/* ========================= 族0154 日程与时钟 ========================= */

export interface CalEvent {
  id: string;
  title: string;
  date: string; // yyyy-mm-dd
  startMin?: number; // 0..1440
  endMin?: number;
  allDay?: boolean;
  tzOffsetMin?: number;
  repeat?: RepeatRule;
  remindMinBefore?: number;
  birthday?: { name: string; md: string };
}

let evSeq = 0;

export function dateKey(d: Date): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

/** F03826 月视图：6×7 网格（含前后月补位，返回 day 数字，非本月为 null）。 */
export function monthGrid(year: number, month0: number): (number | null)[][] {
  const first = new Date(year, month0, 1);
  const startOffset = first.getDay();
  const daysInMonth = new Date(year, month0 + 1, 0).getDate();
  const cells: (number | null)[] = [];
  const prevDays = new Date(year, month0, 0).getDate();
  for (let i = 0; i < startOffset; i++) cells.push(null);
  for (let d = 1; d <= daysInMonth; d++) cells.push(d);
  while (cells.length % 7 !== 0) cells.push(null);
  const weeks: (number | null)[][] = [];
  for (let i = 0; i < cells.length; i += 7) weeks.push(cells.slice(i, i + 7));
  void prevDays;
  return weeks;
}

export function weekDays(anchor: Date): Date[] {
  const start = new Date(anchor.getFullYear(), anchor.getMonth(), anchor.getDate() - anchor.getDay());
  return Array.from({ length: 7 }, (_, i) => new Date(start.getFullYear(), start.getMonth(), start.getDate() + i));
}

export class EventStore {
  private events: CalEvent[] = [];

  add(e: Omit<CalEvent, 'id'>): CalEvent {
    evSeq += 1;
    const ev: CalEvent = { id: `ev-${evSeq}`, ...e };
    this.events.push(ev);
    return ev;
  }

  get list(): readonly CalEvent[] {
    return this.events;
  }

  byDate(date: string): CalEvent[] {
    return this.events.filter((e) => e.date === date);
  }

  agenda(): CalEvent[] {
    return [...this.events].sort((a, b) => a.date.localeCompare(b.date) || (a.startMin ?? 0) - (b.startMin ?? 0));
  }

  /** 快加：「周五 14:00 例会」/「明天 全天 体检」。 */
  quickAdd(input: string, now = new Date()): CalEvent {
    let title = input;
    let date = dateKey(now);
    const week = input.match(/(下?)(?:周|星期)([一二三四五六日天])/);
    if (week) {
      const map: Record<string, number> = { 一: 1, 二: 2, 三: 3, 四: 4, 五: 5, 六: 6, 日: 0, 天: 0 };
      const target = map[week[2] ?? '一'] ?? 1;
      let delta = (target - now.getDay() + 7) % 7;
      if (delta === 0) delta = 7;
      if (week[1] === '下') delta += 7;
      date = dateKey(new Date(now.getFullYear(), now.getMonth(), now.getDate() + delta));
      title = title.replace(/下?(?:周|星期)[一二三四五六日天]\s*/, '');
    }
    const dayWord: Record<string, number> = { 今天: 0, 明天: 1, 后天: 2 };
    for (const [w, add] of Object.entries(dayWord)) {
      if (input.includes(w)) {
        date = dateKey(new Date(now.getFullYear(), now.getMonth(), now.getDate() + add));
        title = title.replace(w, '');
      }
    }
    let startMin: number | undefined;
    const hm = title.match(/(\d{1,2})[:：](\d{2})/);
    if (hm) startMin = Number(hm[1]) * 60 + Number(hm[2]);
    title = title.replace(/\d{1,2}[:：]\d{2}\s*/, '').trim();
    const allDay = /全天/.test(title);
    title = title.replace(/全天\s*/, '').trim();
    return this.add({ title, date, startMin: allDay ? undefined : startMin, allDay });
  }

  /** F03832 重复规则展开未来 N 次发生。 */
  occurrences(ev: CalEvent, limit: number): string[] {
    if (!ev.repeat) return [ev.date];
    const [y, m, d] = ev.date.split('-').map(Number);
    const out: string[] = [];
    let cur = new Date(y ?? 2026, (m ?? 1) - 1, d ?? 1);
    for (let i = 0; i < limit; i++) {
      out.push(dateKey(cur));
      if (ev.repeat === 'daily') cur.setDate(cur.getDate() + 1);
      else if (ev.repeat === 'weekly') cur.setDate(cur.getDate() + 7);
      else cur.setMonth(cur.getMonth() + 1);
    }
    return out;
  }
}

/** F03834 跨时区：按分钟偏移换算。 */
export function timezoneShift(isoLike: string, fromOffsetMin: number, toOffsetMin: number): string {
  const m = isoLike.match(/^(\d{4})-(\d{2})-(\d{2})[T ](\d{2}):(\d{2})/);
  if (!m) return isoLike;
  const [, ys, mos, ds, hs, mins] = m;
  let total = Number(hs) * 60 + Number(mins) - fromOffsetMin + toOffsetMin;
  let day = Number(ds);
  let month = Number(mos);
  let year = Number(ys);
  while (total < 0) {
    total += 1440;
    day -= 1;
  }
  while (total >= 1440) {
    total -= 1440;
    day += 1;
  }
  const dim = new Date(year, month, 0).getDate();
  if (day < 1) {
    month -= 1;
    if (month < 1) {
      month = 12;
      year -= 1;
    }
    day = new Date(year, month, 0).getDate();
  } else if (day > dim) {
    day -= dim;
    month += 1;
    if (month > 12) {
      month = 1;
      year += 1;
    }
  }
  return `${year}-${String(month).padStart(2, '0')}-${String(day).padStart(2, '0')} ${String(Math.floor(total / 60)).padStart(2, '0')}:${String(total % 60).padStart(2, '0')}`;
}

export const CN_HOLIDAYS_2026: Record<string, string> = {
  '2026-01-01': '元旦',
  '2026-02-17': '春节',
  '2026-04-05': '清明',
  '2026-05-01': '劳动节',
  '2026-06-19': '端午',
  '2026-09-25': '中秋',
  '2026-10-01': '国庆',
};

const SOLAR_TERMS_2026: Record<string, string> = {
  '02-04': '立春', '02-19': '雨水', '03-05': '惊蛰', '03-20': '春分', '04-05': '清明', '04-20': '谷雨',
  '05-05': '立夏', '05-21': '小满', '06-06': '芒种', '06-21': '夏至', '07-07': '小暑', '07-23': '大暑',
  '08-07': '立秋', '08-23': '处暑', '09-07': '白露', '09-23': '秋分', '10-08': '寒露', '10-23': '霜降',
  '11-07': '立冬', '11-22': '小雪', '12-07': '大雪', '12-22': '冬至', '01-05': '小寒', '01-20': '大寒',
};

export function solarTermOf(d: Date): string | undefined {
  return SOLAR_TERMS_2026[`${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`];
}

export function birthdayCountdown(md: string, now = new Date()): number {
  const [mm, dd] = md.split('-').map(Number);
  let next = new Date(now.getFullYear(), (mm ?? 1) - 1, dd ?? 1);
  if (next.getTime() < new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()) next = new Date(now.getFullYear() + 1, (mm ?? 1) - 1, dd ?? 1);
  return Math.ceil((next.getTime() - new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime()) / 86400_000);
}

export class CountdownWall {
  private items: { label: string; at: number }[] = [];

  add(label: string, at: number): this {
    this.items.push({ label, at });
    return this;
  }

  sorted(now: number): { label: string; at: number; days: number }[] {
    return [...this.items].sort((a, b) => a.at - b.at).map((x) => ({ ...x, days: countdownDays(x.at, now) }));
  }
}

export class Stopwatch {
  private startedAt: number | undefined;
  private laps: number[] = [];

  start(now: number): void {
    this.startedAt = now;
    this.laps = [];
  }

  lap(now: number): number | undefined {
    if (this.startedAt === undefined) return undefined;
    this.laps.push(now);
    return this.laps.length >= 2 ? now - (this.laps[this.laps.length - 2] ?? now) : now - this.startedAt;
  }

  elapsed(now: number): number {
    return this.startedAt === undefined ? 0 : now - this.startedAt;
  }
}

export class TimerBank {
  private timers: { name: string; remain: number }[] = [];

  add(name: string, seconds: number): this {
    this.timers.push({ name, remain: seconds });
    return this;
  }

  tick(sec: number): void {
    for (const t of this.timers) t.remain = Math.max(0, t.remain - sec);
  }

  expired(): string[] {
    return this.timers.filter((t) => t.remain === 0).map((t) => t.name);
  }

  get remainers(): { name: string; remain: number }[] {
    return [...this.timers];
  }
}

export class Pomodoro {
  phase: 'focus' | 'break' = 'focus';
  remainSec: number;
  completed = 0;
  focusMin: number;
  breakMin: number;

  constructor(focusMin = 25, breakMin = 5) {
    this.focusMin = focusMin;
    this.breakMin = breakMin;
    this.remainSec = focusMin * 60;
  }

  tick(sec: number): void {
    this.remainSec = Math.max(0, this.remainSec - sec);
    if (this.remainSec === 0) {
      if (this.phase === 'focus') {
        this.completed += 1;
        this.phase = 'break';
        this.remainSec = this.breakMin * 60;
      } else {
        this.phase = 'focus';
        this.remainSec = this.focusMin * 60;
      }
    }
  }
}

export function worldClock(now: Date, cities: { name: string; offsetMin: number }[]): { name: string; time: string }[] {
  const utcMin = now.getUTCHours() * 60 + now.getUTCMinutes();
  return cities.map((c) => {
    let t = (utcMin + c.offsetMin) % 1440;
    if (t < 0) t += 1440;
    return { name: c.name, time: `${String(Math.floor(t / 60)).padStart(2, '0')}:${String(t % 60).padStart(2, '0')}` };
  });
}

export class AlarmStore {
  private alarms: { hh: number; mm: number; label: string }[] = [];

  add(hhmm: string, label = '闹钟'): boolean {
    const m = hhmm.match(/^(\d{1,2}):(\d{2})$/);
    if (!m) return false;
    this.alarms.push({ hh: Number(m[1]), mm: Number(m[2]), label });
    return true;
  }

  due(nowMinutes: number): string[] {
    return this.alarms.filter((a) => a.hh * 60 + a.mm === nowMinutes).map((a) => a.label);
  }
}

export function hourlyChime(enabled: boolean): string {
  return enabled ? '整点报时开启' : '整点报时关闭';
}

export function napReminder(nowMinutes: number, start = 13 * 60, end = 14 * 60): boolean {
  return nowMinutes >= start && nowMinutes < end;
}

/** F03846 日落提醒：按纬度近似计算日落（分钟，正午对称）。 */
export function sunsetMinutes(latDeg: number, dayOfYear: number): number {
  const decl = 23.44 * Math.sin(((2 * Math.PI) / 365) * (dayOfYear - 81));
  const lat = (latDeg * Math.PI) / 180;
  const cosH = -Math.tan(lat) * Math.tan((decl * Math.PI) / 180);
  const h = Math.min(12, Math.max(0, (Math.acos(Math.max(-1, Math.min(1, cosH))) * 180) / Math.PI / 15));
  return Math.round(720 + h * 60);
}

export function icsImport(text: string): CalEvent[] {
  const out: CalEvent[] = [];
  const blocks = text.split('BEGIN:VEVENT').slice(1);
  for (const b of blocks) {
    const sum = b.match(/SUMMARY:(.+)/)?.[1]?.trim() ?? '（无标题）';
    const dt = b.match(/DTSTART[^:]*:(\d{4})(\d{2})(\d{2})/);
    if (!dt) continue;
    out.push({ id: `ics-${out.length + 1}`, title: sum, date: `${dt[1]}-${dt[2]}-${dt[3]}` });
  }
  return out;
}

export function icsExport(events: CalEvent[]): string {
  const lines = ['BEGIN:VCALENDAR', 'VERSION:2.0', 'PRODID:-//VARIX//AURORA//CN'];
  for (const e of events) {
    lines.push('BEGIN:VEVENT', `SUMMARY:${e.title}`, `DTSTART;VALUE=DATE:${e.date.replace(/-/g, '')}`, 'END:VEVENT');
  }
  lines.push('END:VCALENDAR');
  return lines.join('\r\n');
}

export const SCHEDULE_TIPS = ['月/周/日/议程四种视图随时切换', 'ics 导入导出兼容主流日历', '世界钟按 UTC 偏移换算'];

/** §15 守卫：日程分享为预留位。 */
export const SCHEDULE_SHARE_RESERVED = true;

/* ========================= 族0155 计算与换算 ========================= */

const FUNCS: Record<string, (...a: number[]) => number> = {
  sin: Math.sin, cos: Math.cos, tan: Math.tan, asin: Math.asin, acos: Math.acos, atan: Math.atan,
  sqrt: Math.sqrt, cbrt: Math.cbrt, log: Math.log10, ln: Math.log, log2: Math.log2, exp: Math.exp,
  abs: Math.abs, round: Math.round, floor: Math.floor, ceil: Math.ceil, sign: Math.sign,
  min: (...a) => Math.min(...a), max: (...a) => Math.max(...a), pow: (a, b) => a ** b,
};
const CONSTS: Record<string, number> = { pi: Math.PI, e: Math.E };

/** 科学计算器：递归下降表达式求值（含 ^ % 括号与函数）。 */
export function evalExpr(src: string): number {
  let i = 0;
  const s = src.replace(/\s+/g, '').toLowerCase();
  const peek = () => s[i];
  function expr(): number {
    let v = term();
    while (peek() === '+' || peek() === '-') {
      const op = s[i++];
      const r = term();
      v = op === '+' ? v + r : v - r;
    }
    return v;
  }
  function term(): number {
    let v = unary();
    while (peek() === '*' || peek() === '/' || peek() === '%') {
      const op = s[i++];
      const r = unary();
      if (op === '*') v *= r;
      else if (op === '/') v /= r;
      else v %= r;
    }
    return v;
  }
  function unary(): number {
    if (peek() === '-') {
      i++;
      return -unary();
    }
    return power();
  }
  function power(): number {
    const base = primary();
    if (peek() === '^') {
      i++;
      return base ** unary();
    }
    return base;
  }
  function primary(): number {
    if (peek() === '(') {
      i++;
      const v = expr();
      if (peek() === ')') i++;
      return v;
    }
    const numMatch = /^\d*\.?\d+(e[+-]?\d+)?/.exec(s.slice(i));
    if (numMatch) {
      i += numMatch[0].length;
      return Number(numMatch[0]);
    }
    const idMatch = /^[a-z]+/.exec(s.slice(i));
    if (idMatch) {
      const id = idMatch[0];
      i += id.length;
      if (peek() === '(') {
        i++;
        const args: number[] = [expr()];
        while (peek() === ',') {
          i++;
          args.push(expr());
        }
        if (peek() === ')') i++;
        const f = FUNCS[id];
        if (!f) throw new Error(`未知函数 ${id}`);
        return f(...args);
      }
      if (id in CONSTS) return CONSTS[id]!;
      throw new Error(`未知标识 ${id}`);
    }
    throw new Error(`无法解析位置 ${i}`);
  }
  const v = expr();
  if (i !== s.length) throw new Error('有多余字符');
  return v;
}

export function toBase(n: number, base: number): string {
  if (base < 2 || base > 36) throw new Error('进制越界');
  return n.toString(base).toUpperCase();
}

export function bitVisualize(a: number, op: '&' | '|' | '^' | '<<' | '>>', b: number): { a: number; op: string; b: number; res: number; aBin: string; resBin: string } {
  const res = op === '&' ? a & b : op === '|' ? a | b : op === '^' ? a ^ b : op === '<<' ? a << b : a >> b;
  const bin = (x: number) => (x < 0 ? `-${Math.abs(x).toString(2)}` : x.toString(2));
  return { a, op, b, res, aBin: bin(a), resBin: bin(res) };
}

type UnitCat = 'length' | 'weight' | 'data' | 'area' | 'volume' | 'speed';
const UNIT_TABLE: Record<UnitCat, Record<string, number>> = {
  length: { mm: 0.001, cm: 0.01, m: 1, km: 1000, inch: 0.0254, ft: 0.3048, mile: 1609.344, 里: 500, 尺: 1 / 3 },
  weight: { mg: 1e-6, g: 0.001, kg: 1, t: 1000, lb: 0.45359237, oz: 0.0283495, 两: 0.05, 斤: 0.5 },
  data: { B: 1, KB: 1024, MB: 1024 ** 2, GB: 1024 ** 3, TB: 1024 ** 4 },
  area: { 'm²': 1, 'km²': 1e6, 亩: 2000 / 3, 公顷: 1e4, 'ft²': 0.092903 },
  volume: { ml: 0.001, L: 1, 'm³': 1000, gal: 3.785411784 },
  speed: { 'm/s': 1, 'km/h': 1 / 3.6, mph: 0.44704, kn: 0.514444 },
};

export function convertUnits(value: number, from: string, to: string): number {
  for (const table of Object.values(UNIT_TABLE)) {
    if (from in table && to in table) return (value * table[from]!) / table[to]!;
  }
  throw new Error(`未知单位 ${from}/${to}`);
}

export function convertTemp(v: number, from: 'C' | 'F' | 'K', to: 'C' | 'F' | 'K'): number {
  const c = from === 'C' ? v : from === 'F' ? ((v - 32) * 5) / 9 : v - 273.15;
  return to === 'C' ? c : to === 'F' ? (c * 9) / 5 + 32 : c + 273.15;
}

/** 汇率：离线缓存表（基准 CNY）。 */
export const FX_RATES: Record<string, number> = { CNY: 1, USD: 0.14, EUR: 0.13, JPY: 20.5, GBP: 0.11, HKD: 1.09 };

export function convertCurrency(amount: number, from: string, to: string): number {
  const f = FX_RATES[from];
  const t = FX_RATES[to];
  if (f === undefined || t === undefined) throw new Error('未知币种');
  return (amount / f) * t;
}

/** 房贷：等额本息月供。 */
export function loanMonthly(principal: number, annualRate: number, months: number): { monthly: number; total: number; interest: number } {
  const r = annualRate / 12;
  const monthly = r === 0 ? principal / months : (principal * r * (1 + r) ** months) / ((1 + r) ** months - 1);
  return { monthly, total: monthly * months, interest: monthly * months - principal };
}

export function compoundInterest(principal: number, annualRate: number, years: number, perYear = 1): { final: number; interest: number } {
  const final = principal * (1 + annualRate / perYear) ** (years * perYear);
  return { final, interest: final - principal };
}

export function dateDiff(a: string, b: string): number {
  const pa = a.split('-').map(Number);
  const pb = b.split('-').map(Number);
  const ta = new Date(pa[0] ?? 1970, (pa[1] ?? 1) - 1, pa[2] ?? 1).getTime();
  const tb = new Date(pb[0] ?? 1970, (pb[1] ?? 1) - 1, pb[2] ?? 1).getTime();
  return Math.round((tb - ta) / 86400_000);
}

export function ageOf(birth: string, now = new Date()): { years: number; months: number; days: number } {
  const [y, m, d] = birth.split('-').map(Number);
  const b = new Date(y ?? 2000, (m ?? 1) - 1, d ?? 1);
  let years = now.getFullYear() - b.getFullYear();
  let months = now.getMonth() - b.getMonth();
  let days = now.getDate() - b.getDate();
  if (days < 0) {
    months -= 1;
    days += new Date(now.getFullYear(), now.getMonth(), 0).getDate();
  }
  if (months < 0) {
    years -= 1;
    months += 12;
  }
  return { years, months, days };
}

export function bmi(weightKg: number, heightM: number): { value: number; label: string } {
  const v = weightKg / heightM ** 2;
  const label = v < 18.5 ? '偏瘦' : v < 24 ? '正常' : v < 28 ? '偏胖' : '肥胖';
  return { value: Math.round(v * 10) / 10, label };
}

export const SHOE_SIZE_TABLE: Record<number, number> = { 35: 22.0, 36: 22.5, 37: 23.5, 38: 24.0, 39: 24.5, 40: 25.0, 41: 25.5, 42: 26.0, 43: 26.5, 44: 27.5, 45: 28.0 };

export function shoeSize(cm: number): number | undefined {
  return Object.entries(SHOE_SIZE_TABLE).find(([, v]) => v >= cm)?.[0] !== undefined
    ? Number(Object.entries(SHOE_SIZE_TABLE).find(([, v]) => v >= cm)?.[0])
    : undefined;
}

export function subnetOf(ip: string, cidr: number): { network: string; broadcast: string; mask: string; hosts: number } {
  const parts = ip.split('.').map(Number);
  const ipInt = ((parts[0] ?? 0) << 24) | ((parts[1] ?? 0) << 16) | ((parts[2] ?? 0) << 8) | (parts[3] ?? 0);
  const maskInt = cidr === 0 ? 0 : (0xffffffff << (32 - cidr)) >>> 0;
  const netInt = (ipInt & maskInt) >>> 0;
  const bcastInt = (netInt | (~maskInt >>> 0)) >>> 0;
  const fmt = (n: number) => [24, 16, 8, 0].map((sh) => (n >>> sh) & 255).join('.');
  const fmtMask = (n: number) => [n >>> 24 & 255, n >>> 16 & 255, n >>> 8 & 255, n & 255].join('.');
  return { network: fmt(netInt), broadcast: fmt(bcastInt), mask: fmtMask(maskInt), hosts: 2 ** (32 - cidr) - 2 };
}

export function hexToRgb(hex: string): [number, number, number] {
  const h = hex.replace('#', '');
  const full = h.length === 3 ? h.split('').map((c) => c + c).join('') : h;
  return [parseInt(full.slice(0, 2), 16), parseInt(full.slice(2, 4), 16), parseInt(full.slice(4, 6), 16)];
}

export function rgbToHex(r: number, g: number, b: number): string {
  return `#${[r, g, b].map((x) => Math.max(0, Math.min(255, Math.round(x))).toString(16).padStart(2, '0')).join('')}`;
}

export function rgbToHsl(r: number, g: number, b: number): [number, number, number] {
  const [rr, gg, bb] = [r / 255, g / 255, b / 255];
  const max = Math.max(rr, gg, bb);
  const min = Math.min(rr, gg, bb);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h = 0;
  if (max === rr) h = ((gg - bb) / d + (gg < bb ? 6 : 0)) / 6;
  else if (max === gg) h = ((bb - rr) / d + 2) / 6;
  else h = ((rr - gg) / d + 4) / 6;
  return [Math.round(h * 360), Math.round(s * 100), Math.round(l * 100)];
}

export function rgbToCmyk(r: number, g: number, b: number): [number, number, number, number] {
  const [rr, gg, bb] = [r / 255, g / 255, b / 255];
  const k = 1 - Math.max(rr, gg, bb);
  if (k === 1) return [0, 0, 0, 100];
  const c = (1 - rr - k) / (1 - k);
  const m = (1 - gg - k) / (1 - k);
  const y = (1 - bb - k) / (1 - k);
  return [Math.round(c * 100), Math.round(m * 100), Math.round(y * 100), Math.round(k * 100)];
}

const RESISTOR_COLORS = ['黑', '棕', '红', '橙', '黄', '绿', '蓝', '紫', '灰', '白'];

export function resistorOhms(bands: string[]): number {
  if (bands.length < 3) throw new Error('至少三环');
  const v1 = RESISTOR_COLORS.indexOf(bands[0] ?? '');
  const v2 = RESISTOR_COLORS.indexOf(bands[1] ?? '');
  const mul = RESISTOR_COLORS.indexOf(bands[2] ?? '');
  if (v1 < 0 || v2 < 0 || mul < 0) throw new Error('未知色环');
  return (v1 * 10 + v2) * 10 ** mul;
}

export const DENSITY_TABLE: Record<string, number> = { 水: 1.0, 冰: 0.92, 铝: 2.7, 铁: 7.87, 铜: 8.96, 银: 10.49, 金: 19.32, 汽油: 0.75, 酒精: 0.79 };

export function trigSamples(fn: 'sin' | 'cos' | 'tan', from: number, to: number, steps: number): { x: number; y: number }[] {
  const out: { x: number; y: number }[] = [];
  for (let i = 0; i <= steps; i++) {
    const x = from + ((to - from) * i) / steps;
    out.push({ x: Math.round(x * 100) / 100, y: Math.round(FUNCS[fn]!(x) * 1000) / 1000 });
  }
  return out;
}

export class CalcHistory {
  private items: { expr: string; value: number; time: number }[] = [];

  push(expr: string, value: number): void {
    this.items.push({ expr, value, time: Date.now() });
  }

  get list(): readonly { expr: string; value: number; time: number }[] {
    return this.items;
  }
}

export function roundTo(v: number, digits: number): number {
  const p = 10 ** digits;
  return Math.round(v * p) / p;
}

export const CALC_TIPS = ['支持 sin cos sqrt ^ 运算符', '程序员模式支持二/八/十六进制', '单位换算含市制（里/亩/斤）'];

/** §15 守卫：螺纹规格查询为预留位；常用单位组合收藏。 */
export const THREAD_SPEC_RESERVED = true;
export const FAV_UNIT_COMBOS: { name: string; from: string; to: string }[] = [
  { name: '身高 cm→尺', from: 'cm', to: '尺' },
  { name: '体重 斤→kg', from: '斤', to: 'kg' },
  { name: '网速 MB→GB', from: 'MB', to: 'GB' },
];
