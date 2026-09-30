/**
 * Z-44 勿扰日程（纯函数，可单测）。
 *
 * - 时段规则：每天 start → end 自动勿扰；支持跨午夜（22:00 → 07:00）。
 * - 手动勿扰优先级高于日程（manualDnd=true 时无条件勿扰）。
 * - 提醒类豁免由调用方结合 dndReminderExempt 判断（见 effectiveNotifySound）。
 */

export interface DndSchedule {
  enabled: boolean;
  /** "HH:MM"（24h）。 */
  start: string;
  /** "HH:MM"（24h）。 */
  end: string;
}

/** 解析 "HH:MM" 为当日分钟数；非法返回 null。 */
export function parseHM(v: string): number | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(v.trim());
  if (!m) return null;
  const h = Number(m[1]);
  const min = Number(m[2]);
  if (h > 23 || min > 59) return null;
  return h * 60 + min;
}

/** 指定时刻（分钟数）是否处于勿扰时段（支持跨午夜）。 */
export function inScheduleWindow(nowMin: number, startMin: number, endMin: number): boolean {
  if (startMin === endMin) return false; // 零长度时段 = 无勿扰
  if (startMin < endMin) {
    // 同日时段：09:00 → 17:00
    return nowMin >= startMin && nowMin < endMin;
  }
  // 跨午夜时段：22:00 → 07:00（22:00..24:00 ∪ 00:00..07:00）
  return nowMin >= startMin || nowMin < endMin;
}

/**
 * 综合勿扰判定：手动（toggleDnd）优先于日程。
 * now 可注入（Date 或分钟数）以便单测；默认取当前系统时间。
 */
export function isDndActive(
  manualDnd: boolean,
  schedule: DndSchedule,
  now?: Date,
): boolean {
  if (manualDnd) return true;
  if (!schedule.enabled) return false;
  const start = parseHM(schedule.start);
  const end = parseHM(schedule.end);
  if (start == null || end == null) return false; // 非法配置不勿扰（诚实降级）
  const d = now ?? new Date();
  const nowMin = d.getHours() * 60 + d.getMinutes();
  return inScheduleWindow(nowMin, start, end);
}

/**
 * 勿扰期间声音判定：
 * - 闹钟（alarm）永远响（用户主动约定）；
 * - 提醒类（reminder）在 exempt=true 时豁免（Z-44/Z-49 联动）；
 * - 其余静默（通知照常入存档 Z-47）。
 */
export function effectiveNotifySound(
  kind: "alarm" | "reminder" | "other",
  dndActive: boolean,
  reminderExempt: boolean,
): boolean {
  if (!dndActive) return true;
  if (kind === "alarm") return true;
  if (kind === "reminder") return reminderExempt;
  return false;
}
