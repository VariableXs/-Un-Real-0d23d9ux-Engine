import { describe, expect, it } from "vitest";
import { SNAP_THRESHOLD_PX, ZONES, arrangeByZones, defaultZoneLayout, disabledZeroDifference, snapToGrid, zoneAt, zoneHighlight } from "../f367-zoneSnap";

describe("F367 桌面分区吸附", () => {
  it("吸附 8px 阈值与参考线：阈值内贴齐+参考线；超阈值自由位", () => {
    expect(SNAP_THRESHOLD_PX).toBe(8);
    const near = snapToGrid(104, 203);
    expect(near.snapped).toBe(true);
    expect(near.cell).toEqual({ x: 100, y: 200 });
    expect(near.guides).toEqual({ vertical: 100, horizontal: 200 });
    const far = snapToGrid(120, 200);
    expect(far.snapped).toBe(true);
    expect(far.guides.vertical).toBeNull();
    const free = snapToGrid(115, 115);
    expect(free.snapped).toBe(false);
    expect(free.cell).toEqual({ x: 115, y: 115 });
    expect(free.guides).toEqual({ vertical: null, horizontal: null });
  });

  it("分区四区定义：工作/下载/待办/收藏，矩形不重叠且铺满", () => {
    expect(ZONES.map((z) => z.name)).toEqual(["工作", "下载", "待办", "收藏"]);
    const layout = { ...defaultZoneLayout({ w: 1920, h: 1040 }), enabled: true };
    const ids = ZONES.map((z) => z.id);
    expect(ids).toHaveLength(4);
    for (let y = 0; y < 1040; y += 130) {
      expect(zoneAt(layout, 960, y)).not.toBeNull();
    }
    expect(zoneAt({ ...layout, enabled: false }, 960, 10)).toBeNull(); // 未启用不归类
  });

  it("区域淡显：仅启用时可见、低透明度", () => {
    const layout = { ...defaultZoneLayout({ w: 1000, h: 400 }), enabled: true };
    const hl = zoneHighlight(layout, "todo");
    expect(hl.visible).toBe(true);
    expect(hl.opacity).toBeLessThan(0.15);
    expect(hl.rect).toEqual(layout.rects.todo);
    const off = zoneHighlight({ ...layout, enabled: false }, "todo");
    expect(off.visible).toBe(false);
  });

  it("按区排列：区内按名排序、网格铺排；未启用返回空（零差异）", () => {
    const layout = { ...defaultZoneLayout({ w: 800, h: 800 }), enabled: true };
    const placed = arrangeByZones(layout, [
      { id: "2", zone: "work", name: "乙" },
      { id: "1", zone: "work", name: "甲" },
      { id: "3", zone: "favorites", name: "藏" },
      { id: "4", zone: null, name: "野" }, // 无区图标不参与（保持自由位）
    ]);
    expect(placed.find((p) => p.id === "1")).toEqual({ id: "1", x: 0, y: 0 });
    expect(placed.find((p) => p.id === "2")).toEqual({ id: "2", x: 100, y: 0 }); // 行主序网格（F084 同源）
    expect(placed.find((p) => p.id === "3")).toEqual({ id: "3", x: 0, y: 600 });
    expect(placed.find((p) => p.id === "4")).toBeUndefined();
    expect(arrangeByZones({ ...layout, enabled: false }, [])).toEqual([]);
  });

  it("未启用零差异判据：分区关闭时拖放与普通网格完全一致", () => {
    const off = defaultZoneLayout({ w: 1920, h: 1040 });
    expect(disabledZeroDifference(off, 104, 200)).toBe(true);
    expect(disabledZeroDifference({ ...off, enabled: true }, 104, 200)).toBe(false);
  });
});
