import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import type { AuditTimeline, CanaryOverview, VaultItem, VaultStatus } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
import { dvError } from "./DataVaultWindow";
import { fmtSize, fmtTime } from "./shared";

/**
 * U-31/U-33 隐私仪表盘：保险箱（AES-256-GCM）+ 审计时间线（热力 + 空档如实标注）+ 金丝雀诱饵。
 * 全部本机、零网络；暂停审计的空档以 gaps 呈现，不留假记录。
 */
export function PrivacyPanel(): React.ReactElement {
  const { t } = useI18n();
  const [vault, setVault] = useState<VaultStatus | null>(null);
  const [items, setItems] = useState<VaultItem[]>([]);
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [importPath, setImportPath] = useState("");
  const [timeline, setTimeline] = useState<AuditTimeline | null>(null);
  const [canary, setCanary] = useState<CanaryOverview | null>(null);
  const [canaryDir, setCanaryDir] = useState("");
  const [canaryTpl, setCanaryTpl] = useState("");

  const refresh = useCallback((): void => {
    if (!isTauriRuntime()) return;
    void ipc.vaultStatus().then(setVault).catch(() => {});
    void ipc.vaultList().then(setItems).catch(() => {});
    void ipc.privTimeline(7).then(setTimeline).catch(() => {});
    void ipc.canaryList().then((o) => {
      setCanary(o);
      const firstTpl = o.templates[0];
      if (firstTpl) setCanaryTpl((prev) => prev || firstTpl);
    }).catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const doInit = async (): Promise<void> => {
    if (pw.length < 4) {
      pushToast("info", t("vaultPwShort"));
      return;
    }
    if (pw !== pw2) {
      pushToast("info", t("vaultPwMismatch"));
      return;
    }
    try {
      await ipc.vaultInit(pw);
      await ipc.vaultUnlock(pw);
      pushToast("success", t("vaultInitOk"));
      setPw("");
      setPw2("");
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const doUnlock = async (): Promise<void> => {
    try {
      await ipc.vaultUnlock(pw);
      pushToast("success", t("vaultUnlockOk"));
      setPw("");
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const doLock = async (): Promise<void> => {
    try {
      await ipc.vaultLock();
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const doImport = async (shred: boolean): Promise<void> => {
    if (!importPath) return;
    try {
      await ipc.vaultImport(importPath, shred);
      pushToast("success", t("vaultImport"));
      setImportPath("");
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const doExport = async (name: string): Promise<void> => {
    const dest = window.prompt(t("vaultExport"));
    if (!dest) return;
    try {
      const p = await ipc.vaultExport(name, dest);
      pushToast("success", p);
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const doDestroy = async (name: string): Promise<void> => {
    if (!window.confirm(`${t("vaultDestroy")}: ${name}?`)) return;
    try {
      await ipc.vaultDestroy(name);
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const plant = async (): Promise<void> => {
    if (!canaryDir || !canaryTpl) return;
    try {
      await ipc.canaryPlant(canaryDir, canaryTpl);
      pushToast("success", t("pdCanaryPlant"));
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const removeCanary = async (id: string): Promise<void> => {
    try {
      await ipc.canaryRemove(id);
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    }
  };

  const maxDaily = timeline
    ? Math.max(1, ...Object.values(timeline.daily))
    : 1;

  return (
    <section>
      <h2>{t("dvTabPrivacy")}</h2>
      <p className="dv-hint">{t("vaultHint")}</p>

      <h3>{t("vaultTitle")}</h3>
      {vault?.initialized ? (
        <>
          <p className="dv-hint">
            {vault.unlocked
              ? t("vaultStateUnlocked", { n: vault.count, size: fmtSize(vault.bytes) })
              : t("vaultStateLocked", { n: vault.count })}
          </p>
          {!vault.unlocked && (
            <div className="dv-row">
              <input className="dv-input" type="password" placeholder={t("vaultPwInput")}
                value={pw} onChange={(e) => setPw(e.target.value)} />
              <button className="dv-btn primary" onClick={() => void doUnlock()}>{t("vaultUnlock")}</button>
            </div>
          )}
          {vault.unlocked && (
            <>
              <div className="dv-row">
                <input className="dv-input wide" placeholder="D:\path\to\file"
                  value={importPath} onChange={(e) => setImportPath(e.target.value)} />
                <button className="dv-btn" onClick={() => void doImport(false)}>{t("vaultImport")}</button>
                <button className="dv-btn danger" onClick={() => void doImport(true)}>
                  {t("vaultImport")} + {t("privacyShred")}
                </button>
                <button className="dv-btn" onClick={() => void doLock()}>{t("vaultLock")}</button>
              </div>
              <div className="dv-list">
                {items.map((it) => (
                  <div key={it.name} className="dv-item">
                    <span className="grow">{it.name}</span>
                    <span className="dv-tl-meta">{fmtSize(it.size)} · {fmtTime(it.addedAt)}</span>
                    <button className="dv-btn" onClick={() => void doExport(it.name)}>{t("vaultExport")}</button>
                    <button className="dv-btn danger" onClick={() => void doDestroy(it.name)}>{t("vaultDestroy")}</button>
                  </div>
                ))}
              </div>
            </>
          )}
        </>
      ) : (
        <div className="dv-row">
          <input className="dv-input" type="password" placeholder={t("vaultNewPw")}
            value={pw} onChange={(e) => setPw(e.target.value)} />
          <input className="dv-input" type="password" placeholder={t("vaultNewPw2")}
            value={pw2} onChange={(e) => setPw2(e.target.value)} />
          <button className="dv-btn primary" onClick={() => void doInit()}>{t("vaultInit")}</button>
        </div>
      )}

      <h3>{t("dvTabPrivacy")} · timeline</h3>
      {timeline ? (
        <>
          {!timeline.enabled && <div className="dv-banner alert">audit paused</div>}
          <div className="dv-row">
            {Object.entries(timeline.daily).slice(-14).map(([day, n]) => (
              <div key={day} title={`${day}: ${n}`}
                style={{
                  width: 22, height: 22, borderRadius: 5,
                  background: `rgba(96, 141, 235, ${0.15 + 0.85 * (n / maxDaily)})`,
                  display: "flex", alignItems: "center", justifyContent: "center",
                  fontSize: 10, opacity: 0.9,
                }}>
                {n > 99 ? "99+" : n || ""}
              </div>
            ))}
          </div>
          {timeline.gaps.length > 0 && (
            <p className="dv-hint">
              gaps: {timeline.gaps.map((g) => `${fmtTime(g.from)} → ${g.to ? fmtTime(g.to) : "…"}`).join(" · ")}
            </p>
          )}
          <div className="dv-list">
            {timeline.events.slice(0, 100).reverse().map((ev) => (
              <div key={ev.id} className="dv-item">
                <span className={`dv-chip ${ev.authorized ? "ok" : "warn"}`}>{ev.kind}</span>
                <span className="grow">{ev.app} — {ev.resource}</span>
                {timeline.anomalies.includes(ev.id) && <span className="dv-chip bad">!</span>}
                <span className="dv-tl-meta">{fmtTime(ev.ts)}</span>
              </div>
            ))}
          </div>
        </>
      ) : (
        <div className="dv-empty">—</div>
      )}

      <h3>{t("pdCanaryTitle")}</h3>
      <div className="dv-row">
        <input className="dv-input wide" placeholder={t("pdCanaryDir")}
          value={canaryDir} onChange={(e) => setCanaryDir(e.target.value)} />
        <select className="dv-select" value={canaryTpl} onChange={(e) => setCanaryTpl(e.target.value)}>
          {(canary?.templates ?? []).map((tp) => <option key={tp} value={tp}>{tp}</option>)}
        </select>
        <button className="dv-btn primary" onClick={() => void plant()}>{t("pdCanaryPlant")}</button>
      </div>
      {canary && canary.files.length > 0 && (
        <div className="dv-list">
          {canary.files.map((f) => (
            <div key={f.id} className="dv-item">
              <span className="grow" title={f.path}>{f.path}</span>
              {f.triggers.length > 0 ? (
                <span className={`dv-chip ${f.triggers.some((x) => !x.exempted) ? "bad" : "warn"}`}>
                  {t("pdCanaryAlerts", { n: f.triggers.length })}
                </span>
              ) : (
                <span className="dv-chip ok">0</span>
              )}
              <button className="dv-btn danger" onClick={() => void removeCanary(f.id)}>{t("pdCanaryRemove")}</button>
            </div>
          ))}
          {canary.whitelist.length > 0 && (
            <div className="dv-item">
              <span className="dv-hint">{t("pdCanaryWhitelist")}: {canary.whitelist.join(", ")}</span>
            </div>
          )}
        </div>
      )}
    </section>
  );
}

