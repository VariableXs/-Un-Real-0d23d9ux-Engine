import { describe, expect, it } from "vitest";
import { AUTOFIT_PADDING_PX, MIN_COLUMN_PX, autofitWidth, distributeWidths, dragBubble, dragWidth, restoreMemory, snapshotMemory, type ColumnSpec } from "../f378-columnAutofit";

/** 等宽近似测量：每字符 8px。 */
const measure = (t: string): number => t.length * 8;

describe("F378 列宽双击自适应", () => {
  it("自适应算法：最长可见项 + 12px 余量（判据）", () => {
    expect(AUTOFIT_PADDING_PX).toBe(12);
    const cells = [
      { columnId: "name", text: "ab" },
      { columnId: "name", text: "abcdefgh" }, // 最长 64px
      { columnId: "size", text: "1 KB" },
    ];
    expect(autofitWidth(cells, "name", measure)).toBe(64 + 12);
    expect(autofitWidth(cells, "size", measure)).toBe(32 + 12);
  });

  it("最小 40px 钳制：短内容与空列都不过窄（判据）", () => {
    expect(autofitWidth([{ columnId: "x", text: "" }], "x", measure)).toBe(MIN_COLUMN_PX);
    expect(autofitWidth([], "x", measure)).toBe(MIN_COLUMN_PX);
    expect(autofitWidth([{ columnId: "x", text: "a" }], "x", measure)).toBe(MIN_COLUMN_PX); // 8+12=20 → 40
  });

  it("拖拽调宽：40px 钳制；气泡实时显示钳制后的值", () => {
    expect(dragWidth(120)).toBe(120);
    expect(dragWidth(10)).toBe(MIN_COLUMN_PX);
    expect(dragWidth(Number.NaN)).toBe(MIN_COLUMN_PX);
    expect(dragBubble(10)).toBe("40 px");
    expect(dragBubble(233)).toBe("233 px");
  });

  it("最右列吃满剩余空间；前列吃超时最右钳回 40 并如实报 overflow", () => {
    const cols: ColumnSpec[] = [
      { id: "a", width: 200, last: false },
      { id: "b", width: 300, last: false },
      { id: "c", width: 0, last: true },
    ];
    expect(distributeWidths(cols, 1000)).toEqual({ widths: { a: 200, b: 300, c: 500 }, overflow: false });
    const tight = distributeWidths(cols, 520);
    expect(tight.widths.c).toBe(MIN_COLUMN_PX);
    expect(tight.overflow).toBe(true);
    expect(distributeWidths(cols, 1000).widths.c).toBe(500);
  });

  it("F219 记忆联动：快照-恢复 round-trip；缺失列回退默认宽", () => {
    const cols: ColumnSpec[] = [
      { id: "a", width: 100, last: false },
      { id: "b", width: 200, last: true },
    ];
    const mem = snapshotMemory({ a: 150 });
    expect(restoreMemory(mem, cols)).toEqual({ a: 150, b: 200 });
    expect(restoreMemory(snapshotMemory({ a: 150, b: 333 }), cols)).toEqual({ a: 150, b: 333 });
    expect(restoreMemory(null, cols)).toEqual({ a: 100, b: 200 });
  });
});
