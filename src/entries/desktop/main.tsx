import React from "react";
import ReactDOM from "react-dom/client";
import { setupEntryRuntime } from "../runtime";
import App from "../../App";
import "../../styles/global.css";
import "../../styles/desktop.css";

// 桌面环境窗口：启动仪式 → 壁纸 → 图标网格 → 任务栏/开始菜单。
setupEntryRuntime("desktop");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App appType="desktop" />
  </React.StrictMode>,
);
