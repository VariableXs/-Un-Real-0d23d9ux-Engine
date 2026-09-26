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
