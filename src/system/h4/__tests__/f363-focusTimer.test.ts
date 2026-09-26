import { describe, expect, it } from "vitest";
import { __clearMem, memStore } from "../internal/store";
import { CUSTOM_MAX_MINUTES, CUSTOM_MIN_MINUTES, abandon, actualMinutes, auditDailyLedger, badgeText, dailySummary, endReminder, isDue, recordRun, remainingSec, validateMinutes, type FocusRun } from "../f363-focusTimer";

function run(plannedMinutes: number, startedAt: number, day = "2026-09-25"): FocusRun {
  return { day, plannedMinutes, startedAt, endedAt: null, outcome: "running" };
}

describe("F363 专注计时器", () => {
  it("两档 + 自定范围：25/50 直接过；自定 5-120 整数过；越界/小数/NaN 拒", () => {
    expect(validateMinutes(25).ok).toBe(true);
    expect(validateMinutes(50).ok).toBe(true);
    expect(validateMinutes(45).ok).toBe(true);
    expect(validateMinutes(4).ok).toBe(false);
    expect(validateMinutes(CUSTOM_MAX_MINUTES + 1).ok).toBe(false);
    expect(validateMinutes(CUSTOM_MIN_MINUTES).ok).toBe(true);
    expect(validateMinutes(25.5).ok).toBe(false);
    expect(validateMinutes(Number.NaN).ok).toBe(false);
  });

  it("徽标实时性：逐秒读数、mm:ss 格式、到点归零", () => {
    const r = run(25, 0);
    expect(remainingSec(r, 0)).toBe(25 * 60);
    expect(badgeText(r, 65_000)).toBe("23:55");
    expect(isDue(r, 25 * 60 * 1000)).toBe(true);
    expect(isDue(r, 25 * 60 * 1000 - 1)).toBe(false);
  });

  it("结束提醒双通道：一声（低音量温和）+ 一条通知", () => {
    const r = endReminder(run(25, 0));
    expect(r.chime).toBe(true);
    expect(r.chimeVolume).toBeLessThanOrEqual(0.5);
    expect(r.notification.length).toBeGreaterThan(0);
  });

  it("放弃真实性：真实已用分钟入账，不虚报满档", () => {
    const r = abandon({ ...run(50, 0), plannedMinutes: 50 }, 10 * 60 * 1000);
    expect(r.outcome).toBe("abandoned");
    expect(actualMinutes(r, 0)).toBe(10);
    const tiny = abandon({ ...run(25, 0) }, 30_000);
    expect(actualMinutes(tiny, 0)).toBe(1); // 不足 1 分钟按 1 记（有始有终）
    const done = { ...run(25, 0), endedAt: 25 * 60 * 1000, outcome: "completed" as const };
    expect(actualMinutes(done, 0)).toBe(25);
  });

  it("每日累计：完成+放弃都入账；对账=逐笔之和", () => {
    __clearMem();
    const s = memStore();
    const a = { ...run(25, 0), endedAt: 25 * 60 * 1000, outcome: "completed" as const };
    const b = abandon(run(50, 1000), 11 * 60 * 1000 + 1000);
    const m1 = recordRun(a, 0, s).minutes;
    const m2 = recordRun(b, 0, s).minutes;
    const sum = auditDailyLedger("2026-09-25", [m1, m2], s);
    expect(sum.consistent).toBe(true);
    expect(dailySummary("2026-09-25", s)).toEqual({ minutes: 25 + 11, runs: 2, abandoned: 1 });
    expect(dailySummary("2099-01-01", s)).toEqual({ minutes: 0, runs: 0, abandoned: 0 });
  });

  it("跨日分账：不同 day 各自成账不串", () => {
    __clearMem();
    const s = memStore();
    recordRun({ ...run(25, 0, "2026-09-25"), endedAt: 1, outcome: "completed" }, 0, s);
    recordRun({ ...run(50, 0, "2026-09-26"), endedAt: 1, outcome: "completed" }, 0, s);
    expect(dailySummary("2026-09-25", s).minutes).toBe(25);
    expect(dailySummary("2026-09-26", s).minutes).toBe(50);
  });
});
