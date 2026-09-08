import { beforeEach, describe, expect, it } from "vitest";
import {
  clearMultiSelect,
  multiSelected,
  planGroupClose,
  planSwap,
  snapGroupRects,
  swapEligible,
  toggleSelect,
  translateGroup,
} from "../multiselect";
import {
  focusBack,
  focusForward,
  focusHistoryCounts,
  recordFocus,
  resetFocusHistory,
} from "../focusHistory";

beforeEach(() => {
  clearMultiSelect();
  resetFocusHistory();
});

function op(winId: string, x: number, y: number, w = 800, h = 600) {
  return { winId, rect: { x, y, w, h } };
}

describe("V-24 多选编组", () => {
  it("Ctrl+点击切换选中；无 Ctrl 点击清空退出", () => {
    toggleSelect("a", true);
    toggleSelect("b", true);
    expect(multiSelected()).toEqual(["a", "b"]);
    toggleSelect("a", true);
    expect(multiSelected()).toEqual(["b"]);
    toggleSelect("b", false);
    expect(multiSelected()).toEqual([]);
  });

  it("编组平移：相对位置误差 0px", () => {
    const ops = [op("a", 0, 0), op("b", 100, 50), op("c", 300, 200)];
    const moved = translateGroup(ops, 37, -19);
    expect(moved.a).toEqual({ x: 37, y: -19 });
    const relBefore = ops[1].rect.x - ops[0].rect.x;
    const relAfter = moved.b.x - moved.a.x;
    expect(relAfter - relBefore).toBe(0);
  });

  it("一起贴靠：全员落到目标矩形", () => {
    const moved = snapGroupRects([op("a", 0, 0), op("b", 100, 50)], { x: 0, y: 0, w: 960, h: 1000 });
    expect(moved.a).toEqual({ x: 0, y: 0, w: 960, h: 1000 });
    expect(moved.b).toEqual({ x: 0, y: 0, w: 960, h: 1000 });
  });

  it("批量关闭计划恒带确认门", () => {
    const plan = planGroupClose([op("a", 0, 0), op("b", 1, 1)]);
    expect(plan.needsConfirm).toBe(true);
    expect(plan.winIds).toEqual(["a", "b"]);
  });
});

describe("V-25 焦点历史", () => {
  it("随机聚焦序列回溯正确（去重）", () => {
    const seq = ["a", "b", "b", "c", "a", "d", "d", "e", "a", "f"];
    seq.forEach(recordFocus);
    expect(focusBack()).toBe("a");
    expect(focusBack()).toBe("e");
    expect(focusBack()).toBe("d");
    expect(focusBack()).toBe("a");
    expect(focusBack()).toBe("c");
    expect(focusBack()).toBe("b");
    expect(focusBack()).toBe("a");
    expect(focusBack()).toBeNull(); // 栈底不循环
    expect(focusForward()).toBe("b");
    expect(focusForward()).toBe("c");
    expect(focusForward()).toBe("a");
    expect(focusForward()).toBe("d");
    expect(focusForward()).toBe("e");
    expect(focusForward()).toBe("a");
    expect(focusForward()).toBe("f");
    expect(focusForward()).toBeNull(); // 栈顶不循环
  });

  it("栈底后退返回 null（不循环）", () => {
    recordFocus("a");
    expect(focusBack()).toBeNull();
  });

  it("栈深 20：超出后最旧历史被裁剪", () => {
    for (let i = 0; i < 25; i++) recordFocus(`w${i}`);
    expect(focusHistoryCounts().back).toBe(20);
    let last: string | null = null;
    for (let i = 0; i < 20; i++) last = focusBack();
    expect(last).toBe("w4");
  });

  it("新聚焦截断前进分支（编辑器语义）", () => {
    recordFocus("a");
    recordFocus("b");
    focusBack();
    recordFocus("c");
    expect(focusHistoryCounts().forward).toBe(0);
    expect(focusForward()).toBeNull();
  });
});

describe("V-26 位置互换", () => {
  it("仅两个成员时可互换", () => {
    expect(swapEligible([op("a", 0, 0), op("b", 1, 1)] as never)).toBe(true);
    expect(swapEligible([op("a", 0, 0)] as never)).toBe(false);
  });

  it("同尺寸互换：几何直接对调（20 次几何正确）", () => {
    for (let i = 0; i < 20; i++) {
      const a = op("a", i, i * 2);
      const b = op("b", 500 + i, 300);
      const plan = planSwap(a, b);
      expect(plan.a.rect).toEqual(b.rect);
      expect(plan.b.rect).toEqual(a.rect);
      expect(plan.resized).toBe(false);
    }
  });

  it("异尺寸互换：并集区域居中", () => {
    const a = op("a", 0, 0, 400, 300);
    const b = op("b", 500, 100, 800, 600);
    const plan = planSwap(a, b);
    expect(plan.resized).toBe(true);
    expect(plan.a.rect).toEqual({ x: Math.round(650 - 200), y: Math.round(350 - 150), w: 400, h: 300 });
    expect(plan.b.rect).toEqual({ x: Math.round(650 - 400), y: Math.round(350 - 300), w: 800, h: 600 });
  });
});



