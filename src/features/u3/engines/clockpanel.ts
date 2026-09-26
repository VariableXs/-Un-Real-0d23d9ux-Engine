/**
 * U3-v7 引擎三：clockpanel —— 时钟点击面板（日历重层）装配引擎
 * （AI-U3 · 批次七）。
 *
 * 判据唯一源（主册摘文）：
 * - F549「悬停轻（Tooltip）、点击重（日历飞出 F078 形制）——两层日历体验
 *   各司其职」——本引擎装配点击重层：月历网格 + 农历逐日标注；悬停轻层
 *   （三要素一行 Tooltip）复用 clockcal，数据同源零漂移。
 * - F560「法定节假日红色标注+名称悬停、调休补班灰色标注——离线内置当年
 *   与次年数据；标注克制（只有法定级）」。
 *
 * 【数据源一处一事实】农历换算复用 clockcal（solarToLunar/weekday——与内核
 * ustar3::clockcal 同一份数据源）；节假日锚点表自 src/features/tools/groupA.ts
 * CN_HOLIDAYS_2026 同源移植（F03835 检查点判据「2026-10-01=国庆」本引擎
 * 同判据互证）。2027 年只内置春节锚（与 CNY_ANCHORS 同源可验证），其余
 * 2027 法定安排随 F122 系统更新到位——数据结构预留、不编造（诚实原则）。
 *
 * 【偏差登记】主册 F549 示例句「2026-09-25 八月初五」与真实万年历不符
 * （该日实为八月十五中秋）——沿用 clockcal 批次已登记的偏差事实锚。
 */

import {
  solarToLunar, weekday, daysFromCivil, lunarMonthName, lunarDayName, ganzhi,
  CNY_ANCHORS, LUNAR_DATA_FIRST_YEAR, LUNAR_DATA_LAST_YEAR,
} from "../clockcal";

/* ------------------------------- F560 节假日数据 ------------------------------- */

/** 标注种类（判据：红=法定放假、灰=调休补班——双色克制）。 */
export type HolidayKind = "holiday" | "workday";

export interface HolidayEntry {
  date: readonly [number, number, number]; // y,m,d
  name: string;
  kind: HolidayKind;
}

/** 2026 法定节日锚点（同源移植 groupA.ts CN_HOLIDAYS_2026——F03835 同判据）。 */
export const HOLIDAYS_2026: ReadonlyArray<HolidayEntry> = [
  { date: [2026, 1, 1], name: "元旦", kind: "holiday" },
  { date: [2026, 2, 17], name: "春节", kind: "holiday" },
  { date: [2026, 4, 5], name: "清明", kind: "holiday" },
  { date: [2026, 5, 1], name: "劳动节", kind: "holiday" },
  { date: [2026, 6, 19], name: "端午", kind: "holiday" },
  { date: [2026, 9, 25], name: "中秋", kind: "holiday" },
  { date: [2026, 10, 1], name: "国庆", kind: "holiday" },
];

/** 2027 已可同源验证的锚点：春节（CNY_ANCHORS[2] = 2027-02-06）。 */
export const HOLIDAYS_2027: ReadonlyArray<HolidayEntry> = [
  { date: [2027, 2, 6], name: "春节", kind: "holiday" },
];

export const HOLIDAY_TABLE: ReadonlyArray<HolidayEntry> = [...HOLIDAYS_2026, ...HOLIDAYS_2027];

/** 数据覆盖诚实账（判据：当年与次年——次年未到位的节日显性 pending 不编造）。 */
export function holidayCoverage(year: number): { covered: number; pendingNote: string } {
  const inYear = HOLIDAY_TABLE.filter((h) => h.date[0] === year && h.kind === "holiday");
  if (year >= LUNAR_DATA_FIRST_YEAR && year <= LUNAR_DATA_LAST_YEAR) {
    return {
      covered: inYear.length,
      pendingNote: inYear.length >= 7 ? "全年法定锚点在册" : `${year} 仅 ${inYear.length} 个锚点在册——其余随 F122 更新到位`,
    };
  }
  return { covered: 0, pendingNote: `${year} 超出内置范围——诚实无数据` };
}

/** 逐日标注查询（红/灰/无——克制：非法定级不标）。 */
export function holidayOf(y: number, m: number, d: number): HolidayEntry | null {
  return HOLIDAY_TABLE.find((h) => h.date[0] === y && h.date[1] === m && h.date[2] === d) ?? null;
}

/* ------------------------------- 月历网格 ------------------------------- */

/** 周起始：周一（六章一致性：全系统月历一套周序）。 */
export const WEEK_START = 1;
/** 网格固定 6 行 × 7 列（任何月份都满格——高度不跳变）。 */
export const GRID_ROWS = 6;
export const GRID_COLS = 7;

export interface CalendarCell {
  y: number; m: number; d: number;
  inMonth: boolean;
  /** 农历标注行（初一显月名、余显日名；范围外 null 诚实降级）。 */
  lunarText: string | null;
  /** 节假日标注（红/灰）。 */
  holiday: HolidayEntry | null;
  isToday: boolean;
}

/** 月份天数（公历）。 */
function daysInMonth(y: number, m: number): number {
  const start = daysFromCivil(y, m, 1);
  const next = m === 12 ? daysFromCivil(y + 1, 1, 1) : daysFromCivil(y, m + 1, 1);
  return next - start;
}

/**
 * 月历网格（判据：周一起始、6×7 满格、前后月补位灰显、农历逐日、
 * 节假日双色、今日高亮）。
 */
export function buildMonthGrid(y: number, m: number, today: readonly [number, number, number]): CalendarCell[] {
  const firstWd = weekday(y, m, 1); // 1=周一 … 7=周日
  const lead = (firstWd - WEEK_START + 7) % 7;
  const dim = daysInMonth(y, m);
  // 前月尾 + 本月 + 后月头 → 凑满 42 格
  const prevDim = m === 1 ? daysInMonth(y - 1, 12) : daysInMonth(y, m - 1);
  const cells: CalendarCell[] = [];
  for (let i = lead - 1; i >= 0; i--) {
    const d = prevDim - i;
    const [py, pm] = m === 1 ? [y - 1, 12] : [y, m - 1];
    cells.push(makeCell(py, pm, d, false, today));
  }
  for (let d = 1; d <= dim; d++) cells.push(makeCell(y, m, d, true, today));
  let nextD = 1;
  const [ny, nm] = m === 12 ? [y + 1, 1] : [y, m + 1];
  while (cells.length < GRID_ROWS * GRID_COLS) {
    cells.push(makeCell(ny, nm, nextD++, false, today));
  }
  return cells;
}

function makeCell(y: number, m: number, d: number, inMonth: boolean, today: readonly [number, number, number]): CalendarCell {
  const l = solarToLunar(y, m, d);
  // 初一显月名（含闰），余显日名——万年历通行形制
  const lunarText = l ? (l.day === 1 ? lunarMonthName(l) : lunarDayName(l)) : null;
  return {
    y, m, d, inMonth, lunarText,
    holiday: holidayOf(y, m, d),
    isToday: today[0] === y && today[1] === m && today[2] === d,
  };
}

/* ------------------------------- 面板两层分工 ------------------------------- */

/** 悬停轻层（F549 Tooltip）：500ms、三要素一行——复用 clockcal 同源数据。 */
export { hoverDateLine } from "../clockcal";

/** 点击重层打开（判据：点击时钟展开面板——与悬停层互不干扰）。 */
export interface PanelState {
  open: boolean;
  viewYear: number;
  viewMonth: number;
  selected: readonly [number, number, number] | null;
}

export function panelInit(today: readonly [number, number, number]): PanelState {
  return { open: false, viewYear: today[0], viewMonth: today[1], selected: null };
}

export function panelToggle(s: PanelState, today: readonly [number, number, number]): PanelState {
  return s.open
    ? { ...s, open: false }
    : { ...s, open: true, viewYear: today[0], viewMonth: today[1], selected: [today[0], today[1], today[2]] };
}

/** 月份翻页（任意 delta 线性化到月序号轴——1↔12 跨年、多月跳转都有归宿）。 */
export function panelShiftMonth(s: PanelState, delta: number): PanelState {
  const total = s.viewYear * 12 + (s.viewMonth - 1) + delta;
  const y = Math.floor(total / 12);
  const m = (((total % 12) + 12) % 12) + 1;
  return { ...s, viewYear: y, viewMonth: m };
}

/** 选中日（点任意格——含灰显的前后月格，翻到该月并选中）。 */
export function panelSelect(s: PanelState, c: CalendarCell): PanelState {
  return { ...s, viewYear: c.y, viewMonth: c.m, selected: [c.y, c.m, c.d] };
}

/** 面板标题（公历月 + 干支年——干支表同源 clockcal，零复制）。 */
export function panelTitle(s: PanelState): string {
  // 取月中任一日换农历年（月内可能跨农历年边界——取 15 日为锚，误差只影响边界月的干支显示，登记为已知形制）
  const l = solarToLunar(s.viewYear, s.viewMonth, 15);
  return `${s.viewYear} 年 ${s.viewMonth} 月${l ? ` · ${ganzhi(l.year)}年` : ""}`;
}

/* ------------------------------- 自检 ------------------------------- */

/** F549 点击重层 / F560 节假日判据自检。 */
export function clockpanelSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];
  const today: readonly [number, number, number] = [2026, 9, 25];

  // 网格结构：42 格满、2026-09-01 周二（首格周一补位灰显）
  const grid = buildMonthGrid(2026, 9, today);
  checks.push({ name: "F549 网格 6×7 满格", pass: grid.length === GRID_ROWS * GRID_COLS });
  const first = grid.find((c) => c.inMonth && c.d === 1);
  checks.push({ name: "F549 周一起始补位", pass: !!first && grid.indexOf(first) === 1 && !grid[0]!.inMonth });
  const last = grid.filter((c) => c.inMonth).at(-1);
  checks.push({ name: "F549 本月 30 天+后月补位", pass: !!last && last.d === 30 && grid.filter((c) => !c.inMonth).length === 12 });

  // 农历逐日：中秋 9/25 = 八月十五（与 clockcal 同源互证）
  const mid = grid.find((c) => c.m === 9 && c.d === 25);
  checks.push({ name: "F549 逐日农历中秋锚", pass: mid?.lunarText === "十五" });
  const day1 = grid.find((c) => c.m === 9 && c.d === 11);
  checks.push({ name: "F549 初一显月名", pass: day1?.lunarText === "八月" });

  // 节假日：红标在位 + 名称
  const h = holidayOf(2026, 10, 1);
  checks.push({ name: "F560 国庆红标在册", pass: !!h && h.kind === "holiday" && h.name === "国庆" });
  const cell = grid.find((c) => c.m === 10 && c.d === 1);
  checks.push({ name: "F560 网格内节日标注挂接", pass: cell?.holiday?.name === "国庆" });
  const cny = holidayOf(2026, 2, 17);
  checks.push({ name: "F560 春节锚点", pass: !!cny && cny.name === "春节" });

  // 节日-农历互证：端午=五月初五、中秋=八月十五、春节=正月初一（数据两源互锁）
  const duan = solarToLunar(2026, 6, 19);
  const zhong = solarToLunar(2026, 9, 25);
  const chunjie = solarToLunar(2026, 2, 17);
  checks.push({
    name: "F560 节日-农历两源互证",
    pass: !!duan && duan.month === 5 && duan.day === 5 && !duan.leap
      && !!zhong && zhong.month === 8 && zhong.day === 15 && !zhong.leap
      && !!chunjie && chunjie.month === 1 && chunjie.day === 1 && !chunjie.leap,
  });

  // 覆盖诚实账：2027 仅春节在册显性 pending、超范围显性无数据
  const cov27 = holidayCoverage(2027);
  const cov24 = holidayCoverage(2024);
  checks.push({ name: "F560 两年覆盖诚实账", pass: cov27.covered === 1 && cov27.pendingNote.includes("F122") && cov24.covered === 0 && cov24.pendingNote.includes("超") });
  // 2027 春节锚与 CNY_ANCHORS 同源
  const cny27 = CNY_ANCHORS.find((a) => a[0] === 2027);
  checks.push({ name: "F560 2027 春节锚同源", pass: !!cny27 && cny27[1] === HOLIDAYS_2027[0]!.date[1] && cny27[2] === HOLIDAYS_2027[0]!.date[2] });

  // 两层分工状态机：初始化不开、点开归今天、再点关闭
  let ps = panelInit(today);
  checks.push({ name: "F549 面板初始关闭", pass: !ps.open && ps.viewMonth === 9 });
  ps = panelToggle(ps, today);
  checks.push({ name: "F549 点开归今天+选中今天", pass: ps.open && ps.viewYear === 2026 && ps.selected?.join("-") === "2026-9-25" });
  ps = panelShiftMonth(ps, -1);
  checks.push({ name: "F549 翻页跨月", pass: ps.viewMonth === 8 });
  ps = panelShiftMonth(ps, -8);
  checks.push({ name: "F549 翻页跨年有归宿", pass: ps.viewYear === 2025 && ps.viewMonth === 12 });
  ps = panelShiftMonth(ps, 1);
  checks.push({ name: "F549 翻页进新年", pass: ps.viewYear === 2026 && ps.viewMonth === 1 });
  const outside = grid[0]!; // 灰显的前月格
  ps = panelSelect(ps, outside);
  checks.push({ name: "F549 点灰显格翻月选中", pass: ps.viewYear === outside.y && ps.viewMonth === outside.m && ps.selected?.join("-") === `${outside.y}-${outside.m}-${outside.d}` });
  ps = panelToggle(ps, today);
  checks.push({ name: "F549 再点关闭", pass: !ps.open });

  // 今日高亮
  const todayCell = grid.find((c) => c.isToday);
  checks.push({ name: "F549 今日唯一高亮", pass: !!todayCell && grid.filter((c) => c.isToday).length === 1 && todayCell.inMonth });

  return checks;
}
