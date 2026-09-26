import { describe, expect, it } from "vitest";
import {
  PLUG_CYCLE_TEST_ROUNDS,
  clampIntoWorkArea,
  planReattach,
  planReflow,
  plugCycleStability,
  rememberPlacement,
  type DisplayInfo,
  type MemoryState,
  type TrackedWindow,
} from "../f353-crossScreenMemory";

const MAIN: DisplayInfo = { id: "main", x: 0, y: 0, w: 1920, h: 1040, primary: true };
const SIDE: DisplayInfo = { id: "side", x: 1920, y: 0, w: 1280, h: 720, primary: false };

function state(): MemoryState {
  return { memories: [], absenceNotified: [] };
}

const WIN: TrackedWindow = { winId: "w1", x: 2000, y: 100, w: 800, h: 600, displayId: "side" };

describe("F353 跨屏拖拽位置记忆", () => {
  it("拖拽落位记账：同窗同屏覆盖保鲜，不同屏并存", () => {
    let s = rememberPlacement(state(), WIN, 1);
    s = rememberPlacement(s, { ...WIN, x: 2100 }, 2);
    expect(s.memories.filter((m) => m.displayId === "side")).toHaveLength(1);
    expect(s.memories[0]!.x).toBe(2100);
    s = rememberPlacement(s, { ...WIN, displayId: "main", x: 10 }, 3);
    expect(s.memories).toHaveLength(2);
  });

  it("副屏拔除：窗口钳制回流主屏工作区，预算内，缺席提示只发一次", () => {
    let s = rememberPlacement(state(), WIN, 1);
    const p1 = planReflow(s, "side", [MAIN, SIDE]);
    expect(p1.entries).toHaveLength(1);
    expect(p1.entries[0]!.to).toEqual({ x: 1920 - 800, y: 100, w: 800, h: 600 });
    expect(p1.notifyAbsence).toBe(true);
    expect(p1.budgetOk).toBe(true);
    const p2 = planReflow(p1.nextState, "side", [MAIN]);
    expect(p2.notifyAbsence).toBe(false); // 提示一次判据
  });

  it("回流钳制：窗口比主屏大 → 先缩尺寸再钳位（F214 降级序）", () => {
    const huge = { x: 5000, y: 5000, w: 4000, h: 3000 };
    expect(clampIntoWorkArea(huge, { x: 0, y: 0, w: 1920, h: 1040 })).toEqual({ x: 0, y: 0, w: 1920, h: 1040 });
    const off = { x: -100, y: -100, w: 800, h: 600 };
    const c = clampIntoWorkArea(off, { x: 0, y: 0, w: 1920, h: 1040 });
    expect(c).toEqual({ x: 0, y: 0, w: 800, h: 600 });
  });

  it("重接副屏：原几何完整放下 → 逐字段回原位（<1px 同义）", () => {
    let s = rememberPlacement(state(), WIN, 1);
    const r = planReattach(s, SIDE);
    expect(r[0]!.restoredExactly).toBe(true);
    expect(r[0]!.rect).toEqual({ x: 2000, y: 100, w: 800, h: 600 });
    // 放不下 → 不动并如实报告
    const big = rememberPlacement(state(), { ...WIN, w: 2000, h: 900 }, 1);
    const r2 = planReattach(big, SIDE);
    expect(r2[0]!.restoredExactly).toBe(false);
    expect(r2[0]!.rect).toBeNull();
  });

  it(`插拔循环稳定性：${PLUG_CYCLE_TEST_ROUNDS} 轮后记忆幂等`, () => {
    const r = plugCycleStability(state(), WIN, SIDE, 1);
    expect(r.rounds).toBe(PLUG_CYCLE_TEST_ROUNDS);
    expect(r.stable).toBe(true);
  });
});
