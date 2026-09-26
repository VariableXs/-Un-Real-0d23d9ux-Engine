/**
 * F381 树形控件半选与记忆（H 域 · AI-H4）：
 * 树形选择（分类树/嵌套目录）：父项三态（全选实心/半选横杠/未选空心——子项部分选中
 * 时父项显半选）；勾父=子全选、取消父=子全消；展开/折叠态按节点记忆（重启后树还是你
 * 展开到的样子，F219 同源）；键盘可达（右方向键展开/左收起/空格勾选）。
 * 判据（主册 F381）：三态联动用例（勾/取消/半选传递）；展开态记忆（重启验证）；
 * 键盘四操作；半选视觉规范（横杠居中 60% 宽）。
 * 依赖锚点：F219 排序与视图记忆。
 * 存储键：variable:h4:f381
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

export interface TreeNode {
  id: string;
  children: TreeNode[];
}

/** 节点三态（判据：全选实心/半选横杠/未选空心）。 */
export type CheckState = "checked" | "indeterminate" | "unchecked";

/** 半选视觉规范（判据）：横杠居中、宽 60%。 */
export const INDETERMINATE_BAR = { widthPct: 60, centered: true } as const;

export interface TreeMemory {
  /** 勾选叶集合。 */
  checked: string[];
  /** 展开的节点集合（重启后还原——记忆判据）。 */
  expanded: string[];
}

const KEY = h4Key("f381");

export function loadMemory(store: KvStore = defaultStore()): TreeMemory {
  const v = readJson<TreeMemory>(store, KEY, { checked: [], expanded: [] }, (x): x is TreeMemory => !!x && Array.isArray((x as TreeMemory).checked) && Array.isArray((x as TreeMemory).expanded));
  return { checked: [...new Set(v.checked)], expanded: [...new Set(v.expanded)] };
}

export function saveMemory(mem: TreeMemory, store: KvStore = defaultStore()): boolean {
  return writeJson(store, KEY, { checked: [...new Set(mem.checked)], expanded: [...new Set(mem.expanded)] });
}

/** 叶节点判定。 */
export function isLeaf(node: TreeNode): boolean {
  return node.children.length === 0;
}

function collectLeaves(node: TreeNode, out: string[]): void {
  if (isLeaf(node)) {
    out.push(node.id);
    return;
  }
  for (const c of node.children) collectLeaves(c, out);
}

/** 子树全部叶 id。 */
export function leavesOf(node: TreeNode): string[] {
  const out: string[] = [];
  collectLeaves(node, out);
  return out;
}

/** 节点三态判定（半选传递判据的核心递归）。 */
export function checkStateOf(node: TreeNode, checked: ReadonlySet<string>): CheckState {
  const leaves = leavesOf(node);
  if (leaves.length === 0) return checked.has(node.id) ? "checked" : "unchecked";
  const hit = leaves.filter((l) => checked.has(l)).length;
  if (hit === 0) return "unchecked";
  if (hit === leaves.length) return "checked";
  return "indeterminate";
}

/**
 * 勾/取消节点：叶=直接翻转；父=子全选/全消（判据「勾父=子全选、取消父=子全消」）。
 * 半选节点上的勾选动作语义：补全为全选（半选→勾 = 把没选的补上）。
 */
export function toggleNode(node: TreeNode, checked: ReadonlySet<string>): string[] {
  const state = checkStateOf(node, checked);
  const leaves = leavesOf(node);
  const targets = leaves.length > 0 ? leaves : [node.id];
  if (state === "checked") return targets; // 调用方从集合中移除
  return targets; // unchecked/indeterminate → 全加入
}

/** 展开态切换（记忆判据）。 */
export function toggleExpanded(mem: TreeMemory, id: string): TreeMemory {
  const expanded = mem.expanded.includes(id) ? mem.expanded.filter((x) => x !== id) : [...mem.expanded, id];
  return { ...mem, expanded };
}

/** 键盘四操作（判据）：右=展开/进首子；左=收起/回父；空格=勾选；上下=移动（返回目标 id 与动作）。 */
export type TreeKeyAction = { key: "right" | "left" | "space" | "up" | "down"; nodeId: string };

export interface KeyOutcome {
  expanded?: string[];
  collapsed?: string[];
  checkToggled?: string;
  /** 焦点移动目标（上下键）。 */
  focusMovedTo?: string;
}

export function keyboardAction(tree: TreeNode, mem: TreeMemory, action: TreeKeyAction, parentOf: Map<string, string>): KeyOutcome {
  const node = findNode(tree, action.nodeId);
  if (!node) return {};
  switch (action.key) {
    case "right":
      if (isLeaf(node) || mem.expanded.includes(node.id)) return {};
      return { expanded: [node.id] };
    case "left":
      if (!mem.expanded.includes(node.id)) {
        const parent = parentOf.get(node.id);
        return parent ? { focusMovedTo: parent } : {};
      }
      return { collapsed: [node.id] };
    case "space": {
      const state = checkStateOf(node, new Set(mem.checked));
      const targets = toggleNode(node, new Set(mem.checked));
      void state;
      if (targets.length === 1 && targets[0] === node.id && isLeaf(node)) {
        const nowChecked = mem.checked.includes(node.id);
        return { checkToggled: node.id, ...(nowChecked ? {} : {}) };
      }
      return { checkToggled: node.id };
    }
    case "up":
    case "down": {
      // 树根永远视为已展开（根是视口本身，不依赖 expanded 记账）——键盘序 = 视觉序
      const flat = flatten(tree, new Set([...mem.expanded, tree.id]));
      const idx = flat.indexOf(action.nodeId);
      const next = action.key === "down" ? flat[idx + 1] : flat[idx - 1];
      return next ? { focusMovedTo: next } : {};
    }
  }
}

/** 深度优先扁平序（键盘上下移动的视觉序）。 */
export function flatten(node: TreeNode, expanded: ReadonlySet<string> = new Set()): string[] {
  const out: string[] = [node.id];
  if (!isLeaf(node) && expanded.has(node.id)) {
    for (const c of node.children) out.push(...flatten(c, expanded));
  }
  return out;
}

function findNode(root: TreeNode, id: string): TreeNode | null {
  if (root.id === id) return root;
  for (const c of root.children) {
    const hit = findNode(c, id);
    if (hit) return hit;
  }
  return null;
}

/** 半选视觉（判据）：横杠居中 60% 宽——渲染面约定一处定义。 */
export function indeterminateVisual(state: CheckState): { bar: boolean; widthPct: number; centered: boolean } {
  return { bar: state === "indeterminate", widthPct: INDETERMINATE_BAR.widthPct, centered: INDETERMINATE_BAR.centered };
}
