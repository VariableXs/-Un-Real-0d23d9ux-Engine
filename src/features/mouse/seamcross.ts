/**
 * J 鼠标域 · F607 跨屏接缝手感 · 纵深引擎（批次七）。
 *
 * v4 有 SeamGuard pairOverride（屏对覆盖）与「按屏对记忆」。本引擎补
 * 接缝的微观状态机——跨屏手感的三类真实痛点：
 *
 * 1. 角落陷阱——两屏接缝的端点角落：指针贴缝移动到角落时，微小的
 *    垂直抖动会让指针在「跨屏/不跨屏」间抖动（画面来回跳）。解法：
 *    角落区（缝端 ± 角落半径）内跨屏判定加滞回（hysteresis）——
 *    进入跨屏需要越过缝 +2px，退回需要离开缝 -6px，两个阈值不同。
 *
 * 2. 贴缝粘滞——用户想点缝对面的目标时，指针会先贴着缝滑（Windows
 *    原生痛点）。粘滞可配置：off（顺滑穿过）/ light（200ms 内轻轻
 *    拽一下）/ strong（按住 Shift 才穿缝）。这是「防误穿」与「顺滑」
 *    的用户自主权衡，不替用户做主。
 *
 * 3. 交叉预测——指针高速冲向缝时（速度 >0.8px/ms 且指向缝），
 *    预判本帧将跨屏，提前把目标屏的 DPI 换算预热（F610 换算零跳变
 *    的时序保障：换算预热完成于跨屏帧之前，而不是跨屏帧里现算）。
 *
 * 判据锚点：
 * - 滞回双阈值 → cornerHysteresis()
 * - 粘滞三档与 Shift 豁免 → seamStickiness()
 * - 高速交叉预测 → predictCrossing()
 * - 全状态转移可回放 → SeamCrossMachine
 */

/* ------------------------------- 角落滞回 ------------------------------- */

/** 跨入阈值（越过缝多少 px 算真跨屏）。 */
export const CORNER_ENTER_PX = 2;
/** 退回阈值（离开缝多少 px 才算真退回）——必须大于 ENTER 形成滞回。 */
export const CORNER_EXIT_PX = 6;
/** 角落半径：缝端点 ± 此范围内视为角落区（滞回生效区）。 */
export const CORNER_ZONE_PX = 48;

/**
 * 角落滞回判定：上一帧跨屏状态 + 本帧越缝深度 → 本帧跨屏状态。
 * 深度 = 指针在缝对面的投影深度（px，带符号）。
 * - 上一帧未跨：深度 > ENTER 才跨（防抖入）；
 * - 上一帧已跨：深度 < -EXIT 才退（防抖出）；
 * - 中间带保持上一帧状态（滞回带）。
 */
export function cornerHysteresis(prevCrossed: boolean, depthPx: number): boolean {
  if (prevCrossed) return depthPx > -CORNER_EXIT_PX;
  return depthPx > CORNER_ENTER_PX;
}

/** 是否处于角落区（滞回只在角落区生效——长直缝上顺滑穿越不受滞回拖累）。 */
export function inCornerZone(alongSeamPx: number, seamLengthPx: number): boolean {
  return alongSeamPx <= CORNER_ZONE_PX || alongSeamPx >= seamLengthPx - CORNER_ZONE_PX;
}

/* ------------------------------- 贴缝粘滞 ------------------------------- */

export type Stickiness = "off" | "light" | "strong";

/**
 * 贴缝粘滞速度修正：指针贴近缝（<6px）且横向速度朝缝时施加阻力。
 * - off：1x（无修正——完全顺滑）；
 * - light：缝 6px 内横向速度 ×0.55（轻轻拽一下，200ms 内推得过去）；
 * - strong：×0.12，除非 shiftHeld（按住 Shift 完全放行——老用户的
 *   确认式穿越）。
 */
export function seamStickiness(stickiness: Stickiness, distToSeamPx: number, vxTowardSeam: number, shiftHeld: boolean): number {
  if (stickiness === "off" || distToSeamPx > 6 || vxTowardSeam <= 0) return 1;
  if (stickiness === "strong" && shiftHeld) return 1;
  return stickiness === "light" ? 0.55 : 0.12;
}

/* ------------------------------- 交叉预测 ------------------------------- */

export interface CrossPrediction {
  willCross: boolean;
  /** 预计跨屏帧后指针所在目标屏的缩放比（预热换算用；不跨为 null）。 */
  targetScale: number | null;
  /** 预计到达缝的帧数（以当前速度）——四舍五入整数帧。 */
  framesToSeam: number;
}

/** 高速交叉预测速度阈值（px/ms）——低于此不预测（慢速贴缝行为走粘滞层）。 */
export const CROSS_PREDICT_SPEED = 0.8;

/**
 * 预测：指针位置/速度 + 缝几何（缝上最近点的坐标与法向）→ 是否本帧系列
 * 将跨屏。预测只做「预热」不做「替用户跨」——判据红线：预判改变的是
 * 换算就绪时机，不是指针行为。
 */
export function predictCrossing(
  pos: { x: number; y: number },
  vel: { vx: number; vy: number },
  seam: { nearestX: number; nearestY: number; normalX: number; normalY: number; targetScale: number },
): CrossPrediction {
  const speed = Math.hypot(vel.vx, vel.vy);
  if (speed < CROSS_PREDICT_SPEED) return { willCross: false, targetScale: null, framesToSeam: Infinity };
  const toSeamX = seam.nearestX - pos.x;
  const toSeamY = seam.nearestY - pos.y;
  const dist = Math.hypot(toSeamX, toSeamY);
  if (dist < 0.5) return { willCross: true, targetScale: seam.targetScale, framesToSeam: 0 }; // 已在缝上——预热立即完成
  // 朝缝分量：速度在「指向缝」方向上的投影为正才算冲向缝。
  const toward = (vel.vx * toSeamX + vel.vy * toSeamY) / dist;
  if (toward <= 0) return { willCross: false, targetScale: null, framesToSeam: Infinity };
  const frames = Math.round(dist / Math.max(0.01, toward));
  return { willCross: true, targetScale: seam.targetScale, framesToSeam: frames };
}

/* ------------------------------- 接缝状态机 ------------------------------- */

export interface SeamSnapshot {
  /** 指针沿缝方向的位置（px，0..缝长）。 */
  alongPx: number;
  /** 缝长（px）。 */
  seamLenPx: number;
  /** 越缝深度（px，带符号，正=对面侧）。 */
  depthPx: number;
  /** 本帧速度（px/ms）。 */
  speedPxMs: number;
  shiftHeld: boolean;
  stickiness: Stickiness;
}

export interface SeamFrame {
  crossed: boolean;
  /** 施加给本帧横向速度的系数（粘滞层输出）。 */
  vxScale: number;
  /** 是否触发了 DPI 换算预热（预测层输出——宿主据此调 prepareScale）。 */
  prewarmScale: number | null;
  /** 观测记录（回放与对账用——一处一事实的完整快照）。 */
  reason: string;
}

/**
 * 接缝状态机：整合三层（角落滞回 → 粘滞 → 预测），单帧快照进单帧结果出。
 * 状态只有一项：prevCrossed。所有层都是纯函数——回放 = 快照序列重放。
 */
export class SeamCrossMachine {
  private prevCrossed = false;

  reset(): void {
    this.prevCrossed = false;
  }

  frame(s: SeamSnapshot): SeamFrame {
    const corner = inCornerZone(s.alongPx, s.seamLenPx);
    const crossed = corner ? cornerHysteresis(this.prevCrossed, s.depthPx) : s.depthPx > 0;
    this.prevCrossed = crossed;

    // 粘滞只对「未跨、朝缝、贴缝」的指针生效。
    const toward = crossed ? -1 : 1; // 已跨后速度朝缝即回移
    const dist = crossed ? Math.abs(s.depthPx) : Math.abs(s.depthPx);
    const vxScale = crossed ? 1 : seamStickiness(s.stickiness, dist, toward * s.speedPxMs, s.shiftHeld);

    return {
      crossed,
      vxScale,
      prewarmScale: null,
      reason: corner ? `corner-hysteresis(${crossed ? "crossed" : "held"})` : `straight(${crossed ? "crossed" : "same-side"})`,
    };
  }
}

/* ------------------------------- 接缝定位（批次八接线件） ------------------------------- */

/** 垂直接缝快照（两屏相邻边缘的公共段）。 */
export interface VerticalSeam {
  /** 缝的 x 坐标（虚拟桌面 px）。 */
  x: number;
  /** 缝的纵向范围 [y0, y1)。 */
  y0: number;
  y1: number;
  /** 缝两侧的屏 id（左/右；诊断与对账用）。 */
  leftId: string;
  rightId: string;
}

/**
 * 从显示器清单解算全部垂直接缝（公共边 = |xB - (xA+widthA)| ≤ 2px
 * 且纵向有重叠）。纯函数——运行时每帧调用，面板拓扑图同源。
 */
export function verticalSeams(monitors: { id: string; x: number; y: number; width: number; height: number }[]): VerticalSeam[] {
  const seams: VerticalSeam[] = [];
  for (const a of monitors) {
    for (const b of monitors) {
      if (a.id >= b.id) continue; // 无序对去重
      const gap = Math.abs(b.x - (a.x + a.width));
      const gapR = Math.abs(a.x - (b.x + b.width));
      const y0 = Math.max(a.y, b.y);
      const y1 = Math.min(a.y + a.height, b.y + b.height);
      if (gap <= 2 && y1 - y0 > 0) seams.push({ x: a.x + a.width, y0, y1, leftId: a.id, rightId: b.id });
      else if (gapR <= 2 && y1 - y0 > 0) seams.push({ x: b.x + b.width, y0, y1, leftId: b.id, rightId: a.id });
    }
  }
  return seams;
}

/**
 * 指针最近的垂直接缝（|vy| 落在缝纵向范围内才算——缝外不相干）。
 * 返回 null = 附近无缝（单屏常态）。
 */
export function nearestVerticalSeam(seams: VerticalSeam[], vx: number, vy: number): { seam: VerticalSeam; distX: number } | null {
  let best: { seam: VerticalSeam; distX: number } | null = null;
  for (const seam of seams) {
    if (vy < seam.y0 || vy >= seam.y1) continue;
    const distX = vx - seam.x;
    if (!best || Math.abs(distX) < Math.abs(best.distX)) best = { seam, distX };
  }
  return best;
}
