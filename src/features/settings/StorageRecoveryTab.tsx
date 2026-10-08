import { useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { errMessage, ipc } from "../../lib/ipc";
import type { ContainerDiag, ContainerStatsView, Shell } from "../../lib/ipc";
import { pushToast } from "../../state/uiStore";
import { formatBytes } from "../../lib/format";

/**
 * F-6 系统维护分区：计划备份（daily/weekly + 立即备份 + 错过补偿提示）、
 * 引擎自更新（本地更新包 SHA-256 校验 + 失败回滚，历史保留 2 份）、
 * 数据自检扩展（备份可恢复性 / 索引一致性 / 媒体孤儿引用）。
 * 边界：增量包下载走白名单下载器；此处只做校验、应用与回滚（如实展示）。
 */
function SysMaintSection(): React.ReactElement {
  const { t } = useI18n();
  const [sched, setSched] = useState<Shell.BackupSchedule | null>(null);
  const [update, setUpdate] = useState<Shell.UpdateCandidate | null>(null);
  const [findings, setFindings] = useState<Shell.MaintainFinding[] | null>(null);
  const [busy, setBusy] = useState(false);

  const loadSched = (): void => {
    void ipc.backupScheduleGet().then(setSched).catch(() => setSched(null));
  };

  useEffect(() => {
    loadSched();
  }, []);

  const setFreq = (freq: "none" | "daily" | "weekly"): void => {
    const hour = sched?.hour ?? 3;
    void ipc
      .backupScheduleSet(freq, hour)
      .then(setSched)
      .catch((e) => pushToast("error", t("smBackupTitle"), errMessage(e).message));
  };

  const setHour = (hour: number): void => {
    const freq = (sched?.freq ?? "none") as "none" | "daily" | "weekly";
    void ipc
      .backupScheduleSet(freq, hour)
      .then(setSched)
      .catch((e) => pushToast("error", t("smBackupTitle"), errMessage(e).message));
  };

  const run = async (fn: () => Promise<void>): Promise<void> => {
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      pushToast("error", t("smTitle"), errMessage(e).message);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="st-sysmaint">
      <h4 className="st-sec-title">{t("smBackupTitle")}</h4>
      <div className="st-actions">
        {(["none", "daily", "weekly"] as const).map((f) => (
          <button
            key={f}
            type="button"
            className={sched?.freq === f ? "sel" : undefined}
            aria-pressed={sched?.freq === f}
            onClick={() => setFreq(f)}
          >
            {t(`smFreq_${f}`)}
          </button>
        ))}
        <label className="st-field-inline small">
          {t("smHour")}
          <select
            value={sched?.hour ?? 3}
            disabled={sched?.freq === "none"}
            onChange={(e) => setHour(Number(e.target.value))}
          >
            {Array.from({ length: 24 }, (_, h) => (
              <option key={h} value={h}>{String(h).padStart(2, "0")}:00</option>
            ))}
          </select>
        </label>
        <button
          type="button"
          disabled={busy}
          onClick={() => void run(async () => {
            const name = await ipc.backupRunNow();
            pushToast("success", t("smBackupNow"), name);
            loadSched();
          })}
        >
          {t("smBackupNow")}
        </button>
      </div>
      {sched?.missed && (
        <p className="st-warn small">⚠ {t("smMissed")}</p>
      )}
      {sched && sched.lastRunMs > 0 && (
        <p className="dim small">
          {t("smLastRun")}: {new Date(sched.lastRunMs).toLocaleString()} · {sched.lastSource}
        </p>
      )}

      <h4 className="st-sec-title">{t("smUpdateTitle")}</h4>
      <div className="st-actions">
        <button
          type="button"
          disabled={busy}
          onClick={() => void run(async () => {
            const c = await ipc.updateScan();
            setUpdate(c);
            pushToast(c ? "info" : "info", t("smUpdateScan"), c ? `v${c.version} · ${c.files} files` : t("smUpdateNone"));
          })}
        >
          {t("smUpdateScan")}
        </button>
        <button
          type="button"
          disabled={busy || !update}
          onClick={() => void run(async () => {
            const msg = await ipc.updateApply();
            pushToast("success", t("smUpdateApply"), msg);
            setUpdate(null);
          })}
        >
          {t("smUpdateApply")}
        </button>
      </div>
      {update && (
        <p className="dim small">
          {t("smUpdateFound")}: v{update.version} · {update.files} {t("smFiles")}
        </p>
      )}
      <p className="dim small">{t("smUpdateBoundary")}</p>

      <h4 className="st-sec-title">{t("smCheckTitle")}</h4>
      <div className="st-actions">
        <button
          type="button"
          disabled={busy}
          onClick={() => void run(async () => {
            setFindings(await ipc.maintainSelfcheck());
          })}
        >
          {t("smCheckRun")}
        </button>
      </div>
      {findings && (
        <ul className="st-findings small">
          {findings.map((f) => (
            <li key={f.id} className={`sm-finding-${f.level}`}>
              {f.level === "ok" ? "✅" : f.level === "warn" ? "⚠" : "·"} {f.message}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

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
      {/* F-6 系统维护：计划备份 / 自更新 / 自检扩展 */}
      <SysMaintSection />
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
