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
