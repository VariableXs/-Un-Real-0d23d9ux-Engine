import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import type { InsDashboard, InsSuggestion } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
import { dvError } from "./DataVaultWindow";
import { fmtDur } from "./shared";

/** N-31 使用洞察：应用时长 / 专注 / 通知 / 搜索的本地聚合仪表盘 + 改进行动建议。 */
export function InsightsPanel(): React.ReactElement {
  const { t } = useI18n();
  const [range, setRange] = useState<"today" | "week">("today");
  const [dash, setDash] = useState<InsDashboard | null>(null);
  const [sugg, setSugg] = useState<InsSuggestion[]>([]);

  const refresh = useCallback((r: "today" | "week"): void => {
    if (!isTauriRuntime()) return;
    void ipc.insDashboard(r).then(setDash).catch(() => {});
    void ipc.insSuggestions().then(setSugg).catch(() => {});
  }, []);

  useEffect(() => {
    refresh(range);
  }, [range, refresh]);

  const burn = async (): Promise<void> => {
    if (!window.confirm(t("insBurn") + "?")) return;
    try {
      await ipc.insBurn();
      refresh(range);
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const top = (m: Record<string, number> | undefined): [string, number][] =>
    Object.entries(m ?? {}).sort((a, b) => b[1] - a[1]).slice(0, 3);

  const empty = !dash || dash.events === 0;

  return (
    <section>
      <h2>{t("dvTabInsights")}</h2>
      <div className="dv-row">
        <button className={`dv-btn${range === "today" ? " primary" : ""}`} onClick={() => setRange("today")}>{t("insToday")}</button>
        <button className={`dv-btn${range === "week" ? " primary" : ""}`} onClick={() => setRange("week")}>{t("insWeek")}</button>
        <div style={{ flex: 1 }} />
        <button className="dv-btn danger" onClick={() => void burn()}>{t("insBurn")}</button>
      </div>

      {dash && !dash.recording && <div className="dv-banner alert">{t("insPaused")}</div>}

      {empty ? (
        <div className="dv-empty">{t("insEmpty")}</div>
      ) : dash && (
        <>
          <h3>{t("insAppsTitle")}</h3>
          <div className="dv-list">
            {dash.apps.slice(0, 10).map((a) => (
              <div key={a.app} className="dv-item">
                <span className="grow">{a.app}</span>
                <span className="dv-tl-meta">{fmtDur(a.ms)} · ×{a.switches}</span>
                <div className="dv-progress" style={{ maxWidth: 160 }}>
                  <div style={{ width: `${Math.max(4, (a.ms / (dash.apps[0]?.ms || 1)) * 100)}%` }} />
                </div>
              </div>
            ))}
          </div>

          <h3>{t("insFocusTitle")}</h3>
          <p className="dv-hint">{t("insLongestStreak", { n: dash.focus.longestStreakMin })}</p>
          {top(dash.focus.interruptTop).map(([app, n]) => (
            <p key={app} className="dv-hint">{t("insInterruptTop", { app, n })}</p>
          ))}

          <h3>{t("insNotifTitle")}</h3>
          {top(dash.notif.bySource).map(([src, n]) => (
            <p key={src} className="dv-hint">{src} ×{n}</p>
          ))}
          {dash.notif.dndCount > 0 && <p className="dv-hint">{t("insNotifDnd", { n: dash.notif.dndCount })}</p>}

          <h3>{t("insSearchTitle")}</h3>
          <p className="dv-hint">{t("insSearchHit", { hits: dash.search.hits, total: dash.search.total })}</p>
          {top(dash.search.missTop).map(([q, n]) => (
            <p key={q} className="dv-hint">{t("insMissTop", { q, n })}</p>
          ))}

          {sugg.length > 0 && (
            <>
              <h3>{t("insSuggestions")}</h3>
              <div className="dv-list">
                {sugg.map((sg) => (
                  <div key={sg.id} className="dv-item">
                    <span className="dv-chip">{sg.rule}</span>
                    <span className="grow">{sg.target} ×{sg.hits}</span>
                  </div>
                ))}
              </div>
            </>
          )}
        </>
      )}
    </section>
  );
}
