import { describe, expect, it } from "vitest";
import {
  anchorsFor, circleDiameter, clampDims, computeEdge, distributedAnchor,
  growDimsForText, inscribedRect, MAX_ASPECT, MIN_NODE_H, MIN_NODE_W,
  nearestAnchor, pathD, pointInShape, polygonArea, sanitizeDims,
  shapeCollapsed, shapePoints, vertexDragSigns,
} from "../geometry";

describe("vertexDragSigns (polygon vertex handles)", () => {
  it("triangle apex maps to a pure north resize", () => {
    expect(vertexDragSigns("triangle", 100, 60, 0)).toEqual({ sx: 0, sy: -1 });
  });
  it("triangle base vertices map to south + horizontal sides", () => {
    expect(vertexDragSigns("triangle", 100, 60, 1)).toEqual({ sx: 1, sy: 1 });
    expect(vertexDragSigns("triangle", 100, 60, 2)).toEqual({ sx: -1, sy: 1 });
  });
  it("diamond vertices cover all four edge directions", () => {
    expect(vertexDragSigns("diamond", 100, 100, 0)).toEqual({ sx: 0, sy: -1 });
    expect(vertexDragSigns("diamond", 100, 100, 1)).toEqual({ sx: 1, sy: 0 });
    expect(vertexDragSigns("diamond", 100, 100, 2)).toEqual({ sx: 0, sy: 1 });
    expect(vertexDragSigns("diamond", 100, 100, 3)).toEqual({ sx: -1, sy: 0 });
  });
  it("regular polygon first vertex sits at the top center", () => {
    for (const shape of ["pentagon", "hexagon", "heptagon"] as const) {
      expect(vertexDragSigns(shape, 120, 120, 0)).toEqual({ sx: 0, sy: -1 });
    }
  });
  it("out-of-range index falls back to the first vertex", () => {
    expect(vertexDragSigns("diamond", 100, 100, 99)).toEqual(vertexDragSigns("diamond", 100, 100, 0));
  });
});

describe("shapePoints", () => {
  it("triangle has 3 vertices inside box", () => {
    const pts = shapePoints("triangle", 100, 60);
    expect(pts).toHaveLength(3);
    for (const p of pts) {
      expect(p.x).toBeGreaterThanOrEqual(0);
      expect(p.x).toBeLessThanOrEqual(100);
      expect(p.y).toBeGreaterThanOrEqual(0);
      expect(p.y).toBeLessThanOrEqual(60);
    }
  });
  it("heptagon has 7 vertices", () => {
    expect(shapePoints("heptagon", 120, 120)).toHaveLength(7);
  });
  it("hexagon has 6 vertices", () => {
    expect(shapePoints("hexagon", 90, 90)).toHaveLength(6);
  });
});

describe("anchors sit on edge midpoints (not corners)", () => {
  it("rect anchors are the four side midpoints", () => {
    const a = anchorsFor("rect", 200, 100);
    expect(a).toHaveLength(4);
    expect(a.some((x) => x.p.x === 100 && x.p.y === 0)).toBe(true); // top mid
    expect(a.some((x) => x.p.x === 100 && x.p.y === 100)).toBe(true); // bottom mid
    expect(a.some((x) => x.p.x === 0 && x.p.y === 50)).toBe(true); // left mid
    expect(a.some((x) => x.p.x === 200 && x.p.y === 50)).toBe(true); // right mid
    // no corner anchors
    expect(a.every((x) => !(x.p.x === 0 && x.p.y === 0))).toBe(true);
  });
  it("pentagon anchors = 5 edge midpoints", () => {
    expect(anchorsFor("pentagon", 100, 100)).toHaveLength(5);
  });
  it("circle provides 8 ring anchors", () => {
    expect(anchorsFor("circle", 80, 80)).toHaveLength(8);
  });
});

describe("nearestAnchor / distributedAnchor", () => {
  it("picks the closest side toward target point", () => {
    const best = nearestAnchor("rect", 200, 100, { x: 500, y: 50 });
    expect(best.p.x).toBe(200); // right side
  });
  it("distributes multiple edges along one side without overlapping", () => {
    const p1 = distributedAnchor({ width: 200, height: 100, shape: "rect" }, { x: 500, y: 50 }, 0, 3);
    const p2 = distributedAnchor({ width: 200, height: 100, shape: "rect" }, { x: 500, y: 50 }, 1, 3);
    const p3 = distributedAnchor({ width: 200, height: 100, shape: "rect" }, { x: 500, y: 50 }, 2, 3);
    expect(p1.y).toBeLessThan(p2.y);
    expect(p2.y).toBeLessThan(p3.y);
    expect(p1.x).toBeCloseTo(200);
  });
});

describe("circle stays circular", () => {
  it("diameter uses min(w,h)", () => {
    expect(circleDiameter(300, 120)).toBe(120);
    expect(circleDiameter(120, 300)).toBe(120);
  });
});

describe("edge paths", () => {
  it("straight is two-point line", () => {
    expect(pathD({ x: 0, y: 0 }, { x: 10, y: 10 }, "straight")).toBe("M 0 0 L 10 10");
  });
  it("curve is cubic with control points", () => {
    const d = pathD({ x: 0, y: 0 }, { x: 100, y: 40 }, "curve");
    expect(d.startsWith("M")).toBe(true);
    expect(d).toContain("C");
  });
  it("ortho produces elbow with quads", () => {
    const d = pathD({ x: 0, y: 0 }, { x: 100, y: 80 }, "ortho");
    expect(d).toContain("Q");
  });
  it("computeEdge endpoints land on node bounds", () => {
    const g = computeEdge({
      from: { id: "a", x: 0, y: 0, width: 100, height: 60, shape: "rounded" },
      to: { id: "b", x: 400, y: 0, width: 100, height: 60, shape: "rounded" },
      pathStyle: "straight",
    });
    // from-node right anchor region and to-node LEFT anchor region (facing side)
    expect(g.a.x).toBeGreaterThanOrEqual(95);
    expect(g.a.x).toBeLessThanOrEqual(105);
    expect(g.b.x).toBeGreaterThanOrEqual(395);
    expect(g.b.x).toBeLessThanOrEqual(405);
  });
});

describe("pointInShape hit tests", () => {
  it("circle rejects corner points of bounding box", () => {
    expect(pointInShape("circle", 100, 100, 1, 1)).toBe(false);
    expect(pointInShape("circle", 100, 100, 50, 50)).toBe(true);
  });
  it("diamond center in, corners out", () => {
    expect(pointInShape("diamond", 100, 100, 50, 50)).toBe(true);
    expect(pointInShape("diamond", 100, 100, 99, 99)).toBe(false);
  });
  it("triangle top vertex area inside", () => {
    expect(pointInShape("triangle", 100, 100, 50, 20)).toBe(true);
    expect(pointInShape("triangle", 100, 100, 5, 5)).toBe(false);
  });
});

describe("clampDims (module-1 hard bounds + aspect guard)", () => {
  it("enforces the absolute minimums", () => {
    expect(clampDims(10, 10)).toEqual({ width: MIN_NODE_W, height: MIN_NODE_H });
  });
  it("repairs non-finite poison to finite bounds", () => {
    const d = clampDims(Number.NaN, Number.POSITIVE_INFINITY);
    expect(Number.isFinite(d.width)).toBe(true);
    expect(Number.isFinite(d.height)).toBe(true);
    expect(d.height).toBe(MIN_NODE_H); // Infinity is non-finite → minimum
  });
  it("caps extreme landscape ratio at 3:1", () => {
    expect(clampDims(600, MIN_NODE_H)).toEqual({ width: MIN_NODE_H * MAX_ASPECT, height: MIN_NODE_H });
  });
  it("caps extreme portrait ratio at 1:3 (needle guard)", () => {
    expect(clampDims(MIN_NODE_W, 900)).toEqual({ width: MIN_NODE_W, height: MIN_NODE_W * MAX_ASPECT });
  });
  it("leaves sane sizes untouched", () => {
    expect(clampDims(240, 88)).toEqual({ width: 240, height: 88 });
  });
});

describe("shapeCollapsed (module-1 vertex safety lock)", () => {
  it("flags sub-minimum slivers", () => {
    expect(shapeCollapsed("hexagon", 60, 40)).toBe(true);
    expect(shapeCollapsed("triangle", 200, MIN_NODE_H - 1)).toBe(true);
  });
  it("accepts healthy frames of every polygon shape", () => {
    for (const s of ["triangle", "diamond", "pentagon", "hexagon", "heptagon"] as const) {
      expect(shapeCollapsed(s, 240, 160)).toBe(false);
    }
  });
});

describe("sanitizeDims (module-4 sanity normalization)", () => {
  it("repairs NaN/Infinity and reports it", () => {
    const r = sanitizeDims(Number.NaN, Number.NaN);
    expect(r.repaired).toBe(true);
    expect(r.dim.width).toBeGreaterThanOrEqual(MIN_NODE_W);
    expect(r.dim.height).toBeGreaterThanOrEqual(MIN_NODE_H);
  });
  it("reports undersized dims as repaired", () => {
    expect(sanitizeDims(50, 30).repaired).toBe(true);
  });
  it("passes clean dims through unflagged", () => {
    const r = sanitizeDims(240, 88);
    expect(r.repaired).toBe(false);
    expect(r.dim).toEqual({ width: 240, height: 88 });
  });
});

describe("growDimsForText (module-3 reverse dilation)", () => {
  it("keeps a box frame unchanged when text already fits", () => {
    expect(growDimsForText("rounded", 300, 200, 100, 40)).toEqual({ width: 300, height: 200 });
  });
  it("grows a box frame when text exceeds it, honoring padding", () => {
    const g = growDimsForText("rounded", MIN_NODE_W, MIN_NODE_H, 260, 120);
    expect(g.width).toBeGreaterThanOrEqual(260 + 32 - 2); // TEXT_PAD*2 margin
    expect(g.height).toBeGreaterThanOrEqual(120 + 32 - 2);
  });
  it("grows polygons until the INSCRIBED rect fits text + padding", () => {
    for (const shape of ["triangle", "diamond", "hexagon"] as const) {
      const tw = 200;
      const th = 90;
      const cur = growDimsForText(shape, 240, 88, tw, th);
      const insc = inscribedRect(shape, cur.width, cur.height);
      expect(insc.w).toBeGreaterThanOrEqual(tw + 32 - 2);
      expect(insc.h).toBeGreaterThanOrEqual(th + 32 - 2);
      // result must still respect the aspect guard
      expect(cur.width / cur.height).toBeLessThanOrEqual(MAX_ASPECT + 0.01);
      expect(cur.height / cur.width).toBeLessThanOrEqual(MAX_ASPECT + 0.01);
    }
  });
  it("never returns below the hard minimums", () => {
    const g = growDimsForText("circle", 400, 400, 0, 0);
    expect(g.width).toBeGreaterThanOrEqual(MIN_NODE_W);
    expect(g.height).toBeGreaterThanOrEqual(MIN_NODE_H);
  });
});

describe("polygonArea", () => {
  it("computes the triangle area exactly", () => {
    expect(polygonArea([{ x: 0, y: 0 }, { x: 100, y: 0 }, { x: 0, y: 100 }])).toBeCloseTo(5000, 6);
  });
});
