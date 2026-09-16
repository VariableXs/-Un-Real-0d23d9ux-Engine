/**
 * 垫片输入事件总线 + 焦点模型原型（任务 26 · 前端侧）。
 *
 * 内核链路（待任务 19，AI-K）：内核输入服务 → 消息通道 → 本总线 → DOM 事件。
 * 本模块只负责前端侧语义：事件按焦点目标路由、焦点栈管理、剪贴板白名单预留点。
 * 与未来 Wine 通道共用同一输入总线结构（总案阶段 3 步骤 5 验收项）。
 */
import type { ShimEventChannel } from "./protocol";

/** 内核输入事件（与内核输入服务对齐的最小结构；19 落地后逐字段核对）。 */
export interface KernelKeyEvent {
  kind: "key";
  code: string;
  key: string;
  down: boolean;
  ctrl: boolean;
  alt: boolean;
  shift: boolean;
}
export interface KernelPointerEvent {
  kind: "pointer";
  action: "move" | "down" | "up" | "wheel";
  x: number;
  y: number;
  button: number;
  deltaY?: number;
}
export interface KernelTextEvent {
  kind: "text";
  text: string;
}
export type KernelInputEvent = KernelKeyEvent | KernelPointerEvent | KernelTextEvent;

/** 焦点目标：一个可收键的前端区域（VWM 窗口/桌面壳）。 */
export interface FocusTarget {
  id: string;
  /** 是否可收键（占位卡/幕帘等不可收键）。 */
  focusable: boolean;
  onKeyDown?: (e: KernelKeyEvent) => void;
  onPointer?: (e: KernelPointerEvent) => void;
  onText?: (e: KernelTextEvent) => void;
}

const listeners = new Map<string, FocusTarget>();
/** 焦点栈：栈顶即当前收键目标。 */
const focusStack: string[] = [];
let transportInstalled = false;
/** 输入事件来源频道（协议 events 表新增 shim://input；19 落地后由垫片传输层调用 dispatchInputEvent）。 */
export const INPUT_EVENT_CHANNEL: ShimEventChannel = "shim://input";

export function installInputBus(): void {
  if (transportInstalled) throw new Error("输入总线已安装");
  transportInstalled = true;
}

export function resetInputBusForTest(): void {
  listeners.clear();
  focusStack.length = 0;
  transportInstalled = false;
}

export function registerTarget(t: FocusTarget): () => void {
  listeners.set(t.id, t);
  return () => {
    listeners.delete(t.id);
    const i = focusStack.indexOf(t.id);
    if (i >= 0) focusStack.splice(i, 1);
  };
}

/** 置顶焦点（重复置顶幂等）。不可收键目标拒绝。 */
export function setFocus(id: string): boolean {
  const t = listeners.get(id);
  if (!t || !t.focusable) return false;
  const i = focusStack.indexOf(id);
  if (i >= 0) focusStack.splice(i, 1);
  focusStack.push(id);
  return true;
}

/** 当前焦点目标 id；无焦点返回 null（桌面壳兜底自取，见 focusFallback）。 */
export function currentFocus(): string | null {
  return focusStack.length ? (focusStack[focusStack.length - 1] ?? null) : null;
}

/** 兜底目标：无焦点/焦点失效时收键者（桌面壳 id）。 */
let fallbackId: string | null = null;
export function setFocusFallback(id: string): void {
  fallbackId = id;
}

function routeTarget(): FocusTarget | null {
  for (let i = focusStack.length - 1; i >= 0; i--) {
    const t = listeners.get(focusStack[i] ?? "");
    if (t?.focusable) return t;
  }
  return fallbackId ? (listeners.get(fallbackId) ?? null) : null;
}

/** 垫片传输层入口：分发一条内核输入事件（未安装总线时静默丢弃，不产生副作用）。 */
export function dispatchInputEvent(e: KernelInputEvent): boolean {
  if (!transportInstalled) return false;
  const t = routeTarget();
  if (!t) return false;
  if (e.kind === "key") t.onKeyDown?.(e);
  else if (e.kind === "text") t.onText?.(e);
  else t.onPointer?.(e);
  return true;
}

/**
 * 剪贴板白名单预留点（总案步骤 5 完善性项）：
 * 目标申请读剪贴板须经本回调裁决（默认拒绝），任务 34 接白名单实现。
 */
let clipboardArbiter: ((targetId: string) => boolean) | null = null;
export function setClipboardArbiter(fn: (targetId: string) => boolean): void {
  clipboardArbiter = fn;
}
export function requestClipboard(targetId: string): boolean {
  return clipboardArbiter ? clipboardArbiter(targetId) : false;
}
