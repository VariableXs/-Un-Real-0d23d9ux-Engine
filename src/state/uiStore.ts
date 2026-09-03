import { createStore, useStore } from "../lib/store";

export type SaveStatus = "saved" | "saving" | "dirty" | "error";

export interface UiState {
  mode: "write" | "mindmap" | "project" | "fate";
  /** Pending .project archive to open when the project space mounts (ch.12.2). */
  pvPendingOpen: string | null;
  /** Pending local file (e.g. .fatetree) to open when the fate space mounts. */
  fatePendingOpen: string | null;
  /** Pending local .md/.txt to open as a document when the write space mounts. */
  writePendingOpen: string | null;
  sidebarOpen: boolean;
  searchOpen: boolean;
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
}

export const uiStore = createStore<UiState>({
  mode: "write",
  pvPendingOpen: null,
  fatePendingOpen: null,
  writePendingOpen: null,
  sidebarOpen: false,
  searchOpen: false,
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
