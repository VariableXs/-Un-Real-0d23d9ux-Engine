/**
 * J 鼠标域 · F608 指针磁吸对齐。
 *
 * 可关的落点磁吸：指针接近可点目标 12px 内松开移动即轻微吸附（视觉上指针
 * 微移对齐目标中心——「帮助对准」不是「抢走控制权」）；仅作用于按钮/复选框
 * 类小目标（≥24px 的大目标不吸——不需要也不该多手）；默认关（尊重原味手感）；
 * 磁吸半径三档可调（8/12/16px）。
 *
 * 红线判据：磁吸从不改变点击判定本身——点没点中仍看真实位置，磁吸只是视觉
 * 与微动辅助，判定零偏移（本模块只产出「视觉吸附偏移」，事件坐标永不改写）。
 */

export type MagnetRadius = 8 | 12 | 16;
export const MAGNET_RADII: MagnetRadius[] = [8, 12, 16];

export interface MagnetConfig {
  enabled: boolean;
  radiusPx: number;
}

/** 小目标判定（白名单判据：<24px 才吸）。 */
export function isSmallTarget(el: Element): boolean {
  const r = el.getBoundingClientRect();
  return r.width > 0 && r.height > 0 && r.width < 24 && r.height < 24;
}

/** 目标是否属于可磁吸控件类型（按钮/复选框/单选/开关类——语义白名单）。 */
export function isMagnetizable(el: Element): boolean {
  const tag = el.tagName.toLowerCase();
  if (tag === "button" || tag === "input" || tag === "a") return true;
  const role = el.getAttribute("role");
  return role === "button" || role === "checkbox" || role === "radio" || role === "switch";
}

/**
 * 磁吸计算：返回视觉吸附偏移（dx,dy）——把指针画到目标中心方向上的一小步。
 * 全部拒绝路径显式返回 {dx:0, dy:0, snapped:null}：
 * - 关档 / 非可磁吸类型 / 大目标（≥24px）/ 超出半径。
 * 判定零偏移铁律在类型上就成立：本函数返回值只允许喂给渲染层。
 */
export function magnetOffset(
  gx: number,
  gy: number,
  hit: Element | null,
  cfg: MagnetConfig,
): { dx: number; dy: number; snapped: string | null } {
  if (!cfg.enabled || !hit) return { dx: 0, dy: 0, snapped: null };
  if (!isMagnetizable(hit) || !isSmallTarget(hit)) return { dx: 0, dy: 0, snapped: null };
  const r = hit.getBoundingClientRect();
  const cx = r.left + r.width / 2;
  const cy = r.top + r.height / 2;
  const dx = cx - gx;
  const dy = cy - gy;
  const dist = Math.hypot(dx, dy);
  const radius = Math.max(4, Math.min(32, cfg.radiusPx || 12));
  if (dist > radius || dist === 0) return { dx: 0, dy: 0, snapped: null };
  // 轻微吸附：向目标中心走 40%（微移对齐，不是瞬移）。
  return { dx: Math.round(dx * 0.4 * 10) / 10, dy: Math.round(dy * 0.4 * 10) / 10, snapped: describeTarget(hit) };
}

function describeTarget(el: Element): string {
  const label = el.getAttribute("aria-label") ?? el.textContent?.trim() ?? "";
  return label ? `${el.tagName.toLowerCase()}「${label.slice(0, 16)}」` : el.tagName.toLowerCase();
}

/* ------------------------------- F608 视觉平滑（v4） ------------------------------- */

/**
 * 磁吸视觉偏移的渐近平滑：当前偏移向目标偏移按系数 k 渐近（每事件一步）。
 * 「微滑不瞬移」的手感来自这里——判定零偏移铁律不受影响（只作用于视觉层）。
 * 收敛到 <0.05px 时直接贴合目标（防无限小数拖尾）。
 */
export function lerpToward(cur: { x: number; y: number }, target: { x: number; y: number }, k = 0.35): { x: number; y: number } {
  const step = (c: number, t: number): number => {
    const d = t - c;
    if (Math.abs(d) < 0.05) return t;
    return Math.round((c + d * k) * 100) / 100;
  };
  return { x: step(cur.x, target.x), y: step(cur.y, target.y) };
}
