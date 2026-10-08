import { describe, expect, it } from "vitest";
import { EVENT_KINDS, TIMELINE_CAP, WINDOW_MS, auditTemplateCoverage, buildTimeline, fillTemplate, fnv1a, humanTime, verifyChain, type AuditEvent } from "../f372-activityTimeline";

const NOW = new Date(2026, 8, 25, 14, 7, 0).getTime();

function ev(seq: number, kind: AuditEvent["kind"], minutesAgo: number, params?: Record<string, string>, detail = "tech:0001"): AuditEvent {
  return { seq, kind, at: NOW - minutesAgo * 60_000, techDetail: detail, params };
}

describe("F372 系统活动人话时间线", () => {
  it("映射表覆盖率：EVENT_KINDS 全类目有人话模板（判据）", () => {
    expect(auditTemplateCoverage()).toEqual({ pass: true, missing: [] });
    expect(EVENT_KINDS).toHaveLength(8);
  });

  it("事件转译人话：动态参数填充正确", () => {
    const rows = buildTimeline([ev(1, "appOpen", 5, { app: "记事本" }), ev(2, "selfHeal", 2, { count: "3" })], NOW);
    expect(rows.map((r) => r.text)).toEqual(["打开 记事本", "自检修复 3 处"]);
  });

  it("缺参数模板如实保留占位（零静默）", () => {
    expect(fillTemplate("打开 {app}", {})).toBe("打开 {app}");
    expect(fillTemplate("打开 {app}")).toBe("打开 {app}");
  });

  it("24h 窗口与存储上限（判据）", () => {
    const outOfWindow = buildTimeline([ev(1, "abnormalShutdown", WINDOW_MS / 60000 + 10)], NOW);
    expect(outOfWindow).toHaveLength(0);
    const many: AuditEvent[] = Array.from({ length: TIMELINE_CAP + 50 }, (_, i) => ev(i + 1, "appOpen", (i % 60) + 1, { app: `a${i}` }, `d${i}`));
    expect(buildTimeline(many, NOW)).toHaveLength(TIMELINE_CAP);
  });

  it("时间准确性：HH:mm 且升序排列", () => {
    const rows = buildTimeline([ev(2, "abnormalShutdown", 2), ev(1, "appOpen", 5, { app: "x" })], NOW);
    expect(rows[0]!.time).toBe("14:02");
    expect(rows[1]!.time).toBe("14:05");
    expect(humanTime(NOW, new Date(NOW))).toBe("14:07");
  });

  it("只读+哈希链：整链可验；改一条即断链定位（判据）", () => {
    const events = [ev(1, "appOpen", 5, { app: "记事本" }), ev(2, "abnormalShutdown", 2), ev(3, "selfHeal", 1, { count: "3" })];
    const rows = buildTimeline(events, NOW);
    expect(verifyChain(rows)).toEqual({ intact: true, brokenAt: null });
    const tampered = rows.map((r, i) => (i === 1 ? { ...r, text: "打开 账本（被改）" } : r));
    const v = verifyChain(tampered);
    expect(v.intact).toBe(false);
    expect(v.brokenAt).toBe(2);
  });

  it("fnv1a 确定性：同文同哈希、异文异哈希", () => {
    expect(fnv1a("abc")).toBe(fnv1a("abc"));
    expect(fnv1a("abc")).not.toBe(fnv1a("abd"));
  });
});
