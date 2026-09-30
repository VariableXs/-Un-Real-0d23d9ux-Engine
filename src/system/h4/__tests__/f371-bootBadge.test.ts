import { describe, expect, it } from "vitest";
import { __clearMem, memStore } from "../internal/store";
import { BADGE_TIMING, RECONCILE_TOLERANCE_MS, auditReconciliation, badgeFromMeasured, badgePhase, bootHistory, curveFromSameLedger, isEnabled, recordBoot, setEnabled, type BootRecord } from "../f371-bootBadge";

const REC: BootRecord = { seq: 1, measuredMs: 3200, at: 1000 };

describe("F371 开机时长徽标", () => {
  it("徽标与实测链同源换算：误差 0（<100ms 判据）", () => {
    const vm = badgeFromMeasured(REC);
    expect(vm.text).toBe("本次开机 3.2 秒");
    expect(vm.driftMs).toBe(0);
    expect(vm.ok).toBe(true);
    expect(RECONCILE_TOLERANCE_MS).toBe(100);
  });

  it("淡入淡出时序：300/3000/500 逐段", () => {
    expect(BADGE_TIMING).toEqual({ fadeInMs: 300, holdMs: 3000, fadeOutMs: 500 });
    expect(badgePhase(0)).toBe("hidden");
    expect(badgePhase(100)).toBe("fade-in");
    expect(badgePhase(299)).toBe("fade-in");
    expect(badgePhase(300)).toBe("hold");
    expect(badgePhase(3299)).toBe("hold");
    expect(badgePhase(3300)).toBe("fade-out");
    expect(badgePhase(3799)).toBe("fade-out");
    expect(badgePhase(3800)).toBe("gone");
  });

  it("可关：默认开、关闭持久化", () => {
    __clearMem();
    const s = memStore();
    expect(isEnabled(s)).toBe(true);
    setEnabled(false, s);
    expect(isEnabled(s)).toBe(false);
  });

  it("历史账滚动上限 90；曲线与徽标同账本（判据）", () => {
    __clearMem();
    const s = memStore();
    for (let i = 1; i <= 95; i++) recordBoot({ seq: i, measuredMs: 2000 + i, at: i }, s);
    expect(bootHistory(s)).toHaveLength(90);
    expect(bootHistory(s)[0]!.seq).toBe(6); // 最旧 5 条被挤掉
    const curve = curveFromSameLedger(s);
    expect(curve).toHaveLength(90);
    expect(curve[89]!.seq).toBe(95);
  });

  it("对账审计：全账误差 <100ms 通过（判据）", () => {
    __clearMem();
    const s = memStore();
    recordBoot(REC, s);
    recordBoot({ seq: 2, measuredMs: 2750, at: 2 }, s);
    const a = auditReconciliation(s);
    expect(a.pass).toBe(true);
    expect(a.worstDriftMs).toBeLessThan(RECONCILE_TOLERANCE_MS); // 2.75s → 一位小数换算漂移 50ms，仍在判据内
  });
});
