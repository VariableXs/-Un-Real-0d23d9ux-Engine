import React from "react";
import ReactDOM from "react-dom/client";
import { setupEntryRuntime } from "../runtime";
import App from "../../App";
import "../../styles/global.css";

// Variable Code 窗口：独立软件，功能内核 🔒（零修改）。
setupEntryRuntime("project");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App appType="project" />
  </React.StrictMode>,
);
