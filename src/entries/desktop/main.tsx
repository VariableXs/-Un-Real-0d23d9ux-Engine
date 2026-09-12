import React from "react";
import ReactDOM from "react-dom/client";
import { setupEntryRuntime } from "../runtime";
import App from "../../App";
// AURORA-10000：AI-06~AI-10 领域02 窗口与空间 25 族参数运行时（F00626~F01250），勿删
import { bootstrapWindowSpace } from "../../system/windows/aurora/bootstrap";
import "../../styles/global.css";
import "../../styles/desktop.css";
import "../../styles/vwm.css";
import "../../styles/singularity.css";

// 桌面环境窗口：启动仪式 → 壁纸 → 图标网格 → 任务栏/开始菜单。
setupEntryRuntime("desktop");
bootstrapWindowSpace();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App appType="desktop" />
  </React.StrictMode>,
);
