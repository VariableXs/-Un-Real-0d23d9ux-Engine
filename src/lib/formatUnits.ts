/**
 * AI-20 质量门禁与收官组 — M-89 单位与数字规范（Units & Numbers Standard）。
 *
 * 全环境速度/容量/时间显示统一口径（终结「1.5GB / 1536MB / 1.53G」混排）：
 *
 * ── 容量（formatCapacity）──
 * · 默认（mode 缺省）= 现状口径：1024 进制 + KB/MB/GB 标签，KB/MB 保留
 *   1 位小数、GB 保留 2 位小数（与 format.ts formatBytes 完全一致，零视觉抖动）；
 * · mode = auto/binary/decimal 时走 M-78 口径（formatBytesLocale，
 *   binary=KiB/MiB 标签、decimal=KB/MB 标签；auto=系统口径即二进制）。
 *
 * ── 速度（formatSpeed）──
 * · 一律十进制 SI（速率是比值，非容量）：1024 B/s → "1.0 KB/s"；
 * · 保留 1 位小数（规格口径「XX MB/s」），单位 B/s…GB/s。
 *
 * ── 时间（formatSmartTime + TIME_DISPLAY_RULE）──
 * · 切换规则（文档化，单一事实源）：
 *   ① 过去 7 天内且未来 1 天内 → 相对时间（「3 分钟前」「2 小时后」）；
 *   ② 其余（≥7 天前 / ≥1 天后 / 跨年） → 绝对时间（日期 + HH:mm）；
 *   ③ 绝对时间今年省略年份，往年带年份。
 * · 相对时间委托 Intl.RelativeTimeFormat（locale 感知，M-78 联动）。
 */

import { formatBytes } from "./format";
import { formatBytesLocale, formatRelativeTime, formatDateTimeLocale, type ByteUnitMode } from "./localeFormat";

export type { ByteUnitMode };

/** 容量：规范入口。mode 缺省 = 现状口径；显式 mode = M-78 口径。 */
export function formatCapacity(n: number, mode?: ByteUnitMode, locale?: string): string {
  if (mode === undefined) return formatBytes(n);
  return locale ? formatBytesLocale(n, mode, locale) : formatBytesLocale(n, mode);
}

const SPEED_UNITS = ["B/s", "KB/s", "MB/s", "GB/s", "TB/s"];

/** 速度：十进制 SI、1 位小数（「12.3 MB/s」；负数/非法输入如实回退 "0 B/s"）。 */
export function formatSpeed(bytesPerSec: number): string {
  if (!Number.isFinite(bytesPerSec) || bytesPerSec < 0) return "0 B/s";
  let v = bytesPerSec;
  let i = 0;
  while (v >= 1000 && i < SPEED_UNITS.length - 1) {
    v /= 1000;
    i++;
  }
  if (i === 0) return `${Math.round(v)} B/s`;
  return `${v.toFixed(1)} ${SPEED_UNITS[i]}`;
}

/** 时间显示切换规则（M-89 文档化单一事实源）。 */
export const TIME_DISPLAY_RULE = {
  /** 过去相对窗口：7 天内用相对时间。 */
  relativePastMs: 7 * 24 * 3600 * 1000,
  /** 未来相对窗口：1 天内用相对时间。 */
  relativeFutureMs: 24 * 3600 * 1000,
} as const;

/** 相对/绝对切换判定（纯函数，供单测与显示点共用）。 */
export function shouldUseRelative(ts: number, now = Date.now()): boolean {
  const diff = ts - now;
  if (diff >= 0) return diff <= TIME_DISPLAY_RULE.relativeFutureMs;
  return -diff <= TIME_DISPLAY_RULE.relativePastMs;
}

/** 智能时间：按切换规则渲染相对或绝对时间。 */
export function formatSmartTime(ts: number, locale?: string, now = Date.now()): string {
  if (shouldUseRelative(ts, now)) {
    return locale ? formatRelativeTime(ts, locale, now) : formatRelativeTime(ts, undefined, now);
  }
  // 绝对时间：今年省略年份；往年带年份
  const d = new Date(ts);
  const sameYear = d.getFullYear() === new Date(now).getFullYear();
  const dateStr = sameYear
    ? `${d.getMonth() + 1}/${d.getDate()}`
    : `${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()}`;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${dateStr} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** 时间切换规则文档化说明（供设置页/毕业页引用，不另立口径）。 */
export const TIME_RULE_DOC = "过去 7 天内与未来 1 天内显示相对时间（如「3 分钟前」），其余显示绝对时间（今年省略年份）。";

export { formatDateTimeLocale };
