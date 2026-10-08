import { describe, expect, it } from "vitest";
import { __clearMem, memStore } from "../internal/store";
import { SYSTEM_TASK_KINDS, advance, auditTierOrdering, effectivelyRunning, initialState, loadPersisted, persistState, registerTask, setGlobalPaused, setPaused, visibility, type BackgroundTask } from "../f369-taskCenter";

function task(id: string, kind: BackgroundTask["kind"], pct = 10, tier: BackgroundTask["tier"] = "background"): BackgroundTask {
  return { id, kind, name: id, progressPct: pct, etaMs: 1000, paused: false, tier };
}

describe("F369 后台任务中心", () => {
  it("任务注册完整性：四类系统任务全入册；未登记类别拒收（零静默）", () => {
    expect(SYSTEM_TASK_KINDS).toEqual(["fileIndex", "diskCheck", "versionSnapshot", "updateDownload"]);
    let s = initialState();
    for (const kind of SYSTEM_TASK_KINDS) {
      const r = registerTask(s, task(`t-${kind}`, kind));
      expect(r.ok).toBe(true);
      s = r.state;
    }
    const bad = registerTask(s, task("x", "hacker" as BackgroundTask["kind"]));
    expect(bad.ok).toBe(false);
    expect(bad.reason).toContain("未登记");
  });

  it("重复注册幂等拒绝", () => {
    let s = registerTask(initialState(), task("a", "fileIndex")).state;
    const again = registerTask(s, task("a", "fileIndex"));
    expect(again.ok).toBe(false);
    expect(again.state.tasks).toHaveLength(1);
  });

  it("四字段信息准确性：进度钳制、ETA 有效性校验", () => {
    let s = registerTask(initialState(), task("a", "diskCheck", 150)).state;
    expect(s.tasks[0]!.progressPct).toBe(100);
    s = advance(s, "a", 50, -5);
    expect(s.tasks[0]!.etaMs).toBeNull(); // 非法 ETA 显式置不可估
    s = advance(s, "a", 50, 1234.6);
    expect(s.tasks[0]!.etaMs).toBe(1235);
  });

  it("两级暂停：逐任务 + 全局；有效运行态叠加正确", () => {
    let s = registerTask(initialState(), task("a", "fileIndex")).state;
    s = registerTask(s, task("b", "diskCheck")).state;
    s = setPaused(s, "a", true);
    expect(effectivelyRunning(s.tasks[0]!, s)).toBe(false);
    expect(effectivelyRunning(s.tasks[1]!, s)).toBe(true);
    s = setGlobalPaused(s, true);
    expect(effectivelyRunning(s.tasks[1]!, s)).toBe(false);
    expect(visibility(s).allQuiet).toBe(true);
    s = setGlobalPaused(s, false);
    expect(visibility(s).badge).toBe(1); // a 仍被逐任务暂停
  });

  it("调度分级对账 F057：interactive 被 batch 顶前 = 倒挂缺陷", () => {
    let s = registerTask(initialState(), task("bg", "fileIndex", 10, "batch")).state;
    s = registerTask(s, task("fg", "updateDownload", 10, "interactive")).state;
    const a = auditTierOrdering(s);
    expect(a.pass).toBe(false);
    expect(a.violations[0]).toContain("倒挂");
    const fixed = { ...s, tasks: [s.tasks[1]!, s.tasks[0]!] };
    expect(auditTierOrdering(fixed).pass).toBe(true);
  });

  it("完成态不占徽标；持久化 round-trip", () => {
    __clearMem();
    const s = memStore();
    let st = registerTask(initialState(), task("done", "fileIndex", 100)).state;
    st = registerTask(st, task("run", "diskCheck", 20)).state;
    expect(visibility(st).badge).toBe(1);
    expect(persistState(st, s)).toBe(true);
    const loaded = loadPersisted(s);
    expect(loaded.tasks).toHaveLength(2);
    // 独立空存储 → 默认态（持久化未写入时如实回退）
    expect(loadPersisted({ getItem: () => null, setItem: () => {} })).toEqual(initialState());
  });
});
