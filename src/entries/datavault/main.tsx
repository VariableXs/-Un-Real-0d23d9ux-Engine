import React, { useEffect, useMemo, useState } from "react";
import ReactDOM from "react-dom/client";
import { setupEntryRuntime, isTauriRuntime, dismissBootSplash } from "../runtime";
import { I18nContext, makeT } from "../../i18n";
import type { Lang } from "../../i18n/dictionaries";
import { loadSettings } from "../../lib/settings";
import { DataVaultWindow } from "../../system/datavault/DataVaultWindow";
import { createWindowRuntime } from "../../features/mouse/windowRuntime";
import "../../styles/global.css";
import "../../styles/datavault.css";

// AI-10 数据安全中心独立系统窗口（datavault.html）。
setupEntryRuntime("datavault");

// J 鼠标域 AI-J1（v3 接线）：数据安全中心的滚轮/侧键内核（headless）。
document.documentElement.dataset.appId = "datavault";
document.documentElement.dataset.appClass = "list";
const j1Runtime = createWindowRuntime({ entry: "datavault", appScope: "datavault", appClass: "list", replica: false });
window.addEventListener("pagehide", () => j1Runtime.dispose(), { once: true });

/** 与桌面主窗口共享语言设置（只读跟随；切换仍在桌面设置里做）。 */
function I18nFollow({ children }: { children: React.ReactNode }): React.ReactElement {
  const [lang, setLang] = useState<Lang>("zh");
  useEffect(() => {
    let alive = true;
    void loadSettings()
      .then((s) => {
        if (alive && s?.language) setLang(s.language);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, []);
  const ctx = useMemo(() => ({ lang, setLang: () => {}, t: makeT(lang) }), [lang]);
  return <I18nContext.Provider value={ctx}>{children}</I18nContext.Provider>;
}

if (!isTauriRuntime()) {
  console.info("[datavault] no Tauri backend, running in browser mode");
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <I18nFollow>
      <DataVaultWindow />
    </I18nFollow>
  </React.StrictMode>,
);

// 首帧渲染后移除 boot-splash
dismissBootSplash();
