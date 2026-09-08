/**
 * V-28 窗口分布小地图（化境 · AI-2 窗口与键位路）：
 * 任务视图（Win+Tab）内「分布」视图：按虚拟桌面 × 显示器网格展示所有窗口
 * 缩略位置，点击直达（切换桌面+聚焦窗口）。
 * - 缩略图 = 窗口几何的等比缩放（缩略截图按需生成不常驻——性能红线）；
 * - 空桌面如实显示为空；不做小地图拖拽摆位（Z-40 领地）。
 *
 * 纯布局计算层：输入窗口几何与桌面/屏拓扑，输出缩略矩形，渲染由任务视图承担。
 */

import type { VwmRect } from "./vwm";

export interface MinimapWindow {
  winId: string;
  title: string;
  rect: VwmRect;
  minimized: boolean;
}

export interface MinimapCell {
  desktop: number;
  /** 显示器键（单显示器环境恒 "main"；多屏为显示器标识）。 */
  display: string;
  /** 该格工作区（用于等比换算）。 */
  workArea: VwmRect;
}

export interface MinimapPlacement {
  winId: string;
  title: string;
  /** 缩略几何（相对单元格左上角的 CSS 像素，含 4px 内边距由渲染层裁剪）。 */
  thumb: VwmRect;
  minimized: boolean;
}

export interface MinimapCellResult {
  desktop: number;
  display: string;
  /** 缩略窗口（最小化窗口仍显示其记忆位置，虚化呈现）。 */
  placements: MinimapPlacement[];
}

/** 单元格缩放比：缩略图最长边 ≤ cell 尺寸的等比系数（返回实际用的系数）。 */
export function minimapScale(cell: MinimapCell, cellW: number, cellH: number): number {
  const pad = 8;
  const sx = (cellW - pad * 2) / Math.max(1, cell.workArea.w);
  const sy = (cellH - pad * 2) / Math.max(1, cell.workArea.h);
  return Math.min(sx, sy);
}

/** 计算一个小地图单元格的窗口缩略布局。 */
export function minimapLayout(cell: MinimapCell, wins: MinimapWindow[], cellW: number, cellH: number): MinimapCellResult {
  const s = minimapScale(cell, cellW, cellH);
  const wa = cell.workArea;
  const pad = 8;
  const placements = wins.map((w) => ({
    winId: w.winId,
    title: w.title,
    minimized: w.minimized,
    thumb: {
      x: Math.round(pad + (w.rect.x - wa.x) * s),
      y: Math.round(pad + (w.rect.y - wa.y) * s),
      w: Math.max(6, Math.round(w.rect.w * s)),
      h: Math.max(4, Math.round(w.rect.h * s)),
    },
  }));
  return { desktop: cell.desktop, display: cell.display, placements };
}

/** 多桌面分布：为每个 (desktop, display) 组合生成布局（窗口按 desktopId 归属，未匹配到桌面的窗口归 0 号桌）。 */
export function minimapGrid(
  cells: MinimapCell[],
  winsByDesktop: Record<number, MinimapWindow[]>,
  cellW: number,
  cellH: number,
): MinimapCellResult[] {
  return cells.map((c) => minimapLayout(c, winsByDesktop[c.desktop] ?? [], cellW, cellH));
}

/** 点击直达：返回应聚焦的窗口 id（缩略命中检测；用于「点击直达 100%」验收）。 */
export function minimapHit(
  result: MinimapCellResult,
  px: number,
  py: number,
): string | null {
  for (const p of result.placements) {
    if (px >= p.thumb.x && px <= p.thumb.x + p.thumb.w && py >= p.thumb.y && py <= p.thumb.y + p.thumb.h) {
      return p.winId;
    }
  }
  return null;
}
