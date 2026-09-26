/**
 * F397 还原演练（H 域 · AI-H4）：
 * 恢复环境（F198）的日常演练制度：设置中心每季度提醒一次「花 5 分钟走一遍恢复流程」
 * （引导式：进恢复环境→看三卡→模拟选一次还原点→退出不真还原）；演练完成记录在
 * 「关于」页（上次演练日期）；从未演练过的系统在恢复时显示加练提示。
 * 判据（主册 F397）：季度提醒周期；引导步骤与 F198 三卡联动；模拟零副作用判据；
 * 演练记录；首次恢复加练提示。
 * 依赖锚点：F198 恢复环境三卡。
 * 存储键：variable:h4:f397（演练记录）。
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

/** 季度提醒周期（判据 90 天）。 */
export const DRILL_CYCLE_DAYS = 90;
/** 演练引导步骤（与 F198 三卡联动——步骤表即口径源）。 */
export const DRILL_STEPS = [
  { id: "enter-recovery", card: "card-1-引导", label: "进入恢复环境" },
  { id: "view-cards", card: "card-2-三卡", label: "看完三卡（备份/还原/修复）" },
  { id: "mock-restore-point", card: "card-3-还原", label: "模拟选一次还原点" },
  { id: "exit-noop", card: null, label: "退出（不真还原）" },
] as const;

export interface DrillRecord {
  at: number;
  /** 全步骤走完 = 演练有效（半途退出不算）。 */
  completedSteps: string[];
  /** 模拟零副作用自证：演练期间产生的写操作数（必须为 0——判据）。 */
  sideEffectWrites: number;
}

const KEY = h4Key("f397", "records");

function isRecords(v: unknown): v is DrillRecord[] {
  return Array.isArray(v) && v.every((r) => r && typeof (r as DrillRecord).at === "number");
}

export function loadRecords(store: KvStore = defaultStore()): DrillRecord[] {
  return readJson<DrillRecord[]>(store, KEY, [], isRecords);
}

/** 记录一次演练（留最近 20 条；零副作用演练才入账——判据「模拟零副作用」）。 */
export function recordDrill(rec: DrillRecord, store: KvStore = defaultStore()): { ok: boolean; reason: string } {
  if (rec.sideEffectWrites !== 0) return { ok: false, reason: "演练产生写操作——零副作用判据不通过，不入账" };
  if (rec.completedSteps.length !== DRILL_STEPS.length) return { ok: false, reason: "演练未走完全部引导步骤" };
  const all = loadRecords(store);
  return { ok: writeJson(store, KEY, [...all.slice(-19), rec]), reason: "ok" };
}

/** 季度提醒到期判定（判据「季度提醒周期」）。 */
export function drillReminderDue(now: number, store: KvStore = defaultStore()): { due: boolean; lastDrillAt: number | null; neverDrilled: boolean } {
  const all = loadRecords(store);
  const last = all.length ? all[all.length - 1]!.at : null;
  return {
    due: last === null || now - last > DRILL_CYCLE_DAYS * 24 * 3600 * 1000,
    lastDrillAt: last,
    neverDrilled: last === null,
  };
}

/** 首次恢复加练提示（判据）：从未演练 → 恢复时给引导加练文案。 */
export function firstRestoreNotice(store: KvStore = defaultStore()): string | null {
  return drillReminderDue(Date.now(), store).neverDrilled
    ? "你还没演练过恢复流程——按引导慢慢来，每一步都有说明"
    : null;
}

/** 模拟零副作用审计（判据）：演练会话的写操作账必须为 0（真还原才会 >0）。 */
export function auditZeroSideEffect(sessionWrites: string[]): { pass: boolean; writes: number } {
  return { pass: sessionWrites.length === 0, writes: sessionWrites.length };
}

/** 引导步骤联动校验（判据「与 F198 三卡联动」）：每个步骤必须挂到对应卡或显式为收尾。 */
export function auditStepCardLinkage(): { pass: boolean; unlinked: string[] } {
  const loose: ReadonlyArray<{ id: string; card: string | null }> = DRILL_STEPS;
  const unlinked = loose.filter((s) => s.card === null && s.id !== "exit-noop").map((s) => s.id);
  return { pass: unlinked.length === 0, unlinked };
}

/** 演练「关于」页视图（判据「演练记录在关于页」）。 */
export function aboutPageView(store: KvStore = defaultStore()): { lastDrillAt: number | null; totalDrills: number; nextReminderAt: number | null } {
  const all = loadRecords(store);
  const last = all.length ? all[all.length - 1]!.at : null;
  return {
    lastDrillAt: last,
    totalDrills: all.length,
    nextReminderAt: last === null ? null : last + DRILL_CYCLE_DAYS * 24 * 3600 * 1000,
  };
}

/* ================= v4 深化批次四：调度数学 / 连续统计 / 加练排程 / 沙盒白名单 ================= */

/** 下次提醒时刻（纯函数面）：从未演练 = 立即到期；否则上次 + 90 天（时区无关的账面口径）。 */
export function nextDrillDueAt(lastDrillAt: number | null, now: number): number | null {
  return lastDrillAt === null ? now : lastDrillAt + DRILL_CYCLE_DAYS * 24 * 3600 * 1000;
}

export interface DrillStats {
  total: number;
  /** 连续有效次数（从最新往回数全步骤、零副作用的记录）。 */
  validStreak: number;
  lastAt: number | null;
  avgSteps: number;
}

/** 演练统计：制度执行度的量化面（连续性比总量更能说明习惯是否养成）。 */
export function drillStats(records: DrillRecord[]): DrillStats {
  let streak = 0;
  for (const r of [...records].reverse()) {
    if (r.completedSteps.length === DRILL_STEPS.length && r.sideEffectWrites === 0) streak++;
    else break;
  }
  const avgSteps = records.length === 0 ? 0 : Math.round((records.reduce((s, r) => s + r.completedSteps.length, 0) / records.length) * 10) / 10;
  return { total: records.length, validStreak: streak, lastAt: records.length ? records[records.length - 1]!.at : null, avgSteps };
}

/** 加练窗口（判据「首次恢复加练提示」）：首次真恢复后 7 天内补一次演练。 */
export const EXTRA_DRILL_WINDOW_DAYS = 7;

export function extraDrillSchedule(records: DrillRecord[], restoredAt: number): { due: boolean; deadline: number; reason: string } {
  const drilledBefore = records.some((r) => r.at < restoredAt);
  if (drilledBefore) return { due: false, deadline: 0, reason: "恢复前已演练过——无需加练" };
  return { due: true, deadline: restoredAt + EXTRA_DRILL_WINDOW_DAYS * 24 * 3600 * 1000, reason: "首次真恢复——7 天内加练一次（判据）" };
}

/** 演练沙盒白名单（零副作用判据的路径级执法）：白名单外写路径 = 违规。 */
export const DRILL_SANDBOX_PREFIXES = ["S:/Varix/drill/", "memory://"] as const;

export function auditSandboxWrites(writePaths: string[]): { pass: boolean; violations: string[] } {
  const violations = writePaths.filter((p) => !DRILL_SANDBOX_PREFIXES.some((pre) => p.startsWith(pre)));
  return { pass: violations.length === 0, violations };
}
