import { describe, expect, it } from "vitest";
import {
  CONTENT_GEOMETRY,
  SKELETON_DELAY_MS,
  hideImmediately,
  shouldShowSkeleton,
  skeletonFor,
} from "../skeleton";

describe("任务76 · E1 骨架屏统一规格", () => {
  it("三类消费方同一入口（单组件复用），形状=内容轮廓同源常量", () => {
    for (const c of ["files", "settings", "board"] as const) {
      const s = skeletonFor(c, 6);
      expect(s.consumer).toBe(c);
      expect(s.rows).toHaveLength(6);
      expect(s.rows.every((r) => r.heightPx === CONTENT_GEOMETRY[c])).toBe(true);
      expect(s.rows.every((r) => r.widthPct > 0 && r.widthPct <= 100)).toBe(true);
    }
  });

  it("行数钳制：0 → 1 行，负数/NaN → 1 行，>24 → 24 行", () => {
    expect(skeletonFor("files", 0).rows).toHaveLength(1);
    expect(skeletonFor("files", -3).rows).toHaveLength(1);
    expect(skeletonFor("files", Number.NaN).rows).toHaveLength(1);
    expect(skeletonFor("files", 99).rows).toHaveLength(24);
  });

  it("快路径不闪：<300ms 不显示，≥300ms 显示，完成立即撤", () => {
    expect(SKELETON_DELAY_MS).toBe(300);
    expect(shouldShowSkeleton(0, false)).toBe(false);
    expect(shouldShowSkeleton(299, false)).toBe(false);
    expect(shouldShowSkeleton(300, false)).toBe(true);
    expect(shouldShowSkeleton(1000, false)).toBe(true);
    expect(shouldShowSkeleton(1000, true)).toBe(false);
    expect(shouldShowSkeleton(Number.NaN, false)).toBe(false);
    expect(hideImmediately(true)).toBe(true);
  });

  it("reduce-motion / 低配（E4）→ static 档；正常 → pulse", () => {
    expect(skeletonFor("files", 3).motion).toBe("pulse");
    expect(skeletonFor("files", 3, { reduceMotion: true }).motion).toBe("static");
    expect(skeletonFor("files", 3, { lowSpec: true }).motion).toBe("static");
  });

  it("宽度节奏逐行循环（轮廓节奏一致），设置卡首行缩进", () => {
    const s = skeletonFor("settings", 4);
    // 请求 4 行恒有产出，索引恒在界内。
    expect(s.rows[0]!.indentPx).toBe(12);
    expect(s.rows[1]!.indentPx).toBe(0);
    expect(s.rows[0]!.widthPct).toBe(s.rows[3]!.widthPct); // 节奏周期 3
  });
});
