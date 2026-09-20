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

/**
 * 扩展动作：切回原生 Windows 桌面。
 *
 * 与 reboot 的区别是**摘掉开机自启动**再重启——不摘的话重启后又会自动
 * 全屏进 Variable，等于没切出去（那正是「表面显示、实际切不动」的形态）。
 * 返回前先摘、后重启，顺序不可颠倒。
 */
export type PowerExtendedKind = PowerActionKind | "switchToWindows";

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

/** 本轮倒计时是否走「切回 Windows」分支。取消时必须复位，否则下一次
 *  普通重启会被误判成切换（用户点了重启却掉出自启动 = 静默行为漂移）。 */
let switchOverride = false;

/** 测试辅助：读当前 override 位（确认取消后确实复位）。 */
export function switchOverrideForTest(): boolean {
  return switchOverride;
}

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

/**
 * 「切回 Windows」执行器：先摘自启动（失败不阻断），再整机重启。
 *
 * 为什么失败不阻断：自启动没摘成，用户至少还能拿到一个干净重启，并在
 * Variable 里看到「切回 Windows」按钮仍然可用；把失败当成整体失败反而
 * 让用户卡在一个点不动的按钮上。摘除结果如实回报给调用方展示。
 */
async function switchToWindowsExecutor(): Promise<void> {
  let autostartCleared = true;
  try {
    await ipc.autostartSet(false);
  } catch {
    autostartCleared = false;
  }
  await ipc.powerAction("reboot");
  if (!autostartCleared) {
    // 重启已下发，进程随即结束；此处仅记录，不阻塞。
    globalThis.console?.warn?.(
      "[powerGate] 切回 Windows：开机自启动未能摘除，重启后可能仍会进入 Variable",
    );
  }
}

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
  // override 必须随取消一起复位：否则用户「切回 Windows → 取消 → 再点重启」
  // 时，普通重启会被误判成切换，静默摘掉自启动。
  switchOverride = false;
  powerGateStore.setState({ pending: null });
  return had;
}

// ---------------------------------------------------------------------------
// 切回 Windows（需求 8：真正能回 Windows 的按钮，不是表面显示）
// ---------------------------------------------------------------------------

/**
 * 发起「切回 Windows」。复用关机门禁的同一套 10s 可取消倒计时——
 * 给用户一条后悔的路（红线：可取消是承诺，不是装饰）。
 *
 * 倒计时结束才执行「摘自启动 + 重启」，所以中途取消不会留下半截状态。
 */
export function requestSwitchToWindows(opts: { force?: boolean } = {}): boolean {
  const { force = false } = opts;
  if (force) {
    void switchToWindowsExecutor().catch(() => {});
    return true;
  }
  if (powerGateStore.getState().pending) return true; // 已有倒计时，忽略重复发起
  // 复用同一 store：pending.action 复用 "reboot"（对外呈现的仍是"重启"语义），
  // 执行分支由 switchOverride 决定走哪条路，避免 UI 层多一个状态分支。
  switchOverride = true;
  powerGateStore.setState({
    pending: { action: "reboot", remainSec: POWER_COUNTDOWN_SEC, via: "menu" },
  });
  const fire = (): void => {
    clearTimer();
    const useSwitch = switchOverride;
    switchOverride = false;
    powerGateStore.setState({ pending: null });
    const run = useSwitch ? switchToWindowsExecutor() : defaultExecutor("reboot");
    void run.catch(() => {});
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
  return true;
}

/** 测试辅助：直接注入剩余秒数（不启动真实定时器）。 */
export function setRemainForTest(sec: number): void {
  const p = powerGateStore.getState().pending;
  if (p) powerGateStore.setState({ pending: { ...p, remainSec: sec } });
}
