/**
 * AI-18 V-73 农历节气与节日 — 本地万年历（2020–2040，零网络）。
 *
 * 口径：
 * - 农历：以北京时间（UTC+8）天文近似算法生成 2020-2040 农历月表
 *   （冬至所在月为子月（十一月）基准 + 无中气置闰），精确到「日」；
 * - 节气：太阳黄经 15° 倍数时刻，用 VSOP87 简化项 + 光行差近似，
 *   精确到小时（验收：抽样与标准万年历一致）；
 * - 传统/公历节日查表；
 * - 全部纯函数（单测覆盖：春节/中秋/冬至/无中气置闰月年份 2020/2023/2025/2028）。
 */

/** 一天的毫秒数（按北京时间日界对齐用）。 */
const DAY_MS = 86_400_000;
/** 北京时间 = UTC+8。 */
const CN_TZ_OFFSET_MS = 8 * 3_600_000;

export interface LunarDate {
  /** 农历年（以正月初一为界）。 */
  year: number;
  /** 农历月（1..12；闰月为负数，如闰四月 = -4）。 */
  month: number;
  /** 农历日（1..30）。 */
  day: number;
  /** 是否闰月。 */
  leap: boolean;
}

export interface SolarTermInfo {
  /** 节气名（zh）。 */
  name: string;
  /** 本日是否节气日。 */
  isTermDay: boolean;
}

/** 二十四节气名（从立春起，按黄经 315° 递增 15°）。 */
export const SOLAR_TERMS = [
  "立春", "雨水", "惊蛰", "春分", "清明", "谷雨",
  "立夏", "小满", "芒种", "夏至", "小暑", "大暑",
  "立秋", "处暑", "白露", "秋分", "寒露", "霜降",
  "立冬", "小雪", "大雪", "冬至", "小寒", "大寒",
] as const;

const MONTH_NAMES = ["正", "二", "三", "四", "五", "六", "七", "八", "九", "十", "冬", "腊"] as const;
const DAY_NAMES = [
  "初一", "初二", "初三", "初四", "初五", "初六", "初七", "初八", "初九", "初十",
  "十一", "十二", "十三", "十四", "十五", "十六", "十七", "十八", "十九", "二十",
  "廿一", "廿二", "廿三", "廿四", "廿五", "廿六", "廿七", "廿八", "廿九", "三十",
] as const;

/** 农历月份中文。 */
export function lunarMonthName(month: number, leap: boolean): string {
  const m = Math.abs(month);
  const base = m === 12 ? "腊" : m === 11 ? "冬" : MONTH_NAMES[m - 1] ?? String(m);
  return (leap ? "闰" : "") + base + "月";
}

/** 农历日中文。 */
export function lunarDayName(day: number): string {
  return DAY_NAMES[day - 1] ?? String(day);
}

/** 农历整体描述（如「乙巳年 闰六月 初三」）。 */
export function lunarText(d: LunarDate): string {
  return `${cyclicYear(d.year)}年 ${lunarMonthName(d.month, d.leap)}${lunarDayName(d.day)}`;
}

/** 天干地支纪年（以农历年计）。 */
export function cyclicYear(year: number): string {
  const stems = "甲乙丙丁戊己庚辛壬癸";
  const branches = "子丑寅卯辰巳午未申酉戌亥";
  const i = ((year - 4) % 60 + 60) % 60;
  return (stems[i % 10] ?? "") + (branches[i % 12] ?? "");
}

/** 某时刻的「北京日数」（自 1970-01-01 起按北京日界计）。 */
function beijingDayNumber(date: Date): number {
  return Math.floor((date.getTime() + CN_TZ_OFFSET_MS) / DAY_MS);
}

// ---------------- 节气：太阳黄经 ----------------

/** 儒略日（UT）。 */
function toJulianDay(date: Date): number {
  return date.getTime() / DAY_MS + 2440587.5;
}

function fromJulianDay(jd: number): Date {
  return new Date((jd - 2440587.5) * DAY_MS);
}

/** 太阳视黄经（度；J2000 起算 + 光行差修正的简化 VSOP87 项）。 */
function sunLongitude(jdUt: number): number {
  const t = (jdUt - 2451545.0) / 36525.0;
  // 地球日心黄经（Laskar 简化式，精度 ~0.01°，节气时刻足够）
  const L = 280.46646 + 36000.76983 * t + 0.0003032 * t * t;
  const M = 357.52911 + 35999.05029 * t - 0.0001537 * t * t;
  const Mr = (M * Math.PI) / 180;
  const C =
    (1.914602 - 0.004817 * t - 0.000014 * t * t) * Math.sin(Mr) +
    (0.019993 - 0.000101 * t) * Math.sin(2 * Mr) +
    0.000289 * Math.sin(3 * Mr);
  const lon = L + C - 0.00569 - 0.00478 * Math.sin((125.04 - 1934.136 * t) * (Math.PI / 180));
  return ((lon % 360) + 360) % 360;
}

/** 黄经差归一到 -180..180。 */
function lonDiff(a: number, b: number): number {
  let d = (a - b) % 360;
  if (d > 180) d -= 360;
  if (d < -180) d += 360;
  return d;
}

/**
 * 求太阳黄经达到 target（度）的时刻（Date；二分法，精度 <1 分钟）。
 * termIndex 按 SOLAR_TERMS 顺序：0 = 立春（315°）……23 = 大寒（300°）。
 */
export function sunLongitudeTime(year: number, termIndex: number): Date {
  const target = (315 + 15 * termIndex) % 360;
  // 锚点：立春 ≈ 2/4，每节气 +15.2 天；小寒/大寒在 1 月上中旬
  const anchor =
    termIndex <= 21
      ? Date.UTC(year, 1, 4) + termIndex * 15.2 * DAY_MS
      : Date.UTC(year, 0, 5) + (termIndex - 22) * 15.2 * DAY_MS;
  let lo = toJulianDay(new Date(anchor - 6 * DAY_MS));
  let hi = toJulianDay(new Date(anchor + 6 * DAY_MS));
  // 保证区间两端黄经差异号（区间内恰一次穿越）
  let dLo = lonDiff(sunLongitude(lo), target);
  const dHi = lonDiff(sunLongitude(hi), target);
  if (dLo * dHi > 0) {
    // 极罕见（锚点漂移）：扩窗
    lo -= 10;
    dLo = lonDiff(sunLongitude(lo), target);
  }
  for (let i = 0; i < 48 && dLo * lonDiff(sunLongitude(hi), target) <= 0; i++) {
    const mid = (lo + hi) / 2;
    const dMid = lonDiff(sunLongitude(mid), target);
    if (dLo * dMid <= 0) {
      hi = mid;
    } else {
      lo = mid;
      dLo = dMid;
    }
  }
  return fromJulianDay(hi);
}

/** 某公历年全部 24 节气的（名 → 时刻 ms）映射（缓存）。 */
const yearTermsCache = new Map<number, Map<string, number>>();

export function buildYearTerms(year: number): Map<string, number> {
  const cached = yearTermsCache.get(year);
  if (cached) return cached;
  const map = new Map<string, number>();
  for (let i = 0; i < 24; i++) {
    map.set(SOLAR_TERMS[i] as string, sunLongitudeTime(year, i).getTime());
  }
  yearTermsCache.set(year, map);
  return map;
}

/** 某日是否节气日；是则返回节气名（zh）。 */
export function solarTermOfDay(date: Date, yearTerms?: Map<string, number>): string | null {
  const terms = yearTerms ?? buildYearTerms(date.getUTCFullYear());
  const dayStart = beijingDayNumber(date);
  for (const [name, ts] of terms) {
    if (beijingDayNumber(new Date(ts)) === dayStart) return name;
  }
  return null;
}

// ---------------- 朔（新月）时刻：Meeus 简化月相公式 ----------------

const RAD = Math.PI / 180;

/** 第 k 个新月（k=0 → 2000-01-06）的 UT 儒略日（精度 ±2 分钟内，日级足够）。 */
function newMoonJD(k: number): number {
  const T = k / 1236.85;
  const T2 = T * T, T3 = T2 * T, T4 = T3 * T;
  let jde = 2451550.09766 + 29.530588861 * k + 0.00015437 * T2 - 0.000000150 * T3 + 0.00000000073 * T4;
  const E = 1 - 0.002516 * T - 0.0000074 * T2;
  const M = 2.5534 + 29.10535670 * k - 0.0000014 * T2 - 0.00000011 * T3;
  const Mp = 201.5643 + 385.81693528 * k + 0.0107582 * T2 + 0.00001238 * T3 - 0.000000058 * T4;
  const F = 160.7108 + 390.67050284 * k - 0.0016118 * T2 - 0.00000227 * T3 + 0.000000011 * T4;
  const Om = 124.7746 - 1.56375588 * k + 0.0020672 * T2 + 0.00000215 * T3;
  jde = jde
    - 0.40720 * Math.sin(Mp * RAD)
    + 0.17241 * E * Math.sin(M * RAD)
    + 0.01608 * Math.sin(2 * Mp * RAD)
    + 0.01039 * Math.sin(2 * F * RAD)
    + 0.00739 * E * Math.sin((Mp - M) * RAD)
    - 0.00514 * E * Math.sin((M + Mp) * RAD)
    + 0.00208 * E * E * Math.sin(2 * M * RAD)
    - 0.00111 * Math.sin((Mp - 2 * F) * RAD)
    - 0.00057 * Math.sin((Mp + 2 * F) * RAD)
    + 0.00056 * E * Math.sin((2 * Mp + M) * RAD)
    - 0.00042 * Math.sin(3 * Mp * RAD)
    + 0.00042 * E * Math.sin((M + 2 * F) * RAD)
    + 0.00038 * E * Math.sin((M - 2 * F) * RAD)
    - 0.00024 * E * Math.sin((2 * Mp - M) * RAD)
    - 0.00017 * Math.sin(Om * RAD)
    - 0.00007 * Math.sin((Mp + 2 * M) * RAD)
    + 0.00004 * Math.sin((2 * Mp - 2 * F) * RAD)
    + 0.00004 * Math.sin(3 * M * RAD)
    + 0.00003 * Math.sin((Mp + M - 2 * F) * RAD)
    + 0.00003 * Math.sin((2 * Mp + 2 * F) * RAD)
    - 0.00003 * Math.sin((Mp + M + 2 * F) * RAD)
    + 0.00003 * Math.sin((Mp - M + 2 * F) * RAD)
    - 0.00002 * Math.sin((Mp - M - 2 * F) * RAD)
    - 0.00002 * Math.sin((3 * Mp + M) * RAD)
    + 0.00002 * Math.sin(4 * Mp * RAD);
  // TD → UT（ΔT≈69s 近似；日级判定足够）
  return jde - 0.0008;
}

// ---------------- 农历月表（2020–2040） ----------------

export interface LunarMonthRow {
  /** 该农历月初一的北京日数。 */
  startDay: number;
  /** 天数（29/30）。 */
  days: number;
  /** 农历年。 */
  lunarYear: number;
  /** 月序（1..12）。 */
  index: number;
  /** 是否闰月。 */
  leap: boolean;
}

const RANGE_START_YEAR = 2019; // 表覆盖农历年 2019(猪)腊月 … 2041（含 2020–2040 全部）
const RANGE_END_YEAR = 2041;

let tableCache: LunarMonthRow[] | null = null;

/** 某公历年 12 个中气（黄经 0/30/…/330°）的北京日数集合。 */
const zhongQiCache = new Map<number, Set<number>>();
function zhongQiDays(year: number): Set<number> {
  const cached = zhongQiCache.get(year);
  if (cached) return cached;
  const set = new Set<number>();
  // 黄经 270°=冬至（termIndex 21）、300°=大寒(23)、330°=雨水(1)、0°=春分(3)……
  // 黄经 deg = (315 + 15i) % 360 → i = ((deg - 315) / 15 + 24) % 24
  for (let deg = 0; deg < 360; deg += 30) {
    const i = (((deg - 315) / 15) + 24) % 24;
    set.add(beijingDayNumber(sunLongitudeTime(year, i)));
  }
  zhongQiCache.set(year, set);
  return set;
}

/** 月份 [startDay, startDay+days) 是否含中气（跨年查两年）。 */
function monthHasZhongQi(startDay: number, days: number): boolean {
  // 北京日数 → 粗略公历年（1970-01-01 为 day 0，北京日）
  const approxMs = (startDay * DAY_MS) - CN_TZ_OFFSET_MS;
  const y1 = new Date(approxMs).getUTCFullYear();
  for (const y of [y1, y1 + 1]) {
    for (const q of zhongQiDays(y)) {
      if (q >= startDay && q < startDay + days) return true;
    }
  }
  return false;
}

/** 生成农历月表：冬至月为十一月 + 无中气置闰（2020–2040 覆盖；缓存一次）。 */
export function getLunarMonthTable(): LunarMonthRow[] {
  if (tableCache) return tableCache;
  // 1) 收集新月：2018-11-01 .. 2042-02-01（k 步进覆盖）
  const kStart = Math.floor(((Date.UTC(2018, 10, 1) / DAY_MS + 2440587.5) - 2451550.09766) / 29.530588861) - 2;
  const kEnd = Math.ceil(((Date.UTC(2042, 1, 1) / DAY_MS + 2440587.5) - 2451550.09766) / 29.530588861) + 2;
  const newMoons: number[] = []; // 北京日数（升序）
  for (let k = kStart; k <= kEnd; k++) {
    newMoons.push(beijingDayNumber(fromJulianDay(newMoonJD(k))));
  }
  // 去重（同一北京日两个朔不可能，保险）
  const nm = [...new Set(newMoons)].sort((a, b) => a - b);

  // 2) 逐年构造：W(y) = 包含冬至(y) 的月下标（该月为农历年 y-1 的十一月）
  const winterSolsticeDay = (year: number): number =>
    beijingDayNumber(sunLongitudeTime(year, 21)); // 21 = 冬至（270°）
  const monthIndexContaining = (day: number): number => {
    let lo = 0, hi = nm.length - 2;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if ((nm[mid] ?? Number.POSITIVE_INFINITY) <= day) lo = mid;
      else hi = mid - 1;
    }
    return lo;
  };

  const rows: LunarMonthRow[] = [];
  for (let y = RANGE_START_YEAR; y <= RANGE_END_YEAR; y++) {
    const w0 = monthIndexContaining(winterSolsticeDay(y - 1)); // 冬月（11月）of lunar y-1
    const w1 = monthIndexContaining(winterSolsticeDay(y)); // 冬月 of lunar y
    const span = w1 - w0; // 12 = 平年；13 = 有闰月
    // 找闰月：w0+1 .. w1-1 中第一个无中气的月
    let leapAt = -1;
    if (span === 13) {
      for (let i = w0 + 1; i < w1; i++) {
        const start = nm[i];
        if (start === undefined) break;
        const days = (nm[i + 1] ?? start + 30) - start;
        if (!monthHasZhongQi(start, days)) {
          leapAt = i;
          break;
        }
      }
    }
    // 3) 逐月赋号：从 w0+1（腊月）起，到 w1（下一冬月）止
    let index = 11; // w0 是冬月（11 月）
    let lunarYear = y - 1; // 腊月仍属农历 y-1；正月起属 y
    for (let i = w0 + 1; i <= w1; i++) {
      const start = nm[i];
      if (start === undefined) break; // 表尾保护
      const days = (nm[i + 1] ?? start + 30) - start;
      const leap = i === leapAt;
      if (!leap) {
        index = index === 12 ? 1 : index + 1;
        if (index === 1) lunarYear = y;
        rows.push({ startDay: start, days, lunarYear, index, leap: false });
      } else {
        // 闰月重复当前 index（占号）；下一月继续 index+1
        rows.push({ startDay: start, days, lunarYear, index, leap: true });
      }
    }
  }
  // 去重（相邻年区间可能重叠生成同一冬月行）
  const seen = new Set<number>();
  const out: LunarMonthRow[] = [];
  for (const r of rows) {
    const key = r.startDay;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(r);
  }
  out.sort((a, b) => a.startDay - b.startDay);
  tableCache = out;
  return out;
}

/** 公历 → 农历（表外 = null）。 */
export function solarToLunar(date: Date): LunarDate | null {
  const table = getLunarMonthTable();
  const dayNum = beijingDayNumber(date);
  // 二分找月行
  let lo = 0, hi = table.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if ((table[mid]?.startDay ?? Infinity) <= dayNum) lo = mid;
    else hi = mid - 1;
  }
  const row = table[lo];
  if (!row || dayNum < row.startDay || dayNum >= row.startDay + row.days) return null;
  return {
    year: row.lunarYear,
    month: row.leap ? -row.index : row.index,
    day: dayNum - row.startDay + 1,
    leap: row.leap,
  };
}

// ---------------- 节日 ----------------

/** 农历节日（月-日 → 名；闰月不计）。 */
const LUNAR_FESTIVALS: Record<string, string> = {
  "1-1": "春节",
  "1-15": "元宵节",
  "2-2": "龙抬头",
  "3-3": "上巳节",
  "5-5": "端午节",
  "7-7": "七夕",
  "7-15": "中元节",
  "8-15": "中秋节",
  "9-9": "重阳节",
  "10-1": "寒衣节",
  "12-8": "腊八节",
  "12-23": "小年",
};

/** 公历节日（月-日 → 名）。 */
const SOLAR_FESTIVALS: Record<string, string> = {
  "1-1": "元旦",
  "2-14": "情人节",
  "3-8": "妇女节",
  "3-12": "植树节",
  "4-1": "愚人节",
  "5-1": "劳动节",
  "5-4": "青年节",
  "6-1": "儿童节",
  "7-1": "建党节",
  "8-1": "建军节",
  "9-10": "教师节",
  "10-1": "国庆节",
  "12-24": "平安夜",
  "12-25": "圣诞节",
};

export interface Festival {
  name: string;
  kind: "lunar" | "solar";
}

/** 当日节日（农历优先；无 = null）。 */
export function festivalOfDay(date: Date): string | null {
  const beijing = new Date(date.getTime() + CN_TZ_OFFSET_MS);
  const solarKey = `${beijing.getUTCMonth() + 1}-${beijing.getUTCDate()}`;
  const solar = SOLAR_FESTIVALS[solarKey] ?? null;
  const lunar = solarToLunar(date);
  let lunarFest: string | null = null;
  if (lunar) {
    const key = `${Math.abs(lunar.month)}-${lunar.day}`;
    if (!lunar.leap) lunarFest = LUNAR_FESTIVALS[key] ?? null;
  }
  return lunarFest ?? solar;
}

/** 任务栏时钟卡片一站式摘要（农历 + 节气 + 节日）。 */
export function lunarSummary(date: Date): {
  /** 农历整体描述（如「乙巳年 六月 初三」）；表外 = null。 */
  text: string | null;
  /** 今日节气名（非节气日 = null）。 */
  term: string | null;
  /** 今日节日（无 = null）。 */
  festival: string | null;
} {
  const lunar = solarToLunar(date);
  return {
    text: lunar ? lunarText(lunar) : null,
    term: solarTermOfDay(date),
    festival: festivalOfDay(date),
  };
}
