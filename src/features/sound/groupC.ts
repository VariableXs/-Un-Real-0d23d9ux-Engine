// AURORA-10000: AI-63 批次（领域13 声音与通知 · 族0311~0315 · F07751~F07875），勿删。
// 通知工作流 / 媒体控制统一 / 闹钟体系 / 计时器秒表 / 通知无障碍。

/* ===================== 族0311 通知工作流 ===================== */

export interface WorkflowRule {
  id: string;
  keyword: string;
  action: 'mute' | 'to-todo' | 'to-calendar' | 'to-note' | 'to-clipboard';
}

export interface PopupCondition {
  wifiOnly?: boolean;
  onWifi?: boolean;
  chargingOnly?: boolean;
  charging?: boolean;
  foregroundOnly?: boolean;
  foreground?: boolean;
}

export interface LaterItem {
  id: string;
  title: string;
  app: string;
  at: number;
  context?: string;
}

/** 通知工作流：一键转换 / 关键词静音 / 弹出条件 / 稍后队列 / 全文与归档搜索。 */
export class NotificationWorkflow {
  private rules: WorkflowRule[] = [];
  private queue: LaterItem[] = [];
  private archive: LaterItem[] = [];
  private exports: string[] = [];

  addRule(rule: WorkflowRule): void {
    this.rules.push(rule);
  }

  rulesAll(): readonly WorkflowRule[] {
    return this.rules;
  }

  routeAction(title: string): WorkflowRule['action'] | null {
    return this.rules.find((r) => title.includes(r.keyword))?.action ?? null;
  }

  exportRules(): string {
    return JSON.stringify(this.rules);
  }

  importRules(json: string): boolean {
    try {
      const raw = JSON.parse(json) as WorkflowRule[];
      if (!Array.isArray(raw)) return false;
      this.rules.push(...raw);
      return true;
    } catch {
      return false;
    }
  }

  addLater(item: LaterItem): void {
    this.queue.push(item);
  }

  laterAll(): readonly LaterItem[] {
    return this.queue;
  }

  processQueue(): LaterItem[] {
    const done = this.queue.splice(0);
    this.archive.push(...done);
    return done;
  }

  searchAll(query: string): LaterItem[] {
    return [...this.queue, ...this.archive].filter((i) => i.title.includes(query) || i.app.includes(query));
  }

  searchArchive(query: string): LaterItem[] {
    return this.archive.filter((i) => i.title.includes(query));
  }

  exportAll(): string {
    const payload = JSON.stringify({ queue: this.queue, archive: this.archive });
    this.exports.push(payload);
    return payload;
  }

  exportCount(): number {
    return this.exports.length;
  }
}

/** 弹出条件判定：仅 WiFi / 充电时 / 前台弹。 */
export function popupAllowedByCondition(cond: PopupCondition): boolean {
  if (cond.wifiOnly && !cond.onWifi) return false;
  if (cond.chargingOnly && !cond.charging) return false;
  if (cond.foregroundOnly && !cond.foreground) return false;
  return true;
}

/* ===================== 族0312 媒体控制统一 ===================== */

export interface MediaSource {
  id: string;
  app: string;
  title: string;
  artist: string;
  positionSec: number;
  durationSec: number;
  playing: boolean;
}

/** 媒体统一控制：全局面板 / 多源切换 / SMTC 集成模型 / 独占仲裁 / 耳机事件。 */
export class MediaControlHub {
  private sources = new Map<string, MediaSource>();
  private activeId: string | null = null;
  private history: string[] = [];
  private sleepTimerEnd = 0;
  private lastResume: { sourceId: string; positionSec: number } | null = null;

  register(src: MediaSource): void {
    this.sources.set(src.id, src);
  }

  sourcesAll(): MediaSource[] {
    return [...this.sources.values()];
  }

  activate(id: string): boolean {
    if (!this.sources.has(id)) return false;
    this.activeId = id;
    this.history.push(id);
    return true;
  }

  active(): MediaSource | null {
    return this.activeId ? this.sources.get(this.activeId) ?? null : null;
  }

  /** SMTC 集成：把当前源映射为系统媒体传输控件。 */
  smtcSnapshot(): { title: string; artist: string; playing: boolean } | null {
    const a = this.active();
    return a ? { title: a.title, artist: a.artist, playing: a.playing } : null;
  }

  togglePlay(): boolean {
    const a = this.active();
    if (!a) return false;
    a.playing = !a.playing;
    return a.playing;
  }

  seek(deltaSec: number): number | null {
    const a = this.active();
    if (!a) return null;
    a.positionSec = Math.max(0, Math.min(a.durationSec, a.positionSec + deltaSec));
    return a.positionSec;
  }

  /** 独占仲裁：同一时刻只允许一个 playing 源。 */
  arbitrate(): MediaSource | null {
    const playing = this.sourcesAll().filter((s) => s.playing);
    if (playing.length <= 1) return this.active();
    const keep = this.active() ?? playing[0]!;
    for (const s of playing) if (s.id !== keep.id) s.playing = false;
    return keep;
  }

  /** 拔耳机暂停：返回是否应暂停。 */
  onHeadphone(connected: boolean): 'play' | 'pause' | 'none' {
    return connected ? 'none' : 'pause';
  }

  /** 耳机双击切歌。 */
  headphoneDoubleClick(): 'next' | 'none' {
    return this.active() ? 'next' : 'none';
  }

  setSleepTimer(endAt: number): void {
    this.sleepTimerEnd = endAt;
  }

  shouldStopAt(now: number): boolean {
    return this.sleepTimerEnd > 0 && now >= this.sleepTimerEnd;
  }

  /** 续播：换设备时保存位置。 */
  saveResume(sourceId: string, positionSec: number): void {
    this.lastResume = { sourceId, positionSec };
  }

  resume(): { sourceId: string; positionSec: number } | null {
    return this.lastResume;
  }

  historyAll(): readonly string[] {
    return this.history;
  }
}

/* ===================== 族0313 闹钟体系 ===================== */

export type Weekday = 0 | 1 | 2 | 3 | 4 | 5 | 6;

export interface Alarm {
  id: string;
  hour: number;
  minute: number;
  label: string;
  enabled: boolean;
  repeatDays: Weekday[];
  ringtone: string;
  volume: number;
  rampMinutes: number;
  snoozeMinutes: number;
  snoozeLimit: number;
  dismissMode: 'tap' | 'math' | 'voice';
  timezoneOffsetMin?: number;
  bypassDnd: boolean;
}

export interface AlarmRingEvent {
  alarmId: string;
  at: number;
  snoozed: boolean;
  dismissedBy: 'tap' | 'math' | 'voice' | 'snooze-limit';
}

/** 闹钟体系：多闹钟并行 / 重复周期 / 渐强 / 贪睡上限 / 算术与语音关闭 / 勿扰例外。 */
export class AlarmSystem {
  private alarms = new Map<string, Alarm>();
  private history: AlarmRingEvent[] = [];
  private snoozeCount = new Map<string, number>();

  add(a: Omit<Alarm, 'id'> & { id: string }): boolean {
    if (this.alarms.has(a.id)) return false;
    this.alarms.set(a.id, { ...a });
    return true;
  }

  get(id: string): Alarm | undefined {
    return this.alarms.get(id);
  }

  remove(id: string): boolean {
    return this.alarms.delete(id);
  }

  all(): Alarm[] {
    return [...this.alarms.values()];
  }

  /** 是否在 hh:mm 触发（含一次性与重复周期）。 */
  shouldRing(id: string, day: Weekday, hh: number, mm: number): boolean {
    const a = this.alarms.get(id);
    if (!a || !a.enabled) return false;
    if (a.hour !== hh || a.minute !== mm) return false;
    if (a.repeatDays.length === 0) return true;
    return a.repeatDays.includes(day);
  }

  /** 渐强曲线：ramp 分钟内音量线性爬升至目标音量。 */
  rampVolume(a: Alarm, elapsedSec: number): number {
    if (a.rampMinutes <= 0) return a.volume;
    const p = Math.min(1, elapsedSec / (a.rampMinutes * 60));
    return Math.round(a.volume * p * 100) / 100;
  }

  snooze(id: string, at: number): boolean {
    const a = this.alarms.get(id);
    if (!a) return false;
    const n = (this.snoozeCount.get(id) ?? 0) + 1;
    if (n > a.snoozeLimit) return false;
    this.snoozeCount.set(id, n);
    this.history.push({ alarmId: id, at, snoozed: true, dismissedBy: 'snooze-limit' });
    return true;
  }

  dismiss(id: string, by: 'tap' | 'math' | 'voice', at: number): boolean {
    const a = this.alarms.get(id);
    if (!a) return false;
    if (by !== a.dismissMode && by !== 'tap') return false;
    this.snoozeCount.delete(id);
    this.history.push({ alarmId: id, at, snoozed: false, dismissedBy: by });
    return true;
  }

  /** 算术关闭：出题与验算。 */
  mathChallenge(seed: number): { q: string; answer: number } {
    const a = 3 + (seed % 9);
    const b = 4 + (seed % 7);
    return { q: `${a} × ${b} + ${seed % 10} = ?`, answer: a * b + (seed % 10) };
  }

  historyAll(): readonly AlarmRingEvent[] {
    return this.history;
  }

  /** 时差闹钟：本地时刻 + 目标时区偏移换算目标地时刻。 */
  localTimeIn(a: Alarm, localMinutes: number): number {
    return (localMinutes + (a.timezoneOffsetMin ?? 0) + 1440) % 1440;
  }
}

/* ===================== 族0314 计时器秒表 ===================== */

export const TIMER_PRESETS: Array<{ id: string; name: string; seconds: number }> = [
  { id: 'noodle', name: '泡面 3 分钟', seconds: 180 },
  { id: 'egg-soft', name: '溏心蛋 6 分钟', seconds: 360 },
  { id: 'tea', name: '泡茶 5 分钟', seconds: 300 },
  { id: 'focus-25', name: '专注 25 分钟', seconds: 1500 },
];

export interface Timer {
  id: string;
  name: string;
  seconds: number;
  color: string;
  doneSound: string;
  notify: boolean;
  repeat: boolean;
}

export interface LapRecord {
  index: number;
  atMs: number;
}

/** 计时器与秒表：多并行 / 预设 / 番茄节奏 / 分段导出 / 悬浮与锁屏位。 */
export class TimerStopwatch {
  private timers = new Map<string, { t: Timer; endsAt: number }>();
  private stopwatchStart = 0;
  private laps: LapRecord[] = [];
  private lastLapMs = 0;

  addTimer(t: Timer, now: number): void {
    this.timers.set(t.id, { t, endsAt: now + t.seconds * 1000 });
  }

  remaining(id: string, now: number): number {
    const e = this.timers.get(id);
    return e ? Math.max(0, e.endsAt - now) : 0;
  }

  finished(now: number): string[] {
    return [...this.timers.entries()].filter(([, e]) => e.endsAt <= now).map(([id]) => id);
  }

  timersAll(): Timer[] {
    return [...this.timers.values()].map((e) => e.t);
  }

  startStopwatch(atMs: number): void {
    this.stopwatchStart = atMs;
    this.laps = [];
    this.lastLapMs = 0;
  }

  lap(atMs: number): LapRecord {
    const rec = { index: this.laps.length + 1, atMs: atMs - this.stopwatchStart - this.lastLapMs };
    this.lastLapMs = atMs - this.stopwatchStart;
    this.laps.push(rec);
    return rec;
  }

  lapsAll(): readonly LapRecord[] {
    return this.laps;
  }

  exportLaps(): string {
    return JSON.stringify(this.laps);
  }

  /** 番茄节奏：25 分钟工作 + 5 分钟短休，每 4 轮 15 分钟长休。 */
  pomodoroPhase(elapsedMin: number): { phase: 'work' | 'short-break' | 'long-break'; round: number } {
    const cycle = 30;
    const round = Math.floor(elapsedMin / cycle) + 1;
    const inCycle = elapsedMin % cycle;
    const phase = inCycle < 25 ? 'work' : (round % 4 === 0 ? 'long-break' : 'short-break');
    return { phase, round };
  }

  /** 间歇训练：workSec/restSec 交替，返回当前段。 */
  intervalPhase(elapsedSec: number, workSec: number, restSec: number): 'work' | 'rest' {
    const period = workSec + restSec;
    return elapsedSec % period < workSec ? 'work' : 'rest';
  }
}

/* ===================== 族0315 通知无障碍 ===================== */

export interface NotifyA11ySettings {
  screenReaderAnnounce: boolean;
  largeText: boolean;
  highContrast: boolean;
  focusReachable: boolean;
  keyboardOperable: boolean;
  soundIdentityPerApp: boolean;
  directionalFlash: boolean;
  vibrationPatterns: boolean;
  strongCue: boolean; // 声光振三合一
  readConfirmation: boolean;
  displayDelayMs: number;
  simplifiedMode: boolean;
  flashProtect: boolean;
}

export const DEFAULT_NOTIFY_A11Y: NotifyA11ySettings = {
  screenReaderAnnounce: true, largeText: false, highContrast: false, focusReachable: true,
  keyboardOperable: true, soundIdentityPerApp: false, directionalFlash: false,
  vibrationPatterns: false, strongCue: false, readConfirmation: false, displayDelayMs: 3000,
  simplifiedMode: false, flashProtect: true,
};

/** 通知无障碍：朗读 / 焦点可达 / 振动编码 / 简化模式 / 闪光保护。 */
export class NotifyAccessibility {
  private settings: NotifyA11ySettings;
  private announceQueue: string[] = [];

  constructor(settings: Partial<NotifyA11ySettings> = {}) {
    this.settings = { ...DEFAULT_NOTIFY_A11Y, ...settings };
  }

  settings_(): NotifyA11ySettings {
    return { ...this.settings };
  }

  update(patch: Partial<NotifyA11ySettings>): void {
    this.settings = { ...this.settings, ...patch };
  }

  /** 朗读队列：保持堆叠顺序先进先出。 */
  announce(id: string, text: string): void {
    if (!this.settings.screenReaderAnnounce) return;
    this.announceQueue.push(`${id}:${text}`);
  }

  announcedAll(): readonly string[] {
    return this.announceQueue;
  }

  /** 振动编码：类别 → 点阵节奏。 */
  vibrationPattern(category: 'notify' | 'warning' | 'error'): number[] {
    switch (category) {
      case 'notify': return [80];
      case 'warning': return [80, 60, 80];
      case 'error': return [80, 60, 80, 60, 80];
    }
  }

  /** 展示延迟：给读屏留出时间。 */
  displayDuration(): number {
    const base = this.settings.displayDelayMs;
    return this.settings.screenReaderAnnounce ? base * 2 : base;
  }

  /** 简化模式：只留标题。 */
  renderTitle(title: string, body: string): string {
    return this.settings.simplifiedMode ? title : `${title} ${body}`.trim();
  }

  /** 闪光保护：敏感用户禁用闪光替代。 */
  flashAllowed(): boolean {
    return !this.settings.flashProtect;
  }
}
