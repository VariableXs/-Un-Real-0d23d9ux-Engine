import { createStore, useStore } from "../../lib/store";
import { openVwmEngine } from "../windows/vwm";
import {
  EMPTY_ENGINE_SESSION,
  cancelEngineLaunch,
  markEngineWindowOpened,
  reduceEngineMsg,
  requestEngineLaunch,
  type EngineSession,
  type EngineStateMsg,
} from "./engineModel";

/**
 * 阶段 6 · 引擎会话 store（任务 50/51）：拉起协议的前端落点。
 * 模型全部在 engineModel.ts（纯函数可测）；本模块只做：
 * - 会话表状态
 * - 副作用接线（openVwmEngine → VWM 窗口；notify → toast）
 * - engine://state 事件通道挂载（Tauri 运行时；垫片降级面安全）
 */

export const engineStore = createStore<{ session: EngineSession }>({
  session: { ...EMPTY_ENGINE_SESSION },
});

export function useEngineSession(): EngineSession {
  return useStore(engineStore, (s) => s.session);
}

/** toast 注入口（避免环依赖 uiStore 由调用方注入；测试可传空实现）。 */
let notifyFn: (level: "info" | "warn" | "error", message: string) => void = () => {};

/** 注入 toast 实现（App 初始化时调用一次；幂等覆盖）。 */
export function setEngineNotify(fn: (level: "info" | "warn" | "error", message: string) => void): void {
  notifyFn = fn;
}

/** 应用消息（reducer 幂等；副作用在此执行）。 */
export function applyEngineMsg(msg: EngineStateMsg): void {
  const s = engineStore.getState().session;
  const { next, effects } = reduceEngineMsg(s, msg);
  engineStore.setState({ session: next });
  for (const fx of effects) {
    if (fx.type === "open-app") {
      // 就绪自动打开：每次会话一个独立流窗口（与 tp: 同语义，不复用旧实例）。
      openVwmEngine(fx.appKey, { focus: true });
      engineStore.setState({ session: markEngineWindowOpened(engineStore.getState().session) });
    } else if (fx.type === "notify") {
      notifyFn(fx.level, fx.message);
    }
    // drop-placeholder：占位卡随 engine 窗口存在——撤卡语义由 VwmAppContent
    // 依 lifecycle 渲染（closed/crashed 不再显示等待卡），无需额外动作。
  }
}

/** 用户点开 engine 通道软件：未就绪 → 登记请求；就绪 → 直接开窗。 */
export function requestEngineApp(appKey: string, nowMs = Date.now()): void {
  const s = engineStore.getState().session;
  if (s.lifecycle === "ready") {
    openVwmEngine(appKey, { focus: true });
    engineStore.setState({ session: markEngineWindowOpened(s) });
    return;
  }
  const { next } = requestEngineLaunch(s, appKey, nowMs);
  engineStore.setState({ session: next });
}

/** 用户取消等待中的拉起（占位卡上的取消按钮）。 */
export function cancelEngineApp(appKey: string): void {
  engineStore.setState({ session: cancelEngineLaunch(engineStore.getState().session, appKey) });
}

/** 重置会话（下次插入恢复语义：U 盘重新插入时由 usb 事件链调用）。 */
export function resetEngineSession(): void {
  engineStore.setState({ session: { ...EMPTY_ENGINE_SESSION } });
}

/**
 * 挂载 engine://state 事件通道（Tauri 运行时专用；App 初始化调用一次）。
 * 返回卸载函数。非 Tauri 环境（vitest/静态预览）安全跳过。
 */
export async function initEngineChannel(): Promise<() => void> {
  try {
    const tauriEvent = await import("@tauri-apps/api/event");
    const un = await tauriEvent.listen<EngineStateMsg>("engine://state", (ev) => {
      applyEngineMsg(ev.payload);
    });
    return un;
  } catch {
    return () => {};
  }
}
