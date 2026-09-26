import { describe, expect, it } from "vitest";
import { INDETERMINATE_BAR, checkStateOf, flatten, indeterminateVisual, keyboardAction, leavesOf, loadMemory, saveMemory, toggleExpanded, toggleNode, type TreeNode } from "../f381-treeTriState";
import { __clearMem, memStore } from "../internal/store";

/** 测试树：root → a(叶)/b(叶)/dir → c(叶)/d(叶)。 */
const TREE: TreeNode = {
  id: "root",
  children: [
    { id: "a", children: [] },
    { id: "b", children: [] },
    { id: "dir", children: [{ id: "c", children: [] }, { id: "d", children: [] }] },
  ],
};

const LEAVES = ["a", "b", "c", "d"];

describe("F381 树形控件半选与记忆", () => {
  it("三态联动：无选=空心、全选=实心、部分=半选（判据递归传递）", () => {
    expect(checkStateOf(TREE, new Set())).toBe("unchecked");
    expect(checkStateOf(TREE, new Set(LEAVES))).toBe("checked");
    expect(checkStateOf(TREE, new Set(["a", "c"]))).toBe("indeterminate");
    expect(checkStateOf(TREE.children[2]!, new Set(["c"]))).toBe("indeterminate");
    expect(checkStateOf(TREE.children[0]!, new Set(["a"]))).toBe("checked");
    expect(leavesOf(TREE)).toEqual(LEAVES);
  });

  it("勾父=子全选、取消父=子全消（判据）", () => {
    expect(toggleNode(TREE, new Set())).toEqual(LEAVES);
    expect(toggleNode(TREE, new Set(LEAVES))).toEqual(LEAVES); // 全选态 → 目标集合（供移除）
    const half = toggleNode(TREE.children[2]!, new Set(["c"]));
    expect(half).toEqual(["c", "d"]); // 半选 → 补全
    expect(toggleNode(TREE.children[0]!, new Set())).toEqual(["a"]);
  });

  it("展开态记忆：切换、去重、持久化 round-trip（重启验证判据的数据面）", () => {
    __clearMem();
    const s = memStore();
    let mem = toggleExpanded({ checked: [], expanded: [] }, "root");
    mem = toggleExpanded(mem, "root");
    mem = toggleExpanded(mem, "dir");
    expect(mem.expanded).toEqual(["dir"]);
    expect(saveMemory({ ...mem, checked: ["a"] }, s)).toBe(true);
    expect(loadMemory(s)).toEqual({ checked: ["a"], expanded: ["dir"] });
  });

  it("键盘四操作：右展开/左收起/空格勾选/上下移动（判据）", () => {
    let mem: ReturnType<typeof loadMemory> = { checked: [], expanded: [] };
    const parentOf = new Map<string, string>([["a", "root"], ["b", "root"], ["dir", "root"], ["c", "dir"], ["d", "dir"]]);
    // 右：展开 dir
    const r = keyboardAction(TREE, mem, { key: "right", nodeId: "dir" }, parentOf);
    expect(r.expanded).toEqual(["dir"]);
    mem = { ...mem, expanded: [...mem.expanded, "dir"] };
    // 左：收起 dir
    expect(keyboardAction(TREE, mem, { key: "left", nodeId: "dir" }, parentOf).collapsed).toEqual(["dir"]);
    // 左：已收起的叶 → 焦点回父
    expect(keyboardAction(TREE, { ...mem, expanded: [] }, { key: "left", nodeId: "c" }, parentOf).focusMovedTo).toBe("dir");
    // 空格：勾叶
    expect(keyboardAction(TREE, mem, { key: "space", nodeId: "a" }, parentOf).checkToggled).toBe("a");
    // 上下：扁平序移动
    const flat = flatten(TREE, new Set(["root", "dir"]));
    expect(flat).toEqual(["root", "a", "b", "dir", "c", "d"]);
    expect(keyboardAction(TREE, mem, { key: "down", nodeId: "b" }, parentOf).focusMovedTo).toBe("dir");
    expect(keyboardAction(TREE, mem, { key: "up", nodeId: "a" }, parentOf).focusMovedTo).toBe("root");
  });

  it("半选视觉规范：横杠居中 60% 宽（判据逐字）", () => {
    expect(INDETERMINATE_BAR).toEqual({ widthPct: 60, centered: true });
    expect(indeterminateVisual("indeterminate")).toEqual({ bar: true, widthPct: 60, centered: true });
    expect(indeterminateVisual("checked").bar).toBe(false);
  });

  it("收起状态下键盘序不含隐藏子节点", () => {
    expect(flatten(TREE, new Set(["root"]))).toEqual(["root", "a", "b", "dir"]);
  });
});
