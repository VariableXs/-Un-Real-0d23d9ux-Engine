import type { VwmRect } from "../vwm";

/**
 * AURORA-10000 · 族0026 窗口吸附系统（F00626~F00650 · AI-06 · W1）。
 * 25 种吸附模式，每项为独立可交付单元；纯几何计算，落位复用 vwm.snapVwmRect。
 * 输入 ctx.drag 为被拖拽窗的期望矩形，ctx.work 为工作区，返回吸附后的矩形或 null（不吸附）。
 */

/** 吸附上下文：拖拽窗 + 工作区 + 屏幕区 + 可选邻居窗。 */
export interface SnapCtx {
  /** 被拖拽窗的期望位置（未吸附）。 */
  drag: VwmRect;
  /** 工作区（已扣任务栏）。 */
  work: VwmRect;
  /** 整屏区域（含任务栏区）。 */
  screen: VwmRect;
  /** 最近的邻居窗（无则 null）。 */
  neighbor: VwmRect | null;
  /** 网格步长（px），默认 8。 */
  grid?: number;
  /** 触发距离（px），默认 24。 */
  threshold?: number;
  /** 设备缩放（DPI 感知用），默认 1。 */
  scale?: number;
  /** 拖拽速度样本（意图预测用，px/ms）。 */
  velocity?: { vx: number; vy: number };
}

export interface SnapResult {
  rect: VwmRect;
  /** 命中的吸附类别（供提示线/音效）。 */
  hit: "edge" | "corner" | "neighbor" | "grid" | "center" | "zone" | "none";
  /** 对齐参考线（可选）。 */
  guides?: { x: number[]; y: number[] };
}

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const round = (v: number, step = 1) => Math.round(v / step) * step;

/** 是否靠近某条边（阈值内）。 */
function near(v: number, target: number, t: number): boolean {
  return Math.abs(v - target) <= t;
}

/**
 * 族0026 主入口：按 ID 分派的吸附候选。
 * 全部模式均为纯函数；未命中返回 null。
 */
export function snapCandidate(itemId: string, ctx: FullSnapCtx): SnapResult | null {
  const t = ctx.threshold ?? 24;
  const g = ctx.grid ?? 8;
  const { drag, work, screen } = ctx;
  const W = work.w;
  const H = work.h;
  const half = { x: work.x, y: work.y, w: Math.round(W / 2), h: H };
  const third = Math.round(W / 3);

  switch (itemId) {
    case "F00626": {
      // 吸附·屏幕边缘 — 贴边自动吸合并预览半屏
      if (near(drag.x, work.x, t)) return { rect: { ...half }, hit: "edge" };
      if (near(drag.x + drag.w, work.x + W, t))
        return { rect: { x: work.x + W - half.w, y: work.y, w: half.w, h: H }, hit: "edge" };
      return null;
    }
    case "F00627": {
      // 吸附·窗间对齐 — 与邻近窗边缘像素级对齐
      const n = ctx.neighbor;
      if (!n) return null;
      const xs = [n.x, n.x + n.w];
      const ys = [n.y, n.y + n.h];
      for (const nx of xs) {
        if (near(drag.x, nx, t)) return { rect: { ...drag, x: nx }, hit: "neighbor", guides: { x: [nx], y: [] } };
        if (near(drag.x + drag.w, nx, t))
          return { rect: { ...drag, x: nx - drag.w }, hit: "neighbor", guides: { x: [nx], y: [] } };
      }
      for (const ny of ys) {
        if (near(drag.y, ny, t)) return { rect: { ...drag, y: ny }, hit: "neighbor", guides: { x: [], y: [ny] } };
        if (near(drag.y + drag.h, ny, t))
          return { rect: { ...drag, y: ny - drag.h }, hit: "neighbor", guides: { x: [], y: [ny] } };
      }
      return null;
    }
    case "F00628": {
      // 吸附·桌面网格 — 拖动按网格步进
      const gx = round(clamp(drag.x, work.x, work.x + W - drag.w), g);
      const gy = round(clamp(drag.y, work.y, work.y + H - drag.h), g);
      if (gx !== round(drag.x, g) || gy !== round(drag.y, g) || drag.x % g !== 0 || drag.y % g !== 0) {
        return { rect: { ...drag, x: gx, y: gy }, hit: "grid" };
      }
      return null;
    }
    case "F00629": {
      // 吸附·左右对称 — 双窗对称位置吸附
      const n = ctx.neighbor;
      if (!n) return null;
      const cx = work.x + W / 2;
      const mirroredX = round(2 * cx - n.x - n.w);
      if (Math.abs(mirroredX - drag.x) <= t)
        return { rect: { ...drag, x: clamp(mirroredX, work.x, work.x + W - drag.w) }, hit: "neighbor" };
      return null;
    }
    case "F00630": {
      // 吸附·角落四分 — 拖到四角自动四分之一
      const qw = Math.round(W / 2);
      const qh = Math.round(H / 2);
      const cx = work.x + W / 2;
      const cy = work.y + H / 2;
      const left = drag.x + drag.w / 2 < cx;
      const top = drag.y + drag.h / 2 < cy;
      const inCornerX = near(left ? drag.x : drag.x + drag.w, left ? work.x : work.x + W, t * 2);
      const inCornerY = near(top ? drag.y : drag.y + drag.h, top ? work.y : work.y + H, t * 2);
      if (inCornerX && inCornerY)
        return {
          rect: { x: left ? work.x : work.x + W - qw, y: top ? work.y : work.y + H - qh, w: qw, h: qh },
          hit: "corner",
        };
      return null;
    }
    case "F00631": {
      // 吸附·任务栏避让 — 自动扣除任务栏高度（工作区夹取）
      const clamped: VwmRect = {
        x: clamp(drag.x, work.x, work.x + W - drag.w),
        y: clamp(drag.y, work.y, work.y + H - drag.h),
        w: Math.min(drag.w, W),
        h: Math.min(drag.h, H),
      };
      if (clamped.x !== drag.x || clamped.y !== drag.y) return { rect: clamped, hit: "edge" };
      return null;
    }
    case "F00632": {
      // 吸附·图标网格 — 与桌面图标网格对齐（默认 96×110 单元）
      const cell = { w: 96, h: 110 };
      const x = round(clamp(drag.x, work.x, work.x + W - drag.w), cell.w);
      const y = round(clamp(drag.y, work.y, work.y + H - drag.h), cell.h);
      return { rect: { ...drag, x, y }, hit: "grid" };
    }
    case "F00633": {
      // 吸附·屏幕中轴 — 水平/垂直居中吸附
      const cx = work.x + W / 2;
      const cy = work.y + H / 2;
      if (near(drag.x + drag.w / 2, cx, t))
        return { rect: { ...drag, x: Math.round(cx - drag.w / 2) }, hit: "center", guides: { x: [cx], y: [] } };
      if (near(drag.y + drag.h / 2, cy, t))
        return { rect: { ...drag, y: Math.round(cy - drag.h / 2) }, hit: "center", guides: { x: [], y: [cy] } };
      return null;
    }
    case "F00634": {
      // 吸附·等距分布 — 三窗以上自动等间距（槽位 = 序号/(n-1)）
      const slots = ctx.slots ?? 3;
      const idx = ctx.slotIndex ?? 0;
      const span = W - drag.w;
      const x = work.x + (slots <= 1 ? 0 : Math.round((span * idx) / (slots - 1)));
      return { rect: { ...drag, x }, hit: "zone" };
    }
    case "F00635": {
      // 吸附·黄金比例 — 0.618 分割吸附线
      const gw = Math.round(W * 0.618);
      if (near(drag.x, work.x, t)) return { rect: { x: work.x, y: work.y, w: gw, h: H }, hit: "zone" };
      if (near(drag.x + drag.w, work.x + W, t))
        return { rect: { x: work.x + W - gw, y: work.y, w: gw, h: H }, hit: "zone" };
      return null;
    }
    case "F00636": {
      // 吸附·三分线 — 摄影三分构图辅助线
      for (let i = 1; i <= 2; i++) {
        const lx = work.x + third * i;
        if (near(drag.x, lx, t)) return { rect: { ...drag, x: lx }, hit: "zone", guides: { x: [lx], y: [] } };
        if (near(drag.x + drag.w, lx, t))
          return { rect: { ...drag, x: lx - drag.w }, hit: "zone", guides: { x: [lx], y: [] } };
      }
      return null;
    }
    case "F00637": {
      // 吸附·磁吸链 — 多窗首尾相接成链
      const n = ctx.neighbor;
      if (!n) return null;
      const gap = 0;
      if (near(drag.x, n.x + n.w + gap, t))
        return { rect: { ...drag, x: n.x + n.w + gap }, hit: "neighbor" };
      if (near(drag.x + drag.w, n.x - gap, t)) return { rect: { ...drag, x: n.x - gap - drag.w }, hit: "neighbor" };
      return null;
    }
    case "F00638": {
      // 吸附·层叠错位 — 层叠窗 24px 阶梯吸附
      const n = ctx.neighbor;
      const step = 24;
      if (!n) return null;
      const x = clamp(n.x + step, work.x, work.x + W - drag.w);
      const y = clamp(n.y + step, work.y, work.y + H - drag.h);
      if (near(drag.x, x, t) && near(drag.y, y, t)) return { rect: { ...drag, x, y }, hit: "neighbor" };
      return null;
    }
    case "F00639": {
      // 吸附·等宽对齐 — 与邻窗同宽吸附
      const n = ctx.neighbor;
      if (!n) return null;
      return { rect: { ...drag, w: n.w, x: clamp(drag.x, work.x, work.x + W - n.w) }, hit: "neighbor" };
    }
    case "F00640": {
      // 吸附·等高对齐 — 与邻窗同高吸附
      const n = ctx.neighbor;
      if (!n) return null;
      return { rect: { ...drag, h: n.h, y: clamp(drag.y, work.y, work.y + H - n.h) }, hit: "neighbor" };
    }
    case "F00641": {
      // 吸附·像素对齐 — 非整数坐标取整
      const rx = Math.round(drag.x);
      const ry = Math.round(drag.y);
      if (rx !== drag.x || ry !== drag.y) return { rect: { ...drag, x: rx, y: ry }, hit: "none" };
      return null;
    }
    case "F00642": {
      // 吸附·DPI 感知 — 混合 DPI 下按缩放取整
      const s = ctx.scale ?? 1;
      const rx = Math.round(drag.x / s) * s;
      const ry = Math.round(drag.y / s) * s;
      if (rx !== drag.x || ry !== drag.y) return { rect: { ...drag, x: rx, y: ry }, hit: "none" };
      return null;
    }
    case "F00643": {
      // 吸附·多屏边界 — 跨屏时边缘减速提示（边界内夹取）
      const x = clamp(drag.x, screen.x, screen.x + screen.w - drag.w);
      if (x !== drag.x) return { rect: { ...drag, x }, hit: "edge" };
      return null;
    }
    case "F00644": {
      // 吸附·虚拟桌边界 — 虚拟桌面切换时贴齐工作区
      const x = clamp(drag.x, work.x, work.x + W - drag.w);
      const y = clamp(drag.y, work.y, work.y + H - drag.h);
      if (x !== drag.x || y !== drag.y) return { rect: { ...drag, x, y }, hit: "edge" };
      return null;
    }
    case "F00645": {
      // 吸附·意图预测 — 根据轨迹速度预测落点（高速横向 → 半屏）
      const v = ctx.velocity;
      if (!v) return null;
      const speed = Math.hypot(v.vx, v.vy);
      if (speed < 2.5) return null;
      const predX = drag.x + v.vx * 120;
      if (Math.abs(v.vx) > Math.abs(v.vy) && predX < work.x + W / 2)
        return { rect: { ...half }, hit: "zone" };
      if (Math.abs(v.vx) > Math.abs(v.vy)) return { rect: { x: work.x + W - half.w, y: work.y, w: half.w, h: H }, hit: "zone" };
      return null;
    }
    case "F00646": {
      // 吸附·提示线 — 显示对齐参考虚线（邻居/中轴/三分）
      const guides = { x: [work.x + W / 2, work.x + third, work.x + 2 * third], y: [work.y + H / 2] };
      const n = ctx.neighbor;
      if (n) {
        guides.x.push(n.x, n.x + n.w);
        guides.y.push(n.y, n.y + n.h);
      }
      return { rect: drag, hit: "none", guides };
    }
    case "F00647": {
      // 吸附·阈值调节 — 触发距离可调（ctx.threshold 即本档配置，命中判定复用屏幕边缘）
      const tt = ctx.threshold ?? 24;
      if (near(drag.x, work.x, tt)) return { rect: { ...half }, hit: "edge" };
      return null;
    }
    case "F00648":
      // 吸附·禁用热键 — 按住 Alt 暂时禁用：由调用方传入 altDown 决定跳过
      return ctx.altDown ? null : { rect: drag, hit: "none" };
    case "F00649":
      // 吸附·模式热切 — 网格/自由/磁吸三态热键循环：本档 = 网格态
      return { rect: { ...drag, x: round(drag.x, g), y: round(drag.y, g) }, hit: "grid" };
    case "F00650": {
      // 吸附·历史回放 — 上次吸附关系一键重放
      const last = ctx.lastSnap;
      if (!last) return null;
      return snapCandidate(last.itemId, { ...ctx, drag: { ...ctx.drag, ...last.offset } });
    }
    default:
      return null;
  }
}

/** SnapCtx 扩展字段（等距分布/历史回放）。 */
export interface SnapExtra {
  slots?: number;
  slotIndex?: number;
  altDown?: boolean;
  lastSnap?: { itemId: string; offset: Partial<VwmRect> } | null;
}

/** 带扩展字段的吸附上下文（SnapExtra 与 SnapCtx 合并）。 */
export type FullSnapCtx = SnapCtx & SnapExtra;
