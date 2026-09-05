import React from "react";
import ReactDOM from "react-dom/client";
import { setupEntryRuntime } from "../runtime";
import App from "../../App";
import "../../styles/global.css";

// Variable Write 窗口：独立软件，功能内核 🔒（迁移自 v1，零修改）。
setupEntryRuntime("write");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App appType="write" />
  </React.StrictMode>,
);
