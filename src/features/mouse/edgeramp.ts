/**
 * J 鼠标域 · F609 拖拽边缘自动滚 · 纵深引擎（批次七）。
 *
 * v3 的 edgeScrollSpeed 解决「深入距离→速率三档」。本引擎补三块
 * 真实工作流纵深：
 *
 * 1. 嵌套容器命中——拖到「列表在滚动区里」的位置（树面板在主滚动区
 *    内）该滚谁？Windows 原生行为是滚最内层可滚容器，但滚到尽头后
 *    「接力」给外层（接力判据：内层 at-end 且指针仍在边缘带）。
 *    不接力 = 拖到列表尽头就卡死，必须把指针挪出边缘再挪回来。
 *
 * 2. 动量交接——拖拽结束（松手）时若自动滚正在滚，滚动带 250ms
 *    余韵衰减（突然停止 = 内容「撞墙感」）；若松手时指针已离带，
 *    立即停（离带本身就是停止意图——两种停止都合理，按用户最后
 *    动作归因）。
 *
 * 3. 跨屏边缘延续——多屏拖拽跨越缝时，边缘带在缝两侧各算一次
 *    （拖到 A 屏右缘应滚 A，穿缝后拖到 B 屏左缘应滚 B）——嵌套
 *    命中按「指针所在屏」取容器树，缝不是容器边界。
 *
 * 判据锚点：
 * - 内层尽头接力 → resolveEdgeTarget()
 * - 松手余韵 vs 离带即停 → edgeReleaseCoast()
 * - 缝两侧各算一次 → sameScreenBands()
 */

/* ------------------------------- 嵌套容器命中 ------------------------------- */

export interface EdgeContainer {
  id: string;
  /** 可滚剩余量（px；垂直上下取小者——边缘滚只关心「还能滚吗」）。 */
  remainingPx: number;
  /** 嵌套深度（0=最外层；命中时取最内层优先）。 */
  depth: number;
}

/**
 * 命中裁决：指针下的容器栈 → 本帧该滚的容器。
 * 规则：最内层 remaining > 0 的容器胜；全部耗尽 → 最外层还有量的接力
 * （接力标记 carried=true——宿主据此切换滚动宿主并记一次体验日志）。
 */
export function resolveEdgeTarget(stack: EdgeContainer[]): { target: EdgeContainer | null; carried: boolean } {
  if (stack.length === 0) return { target: null, carried: false };
  // 最内层（深度最大）还有量 → 直接用它（非接力）。
  const maxDepth = Math.max(...stack.map((c) => c.depth));
  const innerMost = stack.find((c) => c.depth === maxDepth);
  if (innerMost && innerMost.remainingPx > 0) return { target: innerMost, carried: false };
  // 内层耗尽 → 从外层向内找第一个还有量的接力。
  const outer = [...stack].sort((a, b) => a.depth - b.depth).find((c) => c.remainingPx > 0);
  return { target: outer ?? null, carried: outer !== undefined };
}

/* ------------------------------- 松手处置 ------------------------------- */

/** 动量交接余韵时长（ms）。 */
export const EDGE_COAST_MS = 250;

export type ReleaseKind = "pointer-left-band" | "drop";

/**
 * 松手速率曲线：返回 tMs 时刻的速率保留比例。
 * - drop（原地松手）：指数余韵（内容滑稳收尾）；
 * - pointer-left-band（先离带后松手/直接离带）：立即 0（离带 = 停止意图）。
 */
export function edgeReleaseCoast(kind: ReleaseKind, tMs: number): number {
  if (kind === "pointer-left-band") return 0;
  if (tMs >= EDGE_COAST_MS) return 0;
  return Math.exp((-3 * tMs) / EDGE_COAST_MS);
}

/* ------------------------------- 跨屏边缘延续 ------------------------------- */

export interface ScreenBand {
  screenId: string;
  /** 本屏边缘带的世界坐标区间（x 轴口径；y 同构不重复建表）。 */
  bandStart: number;
  bandEnd: number;
  /** 本屏在带内的容器栈（跨缝两侧容器树不同——分别持栈）。 */
  stack: EdgeContainer[];
}

/**
 * 跨缝命中：指针 x → 所在屏的边缘带（缝两侧各算一次的判据本体）。
 * 双带重叠区（缝两侧 24px）按指针几何位置唯一归属——先命中者胜，
 * 不双滚（双滚 = 接力判据被滥用）。
 */
export function sameScreenBands(bands: ScreenBand[], x: number): ScreenBand | null {
  return bands.find((b) => x >= b.bandStart && x < b.bandEnd) ?? null;
}
