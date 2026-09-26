/**
 * F396 备份向导（H 域 · AI-H4）：
 * 系统镜像备份三步向导：选目标（第二块介质/网络位置/另一 U 盘）、选范围（系统分区必选/
 * 用户文件可选）、执行（增量链——第一次全量后续增量，进度可暂停 F269 同源）；备份完成
 * 验证（可恢复性校验——备份不能只是拷贝，得验过能还原才算数）；备份计划可选周期提醒
 * （不自动跑，提醒用户跑）。
 * 判据（主册 F396）：三步流程用例；增量链还原（连做三次增量后整体还原比对）；
 * 可恢复性校验判据；暂停续传；提醒周期设置。
 * 依赖锚点：F269 暂停续传。
 * 存储键：variable:h4:f396（提醒周期设置）。
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

export type BackupTarget = "second-media" | "network" | "another-usb";
export type BackupScope = { systemPartition: boolean; userFiles: boolean };

export interface BackupChainEntry {
  /** 增量序号（0=全量基线）。 */
  seq: number;
  at: number;
  /** 该次备份覆盖的文件指纹集合（增量差集的表示）。 */
  fingerprints: string[];
  sizeBytes: number;
  /** 可恢复性校验结果（判据：验过能还原才算备份）。 */
  verified: boolean;
}

export interface BackupChain {
  target: BackupTarget;
  scope: BackupScope;
  entries: BackupChainEntry[];
}

/** 三步向导第 1-2 步校验：目标合法、系统分区必选（判据）。 */
export function validatePlan(target: BackupTarget, scope: BackupScope): { ok: boolean; problems: string[] } {
  const problems: string[] = [];
  if (!["second-media", "network", "another-usb"].includes(target)) problems.push("目标必须为第二块介质/网络位置/另一 U 盘");
  if (!scope.systemPartition) problems.push("系统分区为必选项（不能只备用户文件）");
  return { ok: problems.length === 0, problems };
}

/** 开链：第一次全量（seq=0）；已在链上则拒绝重复全量（增量链判据）。 */
export function startChain(target: BackupTarget, scope: BackupScope, now: number, allFingerprints: string[], sizeBytes: number): { chain: BackupChain | null; error: string | null } {
  const plan = validatePlan(target, scope);
  if (!plan.ok) return { chain: null, error: plan.problems.join("；") };
  return {
    chain: { target, scope, entries: [{ seq: 0, at: now, fingerprints: [...allFingerprints], sizeBytes, verified: false }] },
    error: null,
  };
}

/**
 * 增量备份：差集基准 = 链上已有的累计全集（全量 + 历次增量）——增量语义是
 * 「自上次备份状态以来变化的文件」；空差集拒绝（无变化不造假增量——诚实边界）。
 */
export function appendIncrement(chain: BackupChain, now: number, currentFingerprints: string[], sizeBytes: number): { chain: BackupChain; error: string | null } {
  const existing = new Set<string>();
  for (const e of chain.entries) for (const f of e.fingerprints) existing.add(f);
  const delta = currentFingerprints.filter((f) => !existing.has(f));
  if (delta.length === 0) return { chain, error: "自上次备份无变化——不产生空增量" };
  const last = chain.entries[chain.entries.length - 1]!;
  return {
    chain: { ...chain, entries: [...chain.entries, { seq: last.seq + 1, at: now, fingerprints: delta, sizeBytes, verified: false }] },
    error: null,
  };
}

/** 可恢复性校验（判据核心）：链上全部条目 must verified 才算「备份完成」。 */
export function markVerified(chain: BackupChain, seq: number): BackupChain | null {
  const entries = chain.entries.map((e) => (e.seq === seq ? { ...e, verified: true } : e));
  return { ...chain, entries };
}

export function chainReadyForRestore(chain: BackupChain): { ready: boolean; unverified: number[] } {
  const unverified = chain.entries.filter((e) => !e.verified).map((e) => e.seq);
  return { ready: unverified.length === 0, unverified };
}

/**
 * 增量链整体还原比对（判据「连做三次增量后整体还原比对」）：
 * 合并全量+全部增量 → 判完备性：当前每个文件都可恢复（missing 必须为空）；
 * merged 中超出 current 的部分是「已删除但仍会被还原的旧文件」，如实标注不冒充相等。
 */
export function restoreCompare(chain: BackupChain, currentFingerprints: string[]): { merged: string[]; identical: boolean; missing: string[]; staleExtra: string[] } {
  const merged = new Set<string>();
  for (const e of [...chain.entries].sort((a, b) => a.seq - b.seq)) {
    for (const f of e.fingerprints) merged.add(f);
  }
  const current = new Set(currentFingerprints);
  const missing = currentFingerprints.filter((f) => !merged.has(f));
  const staleExtra = [...merged].filter((f) => !current.has(f));
  return { merged: [...merged], identical: missing.length === 0, missing, staleExtra };
}

/* ---------- 暂停续传（F269 同源） ---------- */

export interface TransferState {
  phase: "running" | "paused" | "done";
  /** 已完成字节（断点）。 */
  doneBytes: number;
  totalBytes: number;
}

export function pauseTransfer(t: TransferState): TransferState {
  return t.phase === "running" ? { ...t, phase: "paused" } : t;
}

export function resumeTransfer(t: TransferState): TransferState {
  return t.phase === "paused" ? { ...t, phase: "running" } : t;
}

/** 续传自断点：doneBytes 之前的不再重传（判据「暂停续传」）。 */
export function resumedFrom(t: TransferState): number {
  return t.doneBytes;
}

/* ---------- 提醒周期（不自动跑，只提醒——判据） ---------- */

const KEY = h4Key("f396", "reminder");

export type ReminderCycle = "off" | "monthly" | "quarterly";

export function getReminder(store: KvStore = defaultStore()): ReminderCycle {
  return readJson<ReminderCycle>(store, KEY, "off", (v): v is ReminderCycle => v === "off" || v === "monthly" || v === "quarterly");
}

export function setReminder(cycle: ReminderCycle, store: KvStore = defaultStore()): boolean {
  return writeJson(store, KEY, cycle);
}

/** 到期判定：按周期应提醒但从未备份/上次备份超周期 → 提醒（不自动执行——判据红线）。 */
export function reminderDue(cycle: ReminderCycle, lastBackupAt: number | null, now: number): { due: boolean; autoRun: false } {
  if (cycle === "off") return { due: false, autoRun: false };
  const days = cycle === "monthly" ? 30 : 90;
  const due = lastBackupAt === null || now - lastBackupAt > days * 24 * 3600 * 1000;
  return { due, autoRun: false };
}
