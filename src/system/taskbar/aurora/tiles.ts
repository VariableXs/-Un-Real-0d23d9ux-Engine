/**
 * AURORA-10000 领域04 · 族0082 开始菜单磁贴 + 族0084 开始菜单个性（AI-17 批次，勿删）。
 * 磁贴模型：规格/分组/多页/置顶/隐藏/导入导出/重置/A-B + 外观参数。
 */
import { getD4 } from "./prefs";

export type TileSize = "small" | "medium" | "wide" | "large";

export interface Tile {
  appId: string;
  size: TileSize;
  group: string;
  /** 置顶（F02036）。 */
  pinned: boolean;
  /** 暂时隐藏（F02037）。 */
  hidden: boolean;
  /** 单贴透明度 0~1（F02033）。 */
  opacity: number;
}

export interface TileGroup { name: string; folded: boolean; }

/** 规格→格数（跨列 F02044：宽贴跨 2 列）。 */
export const TILE_SPAN: Record<TileSize, { w: number; h: number }> = {
  small: { w: 1, h: 1 }, medium: { w: 2, h: 2 }, wide: { w: 4, h: 2 }, large: { w: 4, h: 4 },
};

/** 默认磁贴布局（重置 F02041 用）。 */
export const DEFAULT_TILES: readonly Tile[] = [
  { appId: "write", size: "wide", group: "效率", pinned: true, hidden: false, opacity: 1 },
  { appId: "mind", size: "medium", group: "效率", pinned: true, hidden: false, opacity: 1 },
  { appId: "explorer", size: "medium", group: "系统", pinned: true, hidden: false, opacity: 1 },
  { appId: "code", size: "wide", group: "开发", pinned: false, hidden: false, opacity: 1 },
  { appId: "datavault", size: "small", group: "系统", pinned: false, hidden: false, opacity: 1 },
];

/** 排序：置顶优先，组内按 appId。 */
export function sortTiles(tiles: readonly Tile[]): Tile[] {
  return [...tiles]
    .filter((t) => !t.hidden)
    .sort((a, b) => (a.pinned === b.pinned ? (a.group === b.group ? a.appId.localeCompare(b.appId) : a.group.localeCompare(b.group)) : a.pinned ? -1 : 1));
}

/** 分组折叠（F02039）。 */
export function foldGroups(tiles: readonly Tile[], folded: ReadonlySet<string>): Map<string, Tile[]> {
  const m = new Map<string, Tile[]>();
  for (const t of sortTiles(tiles)) {
    if (folded.has(t.group)) continue;
    const arr = m.get(t.group) ?? [];
    arr.push(t);
    m.set(t.group, arr);
  }
  return m;
}

/** 磁贴换列布局（F02082 列数）：简单行优先铺放。 */
export function layoutTiles(tiles: readonly Tile[], cols: number): Array<{ tile: Tile; col: number; row: number }> {
  const out: Array<{ tile: Tile; col: number; row: number }> = [];
  let col = 0; let row = 0;
  for (const t of sortTiles(tiles)) {
    const span = TILE_SPAN[t.size].w;
    if (col + span > cols) { col = 0; row += 2; }
    out.push({ tile: t, col, row });
    col += span;
  }
  return out;
}

/** 外观参数（族0084）。 */
export interface StartLook {
  radius: string; cols: number; gap: number; opacity: number; blur: number;
  expand: "top" | "bottom"; greeting: string;
}

export function startLook(): StartLook {
  return {
    radius: getD4<string>("F02081") ?? "corner-3",
    cols: getD4<number>("F02082") ?? 4,
    gap: getD4<number>("F02084") ?? 8,
    opacity: getD4<number>("F02078") ?? 0.96,
    blur: getD4<number>("F02077") ?? 0.4,
    expand: getD4<"top" | "bottom">("F02088") ?? "top",
    greeting: getD4<string>("F02095") ?? "",
  };
}

/** 生日彩蛋（F02096）：MM-DD 匹配时替换招呼语。 */
export function greetingFor(now: Date, nickname: string): string {
  const base = startLook().greeting || (nickname ? `${nickname}，欢迎回来` : "欢迎回来");
  const md = `${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
  const birthday = (getD4<string>("F02096") ?? "") === md;
  return birthday ? `生日快乐！${base}` : base;
}
