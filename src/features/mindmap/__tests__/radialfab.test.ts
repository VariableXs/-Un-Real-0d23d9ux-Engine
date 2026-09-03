import { describe, expect, it } from "vitest";
import { radialPositions } from "../RadialFab";

describe("radialPositions —— 径向菜单几何", () => {
  it("默认 90°→180° 弧线：首项正上方、末项正左方（屏幕坐标 y 向下）", () => {
    const pts = radialPositions(6, 100);
    expect(pts).toHaveLength(6);
    // 首项：90° → (cos90, -sin90)*r = (0, -r) 正上方
    expect(pts[0]!.x).toBeCloseTo(0, 5);
    expect(pts[0]!.y).toBeCloseTo(-100, 5);
    // 末项：180° → (-r, 0) 正左方
    expect(pts[5]!.x).toBeCloseTo(-100, 5);
    expect(pts[5]!.y).toBeCloseTo(0, 5);
  });

  it("每项到圆心的距离恒等于半径", () => {
    for (const pts of [radialPositions(2, 78), radialPositions(6, 78), radialPositions(9, 120)]) {
      for (const p of pts) expect(Math.hypot(p.x, p.y)).toBeCloseTo(78 + (pts.length === 9 ? 42 : 0), 5);
    }
  });

  it("角度沿弧线单调递增（逆时针、向左上展开）", () => {
    const pts = radialPositions(5, 80);
    for (let i = 1; i < pts.length; i++) {
      const a0 = Math.atan2(-pts[i - 1]!.y, pts[i - 1]!.x);
      const a1 = Math.atan2(-pts[i]!.y, pts[i]!.x);
      expect(a1).toBeGreaterThan(a0);
    }
  });

  it("count ≤ 1 / 非法输入：空数组或回到圆心", () => {
    expect(radialPositions(0, 80)).toEqual([]);
    expect(radialPositions(-3, 80)).toEqual([]);
    expect(radialPositions(3, -1)).toEqual([]);
    expect(radialPositions(Number.NaN, 80)).toEqual([]);
    const one = radialPositions(1, 80);
    expect(one).toHaveLength(1);
    expect(one[0]!.x).toBeCloseTo(0, 5);
    expect(one[0]!.y).toBeCloseTo(-80, 5);
  });

  it("自定义角度区间：0°→90° 时首项在右、末项在上", () => {
    const pts = radialPositions(3, 50, 0, 90);
    expect(pts[0]!.x).toBeCloseTo(50, 5);
    expect(pts[0]!.y).toBeCloseTo(0, 5);
    expect(pts[2]!.x).toBeCloseTo(0, 5);
    expect(pts[2]!.y).toBeCloseTo(-50, 5);
  });
});
