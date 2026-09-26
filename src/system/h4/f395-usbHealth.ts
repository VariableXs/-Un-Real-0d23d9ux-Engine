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

/* ================= v4 深化批次四：磨损模型 / 寿命外推 / 建议清单 / 多介质总览 / 趋势 ================= */

/** 磨损模型：写入量 vs 设计寿命写入量（TBW）→ 已耗百分比——「读不到不编数」同样适用：缺 TBW = 不外推。 */
export function wearModel(m: MediaHealth, designTbwTb: number | null): { consumedPct: number | null; consumedTb: number | null } {
  if (m.writtenTb === null || designTbwTb === null || designTbwTb <= 0) return { consumedPct: null, consumedTb: m.writtenTb };
  return { consumedPct: Math.min(100, Math.round((m.writtenTb / designTbwTb) * 1000) / 10), consumedTb: m.writtenTb };
}

/** 寿命外推：按当前日均写入量推到阈值的天数（推不出 = 如实 null，绝不编数）。 */
export function projectionDaysToThreshold(m: MediaHealth, dailyWriteTb: number, thresholdPct: "warn" | "critical", designTbwTb: number | null): number | null {
  if (m.lifePctRemaining === null || designTbwTb === null || designTbwTb <= 0 || dailyWriteTb <= 0) return null;
  const threshold = HEALTH_THRESHOLDS[thresholdPct];
  const remainingLifeTb = (m.lifePctRemaining / 100) * designTbwTb;
  const targetRemainingTb = (threshold / 100) * designTbwTb;
  const tbToThreshold = remainingLifeTb - targetRemainingTb;
  return tbToThreshold <= 0 ? 0 : Math.floor(tbToThreshold / dailyWriteTb);
}

/** 建议清单（出路链接的行动面）：级别 → 具体动作（每条可执行、有去处）。 */
export function advisorActions(level: HealthLevel): Array<{ action: string; target: string | null }> {
  switch (level) {
    case "ok":
      return [{ action: "健康状态良好——无需处理", target: null }];
    case "warn":
      return [
        { action: "迁移重要数据到新介质", target: "migration-guide" },
        { action: "做一次完整备份（F396）", target: "backup-wizard" },
      ];
    case "critical":
      return [
        { action: "立即完整备份（F396 向导直达）", target: "backup-wizard" },
        { action: "备份完成后停止向此介质写入", target: null },
        { action: "准备替换介质", target: "migration-guide" },
      ];
    case "unknown":
      return [{ action: "该介质不提供健康数据——建议依赖备份制度而非健康监控", target: "backup-wizard" }];
  }
}

/** 多介质总览：最差级别决定页面顶色（一眼看到最需要关心的那块盘）。 */
export function multiMediaSummary(medias: MediaHealth[]): { worst: HealthLevel; counts: Record<HealthLevel, number> } {
  const counts: Record<HealthLevel, number> = { ok: 0, warn: 0, critical: 0, unknown: 0 };
  const rank: Record<HealthLevel, number> = { ok: 0, unknown: 1, warn: 2, critical: 3 };
  let worst: HealthLevel = "ok";
  for (const m of medias) {
    const lv = healthLevel(m);
    counts[lv]++;
    if (rank[lv] > rank[worst]) worst = lv;
  }
  return { worst, counts };
}

/** 寿命趋势：{at, lifePct} 采样序列 → 斜率（%/天）——外推的实测修正面。 */
export function lifeTrend(samples: Array<{ at: number; lifePct: number }>): { slopePerDay: number | null; samples: number } {
  if (samples.length < 2) return { slopePerDay: null, samples: samples.length };
  const first = samples[0]!;
  const last = samples[samples.length - 1]!;
  const days = (last.at - first.at) / (24 * 3600 * 1000);
  if (days <= 0) return { slopePerDay: null, samples: samples.length };
  return { slopePerDay: Math.round(((last.lifePct - first.lifePct) / days) * 1000) / 1000, samples: samples.length };
}
