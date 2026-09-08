import { createStore, useStore } from "../lib/store";

export type SaveStatus = "saved" | "saving" | "dirty" | "error";

export type AppMode = "write" | "mindmap" | "project" | "fate";

/** 批次C：快捷面板聚焦分区（规格 6.1/6.2/6.3 快捷键与托盘图标入口）。 */
export type QuickSection = "wifi" | "bluetooth" | "audio";

export interface UiState {
  /** "desktop" = 桌面环境 shell；其余为四款独立软件内部视图（M4 拆窗前的过渡形态）。 */
  mode: AppMode | "desktop";
  /** Pending .project archive to open when the project space mounts (ch.12.2). */
  pvPendingOpen: string | null;
  /** Pending local file (e.g. .fatetree) to open when the fate space mounts. */
  fatePendingOpen: string | null;
  /** Pending local .md/.txt to open as a document when the write space mounts. */
  writePendingOpen: string | null;
  sidebarOpen: boolean;
  searchOpen: boolean;
  /** 开始菜单展开状态（桌面环境 L1，M3）。 */
  startOpen: boolean;
  /** 批次C：快捷面板展开状态 + 聚焦分区（null = 无聚焦，通知中心入口）。 */
  quickOpen: boolean;
  quickSection: QuickSection | null;
  /** 第三方软件管理器（M7 launcher）。 */
  launcherOpen: boolean;
  /** 批次B-9（M3）：AI Hub 面板。 */
  aiHubOpen: boolean;
  /** 批次C：软件管理器当前页（第三方 / 已安装软件，规格 5.6）。 */
  launcherTab: "third" | "installed";
  settingsOpen: boolean;
  settingsTab: string;
  focusMode: boolean;
  currentDocId: string | null;
  currentMapId: string | null;
  saveStatuses: Record<string, SaveStatus>;
  docListVersion: number; // bumped to refresh lists after mutations
  mapListVersion: number;
  recoveryPrompt: { id: string; savedAt: number; title: string; preview: string }[] | null;
  // ---- global canvas interaction state machine (blank-click dismissal) ----
  /** Non-null while any right-click context menu / pinned popover is open. */
  activeContextMenu: string | null;
  /** Primary selected frame id on the mind-map canvas (null = nothing). */
  selectedFrameId: string | null;
  /** Node currently in text-editing session (null = none). */
  editingId: string | null;
  /** M-34 Esc 层级语义：浮层栈（栈顶 = 最后打开的浮层，Esc 逐层消费）。 */
  overlayStack: string[];
}

export const uiStore = createStore<UiState>({
  mode: "desktop",
  pvPendingOpen: null,
  fatePendingOpen: null,
  writePendingOpen: null,
  sidebarOpen: false,
  searchOpen: false,
  startOpen: false,
  quickOpen: false,
  quickSection: null,
  launcherOpen: false,
  launcherTab: "third",
  aiHubOpen: false,
  settingsOpen: false,
  settingsTab: "appearance",
  focusMode: false,
  currentDocId: null,
  currentMapId: null,
  saveStatuses: {},
  docListVersion: 0,
  mapListVersion: 0,
  recoveryPrompt: null,
  activeContextMenu: null,
  selectedFrameId: null,
  editingId: null,
  overlayStack: [],
});

/**
 * Forced-destroy state machine (module-0): ANY left click on a blank canvas
 * background instantly resets the whole global activation state — context
 * menu, selection outline, editing session. Overlays listen for the
 * `variable:mm-dismiss-all` broadcast and run their own fade-out protocol
 * before unmounting; nothing outside menus intercepts the event path.
 */
export function resetGlobalCanvasInteraction(): void {
  const s = uiStore.getState();
  if (s.activeContextMenu !== null || s.selectedFrameId !== null || s.editingId !== null) {
    uiStore.setState({ activeContextMenu: null, selectedFrameId: null, editingId: null });
  } else {
    // Still broadcast so local canvas state (multi-select, free-transform,
    // quick-find…) is cleared even when the mirrors were already empty.
  }
  window.dispatchEvent(new CustomEvent("variable:mm-dismiss-all"));
}

export function useUi<S>(sel: (s: UiState) => S): S {
  return useStore(uiStore, sel);
}

export function setSaveStatus(docId: string, status: SaveStatus): void {
  uiStore.setState((s) => ({ saveStatuses: { ...s.saveStatuses, [docId]: status } }));
}

export function bumpDocList(): void {
  uiStore.setState((s) => ({ docListVersion: s.docListVersion + 1 }));
}

export function bumpMapList(): void {
  uiStore.setState((s) => ({ mapListVersion: s.mapListVersion + 1 }));
}

/** 打开/切换快捷面板（批次C：Ctrl+Alt+B/O、Win+N 与托盘图标共用）。 */
export function openQuickPanel(section: QuickSection | null): void {
  const s = uiStore.getState();
  if (s.quickOpen && s.quickSection === section) {
    uiStore.setState({ quickOpen: false, quickSection: null });
  } else {
    uiStore.setState({ quickOpen: true, quickSection: section, startOpen: false });
  }
}

export function closeQuickPanel(): void {
  uiStore.setState({ quickOpen: false, quickSection: null });
}

// ---------- toasts ----------

export interface Toast {
  id: number;
  kind: "info" | "success" | "error";
  message: string;
  detail?: string;
}

interface ToastState {
  toasts: Toast[];
  nextId: number;
}

const toastStore = createStore<ToastState>({ toasts: [], nextId: 1 });

export function useToasts(): Toast[] {
  return useStore(toastStore, (s) => s.toasts);
}

export function pushToast(kind: Toast["kind"], message: string, detail?: string): void {
  const id = toastStore.getState().nextId;
  toastStore.setState((s) => ({
    toasts: [...s.toasts.slice(-5), { id, kind, message, detail }],
    nextId: id + 1,
  }));
  if (kind !== "error") {
    setTimeout(() => dismissToast(id), 4000);
  }
}

export function dismissToast(id: number): void {
  toastStore.setState((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) }));
}

// ---------- M-34 Esc 层级语义（浮层栈） ----------

/**
 * 浮层入栈。幂等：同 id 重复 push 只刷新位置（移到栈顶）。
 * 调用方：模态/右键菜单/各面板/搜索框等浮层在打开时调用。
 */
export function pushOverlay(id: string): void {
  uiStore.setState((s) => ({ overlayStack: [...s.overlayStack.filter((o) => o !== id), id] }));
}

/** 浮层出栈（浮层关闭时调用，与其打开路径配对）。 */
export function popOverlay(id: string): void {
  uiStore.setState((s) => ({ overlayStack: s.overlayStack.filter((o) => o !== id) }));
}

/**
 * Esc 消费一层：返回被关闭的浮层 id（栈顶），栈空返回 null。
 * 红线：kbdhook 双击 Esc 切环境是既有全局契约——环境内有浮层时
 * 双击 Esc 只关浮层（消费一层 ×2），该判定由 kbdhook 层独立完成，
 * 此处只负责单次 Esc 的栈顶消费，不参与双击判定。
 */
export function consumeOverlayOnEsc(): string | null {
  const stack = uiStore.getState().overlayStack;
  if (stack.length === 0) return null;
  const top = stack[stack.length - 1] ?? null;
  uiStore.setState({ overlayStack: stack.slice(0, -1) });
  return top;
}
