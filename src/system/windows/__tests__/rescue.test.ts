import { describe, expect, it } from "vitest";
import { healthCheck, isVisibleInWorkArea, rescueRect } from "../rescue";
import type { VwmRect, VwmWin } from "../vwm";

const WA: VwmRect = { x: 0, y: 0, w: 1920, h: 1046 };

function win(p: Partial<VwmWin> & { id: string; x: number; y: number; w: number; h: number }): VwmWin {
  return {
    app: "write",
    path: null,
    state: "normal",
    minimized: false,
    z: 1,
    restore: null,
    group: null,
    groupActive: false,
    ...p,
  } as VwmWin;
}

describe("V-22 isVisibleInWorkArea", () => {
  it("可见窗口返回 true", () => {
    expect(isVisibleInWorkArea({ x: 100, y: 100, w: 800, h: 600 }, WA)).toBe(true);
  });
  it("完全在右侧外部返回 false", () => {
    expect(isVisibleInWorkArea({ x: 2000, y: 100, w: 800, h: 600 }, WA)).toBe(false);
  });
  it("仅 1px 交差不算可见（面积 0）", () => {
    expect(isVisibleInWorkArea({ x: 1920, y: 0, w: 100, h: 100 }, WA)).toBe(false);
  });
});

describe("V-22 rescueRect", () => {
  it("右侧失联：保持尺寸贴右缘", () => {
    const r = rescueRect({ x: 2000, y: 200, w: 800, h: 600 }, WA);
    expect(r).toEqual({ x: 1920 - 800, y: 200, w: 800, h: 600 });
  });
  it("左侧失联：贴左缘", () => {
    const r = rescueRect({ x: -900, y: 200, w: 800, h: 600 }, WA);
    expect(r.x).toBe(0);
    expect(r.w).toBe(800);
  });
  it("下方失联：贴下缘", () => {
    const r = rescueRect({ x: 100, y: 2000, w: 800, h: 600 }, WA);
    expect(r.y).toBe(1046 - 600);
  });
  it("跨界窗口：夹取进工作区且不改尺寸", () => {
    const r = rescueRect({ x: 1800, y: 900, w: 800, h: 600 }, WA);
    expect(r.x).toBe(1920 - 800);
    expect(r.y).toBe(1046 - 600);
    expect(r.w).toBe(800);
    expect(r.h).toBe(600);
  });
  it("窗口比工作区还大：从工作区原点摆放、不缩放", () => {
    const r = rescueRect({ x: -100, y: -100, w: 3000, h: 2000 }, { x: 0, y: 0, w: 1920, h: 1046 });
    expect(r).toEqual({ x: 0, y: 0, w: 3000, h: 2000 });
  });
});

describe("V-22 healthCheck", () => {
  it("找出失联窗口并给出拉回几何；最小化窗口跳过", () => {
    const wins = [
      win({ id: "a", x: 100, y: 100, w: 500, h: 400 }),
      win({ id: "lost-right", x: 2500, y: 100, w: 400, h: 300 }),
      win({ id: "min-ok", x: 9999, y: 9999, w: 400, h: 300, minimized: true }),
    ];
    const rep = healthCheck(wins, WA);
    expect(rep.lost).toEqual(["lost-right"]);
    expect(rep.moved["lost-right"].x).toBe(1920 - 400);
    expect(rep.moved["min-ok"]).toBeUndefined();
  });
  it("全部可见时报告为空（零误判）", () => {
    const wins = [win({ id: "a", x: 0, y: 0, w: 500, h: 400 })];
    expect(healthCheck(wins, WA)).toEqual({ lost: [], moved: {} });
  });
  it("找回后不抢占焦点——报告不含焦点字段（契约：调用方不动 focusedId）", () => {
    const rep = healthCheck([win({ id: "a", x: -500, y: 0, w: 100, h: 100 })], WA);
    expect(Object.keys(rep)).toEqual(["lost", "moved"]);
  });
});
