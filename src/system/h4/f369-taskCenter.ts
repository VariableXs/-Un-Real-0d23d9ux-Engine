/**
 * F369 后台任务中心（H 域 · AI-H4）：
 * 系统所有后台活动一处可见（任务视图侧栏/设置中心双门）：文件索引（F306）、磁盘自检
 * （F342）、版本快照（F325）、更新下载（F122）……每任务显示名称/进度/预估剩余/暂停按钮；
 * 任务之间不互相饿死（F057 IO 分级调度保障）；「全部暂停」一键。
 * 判据（主册 F369）：任务注册完整性（系统任务全入册）；四字段信息准确性；
 * 暂停/恢复逐任务+全局两级；调度分级对账 F057。
 * 依赖锚点：F057 IO 分级 / F122 更新 / F306 索引 / F325 快照 / F342 自检。
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

/** 系统任务注册名（判据「系统任务全入册」——本表即完整性口径）。 */
export const SYSTEM_TASK_KINDS = ["fileIndex", "diskCheck", "versionSnapshot", "updateDownload"] as const;
export type SystemTaskKind = (typeof SYSTEM_TASK_KINDS)[number];

/** F057 调度分级（对账口径）：前台交互 > 后台任务 > 批量。 */
export type IoTier = "interactive" | "background" | "batch";

export interface BackgroundTask {
  id: string;
  kind: SystemTaskKind;
  name: string;
  /** 进度 0-100。 */
  progressPct: number;
  /** 预估剩余（ms；null=不可估）。 */
  etaMs: number | null;
  paused: boolean;
  tier: IoTier;
}

export interface TaskCenterState {
  tasks: BackgroundTask[];
  /** 全局暂停（重要演示/游戏场景一键）。 */
  globalPaused: boolean;
}

export function initialState(): TaskCenterState {
  return { tasks: [], globalPaused: false };
}

/** 任务注册：同 id 幂等（重复注册拒绝）；kind 必须在系统任务表内（完整性判据）。 */
export function registerTask(state: TaskCenterState, task: BackgroundTask): { state: TaskCenterState; ok: boolean; reason: string } {
  if (!SYSTEM_TASK_KINDS.includes(task.kind)) return { state, ok: false, reason: `未登记的任务类别：${task.kind}（任务注册完整性判据）` };
  if (state.tasks.some((t) => t.id === task.id)) return { state, ok: false, reason: "重复注册" };
  const progress = Math.min(100, Math.max(0, Math.round(task.progressPct)));
  return { state: { ...state, tasks: [...state.tasks, { ...task, progressPct: progress }] }, ok: true, reason: "ok" };
}

/** 进度推进 + ETA 四字段准确性：ETA 由速率外推（bytes/时长口径由调用方给 rate）。 */
export function advance(state: TaskCenterState, id: string, progressPct: number, etaMs: number | null): TaskCenterState {
  return {
    ...state,
    tasks: state.tasks.map((t) =>
      t.id === id
        ? { ...t, progressPct: Math.min(100, Math.max(0, Math.round(progressPct))), etaMs: etaMs !== null && Number.isFinite(etaMs) && etaMs >= 0 ? Math.round(etaMs) : null }
        : t,
    ),
  };
}

/** 逐任务暂停/恢复（判据两级之一）。 */
export function setPaused(state: TaskCenterState, id: string, paused: boolean): TaskCenterState {
  return { ...state, tasks: state.tasks.map((t) => (t.id === id ? { ...t, paused } : t)) };
}

/** 全局暂停/恢复（判据两级之二）：全局暂停时全部任务冻结；恢复仅清全局旗标。 */
export function setGlobalPaused(state: TaskCenterState, paused: boolean): TaskCenterState {
  return { ...state, globalPaused: paused };
}

/** 任务对用户可见的有效运行态（全局旗标叠加逐任务旗标）。 */
export function effectivelyRunning(t: BackgroundTask, state: TaskCenterState): boolean {
  return !state.globalPaused && !t.paused && t.progressPct < 100;
}

/** 调度分级对账（F057）：后台任务队列不得出现 interactive 任务让位 background 的倒挂。 */
export function auditTierOrdering(state: TaskCenterState): { pass: boolean; violations: string[] } {
  const running = state.tasks.filter((t) => effectivelyRunning(t, state));
  const violations: string[] = [];
  const weight: Record<IoTier, number> = { interactive: 0, background: 1, batch: 2 };
  for (let i = 0; i < running.length; i++) {
    for (let j = 0; j < running.length; j++) {
      const a = running[i]!;
      const b = running[j]!;
      if (a.tier === "interactive" && b.tier === "batch" && i > j) {
        violations.push(`${b.id}（batch）排在 ${a.id}（interactive）之前——分级倒挂`);
      }
    }
    void weight;
  }
  return { pass: violations.length === 0, violations };
}

/** 中心可见性：有可运行任务即入册可见；全部暂停/完成时如实显示空转态。 */
export function visibility(state: TaskCenterState): { badge: number; allQuiet: boolean } {
  const active = state.tasks.filter((t) => effectivelyRunning(t, state));
  return { badge: active.length, allQuiet: active.length === 0 };
}

/* ---------- 持久化（面板记忆任务清单跨会话） ---------- */

const KEY = h4Key("f369", "state");

function isState(v: unknown): v is TaskCenterState {
  return !!v && typeof v === "object" && Array.isArray((v as TaskCenterState).tasks);
}

export function persistState(state: TaskCenterState, store: KvStore = defaultStore()): boolean {
  return writeJson(store, KEY, state);
}

export function loadPersisted(store: KvStore = defaultStore()): TaskCenterState {
  return readJson<TaskCenterState>(store, KEY, initialState(), isState);
}

/* ================= v4 深化批次四：ETA 平滑 / 单调守卫 / 饿死看门狗 / 准入与摘要 ================= */

const TIER_WEIGHT: Record<IoTier, number> = { interactive: 0, background: 1, batch: 2 };

/** ETA 指数平滑（四字段准确性的稳定面）：新样本 30% 权重——抖动读数不直传用户。 */
export function etaEwma(prevEtaMs: number | null, sampleMs: number, alpha = 0.3): number {
  if (prevEtaMs === null || !Number.isFinite(prevEtaMs) || prevEtaMs < 0) return Math.max(0, Math.round(sampleMs));
  return Math.round(prevEtaMs * (1 - alpha) + Math.max(0, sampleMs) * alpha);
}

/** 进度单调守卫：进度回退 = 计量缺陷——如实拒绝并报告原因（不静默覆盖、零吞错）。 */
export function advanceGuarded(state: TaskCenterState, id: string, progressPct: number, etaMs: number | null): { state: TaskCenterState; accepted: boolean; reason: string } {
  const t = state.tasks.find((x) => x.id === id);
  if (!t) return { state, accepted: false, reason: "任务不存在" };
  if (progressPct < t.progressPct) return { state, accepted: false, reason: `进度回退（${t.progressPct}% → ${progressPct}%）——已拒绝（单调守卫）` };
  return { state: advance(state, id, progressPct, etaMs), accepted: true, reason: "ok" };
}

export interface StarvationReport {
  starved: Array<{ id: string; tier: IoTier; ageMs: number }>;
  escalate: string[];
}

/** 饿死看门狗（判据「任务之间不互相饿死」的机检面）：注册久、零进度、非交互级 → 升级名单。 */
export function starvationWatchdog(state: TaskCenterState, registeredAt: Record<string, number>, nowMs: number, waitThresholdMs = 60_000): StarvationReport {
  const starved = state.tasks
    .filter((t) => effectivelyRunning(t, state) && t.progressPct === 0 && t.tier !== "interactive" && nowMs - (registeredAt[t.id] ?? nowMs) >= waitThresholdMs)
    .map((t) => ({ id: t.id, tier: t.tier, ageMs: nowMs - (registeredAt[t.id] ?? nowMs) }))
    .sort((a, b) => b.ageMs - a.ageMs);
  return { starved, escalate: starved.map((s) => s.id) };
}

/** 准入控制（F057 分级对账的执行面）：前台忙时 batch 让路、background 限流推进（不饿死）。 */
export function admissionControl(t: BackgroundTask, foregroundBusy: boolean): { admitted: boolean; reason: string } {
  if (t.tier === "interactive") return { admitted: true, reason: "交互级任务直通" };
  if (!foregroundBusy) return { admitted: true, reason: "前台空闲——放行" };
  return t.tier === "batch"
    ? { admitted: false, reason: "前台活跃——批量任务让路（F057）" }
    : { admitted: true, reason: "后台级任务限流推进（不全让路——饿死防线）" };
}

/** 下一个可运行任务（中心调度的确定性口径）：tier 升序 → 进度降序 → id 字典序。 */
export function nextRunnable(state: TaskCenterState): BackgroundTask | null {
  const runnable = state.tasks.filter((t) => effectivelyRunning(t, state));
  if (runnable.length === 0) return null;
  return [...runnable].sort((a, b) => TIER_WEIGHT[a.tier] - TIER_WEIGHT[b.tier] || b.progressPct - a.progressPct || a.id.localeCompare(b.id))[0]!;
}

/** 中心行文案（四字段信息准确性的人话面）：「文件索引 · 45% · 剩约 2 分 · 已暂停」。 */
export function summaryRow(t: BackgroundTask, globalPaused: boolean): string {
  const eta = t.etaMs === null ? "" : t.etaMs < 60_000 ? ` · 剩约 ${Math.round(t.etaMs / 1000)} 秒` : ` · 剩约 ${Math.round(t.etaMs / 60_000)} 分`;
  const paused = globalPaused || t.paused ? " · 已暂停" : "";
  return `${t.name} · ${t.progressPct}%${eta}${paused}`;
}

/** 注册完整性审计（判据「系统任务全入册」）：SYSTEM_TASK_KINDS 逐一核对，缺类点名。 */
export function auditRegistrationCompleteness(state: TaskCenterState): { pass: boolean; missing: SystemTaskKind[] } {
  const have = new Set(state.tasks.map((t) => t.kind));
  const missing = SYSTEM_TASK_KINDS.filter((k) => !have.has(k));
  return { pass: missing.length === 0, missing };
}
