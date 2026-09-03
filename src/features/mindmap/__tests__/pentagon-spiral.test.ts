import { describe, expect, it } from "vitest";
import { growDimsForText, inscribedRect, shapePoints } from "../geometry";

// 回归：细高盒里的正多边形必须充满包围盒（椭圆映射顶点）。
// 修复前 pentagon/hexagon/heptagon 用 min(rx,ry) 正多边形 —— 细高盒里只是
// 中央一小团，内接矩形高度与盒高无关 → 长文自适应回路永不收敛，高度直冲
// MAX_AUTO_H（E2E 实测 1410 字五边形螺旋到 602×20000）。
describe("多边形充满包围盒 + 长文增长收敛", () => {
  it("五边形顶点充满细高包围盒（不再缩成中央小团）", () => {
    const pts = shapePoints("pentagon", 631, 20000);
    const ys = pts.map((p) => p.y);
    const xs = pts.map((p) => p.x);
    expect(Math.max(...ys) - Math.min(...ys)).toBeGreaterThan(15000); // 纵向充满
    expect(Math.max(...xs) - Math.min(...xs)).toBeGreaterThan(550); // 横向充满
  });

  it("六边形/七边形同样充满", () => {
    for (const s of ["hexagon", "heptagon"] as const) {
      const pts = shapePoints(s, 900, 8000);
      expect(Math.max(...pts.map((p) => p.y)) - Math.min(...pts.map((p) => p.y))).toBeGreaterThan(6000);
    }
  });

  it("方盒时保持近似正多边形（宽度=高度时顶点距离中心一致）", () => {
    const pts = shapePoints("pentagon", 300, 300);
    const rs = pts.map((p) => Math.hypot(p.x - 150, p.y - 150));
    expect(Math.max(...rs) - Math.min(...rs)).toBeLessThan(1);
  });

  it("E2E 捕获序列：五边形 631×2189 + (280,786) 不再螺旋到 20000", () => {
    // 反馈回路模拟：每轮按新框宽重测 th（面积守恒近似）
    let cur = { width: 631, height: 2189 };
    let th = 786;
    for (let i = 0; i < 12; i++) {
      const r = growDimsForText("pentagon", cur.width, cur.height, 280, th, true);
      const insc = inscribedRect("pentagon", r.width, r.height);
      const nextTh = Math.round((786 * 425.5) / Math.max(1, insc.w));
      if (r.width === cur.width && r.height === cur.height && nextTh === th) break;
      cur = r;
      th = nextTh;
    }
    expect(cur.height).toBeLessThan(20000);
    expect(cur.height).toBeLessThanOrEqual(2500); // 1410 字实际收敛在 2189
    // 内容完整可见：内接框高度 ≥ 文字需求
    const insc = inscribedRect("pentagon", cur.width, cur.height);
    expect(insc.h).toBeGreaterThanOrEqual(th - 2);
  });

  it("菱形契约不回归：长文仍按内接矩形增长", () => {
    const r = growDimsForText("diamond", 280, 5081, 280, 16882, true);
    const insc = inscribedRect("diamond", r.width, r.height);
    expect(insc.h).toBeGreaterThanOrEqual(32 + 16882 * (140 / Math.max(312, insc.w)) - 2);
    expect(r.height).toBeLessThan(20000);
  });
});
