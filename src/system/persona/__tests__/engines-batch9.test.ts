import { describe, expect, it } from "vitest";
import { dueTasks, fairOrder, executionPlan, missedCount, nextRunAt, wakeRecoveryPlan, MISSED_POLICY_NOTE, type ScheduledTask } from "../scheduler";
import { rankEvents, topK, withPinned, exportRanking, reconcileWithExternal, type UsageEvent } from "../usage-model";
import { buildCheatSheet, printableGrid, occupancyMatrix, suggestFreeCombos, blindWalkScript } from "../cheatsheet";
import { structuralDiff, describeDiff, threeWayMerge } from "../archive-diff";
import { shardJobs, runShard, runAll, incrementalJobs, type ProbeJob } from "../walkcheck-runner";

const DAY = 86_400_000;

// ---------- scheduler ----------

describe("scheduler · 统一调度（F153/F163 深化）", () => {
  const tasks: ScheduledTask[] = [
    { id: "a", intervalMs: DAY, priority: 1, missed: "skip", lastRunAt: 0, enabled: true },
    { id: "b", intervalMs: DAY, priority: 5, missed: "once", lastRunAt: 0, enabled: true },
    { id: "c", intervalMs: DAY, priority: 5, missed: "catchup", lastRunAt: 0, enabled: true },
    { id: "off", intervalMs: DAY, priority: 9, missed: "skip", lastRunAt: null, enabled: false },
  ];

  it("就绪判定：到点才就绪、停用不就绪、从未跑过立即就绪", () => {
    expect(dueTasks(tasks, DAY - 1)).toHaveLength(0);
    const due = dueTasks(tasks, DAY + 100);
    expect(due.map((d) => d.task.id).sort()).toEqual(["a", "b", "c"]);
    const fresh = [{ ...tasks[0]!, lastRunAt: null }];
    expect(dueTasks(fresh, 0)).toHaveLength(1); // 从未跑 = 立即就绪。
  });

  it("公平序：优先级降序、同级最饿先吃（迟到多的在前）", () => {
    const due = dueTasks(tasks, DAY * 2 + 50); // a 迟到 ~1 天；b/c 同级。
    const order = fairOrder(due).map((d) => d.task.id);
    expect(order[0]).toBe("b"); // 同级且同样迟到 → b/c 按 overdue 稳定序，优先级 5 组在前。
    expect(order).not.toContain("off");
  });

  it("错过策略三选一：skip 补 1、catchup 补多（封顶 8）、once 合并 1", () => {
    const due = dueTasks(tasks, DAY * 20);
    const plan = executionPlan(due, DAY * 20);
    const byId = Object.fromEntries(plan.map((p) => [p.id, p]));
    expect(byId["a"]!.runs).toBe(1); // skip。
    expect(byId["b"]!.runs).toBe(1); // once。
    expect(byId["c"]!.runs).toBe(Math.min(8, missedCount(tasks[2]!, DAY * 20))); // catchup 封顶。
    expect(byId["c"]!.note).toContain("catchup");
  });

  it("错过次数数学：20 天整 → 20 个到期点（lastRunAt=0 是合法时刻）", () => {
    expect(missedCount(tasks[0]!, DAY * 20)).toBe(20);
  });

  it("下次时刻：首个未来周期点（已到期的本周期跳过——现在就该跑的不叫'下次'）", () => {
    const t: ScheduledTask = { id: "x", intervalMs: 1000, priority: 1, missed: "skip", lastRunAt: 500, enabled: true };
    expect(nextRunAt(t, 700)).toBe(1500);
    expect(nextRunAt(t, 1500)).toBe(2500); // 恰在到期点 → 下下个。
    expect(nextRunAt({ ...t, lastRunAt: null }, 42)).toBe(1042);
  });

  it("唤醒恢复：睡眠跨周期按各自策略给显性步骤（停用项不进计划）", () => {
    const plan = wakeRecoveryPlan(tasks, 0, DAY * 5);
    expect(plan).toHaveLength(3); // off 停用——不进计划（显性设计）。
    const a = plan.find((p) => p.id === "a")!;
    expect(a.action).toContain("错过即跳过"); // skip 策略的人话面。
  });

  it("错过策略说明三态齐备（十章一致性——词典存在）", () => {
    expect(Object.keys(MISSED_POLICY_NOTE)).toEqual(["skip", "catchup", "once"]);
  });
});

// ---------- usage-model ----------

describe("usage-model · EMA 排名（F167 深化）", () => {
  const now = 100 * DAY;
  function events(): UsageEvent[] {
    const out: UsageEvent[] = [];
    for (let d = 30; d >= 1; d--) {
      if (d > 20) {
        // archiver：前 10 天每天 5 次，之后彻底停用（旧习惯的典型画像）。
        for (let i = 0; i < 5; i++) out.push({ itemId: "archiver", at: now - d * DAY + i * 100 });
      }
      out.push({ itemId: "editor", at: now - d * DAY + 100 }); // editor 稳定每天。
      if (d % 3 === 0) out.push({ itemId: "scan", at: now - d * DAY + 200 });
    }
    return out;
  }

  it("EMA 衰减：近期稳定使用胜过早期狂点（排名反映最近习惯）", () => {
    const ranked = rankEvents(events(), now);
    expect(ranked[0]!.itemId).toBe("editor"); // archiver 已 20 天没用。
    expect(ranked[0]!.score).toBeGreaterThan(ranked[1]!.score);
  });

  it("原始计数对拍面：archiver 总次数 > editor（两源对账口径）", () => {
    const ranked = rankEvents(events(), now);
    const archiver = ranked.find((r) => r.itemId === "archiver")!;
    const editor = ranked.find((r) => r.itemId === "editor")!;
    expect(archiver.rawCount).toBeGreaterThan(editor.rawCount);
    expect(archiver.score).toBeLessThan(editor.score); // 但分数输给近期使用。
  });

  it("topK 裁掉零分项；钉选置顶不占公平名额的顺序保证", () => {
    const ranked = rankEvents([...events(), { itemId: "ghost", at: now - 90 * DAY }], now);
    // ghost 90 天前一次、半衰期 7 天 → 分数趋零被裁。
    expect(topK(ranked, 10).some((r) => r.itemId === "ghost")).toBe(false);
    const pinned = withPinned(ranked, new Set(["editor"]), 3);
    expect(pinned[0]!.itemId).toBe("editor"); // 钉选是承诺——永远最前。
    expect(pinned).toHaveLength(3);
  });

  it("导出零内容字段 + 两源对账：一致通过、偏差逐条列出", () => {
    const ranked = rankEvents(events(), now);
    const exported = exportRanking(ranked, now);
    expect(exported.items.every((i) => !("content" in i) && !("fileName" in i))).toBe(true);
    expect(reconcileWithExternal(ranked, exported).consistent).toBe(true);
    const drifted = { ...exported, items: exported.items.map((i) => ({ ...i, score: i.score * 2 })) };
    const bad = reconcileWithExternal(ranked, drifted);
    expect(bad.consistent).toBe(false);
    expect(bad.mismatches.length).toBeGreaterThan(0);
  });
});

// ---------- cheatsheet ----------

describe("cheatsheet · 速查表与走查（F169 深化）", () => {
  const bindings = [
    { actionId: "system.save", sequence: ["Ctrl+S"], enabled: true },
    { actionId: "system.quit", sequence: ["Ctrl+Q"], enabled: true },
    { actionId: "app.copy", sequence: ["Ctrl+K Ctrl+C"], enabled: true },
    { actionId: "app.paste", sequence: ["Ctrl+K Ctrl+V"], enabled: true },
    { actionId: "app.disabled", sequence: ["Ctrl+X"], enabled: false },
  ];

  it("分组：系统组在前、停用项不进表", () => {
    const sheet = buildCheatSheet(bindings);
    expect(sheet[0]!.scope).toBe("system");
    expect(sheet.flatMap((g) => g.entries).some((e) => e.actionId === "app.disabled")).toBe(false);
  });

  it("打印栅格：三列行数均衡（列高差 ≤1）", () => {
    const entries = Array.from({ length: 10 }, (_, i) => ({ combo: `K${i}`, actionId: `a${i}` }));
    const grid = printableGrid(entries, 3);
    expect(grid).toHaveLength(3);
    const heights = grid.map((c) => c.length);
    expect(Math.max(...heights) - Math.min(...heights)).toBeLessThanOrEqual(1);
    expect(grid.flat()).toHaveLength(10);
  });

  it("占用矩阵：冲突暴露、保留组合在册、空闲建议", () => {
    const dup = [...bindings, { actionId: "app.save2", sequence: ["Ctrl+S"], enabled: true }];
    const matrix = occupancyMatrix(dup, ["Alt+F4"]);
    expect(matrix.conflicts).toEqual([{ combo: "Ctrl+S", holders: ["system.save", "app.save2"] }]);
    expect(matrix.reserved).toEqual(["Alt+F4"]);
    const suggest = suggestFreeCombos(matrix, "Ctrl+K", ["Ctrl+C", "Ctrl+X"]);
    expect(suggest.taken).toEqual(["Ctrl+K Ctrl+C"]); // 两段序列首段已被占（键序契约：空格分段）。
    expect(suggest.free).toEqual(["Ctrl+K Ctrl+X"]);
  });

  it("盲走脚本：正向 Tab × N + 反向 + Enter + Esc，证据字段五件", () => {
    const walk = blindWalkScript("settings", 4);
    expect(walk.steps).toHaveLength(7); // 4 正向 + 反向 + Enter + Esc。
    expect(walk.steps[0]!.input).toEqual(["Tab"]);
    expect(walk.steps[4]!.input).toEqual(["Shift", "Tab"]);
    expect(walk.steps[6]!.input).toEqual(["Escape"]);
    expect(walk.evidenceFields).toContain("体验结论");
  });
});

// ---------- archive-diff ----------

describe("archive-diff · 结构 diff 与三方合并（F161 深化）", () => {
  it("递归 diff：新增/删除/修改三分类、路径逐叶可读", () => {
    const a = { accent: "#111111", density: "standard", group: { deep: 1, gone: 2 } };
    const b = { accent: "#222222", density: "standard", group: { deep: 1, added: 3 } };
    const diffs = structuralDiff(a, b);
    const kinds = Object.fromEntries(diffs.map((d) => [d.path, d.kind]));
    expect(kinds["accent"]).toBe("changed");
    expect(kinds["group.gone"]).toBe("removed");
    expect(kinds["group.added"]).toBe("added");
    expect(describeDiff(diffs, "zh")[0]).toContain("修改 accent");
  });

  it("三方合并：单侧改动自动取、双侧同改去重、双改不同=冲突", () => {
    const base = { a: 1, b: 1, c: 1, nested: { x: 1 } };
    const mine = { a: 2, b: 1, c: 3, nested: { x: 1 } }; // a 我改、c 我改。
    const theirs = { a: 1, b: 2, c: 9, nested: { x: 5 } }; // b 对方改、c 对方也改（冲突）、nested.x 对方改。
    const r = threeWayMerge(base, mine, theirs);
    expect(r.merged.a).toBe(2); // 我改 → 我的。
    expect(r.merged.b).toBe(2); // 对方改 → 对方的。
    expect((r.merged.nested as { x: number }).x).toBe(5); // 嵌套自动合并。
    const conflict = r.conflicts.find((c) => c.path === "c");
    expect(conflict).toBeDefined();
    expect(conflict!.mine).toBe(3);
    expect(conflict!.theirs).toBe(9);
  });

  it("双侧同值 = 自动去重不是冲突；合并深度保护", () => {
    const r = threeWayMerge({ a: 1 }, { a: 9 }, { a: 9 });
    expect(r.conflicts).toHaveLength(0);
    expect(r.merged.a).toBe(9);
    const loop: Record<string, unknown> = {};
    loop.self = loop;
    expect(() => threeWayMerge({}, loop, loop)).toThrow(); // 循环引用保护。
  });
});

// ---------- walkcheck-runner ----------

describe("walkcheck-runner · 分片执法（F170 深化）", () => {
  function jobs(): ProbeJob[] {
    return [
      { id: "j1", estMs: 2000, resourceGroup: "g1", run: () => ({ ok: true, detail: "ok" }) },
      { id: "j2", estMs: 2000, resourceGroup: "g1", run: () => ({ ok: false, detail: "红" }) },
      { id: "j3", estMs: 2000, resourceGroup: "g2", run: () => ({ ok: true, detail: "ok" }) },
      { id: "j4", estMs: 2000, resourceGroup: "g2", run: () => { throw new Error("boom"); } },
    ];
  }

  it("分片：同资源组同片（串行互不踩）、片宽预算决定片数", () => {
    const shards = shardJobs(jobs(), 4_000);
    expect(shards.length).toBeGreaterThanOrEqual(2);
    // g1 的两项永远同片。
    const g1Shard = shards.find((s) => s.jobs.some((j) => j.id === "j1"))!;
    expect(g1Shard.jobs.some((j) => j.id === "j2")).toBe(true);
  });

  it("执行：异常按项捕获（片不死）、红绿计数准确", () => {
    const shards = shardJobs(jobs(), 100_000);
    const results = shards.map(runShard);
    const flat = results.flatMap((r) => r.results);
    expect(flat.find((x) => x.id === "j4")!.ok).toBe(false);
    expect(flat.find((x) => x.id === "j4")!.detail).toContain("boom");
    const crashedShard = results.find((r) => r.crashedJobs.includes("j4"))!;
    const original = shards.find((s) => s.index === crashedShard.shard)!;
    expect(crashedShard.results).toHaveLength(original.jobs.length); // 片内其他项照跑（崩溃不传染）。
    const outcome = runAll(jobs(), 1_800_000);
    expect(outcome.passCount).toBe(2);
    expect(outcome.failCount).toBe(2);
    expect(outcome.withinBudget).toBe(true);
  });

  it("断点续跑：绿项跳过、失效项重跑（修复验证分钟级）", () => {
    const outcome = runAll(jobs());
    const passedIds = new Set(outcome.results.flatMap((r) => r.results.filter((x) => x.ok).map((x) => x.id)));
    const next = incrementalJobs(jobs(), { passedIds });
    expect(next.map((j) => j.id).sort()).toEqual(["j2", "j4"]); // 只跑红项。
    const withInvalid = incrementalJobs(jobs(), { passedIds: new Set(["j1", "j2", "j3", "j4"]) }, new Set(["j1"]));
    expect(withInvalid.map((j) => j.id)).toEqual(["j1"]); // 绿但失效 → 重跑。
  });
});
