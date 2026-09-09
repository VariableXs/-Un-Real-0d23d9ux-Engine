import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, X } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import type { CompatStatus } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";

/**
 * CEF 应用（Wallpaper Engine / Steam）兼容横幅（悬浮在桌面顶部，桌面环境独有）。
 * - 后端在启动/运行期检测 wallpaper64/wallpaperservice/steam/steamwebhelper 进程
 *   （Steam 与 WE 同为 Chromium 系：GPU 竞争与 z-order 抖动同源）；
 * - 命中时自动进入兼容态（alwaysOnTop=false + 壁纸降载），相关进程全部退出
 *   约 30s 后自动恢复；
 * - 用户可一键“兼容/恢复”或关闭横幅（关闭后本会话不再自动弹出，直到状态翻转）。
 */
export function CompatBanner(): React.ReactElement | null {
  const { t } = useI18n();
  const [st, setSt] = useState<CompatStatus | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    // 初次查询
    ipc
      .compatCheck()
      .then((s) => {
        if (!cancelled) setSt(s);
      })
      .catch(() => {});

    // 后端 watcher 推送
    const un = listen<CompatStatus>("compat://cef-apps", (e) => {
      setSt(e.payload);
      // 状态翻转时重置 dismissed，让横幅有机会再次出现
      setDismissed(false);
      if (e.payload.wallpaperEngineRunning && e.payload.severity === "high") {
        pushToast("info", t("compatWallpaperTitle"), t("compatWallpaperBody"));
      }
    });
    return () => {
      cancelled = true;
      void un.then((f) => f()).catch(() => {});
    };
  }, [t]);

  if (!st || !st.wallpaperEngineRunning || dismissed) return null;

  const isHigh = st.severity === "high";
  const isMitigated = st.severity === "mitigated";

  return (
    <div
      role="alert"
      aria-live="polite"
      style={{
        position: "absolute",
        top: 12,
        left: "50%",
        transform: "translateX(-50%)",
        zIndex: 5000,
        maxWidth: 640,
        width: "calc(100% - 32px)",
        background: isHigh ? "rgba(180, 40, 30, 0.96)" : "rgba(30, 120, 70, 0.96)",
        color: "#fff",
        borderRadius: 10,
        padding: "10px 14px",
        display: "flex",
        gap: 12,
        alignItems: "flex-start",
        boxShadow: "0 8px 24px rgba(0,0,0,0.35)",
        backdropFilter: "blur(6px)",
        fontSize: 13,
        lineHeight: 1.45,
      }}
    >
      <AlertTriangle size={18} style={{ marginTop: 2, flexShrink: 0 }} />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontWeight: 700, marginBottom: 4 }}>{t("compatWallpaperTitle")}</div>
        <div style={{ opacity: 0.96 }}>
          {t("compatWallpaperBody")}
          {st.processes.length > 0 ? (
            <span style={{ opacity: 0.85 }}>（{st.processes.join(", ")}）</span>
          ) : null}
        </div>
        {isMitigated ? (
          <div style={{ opacity: 0.9, marginTop: 6, fontSize: 12 }}>{t("compatHelp")}</div>
        ) : null}
        <div style={{ display: "flex", gap: 8, marginTop: 10, flexWrap: "wrap" }}>
          {isHigh ? (
            <button
              disabled={busy}
              onClick={() => {
                setBusy(true);
                ipc
                  .compatApply()
                  .then((ns) => {
                    setSt(ns);
                    pushToast("success", t("compatWallpaperTitle"), t("compatApply"));
                  })
                  .catch((e) => pushToast("error", t("compatWallpaperTitle"), errMessage(e).message))
                  .finally(() => setBusy(false));
              }}
              style={{
                background: "#fff",
                color: isHigh ? "#b4281e" : "#1e7846",
                border: "none",
                borderRadius: 6,
                padding: "5px 10px",
                fontWeight: 600,
                cursor: "pointer",
              }}
            >
              {t("compatApply")}
            </button>
          ) : (
            <button
              disabled={busy}
              onClick={() => {
                setBusy(true);
                ipc
                  .compatRestore()
                  .then((ns) => {
                    setSt(ns);
                    pushToast("success", t("compatWallpaperTitle"), t("compatRestore"));
                  })
                  .catch((e) => pushToast("error", t("compatWallpaperTitle"), errMessage(e).message))
                  .finally(() => setBusy(false));
              }}
              style={{
                background: "rgba(255,255,255,0.16)",
                color: "#fff",
                border: "1px solid rgba(255,255,255,0.35)",
                borderRadius: 6,
                padding: "5px 10px",
                fontWeight: 600,
                cursor: "pointer",
              }}
            >
              {t("compatRestore")}
            </button>
          )}
          <button
            onClick={() => setDismissed(true)}
            style={{
              background: "transparent",
              color: "#fff",
              border: "1px solid rgba(255,255,255,0.35)",
              borderRadius: 6,
              padding: "5px 10px",
              cursor: "pointer",
            }}
          >
            {t("compatDismiss")}
          </button>
        </div>
      </div>
      <button
        aria-label="close"
        onClick={() => setDismissed(true)}
        style={{
          background: "transparent",
          border: "none",
          color: "#fff",
          cursor: "pointer",
          padding: 4,
          marginLeft: 4,
          flexShrink: 0,
        }}
      >
        <X size={16} />
      </button>
    </div>
  );
}
