// AURORA-10000: AI-33 批次（领域07 效率与工具中枢）逻辑核，勿删。
// 族0161 演示与白板 / 族0162 阅读器 / 族0163 媒体播放器 / 族0164 图片查看器 / 族0165 打印中心。
// 全部纯函数/纯模型，不触网、不落盘。

/* ========================= 族0161 演示与白板 ========================= */

export type BoardTool = 'pen' | 'highlighter' | 'eraser' | 'shape' | 'sticky' | 'image' | 'laser';

export interface BoardElement {
  id: string;
  kind: 'stroke' | 'shape' | 'sticky' | 'image' | 'text';
  x: number;
  y: number;
  w?: number;
  h?: number;
  points?: { x: number; y: number }[];
  shape?: 'rect' | 'ellipse' | 'arrow' | 'line' | 'diamond';
  text?: string;
  color?: string;
  width?: number;
}

let elSeq = 0;

export class Whiteboard {
  private elements: BoardElement[] = [];
  tool: BoardTool = 'pen';
  color = '#e2e8f0';

  stroke(points: { x: number; y: number }[], width = 2): BoardElement {
    elSeq += 1;
    const e: BoardElement = { id: `el-${elSeq}`, kind: 'stroke', x: points[0]?.x ?? 0, y: points[0]?.y ?? 0, points, color: this.color, width };
    this.elements.push(e);
    return e;
  }

  shape(kind: NonNullable<BoardElement['shape']>, x: number, y: number, w: number, h: number): BoardElement {
    elSeq += 1;
    const e: BoardElement = { id: `el-${elSeq}`, kind: 'shape', x, y, w, h, shape: kind, color: this.color };
    this.elements.push(e);
    return e;
  }

  sticky(x: number, y: number, text: string): BoardElement {
    elSeq += 1;
    const e: BoardElement = { id: `el-${elSeq}`, kind: 'sticky', x, y, w: 160, h: 120, text, color: '#fde68a' };
    this.elements.push(e);
    return e;
  }

  insertImage(x: number, y: number, w: number, h: number, alt = ''): BoardElement {
    elSeq += 1;
    const e: BoardElement = { id: `el-${elSeq}`, kind: 'image', x, y, w, h, text: alt };
    this.elements.push(e);
    return e;
  }

  /** 橡皮：擦除命中的笔画。 */
  erase(x: number, y: number, radius = 12): number {
    const before = this.elements.length;
    this.elements = this.elements.filter((e) => {
      if (e.kind !== 'stroke') return true;
      const hit = (e.points ?? []).some((p) => (p.x - x) ** 2 + (p.y - y) ** 2 <= radius ** 2);
      return !hit;
    });
    return before - this.elements.length;
  }

  get all(): readonly BoardElement[] {
    return this.elements;
  }

  /** F04007 导出规格：PNG 画布尺寸 / PDF 页数估算。 */
  exportSpec(bounds = { x: 0, y: 0, w: 1920, h: 1080 }): { png: { w: number; h: number }; pdfPages: number; elements: number } {
    const scale = 2;
    return { png: { w: bounds.w * scale, h: bounds.h * scale }, pdfPages: Math.ceil(bounds.h / bounds.w) || 1, elements: this.elements.length };
  }
}

export const WHITEBOARD_TEMPLATES = ['思维导图', '流程图', '时间线', 'SWOT', '康奈尔笔记', '手帐'] as const;

/** 演示模式：倒计时 / 激光笔轨迹 / 聚光灯。 */
export class PresentationTimer {
  remainSec: number;
  private totalSec: number;

  constructor(minutes = 10) {
    this.totalSec = minutes * 60;
    this.remainSec = this.totalSec;
  }

  tick(sec: number): void {
    this.remainSec = Math.max(0, this.remainSec - sec);
  }

  get progress(): number {
    return 1 - this.remainSec / this.totalSec;
  }
}

export class LaserPointer {
  private trail: { x: number; y: number; t: number }[] = [];
  ttlMs: number;

  constructor(ttlMs = 1200) {
    this.ttlMs = ttlMs;
  }

  move(x: number, y: number, now: number): void {
    this.trail.push({ x, y, t: now });
    this.trail = this.trail.filter((p) => now - p.t <= this.ttlMs);
  }

  visible(now: number): { x: number; y: number }[] {
    return this.trail.filter((p) => now - p.t <= this.ttlMs);
  }
}

export interface Spotlight {
  cx: number;
  cy: number;
  radius: number;
}

export function spotlightMask(s: Spotlight, screenW: number, screenH: number): { dim: string; hole: Spotlight } {
  return { dim: `rect(0,0,${screenW},${screenH})`, hole: s };
}

export function zoomFocus(region: { x: number; y: number; w: number; h: number }, viewport: { w: number; h: number }): { scale: number; tx: number; ty: number } {
  const scale = Math.min(viewport.w / region.w, viewport.h / region.h);
  return { scale, tx: -region.x * scale, ty: -region.y * scale };
}

export const HANDWRITING_BOARD_RESERVED = true;
export const RECORD_LECTURE_RESERVED = true;
export const COLLAB_RESERVED = true;
export const MULTIPLAYER_RESERVED = true;

/** F04024 遥控翻页协议（手机 → 桌面指令）。 */
export type RemoteCommand = 'next' | 'prev' | 'start' | 'stop' | 'black';
export function remotePaging(cmd: RemoteCommand, index: number, total: number): number {
  if (cmd === 'next') return Math.min(total - 1, index + 1);
  if (cmd === 'prev') return Math.max(0, index - 1);
  return index;
}

/** F04015 思维导图布局：简单的层序树布局。 */
export interface MindNode {
  label: string;
  children: MindNode[];
}

export function mindmapLayout(root: MindNode, yGap = 40): { label: string; x: number; y: number }[] {
  const out: { label: string; x: number; y: number }[] = [];
  let cursorY = 0;
  const walk = (n: MindNode, depth: number): number => {
    if (n.children.length === 0) {
      const y = cursorY * yGap;
      cursorY += 1;
      out.push({ label: n.label, x: depth * 200, y });
      return y;
    }
    const ys = n.children.map((c) => walk(c, depth + 1));
    const y = (Math.min(...ys) + Math.max(...ys)) / 2;
    out.push({ label: n.label, x: depth * 200, y });
    return y;
  };
  walk(root, 0);
  return out;
}

export interface FlowNode {
  id: string;
  label: string;
  next: string[];
}

/** F04016 流程图：拓扑分层布局。 */
export function flowLayout(nodes: FlowNode[]): Map<string, number> {
  const depth = new Map<string, number>();
  const walk = (id: string, d: number, seen: Set<string>): void => {
    if (seen.has(id)) return;
    seen.add(id);
    depth.set(id, Math.max(depth.get(id) ?? 0, d));
    const n = nodes.find((x) => x.id === id);
    for (const nx of n?.next ?? []) walk(nx, d + 1, seen);
  };
  const sources = nodes.filter((n) => !nodes.some((x) => x.next.includes(n.id)));
  for (const s of sources) walk(s.id, 0, new Set());
  return depth;
}

export interface TimelineItem {
  label: string;
  at: number;
}

export function timelineLayout(items: TimelineItem[]): { label: string; x: number }[] {
  if (items.length === 0) return [];
  const min = Math.min(...items.map((i) => i.at));
  const max = Math.max(...items.map((i) => i.at));
  const span = max - min || 1;
  return items.map((i) => ({ label: i.label, x: Math.round(((i.at - min) / span) * 1000) }));
}

export interface GanttTask {
  name: string;
  startDay: number;
  days: number;
}

export function ganttLayout(tasks: GanttTask[], totalDays: number): { name: string; xPct: number; wPct: number }[] {
  return tasks.map((t) => ({ name: t.name, xPct: (t.startDay / totalDays) * 100, wPct: (t.days / totalDays) * 100 }));
}

export function fishboneLayout(problem: string, causes: { bone: string; items: string[] }[]): { problem: string; bones: { bone: string; items: string[]; angle: number }[] } {
  return {
    problem,
    bones: causes.map((c, i) => ({ ...c, angle: i % 2 === 0 ? 60 : 120 })),
  };
}

export const SWOT_TEMPLATE = { strengths: '优势', weaknesses: '劣势', opportunities: '机会', threats: '威胁' } as const;
export const CORNELL_TEMPLATE = { cues: '线索栏', notes: '笔记栏', summary: '总结栏' } as const;
export const JOURNAL_TEMPLATE = { date: '日期', mood: '心情', highlights: '今日三件事', gratitude: '感恩' } as const;
export const WHITEBOARD_TIPS = ['P 笔 / H 荧光 / E 橡皮 / S 形状快捷切换', '导出 PNG 为 2x 分辨率'];

/* ========================= 族0162 阅读器 ========================= */

export interface EpubChapter {
  id: string;
  title: string;
  content: string;
}

/** F04026 EPUB：解析 spine（章节序列）。 */
export function epubParse(containerXml: string): string[] {
  return [...containerXml.matchAll(/idref="([^"]+)"/g)].map((m) => m[1] ?? '');
}

export function txtPaginate(text: string, charsPerPage: number): string[] {
  const out: string[] = [];
  for (let i = 0; i < text.length; i += charsPerPage) out.push(text.slice(i, i + charsPerPage));
  return out.length ? out : [''];
}

export function pdfPageCount(text: string): number {
  return Math.max(1, text.split('\f').length);
}

export const MOBI_RESERVED = true;

export interface PageFlipAnim {
  style: 'curl' | 'slide' | 'fade';
  durationMs: number;
}

export const PAGE_FLIP_STYLES: PageFlipAnim[] = [
  { style: 'curl', durationMs: 450 },
  { style: 'slide', durationMs: 240 },
  { style: 'fade', durationMs: 200 },
];

export interface TypographyPrefs {
  fontSize: number;
  lineHeight: number;
  family: 'serif' | 'sans' | 'mono';
  margin: number;
}

export const TYPOGRAPHY_PRESETS: Record<string, TypographyPrefs> = {
  默认: { fontSize: 18, lineHeight: 1.8, family: 'serif', margin: 24 },
  紧凑: { fontSize: 15, lineHeight: 1.5, family: 'sans', margin: 12 },
  舒适: { fontSize: 20, lineHeight: 2.0, family: 'serif', margin: 32 },
};

export type ReaderTheme = 'paper' | 'night' | 'eye-care' | 'high-contrast';
export const READER_THEMES: Record<ReaderTheme, { bg: string; fg: string }> = {
  paper: { bg: '#f7f2e7', fg: '#3b3527' },
  night: { bg: '#111318', fg: '#c9cdd4' },
  'eye-care': { bg: '#cfe6cf', fg: '#27351f' },
  'high-contrast': { bg: '#000000', fg: '#ffffff' },
};

export interface BookMark {
  chapter: string;
  position: number;
  at: number;
}

export class ReaderSession {
  private toc: { id: string; title: string }[] = [];
  private marks: BookMark[] = [];
  private highlights: { text: string; note?: string; at: number }[] = [];
  private history: { book: string; at: number }[] = [];
  progress = 0;

  setToc(items: { id: string; title: string }[]): this {
    this.toc = items;
    return this;
  }

  get tocList(): readonly { id: string; title: string }[] {
    return this.toc;
  }

  jumpChapter(id: string): number {
    return Math.max(0, this.toc.findIndex((t) => t.id === id));
  }

  addMark(chapter: string, position: number): BookMark {
    const m: BookMark = { chapter, position, at: Date.now() };
    this.marks.push(m);
    return m;
  }

  get markList(): readonly BookMark[] {
    return this.marks;
  }

  highlight(text: string, note?: string): boolean {
    if (!text.trim()) return false;
    this.highlights.push({ text: text.trim(), note, at: Date.now() });
    return true;
  }

  exportNotes(): string {
    return this.highlights.map((h) => `> ${h.text}${h.note ? `\n注：${h.note}` : ''}`).join('\n\n');
  }

  setProgress(pct: number): this {
    this.progress = Math.max(0, Math.min(100, pct));
    return this;
  }

  recordHistory(book: string): this {
    this.history.push({ book, at: Date.now() });
    return this;
  }

  get readHistory(): readonly { book: string; at: number }[] {
    return this.history;
  }
}

export class ReadingStats {
  private minutesByDay = new Map<string, number>();

  add(date: string, minutes: number): this {
    this.minutesByDay.set(date, (this.minutesByDay.get(date) ?? 0) + minutes);
    return this;
  }

  total(): number {
    return [...this.minutesByDay.values()].reduce((s, m) => s + m, 0);
  }

  byDay(date: string): number {
    return this.minutesByDay.get(date) ?? 0;
  }
}

export interface TtsConfig {
  voice: string;
  rate: number; // 0.5~3
  autoScroll: boolean;
  autoPageSec: number;
}

export const READER_TTS_DEFAULT: TtsConfig = { voice: '晓晓', rate: 1, autoScroll: false, autoPageSec: 0 };

export const DICT_MINI: Record<string, string> = {
  aurora: 'n. 极光；曙光',
  variable: 'n. 变量；可变物',
  kernel: 'n. 内核；核心',
  render: 'v. 渲染；呈现',
  cache: 'n. 缓存',
};

export function lookupWord(word: string): string | undefined {
  return DICT_MINI[word.toLowerCase()];
}

export function translateWord(word: string, to = 'zh'): string {
  const hit = lookupWord(word);
  return hit ?? `[${to}] ${word}`;
}

export const FULLSCREEN_READER = { immersive: true, hideChrome: true };
export const RSS_RESERVED = true;

export class ReadLater {
  private items: { url: string; title: string; offline: boolean; addedAt: number }[] = [];

  add(url: string, title: string, offline = false): boolean {
    if (this.items.some((x) => x.url === url)) return false;
    this.items.push({ url, title, offline, addedAt: Date.now() });
    return true;
  }

  get list(): readonly { url: string; title: string; offline: boolean }[] {
    return this.items;
  }

  offlineCount(): number {
    return this.items.filter((x) => x.offline).length;
  }
}

export const READER_TIPS = ['纸质/夜间/护眼三主题一键切换', '划词查词典不联网', '听书语速 0.5~3 倍可调'];

/* ========================= 族0163 媒体播放器 ========================= */

export interface MediaItem {
  id: string;
  title: string;
  durationSec: number;
  kind: 'video' | 'audio';
  path: string;
}

export class Playlist {
  private items: MediaItem[] = [];
  index = -1;
  mode: 'sequence' | 'shuffle' | 'loop-one' = 'sequence';

  add(item: MediaItem): this {
    this.items.push(item);
    return this;
  }

  get list(): readonly MediaItem[] {
    return this.items;
  }

  playAt(i: number): MediaItem | undefined {
    if (i < 0 || i >= this.items.length) return undefined;
    this.index = i;
    return this.items[i];
  }

  next(): MediaItem | undefined {
    if (this.items.length === 0) return undefined;
    if (this.mode === 'loop-one') return this.items[this.index];
    if (this.mode === 'shuffle') return this.playAt(Math.floor(Math.random() * this.items.length));
    return this.playAt((this.index + 1) % this.items.length);
  }

  exportM3U(): string {
    return ['#EXTM3U', ...this.items.flatMap((i) => [`#EXTINF:${i.durationSec},${i.title}`, i.path])].join('\n');
  }
}

/** F04054 外挂字幕：SRT 解析 + 时间偏移。 */
export interface SubtitleCue {
  startSec: number;
  endSec: number;
  text: string;
}

function parseTs(ts: string): number {
  const m = ts.match(/(\d{2}):(\d{2}):(\d{2})[,.](\d{3})/);
  if (!m) return 0;
  return Number(m[1]) * 3600 + Number(m[2]) * 60 + Number(m[3]) + Number(m[4]) / 1000;
}

export function srtParse(srt: string): SubtitleCue[] {
  const out: SubtitleCue[] = [];
  for (const block of srt.split(/\r?\n\r?\n/)) {
    const lines = block.split(/\r?\n/).filter(Boolean);
    const tm = lines.find((l) => l.includes('-->'));
    if (!tm) continue;
    const [a, b] = tm.split('-->');
    const textLines = lines.slice(lines.indexOf(tm) + 1);
    out.push({ startSec: parseTs(a ?? ''), endSec: parseTs(b ?? ''), text: textLines.join('\n') });
  }
  return out;
}

export function subtitleShift(cues: SubtitleCue[], deltaSec: number): SubtitleCue[] {
  return cues.map((c) => ({ ...c, startSec: Math.max(0, c.startSec + deltaSec), endSec: Math.max(0, c.endSec + deltaSec) }));
}

export function subtitleAt(cues: SubtitleCue[], t: number): string | undefined {
  return cues.find((c) => t >= c.startSec && t < c.endSec)?.text;
}

export const AUDIO_TRACKS = ['普通话', '粤语', '英语', '导演解说'] as const;
export const QUALITY_LEVELS = ['自动', '2160p', '1080p', '720p', '480p'] as const;

export const PLAYBACK_SPEEDS = [0.25, 0.5, 0.75, 1, 1.25, 1.5, 2, 3, 4] as const;

export function clampSpeed(v: number): number {
  return Math.max(0.25, Math.min(4, v));
}

export class AbLoop {
  aSec: number | undefined;
  bSec: number | undefined;

  setA(t: number): this {
    this.aSec = t;
    return this;
  }

  setB(t: number): this {
    this.bSec = t;
    return this;
  }

  /** 循环窗口内取回绕时间。 */
  wrap(t: number): number {
    if (this.aSec === undefined || this.bSec === undefined || this.aSec >= this.bSec) return t;
    return t >= this.bSec ? this.aSec : t;
  }
}

export class TimeBookmark {
  private marks: { label: string; at: number }[] = [];

  add(label: string, at: number): this {
    this.marks.push({ label, at });
    return this;
  }

  get list(): readonly { label: string; at: number }[] {
    return this.marks;
  }

  nearest(t: number): { label: string; at: number } | undefined {
    return [...this.marks].sort((a, b) => Math.abs(a.at - t) - Math.abs(b.at - t))[0];
  }
}

export function screenshotSpec(atSec: number, video: { w: number; h: number }): { at: number; w: number; h: number; format: 'png' } {
  return { at: atSec, w: video.w, h: video.h, format: 'png' };
}

export interface EqBand {
  freq: number;
  gain: number; // dB, -12..12
}

export const EQ_BANDS = [60, 170, 350, 1000, 3500, 10000];

export class Equalizer {
  private bands: EqBand[];

  constructor(gains: number[] = EQ_BANDS.map(() => 0)) {
    this.bands = EQ_BANDS.map((freq, i) => ({ freq, gain: Math.max(-12, Math.min(12, gains[i] ?? 0)) }));
  }

  set(freq: number, gain: number): boolean {
    const b = this.bands.find((x) => x.freq === freq);
    if (!b) return false;
    b.gain = Math.max(-12, Math.min(12, gain));
    return true;
  }

  preset(name: 'flat' | 'bass' | 'vocal' | 'treble'): void {
    const map: Record<string, number[]> = {
      flat: [0, 0, 0, 0, 0, 0],
      bass: [6, 4, 1, 0, -1, -2],
      vocal: [-2, 0, 2, 4, 2, 0],
      treble: [-2, -1, 0, 1, 4, 6],
    };
    const g = map[name] ?? [];
    this.bands.forEach((b, i) => (b.gain = g[i] ?? 0));
  }

  get gains(): number[] {
    return this.bands.map((b) => b.gain);
  }
}

export function volumeBoost(gain: number): number {
  return Math.max(0, Math.min(6, gain));
}

export class SleepTimer {
  remainSec: number;

  constructor(minutes: number) {
    this.remainSec = minutes * 60;
  }

  /** 返回是否刚好归零（仅触发一次）。 */
  tick(sec: number): boolean {
    if (this.remainSec === 0) return false;
    this.remainSec = Math.max(0, this.remainSec - sec);
    return this.remainSec === 0;
  }
}

export const MINI_MODE_SPEC = { width: 320, height: 56, compact: true };
export const PIP_SPEC = { minWidth: 200, minHeight: 113, alwaysOnTop: true };

export class ResumeMemory {
  private map = new Map<string, number>();

  save(id: string, positionSec: number): this {
    this.map.set(id, positionSec);
    return this;
  }

  resume(id: string): number {
    return this.map.get(id) ?? 0;
  }
}

export class MediaLibrary {
  private videos: MediaItem[] = [];
  private audios: MediaItem[] = [];

  add(item: MediaItem): boolean {
    if ([...this.videos, ...this.audios].some((x) => x.path === item.path)) return false;
    (item.kind === 'video' ? this.videos : this.audios).push(item);
    return true;
  }

  get videoList(): readonly MediaItem[] {
    return this.videos;
  }

  get audioList(): readonly MediaItem[] {
    return this.audios;
  }

  search(q: string): MediaItem[] {
    const k = q.toLowerCase();
    return [...this.videos, ...this.audios].filter((m) => m.title.toLowerCase().includes(k));
  }
}

export interface LyricLine {
  timeSec: number;
  text: string;
}

export function lrcParse(lrc: string): LyricLine[] {
  return [...lrc.matchAll(/\[(\d{1,2}):(\d{1,2})(?:\.(\d{1,3}))?\](.+)/g)]
    .map((m) => ({ timeSec: Number(m[1]) * 60 + Number(m[2]) + Number(m[3] ?? 0) / 1000, text: m[4] ?? '' }))
    .sort((a, b) => a.timeSec - b.timeSec);
}

export function lyricAt(lines: LyricLine[], t: number): string | undefined {
  let cur: string | undefined;
  for (const l of lines) {
    if (l.timeSec <= t) cur = l.text;
    else break;
  }
  return cur;
}

export const DESKTOP_LYRIC_SPEC = { draggable: true, fontScale: 1.2, outline: true };

export function spectrumBars(samples: number[], bars = 24): number[] {
  const chunk = Math.max(1, Math.floor(samples.length / bars));
  return Array.from({ length: bars }, (_, i) => {
    const seg = samples.slice(i * chunk, (i + 1) * chunk);
    return seg.length ? Math.round(seg.reduce((s, x) => s + Math.abs(x), 0) / seg.length * 100) / 100 : 0;
  });
}

export const HW_DECODE = { codecs: ['h264', 'hevc', 'av1'], prefer: true };
export const PLAYER_TIPS = ['A-B 循环先定 A 点再定 B 点', '断点续播按文件记忆位置', '迷你模式可拖到屏幕角落'];

/* ========================= 族0164 图片查看器 ========================= */

export interface ImageMeta {
  path: string;
  width: number;
  height: number;
  format: string;
  exif?: Record<string, string>;
}

export function openBudget(meta: ImageMeta): { instant: boolean; decodeMs: number } {
  const mp = (meta.width * meta.height) / 1e6;
  return { instant: mp <= 12, decodeMs: Math.round(mp * 8) };
}

export function zoomClamp(scale: number): number {
  return Math.max(0.05, Math.min(32, scale));
}

export function smoothZoomStep(current: number, direction: 1 | -1): number {
  const step = current < 1 ? 0.1 : current < 4 ? 0.25 : 1;
  return zoomClamp(current + direction * step);
}

export type Flip = 'none' | 'h' | 'v';

export function transformImage(rotateDeg: number, flip: Flip): { rotate: number; flip: Flip } {
  return { rotate: ((rotateDeg % 360) + 360) % 360, flip };
}

export class ThumbnailStrip {
  private items: ImageMeta[] = [];
  active = -1;

  load(items: ImageMeta[]): this {
    this.items = items;
    return this;
  }

  activate(i: number): ImageMeta | undefined {
    this.active = Math.max(0, Math.min(this.items.length - 1, i));
    return this.items[this.active];
  }

  get list(): readonly ImageMeta[] {
    return this.items;
  }
}

export class Slideshow {
  intervalSec: number;
  private elapsed = 0;

  constructor(intervalSec = 3) {
    this.intervalSec = intervalSec;
  }

  tick(sec: number): boolean {
    this.elapsed += sec;
    if (this.elapsed >= this.intervalSec) {
      this.elapsed = 0;
      return true;
    }
    return false;
  }
}

export function exifPanel(meta: ImageMeta): string[] {
  const rows = Object.entries(meta.exif ?? {}).map(([k, v]) => `${k}: ${v}`);
  return [`尺寸: ${meta.width}×${meta.height}`, `格式: ${meta.format.toUpperCase()}`, ...rows];
}

export const RAW_FORMATS_RESERVED = ['cr2', 'cr3', 'nef', 'arw', 'dng', 'raf'] as const;

export class BatchViewer {
  private paths: string[] = [];
  pos = 0;

  load(paths: string[]): this {
    this.paths = paths;
    return this;
  }

  step(dir: 1 | -1): string | undefined {
    if (this.paths.length === 0) return undefined;
    this.pos = (this.pos + dir + this.paths.length) % this.paths.length;
    return this.paths[this.pos];
  }

  get count(): number {
    return this.paths.length;
  }
}

export function compareImages(a: ImageMeta, b: ImageMeta): { sameSize: boolean; sizeDeltaPct: number } {
  const pa = a.width * a.height;
  const pb = b.width * b.height;
  return { sameSize: a.width === b.width && a.height === b.height, sizeDeltaPct: pa === 0 ? 0 : Math.round(Math.abs(pa - pb) / pa * 100) };
}

export function magnifierRegion(x: number, y: number, radius = 80, zoom = 2): { cx: number; cy: number; r: number; zoom: number } {
  return { cx: x, cy: y, r: radius, zoom: Math.max(1, zoom) };
}

export function pickColor(pixels: Uint8ClampedArray, x: number, y: number, width: number): string {
  const i = (y * width + x) * 4;
  const r = pixels[i] ?? 0;
  const g = pixels[i + 1] ?? 0;
  const b = pixels[i + 2] ?? 0;
  return `#${[r, g, b].map((v) => v.toString(16).padStart(2, '0')).join('')}`;
}

export function histogram(pixels: Uint8ClampedArray): { r: number[]; g: number[]; b: number[] } {
  const r = new Array<number>(32).fill(0);
  const g = new Array<number>(32).fill(0);
  const b = new Array<number>(32).fill(0);
  for (let i = 0; i < pixels.length; i += 4) {
    const ri = Math.min(31, Math.floor((pixels[i] ?? 0) / 8));
    const gi = Math.min(31, Math.floor((pixels[i + 1] ?? 0) / 8));
    const bi = Math.min(31, Math.floor((pixels[i + 2] ?? 0) / 8));
    r[ri] = (r[ri] ?? 0) + 1;
    g[gi] = (g[gi] ?? 0) + 1;
    b[bi] = (b[bi] ?? 0) + 1;
  }
  return { r, g, b };
}

/** 自动转正：按 EXIF Orientation 返回应旋转角度。 */
export function autoOrient(exifOrientation: number): number {
  const map: Record<number, number> = { 1: 0, 3: 180, 6: 90, 8: 270 };
  return map[exifOrientation] ?? 0;
}

export function printHandoff(meta: ImageMeta): { copies: number; paper: 'A4' | 'A3'; fit: 'contain' } {
  return { copies: 1, paper: meta.width > meta.height ? 'A3' : 'A4', fit: 'contain' };
}

export function setWallpaper(meta: ImageMeta): { path: string; mode: 'fill' } {
  return { path: meta.path, mode: 'fill' };
}

export interface CropRect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export function lightCrop(meta: ImageMeta, rect: CropRect): { path: string; width: number; height: number } {
  const w = Math.max(1, Math.min(rect.w, meta.width - rect.x));
  const h = Math.max(1, Math.min(rect.h, meta.height - rect.y));
  return { path: meta.path, width: w, height: h };
}

export const SHARE_RESERVED = true;

export function sameDirNavigate(paths: string[], current: string, dir: 1 | -1): string | undefined {
  const i = paths.indexOf(current);
  if (i < 0) return undefined;
  return paths[(i + dir + paths.length) % paths.length];
}

export class GifControl {
  playing = true;
  frame = 0;
  totalFrames: number;

  constructor(totalFrames: number) {
    this.totalFrames = Math.max(1, totalFrames);
  }

  toggle(): boolean {
    this.playing = !this.playing;
    return this.playing;
  }

  stepFrame(dir: 1 | -1): number {
    this.playing = false;
    this.frame = (this.frame + dir + this.totalFrames) % this.totalFrames;
    return this.frame;
  }
}

export const SVG_VIEW = { vector: true, zoomIndependent: true };
export const HEIC_SUPPORTED = true;
export const DUAL_SCREEN_VIEWER = { primary: 'viewer', secondary: 'thumbnail-wall' };
export const FULLSCREEN_IMAGE = { hideUi: true, escExit: true };
export const IMAGE_TIPS = ['滚轮缩放 + 空格暂停 GIF', 'EXIF 面板可一键清除隐私', '双图对比支持滑动分割'];

/* ========================= 族0165 打印中心 ========================= */

export type PrintJobState = 'queued' | 'printing' | 'paused' | 'done' | 'cancelled' | 'error';

export interface PrintJob {
  id: string;
  title: string;
  pages: number;
  copies: number;
  state: PrintJobState;
  printer: string;
  colorMode: 'color' | 'mono';
  duplex: boolean;
  createdAt: number;
}

let jobSeq = 0;

export class PrintQueue {
  private jobs: PrintJob[] = [];

  add(title: string, pages: number, copies = 1, opts: { printer?: string; colorMode?: 'color' | 'mono'; duplex?: boolean } = {}): PrintJob {
    jobSeq += 1;
    const j: PrintJob = {
      id: `job-${jobSeq}`,
      title,
      pages,
      copies,
      state: 'queued',
      printer: opts.printer ?? '默认打印机',
      colorMode: opts.colorMode ?? 'color',
      duplex: opts.duplex ?? false,
      createdAt: Date.now(),
    };
    this.jobs.push(j);
    return j;
  }

  get list(): readonly PrintJob[] {
    return this.jobs;
  }

  start(): void {
    const next = this.jobs.find((j) => j.state === 'queued');
    if (next) next.state = 'printing';
  }

  pause(id: string): boolean {
    const j = this.jobs.find((x) => x.id === id);
    if (!j || j.state !== 'printing') return false;
    j.state = 'paused';
    return true;
  }

  resume(id: string): boolean {
    const j = this.jobs.find((x) => x.id === id);
    if (!j || j.state !== 'paused') return false;
    j.state = 'printing';
    return true;
  }

  cancel(id: string): boolean {
    const j = this.jobs.find((x) => x.id === id);
    if (!j || ['done', 'cancelled'].includes(j.state)) return false;
    j.state = 'cancelled';
    return true;
  }

  finishTop(): PrintJob | undefined {
    const j = this.jobs.find((x) => x.state === 'printing');
    if (j) j.state = 'done';
    return j;
  }
}

export function printPreview(pages: number, paper: 'A4' | 'A3' | 'B5' = 'A4'): { pages: number; paper: string; mm: string } {
  const sizes: Record<string, string> = { A4: '210×297', A3: '297×420', B5: '176×250' };
  return { pages, paper, mm: sizes[paper] ?? '210×297' };
}

/** N 页合一：每页缩放后可容纳的源页数与网格。 */
export function nUpLayout(n: 1 | 2 | 4 | 6 | 9): { perSheet: number; cols: number; rows: number; scalePct: number } {
  const map: Record<number, { cols: number; rows: number }> = { 1: { cols: 1, rows: 1 }, 2: { cols: 2, rows: 1 }, 4: { cols: 2, rows: 2 }, 6: { cols: 3, rows: 2 }, 9: { cols: 3, rows: 3 } };
  const g = map[n] ?? { cols: 1, rows: 1 };
  return { perSheet: n, cols: g.cols, rows: g.rows, scalePct: Math.round(100 / Math.max(g.cols, g.rows)) };
}

export function duplexHint(pages: number, duplex: boolean): string {
  if (!duplex) return '单面打印';
  return pages % 2 === 0 ? `双面打印 ${pages / 2} 张纸` : `双面打印 ${Math.ceil(pages / 2)} 张纸（末页空白背）`;
}

export const PRINT_TO_PDF = { virtual: true, name: 'Varix PDF 打印机' };
export function printToImages(pages: number, dpi = 150): { count: number; dpi: number; format: 'png' } {
  return { count: pages, dpi, format: 'png' };
}

/** 页码范围解析："1-3,5,8-"。 */
export function parsePageRange(range: string, total: number): number[] {
  const out = new Set<number>();
  for (const part of range.split(',')) {
    const t = part.trim();
    if (!t) continue;
    const m = t.match(/^(\d*)-(\d*)$/);
    if (m) {
      const from = Number(m[1] || 1);
      const to = Number(m[2] || total);
      for (let p = Math.max(1, from); p <= Math.min(total, to); p++) out.add(p);
    } else if (/^\d+$/.test(t)) {
      const p = Number(t);
      if (p >= 1 && p <= total) out.add(p);
    }
  }
  return [...out].sort((a, b) => a - b);
}

export function scaleFit(mode: 'fit' | 'actual' | 'custom', customPct = 100): { pct: number; label: string } {
  if (mode === 'fit') return { pct: 100, label: '适应纸张' };
  if (mode === 'actual') return { pct: 100, label: '实际大小' };
  return { pct: Math.max(10, Math.min(400, customPct)), label: `自定义 ${Math.max(10, Math.min(400, customPct))}%` };
}

export const PAPER_SIZES = ['A3', 'A4', 'A5', 'B5', 'Letter', 'Legal', '16K'] as const;

export function printHistory(jobs: readonly PrintJob[]): { title: string; state: PrintJobState; at: number }[] {
  return jobs.map((j) => ({ title: j.title, state: j.state, at: j.createdAt }));
}

export function setDefaultPrinter(printers: string[], name: string): string | undefined {
  return printers.includes(name) ? name : undefined;
}

export interface DiscoveredPrinter {
  name: string;
  ip: string;
  via: 'mDNS' | 'SNMP' | 'WSD';
  online: boolean;
}

export function discoverPrinters(seed: DiscoveredPrinter[]): DiscoveredPrinter[] {
  return seed.filter((p) => p.online);
}

export function driverStatus(ok: boolean, version = '1.0.0'): { healthy: boolean; version: string; message: string } {
  return ok ? { healthy: true, version, message: '驱动正常' } : { healthy: false, version, message: '驱动异常，建议重装' };
}

/** 成本估算：黑白 0.08 元/页，彩色 0.45 元/页，覆盖率修正。 */
export function costEstimate(pages: number, copies: number, colorMode: 'color' | 'mono', coveragePct = 5): { pages: number; yuan: number } {
  const unit = colorMode === 'color' ? 0.45 : 0.08;
  const factor = Math.max(0.5, coveragePct / 5);
  const total = pages * copies * unit * factor;
  return { pages: pages * copies, yuan: Math.round(total * 100) / 100 };
}

export function ecoInk(coveragePct: number): { enabled: boolean; savedPct: number } {
  const saved = Math.min(60, Math.max(0, Math.round((1 - coveragePct / 100) * 60)));
  return { enabled: true, savedPct: saved };
}

/** 海报分块：目标 A3 尺寸需要的 A4 页数与排布。 */
export function posterTiles(targetW: number, targetH: number, tileW = 210, tileH = 297, overlapMm = 10): { cols: number; rows: number; tiles: number } {
  const cols = Math.ceil(targetW / (tileW - overlapMm));
  const rows = Math.ceil(targetH / (tileH - overlapMm));
  return { cols, rows, tiles: cols * rows };
}

/** 小册子骑马钉排序：输入页数，返回每张纸正反面的页序。 */
export function bookletOrder(totalPages: number): number[][] {
  if (totalPages % 4 !== 0) throw new Error('小册子页数需为 4 的倍数');
  const out: number[][] = [];
  let front = totalPages;
  let back = 1;
  for (let i = 0; i < totalPages / 4; i++) {
    out.push([front, back, back + 1, front - 1]);
    front -= 2;
    back += 2;
  }
  return out;
}

export function diagnoseQueue(jobs: PrintJob[]): string[] {
  const out: string[] = [];
  const stuck = jobs.filter((j) => j.state === 'printing');
  if (stuck.length > 1) out.push(`检测到 ${stuck.length} 个任务同时打印，建议暂停次要任务`);
  const err = jobs.filter((j) => j.state === 'error');
  if (err.length > 0) out.push(`有 ${err.length} 个错误任务，请检查打印机连接与纸张`);
  if (jobs.every((j) => j.state === 'queued')) out.push('队列未启动：检查打印机是否在线');
  if (out.length === 0) out.push('队列正常');
  return out;
}

export function printWatermark(text: string, opacity = 0.12): { text: string; opacity: number; every: 'page' } {
  return { text: text.slice(0, 40), opacity, every: 'page' };
}

export function batchPrint(titles: string[]): PrintJob[] {
  const q = new PrintQueue();
  return titles.map((t) => q.add(t, 1));
}

export const PRINT_TIPS = ['打印到 PDF 不消耗纸张', '省墨模式最多省 60% 耗材', '小册子打印页数需为 4 的倍数'];
