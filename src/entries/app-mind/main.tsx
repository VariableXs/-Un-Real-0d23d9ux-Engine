import React from "react";
import ReactDOM from "react-dom/client";
import { setupEntryRuntime } from "../runtime";
import App from "../../App";
import "../../styles/global.css";

// Variable Mind 窗口：独立软件，功能内核 🔒（零修改）。
setupEntryRuntime("mindmap");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App appType="mindmap" />
  </React.StrictMode>,
);
