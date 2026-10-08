import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { errMessage, ipc } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { formatBytes } from "../../lib/format";
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
  /** B-26：克隆试验档标记 */
  isClone?: boolean;
}

interface DiffEntry {
  path: string;
  /** added | changed | deleted | conflict */
  status: string;
}

interface DiffReport {
  cloneId: string;
  parentId: string;
  clean: boolean;
  entries: DiffEntry[];
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
  // B-26：当前展开 diff 的试验档 + 冲突裁决勾选（勾 = 采用试验档版本）
  const [diffFor, setDiffFor] = useState<DiffReport | null>(null);
  const [keepClone, setKeepClone] = useState<string[]>([]);

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

  // ---- B-26：试验档 diff / 丢弃 / 合并 ----

  const openDiff = (e: EnvView) =>
    void withBusy(async () => {
      try {
        const rep = await ipc.envDiff(e.id);
        setDiffFor(rep);
        setKeepClone([]);
      } catch (err) {
        pushToast("error", t("evDiffFail"), errMessage(err).message);
      }
    });

  const discard = (e: EnvView) =>
    void withBusy(async () => {
      const ok = await askConfirm({
        title: t("evDiscardTitle"),
        body: `${e.name} · ${t("evDiscardWarn")}`,
        danger: true,
        okLabel: t("evDiscardOk"),
      });
      if (!ok) return;
      try {
        await ipc.envDiscard(e.id);
        setDiffFor(null);
        await refresh();
        pushToast("success", t("evDiscarded"), e.name);
      } catch (err) {
        pushToast("error", t("evDiscardFail"), errMessage(err).message);
      }
    });

  const merge = (e: EnvView) =>
    void withBusy(async () => {
      try {
        const r = await ipc.envMerge(e.id, keepClone);
        setDiffFor(null);
        await refresh();
        pushToast(
          "success",
          t("evMerged"),
          `${r.merged} · ${t("evMergeResolved")} ${r.conflictsResolved} · ${t("evMergeKeptMain")} ${r.conflictsKeptMain}`,
        );
      } catch (err) {
        pushToast("error", t("evMergeFail"), errMessage(err).message);
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
              <button
                type="button"
                onClick={() =>
                  void withBusy(async () => {
                    try {
                      const r = await ipc.envClone(e.id, `${e.name} 2`);
                      await refresh();
                      pushToast("success", t("evCloned"), `${r.cloneName || r.cloneId} · ${formatBytes(r.bytesCopied)}`);
                    } catch (err) {
                      pushToast("error", t("evCloneFail"), errMessage(err).message);
                    }
                  })
                }
              >
                {t("evClone")}
              </button>
              <button
                type="button"
                onClick={() =>
                  void withBusy(async () => {
                    try {
                      await ipc.envNested(e.id);
                      pushToast("success", t("evNestedOk"), e.name);
                    } catch (err) {
                      pushToast("error", t("evNestedFail"), errMessage(err).message);
                    }
                  })
                }
                title={t("evNestedTip")}
              >
                {t("evNested")}
              </button>
              {e.id !== "main" && (
                <button type="button" className="danger" onClick={() => remove(e)}>
                  {t("evDelete")}
                </button>
              )}
              {e.isClone && (
                <button type="button" onClick={() => openDiff(e)}>
                  {t("evDiff")}
                </button>
              )}
              {e.isClone && (
                <button type="button" className="danger" onClick={() => discard(e)} title={t("evDiscardWarn")}>
                  {t("evDiscard")}
                </button>
              )}
            </span>
          </div>
        ))}
      </div>
      {/* B-26：试验档 diff 报告 + 冲突裁决（勾选 = 采用试验档版本） */}
      {diffFor && (
        <div className="ev-diff">
          <p className="small">
            <strong>{t("evDiffTitle")}</strong>{" "}
            <span className="dim">{envs.find((x) => x.id === diffFor.cloneId)?.name ?? diffFor.cloneId}</span>
          </p>
          {diffFor.entries.length === 0 ? (
            <p className="dim small">{t("evDiffClean")}</p>
          ) : (
            <div className="ev-diff-list">
              {diffFor.entries.map((d) => (
                <div key={d.path} className="backup-row" style={{ gap: 6 }}>
                  <span className={`dim small ev-status-${d.status}`}>{d.status}</span>
                  <span className="ellipsis small" title={d.path}>{d.path}</span>
                  {d.status === "conflict" && (
                    <label className="small" style={{ display: "flex", alignItems: "center", gap: 4 }}>
                      <input
                        type="checkbox"
                        checked={keepClone.includes(d.path)}
                        onChange={(ev) =>
                          setKeepClone((cur) =>
                            ev.target.checked ? [...cur, d.path] : cur.filter((p) => p !== d.path),
                          )
                        }
                      />
                      {t("evUseClone")}
                    </label>
                  )}
                </div>
              ))}
            </div>
          )}
          <div style={{ display: "flex", gap: 8, justifyContent: "flex-end", marginTop: 8 }}>
            <button type="button" onClick={() => setDiffFor(null)}>{t("evDiffClose")}</button>
            <button
              type="button"
              disabled={diffFor.entries.length === 0}
              onClick={() => {
                const e = envs.find((x) => x.id === diffFor.cloneId);
                if (e) void merge(e);
              }}
            >
              {t("evMerge")}
            </button>
          </div>
        </div>
      )}
      <p className="dim small">{t("evIsolationNote")}</p>
    </div>
  );
}
