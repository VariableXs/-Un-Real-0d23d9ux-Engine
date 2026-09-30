import { useCallback, useEffect, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../i18n";
import { errMessage, ipc, type Shell } from "../../lib/ipc";
import { PERMISSION_HINTS } from "../../lib/uxpack";
import { pushToast } from "../../state/uiStore";

/**
 * X-6：扩展商店原型（设置→扩展→商店）——
 * 市场 = `<data>/market/*.uxpack` 包列表；导入本机 .uxpack / 一键安装（解包到
 * extensions/<id>/ 即生效）/ 移除包。安装前逐权限回显（黄条 = 未签名警示）。
 * 打包工具见 tools/uxpack.cjs；远程分发源（HTTP 拉取）属后续批，V1 零网络。
 */
export function Marketplace({ onInstalled }: { onInstalled: () => void }) {
  const { t } = useI18n();
  const [packs, setPacks] = useState<Shell.MarketPackView[]>([]);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(
    () =>
      ipc
        .extMarketList()
        .then(setPacks)
        .catch((e) => pushToast("error", t("extMarketFail"), errMessage(e).message)),
    [t],
  );

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const importPack = async () => {
    const picked = await openFileDialog({
      multiple: false,
      filters: [{ name: "uxpack", extensions: ["uxpack"] }],
    });
    if (typeof picked !== "string") return;
    setBusy(true);
    try {
      await ipc.extMarketImport(picked);
      await refresh();
      pushToast("success", t("extMarketImported"), picked);
    } catch (e) {
      pushToast("error", t("extMarketFail"), errMessage(e).message);
    } finally {
      setBusy(false);
    }
  };

  const install = (file: string) =>
    void (async () => {
      setBusy(true);
      try {
        const id = await ipc.extMarketInstall(file);
        await refresh();
        onInstalled();
        pushToast("success", t("extMarketInstalled"), `${id} · ${t("extMarketInstalledDetail")}`);
      } catch (e) {
        pushToast("error", t("extMarketFail"), errMessage(e).message);
      } finally {
        setBusy(false);
      }
    })();

  const remove = (file: string) =>
    void (async () => {
      try {
        await ipc.extMarketRemove(file);
        await refresh();
      } catch (e) {
        pushToast("error", t("extMarketFail"), errMessage(e).message);
      }
    })();

  return (
    <div className="ext-market">
      <div className="sec-actions">
        <strong>{t("extMarketTitle")}</strong>
        <button type="button" disabled={busy} onClick={() => void importPack()}>
          {t("extMarketImport")}
        </button>
        <button type="button" onClick={() => void refresh()}>
          {t("extRescan")}
        </button>
      </div>
      <p className="dim small">{t("extMarketHint")}</p>

      {packs.length === 0 && <p className="dim small">{t("extMarketEmpty")}</p>}

      {packs.map((p) => (
        <div key={p.file} className="ext-card">
          <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
            <strong>
              {p.name} <span className="dim small">v{p.version}</span>
            </strong>
            <span className="dim small">
              {p.id} · {p.kind} · {(p.sizeBytes / 1024).toFixed(1)} KB
            </span>
            {!p.signed && <span className="ext-unsigned">⚠ {t("extUnsigned")}</span>}
            <button type="button" disabled={busy} onClick={() => install(p.file)}>
              {p.installed ? t("extMarketReinstall") : t("extMarketInstall")}
            </button>
            <button type="button" onClick={() => remove(p.file)}>
              {t("extMarketRemove")}
            </button>
          </div>
          {p.description && <p className="dim small">{p.description}</p>}
          {p.permissions.length > 0 && (
            <div className="ext-perms">
              {p.permissions.map((perm) => (
                <span key={perm} className="ext-perm" title={PERMISSION_HINTS[perm.split(":")[0] ?? ""] ?? ""}>
                  {perm}
                </span>
              ))}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
