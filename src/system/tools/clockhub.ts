/**
 * Z-22 时钟中心（Clock Hub）—— 纯逻辑层（零 React / 零 IPC，可单测）。
 *
 * 设计取舍（规格红线）：
 * - 只做「世界时钟 / 秒表 / 计时器 / 闹钟」四件事；不做日历视图（批次D 任务栏时钟已有）、
 *   不做日程/会议管理（Z-26 的领地）、不做时钟地图。
 * - 秒表与计时器采用「系统时钟锚定」：任何读数都是 now 与锚点（Date.now() 快照）的差值，
 *   绝不靠 setInterval 累加计数 —— 后台标签被浏览器节流到 1 次/分钟也不漂移，
 *   误差上限即系统时钟本身（<1s/小时），暂停/恢复只是挪锚点，不产生累积误差。
 * - 闹钟/世界城市选择持久化到 tools/clockhub.json；秒表与计时器是瞬时任务，
 *   刻意不跨会话恢复（挂起会话里"运行中的秒表"语义混乱，重开即清）。
 */

// ================= 闹钟 =================

export interface Alarm {
  id: string;
  /** 0-23 */
  hour: number;
  /** 0-59 */
  minute: number;
  /** 重复星期，0=周日（与 Date.getDay() 对齐）；空数组 = 单次（响完由 UI 自动停用） */
  days: number[];
  enabled: boolean;
  label: string;
}

export const ALARM_MAX = 20;
export const ALARM_LABEL_MAX = 40;

/** 单条闹钟校验/清洗：结构性非法（id/hour/minute 域外）整条丢弃；可修复字段（days 去重排序、label 截断）就地修复。 */
export function sanitizeAlarm(raw: unknown): Alarm | null {
  if (typeof raw !== "object" || raw === null) return null;
  const r = raw as Record<string, unknown>;
  if (typeof r.id !== "string" || !r.id) return null;
  const hour = r.hour;
  const minute = r.minute;
  if (typeof hour !== "number" || !Number.isInteger(hour) || hour < 0 || hour > 23) return null;
  if (typeof minute !== "number" || !Number.isInteger(minute) || minute < 0 || minute > 59) return null;
  const days = Array.isArray(r.days)
    ? [...new Set(r.days.filter((d): d is number => typeof d === "number" && Number.isInteger(d) && d >= 0 && d <= 6))].sort((a, b) => a - b)
    : [];
  const label = typeof r.label === "string" ? r.label.slice(0, ALARM_LABEL_MAX) : "";
  return { id: r.id, hour, minute, days, enabled: r.enabled === true, label };
}

/** 批量清洗（持久化数据读回 / 非法 JSON 兜底都走这里）。 */
export function sanitizeAlarms(raw: unknown): Alarm[] {
  if (!Array.isArray(raw)) return [];
  const out: Alarm[] = [];
  for (const r of raw) {
    const a = sanitizeAlarm(r);
    if (a) out.push(a);
    if (out.length >= ALARM_MAX) break;
  }
  return out;
}

/** 当前这一分钟是否应响（hh:mm 匹配 + 星期匹配；单次闹钟不看星期）。同一分钟内的去重由 UI 层用 minuteKey 负责。 */
export function alarmShouldFireNow(alarm: Alarm, now: Date | number = new Date()): boolean {
  if (!alarm.enabled) return false;
  const d = now instanceof Date ? now : new Date(now);
  if (d.getHours() !== alarm.hour || d.getMinutes() !== alarm.minute) return false;
  return alarm.days.length === 0 || alarm.days.includes(d.getDay());
}

/** 本地时区的「分钟键」（同一闹钟同一分钟只响一次的判据）。 */
export function minuteKey(now: Date | number = Date.now()): string {
  const d = now instanceof Date ? now : new Date(now);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** 下次响铃时间戳：days 非空 → 未来 7 天内第一个匹配星期的 hh:mm；days 空 → 今天/明天的 hh:mm。 */
export function nextAlarmAt(alarm: Alarm, now: Date | number = new Date()): number {
  const d = now instanceof Date ? now : new Date(now);
  const base = d.getTime();
  for (let add = 0; add <= 8; add++) {
    const cand = new Date(d.getFullYear(), d.getMonth(), d.getDate() + add, alarm.hour, alarm.minute, 0, 0);
    if (cand.getTime() <= base) continue; // 已过点（含恰好等于当前分钟）→ 看下一天
    if (alarm.days.length === 0 || alarm.days.includes(cand.getDay())) return cand.getTime();
  }
  return base; // 理论不可达：非空 days 7 天内必命中，空 days 2 天内必命中
}

// ================= 秒表（系统时钟锚定） =================

export interface StopwatchState {
  id: string;
  label: string;
  running: boolean;
  /** 最近一次恢复运行的锚点（Date.now() 快照）；暂停时为 null */
  anchor: number | null;
  /** 暂停前累计毫秒（不含当前运行段） */
  accMs: number;
  /** 计次点（各次累计毫秒） */
  laps: number[];
}

/** 新建即开跑（锚点 = now，此刻起零误差）。 */
export function createStopwatch(id: string, label: string, now: number = Date.now()): StopwatchState {
  return { id, label, running: true, anchor: now, accMs: 0, laps: [] };
}

/** 锚定读数：elapsed = accMs + (running ? now - anchor : 0)。纯函数，读多少次都不会累加。 */
export function stopwatchElapsed(sw: StopwatchState, now: number = Date.now()): number {
  if (!sw.running || sw.anchor === null) return sw.accMs;
  return sw.accMs + Math.max(0, now - sw.anchor);
}

/** 启动/暂停切换 = 挪锚点：暂停把运行段结转到 accMs；恢复把锚点重设为 now。 */
export function stopwatchToggle(sw: StopwatchState, now: number = Date.now()): StopwatchState {
  if (sw.running) return { ...sw, running: false, anchor: null, accMs: stopwatchElapsed(sw, now) };
  return { ...sw, running: true, anchor: now };
}

export function stopwatchReset(sw: StopwatchState): StopwatchState {
  return { ...sw, running: false, anchor: null, accMs: 0, laps: [] };
}

/** 计次：记录当前累计值。 */
export function stopwatchLap(sw: StopwatchState, now: number = Date.now()): { sw: StopwatchState; lapMs: number } {
  const lapMs = stopwatchElapsed(sw, now);
  return { sw: { ...sw, laps: [...sw.laps, lapMs] }, lapMs };
}

// ================= 计时器（系统时钟锚定，多实例并发） =================

export interface TimerState {
  id: string;
  label: string;
  durationMs: number;
  /** 到点时间戳锚点（启动/恢复时 endAt = now + 剩余）；未运行为 null */
  endAt: number | null;
  /** 暂停时剩余毫秒 */
  remainOnPause: number;
  running: boolean;
  done: boolean;
}

export function createTimer(id: string, label: string, durationMs: number, now: number = Date.now()): TimerState {
  return { id, label, durationMs, endAt: now + durationMs, remainOnPause: durationMs, running: true, done: false };
}

/** 剩余毫秒（到点后恒为 0）。同样是锚点差值，暂停期间不流失。 */
export function timerRemainMs(timer: TimerState, now: number = Date.now()): number {
  if (timer.done) return 0;
  if (!timer.running || timer.endAt === null) return Math.max(0, timer.remainOnPause);
  return Math.max(0, timer.endAt - now);
}

export function timerToggle(timer: TimerState, now: number = Date.now()): TimerState {
  if (timer.running) {
    return { ...timer, running: false, endAt: null, remainOnPause: timerRemainMs(timer, now) };
  }
  if (timer.done) return timer;
  return { ...timer, running: true, endAt: now + timer.remainOnPause };
}

export function timerReset(timer: TimerState, now: number = Date.now()): TimerState {
  return { ...timer, running: true, endAt: now + timer.durationMs, remainOnPause: timer.durationMs, done: false };
}

/** 到点判定：只在「运行中且未标记完成且已过锚点」时为真 —— UI tick 捕捉后标记 done，天然只报一次。 */
export function timerJustDone(timer: TimerState, now: number = Date.now()): boolean {
  return !timer.done && timer.running && timer.endAt !== null && now >= timer.endAt;
}

export function timerMarkDone(timer: TimerState): TimerState {
  return { ...timer, running: false, endAt: null, remainOnPause: 0, done: true };
}

// ================= 内置城市时区表（20 城，覆盖主要时区） =================

export interface CityZone {
  id: string;
  /** 中文名（界面主语言为中文） */
  city: string;
  /** IANA 时区名（Intl 直接消费，夏令时自动处理） */
  iana: string;
}

/** 20 个内置城市：横跨 UTC+12 ~ UTC-10 共 16 个不同偏移（冬/夏令时各有差异），满足主要时区覆盖。 */
export const CITY_ZONES: readonly CityZone[] = [
  { id: "beijing", city: "北京", iana: "Asia/Shanghai" },
  { id: "shanghai", city: "上海", iana: "Asia/Shanghai" },
  { id: "tokyo", city: "东京", iana: "Asia/Tokyo" },
  { id: "singapore", city: "新加坡", iana: "Asia/Singapore" },
  { id: "sydney", city: "悉尼", iana: "Australia/Sydney" },
  { id: "auckland", city: "奥克兰", iana: "Pacific/Auckland" },
  { id: "mumbai", city: "孟买", iana: "Asia/Kolkata" },
  { id: "dubai", city: "迪拜", iana: "Asia/Dubai" },
  { id: "moscow", city: "莫斯科", iana: "Europe/Moscow" },
  { id: "cairo", city: "开罗", iana: "Africa/Cairo" },
  { id: "johannesburg", city: "约翰内斯堡", iana: "Africa/Johannesburg" },
  { id: "paris", city: "巴黎", iana: "Europe/Paris" },
  { id: "london", city: "伦敦", iana: "Europe/London" },
  { id: "reykjavik", city: "雷克雅未克", iana: "Atlantic/Reykjavik" },
  { id: "saopaulo", city: "圣保罗", iana: "America/Sao_Paulo" },
  { id: "buenosaires", city: "布宜诺斯艾利斯", iana: "America/Argentina/Buenos_Aires" },
  { id: "newyork", city: "纽约", iana: "America/New_York" },
  { id: "chicago", city: "芝加哥", iana: "America/Chicago" },
  { id: "denver", city: "丹佛", iana: "America/Denver" },
  { id: "losangeles", city: "洛杉矶", iana: "America/Los_Angeles" },
];

export const CITY_MAX = 12;

export function cityOf(id: string): CityZone | undefined {
  return CITY_ZONES.find((c) => c.id === id);
}

/** 时区某时刻的 UTC 偏移（分钟，含半小时区如 +5:30）；非法时区返回 null。
 *  实现：把该时区的墙钟时间拼成 UTC 时间戳，与真实时间戳相减 —— 标准做法，无第三方依赖。 */
export function zoneOffsetMinutes(zone: string, at: Date = new Date()): number | null {
  try {
    const dtf = new Intl.DateTimeFormat("en-US", {
      timeZone: zone,
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    });
    const parts = dtf.formatToParts(at);
    const num = (t: string): number => Number(parts.find((p) => p.type === t)?.value);
    const y = num("year");
    const mo = num("month");
    const dd = num("day");
    const hh = num("hour");
    const mi = num("minute");
    const ss = num("second");
    if ([y, mo, dd, hh, mi, ss].some((v) => !Number.isFinite(v))) return null;
    const asUTC = Date.UTC(y, mo - 1, dd, hh % 24, mi, ss);
    return Math.round((asUTC - at.getTime()) / 60_000);
  } catch {
    return null; // 非法 IANA 名（Intl 抛 RangeError）→ 如实降级
  }
}

/** 偏移分钟 → "GMT+8" / "GMT+5:30" / "GMT-3" 显示文本。 */
export function formatOffset(minutes: number | null): string {
  if (minutes === null) return "GMT?";
  const sign = minutes >= 0 ? "+" : "-";
  const abs = Math.abs(minutes);
  const h = Math.floor(abs / 60);
  const m = abs % 60;
  return `GMT${sign}${h}${m > 0 ? `:${String(m).padStart(2, "0")}` : ""}`;
}

// ================= 持久化（tools/clockhub.json） =================

export interface ClockHubData {
  version: 1;
  /** 已选城市 id（CITY_ZONES 子集，最多 CITY_MAX 个） */
  cities: string[];
  alarms: Alarm[];
}

/** 世界时钟默认四城：本地（北京）+ 三大金融时区，开箱即用。 */
export function defaultClockHubData(): ClockHubData {
  return { version: 1, cities: ["beijing", "london", "newyork", "tokyo"], alarms: [] };
}

export function serializeData(d: ClockHubData): string {
  return JSON.stringify({ version: 1, cities: d.cities, alarms: d.alarms });
}

/** 城市列表清洗：只留表内合法 id，去重、限量。 */
export function sanitizeCities(raw: unknown): string[] {
  if (!Array.isArray(raw)) return [];
  const out: string[] = [];
  for (const r of raw) {
    if (typeof r !== "string") continue;
    if (!CITY_ZONES.some((c) => c.id === r)) continue;
    if (!out.includes(r)) out.push(r);
    if (out.length >= CITY_MAX) break;
  }
  return out;
}

/** 读取侧统一入口：JSON 损坏 / 字段缺失 / 全非法 → 逐级兜底到默认，永不抛。 */
export function parseData(json: string | null | undefined): ClockHubData {
  if (!json) return defaultClockHubData();
  try {
    const raw: unknown = JSON.parse(json);
    if (typeof raw !== "object" || raw === null) return defaultClockHubData();
    const obj = raw as { cities?: unknown; alarms?: unknown };
    const cities = sanitizeCities(obj.cities);
    const alarms = sanitizeAlarms(obj.alarms);
    // 城市全被清洗掉（例如城市表演进后 id 全失效）→ 保底默认四城，避免开屏空态
    return { version: 1, cities: cities.length > 0 ? cities : defaultClockHubData().cities, alarms };
  } catch {
    return defaultClockHubData();
  }
}

// ================= 显示格式化 =================

/** 秒表读数 → "h:mm:ss.cc"（不足 1 小时省略小时段）。 */
export function formatStopwatch(ms: number): string {
  const cs = Math.floor(ms / 10) % 100;
  const s = Math.floor(ms / 1000) % 60;
  const m = Math.floor(ms / 60_000) % 60;
  const h = Math.floor(ms / 3_600_000);
  const p = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${p(m)}:${p(s)}.${p(cs)}` : `${p(m)}:${p(s)}.${p(cs)}`;
}

/** 倒计时读数 → "h:mm:ss"（向上取整秒，到点前最后一秒仍显示 00:01）。 */
export function formatCountdown(ms: number): string {
  const total = Math.ceil(ms / 1000);
  const s = total % 60;
  const m = Math.floor(total / 60) % 60;
  const h = Math.floor(total / 3600);
  const p = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${p(m)}:${p(s)}` : `${p(m)}:${p(s)}`;
}

/** 时分 → "08:30"。 */
export function formatHM(hour: number, minute: number): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(hour)}:${p(minute)}`;
}

/** 简易实例 id（会话内唯一即可，不持久化）。 */
export function clockUid(prefix: string): string {
  return `${prefix}-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 7)}`;
}
