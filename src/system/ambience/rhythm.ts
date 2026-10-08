/**
 * AI-18 U-54 节律助手 — 纯逻辑状态机（久坐/用眼/喝水/站立）。
 *
 * 提醒礼仪（全景书口径）：
 * - 全屏专注（焦点舱）自动顺延不打断；
 * - 深夜时段（23..6 点）自动静默只计数；
 * - 同类提醒 10 分钟内不重复；
 * - 时钟回拨/跨时区不错乱 —— 全部用单调时钟（performance.now() 口径）。
 */

import type { AmbienceSettings, RhythmKind, RhythmNotify } from "./schema";

export const RHYTHM_KINDS: readonly RhythmKind[] = ["sitting", "eye", "drink", "stretch"];

/** 同类提醒去重窗口（毫秒）。 */
export const RHYTHM_DEDUP_MS = 10 * 60 * 1000;
/** 深夜静默时段（本地小时；含 23 点与 0..5 点）。 */
export function isQuietHour(hour: number): boolean {
  const h = ((Math.floor(hour) % 24) + 24) % 24;
  return h >= 23 || h < 6;
}

export interface RhythmDue {
  kind: RhythmKind;
  /** 实际采用的提醒方式（深夜降级为 count；焦点舱内不产生 due）。 */
  notify: RhythmNotify;
}

export interface RhythmProgress {
  /** 单调时钟口径：上次重置点（ms）。 */
  startMono: number;
  /** 上次提醒发出点（ms；0 = 从未）。 */
  lastNotified: number;
  /** 今日累计完成次数（跨日清零由调用方负责）。 */
  count: number;
}

export type RhythmStateMap = Partial<Record<RhythmKind, RhythmProgress>>;

export function ensureProgress(kind: RhythmKind, nowMono: number, prev: RhythmStateMap): RhythmProgress {
  return prev[kind] ?? { startMono: nowMono, lastNotified: 0, count: 0 };
}

/**
 * 计算到期的节律提醒（纯函数；不修改入参）。
 * @param nowMono 单调时钟毫秒（performance.now()）
 * @param hour 本地小时（用于深夜静默判定）
 * @param focusCabinOpen 焦点舱开启中 → 全部顺延
 */
export function computeDueRhythms(
  settings: AmbienceSettings,
  nowMono: number,
  hour: number,
  focusCabinOpen: boolean,
  state: RhythmStateMap,
): { due: RhythmDue[]; next: RhythmStateMap } {
  const next: RhythmStateMap = { ...state };
  const due: RhythmDue[] = [];
  if (focusCabinOpen) return { due, next };
  const quiet = isQuietHour(hour);
  for (const kind of RHYTHM_KINDS) {
    const item = settings.rhythm[kind];
    if (!item.enabled) continue;
    const p = ensureProgress(kind, nowMono, state);
    const elapsed = nowMono - p.startMono;
    const intervalMs = item.intervalMin * 60_000;
    if (elapsed < intervalMs) continue;
    // 去重窗口：同类 10 分钟内不重复
    if (p.lastNotified > 0 && nowMono - p.lastNotified < RHYTHM_DEDUP_MS) continue;
    const notify: RhythmNotify = quiet ? "count" : item.notify;
    next[kind] = { startMono: nowMono, lastNotified: nowMono, count: p.count + 1 };
    due.push({ kind, notify });
  }
  return { due, next };
}

/** 手动完成一次（勾掉提醒）—— 计数入账并重置计时。 */
export function completeRhythm(kind: RhythmKind, nowMono: number, state: RhythmStateMap): RhythmStateMap {
  const p = ensureProgress(kind, nowMono, state);
  return { ...state, [kind]: { startMono: nowMono, lastNotified: p.lastNotified, count: p.count + 1 } };
}

/** 今日四环进度（0..1，用于 RhythmCenter 卡片）。 */
export function rhythmRings(settings: AmbienceSettings, nowMono: number, state: RhythmStateMap): Record<RhythmKind, number> {
  const out = {} as Record<RhythmKind, number>;
  for (const kind of RHYTHM_KINDS) {
    const item = settings.rhythm[kind];
    if (!item.enabled) { out[kind] = 0; continue; }
    const p = ensureProgress(kind, nowMono, state);
    const elapsed = Math.max(0, nowMono - p.startMono);
    out[kind] = Math.min(1, elapsed / (item.intervalMin * 60_000));
  }
  return out;
}
