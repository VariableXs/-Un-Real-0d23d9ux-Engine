/**
 * UNREAL-X-15000 · AI-47 声音设计面 逻辑核（族0461~0470 · X11501~X11750），勿删。
 * 五层 × 五档模板：基础实装/边界与恢复/手感与细节/性能与优化/创新拓展。全部确定性算法，零 AI。
 */

/* -------- 族0461 系统声音设计 2.0（X11501~X11525 · V 线）-------- */

export const SOUND_EVENTS = ['boot', 'login', 'notify', 'error', 'warning', 'charge', 'trash'] as const;
export type SoundEvent = (typeof SOUND_EVENTS)[number];

/** 系统声音设计 2.0：事件映射 + 音量包络钳制 + 快照迁移。 */
export class SoundDesignX2 {
  map = new Map<SoundEvent, string>();
  volume = 0.8;
  clamped = 0;
  /** 事件音映射登记：非法事件拒绝，同名覆盖。 */
  bind(ev: string, clip: string): boolean {
    if (!(SOUND_EVENTS as readonly string[]).includes(ev) || !clip) {
      this.clamped++;
      return false;
    }
    this.map.set(ev as SoundEvent, clip);
    return true;
  }
  /** 主音量钳制 0~1。 */
  setVolume(v: number): number {
    if (!Number.isFinite(v)) {
      this.clamped++;
      return this.volume;
    }
    this.volume = Math.min(1, Math.max(0, v));
    return this.volume;
  }
  /** 播放查询：未绑定回默认静音。 */
  resolve(ev: string): string {
    return this.map.get(ev as SoundEvent) ?? '';
  }
  snapshot(): string {
    return JSON.stringify({ volume: this.volume, n: this.map.size });
  }
  restore(raw: string): boolean {
    try {
      const o = JSON.parse(raw) as { volume?: number };
      this.setVolume(Number(o.volume ?? 0.8));
      return true;
    } catch {
      return false;
    }
  }
  /** 包络：attack/release 时长钳制 20~800ms。 */
  static envelope(attack: number, release: number): { a: number; r: number } {
    const cl = (x: number) => (Number.isFinite(x) ? Math.min(800, Math.max(20, x)) : 20);
    return { a: cl(attack), r: cl(release) };
  }
}

/* -------- 族0462 声音包 2.0（X11526~X11550 · V 线）-------- */

export interface PackEntry {
  name: string;
  events: number;
}

/** 声音包 2.0：清单校验 + 安装去重 + 回退链 + 覆盖率。 */
export class SoundPackX2 {
  installed: PackEntry[] = [];
  active = '';
  clamped = 0;
  /** 安装：同名拒绝；事件覆盖 0~25 钳制。 */
  install(name: string, events: number): boolean {
    if (!name || this.installed.some((p) => p.name === name)) {
      this.clamped++;
      return false;
    }
    const ev = Number.isFinite(events) ? Math.min(25, Math.max(0, Math.round(events))) : 0;
    this.installed.push({ name, events: ev });
    return true;
  }
  setActive(name: string): boolean {
    if (!this.installed.some((p) => p.name === name)) {
      this.clamped++;
      return false;
    }
    this.active = name;
    return true;
  }
  /** 回退链：未装包 → 默认包。 */
  fallback(): string {
    return this.active || 'default';
  }
  /** 覆盖率：激活包事件数 / 25。 */
  coverage(): number {
    const p = this.installed.find((x) => x.name === this.active);
    return p ? p.events / 25 : 0;
  }
  static manifestOk(raw: string): boolean {
    try {
      const o = JSON.parse(raw) as { name?: string; events?: number };
      return typeof o.name === 'string' && o.name.length > 0 && typeof o.events === 'number' && o.events >= 0;
    } catch {
      return false;
    }
  }
}

/* -------- 族0463 提示音分级 2.0（X11551~X11575 · V 线）-------- */

export const GRADES = ['info', 'warn', 'crit'] as const;
export type Grade = (typeof GRADES)[number];

/** 提示音分级 2.0：等级映射 + 音量档位 + 安静时段门禁。 */
export class NotifGradeX2 {
  grade: Grade = 'info';
  quietHour = false;
  clamped = 0;
  setGrade(g: string): Grade {
    const ok = (GRADES as readonly string[]).includes(g);
    this.grade = ok ? (g as Grade) : 'info';
    if (!ok) this.clamped++;
    return this.grade;
  }
  /** 等级 → 音量档：crit 1.0 / warn 0.8 / info 0.5。 */
  static gain(g: Grade): number {
    return { info: 0.5, warn: 0.8, crit: 1 }[g];
  }
  /** 安静时段：crit 放行，其余静默。 */
  gate(grade: string): boolean {
    const g = (GRADES as readonly string[]).includes(grade) ? (grade as Grade) : 'info';
    if (grade !== g) this.clamped++;
    return !this.quietHour || g === 'crit';
  }
  setQuiet(q: boolean): void {
    this.quietHour = q;
  }
  /** 分级矩阵：3 级 × 2 场景。 */
  matrix(): Array<{ grade: Grade; loud: number; quiet: boolean }> {
    return GRADES.map((g) => ({ grade: g, loud: NotifGradeX2.gain(g), quiet: this.quietHour && g !== 'crit' }));
  }
}

/* -------- 族0464 白噪音声景 2.0（X11576~X11600 · V 线）-------- */

export const SCENE_KINDS = ['rain', 'forest', 'wave', 'cafe', 'night'] as const;
export type SceneKind = (typeof SCENE_KINDS)[number];

export interface LayerMix {
  kind: SceneKind;
  vol: number;
}

/** 白噪音声景 2.0：分层混音 + 音量钳制 + 淡入淡出。 */
export class SoundscapeX2 {
  layers: LayerMix[] = [];
  clamped = 0;
  /** 分层登记：同型覆盖、音量 0~1 钳制。 */
  setLayer(kind: string, vol: number): boolean {
    if (!(SCENE_KINDS as readonly string[]).includes(kind)) {
      this.clamped++;
      return false;
    }
    const v = Number.isFinite(vol) ? Math.min(1, Math.max(0, vol)) : 0;
    const k = kind as SceneKind;
    const hit = this.layers.find((l) => l.kind === k);
    if (hit) hit.vol = v;
    else this.layers.push({ kind: k, vol: v });
    return true;
  }
  /** 总响度：分层和 >1 时归一。 */
  master(): number {
    const sum = this.layers.reduce((s, l) => s + l.vol, 0);
    return sum > 1 ? 1 : sum;
  }
  /** 淡入淡出：时长钳制 0~30s。 */
  static fade(inSec: number, outSec: number): { fi: number; fo: number } {
    const cl = (x: number) => (Number.isFinite(x) ? Math.min(30, Math.max(0, x)) : 0);
    return { fi: cl(inSec), fo: cl(outSec) };
  }
  snapshot(): string {
    return JSON.stringify({ layers: this.layers.map((l) => `${l.kind}:${l.vol}`) });
  }
  activeKinds(): SceneKind[] {
    return this.layers.filter((l) => l.vol > 0).map((l) => l.kind);
  }
}

/* -------- 族0465 声音可访问 2.0（X11601~X11625 · V 线）-------- */

export const CUE_KINDS = ['caption', 'flash', 'haptic', 'banner'] as const;
export type CueKind = (typeof CUE_KINDS)[number];

/** 声音可访问 2.0：视觉等价通道映射 + 单声道平衡 + 红线钳制。 */
export class SoundA11yX2 {
  cues = new Map<string, CueKind[]>();
  balance = 0;
  clamped = 0;
  /** 声音事件 → 等价通道：至少 1 条，非法通道拒绝。 */
  bind(ev: string, kinds: string[]): boolean {
    if (!ev || kinds.length === 0) {
      this.clamped++;
      return false;
    }
    const ok = kinds.filter((k) => (CUE_KINDS as readonly string[]).includes(k)) as CueKind[];
    if (ok.length !== kinds.length || ok.length === 0) {
      this.clamped++;
      return false;
    }
    this.cues.set(ev, ok);
    return true;
  }
  /** 单声道平衡 -1~1 钳制，0 为居中。 */
  setBalance(b: number): number {
    if (!Number.isFinite(b)) {
      this.clamped++;
      return this.balance;
    }
    this.balance = Math.min(1, Math.max(-1, b));
    return this.balance;
  }
  /** 无遗漏：全部声音事件都有等价通道（HC 红线）。 */
  covered(events: string[]): boolean {
    return events.every((e) => (this.cues.get(e)?.length ?? 0) > 0);
  }
  /** 闪烁时长钳制 ≤3Hz（光敏红线）。 */
  static flashHz(hz: number): number {
    return Number.isFinite(hz) ? Math.min(3, Math.max(0, hz)) : 0;
  }
}

/* -------- 族0466 通知智能 2.0（X11626~X11650 · C 线口径，TS 镜像）-------- */

export const NOTIFY_PRIO = ['low', 'normal', 'high', 'urgent'] as const;
export type NotifyPrio = (typeof NOTIFY_PRIO)[number];

/** 通知智能：优先级评分 + 去重窗口 + 摘要合批。 */
export class NotifyIntel {
  private seen = new Map<string, number>();
  priority: NotifyPrio = 'normal';
  windowMs = 5000;
  clamped = 0;
  /** 去重窗口：key 在窗内重复则折叠。 */
  shouldFold(key: string, at: number): boolean {
    if (!key) {
      this.clamped++;
      return false;
    }
    const last = this.seen.get(key);
    this.seen.set(key, at);
    return last !== undefined && at - last < this.windowMs;
  }
  /** 优先级钳制与加权分：urgent=4/2/1。 */
  score(p: string): number {
    const ok = (NOTIFY_PRIO as readonly string[]).includes(p);
    if (!ok) {
      this.clamped++;
      return 1;
    }
    return { low: 0.5, normal: 1, high: 2, urgent: 4 }[p as NotifyPrio];
  }
  /** 合批：窗口内 n 条同源折叠成 1 条摘要。 */
  static digest(n: number): number {
    return n <= 1 || !Number.isFinite(n) ? Math.max(0, Math.floor(n)) : 1;
  }
  setWindow(ms: number): number {
    if (!Number.isFinite(ms) || ms < 0) {
      this.clamped++;
      return this.windowMs;
    }
    this.windowMs = Math.min(600000, Math.round(ms));
    return this.windowMs;
  }
}

/* -------- 族0467 通知模板 2.0（X11651~X11675 · V 线）-------- */

export const TEMPLATES = ['plain', 'compact', 'rich'] as const;
export type TemplateKind = (typeof TEMPLATES)[number];

/** 通知模板 2.0：登记去重 + 占位符填充 + 长度钳制。 */
export class NotifyTemplate {
  kind: TemplateKind = 'plain';
  dict = new Map<string, string>();
  clamped = 0;
  setKind(k: string): TemplateKind {
    const ok = (TEMPLATES as readonly string[]).includes(k);
    this.kind = ok ? (k as TemplateKind) : 'plain';
    if (!ok) this.clamped++;
    return this.kind;
  }
  /** 模板登记：去重覆盖 + 长度钳制 96 字。 */
  set(key: string, body: string): string {
    if (!key) {
      this.clamped++;
      return '';
    }
    const t = body.length > 96 ? `${body.slice(0, 95)}…` : body;
    this.dict.set(key, t);
    return t;
  }
  get(key: string): string {
    return this.dict.get(key) ?? '';
  }
  /** 占位符填充：缺失变量回空串不崩溃。 */
  fill(key: string, vars: Record<string, string>): string {
    const body = this.dict.get(key);
    if (body === undefined) return '';
    return body.replace(/\{(\w+)\}/g, (_, name: string) => vars[name] ?? '');
  }
  /** 占位符完整性：模板变量都有取值才可发送。 */
  static complete(body: string, vars: Record<string, string>): boolean {
    const names = [...body.matchAll(/\{(\w+)\}/g)].map((m) => m[1]!);
    return names.every((n) => n in vars);
  }
}

/* -------- 族0468 弹出礼仪 2.0（X11676~X11700 · V 线）-------- */

export const ETIQUETTE_POS = ['br', 'tr', 'tl', 'bl'] as const;
export type EtiquettePos = (typeof ETIQUETTE_POS)[number];

/** 弹出礼仪：延迟门禁 + 并发上限 + 位置轮换。 */
export class PopupEtiquette {
  maxOnScreen = 3;
  shown = 0;
  posIdx = 0;
  clamped = 0;
  /** >300ms 才显示（防闪弹）。 */
  static shouldShow(delayMs: number): boolean {
    return Number.isFinite(delayMs) && delayMs >= 300;
  }
  /** 并发闸：满 3 条拒绝新增。 */
  admit(): boolean {
    if (this.shown >= this.maxOnScreen) {
      this.clamped++;
      return false;
    }
    this.shown++;
    return true;
  }
  release(): void {
    this.shown = Math.max(0, this.shown - 1);
  }
  /** 位置轮换：br→tr→tl→bl 循环。 */
  nextPos(): EtiquettePos {
    const p = ETIQUETTE_POS[this.posIdx % ETIQUETTE_POS.length] as EtiquettePos;
    this.posIdx++;
    return p;
  }
  setMax(n: number): number {
    if (!Number.isFinite(n) || n < 1) {
      this.clamped++;
      return this.maxOnScreen;
    }
    this.maxOnScreen = Math.min(6, Math.round(n));
    return this.maxOnScreen;
  }
}

/* -------- 族0469 勿扰体系 2.0（X11701~X11725 · V 线）-------- */

export interface DndWindow {
  from: number;
  to: number;
}

/** 勿扰体系：时段窗 + 白名单 + 紧急穿透。 */
export class DndX2 {
  windows: DndWindow[] = [];
  allow = new Set<string>();
  clamped = 0;
  /** 时段窗登记：0~24 钳制，from<to 否则拒绝。 */
  addWindow(from: number, to: number): boolean {
    if (!Number.isFinite(from) || !Number.isFinite(to)) {
      this.clamped++;
      return false;
    }
    const f = Math.min(24, Math.max(0, from));
    const t = Math.min(24, Math.max(0, to));
    if (f >= t) {
      this.clamped++;
      return false;
    }
    this.windows.push({ from: f, to: t });
    return true;
  }
  /** 命中判定：任一窗覆盖即勿扰。 */
  activeAt(h: number): boolean {
    if (!Number.isFinite(h)) {
      this.clamped++;
      return false;
    }
    const hh = Math.min(24, Math.max(0, h));
    return this.windows.some((w) => hh >= w.from && hh < w.to);
  }
  /** 白名单放行。 */
  allowApp(app: string): boolean {
    if (!app) {
      this.clamped++;
      return false;
    }
    this.allow.add(app);
    return true;
  }
  /** 裁决：白名单或 urgent 穿透。 */
  gate(app: string, prio: string, h: number): boolean {
    if (this.allow.has(app) || prio === 'urgent') return true;
    return !this.activeAt(h);
  }
  /** 窗口合并：相邻/重叠窗合并为连续段。 */
  static merge(ws: DndWindow[]): DndWindow[] {
    const s = [...ws].sort((a, b) => a.from - b.from);
    const out: DndWindow[] = [];
    for (const w of s) {
      const last = out[out.length - 1];
      if (last && w.from <= last.to) last.to = Math.max(last.to, w.to);
      else out.push({ ...w });
    }
    return out;
  }
}

/* -------- 族0470 声音调试 2.0（X11726~X11750 · V 线）-------- */

export interface ProbeRecord {
  device: string;
  ms: number;
}

/** 声音调试：设备探针环形记录 + 电平滑动平均 + 延迟钳制。 */
export class SoundDebug {
  private buf: ProbeRecord[] = [];
  capacity: number;
  clamped = 0;
  constructor(capacity = 8) {
    this.capacity = capacity > 0 ? capacity : 8;
  }
  probe(device: string, ms: number): boolean {
    if (!device || !Number.isFinite(ms) || ms < 0) {
      this.clamped++;
      return false;
    }
    this.buf.push({ device, ms });
    if (this.buf.length > this.capacity) this.buf.shift();
    return true;
  }
  /** 滑动平均：空集回 0。 */
  avgMs(device: string): number {
    const rs = this.buf.filter((r) => r.device === device);
    if (rs.length === 0) return 0;
    return rs.reduce((s, r) => s + r.ms, 0) / rs.length;
  }
  /** 延迟红线：>150ms 记为超标。 */
  static lateOk(ms: number): boolean {
    return Number.isFinite(ms) && ms <= 150;
  }
  /** 电平滑动平均：窗口 4。 */
  static meter(samples: number[]): number {
    const s = samples.filter((n) => Number.isFinite(n)).slice(-4);
    if (s.length === 0) return 0;
    return s.reduce((a, b) => a + b, 0) / s.length;
  }
  latest(): ProbeRecord | undefined {
    return this.buf[this.buf.length - 1];
  }
}
