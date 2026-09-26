import { beforeEach, describe, expect, it } from "vitest";
import { runDomainVerdict, verdictChecklistJson, coveredSections, TOTAL_BUDGET_MINUTES } from "../verdict";
import { personaStore, PERSONA_SECTIONS } from "../store";
import { loadTokenTable, defaultTokenTable, tokenTableHash } from "../tokens";

beforeEach(() => {
  personaStore.reset();
});

describe("F170 个性化域总判据（三步执法）", () => {
  it("19/19 三步全绿（改→生效→回退）", () => {
    const v = runDomainVerdict();
    expect(v.total).toBe(19);
    const failed = v.items.filter((i) => !i.pass);
    // 失败项输出三要素（哪步/差多少/证据路径）——先钉证据再修实现。
    if (failed.length > 0) {
      const detail = failed.map((f) => `${f.id}[${f.name}] ${f.steps.map((s) => `${s.step}:${s.ok ? "ok" : s.detail}`).join(" | ")}`).join("\n");
      throw new Error(`三步执法存在红项：\n${detail}`);
    }
    expect(v.passed).toBe(19);
    expect(v.allGreen).toBe(true);
  });

  it("每项三步证据链完整（证据链完整率 100%）", () => {
    const v = runDomainVerdict();
    expect(v.evidenceComplete).toBe(true);
    for (const item of v.items) {
      expect(item.steps).toHaveLength(3);
      expect(item.steps.map((s) => s.step)).toEqual(["mutate", "take-effect", "rollback"]);
      for (const s of item.steps) {
        expect(s.evidence.length).toBeGreaterThan(0);
        expect(s.detail.length).toBeGreaterThan(0);
      }
    }
  });

  it("全量执行远在 30 分钟预算内（可进 CI 周跑）", () => {
    expect(TOTAL_BUDGET_MINUTES).toBe(30);
    const v = runDomainVerdict();
    expect(v.withinBudget).toBe(true);
    expect(v.elapsedMinutes).toBeLessThan(1);
  });

  it("回退后配置哈希还原（域级零残留抽查：F151 令牌表）", () => {
    const before = tokenTableHash(loadTokenTable());
    runDomainVerdict();
    expect(tokenTableHash(loadTokenTable())).toBe(before);
  });

  it("checklist JSON 可导出且覆盖 19 项（vx-walkcheck-e 消费口径）", () => {
    const json = JSON.parse(verdictChecklistJson()) as { format: string; items: unknown[] };
    expect(json.format).toBe("vx-walkcheck-e");
    expect(json.items).toHaveLength(19);
  });

  it("覆盖面自证：执法探针涉及全部分节", () => {
    expect(coveredSections()).toEqual(PERSONA_SECTIONS);
  });

  it("执法起点干净：默认令牌表哈希一致", () => {
    expect(tokenTableHash(loadTokenTable())).toBe(tokenTableHash(defaultTokenTable()));
  });
});
