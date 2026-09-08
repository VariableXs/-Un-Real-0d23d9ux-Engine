import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import type { SmartFolder } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
import { dvError } from "./DataVaultWindow";

/**
 * U-26 全局文件标签：标签云（tagAll → tagFilter 文件清单）+ 智能文件夹（保存的查询）。
 * 标签的增改在文件管理器侧（Explorer 右键"编辑标签"），此处为全局视图。
 */
export function TagsPanel(): React.ReactElement {
  const { t } = useI18n();
  const [tags, setTags] = useState<string[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [files, setFiles] = useState<string[]>([]);
  const [smart, setSmart] = useState<SmartFolder[]>([]);
  const [sfName, setSfName] = useState("");
  const [sfQuery, setSfQuery] = useState("");

  const refresh = useCallback((): void => {
    if (!isTauriRuntime()) return;
    void ipc.tagAll().then(setTags).catch(() => {});
    void ipc.tagSmartList().then(setSmart).catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const pick = async (tag: string): Promise<void> => {
    setActive(tag);
    try {
      setFiles(await ipc.tagFilter(tag));
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const addSmart = async (): Promise<void> => {
    if (!sfName.trim() || !sfQuery.trim()) return;
    try {
      await ipc.tagSmartAdd(sfName.trim(), sfQuery.trim());
      setSfName("");
      setSfQuery("");
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const removeSmart = async (id: string): Promise<void> => {
    try {
      await ipc.tagSmartRemove(id);
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  return (
    <section>
      <h2>{t("dvTabTags")}</h2>
      <h3>{t("tagAllLabel")}</h3>
      {tags.length === 0 ? (
        <div className="dv-empty">{t("tagNoFiles")}</div>
      ) : (
        <div className="dv-row">
          {tags.map((tag) => (
            <button
              key={tag}
              className={`dv-btn${active === tag ? " primary" : ""}`}
              onClick={() => void pick(tag)}
            >
              {tag}
            </button>
          ))}
        </div>
      )}

      {active && (
        <>
          <h3>{t("tagFilesOf", { tag: active })} · {t("tagCount", { n: files.length })}</h3>
          <div className="dv-list">
            {files.length === 0 ? (
              <div className="dv-empty">{t("tagNoFiles")}</div>
            ) : (
              files.map((f) => (
                <div key={f} className="dv-item"><span className="grow">{f}</span></div>
              ))
            )}
          </div>
        </>
      )}

      <h3>{t("tagSmartTitle")}</h3>
      <div className="dv-row">
        <input className="dv-input" placeholder={t("tagSmartName")} value={sfName} onChange={(e) => setSfName(e.target.value)} />
        <input className="dv-input wide" placeholder={t("tagSmartQuery")} value={sfQuery} onChange={(e) => setSfQuery(e.target.value)} />
        <button className="dv-btn primary" onClick={() => void addSmart()}>{t("tagSmartAdd")}</button>
      </div>
      {smart.length === 0 ? (
        <div className="dv-empty">{t("tagNoSmart")}</div>
      ) : (
        <div className="dv-list">
          {smart.map((s) => (
            <div key={s.id} className="dv-item">
              <span className="grow">
                <strong>{s.name}</strong> — <span className="dv-hint">{s.query}</span>
              </span>
              <button className="dv-btn danger" onClick={() => void removeSmart(s.id)}>{t("tagSmartRemove")}</button>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}
