import { describe, it, expect } from "vitest";
import { layoutTier, suggestLargeType, viewportInfo } from "../breakpoints";

describe("AI-17 breakpoints（U-12）", () => {
  it("三档断点边界正确（compact<1440≤standard<2560≤wide）", () => {
    expect(layoutTier(1279)).toBe("compact");
    expect(layoutTier(1280)).toBe("compact");
    expect(layoutTier(1439)).toBe("compact");
    expect(layoutTier(1440)).toBe("standard");
    expect(layoutTier(2559)).toBe("standard");
    expect(layoutTier(2560)).toBe("wide");
    expect(layoutTier(3840)).toBe("wide");
  });

  it("大字模式：超大屏 + 高缩放", () => {
    expect(suggestLargeType({ width: 3840, height: 2160, dpr: 2 })).toBe(true);
    expect(suggestLargeType({ width: 1920, height: 1080, dpr: 1 })).toBe(false);
    // 有效 CSS 像素 ≥2560 且 DPR ≥1.5（外接 TV 投影）
    expect(suggestLargeType({ width: 5120, height: 2880, dpr: 2 })).toBe(true);
  });

  it("viewportInfo 在无 window 环境返回标准值", () => {
    // vitest node/jsdom 均可安全调用
    const info = viewportInfo();
    expect(info.width).toBeGreaterThan(0);
    expect(info.dpr).toBeGreaterThan(0);
  });

  it("suggestLargeType 不误报普通 2K（DPR=1）", () => {
    expect(suggestLargeType({ width: 2560, height: 1440, dpr: 1 })).toBe(false);
  });
});
