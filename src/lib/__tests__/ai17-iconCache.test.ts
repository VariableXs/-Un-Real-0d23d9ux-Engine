import { describe, it, expect, beforeEach, vi } from "vitest";
import {
  dprNamespace, cacheKey, peekDecoded, decodeIcon, resetIconCache, cacheStats,
} from "../iconCache";

const hasImage = typeof Image !== "undefined";

describe("AI-17 iconCache（Z-66/Z-04）", () => {
  beforeEach(() => resetIconCache());

  it("DPR 命名空间量化到 0.25 步进", () => {
    expect(dprNamespace(1)).toBe("1");
    expect(dprNamespace(1.25)).toBe("1.25");
    expect(dprNamespace(1.5)).toBe("1.5");
    expect(dprNamespace(2)).toBe("2");
    expect(dprNamespace(1.3)).toBe("1.25"); // 量化
    expect(dprNamespace(0.5)).toBe("1"); // 下限 1
  });

  it("cacheKey 按 DPR 隔离命名空间", () => {
    expect(cacheKey("a.png", 1)).toBe("1:a.png");
    expect(cacheKey("a.png", 2)).toBe("2:a.png");
    expect(cacheKey("a.png", 1)).not.toBe(cacheKey("a.png", 2));
  });

  it("未命中返回 null 且计数 miss", () => {
    expect(peekDecoded("a.png", 1)).toBeNull();
    const stats = cacheStats();
    expect(stats.misses).toBe(1);
    expect(stats.hits).toBe(0);
  });

  it("命中复用已解码位图（禁止重复解码）", async () => {
    if (!hasImage) return; // node 环境无 Image，浏览器真机覆盖
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockRejectedValue(new Error("no net"));
    // fetch 失败 → 回退 Image 路径；data: URL 不走 fetch。
    // 这里用可解析 data URL 让 Image.onload 触发：
    const px =
      "data:image/gif;base64,R0lGODlhAQABAIAAAP///wAAACwAAAAAAQABAAACAUwAOw==";
    const e1 = await decodeIcon(px, 1);
    expect(e1.width).toBe(1);
    const e2 = await peekDecoded(px, 1);
    expect(e2).not.toBeNull();
    // 第二次 decodeIcon 走缓存，不触发新解码
    await decodeIcon(px, 1);
    expect(cacheStats().hits).toBeGreaterThanOrEqual(1);
    fetchSpy.mockRestore();
  });

  it("解码失败抛出明确错误（含源标识）", async () => {
    await expect(decodeIcon("data:image/png;base64,////invalid", 1)).rejects.toThrow(/decode failed/);
  });

  it("命名空间隔离：同一 src 不同 DPR 视为不同条目", () => {
    peekDecoded("a.png", 1);
    peekDecoded("a.png", 2);
    peekDecoded("a.png", 2); // 命中
    const stats = cacheStats();
    expect(stats.misses).toBe(2);
    expect(stats.hits).toBe(1);
  });
});
