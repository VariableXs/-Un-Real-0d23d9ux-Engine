import { beforeEach, describe, expect, it } from "vitest";
import {
  STAGE_HOTZONE_OFFSET_PX,
  addToStage,
  createStage,
  deleteStage,
  disbandStage,
  loadStages,
  nextStageId,
  planStageActivate,
  removeFromStage,
  renameStage,
} from "../stages";
import type { VwmWin } from "../vwm";

function win(id: string): VwmWin {
  return {
    id,
    app: "write",
    path: null,
    x: 0,
    y: 0,
    w: 800,
    h: 600,
    state: "normal",
    minimized: false,
    hidden: false,
    z: 1,
    restore: null,
    group: null,
    groupActive: false,
    rolledUp: false,
    minimizedAt: null,
    opacity: 1,
    topmost: false,
  };
}

beforeEach(() => localStorage.clear());

describe("N-02 舞台组管理", () => {
  it("建组 → 重启（重读持久化）→ 组关系完整（验收 ①）", () => {
    const g = createStage("写作组");
    addToStage(g.id, "w1");
    addToStage(g.id, "w2");
    const loaded = loadStages();
    expect(loaded).toHaveLength(1);
    expect(loaded[0]!.members).toEqual(["w1", "w2"]);
  });

  it("入组即从其它组移除（一个窗口同时只属一个组）", () => {
    const a = createStage("A");
    const b = createStage("B");
    addToStage(a.id, "w1");
    addToStage(b.id, "w1");
    expect(loadStages().find((g) => g.id === a.id)?.members).toEqual([]);
    expect(loadStages().find((g) => g.id === b.id)?.members).toEqual(["w1"]);
  });

  it("移出成员 / 重命名 / 删除组", () => {
    const g = createStage("旧名");
    addToStage(g.id, "w1");
    removeFromStage(g.id, "w1");
    expect(loadStages()[0]!.members).toEqual([]);
    renameStage(g.id, "新名");
    expect(loadStages()[0]!.name).toBe("新名");
    deleteStage(g.id);
    expect(loadStages()).toHaveLength(0);
  });

  it("解散组不影响成员窗口本体（只清组记录）", () => {
    const g = createStage("A");
    addToStage(g.id, "w1");
    disbandStage(g.id);
    expect(loadStages()).toHaveLength(0);
  });
});

describe("N-02 上台计划", () => {
  it("成员上台（按成员序聚焦）、非成员收拢侧幕", () => {
    const g = createStage("组");
    addToStage(g.id, "w2");
    addToStage(g.id, "w1");
    const plan = planStageActivate(g.id, [win("w1"), win("w2"), win("w3")]);
    expect(plan.focusOrder).toEqual(["w2", "w1"]); // 成员列表序
    expect(plan.sideline).toEqual(["w3"]);
  });

  it("组不存在 → 空计划（诚实，无副作用）", () => {
    expect(planStageActivate("nope", [win("w1")])).toEqual({ focusOrder: [], sideline: [] });
  });

  it("组间轮换不循环：端点返回 null（行为可预期）", () => {
    const a = createStage("A");
    const b = createStage("B");
    expect(nextStageId(a.id)).toBe(b.id);
    expect(nextStageId(b.id)).toBeNull(); // 不循环
    expect(nextStageId(b.id, true)).toBe(a.id);
    expect(nextStageId(null)).toBe(a.id);
  });

  it("侧幕热区与贴靠热区错开 24px 常量在位", () => {
    expect(STAGE_HOTZONE_OFFSET_PX).toBe(24);
  });
});
