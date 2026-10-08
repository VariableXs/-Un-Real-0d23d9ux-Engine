import { describe, expect, it } from "vitest";
import { dynamicPage, productSpec, seamReviewRecord, stitch, uniformPage } from "../f389-scrollStitch";

describe("F389 滚动长截图", () => {
  it("典型页 1 均匀滚动：全量拼接零重复（去重判据）", () => {
    const frames = uniformPage(20, 8, 4); // 5 帧、每步重叠 4 行
    const r = stitch(frames);
    expect(r.failure).toBeNull();
    expect(r.totalRows).toBe(20);
    expect(r.seams).toHaveLength(5);
    expect(r.seams.map((s) => s.row)).toEqual([0, 4, 8, 12, 16]);
  });

  it("典型页 2 重复捕获同屏（与下一帧有重叠）：重复内容只留一份", () => {
    const page = uniformPage(12, 6, 3); // 步长 3、视口 6 → 相邻帧重叠 3 行
    const frames = [page[0]!, page[0]!, page[1]!, page[2]!, page[3]!]; // 第二帧是重复捕获
    const r = stitch(frames);
    expect(r.failure).toBeNull();
    expect(r.totalRows).toBe(12);
  });

  it("典型页 3 单帧：原样透传", () => {
    const r = stitch(uniformPage(5, 5, 5).slice(0, 1));
    expect(r).toMatchObject({ totalRows: 5, failure: null });
    expect(stitch([]).failure).toContain("无捕获帧");
  });

  it("动态内容诚实失败：内容变了给显式说明不给错位图（判据）", () => {
    const frames = dynamicPage(20, 8, 4, 10);
    const r = stitch(frames);
    expect(r.failure).toContain("不适合长截图");
    expect(r.failure).toContain("无重叠");
  });

  it("接缝人工评审记录（判据）：成功有 seam 账、失败无账", () => {
    const ok = seamReviewRecord(stitch(uniformPage(12, 6, 3)));
    expect(ok.every((s) => s.verdict === "clean")).toBe(true);
    expect(ok).toHaveLength(4);
    expect(seamReviewRecord(stitch(dynamicPage(20, 8, 4, 5)))).toEqual([]);
  });

  it("产物规格与保存链（判据）：尺寸=行数×行高、命名带日期", () => {
    const r = stitch(uniformPage(40, 10, 5));
    const spec = productSpec(r, 24, 800, new Date(2026, 8, 25, 9, 5));
    expect(spec.h).toBe(40 * 24);
    expect(spec.w).toBe(800);
    expect(spec.fileName).toBe("长截图 2026-09-25 09-05.png");
  });
});
