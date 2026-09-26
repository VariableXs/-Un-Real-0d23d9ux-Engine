/**
 * F549 时钟悬停完整日期（AI-U3 · clockcal）——任务栏时钟悬停 Tooltip。
 *
 * 判据唯一源（主册摘文）：「完整日期+星期+农历；Tooltip 延迟沿用 500ms
 * 全局；农历数据离线内置；Tooltip 与 F078 分工（悬停轻、点击重）」。
 *
 * 【数据源一处一事实】农历位表与春节锚点直接移植自内核
 * kernel/varix/src/ustar3/clockcal.rs（AI-U3 v1 已验证数据：内部一致性
 * 不变量 year_walk_lands_on_next_cny + 五年抽检全绿）——前后端共用同一份
 * 数据源，杜绝两套万年历数据漂移。
 *
 * 【偏差登记（与内核 v1 同笔）】主册示例句「2026-09-25 … 八月初五」与
 * 真实万年历不符——该日实为八月十五（中秋节）。以事实为准。
 *
 * 【Tooltip 与 F078 分工】本模块只产 Tooltip 数据（三要素一行、500ms、
 * 不可交互）；月历网格/可点交互归 F078（AI-D1 领地），不在此重复。
 */

import { u3Store } from "./u3store";

/** Tooltip 悬停延迟 500ms（F205 全局延迟一致）。 */
export const TOOLTIP_DELAY_MS = 500;
/** 农历数据覆盖（农历年）：2025-2030。 */
export const LUNAR_DATA_FIRST_YEAR = 2025;
export const LUNAR_DATA_LAST_YEAR = 2030;

/** 春节（正月初一）锚点表（通行万年历）：2025-01-29 … 2031-01-23。 */
export const CNY_ANCHORS: Array<readonly [number, number, number]> = [
  [2025, 1, 29],
  [2026, 2, 17],
  [2027, 2, 6],
  [2028, 1, 26],
  [2029, 2, 13],
  [2030, 2, 3],
  [2031, 1, 23],
];

/** 农历月序压缩位表（2025-2030，与通行 1900-2100 万年历数据表同源）。
 *  编码：bit0-3 = 闰月号（0=无闰）；bit(16-m)=1 为第 m 月 30 天大月（m=1..12）；
 *  bit16 = 闰月为大月。 */
export const LUNAR_INFO: number[] = [
  0x0a6e6, // 2025 乙巳：闰六月
  0x0a4e0, // 2026 丙午：无闰
  0x0d260, // 2027 丁未：无闰
  0x0ea65, // 2028 戊申：闰五月
  0x0d530, // 2029 己酉：无闰
  0x05aa0, // 2030 庚戌：无闰
];

export const LUNAR_MONTH_NAMES = ["正", "二", "三", "四", "五", "六", "七", "八", "九", "十", "冬", "腊"];
export const LUNAR_DAY_NAMES = [
  "初一", "初二", "初三", "初四", "初五", "初六", "初七", "初八", "初九", "初十",
  "十一", "十二", "十三", "十四", "十五", "十六", "十七", "十八", "十九", "二十",
  "廿一", "廿二", "廿三", "廿四", "廿五", "廿六", "廿七", "廿八", "廿九", "三十",
];
export const WEEKDAY_NAMES = ["一", "二", "三", "四", "五", "六", "日"];

/* ------------------------------- 日历算术 ------------------------------- */

/** 公历 (y,m,d) → civil 天数序号（纯整数，与内核同式）。 */
export function daysFromCivil(y: number, m: number, d: number): number {
  const yy = m <= 2 ? y - 1 : y;
  const era = Math.floor(yy / 400);
  const yoe = yy - era * 400;
  const mp = (m + 9) % 12;
  const doy = Math.floor((153 * mp + 2) / 5) + d - 1;
  const doe = yoe * 365 + Math.floor(yoe / 4) - Math.floor(yoe / 100) + doy;
  return era * 146097 + doe - 719468 + 305;
}

/** 星期几（1=周一 … 7=周日）。锚定事实：2026-09-25 为星期五。 */
export function weekday(y: number, m: number, d: number): number {
  const diff = daysFromCivil(y, m, d) - daysFromCivil(2026, 9, 25);
  return ((diff + 4) % 7 + 7) % 7 + 1;
}

/* ------------------------------- 农历换算核心 ------------------------------- */

export interface LunarDate {
  year: number;    // 农历年（干支年，以正月初一分界）
  month: number;   // 1-12（闰月以 leap=true 表达）
  day: number;     // 1-30
  leap: boolean;   // 是否闰月
}

function leapMonthOf(info: number): number {
  return info & 0xf;
}
function monthDays(info: number, month: number): number {
  return (info & (1 << (16 - month))) !== 0 ? 30 : 29;
}
function leapMonthDays(info: number): number {
  return (info & 0x10000) !== 0 ? 30 : 29;
}
/** 农历年总天数（含闰月）。 */
export function lunarYearDays(info: number): number {
  let total = 0;
  for (let m = 1; m <= 12; m++) total += monthDays(info, m);
  if (leapMonthOf(info) > 0) total += leapMonthDays(info);
  return total;
}

/** 内部一致性不变量（与内核同名判据）：位表逐月步进必须落在下一年春节锚点。 */
export function yearWalkLandsOnNextCny(lunarYear: number): boolean {
  const i = lunarYear - LUNAR_DATA_FIRST_YEAR;
  if (i < 0 || i + 1 >= CNY_ANCHORS.length || i + 1 > LUNAR_INFO.length) return false;
  const [sy, sm, sd] = CNY_ANCHORS[i];
  const [ny, nm, nd] = CNY_ANCHORS[i + 1];
  return daysFromCivil(sy, sm, sd) + lunarYearDays(LUNAR_INFO[i]) === daysFromCivil(ny, nm, nd);
}

/**
 * 公历 → 农历（离线，范围外诚实返回 null——不硬编）。
 * 逐锚点向前步进：确定农历年 → 逐月累减确定月 → 余日即日。
 */
export function solarToLunar(y: number, m: number, d: number): LunarDate | null {
  const days = daysFromCivil(y, m, d);
  for (let i = 0; i + 1 < CNY_ANCHORS.length && i < LUNAR_INFO.length; i++) {
    const [sy, sm, sd] = CNY_ANCHORS[i];
    const [ny, nm, nd] = CNY_ANCHORS[i + 1];
    const start = daysFromCivil(sy, sm, sd);
    const next = daysFromCivil(ny, nm, nd);
    if (days < start || days >= next) continue;
    let rest = days - start;
    const info = LUNAR_INFO[i];
    const leap = leapMonthOf(info);
    for (let mo = 1; mo <= 12; mo++) {
      if (leap > 0 && mo === leap + 1) {
        const ld = leapMonthDays(info);
        if (rest < ld) return { year: sy, month: leap, day: rest + 1, leap: true };
        rest -= ld;
      }
      const md = monthDays(info, mo);
      if (rest < md) return { year: sy, month: mo, day: rest + 1, leap: false };
      rest -= md;
    }
    return null; // 位表完备时不可达（yearWalk 不变量保证）
  }
  return null;
}

/** 农历月名（含「闰」前缀）。 */
export function lunarMonthName(l: LunarDate): string {
  const base = LUNAR_MONTH_NAMES[l.month - 1] ?? "?";
  return (l.leap ? "闰" : "") + base + "月";
}
export function lunarDayName(l: LunarDate): string {
  return LUNAR_DAY_NAMES[l.day - 1] ?? "?";
}

/** 天干地支（农历年 → 干支名；4=甲子基准年）。 */
const GAN = ["甲", "乙", "丙", "丁", "戊", "己", "庚", "辛", "壬", "癸"];
const ZHI = ["子", "丑", "寅", "卯", "辰", "巳", "午", "未", "申", "酉", "戌", "亥"];
export function ganzhi(lunarYear: number): string {
  const g = GAN[(lunarYear - 4) % 10];
  const z = ZHI[(lunarYear - 4) % 12];
  return `${g}${z}`;
}

/** Tooltip 三要素一行（判据：完整日期+星期+农历）。 */
export function hoverDateLine(y: number, m: number, d: number, showLunar = true): string {
  const w = WEEKDAY_NAMES[weekday(y, m, d) - 1];
  const greg = `${y} 年 ${m} 月 ${d} 日 星期${w}`;
  if (!showLunar) return greg;
  const l = solarToLunar(y, m, d);
  if (!l) return greg; // 范围外诚实降级（无假农历）
  return `${greg} · ${ganzhi(l.year)}年${lunarMonthName(l)}${lunarDayName(l)}`;
}

/* ------------------------------- 运行时读取 ------------------------------- */

export function clockHoverConfig() {
  const s = u3Store.get("clockHover");
  return { showLunar: (s.showLunar as boolean) ?? true, delayMs: (s.delayMs as number) ?? TOOLTIP_DELAY_MS };
}

/* ------------------------------- 自检 ------------------------------- */

/** F549 判据自检（前端面；五年抽检锚点与内核 v1 同源）。 */
export function clockcalSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  // 位表-锚点内部一致性不变量（2025-2030 六年全查）
  let walk = true;
  for (let y = LUNAR_DATA_FIRST_YEAR; y <= LUNAR_DATA_LAST_YEAR; y++) walk = walk && yearWalkLandsOnNextCny(y);
  checks.push({ name: "F549 位表锚点一致", pass: walk });
  // 五年抽检：春节必为正月初一
  const cnyCheck = CNY_ANCHORS.slice(1, 6).map(([y, m, d]) => {
    const l = solarToLunar(y, m, d);
    return !!l && l.month === 1 && l.day === 1 && !l.leap;
  });
  checks.push({ name: "F549 春节=正月初一(2026-2030)", pass: cnyCheck.every(Boolean) });
  // 主册偏差事实锚：2026-09-25 = 八月十五（中秋）
  const mid = solarToLunar(2026, 9, 25);
  checks.push({ name: "F549 中秋事实锚", pass: !!mid && mid.month === 8 && mid.day === 15 && !mid.leap });
  // 闰月锚：2025 闰六月 / 2028 闰五月
  const l25 = solarToLunar(2025, 8, 1); // 2025-08-01 = 闰六月初八
  const l28 = solarToLunar(2028, 6, 23); // 2028-06-23 = 闰五月初一
  checks.push({ name: "F549 闰月锚(25闰六/28闰五)", pass: !!l25 && l25.leap && l25.month === 6 && !!l28 && l28.leap && l28.month === 5 });
  // 跨年边界：2026-01-01 属乙巳年冬月十三（与内核 v1 同源事实）
  const edge = solarToLunar(2026, 1, 1);
  checks.push({ name: "F549 跨年边界", pass: !!edge && edge.year === 2025 && !edge.leap && edge.month === 11 && edge.day === 13 });
  // 范围外诚实 null + 干支名
  checks.push({ name: "F549 范围外诚实降级", pass: solarToLunar(2024, 1, 1) === null && solarToLunar(2032, 1, 1) === null && ganzhi(2026) === "丙午" });
  // Tooltip 行
  checks.push({ name: "F549 三要素一行", pass: hoverDateLine(2026, 9, 25).includes("星期五") && hoverDateLine(2026, 9, 25).includes("八月十五") });
  return checks;
}
