import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import type { TrItem } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
import { dvError } from "./DataVaultWindow";
import { fmtSize } from "./shared";

/** U-28 传输指挥台：队列可视化（进度 / 暂停 / 继续 / 取消 / 重试 / 清除已完成），1s 轮询。 */
export function TransferPanel(): React.ReactElement {
  const { t } = useI18n();
  const [items, setItems] = useState<TrItem[]>([]);

  const refresh = useCallback((): void => {
    if (!isTauriRuntime()) return;
    void ipc.trList().then(setItems).catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
    const timer = window.setInterval(refresh, 1000);
    return () => window.clearInterval(timer);
  }, [refresh]);

  const act = async (fn: () => Promise<TrItem[]>): Promise<void> => {
    try {
      setItems(await fn());
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const statusChip = (s: TrItem["status"]): { cls: string; label: string } => {
    switch (s) {
      case "queued": return { cls: "", label: t("trQueued") };
      case "running": return { cls: "ok", label: t("trRunning") };
      case "paused": return { cls: "warn", label: t("trPaused") };
      case "done": return { cls: "ok", label: t("trDone") };
      case "failed": return { cls: "bad", label: t("trFailed") };
      default: return { cls: "warn", label: t("trCanceled") };
    }
  };

  return (
    <section>
      <h2>{t("dvTabTransfer")}</h2>
      {items.length === 0 ? (
        <div className="dv-empty">{t("trEmpty")}</div>
      ) : (
        <>
          <div className="dv-row">
            <button className="dv-btn" onClick={() => void act(() => ipc.trClearDone())}>
              {t("trClearDone")}
            </button>
          </div>
          <div className="dv-list">
            {items.map((it) => {
              const chip = statusChip(it.status);
              const pct = it.total > 0 ? Math.min(100, Math.round((it.bytes / it.total) * 100)) : 0;
              return (
                <div key={it.id} className="dv-item">
                  <span className={`dv-chip ${chip.cls}`}>{chip.label}</span>
                  <div className="grow">
                    <div style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                      {it.src} → {it.dest}
                    </div>
                    <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 4 }}>
                      <div className="dv-progress"><div style={{ width: `${pct}%` }} /></div>
                      <span className="dv-tl-meta">
                        {t("trFiles", { done: it.filesDone, total: it.filesTotal })} · {fmtSize(it.bytes)}/{fmtSize(it.total)}
                      </span>
                    </div>
                    {it.error && <div className="dv-tl-meta" style={{ color: "#f2a7b4" }}>{it.error}</div>}
                  </div>
                  {(it.status === "running" || it.status === "queued") && (
                    <button className="dv-btn" onClick={() => void act(() => ipc.trPause(it.id))}>{t("trPause")}</button>
                  )}
                  {it.status === "paused" && (
                    <button className="dv-btn" onClick={() => void act(() => ipc.trResume(it.id))}>{t("trResume")}</button>
                  )}
                  {(it.status === "failed" || it.status === "canceled") && (
                    <button className="dv-btn" onClick={() => void act(() => ipc.trRetry(it.id))}>{t("trRetry")}</button>
                  )}
                  {(it.status === "running" || it.status === "queued" || it.status === "paused") && (
                    <button className="dv-btn danger" onClick={() => void act(() => ipc.trCancel(it.id))}>{t("trCancel")}</button>
                  )}
                </div>
              );
            })}
          </div>
        </>
      )}
    </section>
  );
}
