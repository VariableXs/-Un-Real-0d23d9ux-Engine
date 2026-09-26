import { describe, expect, it } from "vitest";
import { NO_SAFE_AREA, READAPT_BUDGET_MS, listColumnLayout, orientationOf, readaptToScreen, snapRect, taskbarEdge, withinSafeArea } from "../f388-portraitAdaptation";

const LAND = { w: 1920, h: 1080 };
const PORT = { w: 1080, h: 1920 };

describe("F388 竖屏与异形屏适配", () => {
  it("判向：横/竖屏识别", () => {
    expect(orientationOf(LAND)).toBe("landscape");
    expect(orientationOf(PORT)).toBe("portrait");
    expect(orientationOf({ w: 1000, h: 1000 })).toBe("portrait"); // 正方形保守按竖屏
  });

  it("竖屏贴靠形制：左右区变上下分区（判据）", () => {
    expect(snapRect("left", LAND)).toEqual({ x: 0, y: 0, w: 960, h: 1080 });
    expect(snapRect("left", PORT)).toEqual({ x: 0, y: 0, w: 1080, h: 960 });
    expect(snapRect("right", PORT)).toEqual({ x: 0, y: 960, w: 1080, h: 960 });
    // 四角：竖屏 tl/tr 占上半全宽
    expect(snapRect("tl", PORT)).toEqual({ x: 0, y: 0, w: 1080, h: 960 });
    expect(snapRect("br", PORT)).toEqual({ x: 0, y: 960, w: 1080, h: 960 });
    expect(snapRect("br", LAND)).toEqual({ x: 960, y: 540, w: 960, h: 540 });
  });

  it("异形圆角安全区不遮内容（判据）：贴靠矩形整体落在安全区内", () => {
    const safe = { top: 40, bottom: 24, left: 0, right: 0 };
    for (const zone of ["left", "right", "top", "bottom", "tl", "br"] as const) {
      expect(withinSafeArea(snapRect(zone, PORT, safe), PORT, safe)).toBe(true);
    }
    expect(withinSafeArea(snapRect("left", PORT, NO_SAFE_AREA), PORT, NO_SAFE_AREA)).toBe(true);
  });

  it("任务栏侧边模式：竖屏可选 left/right；横屏锁定底边（判据）", () => {
    expect(taskbarEdge("portrait", "left")).toBe("left");
    expect(taskbarEdge("portrait", "right")).toBe("right");
    expect(taskbarEdge("portrait", "bottom")).toBe("bottom");
    expect(taskbarEdge("landscape", "left")).toBe("bottom"); // 横屏锁定
    expect(taskbarEdge("portrait", "auto")).toBe("bottom");
  });

  it("单列默认判据：竖屏列表 1 列吃满、横屏 2 列", () => {
    expect(listColumnLayout(PORT)).toEqual({ columns: 1, rowWidth: 1080 });
    expect(listColumnLayout(LAND)).toEqual({ columns: 2, rowWidth: 960 });
    const safe = { top: 0, bottom: 0, left: 12, right: 12 };
    expect(listColumnLayout(PORT, safe).rowWidth).toBe(1056);
  });

  it("跨屏重适配 ≤100ms 预算（判据）+ 超界钳回（F214 降级序）", () => {
    expect(READAPT_BUDGET_MS).toBeLessThanOrEqual(100);
    const r = readaptToScreen({ x: 1200, y: 200, w: 1400, h: 900 }, PORT);
    expect(r.rect).toEqual({ x: 0, y: 200, w: 1080, h: 900 });
    expect(r.withinBudget).toBe(true);
    const shrink = readaptToScreen({ x: 0, y: 0, w: 1920, h: 1080 }, { w: 800, h: 600 });
    expect(shrink.rect).toEqual({ x: 0, y: 0, w: 800, h: 600 });
  });
});
