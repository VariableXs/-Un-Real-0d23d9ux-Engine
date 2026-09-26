import { describe, expect, it } from "vitest";
import { GRID_OPACITY, GRID_STEP_PX, MEASURE_FRAME_BUDGET_MS, beginMeasure, clickThrough, escapeExit, gridAlignment, gridPaintSpec, initialOverlay, measureReadout, rulerReadout, updateMeasure } from "../f360-pixelRuler";

describe("F360 像素标尺与网格叠加", () => {
  it("量测：起点落定→拖拽实时更新；负方向归一化（往左上拖也成矩形）", () => {
    let s = beginMeasure(initialOverlay("all"), { x: 100, y: 200 });
    expect(s.measureRect).toEqual({ x: 100, y: 200, w: 0, h: 0 });
    s = updateMeasure(s, { x: 340, y: 120 });
    expect(s.measureRect).toEqual({ x: 100, y: 120, w: 240, h: 80 });
    s = updateMeasure(s, { x: 40, y: 320 });
    expect(s.measureRect).toEqual({ x: 40, y: 200, w: 60, h: 120 });
    expect(measureReadout(s.measureRect)).toEqual({ w: 60, h: 120 });
  });

  it("读数零误差：读数恒等于几何差值（判据「已知尺寸窗口零误差」）", () => {
    const s = updateMeasure(beginMeasure(initialOverlay("measure"), { x: 0, y: 0 }), { x: 640, y: 480 });
    expect(measureReadout(s.measureRect)).toEqual({ w: 640, h: 480 });
  });

  it("Esc 秒退：任意模式回 off 且量测清零", () => {
    const s = updateMeasure(beginMeasure(initialOverlay("all"), { x: 1, y: 1 }), { x: 9, y: 9 });
    const after = escapeExit(s);
    expect(after.mode).toBe("off");
    expect(after.measureRect).toBeNull();
    expect(after.measureStart).toBeNull();
  });

  it("点击穿透：所有状态放行底层（判据）", () => {
    expect(clickThrough(initialOverlay("off"))).toBe(true);
    expect(clickThrough(initialOverlay("grid"))).toBe(true);
  });

  it("网格对齐：8px 步长、偏差读数、对齐判定", () => {
    expect(GRID_STEP_PX).toBe(8);
    expect(gridAlignment(64)).toEqual({ nearest: 64, offset: 0, aligned: true });
    expect(gridAlignment(66)).toEqual({ nearest: 64, offset: 2, aligned: false });
    expect(gridAlignment(60).nearest).toBe(64);
  });

  it("网格绘制参数：透明度 20%（判据）、步长 8", () => {
    const spec = gridPaintSpec();
    expect(spec.opacity).toBe(GRID_OPACITY);
    expect(spec.opacity).toBe(0.2);
    expect(spec.step).toBe(8);
  });

  it("标尺读数=坐标本身；实时性预算 ≤1 帧", () => {
    expect(rulerReadout({ x: 123, y: 456 })).toEqual({ horizontal: 123, vertical: 456 });
    expect(MEASURE_FRAME_BUDGET_MS).toBeLessThanOrEqual(16);
  });
});
