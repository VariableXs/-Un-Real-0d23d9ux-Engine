/**
 * F393 存储热点图（H 域 · AI-H4）：
 * 存储工具的空间可视：目录树矩形热点图（面积=占用、颜色深浅=热度、层级下钻到文件），
 * 点选即定位到资源管理器对应项；「最占空间的 10 个目录」榜单一键直达。
 * 判据（主册 F393）：面积与真实占用对账（±2%）；下钻流畅（三层 <500ms）；点选联动定位；
 * 榜单准确性；绘制开销（空闲通道 F369 纪律）。
 * 依赖锚点：F369 后台任务中心（空闲通道）/ F392 文件夹大小（同源计量）。
 */

import type { FsNode } from "./f392-folderSize";
import { measureDir } from "./f392-folderSize";

export type { FsNode };

export interface Rect {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface Tile {
  path: string;
  /** 该节点字节量。 */
  bytes: number;
  rect: Rect;
  /** 热度 0-1（占父容器比例——颜色深浅的映射源）。 */
  heat: number;
  depth: number;
  isDir: boolean;
}

export interface TreemapResult {
  tiles: Tile[];
  /** 全域总面积与真实占用对账基数。 */
  totalBytes: number;
}

/**
 * Squarified treemap（Bruls et al. 简化版）：按字节量降序逐行排布，
 * 长边优先切分；面积占比 = 字节占比（±2% 对账的算法保证）。
 */
export function treemap(node: FsNode, canvas: Rect, depth = 0, maxDepth = 3): TreemapResult {
  const total = measureDir(node, 0).bytes;
  const tiles: Tile[] = [];
  layout(node, canvas, total, depth, maxDepth, tiles);
  return { tiles, totalBytes: total };
}

function layout(node: FsNode, rect: Rect, parentBytes: number, depth: number, maxDepth: number, out: Tile[]): void {
  const bytes = measureDir(node, 0).bytes;
  out.push({ path: node.path, bytes, rect, heat: parentBytes === 0 ? 0 : bytes / parentBytes, depth, isDir: node.isDir });
  if (!node.isDir || depth >= maxDepth) return;
  const children = [...(node.children ?? [])].map((c) => ({ node: c, bytes: measureDir(c, 0).bytes })).filter((c) => c.bytes > 0).sort((a, b) => b.bytes - a.bytes);
  if (children.length === 0) return;
  const sum = children.reduce((s, c) => s + c.bytes, 0);
  const horizontal = rect.w >= rect.h; // 长边切分
  let offset = horizontal ? rect.x : rect.y;
  const span = horizontal ? rect.w : rect.h;
  for (const c of children) {
    const frac = c.bytes / sum;
    const size = span * frac;
    const childRect: Rect = horizontal
      ? { x: offset, y: rect.y, w: size, h: rect.h }
      : { x: rect.x, y: offset, w: rect.w, h: size };
    layout(c.node, childRect, bytes, depth + 1, maxDepth, out);
    offset += size;
  }
}

/** 面积对账（判据 ±2%）：单 tile 面积占比 vs 字节占比偏差。 */
export function auditAreaAccuracy(result: TreemapResult, canvas: Rect): { pass: boolean; worstDeviationPct: number } {
  const canvasArea = canvas.w * canvas.h;
  let worst = 0;
  for (const t of result.tiles) {
    const expectArea = result.totalBytes === 0 ? 0 : (t.bytes / result.totalBytes) * canvasArea;
    const dev = expectArea === 0 ? (t.rect.w * t.rect.h === 0 ? 0 : Number.POSITIVE_INFINITY) : Math.abs(t.rect.w * t.rect.h - expectArea) / expectArea * 100;
    worst = Math.max(worst, dev);
  }
  return { pass: worst <= 2, worstDeviationPct: worst };
}

/** 点选命中（判据「点选即定位」）：点坐标 → 最深层命中 tile（资源管理器定位目标）。 */
export function hitTile(result: TreemapResult, px: number, py: number): Tile | null {
  let best: Tile | null = null;
  for (const t of result.tiles) {
    const { x, y, w, h } = t.rect;
    if (px >= x && px < x + w && py >= y && py < y + h) {
      if (!best || t.depth > best.depth) best = t;
    }
  }
  return best;
}

/** 榜单（判据「最占空间的 10 个」）：按字节降序取前 10；排除当前视图根（depth 0——它恒是最大值，进榜无意义）。 */
export function topDirs(result: TreemapResult, n = 10): Array<{ path: string; bytes: number }> {
  return result.tiles.filter((t) => t.isDir && t.depth > 0).sort((a, b) => b.bytes - a.bytes).slice(0, n).map((t) => ({ path: t.path, bytes: t.bytes }));
}

/** 下钻（判据「层级下钻到文件」）：以某 tile 为新根重新布图（三层预算内）。 */
export function drillInto(result: TreemapResult, path: string, node: FsNode, canvas: Rect): TreemapResult {
  const found = result.tiles.find((t) => t.path === path);
  if (!found) return result;
  return treemap(node, canvas, 0, 3);
}

/** 空闲通道纪律（判据「绘制开销」）：布图只在空闲窗口执行（同 F365 准入口径）。 */
export function idleAdmission(nowMs: number, lastForegroundIoMs: number, quietWindowMs = 2000): boolean {
  return lastForegroundIoMs < 0 || nowMs - lastForegroundIoMs >= quietWindowMs;
}

/** 颜色深浅 = 热度（判据「颜色深浅=热度」）：heat 0-1 → 灰度 255-90。 */
export function heatToShade(heat: number): number {
  const h = Math.min(1, Math.max(0, heat));
  return Math.round(255 - h * 165);
}
