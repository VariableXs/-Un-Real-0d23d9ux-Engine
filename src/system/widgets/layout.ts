/**
 * N-09 小组件布局纯函数：
 * - 位置吸附 32px 网格；尺寸档 1x1/2x1/2x2（单元 128px + 8px 间距）；
 * - 互不遮挡：拖放落点若与其它卡片重叠，按螺形近邻搜索第一个无冲突格；
 * - 布局持久化 localStorage "variable:widgets:layout:v1"（Board.tsx 读写，这里只放纯逻辑）。
 */
export const WGT_GRID = 32;
export const WGT_CELL = 128;
export const WGT_GAP = 8;

export type WgtSize = "1x1" | "2x1" | "2x2";

export const SIZE_PX: Record<WgtSize, { w: number; h: number }> = {
  "1x1": { w: WGT_CELL, h: WGT_CELL },
  "2x1": { w: WGT_CELL * 2 + WGT_GAP, h: WGT_CELL },
  "2x2": { w: WGT_CELL * 2 + WGT_GAP, h: WGT_CELL * 2 + WGT_GAP },
};

export interface WgtRect { x: number; y: number; w: number; h: number }

export interface WgtLayoutItem {
  id: string;
  /** 桌面常驻形态下的像素位置（已吸附 32px 网格）。组件板形态忽略 x/y。 */
  x: number;
  y: number;
  size: WgtSize;
  /** 生命周期：连续失败 3 次自动收起（组件板里可手动恢复）。 */
  collapsed?: boolean;
}

export interface WgtLayoutDoc {
  version: 1;
  /** "desktop" = 桌面常驻；"board" = 组件板侧滑。 */
  mode: "desktop" | "board";
  items: WgtLayoutItem[];
}

export function snap(v: number): number {
  return Math.round(v / WGT_GRID) * WGT_GRID;
}

export function clampToViewport(x: number, y: number, size: WgtSize, vw: number, vh: number): { x: number; y: number } {
  const { w, h } = SIZE_PX[size];
  const maxX = Math.max(0, vw - w);
  const maxY = Math.max(0, vh - h);
  // 网格吸附向下取整：吸附后不得越过视口边界（钳制上限优先于网格对齐）
  const gridMaxX = Math.floor(maxX / WGT_GRID) * WGT_GRID;
  const gridMaxY = Math.floor(maxY / WGT_GRID) * WGT_GRID;
  return { x: Math.min(Math.max(0, x), gridMaxX), y: Math.min(Math.max(0, y), gridMaxY) };
}

export function itemRect(item: WgtLayoutItem): WgtRect {
  const { w, h } = SIZE_PX[item.size];
  return { x: item.x, y: item.y, w, h };
}

export function rectsOverlap(a: WgtRect, b: WgtRect): boolean {
  return a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h;
}

/** moved 与其余卡片是否冲突。 */
export function collides(items: WgtLayoutItem[], moved: WgtLayoutItem): boolean {
  const r = itemRect(moved);
  return items.some((o) => o.id !== moved.id && !o.collapsed && rectsOverlap(r, itemRect(o)));
}

/**
 * 拖放落点解析：吸附 → 视口钳制 → 冲突则螺形近邻搜索（步长 32px，半径 12 环），
 * 找不到无冲突位则回落到 (0,0)（仍冲突则保留钳制结果——桌面极小时才可能）。
 */
export function resolvePlacement(
  items: WgtLayoutItem[],
  movedId: string,
  rawX: number,
  rawY: number,
  vw: number,
  vh: number,
): WgtLayoutItem[] {
  const target = items.find((i) => i.id === movedId);
  if (!target) return items;
  const snapped = clampToViewport(snap(rawX), snap(rawY), target.size, vw, vh);
  const moved: WgtLayoutItem = { ...target, x: snapped.x, y: snapped.y };
  if (!collides(items, moved)) {
    return items.map((i) => (i.id === movedId ? moved : i));
  }
  for (let ring = 1; ring <= 12; ring++) {
    const step = WGT_GRID * ring;
    const candidates: { x: number; y: number }[] = [
      { x: snapped.x - step, y: snapped.y },
      { x: snapped.x + step, y: snapped.y },
      { x: snapped.x, y: snapped.y - step },
      { x: snapped.x, y: snapped.y + step },
      { x: snapped.x - step, y: snapped.y - step },
      { x: snapped.x + step, y: snapped.y - step },
      { x: snapped.x - step, y: snapped.y + step },
      { x: snapped.x + step, y: snapped.y + step },
    ];
    for (const c of candidates) {
      const p = clampToViewport(c.x, c.y, target.size, vw, vh);
      const cand: WgtLayoutItem = { ...target, x: p.x, y: p.y };
      if (!collides(items, cand)) {
        return items.map((i) => (i.id === movedId ? cand : i));
      }
    }
  }
  const origin: WgtLayoutItem = { ...target, x: 0, y: 0 };
  if (!collides(items, origin)) {
    return items.map((i) => (i.id === movedId ? origin : i));
  }
  return items.map((i) => (i.id === movedId ? moved : i));
}

// ---------- 布局持久化（含版本兼容：坏数据回默认空布局，默认无卡片） ----------

export const WGT_LAYOUT_LS_KEY = "variable:widgets:layout:v1";

export function sanitizeLayout(raw: unknown): WgtLayoutDoc {
  const doc = raw as Partial<WgtLayoutDoc> | null;
  if (!doc || doc.version !== 1 || !Array.isArray(doc.items)) {
    return { version: 1, mode: "desktop", items: [] };
  }
  const sizes: WgtSize[] = ["1x1", "2x1", "2x2"];
  const items = doc.items
    .filter((i): i is WgtLayoutItem => !!i && typeof i.id === "string" && typeof i.x === "number" && typeof i.y === "number")
    .map((i) => ({
      id: i.id,
      x: snap(Number(i.x) || 0),
      y: snap(Number(i.y) || 0),
      size: sizes.includes(i.size) ? i.size : "1x1",
      collapsed: !!i.collapsed,
    }));
  return { version: 1, mode: doc.mode === "board" ? "board" : "desktop", items };
}

export function loadLayout(): WgtLayoutDoc {
  try {
    const raw = localStorage.getItem(WGT_LAYOUT_LS_KEY);
    return raw ? sanitizeLayout(JSON.parse(raw) as unknown) : { version: 1, mode: "desktop", items: [] };
  } catch {
    return { version: 1, mode: "desktop", items: [] };
  }
}

export function saveLayout(doc: WgtLayoutDoc): void {
  try {
    localStorage.setItem(WGT_LAYOUT_LS_KEY, JSON.stringify(doc));
  } catch {
    /* storage full —— 会话内继续可用 */
  }
}