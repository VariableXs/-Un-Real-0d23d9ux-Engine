/**
 * AI-19 无障碍与本地化组 — 区域格式跟随（M-78）。
 *
 * 日期/时间/数字/容量显示跟随系统区域设置（12/24h、日期序、
 * 千分位、KB/MB 口径），而非硬编码：
 * - Intl API 按系统 locale 渲染（navigator.language / resolvedOptions）；
 * - 容量单位二进制口径（KiB/MiB，1024 进制）与十进制（KB/MB，1000 进制）
 *   设置项（auto = 系统/Windows 口径，即二进制）；
 * - 全环境 30 处日期容量显示点走查统一由设置页开关逐点接线
 *   （默认关闭 = 等于现状，承「新行为默认关闭」纪律）。
 *
 * 红线：Intl 解析失败一律回退 en-US 并如实保留原始数值（绝不编造）。
 */

export type ByteUnitMode = "auto" | "binary" | "decimal";

/** 系统区域 locale（环境内可被 UI 覆盖语言不影响此口径；M-78 只认系统）。 */
export function systemLocale(): string {
  try {
    if (typeof navigator !== "undefined" && navigator.language) return navigator.language;
  } catch {
    /* 非浏览器环境 */
  }
  return "en-US";
}

/** 解析系统小时制（true = 12h）；解析失败回退 24h。 */
export function systemHour12(locale: string = systemLocale()): boolean {
  try {
    const opts = new Intl.DateTimeFormat(locale, { hour: "numeric" }).resolvedOptions();
    return opts.hour12 === true;
  } catch {
    return false;
  }
}

/** 区域日期（如 zh-CN 2026/9/8 · en-US 9/8/2026 · de-DE 8.9.2026）。 */
export function formatDateLocale(ts: number | Date, locale: string = systemLocale()): string {
  const d = ts instanceof Date ? ts : new Date(ts);
  try {
    return new Intl.DateTimeFormat(locale, { dateStyle: "medium" }).format(d);
  } catch {
    return d.toISOString().slice(0, 10);
  }
}

/** 区域时间（12/24h 跟随系统；秒不显示，与任务栏时钟口径一致）。 */
export function formatTimeLocale(
  ts: number | Date,
  locale: string = systemLocale(),
  forceHour12?: boolean,
): string {
  const d = ts instanceof Date ? ts : new Date(ts);
  try {
    return new Intl.DateTimeFormat(locale, {
      hour: "2-digit",
      minute: "2-digit",
      ...(forceHour12 !== undefined ? { hour12: forceHour12 } : {}),
    }).format(d);
  } catch {
    const pad = (n: number) => String(n).padStart(2, "0");
    return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
  }
}

/** 区域日期时间（完整显示点）。 */
export function formatDateTimeLocale(ts: number | Date, locale: string = systemLocale()): string {
  return `${formatDateLocale(ts, locale)} ${formatTimeLocale(ts, locale)}`;
}

/** 区域数字（千分位跟随系统：en 1,234.5 / de 1.234,5）。 */
export function formatNumberLocale(n: number, locale: string = systemLocale()): string {
  try {
    return new Intl.NumberFormat(locale).format(n);
  } catch {
    return String(n);
  }
}

const BINARY_UNITS = ["B", "KiB", "MiB", "GiB", "TiB"];
const DECIMAL_UNITS = ["B", "KB", "MB", "GB", "TB"];

/**
 * 区域容量（M-78 口径 + M-89 单位数字规范联动：保留 1 位小数，
 * GB 档 2 位与既有 formatBytes 一致）。
 * auto = 系统（Windows）口径 = 二进制 KiB/MiB。
 */
export function formatBytesLocale(n: number, mode: ByteUnitMode = "auto", locale: string = systemLocale()): string {
  const base = mode === "decimal" ? 1000 : 1024;
  const units = mode === "decimal" ? DECIMAL_UNITS : BINARY_UNITS;
  if (!Number.isFinite(n) || n < 0) return "0 B";
  if (n < base) return `${formatNumberLocale(Math.round(n), locale)} B`;
  let v = n;
  let i = 0;
  while (v >= base && i < units.length - 1) {
    v /= base;
    i++;
  }
  const digits = v >= 100 ? 1 : v >= 10 ? 1 : 2;
  return `${formatNumberLocale(Number(v.toFixed(digits)), locale)} ${units[i]}`;
}

/** 相对时间（「3 分钟前」/「in 3 min」；容量页速度时间统一口径）。 */
export function formatRelativeTime(ts: number, locale: string = systemLocale(), now = Date.now()): string {
  const diffMs = ts - now;
  const abs = Math.abs(diffMs);
  const rtf = (() => {
    try {
      return new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
    } catch {
      return null;
    }
  })();
  if (rtf) {
    const units: [Intl.RelativeTimeFormatUnit, number][] = [
      ["year", 31536000000],
      ["month", 2592000000],
      ["day", 86400000],
      ["hour", 3600000],
      ["minute", 60000],
      ["second", 1000],
    ];
    for (const [unit, ms] of units) {
      if (abs >= ms || unit === "second") {
        return rtf.format(Math.round(diffMs / ms), unit);
      }
    }
  }
  return formatDateTimeLocale(ts, locale);
}
