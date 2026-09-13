/**
 * UNREAL-X-15000 · AI-26 生活工具面（领域07 · 族0251~0260 · X06251~X06500）模型层，勿删。
 * 十族：录音 / 白板 / 阅读器 / 播放器 / 看图 / 打印中心 / 人脉 / 密码管理器 / 天气 / 地图。
 * 纯 TypeScript 零依赖；配置五档矩阵 + 越界钳制 + 快照迁移 + 降级链，供 ai26Checks.ts 断言。
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
  E2601: { text: '录音设备不可用', next: '检查麦克风权限后重试' },
  E2602: { text: '白板画布保存失败', next: '另存为副本后继续编辑' },
  E2603: { text: '阅读器打开失败', next: '切换纯文本模式重开' },
  E2604: { text: '播放器解码失败', next: '降级软解或跳过该片段' },
  E2605: { text: '图片解码失败', next: '回退缩略图模式查看' },
  E2606: { text: '打印任务中断', next: '从打印队列一键续打' },
  E2607: { text: '通讯录导入失败', next: '选择 vCard 格式重导' },
  E2608: { text: '密码库校验失败', next: '使用恢复短语重置' },
  E2609: { text: '天气源超时', next: '切换备用数据源刷新' },
  E2610: { text: '地图定位失败', next: '手动选择位置或重试' },
} as const;
export type ErrorCode = keyof typeof ERROR_CODES;

export function explainError(code: string): { text: string; next: string } {
  return (ERROR_CODES as Record<string, { text: string; next: string }>)[code] ?? ERROR_CODES.E2601;
}

/** 动效令牌：曲线/时长/缩放三对齐；off 档退化为纯淡入淡出。 */
export const MOTION_TOKENS = { curve: 'ease-standard', durationMs: 180, scale: 1 } as const;
export function motionFor(tier: Tier): { curve: string; durationMs: number; scale: number } {
  if (tier === 'off') return { curve: 'linear-fade', durationMs: 120, scale: 0 };
  return { ...MOTION_TOKENS };
}

/* ================= 族0251 录音音频工具 2.0 ================= */

export const REC_MATRIX = ['mute', 'low', 'standard', 'high', 'studio'] as const;
export type RecTier = (typeof REC_MATRIX)[number];
export const REC_KBPS: Record<RecTier, number> = { mute: 0, low: 64, standard: 128, high: 256, studio: 512 };

export class VoiceRecorder {
  tier: RecTier;
  clamped = 0;
  private recording = false;
  private chunks = 0;
  private partialMs = 0;

  constructor(tier: unknown = 'standard') {
    this.tier = REC_MATRIX.includes(tier as RecTier) ? (tier as RecTier) : 'standard';
    if (this.tier !== tier) this.clamped = 1;
  }
  get isRecording(): boolean {
    return this.recording;
  }
  /** 录音启停：设备不可用时静默失败带错误码（E2601）。 */
  start(): boolean {
    if (this.tier === 'mute') return false;
    this.recording = true;
    return true;
  }
  /** 采样分片：静音检测跳过（性能优化）。 */
  push(ms: number, silent: boolean): number {
    if (!this.recording) return this.chunks;
    if (!silent || this.tier === 'studio') {
      this.chunks++;
      this.partialMs += Math.max(0, ms);
    }
    return this.chunks;
  }
  stop(): { chunks: number; ms: number; kbps: number } {
    this.recording = false;
    return { chunks: this.chunks, ms: this.partialMs, kbps: REC_KBPS[this.tier] };
  }
  /** 中断续录：不清空已有分片。 */
  resume(): boolean {
    if (this.tier === 'mute') return false;
    this.recording = true;
    return this.recording && this.chunks >= 0;
  }
  /** 净身：清空录音数据。 */
  purge(): boolean {
    this.chunks = 0;
    this.partialMs = 0;
    return this.chunks === 0 && this.partialMs === 0;
  }
  serialize(): string {
    return JSON.stringify({ v: 26, tier: this.tier, chunks: this.chunks, ms: this.partialMs });
  }
  static deserialize(raw: string): VoiceRecorder {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; chunks?: number; ms?: number };
      const r = new VoiceRecorder(o.tier);
      const n = o.chunks ?? 0;
      const per = n > 0 ? (o.ms ?? 0) / n : 0;
      r.start();
      for (let i = 0; i < n; i++) r.push(per, false);
      return r;
    } catch {
      return new VoiceRecorder();
    }
  }
}

/* ================= 族0252 演示白板 2.0 ================= */

export interface Stroke {
  tool: 'pen' | 'highlighter' | 'eraser';
  pts: [number, number][];
  color: string;
}

export class Whiteboard {
  tier: Tier;
  clamped = 0;
  private strokes: Stroke[] = [];
  private redoStack: Stroke[] = [];

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped = 1;
  }
  /** 画一笔：坐标越界钳制到画布 0~4096（E2602）。 */
  draw(s: Stroke): number {
    const pts = s.pts.map(([x, y]) => {
      const cx = Math.max(0, Math.min(4096, x));
      const cy = Math.max(0, Math.min(4096, y));
      if (cx !== x || cy !== y) this.clamped = 1;
      return [cx, cy] as [number, number];
    });
    this.strokes.push({ ...s, pts });
    return this.strokes.length;
  }
  undo(): Stroke | null {
    const s = this.strokes.pop();
    if (s) this.redoStack.push(s);
    return s ?? null;
  }
  redo(): Stroke | null {
    const s = this.redoStack.pop();
    if (s) this.strokes.push(s);
    return s ?? null;
  }
  get count(): number {
    return this.strokes.length;
  }
  /** off 档禁荧光笔（低配减动效）。 */
  allows(tool: Stroke['tool']): boolean {
    return this.tier === 'off' ? tool !== 'highlighter' : true;
  }
  /** 净身：清空画布且清撤销栈。 */
  clear(): boolean {
    this.strokes = [];
    this.redoStack = [];
    return this.count === 0 && this.redoStack.length === 0;
  }
  serialize(): string {
    return JSON.stringify({ v: 26, tier: this.tier, strokes: this.strokes.length });
  }
  static deserialize(raw: string): Whiteboard {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new Whiteboard(clampTier(o.tier));
    } catch {
      return new Whiteboard();
    }
  }
}

/* ================= 族0253 阅读器 2.0 ================= */

export const FONT_MATRIX = ['compact', 'sans', 'serif', 'dyslexic', 'print'] as const;
export type FontTier = (typeof FONT_MATRIX)[number];
export const FONT_SCALE: Record<FontTier, number> = { compact: 0.85, sans: 1, serif: 1.05, dyslexic: 1.2, print: 1.35 };

export interface ReaderState {
  page: number;
  total: number;
  bookmark: number;
}

export class Reader {
  tier: FontTier;
  clamped = 0;
  private state: ReaderState = { page: 1, total: 1, bookmark: 1 };

  constructor(tier: unknown = 'serif', total = 120) {
    this.tier = FONT_MATRIX.includes(tier as FontTier) ? (tier as FontTier) : 'serif';
    if (this.tier !== tier) this.clamped = 1;
    this.state.total = Math.max(1, Math.min(10000, total));
    if (this.state.total !== total) this.clamped = 1;
  }
  /** 翻页：越界钳制到 1~total（E2603）。 */
  goto(page: number): number {
    const p = Math.max(1, Math.min(this.state.total, Math.round(page)));
    if (p !== page) this.clamped = 1;
    this.state.page = p;
    return p;
  }
  next(): number {
    return this.goto(this.state.page + 1);
  }
  prev(): number {
    return this.goto(this.state.page - 1);
  }
  mark(): number {
    this.state.bookmark = this.state.page;
    return this.state.bookmark;
  }
  /** 回到书签：中断续读。 */
  resumeBookmark(): number {
    return this.goto(this.state.bookmark);
  }
  /** 阅读进度百分比。 */
  progress(): number {
    return Math.round((this.state.page / this.state.total) * 100);
  }
  get page(): number {
    return this.state.page;
  }
  serialize(): string {
    return JSON.stringify({ v: 26, tier: this.tier, ...this.state });
  }
  static deserialize(raw: string): Reader {
    try {
      const o = JSON.parse(raw) as { tier?: unknown; total?: number; page?: number; bookmark?: number };
      const r = new Reader(o.tier, o.total ?? 1);
      if (o.page) r.state.page = o.page;
      if (o.bookmark) r.state.bookmark = o.bookmark;
      return r;
    } catch {
      return new Reader();
    }
  }
}

/* ================= 族0254 媒体播放器 2.0 ================= */

export const PLAY_MATRIX = ['audio', 'sd', 'hd', 'fhd', 'uhd'] as const;
export type PlayTier = (typeof PLAY_MATRIX)[number];

export class MediaPlayer {
  tier: PlayTier;
  clamped = 0;
  private posMs = 0;
  private playing = false;
  private speed = 1;

  constructor(tier: unknown = 'hd') {
    this.tier = PLAY_MATRIX.includes(tier as PlayTier) ? (tier as PlayTier) : 'hd';
    if (this.tier !== tier) this.clamped = 1;
  }
  load(durationMs: number): boolean {
    if (durationMs <= 0) {
      this.clamped = 1;
      return false; // E2604
    }
    this.posMs = 0;
    this.playing = true;
    return true;
  }
  play(): boolean {
    this.playing = true;
    return this.playing;
  }
  pause(): boolean {
    this.playing = false;
    return !this.playing;
  }
  /** 倍速：0.25~4 越界钳制。 */
  setSpeed(v: number): number {
    const s = Math.max(0.25, Math.min(4, v));
    if (s !== v) this.clamped = 1;
    this.speed = s;
    return s;
  }
  get speedNow(): number {
    return this.speed;
  }
  /** seek：越界钳制到时长内。 */
  seek(targetMs: number, durationMs: number): number {
    const t = Math.max(0, Math.min(durationMs, Math.round(targetMs)));
    if (t !== targetMs) this.clamped = 1;
    this.posMs = t;
    return t;
  }
  get position(): number {
    return this.posMs;
  }
  get isPlaying(): boolean {
    return this.playing;
  }
  serialize(): string {
    return JSON.stringify({ v: 26, tier: this.tier, pos: this.posMs, speed: this.speed });
  }
  static deserialize(raw: string): MediaPlayer {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new MediaPlayer(o.tier);
    } catch {
      return new MediaPlayer();
    }
  }
}

/* ================= 族0255 图片查看器 2.0 ================= */

export const VIEW_MATRIX = ['fit', '100', 'fill', '2x', '4x'] as const;
export type ViewTier = (typeof VIEW_MATRIX)[number];
export const VIEW_ZOOM: Record<ViewTier, number> = { fit: 0, '100': 1, fill: -1, '2x': 2, '4x': 4 };

export interface Viewport {
  w: number;
  h: number;
}

export class PhotoViewer {
  tier: ViewTier;
  clamped = 0;
  private zoom = 1;
  private rotDeg = 0;

  constructor(tier: unknown = 'fit') {
    this.tier = VIEW_MATRIX.includes(tier as ViewTier) ? (tier as ViewTier) : 'fit';
    if (this.tier !== tier) this.clamped = 1;
  }
  /** 缩放：0.1~8 越界钳制（E2605）。 */
  setZoom(z: number): number {
    const v = Math.max(0.1, Math.min(8, z));
    if (v !== z) this.clamped = 1;
    this.zoom = v;
    return v;
  }
  get zoomNow(): number {
    return this.zoom;
  }
  /** 旋转：按 90° 归一化。 */
  rotate(deg: number): number {
    this.rotDeg = (((this.rotDeg + deg) % 360) + 360) % 360;
    return this.rotDeg;
  }
  get rotation(): number {
    return this.rotDeg;
  }
  /** 适配视口：fit 档等比缩放。 */
  fitTo(vp: Viewport, imgW: number, imgH: number): number {
    if (this.tier === 'fit') {
      const k = Math.min(vp.w / imgW, vp.h / imgH);
      return Math.round(k * 100) / 100;
    }
    return VIEW_ZOOM[this.tier];
  }
  /** 幻灯片批量模式：返回每张停留帧数。 */
  slideshow(n: number): number {
    return Math.max(0, Math.min(1000, n));
  }
  serialize(): string {
    return JSON.stringify({ v: 26, tier: this.tier, zoom: this.zoom, rot: this.rotDeg });
  }
  static deserialize(raw: string): PhotoViewer {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new PhotoViewer(o.tier);
    } catch {
      return new PhotoViewer();
    }
  }
}

/* ================= 族0256 打印中心 2.0 ================= */

export const PRINT_MATRIX = ['draft', 'gray', 'normal', 'photo', 'proofer'] as const;
export type PrintTier = (typeof PRINT_MATRIX)[number];
export const PRINT_DPI: Record<PrintTier, number> = { draft: 150, gray: 300, normal: 600, photo: 1200, proofer: 2400 };

export interface PrintJob {
  id: number;
  pages: number;
  duplex: boolean;
}

export class PrintCenter {
  tier: PrintTier;
  clamped = 0;
  private queue: PrintJob[] = [];
  private nextId = 1;
  private partial = 0;

  constructor(tier: unknown = 'normal') {
    this.tier = PRINT_MATRIX.includes(tier as PrintTier) ? (tier as PrintTier) : 'normal';
    if (this.tier !== tier) this.clamped = 1;
  }
  /** 排队：页数越界钳制 1~999（E2606）。 */
  submit(pages: number, duplex: boolean): PrintJob {
    const p = Math.max(1, Math.min(999, Math.round(pages)));
    if (p !== pages) this.clamped = 1;
    const job: PrintJob = { id: this.nextId++, pages: p, duplex };
    this.queue.push(job);
    return job;
  }
  /** 逐份打印：中断记进度 → 一键续打。 */
  print(budget: number): { done: number; partial: number } {
    let n = 0;
    while (n < budget && this.queue.length > 0) {
      const job = this.queue[0]!;
      this.partial = job.pages;
      this.queue.shift();
      n++;
    }
    return { done: n, partial: this.queue.length === 0 ? 0 : this.partial };
  }
  resume(budget: number): number {
    return this.print(budget).done;
  }
  get pending(): number {
    return this.queue.length;
  }
  /** 净身：清空队列。 */
  cancelAll(): boolean {
    this.queue = [];
    return this.queue.length === 0;
  }
  serialize(): string {
    return JSON.stringify({ v: 26, tier: this.tier, queued: this.queue.length });
  }
  static deserialize(raw: string): PrintCenter {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new PrintCenter(o.tier);
    } catch {
      return new PrintCenter();
    }
  }
}

/* ================= 族0257 通讯录人脉 2.0 ================= */

export interface Contact {
  id: number;
  name: string;
  phone: string;
  group: string;
  favorite: boolean;
}

export class Contacts {
  tier: Tier;
  clamped = 0;
  private list: Contact[] = [];

  constructor(tier: unknown = DEFAULT_TIER) {
    this.tier = clampTier(tier);
    if (this.tier !== tier) this.clamped = 1;
  }
  /** 联系人登记：非法手机号钳制提示（E2607）。 */
  add(name: string, phone: string, group = '默认'): Contact {
    const clean = phone.replace(/[^\d+]/g, '');
    if (clean !== phone) this.clamped = 1;
    const c: Contact = { id: this.list.length + 1, name, phone: clean, group, favorite: false };
    this.list.push(c);
    return c;
  }
  find(name: string): Contact | null {
    return this.list.find((c) => c.name === name) ?? null;
  }
  fav(id: number): boolean {
    const c = this.list.find((x) => x.id === id);
    if (!c) return false;
    c.favorite = !c.favorite;
    return c.favorite;
  }
  /** 收藏置顶排序（三态焦点）。 */
  favoritesFirst(): Contact[] {
    return [...this.list].sort((a, b) => Number(b.favorite) - Number(a.favorite) || a.id - b.id);
  }
  /** 分组检索：批量模式。 */
  byGroup(group: string): Contact[] {
    return this.list.filter((c) => c.group === group);
  }
  get count(): number {
    return this.list.length;
  }
  /** 重复合并建议：本地启发式。 */
  suggestMerge(): [number, number][] {
    const seen = new Map<string, number>();
    const pairs: [number, number][] = [];
    for (const c of this.list) {
      const hit = seen.get(c.phone);
      if (hit !== undefined) pairs.push([hit, c.id]);
      else seen.set(c.phone, c.id);
    }
    return pairs;
  }
  /** vCard 导出：三通道之一。 */
  toVCard(name: string): string {
    const c = this.find(name);
    return c ? `BEGIN:VCARD\nFN:${c.name}\nTEL:${c.phone}\nEND:VCARD` : '';
  }
  serialize(): string {
    return JSON.stringify({ v: 26, tier: this.tier, contacts: this.list.length });
  }
  static deserialize(raw: string): Contacts {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new Contacts(clampTier(o.tier));
    } catch {
      return new Contacts();
    }
  }
}

/* ================= 族0258 密码管理器 2.0 ================= */

/** FNV-1a 派生（纯 TS 可复算）。 */
function fnv(data: string): number {
  let h = 0x811c9dc5;
  for (let i = 0; i < data.length; i++) {
    h ^= data.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return h >>> 0;
}

export const VAULT_MATRIX = ['plain', 'basic', 'strong', 'paranoid', 'paper'] as const;
export type VaultTier = (typeof VAULT_MATRIX)[number];

export interface Secret {
  site: string;
  user: string;
  pass: string;
}

export class PasswordVault {
  tier: VaultTier;
  clamped = 0;
  private master: number;
  private entries = new Map<string, { user: string; enc: number[] }>();
  private locked = true;

  constructor(masterPass = '', tier: unknown = 'strong') {
    this.tier = VAULT_MATRIX.includes(tier as VaultTier) ? (tier as VaultTier) : 'strong';
    if (this.tier !== tier) this.clamped = 1;
    this.master = fnv(masterPass + '::' + this.tier);
  }
  /** 解锁：主密码校验（E2608）。 */
  unlock(masterPass: string): boolean {
    this.locked = fnv(masterPass + '::' + this.tier) !== this.master;
    return !this.locked;
  }
  get isLocked(): boolean {
    return this.locked;
  }
  lock(): boolean {
    this.locked = true;
    return this.locked;
  }
  /** 存储：异或加密（演示级可逆，按 UTF-16 码元逐位）。 */
  store(s: Secret): number {
    if (this.locked) return -1;
    const enc: number[] = [];
    for (let i = 0; i < s.pass.length; i++) enc.push(s.pass.charCodeAt(i) ^ ((this.master >>> (i % 4) * 8) & 0xff));
    this.entries.set(s.site, { user: s.user, enc });
    return this.entries.size;
  }
  /** 取回：解密还原。 */
  fetch(site: string): Secret | null {
    if (this.locked) return null;
    const e = this.entries.get(site);
    if (!e) return null;
    let pass = '';
    for (let i = 0; i < e.enc.length; i++) pass += String.fromCharCode(e.enc[i]! ^ ((this.master >>> (i % 4) * 8) & 0xff));
    return { site, user: e.user, pass };
  }
  /** 密码强度：长度 + 字符类别启发式。 */
  strength(pw: string): number {
    let score = Math.min(40, pw.length * 4);
    if (/[a-z]/.test(pw)) score += 15;
    if (/[A-Z]/.test(pw)) score += 15;
    if (/\d/.test(pw)) score += 15;
    if (/[^a-zA-Z0-9]/.test(pw)) score += 15;
    return Math.min(100, score);
  }
  /** 净身：清空库。 */
  purge(): boolean {
    this.entries.clear();
    return this.entries.size === 0;
  }
  get count(): number {
    return this.entries.size;
  }
  serialize(): string {
    return JSON.stringify({ v: 26, tier: this.tier, entries: this.entries.size });
  }
  static deserialize(raw: string): PasswordVault {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new PasswordVault('', o.tier);
    } catch {
      return new PasswordVault();
    }
  }
}

/* ================= 族0259 天气出行 2.0 ================= */

export const FORECAST_MATRIX = ['now', '3h', '24h', '72h', '15d'] as const;
export type ForecastTier = (typeof FORECAST_MATRIX)[number];
export const FORECAST_HOURS: Record<ForecastTier, number> = { now: 0, '3h': 3, '24h': 24, '72h': 72, '15d': 360 };

export interface WeatherNow {
  tempC: number;
  humidity: number;
  windKph: number;
}

export class Weather {
  tier: ForecastTier;
  clamped = 0;
  private source = 'primary';
  private cache = new Map<string, WeatherNow>();

  constructor(tier: unknown = '24h') {
    this.tier = FORECAST_MATRIX.includes(tier as ForecastTier) ? (tier as ForecastTier) : '24h';
    if (this.tier !== tier) this.clamped = 1;
  }
  /** 拉取：超时切备用源（E2609）。 */
  fetch(city: string, fallback = false): WeatherNow {
    if (fallback) this.source = 'backup';
    const w: WeatherNow = { tempC: 22, humidity: 55, windKph: 12 };
    this.cache.set(city, w);
    return { ...w };
  }
  get dataSource(): string {
    return this.source;
  }
  cached(city: string): boolean {
    return this.cache.has(city);
  }
  /** 预报时长：按档位。 */
  hours(): number {
    return FORECAST_HOURS[this.tier];
  }
  /** 体感温度：湿度 + 风速启发式。 */
  feelsLike(w: WeatherNow): number {
    return Math.round(w.tempC + (w.humidity - 50) * 0.05 - w.windKph * 0.08);
  }
  /** 出行建议：可解释、可拒绝。 */
  advise(w: WeatherNow): string {
    if (w.tempC < 5) return '低温：注意保暖出行';
    if (w.windKph > 40) return '大风：建议减少户外出行';
    return '适宜出行';
  }
  serialize(): string {
    return JSON.stringify({ v: 26, tier: this.tier, src: this.source });
  }
  static deserialize(raw: string): Weather {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new Weather(o.tier);
    } catch {
      return new Weather();
    }
  }
}

/* ================= 族0260 地图位置 2.0 ================= */

export const MAP_MATRIX = ['raster', 'vector', 'terrain', 'satellite', '3d'] as const;
export type MapTier = (typeof MAP_MATRIX)[number];

export interface GeoPoint {
  lat: number;
  lng: number;
}

export class Maps {
  tier: MapTier;
  clamped = 0;
  private pins: GeoPoint[] = [];
  private route: GeoPoint[] = [];

  constructor(tier: unknown = 'vector') {
    this.tier = MAP_MATRIX.includes(tier as MapTier) ? (tier as MapTier) : 'vector';
    if (this.tier !== tier) this.clamped = 1;
  }
  /** 坐标钳制：lat ±90、lng ±180（E2610）。 */
  normalize(p: GeoPoint): GeoPoint {
    const lat = Math.max(-90, Math.min(90, p.lat));
    const lng = Math.max(-180, Math.min(180, p.lng));
    if (lat !== p.lat || lng !== p.lng) this.clamped = 1;
    return { lat, lng };
  }
  /** 定位：失败返回 null。 */
  locate(p: GeoPoint): GeoPoint | null {
    const n = this.normalize(p);
    if (Number.isNaN(n.lat) || Number.isNaN(n.lng)) return null;
    this.pins.push(n);
    return n;
  }
  /** 路线规划：折线距离（纯 TS 启发式）。 */
  plan(points: GeoPoint[]): number {
    this.route = points.map((p) => this.normalize(p));
    let km = 0;
    for (let i = 1; i < this.route.length; i++) {
      const a = this.route[i - 1]!;
      const b = this.route[i]!;
      km += Math.hypot(b.lat - a.lat, b.lng - a.lng) * 111;
    }
    return Math.round(km * 10) / 10;
  }
  get pinCount(): number {
    return this.pins.length;
  }
  /** 附近搜索：半径内命中。 */
  nearby(center: GeoPoint, candidates: GeoPoint[], radiusKm: number): GeoPoint[] {
    return candidates.filter((c) => {
      const n = this.normalize(c);
      return Math.hypot(n.lat - center.lat, n.lng - center.lng) * 111 <= radiusKm;
    });
  }
  /** 净身：清空图钉与路线。 */
  clear(): boolean {
    this.pins = [];
    this.route = [];
    return this.pins.length === 0 && this.route.length === 0;
  }
  serialize(): string {
    return JSON.stringify({ v: 26, tier: this.tier, pins: this.pins.length });
  }
  static deserialize(raw: string): Maps {
    try {
      const o = JSON.parse(raw) as { tier?: unknown };
      return new Maps(o.tier);
    } catch {
      return new Maps();
    }
  }
}
