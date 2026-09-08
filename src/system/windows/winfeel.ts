import type { VwmRect } from "./vwm";

/**
 * AI-01 窗口手感组：纯函数层（无副作用、可单测）——
 * - M-01 detectShake：横向摇晃检测（指针样本 → 方向反转计数）
 * - Z-41 detectGesture：滑动手势识别（净位移主导轴）
 * - M-07 computeGuide：拖拽对齐参考线（边缘/中线吸附）
 * - Z-42/M-05 screenShift + parseScreenDetails：跨屏摆渡的坐标映射
 */

// ---------- M-01 摇晃检测 ----------

/**
 * 横向指针样本序列判定"摇晃"：
 * - 方向反转（dx 符号变化）≥ 2 次（右→左→右 或 左→右→左）
 * - 横向总行程（maxX - minX）> 60px
 * - 全程时长 < 1200ms
 * 样本不足 2 个 → false。
 */
export function detectShake(samples: Array<{ x: number; t: number }>): boolean {
  if (samples.length < 2) return false;
  let reversals = 0;
  let prevDir = 0;
  for (let i = 1; i < samples.length; i++) {
    const dx = samples[i]!.x - samples[i - 1]!.x;
    if (dx === 0) continue; // 静止帧不改变方向
    const dir = Math.sign(dx);
    if (prevDir !== 0 && dir !== prevDir) reversals++;
    prevDir = dir;
  }
  let minX = Infinity;
  let maxX = -Infinity;
  for (const s of samples) {
    if (s.x < minX) minX = s.x;
    if (s.x > maxX) maxX = s.x;
  }
  const duration = samples[samples.length - 1]!.t - samples[0]!.t;
  return reversals >= 2 && maxX - minX > 60 && duration < 1200;
}

// ---------- Z-41 手势识别 ----------

export type GestureDir = "left" | "right" | "up" | "down";

/**
 * 滑动手势：取路径首尾净位移的主导轴；主导轴位移 < threshold → null。
 */
export function detectGesture(path: Array<{ x: number; y: number }>, threshold = 48): GestureDir | null {
  if (path.length < 2) return null;
  const first = path[0]!;
  const last = path[path.length - 1]!;
  const dx = last.x - first.x;
  const dy = last.y - first.y;
  if (Math.abs(dx) >= Math.abs(dy)) {
    if (Math.abs(dx) < threshold) return null;
    return dx > 0 ? "right" : "left";
  }
  if (Math.abs(dy) < threshold) return null;
  return dy > 0 ? "down" : "up";
}

// ---------- M-07 对齐参考线 ----------

export interface GuideResult {
  /** 吸附后的 moving.x（null = X 方向无可吸附线）。 */
  snapX: number | null;
  /** 吸附后的 moving.y（null = Y 方向无可吸附线）。 */
  snapY: number | null;
  /** 阈值内的竖直参考线（目标线绝对坐标，去重升序）。 */
  guideXs: number[];
  /** 阈值内的水平参考线（目标线绝对坐标，去重升序）。 */
  guideYs: number[];
}

/**
 * 拖拽对齐：moving 的左/中/右边缘与上/中/下边缘，逐一比对候选矩形的
 * 各边缘 + 工作区中线（lines.xs / lines.ys）。
 * 吸附 = 距离最近且 ≤ threshold 的（moving 边缘, 目标线）配对；
 * 返回对齐后 moving 的 x / y，以及阈值内的全部目标线。
 */
export function computeGuide(
  candidates: VwmRect[],
  moving: VwmRect,
  threshold = 8,
  lines?: { xs: number[]; ys: number[] },
): GuideResult {
  const movingXs = [moving.x, moving.x + moving.w / 2, moving.x + moving.w];
  const movingYs = [moving.y, moving.y + moving.h / 2, moving.y + moving.h];
  const targetXs: number[] = [];
  const targetYs: number[] = [];
  for (const c of candidates) {
    targetXs.push(c.x, c.x + c.w / 2, c.x + c.w);
    targetYs.push(c.y, c.y + c.h / 2, c.y + c.h);
  }
  if (lines) {
    targetXs.push(...lines.xs);
    targetYs.push(...lines.ys);
  }

  const collect = (
    targets: number[],
    edges: number[],
    base: number,
  ): { snap: number | null; guides: number[] } => {
    let best: { delta: number; dist: number } | null = null;
    const guides: number[] = [];
    for (const t of targets) {
      let within = false;
      for (const e of edges) {
        const d = t - e;
        if (Math.abs(d) <= threshold) {
          within = true;
          if (!best || Math.abs(d) < best.dist) best = { delta: d, dist: Math.abs(d) };
        }
      }
      if (within) guides.push(t);
    }
    return {
      snap: best ? base + best.delta : null,
      guides: [...new Set(guides)].sort((a, b) => a - b),
    };
  };

  const gx = collect(targetXs, movingXs, moving.x);
  const gy = collect(targetYs, movingYs, moving.y);
  return { snapX: gx.snap, snapY: gy.snap, guideXs: gx.guides, guideYs: gy.guides };
}

// ---------- Z-42/M-05 跨屏摆渡 ----------

/** 显示器矩形（getScreenDetails 的最小面：OS 桌面坐标，已重定基则为视口相对坐标）。 */
export interface ScreenInfo {
  left: number;
  top: number;
  width: number;
  height: number;
}

/**
 * 从 Window Management API 的 ScreenDetails 抽取屏幕数组。
 * 统一重定基到 currentScreen 原点（视口局部坐标 ≈ 当前屏坐标）。
 * 结构不完整 → null。
 */
export function parseScreenDetails(details: unknown): ScreenInfo[] | null {
  if (!details || typeof details !== "object") return null;
  const d = details as { screens?: unknown; currentScreen?: unknown };
  if (!Array.isArray(d.screens)) return null;
  const cur = d.currentScreen as { left?: unknown; top?: unknown } | undefined;
  const baseL = typeof cur?.left === "number" ? cur.left : 0;
  const baseT = typeof cur?.top === "number" ? cur.top : 0;
  const screens: ScreenInfo[] = [];
  for (const s of d.screens) {
    if (!s || typeof s !== "object") continue;
    const sc = s as { left?: unknown; top?: unknown; width?: unknown; height?: unknown };
    if (typeof sc.width !== "number" || typeof sc.height !== "number") continue;
    screens.push({
      left: (typeof sc.left === "number" ? sc.left : 0) - baseL,
      top: (typeof sc.top === "number" ? sc.top : 0) - baseT,
      width: sc.width,
      height: sc.height,
    });
  }
  return screens.length > 0 ? screens : null;
}

/**
 * 水平摆渡：把窗口搬到相邻屏幕（rect 与 screens 同一坐标系）。
 * - 当前屏 = 包含窗口中心的屏（都不含 → 取第一块）
 * - 邻屏 = dir 方向上最近的一块（上下堆叠的屏不算水平邻居）
 * - x 平移两屏原点差后钳制进邻屏范围；y 不变
 * 无邻屏 → null。
 */
export function screenShift(rect: VwmRect, screens: ScreenInfo[], dir: "left" | "right"): VwmRect | null {
  if (screens.length === 0) return null;
  const cx = rect.x + rect.w / 2;
  const cy = rect.y + rect.h / 2;
  const cur =
    screens.find((s) => cx >= s.left && cx < s.left + s.width && cy >= s.top && cy < s.top + s.height) ??
    screens[0]!;
  const neighbor =
    dir === "right"
      ? screens.filter((s) => s.left >= cur.left + cur.width - 1).sort((a, b) => a.left - b.left)[0]
      : screens.filter((s) => s.left + s.width <= cur.left + 1).sort((a, b) => b.left + b.width - (a.left + a.width))[0];
  if (!neighbor) return null;
  const dx = neighbor.left - cur.left;
  const maxX = Math.max(neighbor.left, neighbor.left + neighbor.width - rect.w);
  const x = Math.min(Math.max(rect.x + dx, neighbor.left), maxX);
  return { x: Math.round(x), y: rect.y, w: rect.w, h: rect.h };
}
