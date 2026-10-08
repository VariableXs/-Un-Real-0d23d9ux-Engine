import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { errMessage, ipc, type Shell } from "../../lib/ipc";
import { PERMISSION_HINTS } from "../../lib/uxpack";
import { pushToast } from "../../state/uiStore";
import { Marketplace } from "./Marketplace";

/**
 * X-6/X-1：扩展管理页（设置→扩展）——列表 / 启停 / 权限查看 /
 * 未签名黄色警示 / 崩溃重启 / 安装示例扩展 + 商店原型（Marketplace）。
 */

export function ExtensionsTab() {
  const { t } = useI18n();
  const [exts, setExts] = useState<Shell.ExtView[]>([]);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(
    () =>
      ipc
        .extList()
        .then(setExts)
        .catch((e) => pushToast("error", t("extLoadFail"), errMessage(e).message)),
    [t],
  );

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const toggle = (id: string, on: boolean) =>
    void (async () => {
      try {
        await ipc.extSetEnabled(id, on);
        if (on) await ipc.extOpenWeb(id);
        else await ipc.extClose(id);
        await refresh();
      } catch (e) {
        pushToast("error", t("extToggleFail"), errMessage(e).message);
      }
    })();

  const restart = (id: string) =>
    void ipc
      .extOpenWeb(id)
      .then(refresh)
      .catch((e) => pushToast("error", t("extToggleFail"), errMessage(e).message));

  const installExample = () =>
    void (async () => {
      setBusy(true);
      try {
        await ipc.extInstallExample();
        await refresh();
        pushToast("success", t("extExampleDone"), t("extExampleDetail"));
      } catch (e) {
        pushToast("error", t("extToggleFail"), errMessage(e).message);
      } finally {
        setBusy(false);
      }
    })();

  return (
    <div className="ext-tab">
      <p className="dim small">{t("extHint")}</p>
      <div className="sec-actions">
        <button type="button" disabled={busy} onClick={installExample}>
          {t("extInstallExample")}
        </button>
        <button type="button" onClick={() => void refresh()}>{t("extRescan")}</button>
      </div>

      {exts.length === 0 && <p className="dim small">{t("extEmpty")}</p>}

      {exts.map((e) => (
        <div key={e.id} className="ext-card">
          <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
            <strong>
              {e.name} <span className="dim small">v{e.version}</span>
            </strong>
            <span className="dim small">{e.id} · {e.kind}</span>
            {!e.signed && <span className="ext-unsigned">⚠ {t("extUnsigned")}</span>}
            {e.crashed && <span style={{ color: "#e66" }}>■ {t("extCrashed")}</span>}
            <button type="button" onClick={() => toggle(e.id, !e.enabled)}>
              {e.enabled ? "ON" : "OFF"}
            </button>
            {e.crashed && (
              <button type="button" onClick={() => restart(e.id)}>
                {t("extRestart")}
              </button>
            )}
          </div>
          {e.description && <p className="dim small">{e.description}</p>}
          <div className="ext-perms">
            {e.permissions.map((p) => (
              <span key={p} className="ext-perm" title={PERMISSION_HINTS[p.split(":")[0] ?? ""] ?? ""}>
                {p}
              </span>
            ))}
          </div>
        </div>
      ))}

      {/* X-6：商店原型（导入/安装/移除 .uxpack 包） */}
      <Marketplace onInstalled={refresh} />

      {/* X-4/X-5：守护进程示例（JSON-RPC 2.0 over 环回 TCP；插件加载命令已注册，V1 无预编译示例 .rlb） */}
      <div className="sec-actions">
        <strong className="dim small">X-4 / X-5</strong>
        <button
          type="button"
          onClick={() =>
            void (async () => {
              try {
                const script = await ipc.extDaemonExample();
                const port = await ipc.extDaemonStart("calendar-sync", "node", [script]);
                pushToast("success", t("extDaemonStarted"), `127.0.0.1:${port}`);
              } catch (e) {
                pushToast("error", t("extToggleFail"), errMessage(e).message);
              }
            })()
          }
        >
          {t("extDaemonExampleBtn")}
        </button>
        <button
          type="button"
          onClick={() =>
            void ipc
              .extDaemonStatus()
              .then((ds) =>
                pushToast(
                  "info",
                  t("extDaemonStatusTitle"),
                  ds.length ? ds.map((d) => `${d.id}@${d.port}${d.stopped ? " (stopped)" : ""}`).join(", ") : t("extDaemonNone"),
                ),
              )
              .catch((e) => pushToast("error", t("extToggleFail"), errMessage(e).message))
          }
        >
          {t("extDaemonStatusTitle")}
        </button>
      </div>
    </div>
  );
}
