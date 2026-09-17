import { describe, expect, it } from "vitest";
import { computeWindow } from "../virtualList";

describe("虚拟列表窗口计算（定高行 + 滚动窗口 ± 缓冲）", () => {
  const ROW = 56;

  it("空列表 → 空窗口、总高为 0", () => {
    const w = computeWindow({ scrollTop: 0, viewportHeight: 400, rowHeight: ROW, total: 0 });
    expect(w).toMatchObject({ start: 0, end: 0, totalHeight: 0 });
  });

  it("顶部滚动 → 从 0 开始并含上缓冲", () => {
    const w = computeWindow({ scrollTop: 0, viewportHeight: 400, rowHeight: ROW, total: 1000 });
    expect(w.start).toBe(0);
    // 可见行数 = ceil(400/56)=8，+ overscan(6) => end = 14
    expect(w.end).toBe(14);
    expect(w.totalHeight).toBe(1000 * ROW);
    expect(w.offsetY).toBe(0);
  });

  it("滚动到中部 → 窗口随 scrollTop 平移，含上下缓冲", () => {
    const scrollTop = 56 * 100; // 第 100 行顶部
    const w = computeWindow({ scrollTop, viewportHeight: 400, rowHeight: ROW, total: 1000 });
    // firstVisible = 100, start = 100-6 = 94, end = 100 + 8 + 6 = 114
    expect(w.start).toBe(94);
    expect(w.end).toBe(114);
    expect(w.offsetY).toBe(94 * ROW);
  });

  it("滚动到尾部 → 末端被钳制不超过 total", () => {
    const scrollTop = 56 * 990; // 接近末尾
    const w = computeWindow({ scrollTop, viewportHeight: 400, rowHeight: ROW, total: 1000 });
    expect(w.end).toBe(1000);
    expect(w.start).toBeGreaterThanOrEqual(0);
  });

  it("自定义 overscan 生效", () => {
    const w = computeWindow({ scrollTop: 56 * 50, viewportHeight: 400, rowHeight: ROW, total: 1000, overscan: 2 });
    // firstVisible=50, start=48, end=50+8+2=60
    expect(w.start).toBe(48);
    expect(w.end).toBe(60);
  });

  it("行高非法（0）不崩溃，返回空窗口", () => {
    const w = computeWindow({ scrollTop: 100, viewportHeight: 400, rowHeight: 0, total: 10 });
    expect(w.end).toBe(0);
  });
});
