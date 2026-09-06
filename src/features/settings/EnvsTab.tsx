import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { errMessage, ipc } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { askConfirm } from "../../components/Modal";
import type { Settings } from "../../lib/settings";

/**
 * B-24：子环境档管理（设置页「环境」标签）。
 * 切换编排：①当前设置快照写入旧环境（随请求上传）→ ②后端翻转 active →
 * ③返回新环境设置快照 → ④前端 patchSettings 应用（Shell 状态重载短版）。
 * 隔离口径：执行档 {envhome} 指向活动环境的 home——凭据/登录态按环境隔离。
 */
interface EnvView {
  id: string;
  name: string;
  active: boolean;
  createdAt: number;
}

/** 随环境快照的偏好子集（写操作隔离从这批偏好开始）。 */
function snapshotSettings(s: Settings): Record<string, string> {
  return {
    wallpaperMode: s.wallpaperMode,
    theme: s.theme,
    iconSize: String(s.iconSize),
    taskbarPos: s.taskbarPos,
  };
}

export function EnvsTab(props: { settings: Settings; onPatch: (p: Partial<Settings>) => void }) {
  const { t } = useI18n();
  const [envs, setEnvs] = useState<EnvView[]>([]);
  const [newName, setNewName] = useState("");
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setEnvs(await ipc.envList());
    } catch (e) {
      pushToast("error", t("evListFail"), errMessage(e).message);
    }
  }, [t]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const withBusy = async (fn: () => Promise<void>) => {
    if (busy) return;
    setBusy(true);
    try {
      await fn();
    } finally {
      setBusy(false);
    }
  };

  const create = () =>
    void withBusy(async () => {
      if (!newName.trim()) return;
      try {
        await ipc.envCreate(newName.trim());
        setNewName("");
        await refresh();
        pushToast("success", t("evCreated"), newName.trim());
      } catch (e) {
        pushToast("error", t("evCreateFail"), errMessage(e).message);
      }
    });

  const switchTo = (id: string, name: string) =>
    void withBusy(async () => {
      try {
        // ①当前偏好快照随切换上传；③返回目标环境快照
        const snapshot = await ipc.envSwitch(id, snapshotSettings(props.settings));
        // ④应用目标环境快照（Shell 状态重载由设置联动完成——短版启动）
        if (snapshot && typeof snapshot === "object") {
          const snap = snapshot as Record<string, unknown>;
          props.onPatch({
            wallpaperMode: (snap.wallpaperMode as Settings["wallpaperMode"]) ?? props.settings.wallpaperMode,
            theme: (snap.theme as Settings["theme"]) ?? props.settings.theme,
            iconSize: snap.iconSize ? (Number(snap.iconSize) as Settings["iconSize"]) : props.settings.iconSize,
            taskbarPos: (snap.taskbarPos as Settings["taskbarPos"]) ?? props.settings.taskbarPos,
          });
        }
        await refresh();
        pushToast("success", t("evSwitched"), name);
      } catch (e) {
        pushToast("error", t("evSwitchFail"), errMessage(e).message);
      }
    });

  const remove = (e: EnvView) =>
    void withBusy(async () => {
      const ok = await askConfirm({
        title: t("evDeleteTitle"),
        body: `${e.name} · ${t("evDeleteWarn")}`,
        danger: true,
        okLabel: t("evDeleteOk"),
      });
      if (!ok) return;
      try {
        await ipc.envDelete(e.id);
        await refresh();
        pushToast("success", t("evDeleted"), e.name);
      } catch (err) {
        pushToast("error", t("evDeleteFail"), errMessage(err).message);
      }
    });

  return (
    <div className="ev-tab">
      <p className="dim small">{t("evHint")}</p>
      <div className="ev-add">
        <input
          placeholder={t("evNewName")}
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && create()}
        />
        <button type="button" disabled={!newName.trim()} onClick={create}>
          {t("evCreate")}
        </button>
      </div>
      <div className="ev-list">
        {envs.map((e) => (
          <div key={e.id} className={`ev-row${e.active ? " active" : ""}`}>
            <span className="ev-name">
              {e.name}
              {e.active && <span className="ev-badge">{t("evActive")}</span>}
            </span>
            <span className="ev-ops">
              {!e.active && (
                <button type="button" onClick={() => switchTo(e.id, e.name)}>
                  {t("evSwitch")}
                </button>
              )}
              {e.id !== "main" && (
                <button type="button" className="danger" onClick={() => remove(e)}>
                  {t("evDelete")}
                </button>
              )}
            </span>
          </div>
        ))}
      </div>
      <p className="dim small">{t("evIsolationNote")}</p>
    </div>
  );
}
