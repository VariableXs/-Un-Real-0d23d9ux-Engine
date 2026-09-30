import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../../i18n";
import { ipc } from "../../lib/ipc";
import type { PanicConfig, PanicRunReport } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { isTauriRuntime } from "../../entries/runtime";
import { dvError } from "./DataVaultWindow";

/**
 * U-34 紧急擦拭：L1 锁定 / L2 会话焚毁 / L3 撤离 三级协议配置 + 演练（dry-run）。
 * 面板内触发按钮同样遵循「长按 800ms + 进度环」防误触语义（与全局热键一致）。
 * 演练只产生将删除清单（wouldDelete），断言零副作用。
 */

const HOLD_MS = 800;

export function PanicPanel(): React.ReactElement {
  const { t } = useI18n();
  const [cfg, setCfg] = useState<PanicConfig | null>(null);
  const [targets, setTargets] = useState("");
  const [report, setReport] = useState<PanicRunReport | null>(null);
  const [busy, setBusy] = useState(false);
  // 长按进度（0..1）；松开归零
  const [holdPct, setHoldPct] = useState(0);
  const holdTimer = useRef<number | null>(null);
  const holdStart = useRef(0);

  const refresh = useCallback((): void => {
    if (!isTauriRuntime()) return;
    void ipc.panicConfigGet().then((c) => {
      setCfg(c);
      setTargets(c.targets.join("\n"));
    }).catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  useEffect(() => {
    return () => {
      if (holdTimer.current !== null) window.clearInterval(holdTimer.current);
    };
  }, []);

  const save = async (): Promise<void> => {
    const c = cfg;
    if (!c) return;
    setBusy(true);
    try {
      await ipc.panicConfigSet({
        ...c,
        targets: targets
          .split(/[\n;]+/)
          .map((s) => s.trim())
          .filter(Boolean),
      });
      pushToast("success", t("rcpSaved"));
      refresh();
    } catch (e) {
      pushToast("error", dvError(e));
    } finally {
      setBusy(false);
    }
  };

  const drill = async (): Promise<void> => {
    setBusy(true);
    try {
      const rep = await ipc.panicDrill();
      setReport(rep);
      pushToast("success", t("pnDrillOk", { n: rep.wouldDelete.length }));
    } catch (e) {
      pushToast("error", dvError(e));
    } finally {
      setBusy(false);
    }
  };

  // 长按 800ms 触发（演练态才允许面板触发真实流程；默认 dryRun=true 演练）
  const beginHold = (level: "L1" | "L2" | "L3", dryRun: boolean): void => {
    if (holdTimer.current !== null) return;
    holdStart.current = performance.now();
    holdTimer.current = window.setInterval(() => {
      const pct = Math.min(1, (performance.now() - holdStart.current) / HOLD_MS);
      setHoldPct(pct);
      if (pct >= 1) {
        endHold(true);
        void run(level, dryRun);
      }
    }, 40);
  };

  const endHold = (fire: boolean): void => {
    if (holdTimer.current !== null) {
      window.clearInterval(holdTimer.current);
      holdTimer.current = null;
    }
    if (!fire) setHoldPct(0);
    else window.setTimeout(() => setHoldPct(0), 300);
  };

  const run = async (level: "L1" | "L2" | "L3", dryRun: boolean): Promise<void> => {
    if (!dryRun) {
      // 真实触发二次确认（面板路径；全局热键路径由长按 800ms 本身防护）
      const ok = window.confirm(`${t("pnTrigger")} ${level}?\n${dryRun ? "" : t("pnRealConfirm")}`);
      if (!ok) return;
    }
    setBusy(true);
    try {
      const rep = await ipc.panicTrigger(level, dryRun);
      setReport(rep);
      if (dryRun) {
        pushToast("success", t("pnDrillOk", { n: rep.wouldDelete.length }));
      } else {
        pushToast("success", t("pnDone", { l: rep.level }));
      }
    } catch (e) {
      pushToast("error", dvError(e));
    } finally {
      setBusy(false);
    }
  };

  if (!cfg) {
    return (
      <section>
        <h2>{t("pnTitle")}</h2>
        <div className="dv-empty">—</div>
      </section>
    );
  }

  const holdBtn = (level: "L1" | "L2" | "L3", dryRun: boolean, label: string, cls: string): React.ReactNode => (
    <button
      type="button"
      className={`dv-btn ${cls}`}
      disabled={busy}
      onPointerDown={() => beginHold(level, dryRun)}
      onPointerUp={() => endHold(false)}
      onPointerLeave={() => endHold(false)}
    >
      {label}
      <span
        aria-hidden
        style={{
          display: "inline-block",
          width: 46,
          height: 4,
          marginLeft: 8,
          borderRadius: 2,
          background: "rgba(128,128,128,0.35)",
          verticalAlign: "middle",
          overflow: "hidden",
        }}
      >
        <span
          style={{
            display: "block",
            height: "100%",
            width: `${Math.round(holdPct * 100)}%`,
            background: cls.includes("danger") ? "#e5484d" : "#608deb",
            transition: "width 40ms linear",
          }}
        />
      </span>
    </button>
  );

  return (
    <section>
      <h2>{t("pnTitle")}</h2>
      <p className="dv-hint">{t("pnHint")}</p>

      <div className="dv-row">
        <label className="dv-hint">{t("pnLevel")}</label>
        <select
          className="dv-select"
          value={cfg.level}
          onChange={(e) => setCfg({ ...cfg, level: e.target.value })}
        >
          <option value="L1">L1 · {t("pnL1")}</option>
          <option value="L2">L2 · {t("pnL2")}</option>
          <option value="L3">L3 · {t("pnL3")}</option>
        </select>
        <label className="dv-hint" title={t("pnHotkeyHint")}>
          <input
            type="checkbox"
            checked={cfg.hotkeyEnabled}
            onChange={(e) => setCfg({ ...cfg, hotkeyEnabled: e.target.checked })}
          />{" "}
          {t("pnHotkey")}
        </label>
        <label className="dv-hint">
          <input
            type="checkbox"
            checked={cfg.dryRun}
            onChange={(e) => setCfg({ ...cfg, dryRun: e.target.checked })}
          />{" "}
          {t("pnDryRun")}
        </label>
        <button className="dv-btn primary" disabled={busy} onClick={() => void save()}>{t("pnSave")}</button>
      </div>

      <h3>{t("pnTargets")}</h3>
      <p className="dv-hint">{t("pnTargetsHint")}</p>
      <textarea
        className="dv-input wide"
        rows={3}
        style={{ width: "100%", resize: "vertical" }}
        placeholder={"D:\\work\\secret\nE:\\tmp\\cache"}
        value={targets}
        onChange={(e) => setTargets(e.target.value)}
      />

      <h3>{t("pnTrigger")}</h3>
      <div className="dv-row">
        {holdBtn("L1", true, `${t("pnDrill")} L1`, "")}
        {holdBtn("L2", true, `${t("pnDrill")} L2`, "")}
        {holdBtn("L3", true, `${t("pnDrill")} L3`, "")}
        <button className="dv-btn" disabled={busy} onClick={() => void drill()}>{t("pnDrillFull")}</button>
      </div>
      {!cfg.dryRun && (
        <div className="dv-row">
          {holdBtn("L1", false, `${t("pnTrigger")} L1`, "danger")}
          {holdBtn("L2", false, `${t("pnTrigger")} L2`, "danger")}
          {holdBtn("L3", false, `${t("pnTrigger")} L3`, "danger")}
        </div>
      )}

      {report && (
        <>
          <h3>{t("pnReport")}</h3>
          <div className="dv-list">
            <div className="dv-item">
              <span className="dv-chip">{report.level}{report.dryRun ? " · dry-run" : ""}</span>
              <span className="dv-tl-meta">{t("pnClipboard")}: {report.clipboardCleared ? "✓" : "—"}</span>
              <span className="dv-tl-meta">{t("pnVault")}: {report.vaultLocked ? "✓" : "—"}</span>
              <span className="dv-tl-meta">{t("pnIncognito")}: {report.incognitoBurned}</span>
            </div>
            {report.targetsBurned.length > 0 && (
              <div className="dv-item">
                <span className="dv-chip bad">{t("pnBurnedN", { n: report.targetsBurned.length })}</span>
                <span className="grow" title={report.targetsBurned.join("\n")}>
                  {report.targetsBurned.slice(0, 3).join(", ")}
                  {report.targetsBurned.length > 3 ? " …" : ""}
                </span>
              </div>
            )}
            {report.wouldDelete.length > 0 && (
              <div className="dv-item">
                <span className="dv-chip warn">{t("pnWouldDeleteN", { n: report.wouldDelete.length })}</span>
                <span className="grow" title={report.wouldDelete.join("\n")}>
                  {report.wouldDelete.slice(0, 3).join(", ")}
                  {report.wouldDelete.length > 3 ? " …" : ""}
                </span>
              </div>
            )}
          </div>
        </>
      )}
    </section>
  );
}
