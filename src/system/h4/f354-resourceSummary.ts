/**
 * F354 任务栏资源摘要（H 域 · AI-H4）：
 * 任务栏右下角可选资源摘要（默认关）：悬停系统区弹出三行迷你图
 * （CPU/内存/磁盘活动，30 秒历史 sparkline，1s 一点）。
 * 判据（主册 F354）：
 * - 三图数据准确性：读数进入前经 0-100 钳制 + 对账字段（来源计数）登记；
 * - 30s 历史刷新（1s 一点）——固定 30 槽环形历史，乱序/重复点拒收；
 * - 空闲通道判据：前台帧预算紧张时暂停采样（不抢前台帧率——F369 纪律同源）；
 * - 默认关：开关默认 false，开启状态持久化。
 * 依赖锚点：F060 电量账本（对账）/ F081 任务视图（双击直达）。
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

export interface ResourcePoint {
  /** 采样时刻（ms）。 */
  t: number;
  /** CPU 占用 0-100。 */
  cpu: number;
  /** 内存占用 0-100。 */
  mem: number;
  /** 磁盘活动 0-100。 */
  disk: number;
}

/** 30 秒 × 1 秒一点。 */
export const HISTORY_POINTS = 30;
export const SAMPLE_INTERVAL_MS = 1000;
/** 空闲通道门槛：前台帧耗时超过该值（ms）即让路停采（恢复由调用方重试）。 */
export const FG_FRAME_YIELD_MS = 8;

/** 存储键（开关持久化）。 */
const KEY = h4Key("f354", "enabled");

export function isEnabled(store: KvStore = defaultStore()): boolean {
  return readJson<boolean>(store, KEY, false, (v): v is boolean => typeof v === "boolean");
}

export function setEnabled(on: boolean, store: KvStore = defaultStore()): boolean {
  return writeJson(store, KEY, on);
}

/** 读数钳制：非法输入（NaN/超界）显式钳回 [0,100]，返回是否发生钳制。 */
export function clampReading(v: number): { value: number; clamped: boolean; valid: boolean } {
  if (typeof v !== "number" || Number.isNaN(v)) return { value: 0, clamped: false, valid: false };
  if (v < 0) return { value: 0, clamped: true, valid: true };
  if (v > 100) return { value: 100, clamped: true, valid: true };
  return { value: v, clamped: false, valid: true };
}

/** 采样准入：与上一点间隔 <900ms（重复/乱序）拒收；空闲通道紧张时拒收。 */
export function admitSample(history: ResourcePoint[], point: ResourcePoint, fgFrameMs: number): { admit: boolean; reason: "ok" | "interval" | "fg-yield" } {
  if (fgFrameMs > FG_FRAME_YIELD_MS) return { admit: false, reason: "fg-yield" };
  const last = history[history.length - 1];
  if (last && point.t - last.t < SAMPLE_INTERVAL_MS - 100) return { admit: false, reason: "interval" };
  return { admit: true, reason: "ok" };
}

/** 推入历史：固定 30 槽（超出挤最旧），返回新数组（不改入参）。 */
export function pushPoint(history: ResourcePoint[], point: ResourcePoint, fgFrameMs = 0): ResourcePoint[] {
  const verdict = admitSample(history, point, fgFrameMs);
  if (!verdict.admit) return history;
  const next = [...history, point];
  return next.length > HISTORY_POINTS ? next.slice(next.length - HISTORY_POINTS) : next;
}

/** sparkline 折线路径（0-100 → h 像素归一化；空/单点返回空串）。 */
export function sparklinePath(values: number[], width: number, height: number): string {
  if (values.length < 2) return "";
  const step = width / (HISTORY_POINTS - 1);
  const x0 = width - (values.length - 1) * step; // 右对齐：不足 30 点时左侧留白
  return values
    .map((v, i) => {
      const x = x0 + i * step;
      const y = height - (clampReading(v).value / 100) * height;
      return `${i === 0 ? "M" : "L"}${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}

export interface ReconcileRow {
  series: "cpu" | "mem" | "disk";
  /** 展示值（历史末点）。 */
  shown: number;
  /** 源头值（F060/性能计数口径）。 */
  source: number;
  /** 展示与源头偏差 %。 */
  deltaPct: number;
  ok: boolean;
}

/** 三图数据对账：展示值与源头值偏差 ≤3% 判合格（判据「数据准确性」）。 */
export function reconcile(history: ResourcePoint[], source: { cpu: number; mem: number; disk: number }): ReconcileRow[] {
  const last = history[history.length - 1];
  if (!last) return [];
  const rows: ReconcileRow[] = [];
  for (const series of ["cpu", "mem", "disk"] as const) {
    const shown = clampReading(last[series]).value;
    const src = clampReading(source[series]).value;
    const deltaPct = src === 0 ? Math.abs(shown) : Math.abs(shown - src) / src * 100;
    rows.push({ series, shown, source: src, deltaPct, ok: deltaPct <= 3 });
  }
  return rows;
}
