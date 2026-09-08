import { describe, expect, it } from "vitest";
import { groupKeyOf, indexLetters, letterGroups, INDEX_THRESHOLD } from "../indexBar";

/**
 * 化境 V-11 回归：字母索引条分组逻辑 ——
 * - 拉丁字母取首字母；CJK 用 pinyin.ts 的 initialsOf（拼音首字母）；
 * - 未收录汉字/数字/符号不猜音 → 归「#」组（诚实降级，与 pinyin.ts 原则一致）；
 * - 分组按 A→Z、「#」最后；组内按拼音序（确定性，不依赖 ICU）。
 */

describe("groupKeyOf（V-11 分组键）", () => {
  it("拉丁字母直接取首字母（大小写归一）", () => {
    expect(groupKeyOf("Apple")).toBe("A");
    expect(groupKeyOf("blender")).toBe("B");
  });

  it("CJK 按拼音首字母归组", () => {
    expect(groupKeyOf("微信")).toBe("W"); // wei → W
    expect(groupKeyOf("设置")).toBe("S"); // she → S
    expect(groupKeyOf("计算器")).toBe("J"); // jisuanqi → J
  });

  it("数字/未收录汉字/符号不猜音 → #（诚实降级）", () => {
    expect(groupKeyOf("360安全卫士")).toBe("#");
    expect(groupKeyOf("")).toBe("#");
  });
});

describe("letterGroups（V-11 分组与排序）", () => {
  it("字母组按 A→Z 排列，# 组最后", () => {
    const groups = letterGroups([
      { id: "1", label: "微信" },
      { id: "2", label: "Apple" },
      { id: "3", label: "360" },
      { id: "4", label: "设置" },
    ]);
    expect(indexLetters(groups)).toEqual(["A", "S", "W", "#"]);
  });

  it("组内按拼音序（确定性，不依赖运行环境 ICU）", () => {
    const groups = letterGroups([
      { id: "b1", label: "比" }, // bi
      { id: "b2", label: "保存" }, // bao
      { id: "b3", label: "搬运" }, // ban
    ]);
    expect(groups).toHaveLength(1);
    expect(groups[0]?.letter).toBe("B");
    expect(groups[0]?.items.map((i) => i.id)).toEqual(["b3", "b2", "b1"]); // ban < bao < bi
  });

  it("空列表 → 空分组", () => {
    expect(letterGroups([])).toEqual([]);
  });

  it("阈值常量为 30（应用数 >30 才显示索引条）", () => {
    expect(INDEX_THRESHOLD).toBe(30);
  });
});
