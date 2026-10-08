import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import type { VerDiff, VerInfo } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
import { dvError } from "./DataVaultWindow";
import { fmtSize, fmtTime } from "./shared";

/**
 * U-25 版本时光机：登记 → 快照 → 时间线（还原 / 双栏 diff / 保留策略 / GC）。
 * 隐身会话期间后端拒绝快照（返回 None），面板如实显示会话暂停提示。
 */
export function VersionsPanel(): React.ReactElement {
  const { t } = useI18n();
  const [path, setPath] = useState("");
  const [watched, setWatched] = useState<string[]>([]);
  const [versions, setVersions] = useState<VerInfo[]>([]);
  const [selected, setSelected] = useState<string>("");
  const [diff, setDiff] = useState<VerDiff | null>(null);
  const [busy, setBusy] = useState(false);
  const [keepVersions, setKeepVersions] = useState(0);
  const [keepDays, setKeepDays] = useState(0);

  const refreshWatched = useCallback((): void => {
    if (!isTauriRuntime()) return;
    void ipc.verWatchedList().then(setWatched).catch(() => {});
  }, []);

  const refreshVersions = useCallback((p: string): void => {
    if (!isTauriRuntime() || !p) {
      setVersions([]);
      return;
    }
    void ipc.verList(p).then((list) => {
      setVersions(list);
      const first = list[0];
      if (first && !list.some((v) => v.id === selected)) {
        setSelected(first.id);
      }
    }).catch((e) => pushToast("error", dvError(e)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    refreshWatched();
  }, [refreshWatched]);

  useEffect(() => {
    if (!path) return;
    const timer = window.setTimeout(() => refreshVersions(path), 250);
    return () => window.clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [path]);

  const watch = async (): Promise<void> => {
    if (!path) return;
    setBusy(true);
    try {
      await ipc.verWatch(path);
      pushToast("success", t("exVersionWatched"));
      refreshWatched();
      refreshVersions(path);
    } catch (e) {
      pushToast("error", dvError(e));
    } finally {
      setBusy(false);
    }
  };

  const snapshot = async (): Promise<void> => {
    if (!path) return;
    setBusy(true);
    try {
      const v = await ipc.verSnapshot(path);
      if (!v) {
        pushToast("info", t("incActiveLabel"));
      }
      refreshVersions(path);
    } catch (e) {
      pushToast("error", dvError(e));
    } finally {
      setBusy(false);
    }
  };

  const restore = async (versionId: string): Promise<void> => {
    try {
      await ipc.verRestore(path, versionId);
      pushToast("success", t("verRestore"));
      refreshVersions(path);
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const doDiff = async (): Promise<void> => {
    if (versions.length < 2) return;
    const idx = versions.findIndex((v) => v.id === selected);
    const old = versions[Math.min(idx + 1, versions.length - 1)];
    const cur = versions[Math.max(idx, 0)];
    if (!old || !cur || old.id === cur.id) {
      setDiff(null);
      return;
    }
    try {
      setDiff(await ipc.verDiff(path, old.id, cur.id));
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const gc = async (): Promise<void> => {
    setBusy(true);
    try {
      const r = await ipc.verGc();
      pushToast("success", t("verGcDone", { v: r.removedVersions, b: r.removedBlocks }));
    } catch (e) {
      pushToast("error", dvError(e));
    } finally {
      setBusy(false);
    }
  };

  const savePolicy = async (): Promise<void> => {
    try {
      await ipc.verPolicySet(keepVersions || null, keepDays || null);
      pushToast("success", t("rcpSaved"));
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const watchedHere = watched.length > 0 && path && watched.includes(path.replace(/\\/g, "/").toLowerCase());

  return (
    <section>
      <h2>{t("dvTabVersions")}</h2>
      <p className="dv-hint">{t("verEmptyFile")}</p>
      <div className="dv-row">
        <input
          className="dv-input wide"
          value={path}
          placeholder={t("verFileLabel")}
          onChange={(e) => setPath(e.target.value)}
        />
        <button className="dv-btn" disabled={!path || busy} onClick={() => void watch()}>
          {watchedHere ? t("verWatched") : t("verWatch")}
        </button>
        <button className="dv-btn primary" disabled={!path || busy} onClick={() => void snapshot()}>
          {t("verSnapshot")}
        </button>
      </div>

      <h3>{t("verTimeline")}</h3>
      {versions.length === 0 ? (
        <div className="dv-empty">{t("verEmpty")}</div>
      ) : (
        <div className="dv-timeline">
          {versions.map((v) => (
            <div key={v.id} className="dv-tl-item" style={{ cursor: "pointer" }} onClick={() => setSelected(v.id)}>
              <div className="dv-tl-head">
                <strong>{fmtTime(v.savedAt)}</strong>
                {v.changedLines != null && v.changedLines > 0 && (
                  <span className="dv-chip">{t("verChanged", { n: v.changedLines })}</span>
                )}
                <span className="dv-tl-meta">{fmtSize(v.size)} · {v.hash.slice(0, 8)}</span>
                <button className="dv-btn" onClick={(e) => { e.stopPropagation(); void restore(v.id); }}>
                  {t("verRestore")}
                </button>
              </div>
            </div>
          ))}
          <div className="dv-row">
            <button className="dv-btn" disabled={versions.length < 2} onClick={() => void doDiff()}>
              {t("verDiffPrev")}
            </button>
          </div>
        </div>
      )}

      {diff && (
        <div className="dv-diff">
          <div className="dv-diff-col old">
            {diff.hunks.filter((l) => l.kind !== "added").map((l, i) => (
              <div key={i} className={l.kind === "deleted" ? "dv-diff-line-del" : undefined}>{l.text || " "}</div>
            ))}
          </div>
          <div className="dv-diff-col">
            {diff.hunks.filter((l) => l.kind !== "deleted").map((l, i) => (
              <div key={i} className={l.kind === "added" ? "dv-diff-line-add" : undefined}>{l.text || " "}</div>
            ))}
          </div>
        </div>
      )}

      <h3>{t("verPolicy")}</h3>
      <div className="dv-row">
        <label className="dv-hint">{t("verKeepVersions")}</label>
        <input className="dv-input" type="number" min={0} value={keepVersions}
          onChange={(e) => setKeepVersions(Math.max(0, Number(e.target.value) || 0))} />
        <label className="dv-hint">{t("verKeepDays")}</label>
        <input className="dv-input" type="number" min={0} value={keepDays}
          onChange={(e) => setKeepDays(Math.max(0, Number(e.target.value) || 0))} />
        <button className="dv-btn" onClick={() => void savePolicy()}>{t("rcpSaved")}</button>
        <button className="dv-btn danger" disabled={busy} onClick={() => void gc()}>{t("verGc")}</button>
      </div>
    </section>
  );
}

