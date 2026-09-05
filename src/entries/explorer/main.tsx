import React from "react";
import ReactDOM from "react-dom/client";
import { setupEntryRuntime, isTauriRuntime } from "../runtime";
import { ExplorerWindow } from "../../system/explorer/ExplorerWindow";
import "../../styles/global.css";
import "../../styles/desktop.css";

// M6 系统窗口：文件管理器（explorer）/ 回收站（label=recycle，?view=recycle）。
setupEntryRuntime("explorer");

const view = new URLSearchParams(window.location.search).get("view") === "recycle" ? "recycle" : "explorer";
if (!isTauriRuntime()) {
  // 浏览器 dev 模式：无 IPC 后端，界面照常挂载但显示"状态未知"，不伪造数据。
  console.info("[explorer] no Tauri backend, running in browser mode");
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ExplorerWindow initialView={view} />
  </React.StrictMode>,
);

// 首帧渲染后移除 boot-splash（此前从未移除，窗口永远停在启动屏）
requestAnimationFrame(() => {
  const splash = document.getElementById("boot-splash");
  splash?.classList.add("done");
  window.setTimeout(() => splash?.remove(), 400);
});
