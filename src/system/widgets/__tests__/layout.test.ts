import { describe, expect, it } from "vitest";
import {
  clampToViewport, collides, rectsOverlap, resolvePlacement, sanitizeLayout,
  SIZE_PX, snap, type WgtLayoutItem,
} from "../layout";

const item = (id: string, x: number, y: number, size: WgtLayoutItem["size"] = "1x1"): WgtLayoutItem => ({ id, x, y, size });

describe("snap 32px 网格吸附", () => {
  it("四舍五入到最近格", () => {
    expect(snap(0)).toBe(0);
    expect(snap(31)).toBe(32);
    expect(snap(33)).toBe(32);
    expect(snap(100)).toBe(96);
    expect(snap(-10)).toBe(-0); // 负值向 0 收
  });
});

describe("rectsOverlap / collides 互不遮挡判定", () => {
  it("相交/相切/分离", () => {
    const a = { x: 0, y: 0, w: 128, h: 128 };
    expect(rectsOverlap(a, { x: 64, y: 64, w: 128, h: 128 })).toBe(true);
    expect(rectsOverlap(a, { x: 128, y: 0, w: 128, h: 128 })).toBe(false); // 相切不算
    expect(rectsOverlap(a, { x: 500, y: 500, w: 128, h: 128 })).toBe(false);
  });
  it("collides 跳过自身与 collapsed 卡", () => {
    const items = [item("a", 0, 0), item("b", 32, 0), { ...item("c", 32, 0), collapsed: true }];
    expect(collides(items, item("a", 32, 0))).toBe(true); // 与 b 冲突
    expect(collides(items, item("b", 0, 0))).toBe(true);
    expect(collides(items, item("c", 0, 0))).toBe(true); // c 与 a 冲突（c 未移动）
  });
});

describe("resolvePlacement 拖放落点解析", () => {
  it("空位直接吸附", () => {
    const out = resolvePlacement([item("a", 0, 0)], "a", 100, 50, 1920, 1080);
    expect(out[0]).toMatchObject({ x: 96, y: 64 });
  });
  it("冲突时螺形搜索最近空位（+32 右移优先）", () => {
    const items = [item("a", 128, 0), item("b", 0, 0)];
    // b 拖到 a 的位置 → 先试 x-32（0,0 原位冲突？b 原 x=0…原位即 0,0 与 a 不冲突）
    const out = resolvePlacement(items, "b", 128, 0, 1920, 1080);
    const b = out.find((i) => i.id === "b");
    expect(b?.x).not.toBe(128);
    expect(collides(out.filter((i) => !i.collapsed), b as WgtLayoutItem)).toBe(false);
  });
  it("视口钳制", () => {
    const p = clampToViewport(5000, 5000, "2x2", 800, 600);
    expect(p.x).toBeLessThanOrEqual(800 - SIZE_PX["2x2"].w);
    expect(p.y).toBeLessThanOrEqual(600 - SIZE_PX["2x2"].h);
  });
});

describe("sanitizeLayout 布局持久化兼容", () => {
  it("坏数据回默认", () => {
    expect(sanitizeLayout(null)).toEqual({ version: 1, mode: "desktop", items: [] });
    expect(sanitizeLayout({ version: 3 })).toEqual({ version: 1, mode: "desktop", items: [] });
  });
  it("脏字段修正：未知 size → 1x1，坐标吸附，mode 非法 → desktop", () => {
    const doc = sanitizeLayout({
      version: 1,
      mode: "weird",
      items: [{ id: "a", x: 50, y: 100, size: "9x9" }, { id: 42 }, null],
    });
    expect(doc.mode).toBe("desktop");
    expect(doc.items).toHaveLength(1);
    expect(doc.items[0]).toMatchObject({ id: "a", x: 64, y: 96, size: "1x1" });
  });
});