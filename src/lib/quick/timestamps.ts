/**
 * AI-07 · V-49 时间戳速插（命令面板/运行框输入 now/ts → 五种格式结果卡）：
 * ISO 8601 / Unix 秒 / Unix 毫秒 / 人类可读 / 相对描述（含年内天序）。
 * 全部本地计算（时区跟随系统）；不做时区转换（Z-26 领地）、不做反解。
 */

export interface TimestampCard {
  iso: string;
  unixSeconds: string;
  unixMillis: string;
  human: string;
  /** 相对描述：「今天是 2026 年第 N 天」。 */
  relative: string;
  dayOfYear: number;
}

/** 人类可读格式（本地时区）：2026-09-08 18:30:05。 */
function humanReadable(d: Date): string {
  const p = (n: number, w = 2) => String(n).padStart(w, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

/** 年内天序（1 起）。 */
export function dayOfYear(d: Date): number {
  const start = new Date(d.getFullYear(), 0, 1);
  return Math.floor((d.getTime() - start.getTime()) / 86400000) + 1;
}

/** 生成当前时间五格式结果卡（now 注入便于测试跨午夜/跨时区边界）。 */
export function timestampCard(now: Date = new Date()): TimestampCard {
  const t = now.getTime();
  const y = now.getFullYear();
  return {
    iso: new Date(t - now.getTimezoneOffset() * 60000).toISOString().slice(0, 19),
    unixSeconds: String(Math.floor(t / 1000)),
    unixMillis: String(t),
    human: humanReadable(now),
    relative: `今天是 ${y} 年第 ${dayOfYear(now)} 天`,
    dayOfYear: dayOfYear(now),
  };
}

/** 触发词判定：now / ts / 时间 / 时间戳（面板输入 → 出时间戳卡）。 */
export function isTimestampQuery(input: string): boolean {
  const s = input.trim().toLowerCase();
  return s === "now" || s === "ts" || s === "时间" || s === "时间戳";
}
