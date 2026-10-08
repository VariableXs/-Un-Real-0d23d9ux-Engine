import { useCallback, useMemo, useState } from "react";
import { ipc } from "../../lib/ipc";
import { useI18n } from "../../i18n";
import type { Lang } from "../../i18n/dictionaries";
import type { Settings } from "../../lib/settings";

/**
 * B-32 OOBE 首次初始化向导——四步：介质体检 → 口令建卷 → 三模板 → 60 秒导览。
 * 如实边界：
 * - 介质体检只做本机能力探测（VHDX 挂载能力 + 管理员态），不做网络测速；
 * - 口令只在创建容器时使用，不落盘、不回显（丢失即无法解锁，UI 明示）；
 * - 三模板选择仅登记偏好（settings.oobeTools），实际安装仍在 AI Hub 按需进行。
 */

type Probe = { isAdmin: boolean; mountVhdAvailable: boolean; usable: boolean };

export function OobeWizard(props: {
  lang: Lang;
  dataDir: string;
  settings: Settings;
  onDone: (patch: Partial<Settings>) => void;
}) {
  const { t } = useI18n();
  const { dataDir, settings, onDone } = props;
  const [step, setStep] = useState(0);
  const [probe, setProbe] = useState<Probe | null>(null);
  const [pass, setPass] = useState("");
  const [pass2, setPass2] = useState("");
  const [containerPath, setContainerPath] = useState(
    () => settings.oobeContainerPath || `${dataDir.replace(/[\\/]+$/, "")}/data.uxv`,
  );
  const [tools, setTools] = useState<string[]>(settings.oobeTools ?? []);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const runProbe = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      setProbe(await ipc.vhdxProbe());
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  }, []);

  const createContainer = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      const encrypted = pass.length > 0;
      if (encrypted && pass !== pass2) throw new Error(t("oobePassMismatch"));
      await ipc.containerInit(containerPath, encrypted ? pass : undefined);
      onDone({
        oobeContainerPath: containerPath,
        oobeContainerEncrypted: encrypted,
      });
      setStep(2);
    } catch (e) {
      setError(errText(e));
    } finally {
      setBusy(false);
    }
  }, [containerPath, pass, pass2, onDone, t]);

  const finish = useCallback(() => {
    onDone({ oobeDone: true, oobeTools: tools, wizardDone: true });
  }, [onDone, tools]);

  const steps = useMemo(
    () => [t("oobeStepProbe"), t("oobeStepVault"), t("oobeStepTools"), t("oobeStepTour")],
    [t],
  );

  const toggleTool = (id: string) =>
    setTools((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));

  return (
    <div className="oobe-overlay" role="dialog" aria-modal>
      <div className="oobe-card">
        <div className="oobe-steps">
          {steps.map((s, i) => (
            <span key={s} className={`oobe-step${i === step ? " on" : ""}${i < step ? " done" : ""}`}>
              {s}
            </span>
          ))}
        </div>

        {step === 0 && (
          <section>
            <h2>{t("oobeProbeTitle")}</h2>
            <p className="dim">{t("oobeProbeHint")}</p>
            <button type="button" disabled={busy} onClick={runProbe}>
              {t("oobeProbeRun")}
            </button>
            {probe && (
              <ul className="oobe-probe">
                <li>
                  {t("oobeAdmin")}: {probe.isAdmin ? "✅" : "—"}
                </li>
                <li>
                  {t("oobeMountVhd")}: {probe.mountVhdAvailable ? "✅" : "—"}
                </li>
                <li>
                  {t("oobeVhdxUsable")}: {probe.usable ? t("oobeYes") : t("oobeNoUseUxv")}
                </li>
              </ul>
            )}
            <button type="button" className="primary" onClick={() => setStep(1)}>
              {t("oobeNext")}
            </button>
          </section>
        )}

        {step === 1 && (
          <section>
            <h2>{t("oobeVaultTitle")}</h2>
            <p className="dim">{t("oobeVaultHint")}</p>
            <label className="oobe-field">
              {t("oobeContainerPath")}
              <input value={containerPath} onChange={(e) => setContainerPath(e.target.value)} />
            </label>
            <label className="oobe-field">
              {t("oobePassphrase")}
              <input type="password" value={pass} onChange={(e) => setPass(e.target.value)} />
            </label>
            <label className="oobe-field">
              {t("oobePassphrase2")}
              <input type="password" value={pass2} onChange={(e) => setPass2(e.target.value)} />
            </label>
            <p className="dim small">{t("oobePassWarn")}</p>
            <button type="button" disabled={busy} onClick={createContainer}>
              {t("oobeCreate")}
            </button>
            <button type="button" className="primary" onClick={() => setStep(2)}>
              {t("oobeSkip")}
            </button>
          </section>
        )}

        {step === 2 && (
          <section>
            <h2>{t("oobeToolsTitle")}</h2>
            <p className="dim">{t("oobeToolsHint")}</p>
            {["claude-code", "codex", "zcode"].map((id) => (
              <label key={id} className="oobe-tool">
                <input
                  type="checkbox"
                  checked={tools.includes(id)}
                  onChange={() => toggleTool(id)}
                />
                {id}
              </label>
            ))}
            <button type="button" className="primary" onClick={() => setStep(3)}>
              {t("oobeNext")}
            </button>
          </section>
        )}

        {step === 3 && (
          <section>
            <h2>{t("oobeTourTitle")}</h2>
            <ul className="oobe-tour">
              <li>{t("oobeTour1")}</li>
              <li>{t("oobeTour2")}</li>
              <li>{t("oobeTour3")}</li>
              <li>{t("oobeTour4")}</li>
            </ul>
            <button type="button" className="primary" onClick={finish}>
              {t("oobeFinish")}
            </button>
          </section>
        )}

        {error && <p className="oobe-error">{error}</p>}
      </div>
    </div>
  );
}

/** 首启门控：settings.oobeDone 为 false 时渲染向导（App 两处渲染分支共用）。 */
export function OobeGate(props: {
  settings: Settings;
  onDone: (patch: Partial<Settings>) => void;
  dataDir: string;
}) {
  if (props.settings.oobeDone) return null;
  return (
    <OobeWizard
      lang={"zh" as Lang}
      dataDir={props.dataDir || "."}
      settings={props.settings}
      onDone={props.onDone}
    />
  );
}

function errText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e && typeof e === "object" && "message" in e) {
    return String((e as { message: unknown }).message);
  }
  return String(e);
}
