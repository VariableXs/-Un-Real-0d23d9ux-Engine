import { useState } from "react";
import { useI18n } from "../../i18n";
import { errMessage, ipc } from "../../lib/ipc";
import type { ContainerDiag, ContainerStatsView } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { formatBytes } from "../../lib/format";

/**
 * B-33 M2 半包：设置页「存储与恢复」分区。
 * 恢复模式三件事：诊断（判定）→ 修复（journal 重放 UI 化）→ 救援导出（两级降级）。
 * 附容器仪表快照（B-17 数据源）与初始化（B-32 OOBE 同款命令）。
 */
export function StorageRecoveryTab() {
  const { t } = useI18n();
  const [path, setPath] = useState("./data.uxv");
  const [pass, setPass] = useState("");
  const [diag, setDiag] = useState<ContainerDiag | null>(null);
  const [stats, setStats] = useState<ContainerStatsView | null>(null);
  const [busy, setBusy] = useState(false);

  const guard = async <T,>(fn: () => Promise<T>, okKey: string): Promise<T | undefined> => {
    setBusy(true);
    try {
      const r = await fn();
      pushToast("success", t(okKey), "");
      return r;
    } catch (e) {
      pushToast("error", t("stFailed"), errMessage(e).message);
      return undefined;
    } finally {
      setBusy(false);
    }
  };

  const runDiag = async () => {
    const d = await guard(() => ipc.containerDiag(path), "stDiagDone");
    if (d) setDiag(d);
  };

  const runRepair = async () => {
    const r = await guard(
      () => ipc.containerRepair(path, pass || undefined),
      "stRepairDone",
    );
    if (r) setDiag((prev) => (prev ? { ...prev, openable: true, openError: null } : prev));
  };

  const runRescue = async () => {
    const r = await guard(
      () => ipc.containerRescueExport(path, `${path}.rescue`, pass || undefined),
      "stRescueDone",
    );
    if (r) pushToast("info", t("stRescueMode"), String(r.mode));
  };

  const runRev = async () => {
    const r = await guard(
      () => ipc.revocationListExport("revocation-list.md"),
      "stRevDone",
    );
    if (r) pushToast("info", t("stRevDone"), `${r.entries} · ${r.out}`);
  };

  const runDiagExport = async () => {
    const r = await guard(
      () => ipc.diagnosticExport("diagnostic-report.md"),
      "stDiagExportDone",
    );
    if (r) pushToast("info", t("stDiagExportDone"), `${r.sections} sections · ${r.out}`);
  };

  const runStats = async () => {
    const s = await guard(() => ipc.containerStats(path, pass || undefined), "stStatsDone");
    if (s) setStats(s);
  };

  return (
    <div className="st-recovery">
      <p className="dim small">{t("stHint")}</p>
      <label className="st-field">
        {t("stContainerPath")}
        <input value={path} onChange={(e) => setPath(e.target.value)} />
      </label>
      <label className="st-field">
        {t("stPassphrase")}
        <input type="password" value={pass} onChange={(e) => setPass(e.target.value)} />
      </label>

      <div className="st-actions">
        <button type="button" disabled={busy} onClick={runDiag}>
          {t("stDiag")}
        </button>
        <button type="button" disabled={busy} onClick={runRepair}>
          {t("stRepair")}
        </button>
        <button type="button" disabled={busy} onClick={runRescue}>
          {t("stRescue")}
        </button>
        <button type="button" disabled={busy} onClick={runDiagExport}>
          {t("stDiagExport")}
        </button>
        <button type="button" disabled={busy} onClick={runStats}>
          {t("stStats")}
        </button>
        <button type="button" disabled={busy} onClick={() => void runRev()}>
          {t("stRev")}
        </button>
      </div>

      {diag && (
        <div className="st-diag">
          <div>
            {t("stVersion")}: {diag.containerVersion} / {diag.engineVersion}
            {diag.needsMigration ? ` · ${t("stNeedsMig")}` : ""}
          </div>
          <div>
            {t("stOpenable")}: {diag.openable ? "✅" : `❌ ${diag.openError ?? ""}`}
          </div>
          <div>
            {t("stSize")}: {formatBytes(diag.sizeBytes)}
          </div>
        </div>
      )}

      {stats && (
        <div className="st-diag">
          <div>
            {t("stFiles")}: {stats.fileCount} · {t("stChunks")}: {stats.chunkCount}
          </div>
          <div>
            {t("stWriteAmp")}: {stats.writeAmplification.toFixed(2)}
          </div>
          {stats.volumes.map((v) => (
            <div key={v.path}>
              {v.path}: {formatBytes(v.usedBytes)}
              {v.declaredCapacity > 0
                ? ` / ${formatBytes(v.declaredCapacity)}`
                : ""}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
