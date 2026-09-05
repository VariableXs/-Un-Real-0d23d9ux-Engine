import { createStore, useStore } from "../lib/store";

/**
 * 通知中心（M5）：本地操作历史的会话内聚合。
 * - 数据只存内存（本会话），零网络、零持久化上报；
 * - 来源：隐私占用提示（摄像头/麦克风）、硬件状态变化、系统级事件（托盘等）。
 */
export interface NotifyItem {
  id: number;
  time: number;
  kind: "privacy" | "hardware" | "system";
  title: string;
  body: string;
  read: boolean;
}

interface NotifyState {
  items: NotifyItem[];
  nextId: number;
  /** 批次C（规格 6.5.3）：勿扰模式 —— 横幅静默，通知照常入历史，Ctrl+Shift+M 切换。 */
  dnd: boolean;
}

const MAX_ITEMS = 50;

const notifyStore = createStore<NotifyState>({ items: [], nextId: 1, dnd: false });

export function useNotifications(): NotifyItem[] {
  return useStore(notifyStore, (s) => s.items);
}

/** 勿扰状态（PrivacyBanner 据此静默横幅；通知历史不受影响）。 */
export function useDnd(): boolean {
  return useStore(notifyStore, (s) => s.dnd);
}

export function toggleDnd(): boolean {
  const dnd = !notifyStore.getState().dnd;
  notifyStore.setState({ dnd });
  return dnd;
}

export function useUnreadCount(): number {
  return useStore(notifyStore, (s) => s.items.reduce((n, it) => n + (it.read ? 0 : 1), 0));
}

export function pushNotify(kind: NotifyItem["kind"], title: string, body = ""): void {
  const id = notifyStore.getState().nextId;
  notifyStore.setState((s) => ({
    items: [...s.items, { id, time: Date.now(), kind, title, body, read: false }].slice(-MAX_ITEMS),
    nextId: id + 1,
  }));
}

export function markAllRead(): void {
  notifyStore.setState((s) => ({
    items: s.items.map((it) => (it.read ? it : { ...it, read: true })),
  }));
}

export function clearNotifications(): void {
  notifyStore.setState({ items: [] });
}
