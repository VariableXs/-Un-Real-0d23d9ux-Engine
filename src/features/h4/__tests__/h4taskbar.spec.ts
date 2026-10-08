/**
 * H4 深化批次八（v8）深测：任务栏挂载层纯模型（taskbarLayer.ts）。
 * 判据锚定：F363 徽标实时性 / F366 三档渲染与低电三态 / F369 中心徽标 / F373 任务栏指示；
 * 零编造（无数据源不出芯片）与契约执法（点击事件必须已登记）的机检面。
 */
import { describe, expect, it } from "vitest";
import { BINDINGS } from "../consumers";
import { buildTaskbarChips, focusAbandonConfirm, type TaskbarChipInput } from "../taskbarLayer";
import type { FocusRun } from "../../../system/h4/f363-focusTimer";
import type { BatteryReading } from "../../../system/h4/f366-trayBattery";

const run = (plannedMinutes = 25, startedAt = 0, outcome: FocusRun["outcome"] = "running"): FocusRun => ({
  day: "2026-09-26",
  plannedMinutes,
  startedAt,
  endedAt: null,
  outcome,
});

const battery = (percent: number, plugged = false): BatteryReading => ({ percent, plugged });

const base: TaskbarChipInput = {
  focusRun: null,
  now: 25 * 60 * 1000, // 25 分钟整
  layoutName: "中文（拼音）",
  battery: null,
  batteryMode: "iconPercent",
  taskBadge: 0,
};

describe("v8 任务栏芯片模型：零编造（无数据源不出芯片）", () => {
  it("全部无源：只剩布局指示恒显", () => {
    const chips = buildTaskbarChips(base);
    expect(chips.map((c) => c.id)).toEqual(["layout"]);
    expect(chips[0]!.text).toBe("中文（拼音）");
  });

  it("专注进行中 → mm:ss 徽标；放弃/完成后消失（F363 徽标实时性）", () => {
    const active = buildTaskbarChips({ ...base, focusRun: run() });
    expect(active[0]!.id).toBe("focus");
    expect(active[0]!.text).toBe("00:00"); // 25 分钟整点 → 00:00? 归因：remaining=0 触底
    const mid = buildTaskbarChips({ ...base, focusRun: run(25, 5 * 60 * 1000), now: 10 * 60 * 1000 });
    expect(mid[0]!.text).toBe("20:00");
    const done = buildTaskbarChips({ ...base, focusRun: run(25, 0, "completed") });
    expect(done.map((c) => c.id)).toEqual(["layout"]);
  });

  it("任务徽标：运行数 >0 才出芯片", () => {
    expect(buildTaskbarChips({ ...base, taskBadge: 3 }).map((c) => c.id)).toEqual(["tasks", "layout"]);
    expect(buildTaskbarChips({ ...base, taskBadge: 0 }).some((c) => c.id === "tasks")).toBe(false);
  });

  it("电池：无真实源不出芯片（零编造）；有源按档渲染", () => {
    expect(buildTaskbarChips(base).some((c) => c.id === "battery")).toBe(false);
    const withB = buildTaskbarChips({ ...base, battery: battery(80) });
    const b = withB.find((c) => c.id === "battery")!;
    expect(b.text).toBe("80%");
    expect(b.tone).toBe("none");
  });
});

describe("v8 任务栏芯片模型：顺序词典与低电三态", () => {
  const full: TaskbarChipInput = { ...base, focusRun: run(25, 20 * 60 * 1000), taskBadge: 2, battery: battery(8) };

  it("顺序固定：专注 → 任务 → 布局 → 电池（交互词典）", () => {
    expect(buildTaskbarChips(full).map((c) => c.id)).toEqual(["focus", "tasks", "layout", "battery"]);
  });

  it("低电三态独立视觉：critical 红 + 闪烁（F366 判据）", () => {
    const b = buildTaskbarChips(full).find((c) => c.id === "battery")!;
    expect(b.tone).toBe("red");
    expect(b.blink).toBe(true);
    const warn = buildTaskbarChips({ ...full, battery: battery(18) }).find((c) => c.id === "battery")!;
    expect(warn.tone).toBe("yellow");
    expect(warn.blink).toBe(false);
    const plugged = buildTaskbarChips({ ...full, battery: battery(18, true) }).find((c) => c.id === "battery")!;
    expect(plugged.tone).toBe("none"); // 插电 → 低电态关闭
  });

  it("点击事件全部已登记（契约执法总线的预设校验——未登记事件发不出去）", () => {
    const registered = new Set(BINDINGS.flatMap((x) => [...x.eventsIn, ...x.eventsOut]));
    for (const chip of buildTaskbarChips(full)) {
      if (chip.clickEvent) expect(registered.has(chip.clickEvent)).toBe(true);
    }
    expect(buildTaskbarChips(full).find((c) => c.id === "battery")!.clickEvent).toBeNull(); // 不假装可点
  });
});

describe("v8 专注放弃语义（F363 放弃真实性）", () => {
  it("真实记账：足 5 分钟按实记、不足按 1 分钟（有始有终不虚报）", () => {
    const long = focusAbandonConfirm(run(25, 0, "running"), 10 * 60 * 1000);
    expect(long.needConfirm).toBe(false);
    expect(long.reason).toContain("10 分钟");
    const short = focusAbandonConfirm(run(25, 0, "running"), 2 * 60 * 1000);
    expect(short.reason).toContain("1 分钟");
  });
});
