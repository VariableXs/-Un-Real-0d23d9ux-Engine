/**
 * F366 托盘电池显示选项（H 域 · AI-H4）：
 * 电池图标三档显示可选：纯图标（电量色块内显——四段色）、图标+百分比数字、纯百分比；
 * 低电量三阈值（F196/F268 联动）在任一档下都有独立视觉（黄/红/闪烁）；
 * 插电态闪电标记三档通用。
 * 判据（主册 F366）：三档渲染；色段四档阈值与实际电量对账；低电三态独立视觉；
 * 插电标记；切换即时性与持久化。
 * 依赖锚点：F196 电池保护策略 / F268 空间预警（阈值联动）。
 * 存储键：variable:h4:f366:mode
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

export type BatteryDisplayMode = "icon" | "iconPercent" | "percent";

/** 四段色阈值（与实际电量对账的口径源）：>60 绿 / >30 白（正常）/ >15 黄 / ≤15 红。 */
export const COLOR_BANDS = [
  { max: 15, tone: "red", label: "危急" },
  { max: 30, tone: "yellow", label: "低电量" },
  { max: 60, tone: "normal", label: "正常" },
  { max: 100, tone: "green", label: "充足" },
] as const;

export type BandTone = (typeof COLOR_BANDS)[number]["tone"];

/** 低电三阈值（F196/F268 联动口径）：注意 30 / 警告 20 / 危急 10。 */
export const LOW_THRESHOLDS = { notice: 30, warning: 20, critical: 10 } as const;

export type LowLevel = "none" | "notice" | "warning" | "critical";

export interface BatteryReading {
  /** 0-100（整数钳制）。 */
  percent: number;
  plugged: boolean;
}

const KEY = h4Key("f366", "mode");

export function getMode(store: KvStore = defaultStore()): BatteryDisplayMode {
  return readJson<BatteryDisplayMode>(store, KEY, "icon", (v): v is BatteryDisplayMode => v === "icon" || v === "iconPercent" || v === "percent");
}

/** 切换持久化（判据「切换即时性与持久化」——写失败如实上报）。 */
export function setMode(mode: BatteryDisplayMode, store: KvStore = defaultStore()): boolean {
  return writeJson(store, KEY, mode);
}

/** 电量钳制：非法输入（NaN/越界）显式钳回 0-100（零非法读数）。 */
export function clampPercent(p: number): number {
  if (!Number.isFinite(p)) return 0;
  return Math.min(100, Math.max(0, Math.round(p)));
}

/** 色段四档判定（判据「阈值与实际电量对账」）。 */
export function colorBand(percent: number): BandTone {
  const p = clampPercent(percent);
  return COLOR_BANDS.find((b) => p <= b.max)!.tone;
}

/** 低电三态（独立视觉的判定源，任一显示档共用）。 */
export function lowLevel(percent: number, plugged: boolean): LowLevel {
  if (plugged) return "none";
  const p = clampPercent(percent);
  if (p <= LOW_THRESHOLDS.critical) return "critical";
  if (p <= LOW_THRESHOLDS.warning) return "warning";
  if (p <= LOW_THRESHOLDS.notice) return "notice";
  return "none";
}

export interface TrayRender {
  mode: BatteryDisplayMode;
  /** 图标可见性。 */
  icon: boolean;
  /** 百分比文本（纯图标档为 null）。 */
  percentText: string | null;
  /** 内显色段（图标内色块）。 */
  band: BandTone;
  /** 插电闪电标记（三档通用）。 */
  bolt: boolean;
  /** 低电独立视觉（黄色=notice/warning；红色+闪烁=critical）。 */
  low: { level: LowLevel; color: "none" | "yellow" | "red"; blink: boolean };
}

/** 渲染模型（判据「三档渲染」逐档口径）。 */
export function renderTray(r: BatteryReading, mode: BatteryDisplayMode): TrayRender {
  const p = clampPercent(r.percent);
  const level = lowLevel(p, r.plugged);
  return {
    mode,
    icon: mode !== "percent",
    percentText: mode === "icon" ? null : `${p}%`,
    band: colorBand(p),
    bolt: r.plugged,
    low: { level, color: level === "critical" ? "red" : level === "none" ? "none" : "yellow", blink: level === "critical" },
  };
}

/** 对账：色段判定与电量区间逐档核对（判据「四档阈值与实际电量对账」）。 */
export function auditBandReconciliation(): { pass: boolean; cases: Array<{ percent: number; expect: BandTone; got: BandTone }> } {
  const cases: Array<{ percent: number; expect: BandTone; got: BandTone }> = [
    { percent: 0, expect: "red", got: colorBand(0) },
    { percent: 15, expect: "red", got: colorBand(15) },
    { percent: 16, expect: "yellow", got: colorBand(16) },
    { percent: 30, expect: "yellow", got: colorBand(30) },
    { percent: 31, expect: "normal", got: colorBand(31) },
    { percent: 60, expect: "normal", got: colorBand(60) },
    { percent: 61, expect: "green", got: colorBand(61) },
    { percent: 100, expect: "green", got: colorBand(100) },
  ];
  return { pass: cases.every((c) => c.expect === c.got), cases };
}

/* ================= v5 深化批次五：剩余时间 / 阈值边沿 / 双电池分列 ================= */

/** 剩余使用时间估算：当前放电速率注入（mA/h 语义由调用方给——本层不编硬件读数）。 */
export function runtimeEstimate(r: BatteryReading, drainPctPerHour: number | null): { minutes: number | null; honest: string } {
  if (drainPctPerHour === null || !Number.isFinite(drainPctPerHour) || drainPctPerHour <= 0) {
    return { minutes: null, honest: "无放电速率数据——不估算（不编数）" };
  }
  if (r.plugged) return { minutes: null, honest: "插电中——不适用续航估算" };
  const minutes = Math.round((clampPercent(r.percent) / drainPctPerHour) * 60);
  return { minutes, honest: `按当前耗电速率约可再用 ${Math.floor(minutes / 60)} 时 ${minutes % 60} 分` };
}

export interface ThresholdEdge {
  crossed: boolean;
  level: LowLevel;
  /** 边沿触发只提醒一次（同级别不重复轰炸——不惊扰纪律）。 */
  notify: boolean;
}

/** 阈值边沿检测：上一读数级别 → 当前级别，级别恶化才通知（恶化一级提一次）。 */
export function thresholdEdge(prevLevel: LowLevel, r: BatteryReading): ThresholdEdge {
  const cur = lowLevel(r.percent, r.plugged);
  const rank: Record<LowLevel, number> = { none: 0, notice: 1, warning: 2, critical: 3 };
  const crossed = rank[cur] > rank[prevLevel];
  return { crossed, level: cur, notify: crossed };
}

export interface DualBatteryReading {
  batteries: Array<{ id: string; label: string } & BatteryReading>;
}

/** 双电池分列（多电池设备一等公民）：每块独立判级、聚合读数取均值。 */
export function dualBatteryRender(bs: DualBatteryReading, mode: BatteryDisplayMode): { perBattery: TrayRender[]; aggregate: TrayRender } {
  const perBattery = bs.batteries.map((b) => renderTray(b, mode));
  const aggPct = bs.batteries.length === 0 ? 0 : Math.round(bs.batteries.reduce((s, b) => s + b.percent, 0) / bs.batteries.length);
  const plugged = bs.batteries.length > 0 && bs.batteries.every((b) => b.plugged);
  return { perBattery, aggregate: renderTray({ percent: aggPct, plugged }, mode) };
}
