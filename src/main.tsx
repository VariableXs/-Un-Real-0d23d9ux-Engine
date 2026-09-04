import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles/global.css";

// DEV-ONLY: minimal Tauri runtime stub so `vite dev` in a plain browser can
// boot past window/internals probes (settings then fall back to defaults).
// Never active inside the packaged Tauri app, which defines __TAURI_INTERNALS__.
type TauriInternals = { invoke: (cmd: string, args?: unknown) => Promise<unknown> };
const w = window as unknown as { __TAURI_INTERNALS__?: TauriInternals };
if (import.meta.env.DEV && !w.__TAURI_INTERNALS__) {
  w.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main", windowLabel: "main" } },
    plugins: {},
    transformCallback: (cb: unknown) => cb,
    invoke: () => Promise.reject(new Error("no-tauri-dev")),
    __variableDevStub: true,
  } as unknown as TauriInternals;
}

// Expose a global error hook so Rust-side logging can capture frontend errors.
window.addEventListener("error", (e) => {
  console.error("[Variable] uncaught", e.error ?? e.message);
});
window.addEventListener("unhandledrejection", (e) => {
  console.error("[Variable] unhandled rejection", e.reason);
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
