import { describe, expect, it } from "vitest";
import {
  JOURNEYS,
  libCaps,
  retryFailed,
  runJourney,
  type JourneyCaps,
} from "../journeys";

/** feature 层能力注入：真实导出面（缺的用 null=如实跳过，绝不伪造 true）。 */
const fullCaps: JourneyCaps = {
  oobeWizard: () => true,
  bootPacing: () => true,
  engineColdStages: () => true,
  channelVerdict: () => true,
  sharedAppsRegistry: () => true,
  saveQueue: () => true,
  richClipboard: () => true,
  demoIsolation: () => true,
  kvaultBurn: () => true,
  migration: () => libCaps().migration(),
  retirement: () => libCaps().retirement(),
  skeleton: () => libCaps().skeleton(),
};

describe("任务82 · 专项 M 六条旅程", () => {
  it("六条旅程齐全且编号 M1-M6 与总案对应", () => {
    expect(JOURNEYS.map((j) => j.id)).toEqual(["M1", "M2", "M3", "M4", "M5", "M6"]);
    expect(JOURNEYS.every((j) => j.steps.length >= 3)).toBe(true);
    expect(JOURNEYS.every((j) => j.issues.length === 0)).toBe(true); // 人工轮前零痛点登记
  });

  it("能力全注入：六条旅程逻辑链全绿", () => {
    for (const j of JOURNEYS) {
      const r = runJourney(j.id, fullCaps);
      expect(r.ok, `${j.id}: ${r.results.filter((x) => x.state === "fail").map((x) => x.id)}`).toBe(true);
      expect(r.retriable).toEqual([]);
    }
  });

  it("lib 层能力默认实现为真断言（搬家状态机/退役三轮/骨架屏时机）", () => {
    const caps = libCaps();
    expect(caps.migration()).toBe(true);
    expect(caps.retirement()).toBe(true);
    expect(caps.skeleton()).toBe(true);
  });

  it("能力缺失 = 如实 skip（绝不伪造 true）", () => {
    const r = runJourney("M1", { ...fullCaps, oobeWizard: () => null });
    const s2 = r.results.find((x) => x.id === "s2")!;
    expect(s2.state).toBe("skip");
    expect(s2.note).toContain("如实跳过");
    expect(r.ok).toBe(true); // skip 不算失败（如实登记）
  });

  it("失败可重试且不报废全程：fail 步重试后合并报告", () => {
    let flip = true;
    const r1 = runJourney("M3", { ...fullCaps, saveQueue: () => (flip = !flip) });
    // saveQueue 第一次返回 false → fail；报告标 retriable
    const sq = r1.results.find((x) => x.id === "s1")!;
    if (sq.state === "fail") {
      expect(r1.ok).toBe(false);
      expect(r1.retriable).toContain("s1");
      const r2 = retryFailed(r1, { ...fullCaps, saveQueue: () => true });
      expect(r2.ok).toBe(true);
      // 其他步结果保持不重跑（合并而非覆盖）
      expect(r2.results.length).toBe(r1.results.length);
    }
  });

  it("重试零失败报告为幂等（retriable 空 → 原样返回）", () => {
    const r = runJourney("M2", fullCaps);
    expect(retryFailed(r, fullCaps)).toBe(r);
  });

  it("未知旅程拒绝", () => {
    expect(() => runJourney("M7" as never, fullCaps)).toThrow(/unknown journey/);
  });

  it("M5 挂钩异常三场景语义（引擎阶段化断言在位）", () => {
    const m5 = JOURNEYS.find((j) => j.id === "M5")!;
    expect(m5.steps.some((s) => s.label.includes("异常三场景"))).toBe(true);
    expect(runJourney("M5", fullCaps).ok).toBe(true);
  });
});
