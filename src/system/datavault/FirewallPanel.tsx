import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import type { FwAlertCenter, FwProfile } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
import { dvError } from "./DataVaultWindow";
import { fmtSize, fmtTime } from "./shared";

/**
 * U-32 应用防火墙 2.0：应用级网络策略档案（blocked 默认 / whitelist / full）、
 * 出站告警中心（一键放行或保持拦截、高频 ≥10 标红）、单应用日流量配额
 * （超出后端自动降级禁网）。与 netconsent 域名库互补，不重复造域名库。
 */
type FwLevel = "blocked" | "whitelist" | "full";

export function FirewallPanel(): React.ReactElement {
  const { t } = useI18n();
  const [profiles, setProfiles] = useState<FwProfile[]>([]);
  const [center, setCenter] = useState<FwAlertCenter | null>(null);
  const [app, setApp] = useState("");
  const [whitelist, setWhitelist] = useState("");
  const [busy, setBusy] = useState(false);

  const refresh = useCallback((): void => {
    if (!isTauriRuntime()) return;
    void ipc.fwProfiles().then(setProfiles).catch(() => {});
    void ipc.fwAlerts().then(setCenter).catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const setLevel = async (p: FwProfile, level: FwLevel): Promise<void> => {
    try {
      await ipc.fwProfileSet({ ...p, level });
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const saveWhitelist = async (p: FwProfile): Promise<void> => {
    const hosts = whitelist
      .split(/[\n,;]+/)
      .map((s) => s.trim())
      .filter(Boolean);
    try {
      await ipc.fwProfileSet({ ...p, level: "whitelist", whitelist: hosts });
      pushToast("success", t("fwSaved"));
      setWhitelist("");
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const setQuotaOf = async (p: FwProfile, bytes: number): Promise<void> => {
    try {
      await ipc.fwProfileSet({ ...p, dailyQuotaBytes: Math.max(0, bytes) });
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const addProfile = async (): Promise<void> => {
    const name = app.trim();
    if (!name) return;
    setBusy(true);
    try {
      await ipc.fwProfileSet({ app: name, level: "blocked", whitelist: [], dailyQuotaBytes: 0 });
      setApp("");
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    } finally {
      setBusy(false);
    }
  };

  const resolve = async (id: string, allow: boolean): Promise<void> => {
    try {
      await ipc.fwAlertResolve(id, allow);
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const levelChip = (level: string): { cls: string; label: string } => {
    if (level === "full") return { cls: "warn", label: t("fwLevelFull") };
    if (level === "whitelist") return { cls: "", label: t("fwLevelWhitelist") };
    return { cls: "ok", label: t("fwLevelBlocked") };
  };

  return (
    <section>
      <h2>{t("fwTitle")}</h2>
      <p className="dv-hint">{t("fwHint")}</p>

      <div className="dv-row">
        <input className="dv-input" placeholder={t("fwAppLabel")}
          value={app} onChange={(e) => setApp(e.target.value)} />
        <button className="dv-btn primary" disabled={busy || !app.trim()} onClick={() => void addProfile()}>
          {t("fwAddProfile")}
        </button>
      </div>

      {profiles.length === 0 ? (
        <div className="dv-empty">{t("fwEmpty")}</div>
      ) : (
        <div className="dv-list">
          {profiles.map((p) => {
            const chip = levelChip(p.level);
            return (
              <div key={p.app} className="dv-item" style={{ flexWrap: "wrap", gap: 6 }}>
                <span className="grow" style={{ minWidth: 120 }}>{p.app}</span>
                <span className={`dv-chip ${chip.cls}`}>{chip.label}</span>
                <select className="dv-select" value={p.level} onChange={(e) => void setLevel(p, e.target.value as FwLevel)}>
                  <option value="blocked">{t("fwLevelBlocked")}</option>
                  <option value="whitelist">{t("fwLevelWhitelist")}</option>
                  <option value="full">{t("fwLevelFull")}</option>
                </select>
                <span className="dv-tl-meta" title={t("fwQuotaHint")}>
                  {t("fwQuota")}: {p.dailyQuotaBytes > 0 ? fmtSize(p.dailyQuotaBytes) : t("fwQuotaNone")}
                </span>
                <input className="dv-input" type="number" min={0} style={{ width: 110 }}
                  defaultValue={p.dailyQuotaBytes}
                  onBlur={(e) => {
                    const mb = Math.max(0, Number(e.target.value) || 0);
                    if (mb !== p.dailyQuotaBytes) void setQuotaOf(p, mb);
                  }}
                  title={t("fwQuotaHint")} />
                {p.whitelist.length > 0 && (
                  <span className="dv-chip" title={p.whitelist.join(", ")}>
                    {t("fwWhitelistN", { n: p.whitelist.length })}
                  </span>
                )}
              </div>
            );
          })}
        </div>
      )}

      {profiles.length > 0 && (
        <>
          <h3>{t("fwWhitelistTitle")}</h3>
          <div className="dv-row">
            <select className="dv-select" onChange={(e) => {
              const p = profiles.find((q) => q.app === e.target.value);
              if (p) setWhitelist(p.whitelist.join("\n"));
            }} defaultValue="">
              <option value="">{t("fwAppLabel")}</option>
              {profiles.map((p) => <option key={p.app} value={p.app}>{p.app}</option>)}
            </select>
            <textarea
              className="dv-input wide"
              rows={3}
              style={{ width: "100%", resize: "vertical" }}
              placeholder={t("fwWhitelistPlaceholder")}
              value={whitelist}
              onChange={(e) => setWhitelist(e.target.value)}
            />
            <button className="dv-btn" onClick={() => {
              const first = profiles[0];
              if (first) void saveWhitelist(first);
            }}>{t("fwWhitelistSave")}</button>
          </div>
        </>
      )}

      <h3>{t("fwAlertsTitle")}</h3>
      {!center || center.alerts.length === 0 ? (
        <div className="dv-empty">{t("fwNoAlerts")}</div>
      ) : (
        <div className="dv-list">
          {center.alerts.slice(0, 200).map((a) => {
            const hot = center.hotApps.includes(a.app);
            return (
              <div key={a.id} className="dv-item">
                <span className={`dv-chip ${hot ? "bad" : "warn"}`}>
                  {hot ? t("fwHot") : t("fwBlocked")}
                </span>
                <span className="grow">{a.app} — {a.host}</span>
                <span className="dv-chip">×{a.attempts}</span>
                <span className="dv-tl-meta">{fmtTime(a.ts)}</span>
                <button className="dv-btn" onClick={() => void resolve(a.id, true)}>{t("fwAllow")}</button>
                <button className="dv-btn danger" onClick={() => void resolve(a.id, false)}>{t("fwKeepBlocked")}</button>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
