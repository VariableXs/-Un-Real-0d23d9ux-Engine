import type { AppMode } from "../state/uiStore";

/** 窗口入口类型：desktop = 桌面环境窗口；四款独立软件；explorer = 系统窗口（文件管理器/回收站）。 */
export type EntryType = "desktop" | AppMode | "explorer";

export function isTauriRuntime(): boolean {
  const internals = (window as { __TAURI_INTERNALS__?: { __variableDevStub?: boolean } }).__TAURI_INTERNALS__;
  return typeof window !== "undefined" && !!internals && internals.__variableDevStub !== true;
}

/**
 * 每窗口入口共享的运行时装配（M4 拆窗）：
 * - DEV-ONLY Tauri 运行时 stub：让 `vite dev` 在纯浏览器中也能启动
 *   （打包后的 Tauri webview 定义了 __TAURI_INTERNALS__，stub 永不生效）。
 * - 全局错误钩子：Rust 侧日志可捕获前端错误。
 */
export function setupEntryRuntime(entry: EntryType): void {
  type TauriInternals = { invoke: (cmd: string, args?: unknown) => Promise<unknown> };
  const w = window as unknown as { __TAURI_INTERNALS__?: TauriInternals };
  if (import.meta.env.DEV && !w.__TAURI_INTERNALS__) {
    const label = entry === "desktop" || entry === "explorer" ? entry : `app-${entry}`;
    w.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label },
        currentWebview: { label, windowLabel: label },
      },
      plugins: {},
      transformCallback: (cb: unknown) => cb,
      invoke: () => Promise.reject(new Error("no-tauri-dev")),
      __variableDevStub: true,
    } as unknown as TauriInternals;
  }

  window.addEventListener("error", (e) => {
    console.error("[Variable] uncaught", e.error ?? e.message);
  });
  window.addEventListener("unhandledrejection", (e) => {
    console.error("[Variable] unhandled rejection", e.reason);
  });
}
