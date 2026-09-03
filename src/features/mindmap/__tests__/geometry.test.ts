import { describe, expect, it } from "vitest";
import {
  anchorsFor, boxShapeExempt, circleDiameter, clampDims, clampInteractive, computeEdge,
  distributedAnchor, growDimsForText, inscribedRect, MAX_ASPECT, MAX_AUTO_H, MIN_NODE_H, MIN_NODE_W,
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

describe("clampDims (持久化路径的绝对硬边界)", () => {
  it("enforces the absolute minimums", () => {
    expect(clampDims(10, 10)).toEqual({ width: MIN_NODE_W, height: MIN_NODE_H });
  });
  it("repairs non-finite poison to finite bounds", () => {
    const d = clampDims(Number.NaN, Number.POSITIVE_INFINITY);
    expect(Number.isFinite(d.width)).toBe(true);
    expect(Number.isFinite(d.height)).toBe(true);
    expect(d.height).toBe(MIN_NODE_H); // Infinity is non-finite → minimum
  });
  it("enforces the absolute maximums", () => {
    expect(clampDims(99999, 99999)).toEqual({ width: 1400, height: 20000 });
  });
  it("leaves sane sizes untouched", () => {
    expect(clampDims(240, 88)).toEqual({ width: 240, height: 88 });
  });
  it("文本优先：任何长宽比都不被钳制（溢出 BUG 回归）", () => {
    // 修复前：3:1 守卫把宽 280 的框钳在 840px 高，长文被裁切
    expect(clampDims(280, 4400)).toEqual({ width: 280, height: 4400 });
    // 多边形的长文尺寸同样完整保留（存储值即渲染值）
    expect(clampDims(1400, 10800)).toEqual({ width: 1400, height: 10800 });
  });
});

describe("clampInteractive (手动拖拽缩放：长宽比守卫仅在此生效)", () => {
  it("caps extreme landscape ratio at 3:1 for polygons", () => {
    expect(clampInteractive(600, MIN_NODE_H, "triangle"))
      .toEqual({ width: MIN_NODE_H * MAX_ASPECT, height: MIN_NODE_H });
  });
  it("caps extreme portrait ratio at 1:3 (needle guard) for polygons", () => {
    expect(clampInteractive(MIN_NODE_W, 900, "diamond"))
      .toEqual({ width: MIN_NODE_W, height: MIN_NODE_W * MAX_ASPECT });
  });
  it("box frames (rect/rounded) are exempt — tall text columns allowed", () => {
    expect(clampInteractive(280, 4400, "rect")).toEqual({ width: 280, height: 4400 });
    expect(clampInteractive(280, 4400, "rounded")).toEqual({ width: 280, height: 4400 });
  });
  it("circle keeps the guard (degenerate ellipses are meaningless)", () => {
    expect(clampInteractive(MIN_NODE_W, 900, "circle").height)
      .toBeLessThanOrEqual(MIN_NODE_W * MAX_ASPECT + 1);
  });
  it("undefined shape keeps the guard (defensive default)", () => {
    expect(clampInteractive(MIN_NODE_W, 900).height).toBe(MIN_NODE_W * MAX_ASPECT);
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
    // th=120 是在 120px 宽度下测得的；宽度增长后文字重排变矮（面积守恒），
    // 框架只需容纳 120×120 的文字面积。
    expect(g.width * g.height).toBeGreaterThanOrEqual(120 * 120 * 0.98);
  });
  it("grows polygons until the INSCRIBED rect fits text + padding", () => {
    for (const shape of ["triangle", "diamond", "hexagon"] as const) {
      const tw = 200;
      const th = 90;
      const cur = growDimsForText(shape, 240, 88, tw, th);
      const insc = inscribedRect(shape, cur.width, cur.height);
      expect(insc.w).toBeGreaterThanOrEqual(tw + 32 - 2);
      // 文本优先（面积守恒）：textH 是在旧内接宽度下测得的；框变宽后文字
      // 重排变矮，因此最终只需容纳 baseW×th 的文字面积，而非原始 th。
      const baseW = inscribedRect(shape, 240, 88).w;
      expect(insc.w * insc.h).toBeGreaterThanOrEqual(baseW * th * 0.98);
    }
  });
  it("文本优先：多边形按文字需求增长，极限文封顶后框内滚动", () => {
    // 修复前：多边形被 1400 宽 + 3:1 守卫钳死，长文必然被裁切
    const cur = growDimsForText("triangle", 240, 88, 280, 5300);
    const insc = inscribedRect("triangle", cur.width, cur.height);
    expect(insc.w).toBeGreaterThanOrEqual(280 + 32 - 2);
    expect(cur.height).toBeLessThanOrEqual(MAX_AUTO_H);
    // 面积守恒：重排后的文字（更矮）必须放得下
    const baseW = inscribedRect("triangle", 240, 88).w;
    expect(insc.w * insc.h).toBeGreaterThanOrEqual(baseW * 5300 * 0.98);
    // 极端长文（面积远超上限）：高度封顶 MAX_AUTO_H，宽度不再无谓扩张
    const big = growDimsForText("triangle", 240, 88, 280, 300000);
    expect(big.height).toBe(MAX_AUTO_H);
    expect(big.width).toBeLessThanOrEqual(1400);
  });
  it("自适应列宽：超长文加宽文本列以塞进限高（≈5 万字全可见）", () => {
    // 面积 240×64000=15.36M px²，在 280 列宽下高约 5.5 万 px —— 列宽按面积
    // 反推加宽到 ~790，高度回到 ~1.9 万 px，无需滚动即可整体容纳。
    const cur = growDimsForText("rounded", 240, 88, 280, 64000);
    expect(cur.width).toBeGreaterThan(400); // 列宽已加宽
    expect(cur.width).toBeLessThanOrEqual(1200 + 32);
    expect(cur.height).toBeLessThanOrEqual(MAX_AUTO_H);
    const insc = inscribedRect("rounded", cur.width, cur.height);
    expect(insc.w * insc.h).toBeGreaterThanOrEqual(240 * 64000 * 0.98); // 面积装得下
  });
  it("按轴独立增长：扁平节点 + 长文不再把宽度竞速到上限", () => {
    // 修复前：统一比例放大让高度亏欠比（巨量）拖动宽度一起暴涨至 MAX_W
    const cur = growDimsForText("diamond", 240, 88, 280, 4000);
    expect(cur.width).toBeLessThan(1000); // 只需 ~2×(280+32)，远够放文字
    expect(cur.height).toBeLessThanOrEqual(MAX_AUTO_H);
    const insc = inscribedRect("diamond", cur.width, cur.height);
    expect(insc.w).toBeGreaterThanOrEqual(280 + 32 - 2);
  });
  it("宽度已足时只增高，不放大宽度", () => {
    const cur = growDimsForText("diamond", 900, 200, 280, 2600);
    expect(cur.width).toBe(900); // 内接宽 450 ≥ 312，宽度无需增长
    expect(cur.height).toBeGreaterThan(2600); // 原始测量宽度未变，直接信任
    expect(cur.height).toBeLessThanOrEqual(MAX_AUTO_H);
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

/**
 * 回归测试：「节点文字严重溢出边框」BUG。
 *
 * 根因：MAX_ASPECT=3 长宽比守卫（为防止多边形被拖成针状而设计）曾被
 * 无差别应用到所有尺寸路径——宽 280px 的写作列无论存了多少文字，渲染
 * 高度被硬钳制在 840px，超出部分全部被裁切。
 *
 * 最终架构（文本优先）：
 * - 持久化路径（存储/渲染/加载/导入/自动增长）只做绝对边界钳制，
 *   不做长宽比钳制——容器永远跟随文字内容；
 * - 长宽比守卫只在手动拖拽缩放（clampInteractive）生效，且
 *   rect/rounded 写作框豁免。
 */
describe("boxShapeExempt：只有 rect/rounded 豁免长宽比守卫", () => {
  it("rect/rounded 为 true，其余形状为 false", () => {
    expect(boxShapeExempt("rect")).toBe(true);
    expect(boxShapeExempt("rounded")).toBe(true);
    for (const s of ["circle", "triangle", "diamond", "pentagon", "hexagon", "heptagon"] as const) {
      expect(boxShapeExempt(s)).toBe(false);
    }
    expect(boxShapeExempt(undefined)).toBe(false);
  });
});

describe("sanitizeDims 渲染归一化（文本优先，不做长宽比钳制）", () => {
  it("rect 长文节点：渲染尺寸不再被 3:1 钳制（溢出 BUG 的直接回归）", () => {
    // 宽 280 的框 + 4440px 高的文字 → 修复前渲染高度被钳到 840
    const { dim, repaired } = sanitizeDims(280, 4440);
    expect(repaired).toBe(false);
    expect(dim).toEqual({ width: 280, height: 4440 });
  });

  it("多边形长文节点：存储尺寸原样通过渲染归一化", () => {
    const { dim, repaired } = sanitizeDims(1400, 10800);
    expect(repaired).toBe(false);
    expect(dim).toEqual({ width: 1400, height: 10800 });
  });

  it("损坏尺寸（NaN/过小）依然被修复", () => {
    const { dim, repaired } = sanitizeDims(NaN, 40);
    expect(repaired).toBe(true);
    expect(dim.width).toBeGreaterThanOrEqual(120);
    expect(dim.height).toBeGreaterThanOrEqual(80);
  });
});
