/**
 * J 鼠标域 · F620 指针衬底与投影 · 纵深引擎（批次七）。
 *
 * v4 有衬底基本形态。本引擎把「衬底」从静态样式升级为物理光照：
 *
 * 1. 速度感投影——指针静止时投影居中且柔（光源在正上方）；移动时
 *    投影按速度反向偏移（举起抬离桌面的物理直觉：越快「抬」得越高，
 *    影子偏得越远、越淡越大）。偏移量 = 速度 × 高度系数，模糊半径
 *    与透明度同步随「高度」变化——单参数驱动的三联变化（物理一致性：
 *    三个量是同一「高度」的三个投影，不是三个独立旋钮）。
 *
 * 2. 衬底色彩物理——衬底不是纯黑半透明，而是「地面反光」：按壁纸
 *    主题令牌取地面基色，投影色 = 基色 × 吸收系数（0.82——白墙上的
 *    影子带墙面色，黑板上的影子近黑）。亮暗主题自动换算，色弱可辨
 *    （明度差 ≥18%——对比度纪律的影子版）。
 *
 * 3. 点击呼吸——按下时「高度」骤降（影子收缩变实——指针「按」到
 *    地面），松开回弹带 90ms 微弹（欠阻尼一拍，与 v5 physics 弹簧
 *    族同一格律但参数独立——这里是视觉，不是指针位置）。
 *
 * 4. 4K 纹理纪律——衬底纹理坐标按 2x 超采样制作（512px 图集，4K 屏
 *    实际渲染 256px 不糊）；弱动效偏好下全部归零（无障碍红线：动效
 *    是增强不是必需）。
 *
 * 判据锚点：
 * - 三联物理一致性（单参数三投影）→ shadowFromLift()
 * - 地面反光换算与明度差 → groundShadowColor()
 * - 按下骤降与 90ms 回弹 → pressLift() 
 * - 弱动效归零 → respectReducedMotion()
 */

/* ------------------------------- 高度→三联投影 ------------------------------- */

/** 静息高度（抽象单位——投影几何的基准面）。 */
export const REST_LIFT = 1;
/** 满速高度（速度 1.2px/ms 处封顶）。 */
export const MAX_LIFT = 3.2;

/**
 * 高度 → 投影三联（offsetPx / blurPx / opacity）。
 * 物理口径：offset = (lift-1) × 6px 反速度方向；blur = 4 + lift × 5；
 * opacity = 0.38 / lift（越高越淡）。三联由单一 lift 派生——
 * 调用方不可能造出「偏移大但模糊小」的物理矛盾态（类型级保证）。
 */
export function shadowFromLift(lift: number): { offsetPx: number; blurPx: number; opacity: number } {
  const l = Math.max(0.4, Math.min(MAX_LIFT, lift));
  return {
    offsetPx: Math.round((l - REST_LIFT) * 6 * 100) / 100,
    blurPx: Math.round((4 + l * 5) * 100) / 100,
    opacity: Math.round((0.38 / l) * 1000) / 1000,
  };
}

/** 速度（px/ms）→ 高度（线性到封顶；静止恒为 REST_LIFT）。 */
export function liftFromSpeed(speedPxMs: number): number {
  if (speedPxMs <= 0) return REST_LIFT;
  return REST_LIFT + Math.min(1, speedPxMs / 1.2) * (MAX_LIFT - REST_LIFT);
}

/* ------------------------------- 地面反光 ------------------------------- */

/** 吸收系数——投影色 = 地面色 × 吸收（影子不是纯黑）。 */
export const SHADOW_ABSORB = 0.82;

export interface Rgb {
  r: number;
  g: number;
  b: number;
}

/**
 * 地面反光换算：地面基色（主题令牌）→ 投影色。
 * 吸收后与地面色算明度差，<18% 时加深到 18%（色弱可辨底线）。
 */
export function groundShadowColor(ground: Rgb): { shadow: Rgb; lumDeltaPct: number } {
  const s: Rgb = {
    r: Math.round(ground.r * SHADOW_ABSORB),
    g: Math.round(ground.g * SHADOW_ABSORB),
    b: Math.round(ground.b * SHADOW_ABSORB),
  };
  const lum = (c: Rgb) => 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
  const gl = lum(ground);
  const sl = lum(s);
  const delta = gl === 0 ? 0 : Math.abs(gl - sl) / gl;
  if (delta < 0.18) {
    // 精确压暗到 18% 明度差：s 整体缩放使 lum(s') = gl × 0.82（保持色相只动明度）。
    const scale = sl === 0 ? 0 : (gl * SHADOW_ABSORB) / sl;
    s.r = Math.min(255, Math.round(s.r * scale));
    s.g = Math.min(255, Math.round(s.g * scale));
    s.b = Math.min(255, Math.round(s.b * scale));
    return { shadow: s, lumDeltaPct: 0.18 };
  }
  return { shadow: s, lumDeltaPct: Math.round(delta * 1000) / 1000 };
}

/* ------------------------------- 点击呼吸 ------------------------------- */

/** 按下「按到地面」的高度。 */
export const PRESS_LIFT = 0.35;
/** 回弹时长（ms）——欠阻尼一拍。 */
export const PRESS_REBOUND_MS = 90;

/**
 * 按压高度曲线：tMs=0 按下 → 骤降到 PRESS_LIFT；tMs∈(0,REBOUND]
 * 欠阻尼回弹到 REST_LIFT（一拍过冲 ≤1.18x——视觉弹簧不许疯）。
 */
export function pressLift(tMs: number, pressed: boolean): number {
  if (pressed) return PRESS_LIFT;
  if (tMs <= 0 || tMs >= PRESS_REBOUND_MS) return REST_LIFT;
  const t = tMs / PRESS_REBOUND_MS;
  // 欠阻尼：e^-5t · cos(2π·1.5t) 摆动叠加到回归线。
  const wobble = Math.exp(-5 * t) * Math.cos(2 * Math.PI * 1.5 * t) * 0.18;
  return REST_LIFT + (PRESS_LIFT - REST_LIFT) * (1 - t) + wobble * (PRESS_LIFT - REST_LIFT);
}

/* ------------------------------- 弱动效归零 ------------------------------- */

export interface ShadowFrame {
  offsetPx: number;
  blurPx: number;
  opacity: number;
  /** 是否处于呼吸动画态（弱动效下恒 false——动画层不启动）。 */
  animated: boolean;
}

/**
 * 弱动效合规出口：prefers-reduced-motion 时投影钉在静息值、动画层关闭。
 * 静息投影仍然存在（影子是常态渲染，不是动效）——归零的是「变化」，不是「存在」。
 */
export function respectReducedMotion(reduced: boolean, speedPxMs: number, pressed: boolean, tMs: number): ShadowFrame {
  if (reduced) {
    const s = shadowFromLift(REST_LIFT);
    return { ...s, animated: false };
  }
  const lift = pressed ? PRESS_LIFT : liftFromSpeed(speedPxMs);
  const s = shadowFromLift(lift);
  return { ...s, animated: !pressed && tMs < PRESS_REBOUND_MS };
}

/* ------------------------------- 4K 图集坐标 ------------------------------- */

/** 衬底图集：2x 超采样帧尺寸（512 实际渲染 256——4K 不糊纪律）。 */
export const ATLAS_FRAME_PX = 512;
export const ATLAS_COLS = 4;
/** 图集帧序：0 静息 / 1 按下 / 2 满速 / 3 禁用态。 */
export type AtlasFrame = 0 | 1 | 2 | 3;

/** 帧序号 → 图集 UV（归一化坐标，直接喂渲染器）。 */
export function atlasUV(frame: AtlasFrame): { u0: number; v0: number; u1: number; v1: number } {
  const col = frame % ATLAS_COLS;
  const u0 = col / ATLAS_COLS;
  return { u0, v0: 0, u1: u0 + 1 / ATLAS_COLS, v1: 1 };
}
