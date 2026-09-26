/**
 * J 鼠标域 · F604 中键自动滚动 + F609 拖拽边缘自动滚。
 *
 * F604——中键按下出现滚动锚点（圆形锚标），此后指针离锚的方位与距离决定
 * 滚动方向与速度（8px 起步线性加速、60px 封顶），再按中键或任一键点击即退出；
 * 锚标走 F335 优先平面、样式走主题令牌；与 F204 惯性滚动互斥语义：
 * 自动滚接管时惯性挂起、退出恢复。
 *
 * F609——拖着文件到达容器边缘时自动滚动（边缘 24px 触发带、速度按深入距离
 * 线性三档）；树/列表/桌面/跨窗四容器全部生效（data-autoscroll 声明式接入）；
 * 与 F262 拖拽语义、F553 拖影计数叠加无冲突（自动滚是容器行为不是拖拽行为）；
 * 悬停静止区（<触发带）绝不误滚。
 *
 * 判据锚点：
 * - 方向-速度映射 16 方位采样对拍 → autoscrollVelocity()
 * - 退出三路（中键/左右键点击/Esc）→ AutoScroll.exitFor()
 * - 24px 触发带与三档速度 → edgeScrollSpeed()
 * - 边缘静止区判据 → 深入 0 返回 0
 */

/* ------------------------------- F604 中键自动滚动 ------------------------------- */

export interface AutoscrollConfig {
  enabled: boolean;
  deadZonePx: number;
  maxPx: number;
}

/** 默认档对拍 Windows（8px 起步 / 60px 封顶）。 */
export const AUTOSCROLL_PRESET = { deadZonePx: 8, maxPx: 60 } as const;

/**
 * 方向-速度映射：锚点到指针的偏移 → 每帧滚动像素（16 方位采样对拍口径）。
 * - 死区（8px 内）不动——锚点就是「零速点」；
 * - 线性加速到 60px 封顶；
 * - 方向 = 偏移角按 16 方位量化（上下左右 + 斜向）。
 */
export function autoscrollVelocity(
  offsetX: number,
  offsetY: number,
  cfg: AutoscrollConfig,
): { vx: number; vy: number; dirIndex: number } {
  const dead = Math.max(1, cfg.deadZonePx);
  const max = Math.max(dead + 1, cfg.maxPx);
  const dist = Math.hypot(offsetX, offsetY);
  const dirIndex = quantizeDirection(offsetX, offsetY);
  if (dist <= dead) return { vx: 0, vy: 0, dirIndex };
  const speed = Math.min(max, dist - dead);
  const k = speed / dist;
  return { vx: offsetX * k, vy: offsetY * k, dirIndex };
}

/** 16 方位量化（0=正右，顺时针，每 22.5° 一档）——采样对拍表用。 */
export function quantizeDirection(ox: number, oy: number): number {
  if (ox === 0 && oy === 0) return -1;
  let deg = (Math.atan2(oy, ox) * 180) / Math.PI; // -180..180
  if (deg < 0) deg += 360;
  return Math.round(deg / 22.5) % 16;
}

/**
 * 退出判定三路（中键/左右键点击/Esc）——单一函数供 runtime 与测试同源。
 * 返回 "exit" | "ignore"。
 */
export function autoscrollExitFor(button: number | "esc"): "exit" | "ignore" {
  if (button === "esc") return "exit";
  if (button === 0 || button === 1 || button === 2) return "exit"; // 左/中/右
  return "ignore"; // 侧键不退出
}

/* ------------------------------- F609 拖拽边缘自动滚 ------------------------------- */

export interface DragScrollConfig {
  enabled: boolean;
  bandPx: number;
}

/** 边缘触发带固定 24px（判据原文）。 */
export const DRAG_BAND_PX = 24;

/**
 * 深入距离 → 每帧滚动像素（线性三档：0-8px 慢档 4px/帧，8-16px 中档 10，
 * 16-24px 快档 18；深入 0 = 静止区绝不误滚）。
 * @param depth 指针深入触发带的像素（0..band）
 */
export function edgeScrollSpeed(depth: number, bandPx = DRAG_BAND_PX): number {
  const band = Math.max(4, bandPx);
  if (depth <= 0) return 0;
  const d = Math.min(depth, band);
  const t = d / band;
  if (t <= 1 / 3) return 4;
  if (t <= 2 / 3) return 10;
  return 18;
}

/**
 * 边缘深入计算：指针在容器可视矩形内时，返回四边各自的深入量（0=不在带内）。
 * 静止区（矩形中央）四边全 0——「滚太快想停就把指针往回收」的油门。
 */
export function edgeDepth(
  gx: number,
  gy: number,
  rect: { left: number; top: number; right: number; bottom: number },
  bandPx = DRAG_BAND_PX,
): { left: number; right: number; top: number; bottom: number } {
  const inside = gx >= rect.left && gx <= rect.right && gy >= rect.top && gy <= rect.bottom;
  if (!inside) return { left: 0, right: 0, top: 0, bottom: 0 };
  return {
    left: Math.max(0, bandPx - (gx - rect.left)),
    right: Math.max(0, bandPx - (rect.right - gx)),
    top: Math.max(0, bandPx - (gy - rect.top)),
    bottom: Math.max(0, bandPx - (rect.bottom - gy)),
  };
}

/**
 * 容器声明式接入标记（四容器统一：树/列表/桌面/跨窗滚动容器都加此属性）。
 * runtime 只对带标记的容器生效——不越权滚动未声明的容器。
 */
export const AUTOSCROLL_ATTR = "data-autoscroll";
