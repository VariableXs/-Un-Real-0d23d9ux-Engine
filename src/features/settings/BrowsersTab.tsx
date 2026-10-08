import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { errMessage, ipc, type BrowserProfileDto, type DetectedBrowser } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { askConfirm, askPrompt } from "../../components/Modal";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";

/**
 * B-18/B-19：浏览器矩阵管理（设置页「浏览器」标签）。
 * - 检测本机浏览器（App Paths + 常见路径，纯本机零网络）；
 * - Profile 管理器：新建/改名/克隆/删除（可焚毁）——数据目录全在容器 browsers/ 下；
 * - 一键启动（执行档通道注入，user-data-dir/-profile 指向容器）；
 * - 首次导入：书签 HTML / 密码 CSV 文件复制进容器（绝不读取宿主浏览器运行数据）。
 */
export function BrowsersTab() {
  const { t } = useI18n();
  const [detected, setDetected] = useState<DetectedBrowser[]>([]);
  const [profiles, setProfiles] = useState<BrowserProfileDto[]>([]);
  const [newName, setNewName] = useState("");
  const [selectedBrowser, setSelectedBrowser] = useState<string>("");
  const busyRef = useRef(false);

  const refresh = useCallback(async () => {
    try {
      const [d, p] = await Promise.all([ipc.browserDetect(), ipc.browserProfiles()]);
      setDetected(d);
      setProfiles(p);
    } catch (e) {
      pushToast("error", t("brDetectFail"), errMessage(e).message);
    }
  }, [t]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const withBusy = async (fn: () => Promise<void>) => {
    if (busyRef.current) return;
    busyRef.current = true;
    try {
      await fn();
    } finally {
      busyRef.current = false;
    }
  };

  const addProfile = () =>
    void withBusy(async () => {
      const d = detected.find((x) => x.id === selectedBrowser) ?? detected[0];
      if (!d || !newName.trim()) return;
      try {
        await ipc.browserProfileAdd(d.id, d.exe, newName.trim());
        setNewName("");
        await refresh();
        pushToast("success", t("brAdded"), newName.trim());
      } catch (e) {
        pushToast("error", t("brAddFail"), errMessage(e).message);
      }
    });

  const launch = (p: BrowserProfileDto) =>
    void withBusy(async () => {
      try {
        await ipc.browserProfileLaunch(p.id);
        pushToast("success", t("brLaunched"), p.name);
      } catch (e) {
        pushToast("error", t("brLaunchFail"), errMessage(e).message);
      }
    });

  const clone = (p: BrowserProfileDto) =>
    void withBusy(async () => {
      const name = await askPrompt({
        title: t("brCloneTitle"),
        initial: `${p.name} 2`,
      });
      if (!name) return;
      try {
        await ipc.browserProfileClone(p.id, name);
        await refresh();
        pushToast("success", t("brCloned"), name);
      } catch (e) {
        pushToast("error", t("brCloneFail"), errMessage(e).message);
      }
    });

  const remove = (p: BrowserProfileDto) =>
    void withBusy(async () => {
      const ok = await askConfirm({
        title: t("brDeleteTitle"),
        body: `${p.name} · ${t("brDeleteWarn")}`,
        danger: true,
        okLabel: t("brShred"),
      });
      if (!ok) return;
      try {
        await ipc.browserProfileDelete(p.id, true);
        await refresh();
        pushToast("success", t("brDeleted"), p.name);
      } catch (e) {
        pushToast("error", t("brDeleteFail"), errMessage(e).message);
      }
    });

  const importFiles = (p: BrowserProfileDto) =>
    void withBusy(async () => {
      // 两选一：书签 HTML 或密码 CSV（导出文件复制进容器，绝不读宿主浏览器运行数据）
      const html = await openFileDialog({
        multiple: false,
        filters: [{ name: "Bookmarks HTML", extensions: ["html", "htm"] }],
      });
      const bookmarkHtml = typeof html === "string" ? html : undefined;
      let passwordCsv: string | undefined;
      if (!bookmarkHtml) {
        const csv = await openFileDialog({
          multiple: false,
          filters: [{ name: "Passwords CSV", extensions: ["csv"] }],
        });
        passwordCsv = typeof csv === "string" ? csv : undefined;
      }
      if (!bookmarkHtml && !passwordCsv) return;
      try {
        const r = await ipc.browserImport(p.id, bookmarkHtml, passwordCsv);
        pushToast(
          "success",
          t("brImportDone"),
          t("brImportSummary", { b: r.bookmarks, c: r.passwords }),
        );
        await refresh();
      } catch (e) {
        pushToast("error", t("brImportFail"), errMessage(e).message);
      }
    });

  return (
    <div className="br-tab">
      <p className="dim small">{t("brHint")}</p>

      <h4>{t("brDetected")}</h4>
      {detected.length === 0 && <p className="dim small">{t("brNone")}</p>}
      <div className="br-detected">
        {detected.map((d) => (
          <button
            key={d.id}
            type="button"
            className={selectedBrowser === d.id ? "on" : ""}
            onClick={() => setSelectedBrowser(d.id)}
          >
            {d.name}
          </button>
        ))}
      </div>

      <div className="br-add">
        <input
          placeholder={t("brNewName")}
          value={newName}
          onChange={(e) => setNewName(e.target.value)}
        />
        <button type="button" disabled={!newName.trim() || detected.length === 0} onClick={addProfile}>
          {t("brAdd")}
        </button>
      </div>

      <h4>{t("brProfiles")}</h4>
      {profiles.length === 0 && <p className="dim small">{t("brNoProfiles")}</p>}
      <div className="br-profiles">
        {profiles.map((p) => (
          <div key={p.id} className="br-profile">
            <span className="br-name">
              {p.name} <span className="dim small">({p.browserId})</span>
            </span>
            <span className="br-ops">
              <button type="button" onClick={() => launch(p)}>
                {t("brLaunch")}
              </button>
              <button type="button" onClick={() => clone(p)}>
                {t("brClone")}
              </button>
              <button type="button" onClick={() => importFiles(p)}>
                {t("brImport")}
              </button>
              <button type="button" className="danger" onClick={() => remove(p)}>
                {t("brDelete")}
              </button>
            </span>
          </div>
        ))}
      </div>
      <p className="dim small">{t("brPrivacyNote")}</p>
    </div>
  );
}
