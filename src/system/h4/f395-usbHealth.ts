/**
 * F395 U 盘健康监控（H 域 · AI-H4）：
 * U 盘系统专属：宿主介质健康页（可读 SMART/厂商寿命寄存器则读，读不到诚实显示
 * 「该介质不提供健康数据」不编数）——写入量累计/剩余寿命百分比/温度（如有）/坏块重映射
 * 计数；寿命 <20% 黄色提示（建议迁移）、<10% 红色（强烈建议立即备份 F396）。
 * 判据（主册 F395）：读得到/读不到两分支用例；写入量累计准确性（对账 IO 计数）；
 * 两阈值提示与出路链接；多介质（S: 共享卷）分列。
 * 依赖锚点：F396 备份向导（出路链接）/ F183 存储健康监测（内核同源数据）。
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

/** 两阈值（判据）：20% 黄、10% 红。 */
export const HEALTH_THRESHOLDS = { warn: 20, critical: 10 } as const;

export type HealthLevel = "ok" | "warn" | "critical" | "unknown";

export interface MediaHealth {
  /** 介质标识（多介质分列的键）。 */
  mediaId: string;
  label: string;
  /** 读到健康数据的介质才有下列字段（null=读不到——诚实分支）。 */
  lifePctRemaining: number | null;
  /** 累计写入量（TB，对账 IO 计数）。 */
  writtenTb: number | null;
  temperatureC: number | null;
  /** 坏块重映射计数。 */
  remappedBlocks: number | null;
}

/** 健康级别判定（读不到=unknown，绝不编数——判据「读得到/读不到两分支」）。 */
export function healthLevel(m: MediaHealth): HealthLevel {
  if (m.lifePctRemaining === null) return "unknown";
  const p = Math.min(100, Math.max(0, m.lifePctRemaining));
  if (p < HEALTH_THRESHOLDS.critical) return "critical";
  if (p < HEALTH_THRESHOLDS.warn) return "warn";
  return "ok";
}

export interface HealthNotice {
  level: Exclude<HealthLevel, "ok" | "unknown">;
  message: string;
  /** 出路链接（判据「两阈值提示与出路链接」）：迁移建议 / F396 备份直达。 */
  action: { label: string; target: "backup-wizard" | "migration-guide" };
}

/** 阈值提示与出路（判据）：warn→迁移建议；critical→立即备份（F396 直达）。 */
export function noticeFor(m: MediaHealth): HealthNotice | null {
  const level = healthLevel(m);
  if (level === "warn") {
    return { level, message: `「${m.label}」剩余寿命 ${m.lifePctRemaining}%——建议尽快迁移重要数据`, action: { label: "查看迁移指南", target: "migration-guide" } };
  }
  if (level === "critical") {
    return { level, message: `「${m.label}」剩余寿命仅 ${m.lifePctRemaining}%——强烈建议立即备份`, action: { label: "打开备份向导", target: "backup-wizard" } };
  }
  return null;
}

/** 诚实降级文案（判据「不编数」）：读不到时的页面显示。 */
export function unknownHealthMessage(m: MediaHealth): string {
  return healthLevel(m) === "unknown" ? `「${m.label}」该介质不提供健康数据` : "";
}

/** 写入量累计对账（判据「对账 IO 计数」）：页面值 vs IO 计数器，误差 ≤2% 合格。 */
export function auditWriteAccounting(pageTb: number, ioCounterTb: number): { pass: boolean; deltaPct: number } {
  if (ioCounterTb === 0) return { pass: pageTb === 0, deltaPct: pageTb === 0 ? 0 : Number.POSITIVE_INFINITY };
  const delta = Math.abs(pageTb - ioCounterTb) / ioCounterTb * 100;
  return { pass: delta <= 2, deltaPct: delta };
}

/** 多介质分列（判据 S: 共享卷）：按 mediaId 分列，各列独立判级。 */
export function columnsFor(medias: MediaHealth[]): Array<{ mediaId: string; label: string; level: HealthLevel }> {
  return medias.map((m) => ({ mediaId: m.mediaId, label: m.label, level: healthLevel(m) }));
}

/* ---------- 累计账持久化（写入量跨会话累计） ---------- */

const KEY = h4Key("f395", "written");

type WrittenLedger = Record<string, number>;

function isLedger(v: unknown): v is WrittenLedger {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

export function accumulateWritten(mediaId: string, deltaTb: number, store: KvStore = defaultStore()): { ok: boolean; total: number } {
  const ledger = readJson<WrittenLedger>(store, KEY, {}, isLedger);
  const total = (ledger[mediaId] ?? 0) + Math.max(0, deltaTb);
  ledger[mediaId] = total;
  return { ok: writeJson(store, KEY, ledger), total };
}

export function writtenTotal(mediaId: string, store: KvStore = defaultStore()): number {
  return readJson<WrittenLedger>(store, KEY, {}, isLedger)[mediaId] ?? 0;
}
