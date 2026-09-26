/**
 * H4 域状态对账（v3 · 检查项对账的运行时面）：
 * 给 F375/F400 门禁面板提供**诚实的**域完成度状态——
 * - H4 自己的 50 项：已验数（调用方传真实通过结果，面板不伪造）；
 * - H1-H3 的 150 项：**登记册尚未就绪**（v3 时点各队登记册未落库），
 *   状态如实标「待注入」，绝不代写、绝不代绿——主册总检基线 200 的
 *   逐字对账由离线对账测试（h4reconcile.spec.ts 读主册原文）钉死，
 *   运行时只消费该测试钉死的结论，不复制主册文本（一处一事实）。
 */

import { H4_REGISTRY } from "../../system/h4/registry";
import { buildHDomainCheckpoints, type MasterCheckpoint } from "../../system/h4/f400-hDomainClosure";

/** 主册 H 域总检基线（F201-F400 共 200 项——离线对账测试对主册原文逐字钉死）。 */
export const MASTER_H_DOMAIN_BASELINE = 200;
/** H1-H3 待注入项数（150 = 200 - 50；注入通道：buildHDomainCheckpoints 参数面）。 */
export const H1H3_PENDING_COUNT = MASTER_H_DOMAIN_BASELINE - H4_REGISTRY.length;

export interface H4DomainStatus {
  /** H4 已验通过数（来自调用方的真实单测/走查结果）。 */
  h4Passed: number;
  h4Total: number;
  /** H1-H3 已注入登记册数（当前为 0——各队登记册就绪后由消费面注入）。 */
  h1h3Injected: number;
  /** 当前检查点总数（50 + 注入数；主册基线 200 由离线对账钉死）。 */
  checkpointTotal: number;
  masterBaseline: number;
  /** F400 判据「新增 200 个全绿基线」的达成状态：只有 200 全注入且全绿才真。 */
  baselineComplete: boolean;
  /** 诚实文案（面板直接渲染）。 */
  note: string;
}

/**
 * 域状态计算（纯函数）：results 只接受 H4 自己的逐项结果；
 * injected 只接受 H1-H3 登记册的**真实行**（item+title+passed）——
 * 本函数绝不编造 H1-H3 的标题与通过态；未传 = 未注入 = 如实待注入。
 */
export function h4DomainStatus(
  results: ReadonlyArray<{ item: string; passed: boolean }>,
  injected: ReadonlyArray<{ item: string; title: string; passed: boolean }> = [],
): H4DomainStatus {
  const h4Passed = H4_REGISTRY.filter((e) => results.find((r) => r.item === e.item)?.passed === true).length;
  const agg = buildHDomainCheckpoints(
    injected.map((r) => ({ item: r.item, title: r.title })),
    [...injected.map((r) => ({ item: r.item, passed: r.passed })), ...results],
  );
  const checkpointTotal = agg.total;
  const baselineComplete = checkpointTotal === MASTER_H_DOMAIN_BASELINE && agg.allGreen;
  const note = baselineComplete
    ? "主册 200 项基线全注入且全绿（F400 判据达成）"
    : `F351-F400 ${h4Passed}/${H4_REGISTRY.length} 已验 · F201-F350 ${H1H3_PENDING_COUNT} 项待 H1-H3 登记册注入（不代写不代绿；主册 200 基线由离线对账测试钉死）`;
  return {
    h4Passed,
    h4Total: H4_REGISTRY.length,
    h1h3Injected: injected.length,
    checkpointTotal,
    masterBaseline: MASTER_H_DOMAIN_BASELINE,
    baselineComplete,
    note,
  };
}

/** 供门禁面板消费的检查点转换（H4 50 项 → MasterCheckpoint）。 */
export function h4Checkpoints(results: ReadonlyArray<{ item: string; passed: boolean }>, evidenceOf: (item: string) => string): MasterCheckpoint[] {
  return H4_REGISTRY.map((e) => {
    const passed = results.find((r) => r.item === e.item)?.passed === true;
    return { item: e.item, name: e.title, passed, evidence: evidenceOf(e.item) };
  });
}
