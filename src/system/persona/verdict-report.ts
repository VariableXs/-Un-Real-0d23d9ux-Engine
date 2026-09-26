/**
 * F170 域总检深化 · 报告渲染 + 跨轮回归对比 + 抖动检测 + 证据完整性。
 *
 * 主册判据延伸：
 * - F170「证据链完整率 100%」「脚本全量执行 <30 分钟（可进 CI 周跑）」——
 *   CI 周跑需要机器可读产物：本轮 DomainVerdict → Markdown 报告 +
 *   JSON 证据包；连续两轮可对比回归（新红项 = 回归，连续绿 = 稳定）；
 * - 「抖动检测」：多轮结果中红绿交替的项标 flaky（间歇性失败不是稳定
 *   通过——诚实口径不把抖动算绿）；
 * - 证据完整性：每项三步证据（mutate/take-effect/rollback）缺一即不完整
 *   ——导出前门禁（verdict.ts evidenceDocComplete 的深化版：带缺什么的人话）。
 * 一处一事实：类型直接复用 verdict.ts 的 ItemVerdict / StepEvidence /
 * DomainVerdict——不造平行类型（本批教训：先回读再引用）。
 */

import type { DomainVerdict, ItemVerdict, StepEvidence, VerdictStep } from "./verdict";

// ---------- 域总检 → 轮次快照（CI 周跑的历史单元） ----------

export interface VerdictRun {
  /** 轮次 id（时间戳——回归对比的序）。 */
  runId: string;
  startedAt: number;
  finishedAt: number;
  /** 域总检产物（verdict.ts runDomainVerdict 的返回——不复制结构）。 */
  verdict: DomainVerdict;
}

export interface RenderedReport {
  markdown: string;
  passCount: number;
  failCount: number;
  durationMs: number;
}

/** 轮次 → Markdown 报告（证据三件套——台账与证据 12 查的产物面）。 */
export function renderReport(run: VerdictRun): RenderedReport {
  const items = run.verdict.items;
  const pass = items.filter((i) => i.pass);
  const fail = items.filter((i) => !i.pass);
  const durationMs = run.finishedAt - run.startedAt;
  const lines: string[] = [];
  lines.push(`# 个性化域总检报告 · ${run.runId}`);
  lines.push("");
  lines.push(`- 开始：${new Date(run.startedAt).toISOString()}`);
  lines.push(`- 结果：**${pass.length}/${items.length} 绿**（红 ${fail.length} 项）`);
  lines.push(`- 用时：${(durationMs / 1000).toFixed(1)}s（预算 1800s——${run.verdict.withinBudget ? "达标" : "超预算"}）`);
  lines.push(`- 证据链完整率：${run.verdict.evidenceComplete ? "100%" : "不完整——导出前先补证据"}`);
  lines.push("");
  if (fail.length > 0) {
    lines.push("## 红项");
    lines.push("");
    for (const it of fail) {
      const bad = it.steps.filter((s) => !s.ok);
      const stepNames: Record<VerdictStep, string> = { mutate: "改", "take-effect": "生效", rollback: "回退" };
      const where = bad.length > 0 ? `（卡在「${bad.map((s) => stepNames[s.step]).join("/")}」步）` : "";
      lines.push(`- **${it.id} ${it.name}**${where}：${bad[0]?.detail ?? "无详情"}`);
    }
    lines.push("");
  }
  lines.push("## 全量明细");
  lines.push("");
  lines.push("| 项 | 验证点 | 改 | 生效 | 回退 | 结论 |");
  lines.push("| --- | --- | --- | --- | --- | --- |");
  for (const it of items) {
    const mark = (step: VerdictStep): string => {
      const s = it.steps.find((x) => x.step === step);
      return s === undefined ? "—" : s.ok ? "✅" : "❌";
    };
    lines.push(`| ${it.id} ${it.name} | ${it.probe} | ${mark("mutate")} | ${mark("take-effect")} | ${mark("rollback")} | ${it.pass ? "绿" : "红"} |`);
  }
  lines.push("");
  return { markdown: lines.join("\n"), passCount: pass.length, failCount: fail.length, durationMs };
}

// ---------- 跨轮回归对比 ----------

export interface RegressionDiff {
  /** 上轮绿本轮红——回归（最高优先）。 */
  regressed: string[];
  /** 上轮红本轮绿——修复。 */
  fixed: string[];
  /** 两轮皆红——持续红（不是回归但需升级）。 */
  persistentlyFailing: string[];
  /** 两轮皆绿——稳定。 */
  stable: string[];
}

export function diffRuns(prev: VerdictRun, curr: VerdictRun): RegressionDiff {
  const ids = new Set([...prev.verdict.items.map((i) => i.id), ...curr.verdict.items.map((i) => i.id)]);
  const regressed: string[] = [];
  const fixed: string[] = [];
  const persistentlyFailing: string[] = [];
  const stable: string[] = [];
  for (const id of ids) {
    const a = prev.verdict.items.find((i) => i.id === id)?.pass;
    const b = curr.verdict.items.find((i) => i.id === id)?.pass;
    if (a === true && b === false) regressed.push(id);
    else if (a === false && b === true) fixed.push(id);
    else if (a === false && b === false) persistentlyFailing.push(id);
    else stable.push(id);
  }
  return { regressed, fixed, persistentlyFailing, stable };
}

/** 回归报告的人话结论（三要素：现状/原因方向/下一步）。 */
export function regressionMessage(d: RegressionDiff): string {
  if (d.regressed.length === 0 && d.persistentlyFailing.length === 0) {
    return `无回归${d.fixed.length > 0 ? `；${d.fixed.length} 项本轮转绿` : ""}——可归档。`;
  }
  const parts: string[] = [];
  if (d.regressed.length > 0) parts.push(`本轮新增红 ${d.regressed.length} 项（${d.regressed.slice(0, 3).join(", ")}${d.regressed.length > 3 ? "…" : ""}）——查最近改动`);
  if (d.persistentlyFailing.length > 0) parts.push(`持续红 ${d.persistentlyFailing.length} 项——升级为缺陷工单`);
  return parts.join("；") + "。";
}

// ---------- 抖动检测（多轮口径） ----------

export interface FlakyVerdict {
  id: string;
  flaky: boolean;
  /** 各轮 pass 序列。 */
  history: boolean[];
  /** 人话：抖动 = 稳定未达成。 */
  message: string;
}

/**
 * 抖动判定：N 轮中红绿并存 = flaky（稳定通过要求全绿；
 * 全红是持续红不是抖动——分类要准）。
 */
export function detectFlaky(history: VerdictRun[], itemId: string): FlakyVerdict {
  const seq = history.map((r) => r.verdict.items.find((i) => i.id === itemId)?.pass ?? false);
  const greens = seq.filter(Boolean).length;
  const flaky = greens > 0 && greens < seq.length;
  return {
    id: itemId,
    flaky,
    history: seq,
    message: flaky
      ? `抖动：${seq.length} 轮中 ${greens} 绿——间歇性失败按红处理，不算稳定通过`
      : greens === 0
        ? `持续红 ${seq.length} 轮——按持续红升级`
        : `稳定绿 ${seq.length} 轮`,
  };
}

// ---------- 证据完整性（导出前的深度门禁） ----------

export interface EvidenceGap {
  itemId: string;
  missing: Array<"mutate" | "take-effect" | "rollback" | "detail" | "freshness">;
  /** 缺什么的人话（导出面板直接展示）。 */
  human: string;
}

export const EVIDENCE_MAX_AGE_MS = 7 * 24 * 3600 * 1000; // 证据 7 天有效（过期重跑——新鲜度是证据的一部分）。

/**
 * 证据包完整性深度检查：
 * - 三步（改/生效/回退）全 ok 且 detail/evidence 在案；
 * - 证据年龄 ≤7 天（stale 证据不算证据——重跑取新证）。
 */
export function auditEvidence(run: VerdictRun, now: number): { complete: boolean; gaps: EvidenceGap[] } {
  const gaps: EvidenceGap[] = [];
  const stale = now - run.finishedAt > EVIDENCE_MAX_AGE_MS;
  for (const it of run.verdict.items) {
    const missing: EvidenceGap["missing"] = [];
    const step = (s: VerdictStep): StepEvidence | undefined => it.steps.find((x) => x.step === s);
    if (step("mutate")?.ok !== true) missing.push("mutate");
    if (step("take-effect")?.ok !== true) missing.push("take-effect");
    if (step("rollback")?.ok !== true) missing.push("rollback");
    if (it.steps.some((s) => !s.detail || !s.evidence)) missing.push("detail");
    if (stale) missing.push("freshness");
    if (missing.length > 0) {
      const zh: Record<string, string> = {
        mutate: "「改」步未记录",
        "take-effect": "「生效」步未记录",
        rollback: "「回退」步未记录",
        detail: "证据说明/证据路径缺失",
        freshness: "证据已超 7 天（过期——重跑取新证）",
      };
      gaps.push({ itemId: it.id, missing, human: `${it.id}：${missing.map((m) => zh[m]).join("、")}` });
    }
  }
  return { complete: gaps.length === 0, gaps };
}

// ---------- 证据包序列化（可导出可校验——第三方独立复核的格式面） ----------

export interface EvidencePackage {
  format: "vx-verdict-evidence";
  version: 1;
  runId: string;
  startedAt: number;
  finishedAt: number;
  items: ItemVerdict[];
  /** 包内容哈希（FNV-1a——篡改检出 F194 同源口径）。 */
  contentHash: string;
}

/** FNV-1a 32 位哈希（证据篡改检出——改一个字节哈希即变）。 */
export function fnv1a(input: string): string {
  let h = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    h ^= input.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return (h >>> 0).toString(16).padStart(8, "0");
}

function canonicalItems(items: ItemVerdict[]): ItemVerdict[] {
  return [...items].sort((a, b) => a.id.localeCompare(b.id));
}

export function buildEvidencePackage(run: VerdictRun): EvidencePackage {
  const stripped = { format: "vx-verdict-evidence" as const, version: 1 as const, runId: run.runId, startedAt: run.startedAt, finishedAt: run.finishedAt, items: canonicalItems(run.verdict.items) };
  return { ...stripped, contentHash: fnv1a(JSON.stringify(stripped)) };
}

/** 证据包验真（导入侧——哈希重算比对，篡改即拒）。 */
export function verifyEvidencePackage(pkg: EvidencePackage): { ok: boolean; reason?: string } {
  if (pkg.format !== "vx-verdict-evidence" || pkg.version !== 1) return { ok: false, reason: `格式不符：${pkg.format}@${pkg.version}` };
  const recomputed = fnv1a(JSON.stringify({ format: pkg.format, version: pkg.version, runId: pkg.runId, startedAt: pkg.startedAt, finishedAt: pkg.finishedAt, items: canonicalItems(pkg.items) }));
  return recomputed === pkg.contentHash ? { ok: true } : { ok: false, reason: "内容哈希不符——证据被篡改" };
}
