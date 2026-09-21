import { createStore, useStore } from "../../lib/store";
import { errMessage, ipc } from "../../lib/ipc";
import { focusVwmWin, openVwmEngine, vwmStore } from "../windows/vwm";
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
 * 阶段 6 · 引擎会话 store（任务 50/51 + 三体 AI-2 S3.7 拉起协议闭环）：
 * 拉起协议的前端落点。模型全部在 engineModel.ts（纯函数可测）；本模块只做：
 * - 会话表状态
 * - 副作用接线（开占位/流窗 → VWM 窗口；notify → toast；wake → 后端编排）
 * - engine://state 事件通道挂载（Tauri 运行时；垫片降级面安全）
 *
 * S3.7 拉起协议闭环（总案 6.7）：
 *   点 engine 通道软件 → 未就绪：占位卡立即出现（冷启动阶段化叙事 + 取消路径）
 *   + 唤醒后端（幂等）→ 就绪自动开（占位窗聚焦自然过渡到流面，不双开）；
 *   取消/失败撤卡路径由 reducer effects 驱动。
 *
 * 可注入缝：setEngineNotify / setEngineWake / setEngineWindowOps ——
 * 测试注入假实现（本文件对 vwm 为静态依赖：模块图已被既有集成测试证明 vitest 可载）。
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

// ---------- S3.7 可注入缝 ----------

/** 唤醒后端编排（默认 engine_wake；幂等，冷启动重活在后端线程）。 */
export type EngineWakeFn = () => Promise<unknown>;

/** 引擎窗口操作（默认走 vwm 真实窗口；测试注入假实现）。 */
export interface EngineWindowOps {
  /** 打开该软件的引擎占位/流窗（engine:appKey 实例）。 */
  open: (appKey: string) => void;
  /** 该软件的窗口已存在则聚焦并返回 true（撤卡 = 占位窗自然过渡到流面）。 */
  focusExisting: (appKey: string) => boolean;
}

let wakeFn: EngineWakeFn = () => ipc.engineWake();

let winOps: EngineWindowOps = {
  open: (appKey) => {
    openVwmEngine(appKey, { focus: true });
  },
  focusExisting: (appKey) => {
    const w = vwmStore.getState().wins.find((x) => x.app === `engine:${appKey}`);
    if (!w) return false;
    focusVwmWin(w.id);
    return true;
  },
};

/** 注入唤醒实现（测试缝；默认 ipc.engineWake）。 */
export function setEngineWake(fn: EngineWakeFn): void {
  wakeFn = fn;
}

/** 注入窗口操作（测试缝；默认 vwm 实现）。 */
export function setEngineWindowOps(ops: EngineWindowOps): void {
  winOps = ops;
}

/** 应用消息（reducer 幂等；副作用在此执行）。 */
export function applyEngineMsg(msg: EngineStateMsg): void {
  const s = engineStore.getState().session;
  const { next, effects } = reduceEngineMsg(s, msg);
  engineStore.setState({ session: next });
  for (const fx of effects) {
    if (fx.type === "open-app") {
      // 就绪自动打开（总案 6.7 撤卡语义）：占位窗已存在 → 聚焦（窗内容由
      // EngineStreamPane 依 lifecycle 自然过渡到流面，不双开）；无窗（如
      // 外部事件驱动的就绪）→ 新开流窗并计数（泄漏审计）。
      if (!winOps.focusExisting(fx.appKey)) {
        winOps.open(fx.appKey);
        engineStore.setState({ session: markEngineWindowOpened(engineStore.getState().session) });
      }
    } else if (fx.type === "notify") {
      notifyFn(fx.level, fx.message);
    }
    // drop-placeholder：占位卡随 engine 窗口存在——撤卡语义由 VwmAppContent
    // 依 lifecycle 渲染（closed/crashed 不再显示等待卡），无需额外动作。
  }
}

/** 用户点开 engine 通道软件：未就绪 → 占位卡 + 登记请求 + 唤醒；就绪 → 直接开窗。 */
export function requestEngineApp(appKey: string, nowMs = Date.now()): void {
  const s = engineStore.getState().session;
  if (s.lifecycle === "ready") {
    // 已就绪：直接开流窗（每次会话独立实例，与 tp: 同语义）。
    winOps.open(appKey);
    engineStore.setState({ session: markEngineWindowOpened(engineStore.getState().session) });
    return;
  }
  // 防双击双窗：同一软件已有未取消的等待请求 → 不重复登记/开窗。
  if (s.pending.some((r) => r.appKey === appKey && !r.cancelled)) return;
  const { next, needWake } = requestEngineLaunch(s, appKey, nowMs);
  engineStore.setState({ session: next });
  // 占位卡立即出现（总案 6.7：未就绪占位卡 → 就绪自动开）；计入泄漏审计。
  winOps.open(appKey);
  engineStore.setState({ session: markEngineWindowOpened(engineStore.getState().session) });
  if (needWake) {
    // 唤醒后端（幂等）。失败如实提示三要素（如差分链未就绪 / Hyper-V 缺席）；
    // 后端状态保持不变，占位窗会依引擎事件或用户取消收束。
    void wakeFn().catch((e: unknown) => {
      const m = errMessage(e);
      notifyFn("error", m.message);
    });
  }
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
