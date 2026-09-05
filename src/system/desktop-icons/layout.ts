/**
 * 批次B：桌面图标布局 store v2 —— 格子位置 + 文件架 + 排序方式。
 * - v1（仅 positions/autoArrange）自动迁移；文件架属于桌面 UI 层，存 localStorage
 * - 文件架（shelf）= 桌面启动器分组容器：可嵌套、可着色、可选链接真实文件夹
 *   （链接仅用于"在文件管理器中打开"定位，不产生文件操作）
 * - 成员图标本体（登记/软件定义）不受影响：归架只是"从桌面网格隐藏、从架中打开"
 */

export interface Cell {
  c: number;
  r: number;
}

export type ShelfColor = "blue" | "green" | "amber" | "rose" | "violet" | "slate";

export const SHELF_COLORS: ShelfColor[] = ["blue", "green", "amber", "rose", "violet", "slate"];

export interface ShelfDef {
  name: string;
  color: ShelfColor;
  /** 可选：链接的真实文件夹（双击飞出面板标题/右键 → 文件管理器定位）。 */
  linkedPath?: string;
  /** 归入的图标 id（app-* / tp-* / shelf-* 前缀，可嵌套文件架）。 */
  members: string[];
  /** 批次B：自定义图标（data URL，.ico/.png）。 */
  icon?: string;
}

export type SortMode = "type" | "name";

export interface DesktopLayout {
  autoArrange: boolean;
  positions: Record<string, Cell>;
  shelves: Record<string, ShelfDef>;
  sort: SortMode;
}

const LS_KEY = "variable:desktop:layout:v2";
const LS_KEY_V1 = "variable:desktop:layout:v1";

const DEFAULT_LAYOUT: DesktopLayout = { autoArrange: true, positions: {}, shelves: {}, sort: "type" };

export function newShelfId(): string {
  return `shelf-${Date.now().toString(36)}-${Math.floor(Math.random() * 1e6).toString(36)}`;
}

/** 归架 id 集合（含嵌套）：这些图标不出现在主网格。 */
export function allShelfMembers(l: DesktopLayout): Set<string> {
  const out = new Set<string>();
  for (const s of Object.values(l.shelves)) for (const m of s.members) out.add(m);
  return out;
}

/** shelf 是否是 target 的祖先（防环：嵌套上限 + 自归）。 */
export function shelfWouldCycle(l: DesktopLayout, shelfId: string, intoId: string): boolean {
  if (shelfId === intoId) return true;
  let cur: string | undefined = intoId;
  const seen = new Set<string>();
  while (cur && !seen.has(cur)) {
    seen.add(cur);
    if (cur === shelfId) return true;
    // 找到 cur 所在的父架
    const parent = Object.entries(l.shelves).find(([, s]) => s.members.includes(cur!));
    cur = parent?.[0];
  }
  return false;
}

export function loadDesktopLayout(): DesktopLayout {
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (raw) {
      const p = JSON.parse(raw) as Partial<DesktopLayout>;
      if (p && typeof p === "object") {
        return {
          autoArrange: typeof p.autoArrange === "boolean" ? p.autoArrange : true,
          positions: p.positions && typeof p.positions === "object" ? p.positions : {},
          shelves: p.shelves && typeof p.shelves === "object" ? p.shelves : {},
          sort: p.sort === "name" ? "name" : "type",
        };
      }
    }
    // v1 迁移（纯格子位置）
    const old = localStorage.getItem(LS_KEY_V1);
    if (old) {
      const p = JSON.parse(old) as { autoArrange?: boolean; positions?: Record<string, Cell> };
      if (p && typeof p.autoArrange === "boolean" && p.positions) {
        return { ...DEFAULT_LAYOUT, autoArrange: p.autoArrange, positions: p.positions };
      }
    }
  } catch {
    /* corrupted → defaults */
  }
  return { ...DEFAULT_LAYOUT };
}

export function saveDesktopLayout(l: DesktopLayout): void {
  try {
    localStorage.setItem(LS_KEY, JSON.stringify(l));
  } catch {
    /* storage full/blocked → won't persist */
  }
}

// ---------- 剪贴板（图标层：剪切=移动位置，复制=可克隆项的克隆） ----------

export interface IconClip {
  ids: string[];
  mode: "copy" | "cut";
}

// ---------- 撤销/重做（快照栈，模块级 —— 组件重挂不丢历史） ----------

const UNDO_MAX = 50;
let undoStack: DesktopLayout[] = [];
let redoStack: DesktopLayout[] = [];

export function pushUndo(prev: DesktopLayout): void {
  undoStack.push(prev);
  if (undoStack.length > UNDO_MAX) undoStack.shift();
  redoStack = [];
}

export function popUndo(): DesktopLayout | null {
  const prev = undoStack.pop();
  if (prev) return prev;
  return null;
}

export function pushRedo(cur: DesktopLayout): void {
  redoStack.push(cur);
}

export function popRedo(): DesktopLayout | null {
  return redoStack.pop() ?? null;
}
