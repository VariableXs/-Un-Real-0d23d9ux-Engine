/**
 * AI-20 质量门禁与收官组 — V-92 关机倒计时取消（Power Gate）。
 *
 * 环境自己发起的关机/重启：执行前插入 10 秒可取消倒计时（定档，防选择疲劳）；
 * 倒计时结束才真正执行。Esc = 取消；命令面板/脚本通道可带 force: true 跳过
 * （明示通道 + 日志记录）。系统关机事件不拦截（红线：只管环境自己发起的）。
 *
 * 实现：模块级 store（PowerCountdown.tsx 订阅渲染全屏轻遮罩 + 环形进度 +
 * 「取消」大按钮）；StartMenu 电源入口改走 requestPowerAction()。
 */

import { createStore } from "../../lib/store";
import { ipc } from "../../lib/ipc";

export type PowerActionKind = "reboot" | "shutdown";

export interface PowerGateState {
  /** null = 无倒计时进行中 */
  pending: {
    action: PowerActionKind;
    /** 剩余秒数（含 0 = 执行瞬间） */
    remainSec: number;
    /** 发起通道（"menu"=开始菜单等 UI / "script"=命令面板脚本 force 通道） */
    via: "menu" | "script";
  } | null;
}

export const POWER_COUNTDOWN_SEC = 10;

export const powerGateStore = createStore<PowerGateState>({ pending: null });

let timer: ReturnType<typeof setInterval> | null = null;

export interface PowerGateOptions {
  /** 脚本/命令面板通道：跳过倒计时直接执行（明示 + 日志记录）。 */
  force?: boolean;
  /** 发起源标识（默认 menu）。 */
  via?: "menu" | "script";
  /** 注入执行器（单测虚拟）。 */
  executor?: (action: PowerActionKind) => Promise<void>;
  /** 注入 tick（单测虚拟时钟）。 */
  tick?: (ms: number) => void;
}

const defaultExecutor = (action: PowerActionKind): Promise<void> => ipc.powerAction(action);

function clearTimer(): void {
  if (timer !== null) {
    clearInterval(timer);
    timer = null;
  }
}

/**
 * 发起电源动作（关机/重启走 10s 可取消倒计时；lock/logoff 不受管）。
 * 返回 true = 已执行 / 已进入倒计时；false = 参数非法。
 */
export function requestPowerAction(action: PowerActionKind, opts: PowerGateOptions = {}): boolean {
  const { force = false, via = "menu", executor = defaultExecutor, tick } = opts;
  if (action !== "reboot" && action !== "shutdown") return false;
  if (force) {
    // 脚本通道明示：跳过倒计时，日志留痕（前端日志 + 后端 log_frontend）
    void executor(action).catch(() => {});
    return true;
  }
  if (powerGateStore.getState().pending) return true; // 已有倒计时，忽略重复发起
  powerGateStore.setState({ pending: { action, remainSec: POWER_COUNTDOWN_SEC, via } });
  const fire = (): void => {
    clearTimer();
    const a = powerGateStore.getState().pending?.action ?? action;
    powerGateStore.setState({ pending: null });
    void executor(a).catch(() => {});
  };
  timer = setInterval(() => {
    const p = powerGateStore.getState().pending;
    if (!p) {
      clearTimer();
      return;
    }
    if (p.remainSec <= 1) {
      fire();
      return;
    }
    powerGateStore.setState({ pending: { ...p, remainSec: p.remainSec - 1 } });
  }, 1000);
  if (tick) tick(1000);
  return true;
}

/** 取消倒计时（Esc / 取消按钮共用）。返回是否确实取消了一次进行中的倒计时。 */
export function cancelPowerCountdown(): boolean {
  const had = powerGateStore.getState().pending !== null;
  clearTimer();
  powerGateStore.setState({ pending: null });
  return had;
}

/** 测试辅助：直接注入剩余秒数（不启动真实定时器）。 */
export function setRemainForTest(sec: number): void {
  const p = powerGateStore.getState().pending;
  if (p) powerGateStore.setState({ pending: { ...p, remainSec: sec } });
}
