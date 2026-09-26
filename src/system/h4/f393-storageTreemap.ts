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
 * Squarified treemap（Bruls et al. 完整算法 · v4 深化升级）：按字节量降序，
 * 逐行贪心装箱——行内矩形沿短边铺开，行宽高比开始恶化即收行换向；
 * 面积占比 = 字节占比（±2% 对账的算法保证），且长宽比显著优于切分式布图。
 */
export function treemap(node: FsNode, canvas: Rect, depth = 0, maxDepth = 3): TreemapResult {
  const total = measureDir(node, 0).bytes;
  const tiles: Tile[] = [];
  layout(node, canvas, total, depth, maxDepth, tiles);
  return { tiles, totalBytes: total };
}

interface Weighted {
  node: FsNode;
  bytes: number;
}

/** 行内矩形宽高比的最差值（贪心装箱的判据：恶化即收行）。 */
function worstRowRatio(row: Weighted[], rowSum: number, w: number, h: number, remainingSum: number): number {
  if (rowSum === 0 || remainingSum === 0 || w === 0 || h === 0) return Number.POSITIVE_INFINITY;
  const horizontal = w >= h;
  const thickness = (rowSum / remainingSum) * (horizontal ? h : w);
  let worst = 0;
  for (const it of row) {
    const len = (it.bytes / rowSum) * (horizontal ? w : h);
    const ratio = len === 0 ? Number.POSITIVE_INFINITY : Math.max(len / thickness, thickness / len);
    worst = Math.max(worst, ratio);
  }
  return worst;
}

/** Squarified 装箱：子节点矩形排布（经典贪心——逐步加项直到宽高比恶化）。 */
function squarifyChildren(items: Weighted[], rect: Rect): Array<{ node: FsNode; rect: Rect }> {
  const out: Array<{ node: FsNode; rect: Rect }> = [];
  const remaining: Rect = { ...rect };
  let remainingSum = items.reduce((s, it) => s + it.bytes, 0);
  let i = 0;
  while (i < items.length) {
    const { w, h } = remaining;
    const horizontal = w >= h;
    let row: Weighted[] = [items[i]!];
    let rowSum = items[i]!.bytes;
    let worst = worstRowRatio(row, rowSum, w, h, remainingSum);
    let j = i + 1;
    while (j < items.length) {
      const candidate = [...row, items[j]!];
      const candidateWorst = worstRowRatio(candidate, rowSum + items[j]!.bytes, w, h, remainingSum);
      if (candidateWorst <= worst) {
        row = candidate;
        rowSum += items[j]!.bytes;
        worst = candidateWorst;
        j++;
      } else break;
    }
    const thickness = remainingSum === 0 ? 0 : (rowSum / remainingSum) * (horizontal ? h : w);
    let offset = horizontal ? remaining.x : remaining.y;
    const span = horizontal ? remaining.w : remaining.h;
    for (const it of row) {
      const len = rowSum === 0 ? 0 : (it.bytes / rowSum) * span;
      const r: Rect = horizontal
        ? { x: offset, y: remaining.y, w: len, h: thickness }
        : { x: remaining.x, y: offset, w: thickness, h: len };
      out.push({ node: it.node, rect: r });
      offset += len;
    }
    if (horizontal) remaining.y += thickness;
    else remaining.x += thickness;
    if (horizontal) remaining.h = Math.max(0, remaining.h - thickness);
    else remaining.w = Math.max(0, remaining.w - thickness);
    remainingSum -= rowSum;
    i = j;
  }
  return out;
}

function layout(node: FsNode, rect: Rect, parentBytes: number, depth: number, maxDepth: number, out: Tile[]): void {
  const bytes = measureDir(node, 0).bytes;
  out.push({ path: node.path, bytes, rect, heat: parentBytes === 0 ? 0 : bytes / parentBytes, depth, isDir: node.isDir });
  if (!node.isDir || depth >= maxDepth) return;
  const children = [...(node.children ?? [])].map((c) => ({ node: c, bytes: measureDir(c, 0).bytes })).filter((c) => c.bytes > 0).sort((a, b) => b.bytes - a.bytes);
  if (children.length === 0) return;
  for (const placed of squarifyChildren(children, rect)) {
    layout(placed.node, placed.rect, bytes, depth + 1, maxDepth, out);
  }
}

/** 布局质量（v4 新增）：全 tile 最差长宽比——squarified 装箱相对切分式的收益度量。 */
export function worstAspectRatio(result: TreemapResult): number {
  let worst = 1;
  for (const t of result.tiles) {
    if (t.rect.w <= 0 || t.rect.h <= 0) continue;
    worst = Math.max(worst, Math.max(t.rect.w / t.rect.h, t.rect.h / t.rect.w));
  }
  return Math.round(worst * 100) / 100;
}

/** 质量审计：最差长宽比 ≤ 阈值（默认 4）判合格——下钻三层内矩形可辨识（判据「下钻流畅」的视觉面）。 */
export function layoutQuality(result: TreemapResult, threshold = 4): { worst: number; pass: boolean } {
  const worst = worstAspectRatio(result);
  return { worst, pass: worst <= threshold };
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
