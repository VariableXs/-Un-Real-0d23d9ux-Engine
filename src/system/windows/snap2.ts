import type { VwmRect } from "./vwm";

/**
 * U-14 智能窗口吸附 2.0（ASCENT-60 · AI-2 桌面窗口路）：
 * 拖拽窗口时的区域预测高亮与幽灵预览，落地为多列布局。
 * - 分区方案库：1/2、1/3×3（三分）、2+1、四象限、自定义网格（最长 3×3）；
 * - 幽灵预览为独立图层节点（frosted），与最终落位偏差 0px（同一 rect 来源）；
 * - 按住 Shift 临时禁用吸附；
 * - 纯几何计算，落位复用 vwm.snapVwmRect 既有实现（贴靠音效/落位逻辑不变）。
 */

/** 分区方案：把工作区划分为若干槽位。 */
export type ZoneLayout =
  | "half-left"
  | "half-right"
  | "third-1"
  | "third-2"
  | "third-3"
  | "two-plus-one-left"
  | "two-plus-one-right"
  | "quadrant-tl"
  | "quadrant-tr"
  | "quadrant-bl"
  | "quadrant-br"
  | { grid: { cols: number; rows: number; col: number; row: number } };

export interface ZoneSchema {
  id: string;
  /** 槽位描述（用于设置页预览与文档）。 */
  label: string;
}

/** 分区方案库（U-14 规格：1/2、1/3×3、2+1、四象限、自定义网格 ≤3×3）。 */
export const ZONE_LIBRARY: ZoneSchema[] = [
  { id: "half", label: "左右 1/2" },
  { id: "thirds", label: "三等分 1/3×3" },
  { id: "two-plus-one", label: "2+1" },
  { id: "quadrants", label: "四象限" },
  { id: "grid", label: "自定义网格（≤3×3）" },
];

function colRect(wa: VwmRect, cols: number, col: number, rows = 1, row = 0): VwmRect {
  const w = Math.floor(wa.w / cols);
  const h = Math.floor(wa.h / rows);
  // 最后一列/行吸收取整余数，保证铺满工作区（与最终落位 0px 偏差）
  const x = wa.x + col * w;
  const y = wa.y + row * h;
  const isLastCol = col === cols - 1;
  const isLastRow = row === rows - 1;
  return {
    x,
    y,
    w: isLastCol ? wa.w - col * w : w,
    h: isLastRow ? wa.h - row * h : h,
  };
}

/** 计算分区槽位在给定工作区内的目标矩形（视口局部 CSS 像素）。 */
export function zoneRect(layout: ZoneLayout, wa: VwmRect): VwmRect {
  if (typeof layout === "string") {
    switch (layout) {
      case "half-left":
        return colRect(wa, 2, 0);
      case "half-right":
        return colRect(wa, 2, 1);
      case "third-1":
        return colRect(wa, 3, 0);
      case "third-2":
        return colRect(wa, 3, 1);
      case "third-3":
        return colRect(wa, 3, 2);
      case "two-plus-one-left":
        // 左 2/3 上下分，右 1/3 通高
        return colRect(wa, 3, 0, 2, 0);
      case "two-plus-one-right":
        return colRect(wa, 3, 2);
      case "quadrant-tl":
        return colRect(wa, 2, 0, 2, 0);
      case "quadrant-tr":
        return colRect(wa, 2, 1, 2, 0);
      case "quadrant-bl":
        return colRect(wa, 2, 0, 2, 1);
      case "quadrant-br":
        return colRect(wa, 2, 1, 2, 1);
    }
  }
  const { cols, rows, col, row } = layout.grid;
  const c = Math.min(Math.max(cols, 1), 3);
  const r = Math.min(Math.max(rows, 1), 3);
  return colRect(wa, c, Math.min(Math.max(col, 0), c - 1), r, Math.min(Math.max(row, 0), r - 1));
}

/** 拖拽点（指针位置）→ 预测分区：按边缘/位置推断布局档。Shift 按住时返回 null（临时禁用）。 */
export function predictZone(
  px: number,
  py: number,
  wa: VwmRect,
  opts: { shiftHeld?: boolean } = {},
): { layout: ZoneLayout; rect: VwmRect } | null {
  if (opts.shiftHeld) return null;
  if (!snap2Enabled()) return null;
  const EDGE = 24;
  const nearLeft = px <= wa.x + EDGE;
  const nearRight = px >= wa.x + wa.w - EDGE;
  const nearTop = py <= wa.y + EDGE;
  const nearBottom = py >= wa.y + wa.h - EDGE;
  const thirdY = wa.y + wa.h / 3;
  const twoThirdsY = wa.y + (wa.h * 2) / 3;
  if (nearLeft && nearTop) return { layout: "quadrant-tl", rect: zoneRect("quadrant-tl", wa) };
  if (nearRight && nearTop) return { layout: "quadrant-tr", rect: zoneRect("quadrant-tr", wa) };
  if (nearLeft && nearBottom) return { layout: "quadrant-bl", rect: zoneRect("quadrant-bl", wa) };
  if (nearRight && nearBottom) return { layout: "quadrant-br", rect: zoneRect("quadrant-br", wa) };
  if (nearLeft) {
    // 左缘：上/中/下三分 → third-1/2/3，否则半屏
    if (py < thirdY) return { layout: "third-1", rect: zoneRect("third-1", wa) };
    if (py > twoThirdsY) return { layout: "third-3", rect: zoneRect("third-3", wa) };
    return { layout: "half-left", rect: zoneRect("half-left", wa) };
  }
  if (nearRight) {
    if (py < thirdY) return { layout: "third-2", rect: zoneRect("third-2", wa) };
    if (py > twoThirdsY) return { layout: "third-3", rect: zoneRect("third-3", wa) };
    return { layout: "half-right", rect: zoneRect("half-right", wa) };
  }
  if (nearTop) return { layout: "two-plus-one-right", rect: zoneRect("two-plus-one-right", wa) };
  if (nearBottom) return { layout: "two-plus-one-left", rect: zoneRect("two-plus-one-left", wa) };
  return null;
}

// ---------- U-14 吸附 2.0 总开关（编排中心设置页接线） ----------

const SNAP2_KEY = "variable:snap2:enabled:v1";

/** U-14 吸附 2.0 总开关（默认开；Shift 临时禁用是行为级，不受此开关影响）。 */
export function snap2Enabled(): boolean {
  try {
    return localStorage.getItem(SNAP2_KEY) !== "0";
  } catch {
    return true;
  }
}

/** 设置吸附 2.0 总开关（编排中心「窗口编排设置」区写入）。 */
export function setSnap2Enabled(on: boolean): void {
  try {
    localStorage.setItem(SNAP2_KEY, on ? "1" : "0");
  } catch {
    /* storage 不可用：内存态由下次读取回落默认 */
  }
}
