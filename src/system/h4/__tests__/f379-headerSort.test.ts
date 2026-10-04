import { describe, expect, it } from "vitest";
import { APPLY_BUDGET_MS, ARROW_GAP_PX, MAX_SORT_LEVELS, clickHeader, comparator, indicatorFor, initialSort, loadSort, persistSort, scrollAfterSort, withinApplyBudget } from "../f379-headerSort";
import { __clearMem, memStore } from "../internal/store";

describe("F379 表头排序指示", () => {
  it("三态循环：未排→升→降→取消（判据）", () => {
    let s = initialSort();
    s = clickHeader(s, "name", false);
    expect(indicatorFor(s, "name")).toEqual({ arrow: "▲", level: null, bold: true });
    s = clickHeader(s, "name", false);
    expect(indicatorFor(s, "name").arrow).toBe("▼");
    s = clickHeader(s, "name", false);
    expect(s.keys).toEqual([]);
    expect(indicatorFor(s, "name")).toEqual({ arrow: null, level: null, bold: false });
    expect(ARROW_GAP_PX).toBe(4);
  });

  it("Shift 多级：加入尾键、再次点击翻向、三级上限淘汰最旧", () => {
    let s = initialSort();
    s = clickHeader(s, "date", false);
    s = clickHeader(s, "name", true);
    s = clickHeader(s, "size", true);
    expect(s.keys.map((k) => k.columnId)).toEqual(["date", "name", "size"]);
    expect(indicatorFor(s, "name")).toEqual({ arrow: "▲", level: 2, bold: true });
    s = clickHeader(s, "ext", true); // 超三级：date 被淘汰
    expect(s.keys.map((k) => k.columnId)).toEqual(["name", "size", "ext"]);
    s = clickHeader(s, "name", true); // name 翻降序
    expect(indicatorFor(s, "name").arrow).toBe("▼");
    s = clickHeader(s, "name", true); // name 取消（Shift 降序再点=移除）
    expect(s.keys.map((k) => k.columnId)).toEqual(["size", "ext"]);
  });

  it("滚动位置保持（判据）：排序前后偏移恒等", () => {
    expect(scrollAfterSort(321)).toBe(321);
    expect(scrollAfterSort(0)).toBe(0);
  });

  it("即时性 ≤100ms（预算即上限，生效判定严格小于）（判据）", () => {
    expect(APPLY_BUDGET_MS).toBeLessThanOrEqual(100);
    expect(withinApplyBudget(1000, 999)).toBe(true);
    expect(withinApplyBudget(1100, 1000)).toBe(false);
  });

  it("F219 记忆联动：持久化 round-trip、越界键被钳回三级", () => {
    __clearMem();
    const s = memStore();
    let st = clickHeader(initialSort(), "date", false);
    st = clickHeader(st, "name", true);
    expect(persistSort(st, s)).toBe(true);
    expect(loadSort(s).keys).toEqual(st.keys);
    s.setItem("variable:h4:f379:sort", JSON.stringify([{ columnId: "a", dir: "asc" }, { columnId: "b", dir: "desc" }, { columnId: "c", dir: "asc" }, { columnId: "d", dir: "desc" }]));
    expect(loadSort(s).keys).toHaveLength(MAX_SORT_LEVELS);
  });

  it("比较器：主序→次序逐级比较、降序取反、同值稳定（中文 locale）", () => {
    const rows = [
      { name: "乙", size: 2 },
      { name: "甲", size: 2 },
      { name: "丙", size: 1 },
    ];
    const cmp = comparator<typeof rows[number]>([{ columnId: "size", dir: "asc" }, { columnId: "name", dir: "desc" }], (r, c) => (c === "size" ? r.size : r.name));
    expect([...rows].sort(cmp).map((r) => r.name)).toEqual(["丙", "乙", "甲"]); // size 升序先分出丙(1)；余两名 size 同、名称降序 乙>甲
    const byName = comparator<typeof rows[number]>([{ columnId: "name", dir: "asc" }], (r) => r.name);
    expect([...rows].sort(byName).map((r) => r.name)).toEqual(["丙", "甲", "乙"]);
  });
});
