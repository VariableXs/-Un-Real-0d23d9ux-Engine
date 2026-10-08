import { describe, expect, it } from "vitest";
import { __clearMem, memStore } from "../internal/store";
import { TASK_CHAIN_BUDGET_SECONDS, TASK_CHAIN_REQUIRED_STEPS, archivedReports, archiveReport, consistencyWalkthrough, degradedWalkthrough, releaseDecision, taskChainWalkthrough, type Checkpoint, type TaskChainStep } from "../f375-hDomainGate";

function cp(item: string, passed: boolean, name = "锚点"): Checkpoint {
  return { item, name, passed, evidence: `evidence-${item}` };
}

function fullChain(): TaskChainStep[] {
  return Array.from({ length: TASK_CHAIN_REQUIRED_STEPS }, (_, i) => ({ task: `t${i}`, seconds: 40, stuck: false }));
}

describe("F375 H 域总判据", () => {
  it("一致性走查：对照表 100% 绿才过；一红即红", () => {
    const ok = consistencyWalkthrough([
      { interaction: "Esc 关闭浮层", surfaces: ["a", "b"], consistent: true },
      { interaction: "表头排序三态", surfaces: ["a", "c"], consistent: true },
    ]);
    expect(ok.pass).toBe(true);
    const bad = consistencyWalkthrough([{ interaction: "Esc", surfaces: ["a", "b"], consistent: false }]);
    expect(bad.pass).toBe(false);
    expect(consistencyWalkthrough([]).pass).toBe(false);
  });

  it("手感走查：20 任务链 + ≤15 分钟 + 零卡壳（判据三条件）", () => {
    const good = taskChainWalkthrough(fullChain());
    expect(good.pass).toBe(true);
    expect(good.budgetSeconds).toBe(TASK_CHAIN_BUDGET_SECONDS);
    expect(taskChainWalkthrough(fullChain().slice(0, 19)).pass).toBe(false);
    expect(taskChainWalkthrough(fullChain().map((s) => ({ ...s, seconds: 60 }))).pass).toBe(false); // 20min > 15min
    const stuck = taskChainWalkthrough(fullChain().map((s, i) => (i === 3 ? { ...s, stuck: true } : s)));
    expect(stuck.pass).toBe(false);
    expect(stuck.steps[3]!.stuck).toBe(true);
  });

  it("降级走查：三态缺一不可（缺省态判不合格）", () => {
    const ok = degradedWalkthrough([
      { state: "perf-low", allFunctional: true },
      { state: "battery-low", allFunctional: true },
      { state: "reduce-motion", allFunctional: true },
    ]);
    expect(ok.pass).toBe(true);
    const missing = degradedWalkthrough([
      { state: "perf-low", allFunctional: true },
      { state: "battery-low", allFunctional: true },
    ]);
    expect(missing.pass).toBe(false);
    expect(missing.states.find((s) => s.state === "reduce-motion")!.allFunctional).toBe(false);
  });

  it("总判定：任何一项红 = 回炉不发布；回炉清单逐条列出", () => {
    const reports = {
      consistency: consistencyWalkthrough([{ interaction: "i", surfaces: ["a"], consistent: true }]),
      taskChain: taskChainWalkthrough(fullChain()),
      degraded: degradedWalkthrough([
        { state: "perf-low", allFunctional: true },
        { state: "battery-low", allFunctional: true },
        { state: "reduce-motion", allFunctional: true },
      ]),
    };
    const allGreen = releaseDecision([cp("F351", true), cp("F399", true)], reports);
    expect(allGreen.release).toBe(true);
    const withRed = releaseDecision([cp("F351", true), cp("F360", false, "读数漂移")], reports);
    expect(withRed.release).toBe(false);
    expect(withRed.rework.map((c) => c.item)).toEqual(["F360"]);
  });

  it("报告归档：ISO 日期入账、滚动 12 份", () => {
    __clearMem();
    const s = memStore();
    const reports = {
      consistency: consistencyWalkthrough([{ interaction: "i", surfaces: ["a"], consistent: true }]),
      taskChain: taskChainWalkthrough(fullChain()),
      degraded: degradedWalkthrough([
        { state: "perf-low", allFunctional: true },
        { state: "battery-low", allFunctional: true },
        { state: "reduce-motion", allFunctional: true },
      ]),
    };
    for (let i = 0; i < 15; i++) archiveReport(releaseDecision([], reports), `2026-09-${String(i + 1).padStart(2, "0")}`, s);
    const all = archivedReports(s);
    expect(all).toHaveLength(12);
    expect(all[0]!.at).toBe("2026-09-04");
    expect(all[11]!.decision.release).toBe(true);
  });
});
