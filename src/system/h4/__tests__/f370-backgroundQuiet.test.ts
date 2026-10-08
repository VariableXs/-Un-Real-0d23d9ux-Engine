import { describe, expect, it } from "vitest";
import { BACKOFF_SCHEDULE_MS, MAX_RETRIES, auditNoResidentUi, auditThreePromises, finalFailureNotice, ioYieldRequired, onRetrySuccess, onTaskFailure, retryDue } from "../f370-backgroundQuiet";

describe("F370 后台不惊扰承诺", () => {
  it("三次指数退避：1s→2s→4s，第三次失败后终态", () => {
    expect(BACKOFF_SCHEDULE_MS).toEqual([1000, 2000, 4000]);
    let s = { attempts: 0, nextRetryAt: null as number | null, failed: false };
    s = onTaskFailure(s, 0).state;
    expect(s.nextRetryAt).toBe(1000);
    s = onTaskFailure({ ...s, nextRetryAt: null }, 1000).state;
    expect(s.nextRetryAt).toBe(3000);
    const third = onTaskFailure({ ...s, nextRetryAt: null }, 3000);
    expect(third.state.nextRetryAt).toBe(7000);
    expect(third.finalFailure).toBe(false);
    const fourth = onTaskFailure({ ...third.state, nextRetryAt: null }, 7000);
    expect(fourth.finalFailure).toBe(true);
    expect(fourth.state.failed).toBe(true);
  });

  it("重试到期判定与成功清账", () => {
    const s = onTaskFailure({ attempts: 0, nextRetryAt: null, failed: false }, 0).state;
    expect(retryDue(s, 999)).toBe(false);
    expect(retryDue(s, 1000)).toBe(true);
    const cleared = onRetrySuccess(s);
    expect(cleared).toEqual({ attempts: 0, nextRetryAt: null, failed: false });
  });

  it("最终失败只发一条：进通知中心、横幅=0（判据）、文案三要素", () => {
    const n = finalFailureNotice("文件索引", "磁盘空间不足");
    expect(n.banner).toBe(false);
    expect(n.severity).toBe("center");
    expect(n.message.what).toContain("3 次");
    expect(n.message.next).toContain("后台任务中心");
  });

  it("不抢 IO：前台忙碌时后台请求被拒（F057 对账）", () => {
    expect(ioYieldRequired(true).allowed).toBe(false);
    expect(ioYieldRequired(false).allowed).toBe(true);
  });

  it("不显眼：白名单外常驻 UI 面即违规", () => {
    expect(auditNoResidentUi(["f369-task-center", "notify-center"]).pass).toBe(true);
    const bad = auditNoResidentUi(["f369-task-center", "always-on-top-floating-bar"]);
    expect(bad.pass).toBe(false);
    expect(bad.illegal).toEqual(["always-on-top-floating-bar"]);
  });

  it("三不逐项测试汇总判据", () => {
    const ok = auditThreePromises({ foregroundBusy: false, residentSurfaces: ["f369-task-center"], attemptFailures: 2 });
    expect(ok.pass).toBe(true);
    const bad = auditThreePromises({ foregroundBusy: true, residentSurfaces: ["popup-spammer"], attemptFailures: 5 });
    expect(bad.pass).toBe(false);
    expect(MAX_RETRIES).toBe(3);
  });
});
