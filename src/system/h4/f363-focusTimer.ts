/**
 * F363 专注计时器（H 域 · AI-H4）：
 * 轻量番茄钟进专注模式（F349）：25/50 分钟两档默认（可自定 5-120 分钟），
 * 计时条驻任务栏（剩余时间徽标），结束温和提醒（一声+一条通知，不吓人）；
 * 中途放弃记录真实时长（小结不虚报）；每日专注累计入专注小结（F349 联动）。
 * 判据（主册 F363）：两档+自定范围；徽标实时性；结束提醒双通道；放弃真实性；每日累计对账。
 * 依赖锚点：F349 专注模式。
 * 存储键：variable:h4:f363（每日累计账）。
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

/** 两档默认（分钟）。 */
export const PRESET_MINUTES = [25, 50] as const;
/** 自定范围（分钟，闭区间）。 */
export const CUSTOM_MIN_MINUTES = 5;
export const CUSTOM_MAX_MINUTES = 120;

export interface FocusConfig {
  minutes: number;
}

/** 时长校验：两档之外的自定值必须在 5-120 内（判据范围）。 */
export function validateMinutes(m: number): { ok: boolean; reason: string } {
  if (!Number.isFinite(m)) return { ok: false, reason: "时长必须是数字" };
  if (PRESET_MINUTES.includes(m as 25 | 50)) return { ok: true, reason: "默认档" };
  if (m < CUSTOM_MIN_MINUTES || m > CUSTOM_MAX_MINUTES || !Number.isInteger(m)) {
    return { ok: false, reason: `自定时长须为 ${CUSTOM_MIN_MINUTES}-${CUSTOM_MAX_MINUTES} 的整数分钟` };
  }
  return { ok: true, reason: "自定档" };
}

export interface FocusRun {
  /** 当日 key（YYYY-MM-DD）。 */
  day: string;
  plannedMinutes: number;
  startedAt: number;
  /** null = 进行中。 */
  endedAt: number | null;
  /** completed / abandoned。 */
  outcome: "completed" | "abandoned" | "running";
}

/** 剩余秒数（徽标实时性：逐秒可读）。 */
export function remainingSec(run: FocusRun, now: number): number {
  const total = run.plannedMinutes * 60 * 1000;
  const used = Math.max(0, now - run.startedAt);
  return Math.max(0, Math.ceil((total - used) / 1000));
}

/** 徽标文本：mm:ss（实时徽标）。 */
export function badgeText(run: FocusRun, now: number): string {
  const s = remainingSec(run, now);
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

/** 到点判定：剩余归零即完成。 */
export function isDue(run: FocusRun, now: number): boolean {
  return remainingSec(run, now) === 0;
}

/** 结束提醒双通道（判据）：一声 + 一条通知；温和不吓人（音量档固定低）。 */
export function endReminder(run: FocusRun): { chime: boolean; chimeVolume: number; notification: string } {
  void run;
  return { chime: true, chimeVolume: 0.4, notification: "专注时段完成——休息一下吧" };
}

/** 放弃：记录真实时长（真实已用分钟，向下取整——小结不虚报）。 */
export function abandon(run: FocusRun, now: number): FocusRun {
  return { ...run, endedAt: now, outcome: "abandoned" };
}

/** 真实专注分钟数（完成=计划值；放弃=已用值，不足 1 分钟按 1 记——有始有终但不虚报）。 */
export function actualMinutes(run: FocusRun, now: number): number {
  if (run.outcome === "completed") return run.plannedMinutes;
  const used = Math.floor(((run.endedAt ?? now) - run.startedAt) / 60_000);
  return Math.max(1, Math.min(run.plannedMinutes, used));
}

/* ---------- 每日累计账（F349 联动） ---------- */

const KEY = h4Key("f363", "daily");

type DailyLedger = Record<string, { minutes: number; runs: number; abandoned: number }>;

function isLedger(v: unknown): v is DailyLedger {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

function load(store: KvStore): DailyLedger {
  return readJson<DailyLedger>(store, KEY, {}, isLedger);
}

function save(store: KvStore, l: DailyLedger): boolean {
  return writeJson(store, KEY, l);
}

/** 记账：完成/放弃都入账（放弃真实性=账上有迹可循）。 */
export function recordRun(run: FocusRun, now: number, store: KvStore = defaultStore()): { minutes: number; ok: boolean } {
  const minutes = actualMinutes(run, now);
  const ledger = load(store);
  const day = ledger[run.day] ?? { minutes: 0, runs: 0, abandoned: 0 };
  ledger[run.day] = {
    minutes: day.minutes + minutes,
    runs: day.runs + 1,
    abandoned: day.abandoned + (run.outcome === "abandoned" ? 1 : 0),
  };
  return { minutes, ok: save(store, ledger) };
}

/** 每日小结（专注小结数据源）。 */
export function dailySummary(day: string, store: KvStore = defaultStore()): { minutes: number; runs: number; abandoned: number } {
  return load(store)[day] ?? { minutes: 0, runs: 0, abandoned: 0 };
}

/** 每日累计对账：账面 = 逐笔之和（判据「每日累计对账」）。 */
export function auditDailyLedger(day: string, perRun: number[], store: KvStore = defaultStore()): { consistent: boolean; ledgerMinutes: number; sumOfRuns: number } {
  const s = dailySummary(day, store);
  return { consistent: s.minutes === perRun.reduce((a, b) => a + b, 0), ledgerMinutes: s.minutes, sumOfRuns: perRun.reduce((a, b) => a + b, 0) };
}

/* ================= v5 深化批次五：番茄连击 / 打断归类 / 周报 / 提醒降级 ================= */

/** 番茄连击：连续 4 个完成 → 建议长休息（经典番茄节奏，建议制不强制）。 */
export const POMODORO_CYCLE = 4;
export function pomodoroAdvice(completedStreak: number): { longBreak: boolean; advice: string } {
  return completedStreak > 0 && completedStreak % POMODORO_CYCLE === 0
    ? { longBreak: true, advice: `连续 ${POMODORO_CYCLE} 轮完成——建议长休息 15 分钟` }
    : { longBreak: false, advice: "继续下一轮，或按需休息" };
}

/** 打断归类（放弃真实性深化）：自打断（自己点放弃）vs 外部打断（系统事件打断）分开记账。 */
export type Interruption = "self" | "external";
export interface InterruptedRun extends FocusRun {
  interruption: Interruption;
}

export function classifyInterruption(run: FocusRun, bySystemEvent: boolean): InterruptedRun {
  return { ...run, interruption: run.outcome === "abandoned" ? (bySystemEvent ? "external" : "self") : "self" };
}

/** 周报（7 天聚合）：专注分钟/轮数/放弃率——趋势比单日更诚实。 */
export function weeklySummary(today: string, dailyLookback: (day: string) => { minutes: number; runs: number; abandoned: number }, days = 7): { minutes: number; runs: number; abandonRatePct: number } {
  const base = new Date(`${today}T00:00:00`);
  let minutes = 0;
  let runs = 0;
  let abandoned = 0;
  for (let i = 0; i < days; i++) {
    const d = new Date(base);
    d.setDate(base.getDate() - i);
    const key = d.toISOString().slice(0, 10);
    const s = dailyLookback(key);
    minutes += s.minutes;
    runs += s.runs;
    abandoned += s.abandoned;
  }
  return { minutes, runs, abandonRatePct: runs === 0 ? 0 : Math.round((abandoned / runs) * 100) };
}

/** 提醒通道降级：音效通道失败 → 通知单通道仍达（双通道判据的降级面——提醒永不静默丢失）。 */
export function reminderFallback(chimeOk: boolean): { channels: Array<"chime" | "notification">; honest: string } {
  return chimeOk
    ? { channels: ["chime", "notification"], honest: "双通道齐发（一声 + 一条通知）" }
    : { channels: ["notification"], honest: "音效通道不可用——通知通道独立送达（提醒不丢）" };
}
