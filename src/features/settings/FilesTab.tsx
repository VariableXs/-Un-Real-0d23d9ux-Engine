import { useCallback, useEffect, useState } from "react";
import { ArrowDown, ArrowUp, Plus, RotateCcw, Trash2 } from "lucide-react";
import { useI18n } from "../../i18n";
import { errMessage, ipc, type FileAssoc, type SentinelCfg } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { getThirdApps } from "../../system/launcher/thirdApps";
import {
  CTX_ITEMS,
  loadCtxConfig,
  loadDeleteTier,
  resetCtxConfig,
  saveCtxConfig,
  saveDeleteTier,
  type CtxConfig,
  type DeleteTier,
} from "../../system/explorer/ctxMenu";

/**
 * AI-09 设置页「文件管理器」tab：
 * - M-19 右键菜单自定义编辑器（显隐 / 排序 / 恢复默认；仅注册表内安全动作）
 * - M-26 删除档位（默认 = 现状：环境回收站）
 * - Z-30 打开方式管理面板（按扩展名查看/修改/移除默认应用）
 * - M-27 目录监控哨兵管理（增删 / 开关 / 免打扰期）
 */
export function FilesTab(): React.ReactElement {
  const { t } = useI18n();
  const [ctx, setCtx] = useState<CtxConfig>(() => loadCtxConfig());
  const [tier, setTier] = useState<DeleteTier>(() => loadDeleteTier());
  const [assocs, setAssocs] = useState<FileAssoc[]>([]);
  const [sentinels, setSentinels] = useState<SentinelCfg[]>([]);
  const [newExt, setNewExt] = useState("");
  const [newApp, setNewApp] = useState("");
  const [sentPath, setSentPath] = useState("");

  const refreshAssocs = useCallback(() => {
    void ipc
      .fileAssocList()
      .then(setAssocs)
      .catch(() => {});
  }, []);
  const refreshSentinels = useCallback(() => {
    void ipc
      .sentinelList()
      .then(setSentinels)
      .catch(() => {});
  }, []);

  useEffect(() => {
    refreshAssocs();
    refreshSentinels();
  }, [refreshAssocs, refreshSentinels]);

  const moveItem = (idx: number, dir: -1 | 1): void => {
    setCtx((c) => {
      const order = [...c.order];
      const j = idx + dir;
      if (j < 0 || j >= order.length) return c;
      [order[idx], order[j]] = [order[j]!, order[idx]!];
      const next = { ...c, order };
      saveCtxConfig(next);
      return next;
    });
  };

  const toggleHidden = (id: (typeof ctx.order)[number]): void => {
    setCtx((c) => {
      const hidden = c.hidden.includes(id) ? c.hidden.filter((x) => x !== id) : [...c.hidden, id];
      const next = { ...c, hidden };
      saveCtxConfig(next);
      return next;
    });
  };

  const resetCtx = (): void => {
    setCtx(resetCtxConfig());
    pushToast("success", t("fsCtxReset"), t("fsCtxResetDone"));
  };

  const chooseTier = (v: DeleteTier): void => {
    setTier(v);
    saveDeleteTier(v);
  };

  const addAssoc = (): void => {
    const ext = newExt.trim().replace(/^\./, "").toLowerCase();
    if (!ext || !newApp) return;
    const app = getThirdApps().find((a) => a.id === newApp);
    void ipc
      .fileAssocSet(ext, newApp, app?.name ?? newApp)
      .then(() => {
        setNewExt("");
        refreshAssocs();
        pushToast("success", t("fsOpenWithTitle"), `.${ext} → ${app?.name ?? newApp}`);
      })
      .catch((e: unknown) => pushToast("error", t("fsOpenWithTitle"), errMessage(e).message));
  };

  const removeAssoc = (ext: string): void => {
    void ipc
      .fileAssocRemove(ext)
      .then(refreshAssocs)
      .catch((e: unknown) => pushToast("error", t("fsOpenWithTitle"), errMessage(e).message));
  };

  const addSentinel = (): void => {
    const p = sentPath.trim();
    if (!p) return;
    void ipc
      .sentinelAdd(p)
      .then((list) => {
        setSentinels(list);
        setSentPath("");
      })
      .catch((e: unknown) => pushToast("error", t("fsSentinelTitle"), errMessage(e).message));
  };

  const mutateSentinel = (fn: (id: string) => Promise<SentinelCfg[]>) => (id: string): void => {
    void fn(id)
      .then(setSentinels)
      .catch((e: unknown) => pushToast("error", t("fsSentinelTitle"), errMessage(e).message));
  };

  return (
    <div className="files-tab">
      {/* ---- M-19 右键菜单编辑器 ---- */}
      <h3>{t("fsCtxTitle")}</h3>
      <p className="dim small">{t("fsCtxHint")}</p>
      <div className="sec-actions">
        <button type="button" className="btn ghost tiny" onClick={resetCtx}>
          <RotateCcw size={12} /> {t("fsCtxReset")}
        </button>
      </div>
      <ul className="fs-ctx-list">
        {ctx.order.map((id, i) => {
          const def = CTX_ITEMS[id];
          const hidden = ctx.hidden.includes(id);
          return (
            <li key={id} className={hidden ? "fs-ctx-off" : ""}>
              <label>
                <input type="checkbox" checked={!hidden} onChange={() => toggleHidden(id)} />
                <span>{t(def.labelKey)}</span>
                <span className="dim small">· {t(def.scope === "file" ? "fsScopeFile" : def.scope === "dir" ? "fsScopeDir" : "fsScopeAll")}</span>
              </label>
              <span className="fs-ctx-ops">
                <button type="button" className="icon-btn tiny" disabled={i === 0} onClick={() => moveItem(i, -1)} aria-label={t("fsMoveUp")}>
                  <ArrowUp size={12} />
                </button>
                <button type="button" className="icon-btn tiny" disabled={i === ctx.order.length - 1} onClick={() => moveItem(i, 1)} aria-label={t("fsMoveDown")}>
                  <ArrowDown size={12} />
                </button>
              </span>
            </li>
          );
        })}
      </ul>

      {/* ---- M-26 删除档位 ---- */}
      <h3>{t("fsTierTitle")}</h3>
      <p className="dim small">{t("fsTierHint")}</p>
      <div className="fs-tier-row" role="radiogroup" aria-label={t("fsTierTitle")}>
        <label>
          <input type="radio" name="delete-tier" checked={tier === "variable"} onChange={() => chooseTier("variable")} />
          {t("fsTierVariable")}
        </label>
        <label>
          <input type="radio" name="delete-tier" checked={tier === "ask"} onChange={() => chooseTier("ask")} />
          {t("fsTierAsk")}
        </label>
      </div>

      {/* ---- Z-30 打开方式管理 ---- */}
      <h3>{t("fsOpenWithTitle")}</h3>
      <p className="dim small">{t("fsOpenWithHint")}</p>
      <div className="fs-assoc-add">
        <input
          className="text-input tiny"
          type="text"
          placeholder="txt"
          value={newExt}
          onChange={(e) => setNewExt(e.target.value)}
          aria-label={t("fsOpenWithExt")}
        />
        <select
          className="ex-sort-select"
          value={newApp}
          onChange={(e) => setNewApp(e.target.value)}
          aria-label={t("fsOpenWithApp")}
        >
          <option value="">{t("fsOpenWithApp")}</option>
          {getThirdApps().map((a) => (
            <option key={a.id} value={a.id}>
              {a.name}
            </option>
          ))}
        </select>
        <button type="button" className="btn tiny" onClick={addAssoc} disabled={!newExt.trim() || !newApp}>
          <Plus size={12} /> {t("fsOpenWithAdd")}
        </button>
      </div>
      {assocs.length === 0 ? (
        <p className="dim small">{t("fsOpenWithEmpty")}</p>
      ) : (
        <ul className="fs-assoc-list">
          {assocs.map((a) => (
            <li key={a.ext}>
              <span className="fs-assoc-ext">.{a.ext}</span>
              <span className="fs-assoc-app">{a.appName}</span>
              <button type="button" className="icon-btn tiny danger-hover" onClick={() => removeAssoc(a.ext)} aria-label={t("fsOpenWithRemove")}>
                <Trash2 size={12} />
              </button>
            </li>
          ))}
        </ul>
      )}

      {/* ---- M-27 目录监控哨兵 ---- */}
      <h3>{t("fsSentinelTitle")}</h3>
      <p className="dim small">{t("fsSentinelHint")}</p>
      <div className="fs-assoc-add">
        <input
          className="text-input tiny"
          type="text"
          placeholder="D:\\Downloads"
          value={sentPath}
          onChange={(e) => setSentPath(e.target.value)}
          aria-label={t("fsPath")}
        />
        <button type="button" className="btn tiny" onClick={addSentinel} disabled={!sentPath.trim()}>
          <Plus size={12} /> {t("fsSentinelAdd")}
        </button>
      </div>
      {sentinels.length === 0 ? (
        <p className="dim small">{t("fsSentinelEmpty")}</p>
      ) : (
        <ul className="fs-assoc-list">
          {sentinels.map((s) => (
            <li key={s.id}>
              <label className="fs-sentinel-toggle">
                <input
                  type="checkbox"
                  checked={s.enabled}
                  onChange={() => mutateSentinel((id) => ipc.sentinelToggle(id, !s.enabled))(s.id)}
                />
                <span className="fs-assoc-app" title={s.path}>{s.path}</span>
              </label>
              <button type="button" className="icon-btn tiny danger-hover" onClick={() => mutateSentinel((id) => ipc.sentinelRemove(id))(s.id)} aria-label={t("fsOpenWithRemove")}>
                <Trash2 size={12} />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
