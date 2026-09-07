import { useEffect, useState } from "react";
import {
  listSnapshots,
  saveSnapshot,
  deleteSnapshot,
  renameSnapshot,
  restoreSnapshot,
  exportSnapshot,
  importSnapshot,
  type VwmSnapshot,
} from "../../system/windows/snapshots";
import { tabsEnabled, setTabsEnabled } from "../../system/windows/vwm";
import { useI18n } from "../../i18n";
import { isTauriRuntime } from "../../entries/runtime";
import { ipc } from "../../lib/ipc";

/**
 * 批次W-4：布局快照管理 UI（设置→外观→布局快照）。
 * 列表 / 保存当前布局 / 重命名 / 删除 / 导出导入（JSON）/ 恢复（含缺失项提示）。
 */
export function SnapshotManager(): React.ReactElement {
  const { t } = useI18n();
  const [items, setItems] = useState<VwmSnapshot[]>(() => listSnapshots().filter((s) => !s.name.startsWith("__auto:")));
  const [msg, setMsg] = useState<string>("");

  const refresh = (): void => setItems(listSnapshots().filter((s) => !s.name.startsWith("__auto:")));

  const onSave = (): void => {
    const name = window.prompt(t("snapNamePrompt"), "") ?? "";
    if (!name.trim()) return;
    saveSnapshot(name);
    refresh();
    setMsg(t("snapSaved"));
  };

  const onRestore = (name: string): void => {
    const r = restoreSnapshot(name);
    setMsg(
      r.missing.length > 0
        ? `${t("snapRestored", { n: r.restored })} · ${t("snapMissing")}：${r.missing.join("、")}`
        : t("snapRestored", { n: r.restored }),
    );
  };

  const onRename = (oldName: string): void => {
    const name = window.prompt(t("snapRenamePrompt"), oldName) ?? "";
    if (!name.trim() || name === oldName) return;
    setMsg(renameSnapshot(oldName, name) ? t("snapRenamed") : t("snapDupName"));
    refresh();
  };

  const onDelete = (name: string): void => {
    if (!window.confirm(t("snapDeleteConfirm", { name }))) return;
    deleteSnapshot(name);
    refresh();
    setMsg(t("snapDeleted"));
  };

  const onExport = (name: string): void => {
    const json = exportSnapshot(name);
    if (!json) return;
    const blob = new Blob([json], { type: "application/json" });
    const a = document.createElement("a");
    a.href = URL.createObjectURL(blob);
    a.download = `vwm-snapshot-${name}.json`;
    a.click();
    URL.revokeObjectURL(a.href);
  };

  const onImport = async (): Promise<void> => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".json,application/json";
    input.onchange = (): void => {
      const f = input.files?.[0];
      if (!f) return;
      void f.text().then((txt) => {
        const r = importSnapshot(txt);
        setMsg(r.ok ? t("snapImported") : `${t("snapImportFail")}: ${r.error ?? ""}`);
        refresh();
      });
    };
    input.click();
  };

  return (
    <section className="vwm-snapshots">
      <h3>{t("snapTitle")}</h3>
      <div className="row">
        <button type="button" onClick={onSave}>
          {t("snapSaveNow")}
        </button>
        <button type="button" onClick={() => void onImport()}>
          {t("snapImport")}
        </button>
      </div>
      {items.length === 0 ? (
        <p className="dim small">{t("snapEmpty")}</p>
      ) : (
        <ul>
          {items.map((s) => (
            <li key={s.name} className="row">
              <span className="snap-name">
                {s.name === "__autosave__" ? t("snapAutosave") : s.name}
                <span className="dim small"> · {s.windows.length}w · {new Date(s.created).toLocaleString()}</span>
              </span>
              <button type="button" onClick={() => onRestore(s.name)}>
                {t("snapRestore")}
              </button>
              <button type="button" onClick={() => onRename(s.name)}>
                {t("snapRename")}
              </button>
              <button type="button" onClick={() => onExport(s.name)}>
                {t("snapExport")}
              </button>
              <button type="button" onClick={() => onDelete(s.name)}>
                {t("snapDelete")}
              </button>
            </li>
          ))}
        </ul>
      )}
      {msg && <p className="small snap-msg">{msg}</p>}
      <p className="dim small">{t("snapHint")}</p>
    </section>
  );
}

/** 批次W-5：标签页化开关（可选开启；拖同应用窗口到另一窗口标题栏合并为标签组）。 */
export function VwmTabsToggle(): React.ReactElement {
  const { t } = useI18n();
  const [on, setOn] = useState<boolean>(() => tabsEnabled());
  return (
    <section className="vwm-tabs-toggle">
      <h3>{t("tabsTitle")}</h3>
      <label className="row">
        <input
          type="checkbox"
          checked={on}
          onChange={(e) => {
            setOn(e.target.checked);
            setTabsEnabled(e.target.checked);
          }}
        />
        <span>{t("tabsEnable")}</span>
      </label>
      <p className="dim small">{t("tabsHint")}</p>
    </section>
  );
}

/** D-3：全域软件接管看门狗开关与处置策略（关闭 = 回滚「仅手动启动才嵌入」）。 */
export function WatchdogToggle(): React.ReactElement {
  const { t } = useI18n();
  const [on, setOn] = useState<boolean>(true);
  const [policy, setPolicy] = useState<string>("ask");
  useEffect(() => {
    if (!isTauriRuntime()) return;
    void ipc
      .watchGetSettings()
      .then((s) => {
        setOn(s.enabled);
        setPolicy(s.policy);
      })
      .catch(() => {});
  }, []);
  const apply = (enabled: boolean, pol: string): void => {
    setOn(enabled);
    setPolicy(pol);
    if (isTauriRuntime()) void ipc.watchSetSettings(enabled, pol).catch(() => {});
  };
  return (
    <section className="vwm-tabs-toggle">
      <h3>{t("watchTitle")}</h3>
      <label className="row">
        <input
          type="checkbox"
          checked={on}
          onChange={(e) => apply(e.target.checked, policy)}
        />
        <span>{t("watchEnable")}</span>
      </label>
      <label className="row" style={{ marginTop: 8 }}>
        <span className="dim small">{t("watchPolicy")}</span>
        <select value={policy} onChange={(e) => apply(on, e.target.value)}>
          <option value="ask">{t("watchPolicyAsk")}</option>
          <option value="auto">{t("watchPolicyAuto")}</option>
        </select>
      </label>
      <p className="dim small">{t("watchHint")}</p>
    </section>
  );
}
