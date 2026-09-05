import { useEffect, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { PackagePlus, Play, RotateCcw, Search, Trash2 } from "lucide-react";
import { askConfirm, askPrompt, Modal } from "../../components/Modal";
import { errMessage, ipc, type OfficialUsage, type TpGrade, type TpScanCandidate, type ThirdApp } from "../../lib/ipc";
import { formatBytes } from "../../lib/format";
import { useI18n } from "../../i18n";
import { pushToast, uiStore, useUi, type AppMode } from "../../state/uiStore";
import { closeAppWindows } from "../windows/appWindows";
import { desktopAppLabel, desktopIconDefs } from "../desktop-icons/DesktopIcons";
import {
  markOfficialInstalled,
  markOfficialUninstalled,
  useUninstalledOfficial,
} from "./official";
import { launchThirdApp, reloadThirdApps, useThirdApps } from "./thirdApps";

/**
 * 软件管理器（M7 + 批次C，桌面窗口模态）：
 * - 页签「第三方软件」：登记/启动/改名/调级/移除登记/彻底删除文件（仅数据目录内便携软件）
 * - 页签「已安装软件」：预装四软件平级管理（规格 5.6）：
 *   · 卸载前显示数据占用；需输入「确认卸载 Variable ××」防误操作
 *   · 卸载 = 隐藏入口 + 自动关闭运行窗口；数据默认保留，24h 内可一键恢复
 *   · 彻底删除数据（仅 write/mindmap 库数据）为独立危险操作，与卸载分离
 * - 预装软件与第三方平级，无任何特权；卸载互不影响
 */

const EXE_FILTERS = [{ name: "程序文件 / Programs", extensions: ["exe", "lnk", "bat", "cmd"] }];

const GRADE_OPTIONS: TpGrade[] = ["portable", "standalone", "shortcut"];

export function LauncherManager(): React.ReactElement | null {
  const { t } = useI18n();
  const open = useUi((s) => s.launcherOpen);
  const tab = useUi((s) => s.launcherTab);
  const apps = useThirdApps();
  const uninstalled = useUninstalledOfficial();
  const [busy, setBusy] = useState(false);
  const [usage, setUsage] = useState<Partial<Record<AppMode, OfficialUsage>>>({});
  // 卸载确认（输入短语防误操作，规格 5.6.3）
  const [uninstallTarget, setUninstallTarget] = useState<AppMode | null>(null);
  const [confirmText, setConfirmText] = useState("");
  const [confirmErr, setConfirmErr] = useState<string | null>(null);
  // 批次E（规格 5.9.2）：开始菜单扫描弹窗
  const [scan, setScan] = useState<TpScanCandidate[] | null>(null);
  const [scanPicked, setScanPicked] = useState<Set<string>>(new Set());
  const [scanBusy, setScanBusy] = useState(false);
  // 批次E（规格 5.9.1）：拖入 exe/lnk 添加（本窗口为拖放目标）

  // 已安装页：拉取各软件数据占用（卸载前展示，规格 5.6.3）
  useEffect(() => {
    if (!open || tab !== "installed") return;
    let alive = true;
    const defs = desktopIconDefs();
    void Promise.all(
      defs.map(async (d) => {
        try {
          const u = await ipc.officialUsage(d.app);
          if (alive) setUsage((prev) => ({ ...prev, [d.app]: u }));
        } catch {
          /* 查询失败 → 行内如实显示"未知"，不伪造 */
        }
      }),
    );
    return () => {
      alive = false;
    };
  }, [open, tab, uninstalled]);

  if (!open) return null;
  const close = (): void => uiStore.setState({ launcherOpen: false });

  const setTab = (tab: "third" | "installed"): void => uiStore.setState({ launcherTab: tab });

  // ---------- 第三方软件页 ----------

  const addApp = async (): Promise<void> => {
    if (busy) return;
    const picked = await openFileDialog({ multiple: false, filters: EXE_FILTERS });
    if (typeof picked !== "string") return;
    setBusy(true);
    try {
      await ipc.tpAdd(picked);
      await reloadThirdApps();
      pushToast("success", t("tpAdded"));
    } catch (e) {
      pushToast("error", t("addApp"), errMessage(e).message);
    } finally {
      setBusy(false);
    }
  };

  const rename = async (a: ThirdApp): Promise<void> => {
    const name = await askPrompt({ title: t("exRenameTitle"), initial: a.name });
    if (!name || name === a.name) return;
    try {
      await ipc.tpRename(a.id, name);
      await reloadThirdApps();
    } catch (e) {
      pushToast("error", t("exRename"), errMessage(e).message);
    }
  };

  const setGrade = async (a: ThirdApp, grade: TpGrade): Promise<void> => {
    if (grade === a.grade) return;
    try {
      await ipc.tpSetGrade(a.id, grade);
      await reloadThirdApps();
    } catch (e) {
      pushToast("error", t("launcherTitle"), errMessage(e).message);
    }
  };

  const remove = async (a: ThirdApp): Promise<void> => {
    const ok = await askConfirm({
      title: t("tpRemove"),
      body: t("tpRemoveBody", { name: a.name }),
      danger: true,
      okLabel: t("tpRemove"),
    });
    if (!ok) return;
    try {
      await ipc.tpRemove(a.id);
      await reloadThirdApps();
    } catch (e) {
      pushToast("error", t("tpRemove"), errMessage(e).message);
    }
  };

  /** 批次C（规格 5.6.2）：仅数据目录内的便携软件可彻底删除文件。 */
  const purgeFiles = async (a: ThirdApp): Promise<void> => {
    if (a.grade !== "portable") {
      pushToast("error", t("tpPurge"), t("tpPurgeOnlyPortable"));
      return;
    }
    const ok = await askConfirm({
      title: t("tpPurge"),
      body: t("tpPurgeBody", { name: a.name }),
      danger: true,
      okLabel: t("tpPurge"),
    });
    if (!ok) return;
    try {
      await ipc.tpPurge(a.id);
      await reloadThirdApps();
      pushToast("success", t("tpPurgedToast"));
    } catch (e) {
      pushToast("error", t("tpPurge"), errMessage(e).message);
    }
  };

  const gradeLabel = (g: TpGrade): string =>
    g === "portable" ? t("gradePortable") : g === "standalone" ? t("gradeStandalone") : t("gradeShortcut");

  // ---------- 批次E：开始菜单扫描 / 便携化 / 拖入添加 ----------

  const openScan = async (): Promise<void> => {
    setScanBusy(true);
    try {
      const list = await ipc.tpScanStartMenu();
      setScan(list);
      setScanPicked(new Set(list.map((c) => c.lnk)));
    } catch (e) {
      pushToast("error", t("tpScanTitle"), errMessage(e).message);
    } finally {
      setScanBusy(false);
    }
  };

  const addPicked = async (): Promise<void> => {
    if (!scan) return;
    setScanBusy(true);
    let ok = 0;
    for (const c of scan) {
      if (!scanPicked.has(c.lnk)) continue;
      try {
        await ipc.tpAdd(c.target, c.name);
        ok++;
      } catch {
        /* 单项失败不中断，结束后统一提示 */
      }
    }
    setScanBusy(false);
    setScan(null);
    await reloadThirdApps();
    if (ok > 0) pushToast("success", t("tpScanAdded", { n: ok }));
  };

  const portableize = async (a: ThirdApp): Promise<void> => {
    const ok = await askConfirm({
      title: t("tpPortTitle"),
      body: t("tpPortBody", { name: a.name }),
      okLabel: t("tpPortTitle"),
    });
    if (!ok) return;
    try {
      await ipc.tpPortableize(a.id);
      await reloadThirdApps();
      pushToast("success", t("tpPortDone", { name: a.name }));
    } catch (e) {
      pushToast("error", t("tpPortTitle"), errMessage(e).message);
    }
  };

  /** 拖入 exe/lnk 添加由桌面窗口 onDragDropEvent 统一处理（Tauri v2 事件），见 DesktopShell。 */

  // ---------- 已安装软件页（规格 5.6） ----------

  const usageText = (app: AppMode): string => {
    const u = usage[app];
    if (!u) return t("officialUsageUnknown");
    if (u.bytes === null) return t("officialUsageWs");
    return `${t("officialUsageItems", { n: u.items })} · ${formatBytes(u.bytes)}`;
  };

  const openUninstall = (app: AppMode): void => {
    setUninstallTarget(app);
    setConfirmText("");
    setConfirmErr(null);
  };

  const phraseOf = (app: AppMode): string =>
    t("officialUninstallPhrase", { name: desktopAppLabel(app) });

  const doUninstall = async (): Promise<void> => {
    const app = uninstallTarget;
    if (!app) return;
    if (confirmText.trim() !== phraseOf(app)) {
      setConfirmErr(t("officialUninstallMismatch", { phrase: phraseOf(app) }));
      return;
    }
    try {
      await closeAppWindows(app); // 规格5.6.3：自动关闭全部运行窗口
      markOfficialUninstalled(app); // 记录时间 → 24h 恢复窗口；入口即刻隐藏
      pushToast("success", t("officialUninstalledToast", { name: desktopAppLabel(app) }));
      setUninstallTarget(null);
    } catch (e) {
      pushToast("error", t("officialUninstall"), errMessage(e).message);
    }
  };

  const restore = (app: AppMode): void => {
    markOfficialInstalled(app);
    pushToast("success", t("officialRestoredToast", { name: desktopAppLabel(app) }));
  };

  const purgeOfficial = async (app: AppMode): Promise<void> => {
    const u = usage[app];
    const detail = u && u.bytes !== null ? `\n${usageText(app)}` : "";
    const ok = await askConfirm({
      title: t("officialPurgeData"),
      body: t("officialPurgeBody", { name: desktopAppLabel(app) }) + detail,
      danger: true,
      okLabel: t("officialPurgeData"),
    });
    if (!ok) return;
    try {
      await ipc.officialPurge(app);
      pushToast("success", t("officialPurgedToast"));
    } catch (e) {
      // code/fate 数据在工作区 → 后端如实报错，此处引导到正确入口
      pushToast("error", t("officialPurgeData"), errMessage(e).message);
    }
  };

  return (
    <Modal open title={t("launcherTitle")} onClose={close} width={640}>
      <div className="row gap8 tp-tabs" role="tablist">
        <button
          type="button"
          role="tab"
          aria-selected={tab === "third"}
          className={`btn tp-tab${tab === "third" ? " active" : ""}`}
          onClick={() => setTab("third")}
        >
          {t("tabThird")}
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={tab === "installed"}
          className={`btn tp-tab${tab === "installed" ? " active" : ""}`}
          onClick={() => setTab("installed")}
        >
          {t("tabInstalled")}
        </button>
      </div>

      {tab === "third" ? (
        <>
          <p className="dim small tp-hint">{t("launcherHint")}</p>
          <div className="row gap8 tp-toolbar">
            <button type="button" className="btn primary" disabled={busy} onClick={() => void addApp()}>
              <PackagePlus size={15} /> {t("addApp")}
            </button>
            <button type="button" className="btn" disabled={scanBusy} onClick={() => void openScan()}>
              <Search size={15} /> {t("tpScanTitle")}
            </button>
          </div>
          {apps.length === 0 ? (
            <p className="dim tp-empty">{t("launcherEmpty")}</p>
          ) : (
            <div className="tp-list">
              {apps.map((a) => (
                <div key={a.id} className="tp-row">
                  <div className="tp-info">
                    <button type="button" className="tp-name" title={t("exRename")} onClick={() => void rename(a)}>
                      {a.name}
                    </button>
                    <span className="tp-path dim" title={a.path}>
                      {a.path}
                    </span>
                  </div>
                  <select
                    className="text-input tp-grade"
                    value={a.grade}
                    aria-label={t("colGrade")}
                    onChange={(e) => void setGrade(a, e.target.value as TpGrade)}
                  >
                    {GRADE_OPTIONS.map((g) => (
                      <option key={g} value={g}>
                        {gradeLabel(g)}
                      </option>
                    ))}
                  </select>
                  <div className="tp-actions">
                    <button
                      type="button"
                      className="icon-btn small"
                      aria-label={t("tpLaunch")}
                      title={t("tpLaunch")}
                      onClick={() => void launchThirdApp(a.id, a.name)}
                    >
                      <Play size={14} />
                    </button>
                    {a.grade === "portable" && (
                      <button
                        type="button"
                        className="icon-btn small"
                        aria-label={t("tpPurge")}
                        title={t("tpPurge")}
                        onClick={() => void purgeFiles(a)}
                      >
                        <Trash2 size={14} />
                      </button>
                    )}
                    {a.grade !== "portable" && (
                      <button
                        type="button"
                        className="icon-btn small"
                        aria-label={t("tpPortTitle")}
                        title={t("tpPortTitle")}
                        onClick={() => void portableize(a)}
                      >
                        <PackagePlus size={14} />
                      </button>
                    )}
                    <button
                      type="button"
                      className="icon-btn small"
                      aria-label={t("tpRemove")}
                      title={t("tpRemove")}
                      onClick={() => void remove(a)}
                    >
                      <Trash2 size={14} />
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </>
      ) : (
        <div className="tp-list">
          {desktopIconDefs().map((d) => {
            const Icon = d.icon;
            const gone = uninstalled[d.app] !== undefined;
            return (
              <div key={d.id} className={`tp-row${gone ? " off" : ""}`}>
                <div className="tp-info">
                  <span className="tp-name">
                    <span className="desktop-icon-tile tp-tile" style={{ ["--hue" as string]: String(d.hue) }}>
                      <Icon size={16} strokeWidth={1.6} />
                    </span>
                    {desktopAppLabel(d.app)}
                    {gone && <span className="tp-badge dim">{t("officialUninstalledBadge")}</span>}
                  </span>
                  <span className="tp-path dim">{usageText(d.app)}</span>
                </div>
                <div className="tp-actions">
                  {gone ? (
                    <button type="button" className="btn tiny" onClick={() => restore(d.app)}>
                      <RotateCcw size={13} /> {t("officialRestore")}
                    </button>
                  ) : (
                    <button type="button" className="btn tiny" onClick={() => openUninstall(d.app)}>
                      {t("officialUninstall")}
                    </button>
                  )}
                  <button
                    type="button"
                    className="icon-btn small"
                    aria-label={t("officialPurgeData")}
                    title={t("officialPurgeData")}
                    onClick={() => void purgeOfficial(d.app)}
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
            );
          })}
          <p className="dim small tp-hint">{t("officialHint")}</p>
        </div>
      )}

      {/* 批次E（规格 5.9.2）：开始菜单扫描结果 → 勾选登记 */}
      <Modal
        open={scan !== null}
        title={t("tpScanTitle")}
        onClose={() => setScan(null)}
        width={520}
        footer={
          <>
            <button type="button" className="btn" onClick={() => setScan(null)}>
              {t("cancel")}
            </button>
            <button
              type="button"
              className="btn primary"
              disabled={scanBusy || scanPicked.size === 0}
              onClick={() => void addPicked()}
            >
              {t("tpScanAddSel", { n: scanPicked.size })}
            </button>
          </>
        }
      >
        {scan === null ? null : scan.length === 0 ? (
          <p className="dim small">{t("tpScanEmpty")}</p>
        ) : (
          <div className="tp-list">
            {scan.map((c) => (
              <label key={c.lnk} className="tp-row" style={{ cursor: "pointer" }}>
                <input
                  type="checkbox"
                  checked={scanPicked.has(c.lnk)}
                  onChange={(e) => {
                    setScanPicked((prev) => {
                      const next = new Set(prev);
                      if (e.target.checked) next.add(c.lnk);
                      else next.delete(c.lnk);
                      return next;
                    });
                  }}
                />
                <div className="tp-info">
                  <span className="tp-name">{c.name}</span>
                  <span className="tp-path dim" title={c.target}>
                    {c.target}
                  </span>
                </div>
              </label>
            ))}
          </div>
        )}
      </Modal>

      {/* 卸载确认：输入短语防误操作（规格 5.6.3） */}
      <Modal
        open={uninstallTarget !== null}
        title={uninstallTarget ? t("officialUninstallTitle", { name: desktopAppLabel(uninstallTarget) }) : ""}
        onClose={() => setUninstallTarget(null)}
        width={440}
        footer={
          <>
            <button type="button" className="btn" onClick={() => setUninstallTarget(null)}>
              {t("cancel")}
            </button>
            <button
              type="button"
              className="btn danger"
              disabled={confirmText.trim() !== (uninstallTarget ? phraseOf(uninstallTarget) : "")}
              onClick={() => void doUninstall()}
            >
              {t("officialUninstall")}
            </button>
          </>
        }
      >
        {uninstallTarget && (
          <>
            <p className="small">
              {t("officialUninstallUsage", { usage: usageText(uninstallTarget) })}
            </p>
            <p className="dim small">{t("officialUninstallBody")}</p>
            <label style={{ display: "flex", flexDirection: "column", gap: 4, marginTop: 10 }}>
              <span className="small">{t("officialUninstallType", { phrase: phraseOf(uninstallTarget) })}</span>
              <input
                className="text-input"
                value={confirmText}
                autoFocus
                onChange={(e) => {
                  setConfirmText(e.target.value);
                  setConfirmErr(null);
                }}
                onKeyDown={(e) => e.key === "Enter" && void doUninstall()}
                placeholder={phraseOf(uninstallTarget)}
              />
              {confirmErr && <span className="small" style={{ color: "var(--danger)" }}>{confirmErr}</span>}
            </label>
          </>
        )}
      </Modal>
    </Modal>
  );
}
