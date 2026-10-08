import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import type { LinEvent, LinStats } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
import { dvError } from "./DataVaultWindow";
import { fmtTime } from "./shared";

/** U-30 数据血缘：进入/使用/离开三站点事件账本（查询 / 统计 / 导出 / 焚毁）。 */
export function LineagePanel(): React.ReactElement {
  const { t } = useI18n();
  const [events, setEvents] = useState<LinEvent[]>([]);
  const [stats, setStats] = useState<LinStats | null>(null);

  const refresh = useCallback((): void => {
    if (!isTauriRuntime()) return;
    void ipc.linList(null).then(setEvents).catch(() => {});
    void ipc.linStats().then(setStats).catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const kindLabel = (k: string): string =>
    k === "import" ? t("linImport") : k === "use" ? t("linUse") : t("linDepart");

  const exportAll = async (): Promise<void> => {
    const dest = window.prompt(t("linExport"));
    if (!dest) return;
    try {
      const p = await ipc.linExport(dest);
      pushToast("success", p);
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const burn = async (): Promise<void> => {
    if (!window.confirm(t("linBurnConfirm"))) return;
    try {
      await ipc.linBurn();
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  return (
    <section>
      <h2>{t("dvTabLineage")}</h2>
      {stats && (
        <p className="dv-hint">{t("linStatsLine", { total: stats.total, paths: stats.distinctPaths })}</p>
      )}
      <div className="dv-row">
        <button className="dv-btn" onClick={() => void exportAll()}>{t("linExport")}</button>
        <button className="dv-btn danger" onClick={() => void burn()}>{t("linBurn")}</button>
      </div>
      {events.length === 0 ? (
        <div className="dv-empty">{t("linEmpty")}</div>
      ) : (
        <div className="dv-list">
          {events.slice(0, 200).map((ev) => (
            <div key={ev.id} className="dv-item">
              <span className={`dv-chip ${ev.kind === "depart" ? "warn" : ev.kind === "import" ? "ok" : ""}`}>
                {kindLabel(ev.kind)}
              </span>
              <span className="grow" title={ev.path}>{ev.display}</span>
              {ev.app && <span className="dv-chip">{ev.app}</span>}
              <span className="dv-tl-meta">{fmtTime(ev.ts)}</span>
            </div>
          ))}
        </div>
      )}
    </section>
  );
}

