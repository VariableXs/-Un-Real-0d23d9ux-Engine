/**
 * UNREAL-X-15000 · AI-48 通知与节拍 逻辑核（族0471~0480 · X11751~X12000），勿删。
 * 五层 × 五档模板：基础实装/边界与恢复/手感与细节/性能与优化/创新拓展。全部确定性算法，零 AI。
 */

/* -------- 族0471 通知工作流 2.0（X11751~X11775 · V 线）-------- */

export const FLOW_ACTIONS = ['show', 'mute', 'digest', 'forward'] as const;
export type FlowAction = (typeof FLOW_ACTIONS)[number];
export const FLOW_STATES = ['draft', 'armed', 'fired', 'retired'] as const;
export type FlowState = (typeof FLOW_STATES)[number];

export interface FlowRule {
  app: string;
  action: FlowAction;
}

/** 通知工作流 2.0：规则表 + 状态机 + 裁决日志。 */
export class NotifyFlow {
  rules: FlowRule[] = [];
  state: FlowState = 'draft';
  log: string[] = [];
  clamped = 0;
  /** 规则登记：同名覆盖，非法动作拒绝。 */
  setRule(app: string, action: string): boolean {
    if (!app || !(FLOW_ACTIONS as readonly string[]).includes(action)) {
      this.clamped++;
      return false;
    }
    const a = action as FlowAction;
    const hit = this.rules.find((r) => r.app === app);
    if (hit) hit.action = a;
    else this.rules.push({ app, action: a });
    this.log.push(`${app}->${a}`);
    return true;
  }
  /** 裁决：命中规则回动作，未命中回 show。 */
  decide(app: string): FlowAction {
    return this.rules.find((r) => r.app === app)?.action ?? 'show';
  }
  /** 状态机：draft→armed→fired→retired，顺序不可跳。 */
  advance(): FlowState {
    const i = FLOW_STATES.indexOf(this.state);
    if (i >= FLOW_STATES.length - 1) {
      this.clamped++;
      return this.state;
    }
    this.state = FLOW_STATES[i + 1] as FlowState;
    return this.state;
  }
  snapshot(): string {
    return JSON.stringify({ state: this.state, rules: this.rules.map((r) => `${r.app}:${r.action}`) });
  }
  restore(raw: string): boolean {
    try {
      const o = JSON.parse(raw) as { state?: string };
      const s = String(o.state ?? 'draft');
      if (!(FLOW_STATES as readonly string[]).includes(s)) return false;
      this.state = s as FlowState;
      return true;
    } catch {
      return false;
    }
  }
}

/* -------- 族0472 媒体控制统一 2.0（X11776~X11800 · V 线）-------- */

/** 媒体控制统一：多播放器登记 + 焦点仲裁 + 分应用音量钳制。 */
export class MediaCtlX2 {
  players = new Map<string, number>();
  focused = '';
  clamped = 0;
  /** 播放器登记：音量 0~1 钳制。 */
  register(id: string, vol: number): boolean {
    if (!id) {
      this.clamped++;
      return false;
    }
    const v = Number.isFinite(vol) ? Math.min(1, Math.max(0, vol)) : 0;
    this.players.set(id, v);
    return true;
  }
  /** 焦点仲裁：只有已登记的播放器可获得焦点。 */
  focus(id: string): boolean {
    if (!this.players.has(id)) {
      this.clamped++;
      return false;
    }
    this.focused = id;
    return true;
  }
  /** 硬件键路由：发给焦点播放器，无焦点回 false。 */
  routeKey(cmd: string): string {
    if (!this.focused || !cmd) {
      this.clamped++;
      return '';
    }
    return `${this.focused}:${cmd}`;
  }
  /** 分应用音量：未登记回 -1 哨兵。 */
  volOf(id: string): number {
    return this.players.get(id) ?? -1;
  }
  setVol(id: string, vol: number): boolean {
    if (!this.players.has(id)) {
      this.clamped++;
      return false;
    }
    return this.register(id, vol);
  }
  count(): number {
    return this.players.size;
  }
}

/* -------- 族0473 闹钟体系 2.0（X11801~X11825 · V 线）-------- */

/** 闹钟体系：时刻校验 + 贪睡钳制 + 重复位掩码 + 下次触发。 */
export class AlarmX2 {
  hour = 7;
  minute = 0;
  snooze = 5;
  repeat = 0b1111111;
  clamped = 0;
  setTime(h: number, m: number): boolean {
    if (!Number.isInteger(h) || !Number.isInteger(m) || h < 0 || h > 23 || m < 0 || m > 59) {
      this.clamped++;
      return false;
    }
    this.hour = h;
    this.minute = m;
    return true;
  }
  /** 贪睡钳制 1~30 分钟。 */
  setSnooze(min: number): number {
    if (!Number.isFinite(min)) {
      this.clamped++;
      return this.snooze;
    }
    this.snooze = Math.min(30, Math.max(1, Math.round(min)));
    return this.snooze;
  }
  /** 重复日位掩码 0~127 钳制。 */
  setRepeat(mask: number): number {
    if (!Number.isFinite(mask)) {
      this.clamped++;
      return this.repeat;
    }
    this.repeat = Math.min(127, Math.max(0, Math.round(mask)));
    return this.repeat;
  }
  /** 触发判定：当日位为 1 即响。 */
  firesOn(day: number): boolean {
    const d = Number.isInteger(day) ? Math.min(6, Math.max(0, day)) : 0;
    return (this.repeat & (1 << d)) !== 0;
  }
  /** 下一分钟分钟数推进（确定性模拟）。 */
  static tick(h: number, m: number): { h: number; m: number } {
    let hh = h;
    let mm = m + 1;
    if (mm >= 60) {
      mm = 0;
      hh = (hh + 1) % 24;
    }
    return { h: hh, m: mm };
  }
  label(): string {
    return `${String(this.hour).padStart(2, '0')}:${String(this.minute).padStart(2, '0')}`;
  }
}

/* -------- 族0474 计时秒表 2.0（X11826~X11850 · V 线）-------- */

/** 计时秒表：倒计时钳制 + 圈次记录 + 暂停续跑。 */
export class TimerX2 {
  total = 300;
  remain = 300;
  running = false;
  laps: number[] = [];
  clamped = 0;
  /** 设置时长：1 秒 ~ 24 小时钳制。 */
  setTotal(sec: number): number {
    if (!Number.isFinite(sec)) {
      this.clamped++;
      return this.total;
    }
    const v = Math.min(86400, Math.max(1, Math.round(sec)));
    if (v !== sec) this.clamped++;
    this.total = v;
    this.remain = v;
    return v;
  }
  start(): void {
    this.running = true;
  }
  pause(): void {
    this.running = false;
  }
  /** 每秒推进：暂停不减。 */
  tick(): number {
    if (!this.running) return this.remain;
    this.remain = Math.max(0, this.remain - 1);
    return this.remain;
  }
  lap(): boolean {
    if (!this.running || this.laps.length >= 99) {
      this.clamped++;
      return false;
    }
    this.laps.push(this.total - this.remain);
    return true;
  }
  /** 时长格式化 mm:ss。 */
  static fmt(sec: number): string {
    const s = Math.max(0, Math.round(sec));
    return `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
  }
  done(): boolean {
    return this.remain === 0;
  }
}

/* -------- 族0475 通知无障碍 2.0（X11851~X11875 · V 线）-------- */

export const ANNOUNCE_PRIORITY = ['polite', 'assertive'] as const;
export type AnnouncePriority = (typeof ANNOUNCE_PRIORITY)[number];

export interface Announce {
  text: string;
  prio: AnnouncePriority;
}

/** 通知无障碍：播报队列 + aria 语义 + 闪烁钳制 + reduce-motion。 */
export class NotifyA11yX2 {
  queue: Announce[] = [];
  reducedMotion = false;
  clamped = 0;
  /** 播报入队：assertive 优先（插队）。 */
  announce(text: string, prio: string): boolean {
    if (!text || !(ANNOUNCE_PRIORITY as readonly string[]).includes(prio)) {
      this.clamped++;
      return false;
    }
    const a = { text, prio: prio as AnnouncePriority };
    if (prio === 'assertive') this.queue.unshift(a);
    else this.queue.push(a);
    return true;
  }
  next(): Announce | undefined {
    return this.queue.shift();
  }
  /** reduce-motion：清空闪烁通道，仅保留文本播报。 */
  applyReduceMotion(): void {
    this.reducedMotion = true;
  }
  /** 闪烁时长：reduce-motion 下恒 0，否则 ≤3Hz。 */
  flashHz(hz: number): number {
    return this.reducedMotion ? 0 : SoundHzClamp(hz);
  }
  /** aria 红线：文本必须非空才可播报。 */
  static ariaOk(text: string): boolean {
    return text.trim().length > 0;
  }
  queueTexts(): string[] {
    return this.queue.map((a) => a.text);
  }
}

function SoundHzClamp(hz: number): number {
  return Number.isFinite(hz) ? Math.min(3, Math.max(0, hz)) : 0;
}

/* -------- 族0476 内容保护（X11876~X11900 · V 线）-------- */

export const SENSITIVE_WORDS = ['密码', '验证码', '卡号', '余额'] as const;

/** 内容保护：锁屏脱敏 + 敏感词识别 + 白名单。 */
export class ContentGuard {
  locked = false;
  allow = new Set<string>();
  clamped = 0;
  /** 敏感词识别：命中即受保护。 */
  static sensitive(text: string): boolean {
    return SENSITIVE_WORDS.some((w) => text.includes(w));
  }
  /** 数字脱敏：逐位替换为 ●。 */
  static redact(text: string): string {
    return text.replace(/\d/g, '●');
  }
  /** 锁屏预览：锁屏 + 敏感 → 只显示应用名。 */
  preview(app: string, text: string): string {
    if (!this.locked) return text;
    if (this.allow.has(app)) return text;
    if (ContentGuard.sensitive(text)) return `${app}：内容已隐藏`;
    return ContentGuard.redact(text);
  }
  allowApp(app: string): boolean {
    if (!app) {
      this.clamped++;
      return false;
    }
    this.allow.add(app);
    return true;
  }
  setLocked(v: boolean): void {
    this.locked = v;
  }
  /** 计数模式：锁屏只显示条数。 */
  static countMode(n: number): string {
    return `有 ${Math.max(0, Math.round(n))} 条新通知`;
  }
}

/* -------- 族0477 通知工程 2.0（X11901~X11925 · V 线）-------- */

/** 通知工程：队列环形 + 批量冲刷 + 降级链。 */
export class NotifyEng {
  private queue: string[] = [];
  capacity: number;
  clamped = 0;
  constructor(capacity = 20) {
    this.capacity = capacity > 0 ? capacity : 20;
  }
  push(id: string): boolean {
    if (!id) {
      this.clamped++;
      return false;
    }
    if (this.queue.length >= this.capacity) this.queue.shift();
    this.queue.push(id);
    return true;
  }
  /** 批量冲刷：取出全部并清空。 */
  flush(): string[] {
    const out = [...this.queue];
    this.queue = [];
    return out;
  }
  size(): number {
    return this.queue.length;
  }
  /** 降级链：full→lite→none。 */
  static degrade(level: string): string {
    return { full: 'lite', lite: 'none', none: 'none' }[level] ?? 'none';
  }
  /** 持久化快照：容量内才完整。 */
  snapshot(): string {
    return JSON.stringify({ n: this.queue.length, head: this.queue[0] ?? '' });
  }
}

/* -------- 族0478 节日音景（X11926~X11950 · V 线）-------- */

export const FESTIVE_TIERS = ['off', 'subtle', 'full'] as const;
export type FestiveTier = (typeof FESTIVE_TIERS)[number];

/** 节日音景：月份映射 + 档位钳制 + 一次性发现。 */
export class FestiveSound {
  tier: FestiveTier = 'subtle';
  discovered = new Set<number>();
  clamped = 0;
  setTier(t: string): FestiveTier {
    const ok = (FESTIVE_TIERS as readonly string[]).includes(t);
    this.tier = ok ? (t as FestiveTier) : 'off';
    if (!ok) this.clamped++;
    return this.tier;
  }
  /** 月份→节日音景名（1~12 循环）。 */
  static festivalOf(month: number): string {
    if (!Number.isInteger(month) || month < 1 || month > 12) return '';
    const names = ['元旦', '春节', '上巳', '劳动', '初夏', '端午', '小暑', '七夕', '中秋', '国庆', '立冬', '冬至'];
    return names[month - 1]!;
  }
  /** 一次性发现：重复发现不再计数。 */
  discover(month: number): boolean {
    if (!Number.isInteger(month) || month < 1 || month > 12) {
      this.clamped++;
      return false;
    }
    if (this.discovered.has(month)) return false;
    this.discovered.add(month);
    return true;
  }
  /** 播放裁决：off 静默。 */
  play(month: number): string {
    if (this.tier === 'off') return '';
    return FestiveSound.festivalOf(month);
  }
}

/* -------- 族0479 提醒 2.0（X11951~X11975 · V 线）-------- */

export const REMINDER_STATES = ['pending', 'done', 'overdue'] as const;
export type ReminderState = (typeof REMINDER_STATES)[number];

export interface Reminder {
  id: string;
  atMin: number;
  state: ReminderState;
}

/** 提醒 2.0：登记去重 + 偏移钳制 + 逾期标记。 */
export class ReminderX2 {
  items: Reminder[] = [];
  clamped = 0;
  /** 登记：同 id 拒绝；时刻 0~1440 钳制。 */
  add(id: string, atMin: number): boolean {
    if (!id || this.items.some((r) => r.id === id)) {
      this.clamped++;
      return false;
    }
    const at = Number.isFinite(atMin) ? Math.min(1440, Math.max(0, Math.round(atMin))) : 0;
    this.items.push({ id, atMin: at, state: 'pending' });
    return true;
  }
  /** 逾期标记：now 超过时刻且未完成。 */
  sweep(nowMin: number): number {
    if (!Number.isFinite(nowMin)) {
      this.clamped++;
      return 0;
    }
    let n = 0;
    for (const r of this.items) {
      if (r.state === 'pending' && nowMin > r.atMin) {
        r.state = 'overdue';
        n++;
      }
    }
    return n;
  }
  complete(id: string): boolean {
    const r = this.items.find((x) => x.id === id);
    if (!r || r.state === 'overdue') {
      this.clamped++;
      return false;
    }
    r.state = 'done';
    return true;
  }
  /** 叙事：逾期给下一步建议（禁裸报错）。 */
  static narrative(state: ReminderState): string {
    return { pending: '提醒待处理。', done: '已完成。', overdue: '已过期：可稍后重试或改期。' }[state];
  }
}

/* -------- 族0480 个性收藏 2.0（X11976~X12000 · V 线）-------- */

/** 个性收藏：声音收藏登记去重 + 排序调整 + 导出净身。 */
export class SoundFavX2 {
  favs: string[] = [];
  clamped = 0;
  /** 收藏：去重。 */
  add(name: string): boolean {
    if (!name || this.favs.includes(name)) {
      this.clamped++;
      return false;
    }
    this.favs.push(name);
    return true;
  }
  remove(name: string): boolean {
    const i = this.favs.indexOf(name);
    if (i < 0) {
      this.clamped++;
      return false;
    }
    this.favs.splice(i, 1);
    return true;
  }
  /** 置顶：存在的条目移到首位。 */
  pin(name: string): boolean {
    if (!this.favs.includes(name)) {
      this.clamped++;
      return false;
    }
    this.favs = [name, ...this.favs.filter((f) => f !== name)];
    return true;
  }
  /** 导出净身：去除路径分隔与控制字符。 */
  static exportSanitize(raw: string): string {
    return raw.replace(/[\\/]/g, '_').replace(/[\x00-\x1f]/g, '');
  }
  /** 迁移：旧格式 {items:[]} 携带。 */
  static migrate(raw: string): string[] {
    try {
      const o = JSON.parse(raw) as { items?: unknown };
      if (!Array.isArray(o.items)) return [];
      return o.items.filter((x): x is string => typeof x === 'string');
    } catch {
      return [];
    }
  }
  count(): number {
    return this.favs.length;
  }
}
