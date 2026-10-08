/**
 * J 鼠标域 · 多屏拓扑引擎（v5 · 深化批次五 · F607）。
 *
 * v4 的 SeamGuard 是「逐事件问四方向」——能判穿越，但看不见整张拓扑图：
 * 接缝在哪、多长、两屏怎么相邻、混合 DPI 下逻辑/物理坐标怎么换算，都是
 * 散落在运行时里的即时计算。本模块把拓扑显性化为一等公民：
 * - 邻接求解：两屏共享边 → 接缝线段（区间求交——重叠部分才是真接缝）；
 * - 接缝距离：任意点到最近接缝的距离（护边可视化的数据层）；
 * - DPI 变换：逻辑↔物理坐标逐屏换算（125%/150% 的量化误差有明确的数学
 *   位置——F607 最丑角落「±1px 量化」从玄学变成可计算项）；
 * - 虚拟桌面包围盒：越界钳制与 F513 涟漪兜底的几何底盘。
 *
 * 消费端：跨屏拓扑面板（v5 新增，渲染接缝图与身份卡）+ 单测对拍。
 */

import type { MonitorInfo } from "./screen";

export type Axis = "v" | "h";

export interface SeamSegment {
  /** 两屏 id（字典序——与 seamPairKey 同方向纪律）。 */
  a: string;
  b: string;
  /** 接缝轴向：v=竖直缝（左右相邻），h=水平缝（上下相邻）。 */
  axis: Axis;
  /** 缝所在坐标（v 轴=两侧屏共享的 x 分界；h 轴=共享的 y 分界）。 */
  at: number;
  /** 缝的延伸区间 [from,to]（沿另一轴的重叠区间）。 */
  from: number;
  to: number;
}

/**
 * 接缝求解：两屏矩形共边判定 + 重叠区间求交。
 * 触碰长度 >0 才算接缝（仅角对角触碰不算——四角豁免的几何前提）。
 */
export function seamBetween(a: MonitorInfo, b: MonitorInfo): SeamSegment | null {
  // 竖直缝：a 在左（a.x+a.width ≈ b.x）或 b 在左——容差 1px（Tauri 取整尾巴）。
  const tol = 1;
  if (Math.abs(a.x + a.width - b.x) <= tol || Math.abs(b.x + b.width - a.x) <= tol) {
    const at = Math.abs(a.x + a.width - b.x) <= tol ? a.x + a.width : b.x + b.width;
    const from = Math.max(a.y, b.y);
    const to = Math.min(a.y + a.height, b.y + b.height);
    if (to > from) return { a: a.id < b.id ? a.id : b.id, b: a.id < b.id ? b.id : a.id, axis: "v", at, from, to };
  }
  if (Math.abs(a.y + a.height - b.y) <= tol || Math.abs(b.y + b.height - a.y) <= tol) {
    const at = Math.abs(a.y + a.height - b.y) <= tol ? a.y + a.height : b.y + b.height;
    const from = Math.max(a.x, b.x);
    const to = Math.min(a.x + a.width, b.x + b.width);
    if (to > from) return { a: a.id < b.id ? a.id : b.id, b: a.id < b.id ? b.id : a.id, axis: "h", at, from, to };
  }
  return null;
}

/** 全拓扑接缝集：两两求解（O(n²)，屏数 ≤8 —— 上限纪律在 F613）。 */
export function seamSegments(monitors: MonitorInfo[]): SeamSegment[] {
  const out: SeamSegment[] = [];
  for (let i = 0; i < monitors.length; i++) {
    for (let j = i + 1; j < monitors.length; j++) {
      const s = seamBetween(monitors[i]!, monitors[j]!);
      if (s) out.push(s);
    }
  }
  return out;
}

/** 点到一条接缝段的距离（标准点到线段距离）。 */
function distToSegment(px: number, py: number, from: number, to: number, at: number, axis: Axis): number {
  if (axis === "v") {
    if (py <= from) return Math.hypot(px - at, py - from);
    if (py >= to) return Math.hypot(px - at, py - to);
    return Math.abs(px - at);
  }
  if (px <= from) return Math.hypot(py - at, px - from);
  if (px >= to) return Math.hypot(py - at, px - to);
  return Math.abs(py - at);
}

/** 最近接缝与距离：护边可视化「离缝多远」的数据层（单屏返回 null）。 */
export function nearestSeam(monitors: MonitorInfo[], gx: number, gy: number): { seam: SeamSegment; dist: number } | null {
  let best: { seam: SeamSegment; dist: number } | null = null;
  for (const s of seamSegments(monitors)) {
    const d = distToSegment(gx, gy, s.from, s.to, s.at, s.axis);
    if (!best || d < best.dist) best = { seam: s, dist: d };
  }
  return best;
}

/** 虚拟桌面包围盒（越界钳制与涟漪兜底的几何底盘）。 */
export function virtualBounds(monitors: MonitorInfo[]): { x: number; y: number; width: number; height: number } {
  if (monitors.length === 0) return { x: 0, y: 0, width: 0, height: 0 };
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const m of monitors) {
    minX = Math.min(minX, m.x);
    minY = Math.min(minY, m.y);
    maxX = Math.max(maxX, m.x + m.width);
    maxY = Math.max(maxY, m.y + m.height);
  }
  return { x: minX, y: minY, width: maxX - minX, height: maxY - minY };
}

/** 点钳制进虚拟桌面（逐屏找最近落点——简化为包围盒钳制，够用且可解释）。 */
export function clampToDesktop(monitors: MonitorInfo[], gx: number, gy: number): { x: number; y: number } {
  const b = virtualBounds(monitors);
  return {
    x: Math.max(b.x, Math.min(b.x + b.width, gx)),
    y: Math.max(b.y, Math.min(b.y + b.height, gy)),
  };
}

/* ------------------------------- 混合 DPI 变换 ------------------------------- */

/** 该屏所属判定（screen.monitorAt 的本地复用——避免再 import 生成环）。 */
function monitorOf(monitors: MonitorInfo[], gx: number, gy: number): MonitorInfo | null {
  return monitors.find((m) => gx >= m.x && gx <= m.x + m.width && gy >= m.y && gy <= m.y + m.height) ?? null;
}

/**
 * 逻辑 → 物理（像素）坐标：所在屏 scale 相乘。F607 护边在物理像素口径
 * 判定（Tauri 多屏缓存即物理坐标）——换算位置显性化，量化误差有据可查。
 */
export function toPhysical(monitors: MonitorInfo[], gx: number, gy: number): { x: number; y: number; monitor: MonitorInfo | null } {
  const m = monitorOf(monitors, gx, gy);
  if (!m) return { x: gx, y: gy, monitor: null };
  return { x: gx * m.scale, y: gy * m.scale, monitor: m };
}

/**
 * 物理 → 逻辑坐标 + 量化残差：除以 scale 的取整尾巴（F607 最丑角落的
 * 「±1px 量化」本体）——残差显性返回，面板可标注、单测可断言 <0.5px。
 */
export function toLogical(monitors: MonitorInfo[], px: number, py: number): { x: number; y: number; residual: number; monitor: MonitorInfo | null } {
  const m = monitors.find((mm) => px >= mm.x * mm.scale && px <= (mm.x + mm.width) * mm.scale && py >= mm.y * mm.scale && py <= (mm.y + mm.height) * mm.scale) ?? null;
  if (!m) return { x: px, y: py, residual: 0, monitor: null };
  const x = px / m.scale;
  const y = py / m.scale;
  return { x, y, residual: Math.max(Math.abs(x - Math.round(x)), Math.abs(y - Math.round(y))), monitor: m };
}

/**
 * 屏对 DPI 组合预检（面板提示线）：两屏 scale 差异 >0.01 即混合 DPI——
 * 护边时序在物理口径下工作正常，但穿越瞬间的视觉连续性依赖合成器帧对拍
 * （判据「混合 DPI 双屏穿越帧对拍」的机器预检件，实机走查的替身探针）。
 */
export function isMixedDpi(monitors: MonitorInfo[]): boolean {
  const scales = new Set(monitors.map((m) => Math.round(m.scale * 100) / 100));
  return scales.size > 1;
}

/**
 * 拓扑摘要（面板一行卡）：屏数/接缝数/混合 DPI/包围盒——
 * 「一眼看清这张桌面长什么样」的单一数据源。
 */
export function topologySummary(monitors: MonitorInfo[]): { screens: number; seams: number; mixedDpi: boolean; bounds: string } {
  const b = virtualBounds(monitors);
  return {
    screens: monitors.length,
    seams: seamSegments(monitors).length,
    mixedDpi: isMixedDpi(monitors),
    bounds: `${Math.round(b.width)}×${Math.round(b.height)} @(${Math.round(b.x)},${Math.round(b.y)})`,
  };
}
