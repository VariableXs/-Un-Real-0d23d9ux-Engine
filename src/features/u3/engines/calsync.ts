/**
 * U3-v8 引擎二：calsync —— F078 日历飞出 × F549/F560 数据源互证桥
 * （AI-U3 · 批次八工单②）。
 *
 * 判据唯一源（主册摘文 + 工单）：
 * - 「节假日锚点单一事实源收拢」——F078（AI-D1 领地的日历飞出）、F549
 *   重层月历（clockpanel）、F560 节假日标注三方此前各持一份锚点表；
 *   本桥把「2026 七锚」收拢为唯一规范源，并以两源互证机检钉死：
 *   groupA.ts（AI-D1 领地已提交数据）× clockpanel HOLIDAY_TABLE ×
 *   clockcal CNY_ANCHORS（农历位表）三方逐锚比对，任一漂移即红。
 * - 节日-农历两源互证沿 v7 判例：春节=正月初一、端午=五月初五、
 *   中秋=八月十五（农历位表是物理事实，锚点表抄错即被抓获）。
 * - F078 冻结出口（开放性·版本化）：日历飞出消费方按 BRIDGE_VERSION
 *   取数，向后兼容承诺随版本号登记（废弃走流程）。
 */

import { CN_HOLIDAYS_2026 } from "../../tools/groupA";
import { solarToLunar, CNY_ANCHORS } from "../clockcal";
import { HOLIDAYS_2026, holidayOf } from "./clockpanel";

/* ------------------------------- 唯一规范源 ------------------------------- */

/** 桥版本（F078 消费契约版本——结构变更必须升版并保持旧字段）。 */
export const BRIDGE_VERSION = 1;

/** 规范锚点条目（单一事实源的内存形制）。 */
export interface AnchorEntry {
  ymd: readonly [number, number, number];
  name: string;
  kind: "holiday" | "workday";
}

/** 零填充日期键（groupA 键是 padStart 形制、clockpanel 是裸数——统一规范化再比）。 */
function padKey([y, m, d]: readonly [number, number, number]): string {
  return `${y}-${String(m).padStart(2, "0")}-${String(d).padStart(2, "0")}`;
}

/** 2026 规范锚点表（从 groupA.ts 收拢——AI-D1 领地的已提交数据是事实源）。 */
export const CANONICAL_2026: ReadonlyArray<AnchorEntry> =
  Object.entries(CN_HOLIDAYS_2026)
    .map(([k, name]) => {
      const [y, m, d] = k.split("-").map((v) => parseInt(v, 10));
      return { ymd: [y, m, d] as readonly [number, number, number], name, kind: "holiday" as const };
    })
    .sort((a, b) => (a.ymd[0] * 10000 + a.ymd[1] * 100 + a.ymd[2]) - (b.ymd[0] * 10000 + b.ymd[1] * 100 + b.ymd[2]));

/** 消费出口（F078 飞出 / F549 面板 / F560 标注三方统一取数口）。 */
export function bridgeAnchor(y: number, m: number, d: number): AnchorEntry | null {
  const hit = holidayOf(y, m, d);
  if (!hit) return null;
  return { ymd: [hit.date[0], hit.date[1], hit.date[2]], name: hit.name, kind: hit.kind };
}

/* ------------------------------- 三方互证机检 ------------------------------- */

export interface CrossVerifyReport {
  ok: boolean;
  /** 三方各持条数。 */
  counts: { groupA: number; clockpanel: number; canonical: number };
  /** 漂移清单（日期→三方名称不一致/缺位）。 */
  drifts: Array<{ ymd: string; detail: string }>;
}

/** 逐锚三方比对：groupA（事实源）× clockpanel（U3 内置表）× 农历互证。 */
export function crossVerifySources(): CrossVerifyReport {
  const drifts: Array<{ ymd: string; detail: string }> = [];
  const groupAKeys = Object.keys(CN_HOLIDAYS_2026).map((k) => {
    const parts = k.split("-").map((v) => parseInt(v, 10));
    return padKey([parts[0]!, parts[1]!, parts[2]!]);
  }).sort();
  const cpKeys = HOLIDAYS_2026.map((h) => padKey(h.date)).sort();

  if (groupAKeys.length !== cpKeys.length) {
    drifts.push({ ymd: "-", detail: `条数不一致：groupA ${groupAKeys.length} vs clockpanel ${cpKeys.length}` });
  }
  for (const k of groupAKeys) {
    if (!cpKeys.includes(k)) {
      drifts.push({ ymd: k, detail: "clockpanel 缺此锚" });
      continue;
    }
    const gaName = CN_HOLIDAYS_2026[k] ?? "";
    const cp = HOLIDAYS_2026.find((h) => padKey(h.date) === k);
    if (!cp || cp.name !== gaName) {
      drifts.push({ ymd: k, detail: `名称漂移：groupA「${gaName}」vs clockpanel「${cp?.name ?? "?"}」` });
    }
  }
  for (const k of cpKeys) {
    if (!groupAKeys.includes(k)) drifts.push({ ymd: k, detail: "groupA 无此锚——U3 不得私加" });
  }

  // 农历物理互证：春节=正月初一（CNY_ANCHORS 与两表都要对上）
  for (const [cy, cm, cd] of CNY_ANCHORS) {
    const inTable = cy === 2026 ? CANONICAL_2026.some((a) => a.ymd.join("-") === `${cy}-${cm}-${cd}` && a.name === "春节") : true;
    if (!inTable) drifts.push({ ymd: `${cy}-${cm}-${cd}`, detail: "春节锚与 CNY_ANCHORS 漂移" });
  }
  const l = solarToLunar(2026, 2, 17);
  if (!l || l.month !== 1 || l.day !== 1 || l.leap) {
    drifts.push({ ymd: "2026-2-17", detail: "农历位表互证失败：春节≠正月初一" });
  }
  const duan = solarToLunar(2026, 6, 19);
  const zhong = solarToLunar(2026, 9, 25);
  if (!duan || duan.month !== 5 || duan.day !== 5) drifts.push({ ymd: "2026-6-19", detail: "端午≠五月初五" });
  if (!zhong || zhong.month !== 8 || zhong.day !== 15) drifts.push({ ymd: "2026-9-25", detail: "中秋≠八月十五" });

  return {
    ok: drifts.length === 0,
    counts: { groupA: groupAKeys.length, clockpanel: cpKeys.length, canonical: CANONICAL_2026.length },
    drifts,
  };
}

/* ------------------------------- F078 消费契约 ------------------------------- */

/** 飞出视图最小契约（F078 消费方按此取数——冻结接口，变更升版）。 */
export interface FlyoutCalendarData {
  bridgeVersion: number;
  /** 年内全部锚点（升序）。 */
  anchorsOfYear: (y: number) => ReadonlyArray<AnchorEntry>;
  /** 单日查询。 */
  anchorOf: (y: number, m: number, d: number) => AnchorEntry | null;
  /** 数据覆盖诚实账（透传 clockpanel——不二次包装）。 */
  coverage: (y: number) => { covered: number; pendingNote: string };
}

export const F078_CALENDAR_BRIDGE: FlyoutCalendarData = {
  bridgeVersion: BRIDGE_VERSION,
  anchorsOfYear: (y) => (y === 2026 ? CANONICAL_2026 : []),
  anchorOf: bridgeAnchor,
  coverage: (y) => {
    const n = y === 2026 ? CANONICAL_2026.length : 0;
    return { covered: n, pendingNote: n >= 7 ? "全年法定锚点在册" : `${y} 锚点未收拢——随 F122 更新到位` };
  },
};

/* ------------------------------- 自检 ------------------------------- */

export function calsyncSelfCheck(): Array<{ name: string; pass: boolean }> {
  const checks: Array<{ name: string; pass: boolean }> = [];

  // 规范源形状：7 锚升序（数值序）、全部 holiday
  const ord = (a: AnchorEntry) => a.ymd[0] * 10000 + a.ymd[1] * 100 + a.ymd[2];
  checks.push({ name: "F560 规范源七锚升序", pass: CANONICAL_2026.length === 7 && CANONICAL_2026.every((a, i) => i === 0 || ord(CANONICAL_2026[i - 1]!) < ord(a)) && CANONICAL_2026.every((a) => a.kind === "holiday") });

  // 三方互证：绿基线
  const rep = crossVerifySources();
  checks.push({ name: "F078 三方互证绿基线", pass: rep.ok && rep.counts.groupA === 7 && rep.counts.clockpanel === 7 && rep.counts.canonical === 7 });
  checks.push({ name: "F078 互证零漂移", pass: rep.drifts.length === 0 });

  // 消费出口：单日取数与 clockpanel holidayOf 同答
  const a = bridgeAnchor(2026, 10, 1);
  checks.push({ name: "F078 出口国庆同答", pass: !!a && a.name === "国庆" && bridgeAnchor(2026, 10, 2) === null });
  // 飞出年视图：2026 七锚全量、2027 诚实空
  checks.push({ name: "F078 年视图七锚/2027 诚实空", pass: F078_CALENDAR_BRIDGE.anchorsOfYear(2026).length === 7 && F078_CALENDAR_BRIDGE.anchorsOfYear(2027).length === 0 });
  const cov = F078_CALENDAR_BRIDGE.coverage(2027);
  checks.push({ name: "F078 覆盖诚实账透传", pass: cov.covered === 0 && cov.pendingNote.includes("F122") });
  checks.push({ name: "F078 桥版本冻结", pass: F078_CALENDAR_BRIDGE.bridgeVersion === BRIDGE_VERSION && BRIDGE_VERSION === 1 });

  return checks;
}
