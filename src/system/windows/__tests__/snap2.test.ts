import { describe, expect, it } from "vitest";
import { ZONE_LIBRARY, predictZone, zoneRect } from "../snap2";
import type { VwmRect } from "../vwm";

const WA: VwmRect = { x: 0, y: 0, w: 1920, h: 1080 };

describe("U-14 分区方案库", () => {
  it("规格五种方案齐备", () => {
    expect(ZONE_LIBRARY.map((z) => z.id)).toEqual(["half", "thirds", "two-plus-one", "quadrants", "grid"]);
  });

  it("半屏左右：宽度各半、铺满高度", () => {
    const l = zoneRect("half-left", WA);
    const r = zoneRect("half-right", WA);
    expect(l).toEqual({ x: 0, y: 0, w: 960, h: 1080 });
    expect(r).toEqual({ x: 960, y: 0, w: 960, h: 1080 });
    expect(l.w + r.w).toBe(WA.w);
  });

  it("三等分：三列宽度铺满无重叠（含取整余数吸收）", () => {
    const odd: VwmRect = { x: 0, y: 0, w: 1000, h: 800 };
    const cols = [zoneRect("third-1", odd), zoneRect("third-2", odd), zoneRect("third-3", odd)];
    expect(cols.reduce((s, c) => s + c.w, 0)).toBe(1000);
    expect(cols[2]!.x + cols[2]!.w).toBe(1000);
  });

  it("2+1：左侧上下两块 + 右侧通高", () => {
    const tl = zoneRect("two-plus-one-left", WA);
    const right = zoneRect("two-plus-one-right", WA);
    expect(tl.x).toBe(0);
    expect(tl.w).toBe(640);
    expect(right.x).toBe(1280);
    expect(right.h).toBe(1080);
  });

  it("四象限：四块拼满工作区", () => {
    const q = [
      zoneRect("quadrant-tl", WA),
      zoneRect("quadrant-tr", WA),
      zoneRect("quadrant-bl", WA),
      zoneRect("quadrant-br", WA),
    ];
    expect(q.reduce((s, c) => s + c.w * c.h, 0)).toBe(WA.w * WA.h);
  });

  it("自定义网格：钳制到 3×3", () => {
    expect(() => zoneRect({ grid: { cols: 9, rows: 9, col: 5, row: 5 } }, WA)).not.toThrow();
    const g = zoneRect({ grid: { cols: 9, rows: 9, col: 5, row: 5 } }, WA);
    expect(g.w).toBe(640);
    expect(g.h).toBe(360);
  });
});

describe("U-14 predictZone", () => {
  it("Shift 按住 → 临时禁用吸附（null）", () => {
    expect(predictZone(0, 0, WA, { shiftHeld: true })).toBeNull();
  });

  it("左缘中部 → 半屏；左缘上/下三分之一 → 三分列", () => {
    expect(predictZone(10, 540, WA)?.layout).toBe("half-left");
    expect(predictZone(10, 100, WA)?.layout).toBe("third-1");
    expect(predictZone(10, 1000, WA)?.layout).toBe("third-3");
  });

  it("角落 → 四象限", () => {
    expect(predictZone(5, 5, WA)?.layout).toBe("quadrant-tl");
    expect(predictZone(1915, 1075, WA)?.layout).toBe("quadrant-br");
  });

  it("屏幕中央 → 无预测（null）", () => {
    expect(predictZone(960, 540, WA)).toBeNull();
  });

  it("预测 rect 与 zoneRect 落位同源（幽灵预览与最终落位 0px 偏差）", () => {
    const p = predictZone(10, 540, WA);
    expect(p?.rect).toEqual(zoneRect("half-left", WA));
  });
});
