import type { VwmRect, VwmWin } from "./vwm";

/**
 * V-22 失联窗口救援（化境 · AI-2 窗口与键位路）：
 * 窗口几何在所有可视区之外（拔显示器 / DPI 突变 / 应用自救失败）时——
 * - 体检：启动、显示器配置变更事件、手动触发（事件驱动，不做持续后台监控）
 * - 拉回策略：保持尺寸、贴到主屏（工作区）最近可视边缘；绝不改窗口尺寸
 * - 体检报告如实返回找回数量，找回后不抢占焦点
 *
 * 本模块是 V-21/23/24/26/30 的几何计算地基（纯函数，可独立测试）。
 */

/** 判定一个矩形是否与可视工作区有可见交集（面积 > 0 才算可见）。 */
export function isVisibleInWorkArea(r: VwmRect, wa: VwmRect): boolean {
  const ix = Math.max(r.x, wa.x);
  const iy = Math.max(r.y, wa.y);
  const ix2 = Math.min(r.x + r.w, wa.x + wa.w);
  const iy2 = Math.min(r.y + r.h, wa.y + wa.h);
  return ix2 - ix > 0 && iy2 - iy > 0;
}

/**
 * 对单个失联窗口计算拉回几何：保持宽高，将窗口整体平移到工作区内，
 * 距其原位置最近的可视边缘（优先保留原相对位置感，不强制居中）。
 */
export function rescueRect(r: VwmRect, wa: VwmRect): VwmRect {
  const w = r.w;
  const h = r.h;
  // 横向：窗口整体在左外侧 → 贴左缘；在右外侧 → 贴右缘；跨界 → 夹取
  let x: number;
  if (r.x + w <= wa.x) x = wa.x;
  else if (r.x >= wa.x + wa.w) x = wa.x + wa.w - w;
  else x = Math.min(Math.max(r.x, wa.x), Math.max(wa.x, wa.x + wa.w - w));
  // 纵向同理
  let y: number;
  if (r.y + h <= wa.y) y = wa.y;
  else if (r.y >= wa.y + wa.h) y = wa.y + wa.h - h;
  else y = Math.min(Math.max(r.y, wa.y), Math.max(wa.y, wa.y + wa.h - h));
  // 工作区比窗口还小：仍然保尺寸（不做缩放，尊重应用布局），从工作区原点摆放
  if (w > wa.w) x = wa.x;
  if (h > wa.h) y = wa.y;
  return { x: Math.round(x), y: Math.round(y), w: Math.round(w), h: Math.round(h) };
}

export interface RescueReport {
  /** 失联窗口实例 id 列表（按体检顺序）。 */
  lost: string[];
  /** 拉回后的几何（id → 新矩形），仅失联窗口有条目。 */
  moved: Record<string, VwmRect>;
}

/**
 * 体检一批窗口：找出完全不可见的窗口并给出拉回几何。
 * 纯计算——调用方（启动/显示器变更/手动触发）负责把 moved 应用回 store。
 * 最小化窗口不算失联（用户主动收起）；最大化窗口以当前几何参与体检。
 */
export function healthCheck(wins: VwmWin[], wa: VwmRect): RescueReport {
  const lost: string[] = [];
  const moved: Record<string, VwmRect> = {};
  for (const w of wins) {
    if (w.minimized) continue;
    const rect: VwmRect = { x: w.x, y: w.y, w: w.w, h: w.h };
    if (isVisibleInWorkArea(rect, wa)) continue;
    lost.push(w.id);
    moved[w.id] = rescueRect(rect, wa);
  }
  return { lost, moved };
}
