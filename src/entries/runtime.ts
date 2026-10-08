import type { AppMode } from "../state/uiStore";
import { installLogCapture } from "../lib/logger";

/** 窗口入口类型：desktop = 桌面环境窗口；四款独立软件；explorer = 系统窗口（文件管理器/回收站）；datavault = 数据安全中心（AI-10）；taskbar = 任务栏独立顶层窗（M4-B）。 */
export type EntryType = "desktop" | AppMode | "explorer" | "datavault" | "taskbar";

export function isTauriRuntime(): boolean {
  const internals = (window as { __TAURI_INTERNALS__?: { __variableDevStub?: boolean } }).__TAURI_INTERNALS__;
  return typeof window !== "undefined" && !!internals && internals.__variableDevStub !== true;
}

/**
 * 每窗口入口共享的运行时装配（M4 拆窗）：
 * - DEV-ONLY Tauri 运行时 stub：让 `vite dev` 在纯浏览器中也能启动
 *   （打包后的 Tauri webview 定义了 __TAURI_INTERNALS__，stub 永不生效）。
 * - 全局错误钩子：errBoard 聚合 + 崩溃叙事 + 统一日志时间线
 *   （采集/存储/导出链路见 src/lib/logger.ts 与 docs/异常日志系统-AI诊断指南.md）。
 */
export function setupEntryRuntime(entry: EntryType): void {
  type TauriInternals = { invoke: (cmd: string, args?: unknown) => Promise<unknown> };
  const w = window as unknown as { __TAURI_INTERNALS__?: TauriInternals };
  if (import.meta.env.DEV && !w.__TAURI_INTERNALS__) {
    const label = entry === "desktop" || entry === "explorer" || entry === "datavault" || entry === "taskbar" ? entry : `app-${entry}`;
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

  // 异常实时分析统一装配：errBoard 全局错误环形 + 崩溃叙事 + console 桥接
  // + window/promise 兜底（console 镜像保持 "[Variable] uncaught" 原样）+ 落盘。
  installLogCapture(entry);
}

/**
 * 首帧渲染后移除 boot-splash（各入口共用；此前 explorer 曾漏移除，
 * 窗口永远停在启动屏）。淡出 400ms 与 boot-splash 的 CSS 过渡对齐。
 */
export function dismissBootSplash(): void {
  requestAnimationFrame(() => {
    const splash = document.getElementById("boot-splash");
    splash?.classList.add("done");
    window.setTimeout(() => splash?.remove(), 400);
  });
}
