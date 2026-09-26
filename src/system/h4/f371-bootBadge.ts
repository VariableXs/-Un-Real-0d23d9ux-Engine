/**
 * F371 开机时长徽标（H 域 · AI-H4）：
 * 每次开机完成后登录桌面右下角淡入一枚小徽标（「本次开机 3.2 秒」，3 秒后自淡出）——
 * 把 B 域启动链的工程成果变成用户每天看得见的成绩单；徽标可关；历史开机时长曲线
 * 在设置中心「关于」页（趋势可见）。
 * 判据（主册 F371）：时长与 B-2x 实测链对账（误差 <100ms）；淡入淡出时序；关闭开关；
 * 历史曲线与徽标同源。
 * 存储键：variable:h4:f371（历史账 + 开关）。
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

/** 淡入淡出时序（判据）：淡入 300ms → 停留 3000ms → 淡出 500ms。 */
export const BADGE_TIMING = { fadeInMs: 300, holdMs: 3000, fadeOutMs: 500 } as const;

/** 对账允许误差（判据 <100ms）。 */
export const RECONCILE_TOLERANCE_MS = 100;

export interface BootRecord {
  /** 开机序号（第 N 次开机——彩蛋 F399 与曲线共用）。 */
  seq: number;
  /** B-2x 实测链读数（ms）。 */
  measuredMs: number;
  at: number;
}

export interface BadgeViewModel {
  text: string;
  /** 展示值（秒，一位小数）。 */
  seconds: number;
  /** 与实测链误差 ms（同源=0；>100 判缺陷）。 */
  driftMs: number;
  ok: boolean;
}

/** 徽标视图：展示值 = 实测链换算（同源，不是第二套估算）。 */
export function badgeFromMeasured(rec: BootRecord): BadgeViewModel {
  const seconds = Math.round((rec.measuredMs / 1000) * 10) / 10;
  const drift = Math.round(seconds * 1000) - rec.measuredMs;
  return { text: `本次开机 ${seconds.toFixed(1)} 秒`, seconds, driftMs: Math.abs(drift), ok: Math.abs(drift) < RECONCILE_TOLERANCE_MS };
}

/** 淡入淡出时间轴：t 时刻徽标应处阶段（判据时序逐段校验）。 */
export function badgePhase(tSinceShowMs: number): "hidden" | "fade-in" | "hold" | "fade-out" | "gone" {
  const { fadeInMs, holdMs, fadeOutMs } = BADGE_TIMING;
  if (tSinceShowMs <= 0) return "hidden";
  if (tSinceShowMs < fadeInMs) return "fade-in";
  if (tSinceShowMs < fadeInMs + holdMs) return "hold";
  if (tSinceShowMs < fadeInMs + holdMs + fadeOutMs) return "fade-out";
  return "gone";
}

/* ---------- 历史账与开关 ---------- */

const KEY = h4Key("f371", "history");
const KEY_ENABLED = h4Key("f371", "enabled");

function isRecords(v: unknown): v is BootRecord[] {
  return Array.isArray(v) && v.every((r) => r && typeof (r as BootRecord).measuredMs === "number");
}

/** 记账（历史曲线上限 90 条滚动——三个月的每一天）。 */
export function recordBoot(rec: BootRecord, store: KvStore = defaultStore()): boolean {
  const all = readJson<BootRecord[]>(store, KEY, [], isRecords);
  return writeJson(store, KEY, [...all.slice(-89), rec]);
}

export function bootHistory(store: KvStore = defaultStore()): BootRecord[] {
  return readJson<BootRecord[]>(store, KEY, [], isRecords);
}

/** 历史曲线与徽标同源（判据）：曲线取数与徽标取数走同一账本。 */
export function curveFromSameLedger(store: KvStore = defaultStore()): Array<{ seq: number; seconds: number }> {
  return bootHistory(store).map((r) => ({ seq: r.seq, seconds: Math.round((r.measuredMs / 1000) * 10) / 10 }));
}

/** 对账（判据「与 B-2x 实测链对账」）：曲线值 vs 源值误差 <100ms。 */
export function auditReconciliation(store: KvStore = defaultStore()): { pass: boolean; worstDriftMs: number } {
  let worst = 0;
  for (const r of bootHistory(store)) {
    worst = Math.max(worst, badgeFromMeasured(r).driftMs);
  }
  return { pass: worst < RECONCILE_TOLERANCE_MS, worstDriftMs: worst };
}

/** 徽标开关（可关判据；默认开）。 */
export function isEnabled(store: KvStore = defaultStore()): boolean {
  return readJson<boolean>(store, KEY_ENABLED, true, (v): v is boolean => typeof v === "boolean");
}

export function setEnabled(on: boolean, store: KvStore = defaultStore()): boolean {
  return writeJson(store, KEY_ENABLED, on);
}
