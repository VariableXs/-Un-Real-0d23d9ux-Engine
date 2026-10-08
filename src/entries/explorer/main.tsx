import React from "react";
import ReactDOM from "react-dom/client";
import { setupEntryRuntime, isTauriRuntime, dismissBootSplash } from "../runtime";
import { ExplorerWindow } from "../../system/explorer/ExplorerWindow";
import { createWindowRuntime } from "../../features/mouse/windowRuntime";
import "../../styles/global.css";
import "../../styles/desktop.css";

// M6 系统窗口：文件管理器（explorer）/ 回收站（label=recycle，?view=recycle）。
setupEntryRuntime("explorer");

const view = new URLSearchParams(window.location.search).get("view") === "recycle" ? "recycle" : "explorer";
if (!isTauriRuntime()) {
  // 浏览器 dev 模式：无 IPC 后端，界面照常挂载但显示"状态未知"，不伪造数据。
  console.info("[explorer] no Tauri backend, running in browser mode");
}

// J 鼠标域 AI-J1（v3 接线）：explorer 窗口的指针/滚轮/侧键/手势内核
// （headless——滚轮刻度/自适应增益/穿透/侧键后退前进/慢速微调在此窗真实生效；
// 作用域声明让 F605 应用覆盖与 F616 应用档案可按 explorer 单独定制）。
document.documentElement.dataset.appId = view === "recycle" ? "recycle" : "explorer";
document.documentElement.dataset.appClass = "list";
const j1Runtime = createWindowRuntime({ entry: "explorer", appScope: document.documentElement.dataset.appId, appClass: "list", replica: false });
window.addEventListener("pagehide", () => j1Runtime.dispose(), { once: true });

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ExplorerWindow initialView={view} />
  </React.StrictMode>,
);

// 首帧渲染后移除 boot-splash（此前从未移除，窗口永远停在启动屏）
dismissBootSplash();
