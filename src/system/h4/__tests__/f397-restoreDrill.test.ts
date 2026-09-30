import { describe, expect, it } from "vitest";
import { DRILL_CYCLE_DAYS, DRILL_STEPS, aboutPageView, auditStepCardLinkage, auditZeroSideEffect, drillReminderDue, firstRestoreNotice, loadRecords, recordDrill } from "../f397-restoreDrill";
import { __clearMem, memStore } from "../internal/store";

const DAY = 24 * 3600 * 1000;
const STEPS = DRILL_STEPS.map((s) => s.id);

describe("F397 还原演练", () => {
  it("引导步骤与 F198 三卡联动（判据）：四步全挂卡、收尾步显式无卡", () => {
    expect(DRILL_STEPS).toHaveLength(4);
    expect(DRILL_STEPS[0]!.card).toContain("card-1");
    expect(DRILL_STEPS[2]!.card).toContain("card-3");
    expect(auditStepCardLinkage()).toEqual({ pass: true, unlinked: [] });
  });

  it("模拟零副作用判据：有写操作的演练不入账（判据）", () => {
    __clearMem();
    const s = memStore();
    const dirty = recordDrill({ at: 1, completedSteps: STEPS, sideEffectWrites: 2 }, s);
    expect(dirty.ok).toBe(false);
    expect(dirty.reason).toContain("零副作用");
    expect(loadRecords(s)).toHaveLength(0);
    const clean = recordDrill({ at: 1, completedSteps: STEPS, sideEffectWrites: 0 }, s);
    expect(clean.ok).toBe(true);
    expect(loadRecords(s)).toHaveLength(1);
  });

  it("半途退出不算演练（判据「全步骤走完」）", () => {
    __clearMem();
    const s = memStore();
    const partial = recordDrill({ at: 1, completedSteps: STEPS.slice(0, 2), sideEffectWrites: 0 }, s);
    expect(partial.ok).toBe(false);
    expect(partial.reason).toContain("未走完");
  });

  it("季度提醒周期（判据 90 天）：从未演练=到期；90 天内不提醒", () => {
    __clearMem();
    const s = memStore();
    expect(drillReminderDue(1, s)).toEqual({ due: true, lastDrillAt: null, neverDrilled: true });
    recordDrill({ at: 0, completedSteps: STEPS, sideEffectWrites: 0 }, s);
    expect(drillReminderDue(89 * DAY, s).due).toBe(false);
    expect(drillReminderDue(91 * DAY, s).due).toBe(true);
    expect(DRILL_CYCLE_DAYS).toBe(90);
  });

  it("首次恢复加练提示（判据）：演练过则不再提示", () => {
    __clearMem();
    const s = memStore();
    expect(firstRestoreNotice(s)).toContain("还没演练过");
    recordDrill({ at: 0, completedSteps: STEPS, sideEffectWrites: 0 }, s);
    expect(firstRestoreNotice(s)).toBeNull();
  });

  it("演练记录「关于」页视图（判据）：上次日期/总次数/下次提醒", () => {
    __clearMem();
    const s = memStore();
    recordDrill({ at: 100, completedSteps: STEPS, sideEffectWrites: 0 }, s);
    recordDrill({ at: 200, completedSteps: STEPS, sideEffectWrites: 0 }, s);
    const v = aboutPageView(s);
    expect(v).toEqual({ lastDrillAt: 200, totalDrills: 2, nextReminderAt: 200 + 90 * DAY });
  });

  it("零副作用审计器：写账为空才过", () => {
    expect(auditZeroSideEffect([])).toEqual({ pass: true, writes: 0 });
    expect(auditZeroSideEffect(["write://disk"]).pass).toBe(false);
  });
});
