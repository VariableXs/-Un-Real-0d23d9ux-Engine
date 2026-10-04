/**
 * J 鼠标域 · F617 Protractor 形状手势识别引擎（v5 · 深化批次五）。
 *
 * v4 的八向编码只认「方向串」手势——画个圆、打个勾这类「形状」它无能为力
 * （方向串对圆是 8 个方向循环、对勾是两条斜线，语义全丢）。本模块引入
 * Protractor（Li, Wobbrock & Landay, UIST 2010）——$1 Unistroke 的快速变体：
 * 重采样 32 点 → 指示角对齐 → 归一化 → 余弦距离打分，无旋转搜索（快
 * 两个数量级，指针实时识别的正当理由）。
 *
 * 双引擎仲裁（一处一事实：派发出口仍走 gestures.recognize）：
 * 1. 方向串引擎先判（内置 12 手势 + 方向自定义——快路径，<1ms）；
 * 2. 未命中且登记了形状手势 → Protractor 精判（原始点列，置信度阈值 0.8）；
 * 3. 两引擎都未命中 → null（右键菜单兜底——零误伤铁律不变）。
 *
 * 形状手势存 gestures 分节 `shapes` 字段（模板为归一化 32 点向量）；
 * 录制台（面板）用 ShapeRecorder 收原始点列 → makeTemplate 归一化入库。
 */

import type { GestureLibraryConfig } from "./gestures";

export const PROTRACTOR_POINTS = 32;
/** 置信度阈值：低于此分不认（宁可回退菜单，不误触发）。 */
export const PROTRACTOR_THRESHOLD = 0.8;

export interface Pt {
  x: number;
  y: number;
}

/* ------------------------------- 几何原语 ------------------------------- */

export function pathLength(pts: Pt[]): number {
  let d = 0;
  for (let i = 1; i < pts.length; i++) {
    d += Math.hypot(pts[i]!.x - pts[i - 1]!.x, pts[i]!.y - pts[i - 1]!.y);
  }
  return d;
}

export function centroid(pts: Pt[]): Pt {
  const n = pts.length || 1;
  let sx = 0;
  let sy = 0;
  for (const p of pts) {
    sx += p.x;
    sy += p.y;
  }
  return { x: sx / n, y: sy / n };
}

/**
 * 等距重采样到 N 点（Protractor/$1 共同的第一步）：
 * 采样点间距 = 总长/(N-1)，沿路径弧长等距插值——手速快慢不影响模板形状。
 */
export function resample(pts: Pt[], n = PROTRACTOR_POINTS): Pt[] {
  if (pts.length < 2) return pts.map((p) => ({ ...p }));
  const total = pathLength(pts);
  if (total === 0) return Array.from({ length: n }, () => ({ ...pts[0]! }));
  const interval = total / (n - 1);
  const out: Pt[] = [{ ...pts[0]! }];
  let acc = 0;
  let prev = pts[0]!;
  for (let i = 1; i < pts.length; i++) {
    const cur = pts[i]!;
    let seg = Math.hypot(cur.x - prev.x, cur.y - prev.y);
    while (acc + seg >= interval && seg > 0) {
      const t = (interval - acc) / seg;
      const nx = prev.x + t * (cur.x - prev.x);
      const ny = prev.y + t * (cur.y - prev.y);
      out.push({ x: nx, y: ny });
      prev = { x: nx, y: ny };
      seg = Math.hypot(cur.x - prev.x, cur.y - prev.y);
      acc = 0;
    }
    acc += seg;
    prev = cur;
  }
  // 浮点尾巴：末点可能差一两个采样位——直接补齐到 N（首尾形状保真）。
  while (out.length < n) out.push({ ...pts[pts.length - 1]! });
  return out.slice(0, n);
}

/** 指示角：起点→质心的方向（Protractor 的旋转对齐基准）。 */
export function indicativeAngle(pts: Pt[]): number {
  const c = centroid(pts);
  return Math.atan2(c.y - pts[0]!.y, c.x - pts[0]!.x);
}

export function rotateBy(pts: Pt[], ang: number): Pt[] {
  const cos = Math.cos(ang);
  const sin = Math.sin(ang);
  const c = centroid(pts);
  return pts.map((p) => ({
    x: (p.x - c.x) * cos - (p.y - c.y) * sin + c.x,
    y: (p.x - c.x) * sin + (p.y - c.y) * cos + c.y,
  }));
}

/** 各向异性缩放到单位包围盒（Protractor 尺度归一——形状只留拓扑）。 */
export function scaleToUnit(pts: Pt[]): Pt[] {
  let minX = Infinity;
  let minY = Infinity;
  let maxX = -Infinity;
  let maxY = -Infinity;
  for (const p of pts) {
    if (p.x < minX) minX = p.x;
    if (p.y < minY) minY = p.y;
    if (p.x > maxX) maxX = p.x;
    if (p.y > maxY) maxY = p.y;
  }
  const w = maxX - minX || 1e-6;
  const h = maxY - minY || 1e-6;
  return pts.map((p) => ({ x: (p.x - minX) / w, y: (p.y - minY) / h }));
}

/** 平移到原点（质心归零——余弦距离的尺度无关前提）。 */
export function translateToOrigin(pts: Pt[]): Pt[] {
  const c = centroid(pts);
  return pts.map((p) => ({ x: p.x - c.x, y: p.y - c.y }));
}

/* ------------------------------- 模板与识别 ------------------------------- */

/** 模板向量：原始点列 → 重采样 → 指示角归零 → 尺度归一 → 质心归零。 */
export function makeTemplate(raw: Pt[]): number[] {
  let pts = resample(raw);
  pts = rotateBy(pts, -indicativeAngle(pts));
  pts = scaleToUnit(pts);
  pts = translateToOrigin(pts);
  return pts.flatMap((p) => [p.x, p.y]);
}

/** 归一化向量（余弦距离的分母）。 */
function normalizeVec(v: number[]): number[] {
  const mag = Math.hypot(...v) || 1e-9;
  return v.map((x) => x / mag);
}

/** Protractor 余弦距离：acos(单位向量点积)，钳制到 [0, π/2]。 */
export function protractorDistance(a: number[], b: number[]): number {
  const ua = normalizeVec(a);
  const ub = normalizeVec(b);
  let dot = 0;
  for (let i = 0; i < ua.length; i++) dot += ua[i]! * ub[i]!;
  return Math.acos(Math.max(-1, Math.min(1, dot)));
}

/** 置信度打分：距离 0 → 1 分；距离 π/2（正交=不像）→ 0 分。 */
export function protractorScore(dist: number): number {
  return Math.max(0, 1 - dist / (Math.PI / 2));
}

export interface ShapeGesture {
  name: string;
  action: string;
  /** 入库模板（makeTemplate 产出——只存归一化向量，不存原始点）。 */
  template: number[];
}

/**
 * Protractor 识别：样本模板 vs 模板库逐个打分，取最高分且 ≥ 阈值者。
 * 返回 null = 库空或全不达标（调用方走兜底——零误伤）。
 */
export function recognizeProtractor(
  sample: number[],
  shapes: Record<string, ShapeGesture>,
): { id: string; action: string; score: number } | null {
  let best: { id: string; action: string; score: number } | null = null;
  for (const [id, g] of Object.entries(shapes)) {
    if (!g?.template || g.template.length !== sample.length) continue;
    const score = protractorScore(protractorDistance(sample, g.template));
    if (!best || score > best.score) best = { id, action: g.action, score };
  }
  if (!best || best.score < PROTRACTOR_THRESHOLD) return null;
  return best;
}

/* ------------------------------- 形状录制器（面板消费） ------------------------------- */

/**
 * 形状录制器：begin → feed（原始点列，不过 GESTURE_STEP_PX 门槛——形状
 * 识别要细粒度弧长）→ finish 出模板。最少 12 点 / 最短 120px 弧长才成模板
 * （太短的形状既难画准也易误触）。
 */
export class ShapeRecorder {
  private raw: Pt[] = [];

  begin(x: number, y: number): void {
    this.raw = [{ x, y }];
  }

  feed(x: number, y: number): void {
    const last = this.raw[this.raw.length - 1]!;
    if (Math.hypot(x - last.x, y - last.y) < 1) return; // 原始亚像素抖动直滤
    this.raw.push({ x, y });
  }

  get trail(): Pt[] {
    return this.raw.map((p) => ({ ...p }));
  }

  get arcLength(): number {
    return pathLength(this.raw);
  }

  get ready(): boolean {
    return this.raw.length >= 12 && this.arcLength >= 120;
  }

  /** 出模板；不达标抛错（显性化——不静默给半成品模板）。 */
  finish(): number[] {
    if (!this.ready) {
      throw new Error(`[mouse-j1:F617] 形状录制不达标（点数 ${this.raw.length}/12，弧长 ${Math.round(this.arcLength)}/120px）——请画完整些`);
    }
    return makeTemplate(this.raw);
  }

  reset(): void {
    this.raw = [];
  }
}

/* ------------------------------- 形状查重（录制台消费） ------------------------------- */

/**
 * 形状查重（F617 判据「查重」在形状库的延伸——方向串录制台查重同源纪律）：
 * 新模板与既有形状逐一打分，相似度 ≥ 0.92 视为「同一画法」——拒绝入库并
 * 返回撞名者（用户重画或直接重绑那个形状；两笔近乎一样的轨迹共存只会让
 * 识别器随机二选一）。0.92 高于识别阈值 0.8：略见差异的画法允许共存。
 */
export function findDuplicateShape(
  template: number[],
  shapes: Record<string, ShapeGesture>,
  threshold = 0.92,
): { id: string; score: number } | null {
  let best: { id: string; score: number } | null = null;
  for (const [id, g] of Object.entries(shapes)) {
    if (!g?.template || g.template.length !== template.length) continue;
    const score = protractorScore(protractorDistance(template, g.template));
    if (!best || score > best.score) best = { id, score };
  }
  return best && best.score >= threshold ? best : null;
}

/* ------------------------------- 双引擎仲裁（接线件） ------------------------------- */

/**
 * 形状兜底判定（gestures.recognize 的第二引擎入口）：
 * 方向串未命中时用原始点列过 Protractor；命中返回与方向引擎同构的结果、
 * 置信度随行——派发侧（windowRuntime）零改动即得形状能力（横切不纵切）。
 */
export function shapeFallback(
  rawPts: Pt[],
  cfg: GestureLibraryConfig,
): { id: string; action: string; score: number } | null {
  const shapes = cfg.shapes;
  if (!shapes || Object.keys(shapes).length === 0 || rawPts.length < 12) return null;
  const sample = makeTemplate(rawPts);
  const hit = recognizeProtractor(sample, shapes);
  if (!hit) return null;
  // 重绑定一致性：形状手势与方向手势共用 bindings 表（动作才是可自定义面）。
  const bound = cfg.bindings?.[hit.id];
  return { ...hit, action: bound && bound.trim() ? bound.trim() : hit.action };
}
