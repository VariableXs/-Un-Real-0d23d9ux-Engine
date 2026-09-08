import { describe, expect, it } from "vitest";
import { computeOverflow, MIN_ICON_WIDTH, type OverflowItem } from "../overflow";

/** V-17 / U-15：溢出折叠几何计算（固定常驻；装不下的非固定项进折叠菜单）。 */
function mk(id: string, width: number, pinned = false): OverflowItem {
  return { id, width, pinned };
}

describe("taskbar overflow (U-15 / V-17)", () => {
  it("空间充足时零溢出", () => {
    const items = [mk("a", 40), mk("b", 40), mk("c", 40)];
    expect(computeOverflow(items, 200)).toEqual([]);
  });

  it("容量不足时装不下的项进折叠区（从右往左优先保留，结果保持原顺序）", () => {
    const items = [mk("a", 40), mk("b", 40), mk("c", 40)];
    expect(computeOverflow(items, 90)).toEqual(["a"]);
    expect(computeOverflow(items, 50)).toEqual(["a", "b"]);
  });

  it("固定（pinned）图标永远在外（锁定常驻），其余全部溢出", () => {
    const items = [mk("a", 40), mk("p1", 40, true), mk("b", 40), mk("c", 40)];
    expect(computeOverflow(items, 50)).toEqual(["a", "b", "c"]);
    expect(computeOverflow(items, 40)).toEqual(["a", "b", "c"]);
  });

  it("图标被压缩到最小宽度以下按最小宽计（触发口径等价 20% 压缩）", () => {
    const items = [mk("a", 10), mk("b", 10)];
    expect(computeOverflow(items, MIN_ICON_WIDTH)).toEqual(["a"]);
  });

  it("全部 pinned 溢出时 pinned 仍保留在外", () => {
    const items = [mk("p1", 40, true), mk("p2", 40, true)];
    expect(computeOverflow(items, 40)).toEqual([]);
  });
});
