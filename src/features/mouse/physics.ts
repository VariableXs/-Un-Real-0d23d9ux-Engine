/**
 * J 鼠标域 · 指针物理引擎（v5 · 深化批次五 · F604/F608）。
 *
 * v4 的磁吸视觉是指数渐近（lerpToward，帧率相关、无「到位」时刻）；锚标
 * 出现是 CSS 直切。本模块把两件视觉收进真实物理：
 * - 临界阻尼弹簧（解析解，帧率无关）：磁吸微移从「渐近不抵达」变成
 *   「到位即停」——快而稳，无振荡无过冲；dt 实测注入，任意帧率手感一致；
 * - 缓动族：easeOutCubic / easeInOutCubic / easeOutBack——锚标出现用
 *   easeOutBack（轻微过冲的「pop」手感），消失用 easeOutCubic（干脆收走）；
 * - 进度感曲线：长按进度环（F619/F496 消费面）的视觉进度用 easeOutQuad
 *   预弯——前 20% 时间走 30% 进度的「响应感」，真实判定不弯（视觉层专用）。
 *
 * 接线：windowRuntime 磁吸视觉已切 springToward（dt 从事件时间戳实测）。
 */

export interface Vec2 {
  x: number;
  y: number;
}

/**
 * 临界阻尼二阶弹簧（解析解，非迭代模拟——数值稳定、帧率无关）：
 * x(t) = target + (x0 - target)·(1 + ω t)·e^(−ω t)，ω 为固有频率。
 * 临界阻尼 = 最快无过冲收敛——「到位即停」的磁吸手感。
 * ω=40 口径：16ms 帧下 ~400ms 收敛到亚像素（磁吸微移的恰当节奏——快到
 * 不拖沓、慢到看得见「吸」的过程）。
 * @param dtMs 距上次的毫秒（事件时间戳实测）
 */
export function springToward(cur: Vec2, target: Vec2, dtMs: number, omega = 40): Vec2 {
  const dt = Math.max(0, Math.min(100, dtMs)) / 1000;
  const e = Math.exp(-omega * dt);
  const decay = (1 + omega * dt) * e;
  const step = (c: number, t: number): number => {
    const v = t + (c - t) * decay;
    // 残差 <0.05px 直接贴合（防小数拖尾——v4 lerpToward 同一纪律）。
    return Math.abs(t - v) < 0.05 ? t : Math.round(v * 100) / 100;
  };
  return { x: step(cur.x, target.x), y: step(cur.y, target.y) };
}

/** 弹簧到位判定（面板/测试共用：「吸附完成了吗」）。 */
export function springSettled(cur: Vec2, target: Vec2, tol = 0.05): boolean {
  return Math.hypot(cur.x - target.x, cur.y - target.y) <= tol;
}

/* ------------------------------- 缓动族 ------------------------------- */

export function easeOutCubic(t: number): number {
  const u = 1 - t;
  return 1 - u * u * u;
}

export function easeInOutCubic(t: number): number {
  return t < 0.5 ? 4 * t * t * t : 1 - Math.pow(-2 * t + 2, 3) / 2;
}

/** easeOutBack：轻微过冲的 pop 手感（锚标出现——「 arrive with intent」）。 */
export function easeOutBack(t: number, s = 1.70158): number {
  const u = t - 1;
  return 1 + (s + 1) * u * u * u + s * u * u;
}

export function easeOutQuad(t: number): number {
  return 1 - (1 - t) * (1 - t);
}

/**
 * 锚标生命曲线（F604 出现/消失动画的数学面）：
 * 出现 160ms easeOutBack（scale/透明度共用），消失 120ms easeOutCubic——
 * 判据「墨迹淡出 120ms」同款的干脆退场。返回 0..1（可当 scale 或 alpha）。
 */
export function anchorLifeCurve(elapsedMs: number, appearMs = 160, disappearMs = 120): number {
  if (elapsedMs < 0) return 0;
  if (elapsedMs <= appearMs) {
    const t = Math.max(0, Math.min(1, elapsedMs / appearMs));
    return Math.max(0, Math.min(1.12, easeOutBack(t)));
  }
  const gone = elapsedMs - appearMs;
  return Math.max(0, easeOutCubic(Math.max(0, 1 - gone / disappearMs)));
}

/**
 * 长按进度预弯（视觉层专用——真实判定时刻不弯，F619 判据原样）：
 * 进度环前段「快一点」的响应感来自 easeOutQuad 预弯；进度 0 与 1 处
 * 精确（起止不骗人），中段最多提前 ~15%。
 */
export function longPressVisualProgress(realT: number): number {
  const t = Math.max(0, Math.min(1, realT));
  return easeOutQuad(t);
}

/**
 * 慢速微调 HUD 呼吸（F602 HUD 的待机态）：1.6s 周期、±0.04 透明度呼吸——
 * 「精修徽标活着但不抢戏」。纯函数（面板/渲染层喂时间）。
 */
export function hudBreathAlpha(elapsedMs: number): number {
  const t = (elapsedMs % 1600) / 1600;
  return 0.96 + 0.04 * Math.sin(t * Math.PI * 2);
}

/**
 * 倾斜连发缓升（F606 连发节奏的第三段——首档即时、延迟启动后的连发从
 * 40ms 渐快到 24ms：第一感是「稳定节奏」，长按是「越滚越快」，两段都有
 * 理由）。返回第 i 次连发的间隔毫秒。
 */
export function tiltRepeatInterval(i: number, baseMs: number): number {
  const ramp = Math.min(1, i / 8);
  return Math.max(16, Math.round(baseMs - ramp * Math.min(16, baseMs * 0.4)));
}
