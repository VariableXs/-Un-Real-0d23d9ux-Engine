import { describe, expect, it } from "vitest";
import { boxIntersectsRect, computeGuides, rotatePt } from "../geometry";

describe("rotatePt", () => {
  it("rotates around a center", () => {
    const c = { x: 10, y: 10 };
    const p = rotatePt({ x: 20, y: 10 }, c, 90);
    expect(p.x).toBeCloseTo(10);
    expect(p.y).toBeCloseTo(20);
  });
  it("zero rotation is identity", () => {
    const p = { x: 3, y: 4 };
    expect(rotatePt(p, { x: 0, y: 0 }, 0)).toEqual(p);
  });
});

describe("computeGuides", () => {
  const others = [
    { id: "a", x: 0, y: 0, width: 100, height: 50 },
    { id: "b", x: 300, y: 200, width: 120, height: 60 },
  ];

  it("detects vertical center alignment within threshold and reports gap", () => {
    // moving box centered on 'a' centerX (50)
    const g = computeGuides(
      { id: "m", x: 20, y: 80, width: 60, height: 40 },
      others,
      6,
    );
    const v = g.find((x) => x.axis === "v");
    expect(v).toBeDefined();
    expect(v!.at).toBeCloseTo(50); // a.centerX
    // horizontal clearance between facing edges: min(|20-100|, |0-80|) = 80
    expect(v!.gap).toBe(80);
  });

  it("no guides when far from everything", () => {
    const g = computeGuides(
      { id: "m", x: 900, y: 900, width: 60, height: 40 },
      others,
      6,
    );
    expect(g).toHaveLength(0);
  });

  it("flush edge alignment has no gap annotation", () => {
    // moving top edge exactly at a.bottom (y=50): touching → gap omitted
    const g = computeGuides(
      { id: "m", x: 5, y: 50, width: 80, height: 30 },
      others,
      6,
    );
    const h = g.find((x) => x.axis === "h");
    expect(h).toBeDefined();
    expect(h!.at).toBeCloseTo(50);
    expect(h!.gap).toBeUndefined();
  });

  it("ignores self by id", () => {
    const self = { id: "a", x: 0, y: 0, width: 100, height: 50 };
    expect(computeGuides(self, [self], 10)).toHaveLength(0);
  });
});

describe("boxIntersectsRect", () => {
  const r = { x0: 0, y0: 0, x1: 100, y1: 100 };
  it("inside", () => expect(boxIntersectsRect({ x: 10, y: 10, width: 5, height: 5 }, r)).toBe(true));
  it("overlapping edge", () => expect(boxIntersectsRect({ x: 95, y: 10, width: 20, height: 5 }, r)).toBe(true));
  it("outside", () => expect(boxIntersectsRect({ x: 200, y: 200, width: 10, height: 10 }, r)).toBe(false));
});
