/**
 * F375 H 域总判据（H 域 · AI-H4）：
 * H 域 200 项（F201-F400）的收官判据：全部条目的验收锚点汇入全域总检（F200 扩展），
 * 另加三域专项走查——①一致性走查（同一交互全系统行为一致，对照表逐项过）；
 * ②手感走查（Windows 迁移用户 15 分钟无引导完成 20 个日常任务）；
 * ③降级走查（性能三档/低电量/减少动效三态下全功能可用）。
 * 判据（主册 F375）：三走查各出报告；一致性对照表 100% 绿；15 分钟任务链无卡壳；
 * 走查不过的条目回炉不发布。
 * 本模块为走查引擎：输入各项检查点结果，产出三份走查报告与发布判定。
 */

import type { KvStore } from "./internal/store";
import { defaultStore, h4Key, readJson, writeJson } from "./internal/store";

export interface Checkpoint {
  /** F 编号。 */
  item: string;
  /** 检查点名。 */
  name: string;
  passed: boolean;
  /** 证据引用（数据/录屏/日期——判据「报告三件入账」）。 */
  evidence: string;
}

export interface ConsistencyRow {
  /** 交互名（如「Esc 关闭浮层」）。 */
  interaction: string;
  /** 代表界面清单。 */
  surfaces: string[];
  /** 各面行为一致（判据：对照表逐项过）。 */
  consistent: boolean;
}

export interface TaskChainStep {
  task: string;
  /** 完成耗时（秒）。 */
  seconds: number;
  /** 是否卡壳（卡壳点回炉判据）。 */
  stuck: boolean;
}

export type DegradedState = "perf-low" | "battery-low" | "reduce-motion";

export interface WalkthroughReports {
  consistency: { rows: ConsistencyRow[]; pass: boolean };
  taskChain: { steps: TaskChainStep[]; totalSeconds: number; pass: boolean; budgetSeconds: number };
  degraded: { states: Array<{ state: DegradedState; allFunctional: boolean }>; pass: boolean };
}

export const TASK_CHAIN_BUDGET_SECONDS = 15 * 60;
export const TASK_CHAIN_REQUIRED_STEPS = 20;

/** ① 一致性走查：对照表 100% 绿才过（判据）。 */
export function consistencyWalkthrough(rows: ConsistencyRow[]): WalkthroughReports["consistency"] {
  return { rows, pass: rows.length > 0 && rows.every((r) => r.consistent) };
}

/** ② 手感走查：20 任务链、总时长 ≤15 分钟、零卡壳（判据）。 */
export function taskChainWalkthrough(steps: TaskChainStep[]): WalkthroughReports["taskChain"] {
  const total = steps.reduce((s, x) => s + x.seconds, 0);
  return {
    steps,
    totalSeconds: total,
    pass: steps.length >= TASK_CHAIN_REQUIRED_STEPS && total <= TASK_CHAIN_BUDGET_SECONDS && steps.every((s) => !s.stuck),
    budgetSeconds: TASK_CHAIN_BUDGET_SECONDS,
  };
}

/** ③ 降级走查：三态下全功能可用（判据）。 */
export function degradedWalkthrough(results: Array<{ state: DegradedState; allFunctional: boolean }>): WalkthroughReports["degraded"] {
  const required: DegradedState[] = ["perf-low", "battery-low", "reduce-motion"];
  const states = required.map((r) => results.find((x) => x.state === r) ?? { state: r, allFunctional: false });
  return { states, pass: states.every((s) => s.allFunctional) };
}

export interface ReleaseDecision {
  /** 总检查点全绿（锚点汇入全域总检判据）。 */
  checkpointsPass: boolean;
  /** 三走查全过。 */
  walkthroughs: WalkthroughReports;
  /** 发布判定：任何一项红 = 回炉不发布（判据）。 */
  release: boolean;
  /** 回炉清单（未过的检查点——「走查不过的条目回炉」）。 */
  rework: Checkpoint[];
}

/** 总判定（F375 的收官函数）。 */
export function releaseDecision(checkpoints: Checkpoint[], reports: WalkthroughReports): ReleaseDecision {
  const rework = checkpoints.filter((c) => !c.passed);
  return {
    checkpointsPass: rework.length === 0,
    walkthroughs: reports,
    release: rework.length === 0 && reports.consistency.pass && reports.taskChain.pass && reports.degraded.pass,
    rework,
  };
}

/* ---------- 报告归档（数据+录屏+日期入账——判据「三走查各出报告」） ---------- */

export interface ArchivedReport {
  at: string;
  decision: ReleaseDecision;
}

const KEY = h4Key("f375", "reports");

/** 报告归档（ISO 日期入账；留最近 12 份）。 */
export function archiveReport(decision: ReleaseDecision, nowIso: string, store: KvStore = defaultStore()): boolean {
  const all = readJson<ArchivedReport[]>(store, KEY, [], Array.isArray);
  return writeJson(store, KEY, [...all.slice(-11), { at: nowIso, decision }]);
}

export function archivedReports(store: KvStore = defaultStore()): ArchivedReport[] {
  return readJson<ArchivedReport[]>(store, KEY, [], Array.isArray);
}

/* ================= v5 深化批次五：回归对比 / 豁免登记 / 人话摘要 ================= */

export interface GateDiff {
  newlyRed: string[];
  newlyGreen: string[];
  stillRed: string[];
  /** 回归方向：true = 比上次差（新红出现）——回归即点名。 */
  regressed: boolean;
}

/** 两次门禁回归对比：检查点逐项对齐，新增红/转绿/持续红三清单（改进是否有回潮一目了然）。 */
export function diffReports(prev: Checkpoint[], curr: Checkpoint[]): GateDiff {
  const prevMap = new Map(prev.map((c) => [`${c.item}/${c.name}`, c.passed]));
  const currMap = new Map(curr.map((c) => [`${c.item}/${c.name}`, c.passed]));
  const newlyRed = [...currMap].filter(([k, p]) => p === false && prevMap.get(k) === true).map(([k]) => k);
  const newlyGreen = [...currMap].filter(([k, p]) => p === true && prevMap.get(k) === false).map(([k]) => k);
  // stillRed = 当前红且并非「上次绿转红」——含上次就红与新增检查点即红（未解决项全在此列）
  const stillRed = [...currMap].filter(([k, p]) => p === false && prevMap.get(k) !== true).map(([k]) => k).filter((k) => !newlyRed.includes(k));
  return { newlyRed, newlyGreen, stillRed, regressed: newlyRed.length > 0 };
}

export interface Exemption {
  item: string;
  name: string;
  reason: string;
  /** 豁免期限（ISO 日期）——过期自动失效回红（豁免不是永久牌）。 */
  untilIso: string;
}

/** 豁免判定：未过期且有理由才生效；过期豁免如实失效（不静默续期）。 */
export function exemptionActive(ex: Exemption, nowIso: string): { active: boolean; reason: string } {
  if (!ex.reason.trim()) return { active: false, reason: "豁免缺理由——无效" };
  if (ex.untilIso < nowIso) return { active: false, reason: "豁免已过期——检查点恢复计红" };
  return { active: true, reason: `豁免至 ${ex.untilIso}（${ex.reason}）` };
}

/** 门禁人话摘要（报告的第一行）：三走查 + 检查点 → 一句话结论。 */
export function walkthroughDigest(d: ReleaseDecision): string {
  const parts = [
    `检查点 ${d.checkpointsPass ? "全绿" : `${d.rework.length} 项未过`}`,
    `一致性 ${d.walkthroughs.consistency.pass ? "过" : "红"}`,
    `任务链 ${d.walkthroughs.taskChain.pass ? `${d.walkthroughs.taskChain.totalSeconds}s 过` : "红"}`,
    `降级 ${d.walkthroughs.degraded.pass ? "过" : "红"}`,
  ];
  return `${d.release ? "【可发布】" : "【回炉】"}${parts.join(" · ")}`;
}
