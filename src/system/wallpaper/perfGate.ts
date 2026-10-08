/**
 * N-07 壁纸性能联动闸门（纯逻辑，无任何运行时依赖）。
 *
 * 规格（功能全景 L817）：
 * - 打字时降级渲染（现有行为）扩展到全部引擎；
 * - 全屏应用时 shader 引擎自动降帧至 10fps；
 * - 电池模式暂停视频引擎。
 *
 * 事件/数据源由消费方（Workshop / 天气层）注入，本文件只做决策：
 * - typing      最近 1s 有按键（keydown 时间戳 + isTypingRecent）
 * - fullscreen  sys://fullscreen 事件（DesktopShell 同款监听，消费方接）
 * - battery     navigator.getBattery（不可用视为非电池 = false）
 */

export interface PerfGateInputs {
  typing: boolean;
  fullscreen: boolean;
  battery: boolean;
}

export type PerfGateReason = "typing" | "fullscreen" | "battery";

export interface PerfGateDecision {
  /** 任一降级条件命中（总体开关）。 */
  degraded: boolean;
  /** shader 引擎目标帧率：全屏 10 / 打字 12 / 正常 60。 */
  shaderFps: number;
  /** 视频引擎暂停（电池模式 / 打字）。 */
  videoPaused: boolean;
  /** 生成式（星空）引擎停帧（打字 / 全屏）。 */
  generativePaused: boolean;
  /** N-11 场景粒子静止（打字 / 全屏降级联动）。 */
  particlesStatic: boolean;
  /** 命中原因（UI 诚实提示用）。 */
  reasons: PerfGateReason[];
}

/** 打字判定窗口：最近 1s 有按键即视为输入态。 */
export const TYPING_WINDOW_MS = 1000;

/** 纯函数：lastKeyAt 距 now 是否仍在打字窗口内（0/负值 = 从未按过键）。 */
export function isTypingRecent(lastKeyAt: number, now: number): boolean {
  return lastKeyAt > 0 && now - lastKeyAt < TYPING_WINDOW_MS;
}

/** 纯函数：按三输入产出全部引擎的降级决策。 */
export function isDegradeActive(inputs: PerfGateInputs): PerfGateDecision {
  const reasons: PerfGateReason[] = [];
  if (inputs.typing) reasons.push("typing");
  if (inputs.fullscreen) reasons.push("fullscreen");
  if (inputs.battery) reasons.push("battery");

  const typing = inputs.typing;
  const fullscreen = inputs.fullscreen;
  const battery = inputs.battery;

  return {
    degraded: reasons.length > 0,
    shaderFps: fullscreen ? 10 : typing ? 12 : 60,
    videoPaused: battery || typing,
    generativePaused: typing || fullscreen,
    particlesStatic: typing || fullscreen,
    reasons,
  };
}