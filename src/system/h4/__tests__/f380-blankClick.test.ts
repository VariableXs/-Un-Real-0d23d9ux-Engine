import { describe, expect, it } from "vitest";
import { BACKGROUND_MENU_ITEMS, auditBackgroundMenu, bulkClearPerformance, clickBlank, ctrlClickBlank, rightClickBlank, rubberBandReleaseOnBlank, type ListSelection } from "../f380-blankClick";

function sel(n: number): ListSelection {
  return { selectedIds: Array.from({ length: n }, (_, i) => `i${i}`), anchorId: n > 0 ? "i0" : null };
}

describe("F380 空行点击清除选择", () => {
  it("单击空白：清全部选择+锚点、焦点回列表容器（判据）", () => {
    const r = clickBlank(sel(200));
    expect(r.selection).toEqual({ selectedIds: [], anchorId: null });
    expect(r.focusGoesTo).toBe("list-container");
    expect(r.focusedItemId).toBeNull();
  });

  it("Ctrl+单击空白：保留选择（手滑保护判据）且不共享引用", () => {
    const s = sel(3);
    const r = ctrlClickBlank(s);
    expect(r.selection.selectedIds).toEqual(s.selectedIds);
    expect(r.selection.selectedIds).not.toBe(s.selectedIds);
    expect(r.selection.anchorId).toBe("i0");
  });

  it("右键空白：弹背景菜单且与项上菜单明确区分（无删除类）", () => {
    const r = rightClickBlank(sel(2));
    expect(r.menu).toEqual([...BACKGROUND_MENU_ITEMS]);
    expect(r.selection.selectedIds).toEqual([]);
    const audit = auditBackgroundMenu(r.menu!);
    expect(audit.pass).toBe(true);
    expect(audit.forbidden).toEqual([]);
  });

  it("空白菜单审计：删除/重命名/剪切等无对象操作全部拦截（判据）", () => {
    const bad = auditBackgroundMenu(["paste", "delete", "rename"]);
    expect(bad.pass).toBe(false);
    expect(bad.forbidden).toEqual(["delete", "rename"]);
    const weird = auditBackgroundMenu(["paste", "selfDestruct"]);
    expect(weird.unknown).toEqual(["selfDestruct"]);
    expect(weird.pass).toBe(false);
  });

  it("与 F203 框选衔接：松手在空白=清除；松手在项上=保留框选结果", () => {
    const s = sel(5);
    expect(rubberBandReleaseOnBlank(s, true).selectedIds).toEqual([]);
    expect(rubberBandReleaseOnBlank(s, false).selectedIds).toHaveLength(5);
  });

  it("200 项误选后的救命单击 O(1) 清空", () => {
    expect(bulkClearPerformance(sel(200))).toEqual({ cleared: true, complexity: "O(1)" });
  });
});
