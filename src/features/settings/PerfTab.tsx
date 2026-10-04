/**
 * AI-13 性能与长跑组 — 设置页「性能与维护」标签
 * （Z-58 健康面板 / Z-59 空闲冻结 / Z-60 内存压力 / Z-61 更新通道 / Z-62 本地统计 /
 *   M-46 日志轮转 / M-48 DB 紧凑 / N-35 分身 / N-36 接力 / M-53 崩溃转储）
 *
 * 红线：采集与统计默认关（零采集）；危险操作（DB 紧凑）强制确认；
 * 分身接管互斥在后端文件锁仲裁（最高风险项不靠前端约定）。
 */

import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../../i18n";
import { errMessage, ipc, type Shell } from "../../lib/ipc";
import type { Settings } from "../../lib/settings";
import { pushToast } from "../../state/uiStore";
import { askConfirm } from "../../components/Modal";
import { formatBytes } from "../../lib/format";
import {
  startHealthCollection,
  stopHealthCollection,
  healthReport,
  type HealthSample,
} from "../../system/perf/threadHealth";
import {
  setOccluded,
  freezeReason,
  onFreezeChange,
  type FreezeReason,
} from "../../system/perf/idleFreeze";
import {
  memTier,
  onMemPressure,
  manualReset,
  startMemWatch,
  TIER_ACTIONS,
} from "../../system/perf/memPressure";
import {
  topActions,
  dailyTotals,
  clearStats,
  loadStats,
} from "../../system/perf/usageStats";
import {
  channelInfo,
  setChannel as setUpdateChannel,
  CHANNEL_LABELS,
} from "../../system/perf/updateChannel";

export function PerfTab(props: { settings: Settings; onPatch: (p: Partial<Settings>) => void }): React.ReactElement {
  const { t, lang } = useI18n();
  const s = props.settings;
  const set = <K extends keyof Settings>(key: K, value: Settings[K]): void =>
    props.onPatch({ [key]: value } as Partial<Settings>);

  // ---- Z-58 健康面板（开关驱动；关闭态零采集） ----
  const [health, setHealth] = useState<HealthSample | null>(null);
  useEffect(() => {
    if (s.perfThreadHealth) {
      startHealthCollection((sample) => setHealth(sample));
      return () => stopHealthCollection();
    }
    stopHealthCollection();
    return undefined;
  }, [s.perfThreadHealth]);
  const report = s.perfThreadHealth ? healthReport() : null;

  // ---- Z-59 空闲冻结：演示站点（本面板自身 rAF 频率展示冻结状态） ----
  const [frozen, setFrozen] = useState<FreezeReason>(freezeReason());
  useEffect(() => onFreezeChange(setFrozen), []);
  useEffect(() => {
    // 设置切换即接入全局遮挡事件（真实生效点在桌面循环；此处演示文档可见性）
    if (!s.perfIdleFreeze) return undefined;
    const onVis = (): void => setOccluded(document.hidden ? "hidden" : null);
    document.addEventListener("visibilitychange", onVis);
    return () => {
      document.removeEventListener("visibilitychange", onVis);
      setOccluded(null);
    };
  }, [s.perfIdleFreeze]);

  // ---- Z-60 内存压力 ----
  const [tier, setTier] = useState(memTier());
  useEffect(() => {
    const off = onMemPressure((e) => setTier(e.tier));
    if (s.perfThreadHealth) startMemWatch(() => ipc.perfMemSnapshot().then((m) => ({ loadPct: m.load_pct, tier: m.tier as 0 | 1 | 2 })).catch(() => null));
    return () => {
      off();
    };
  }, [s.perfThreadHealth]);

  // ---- M-46 日志 / M-48 DB / M-53 崩溃转储 ----
  const [logUsage, setLogUsage] = useState<Shell.LogUsage | null>(null);
  const [dumps, setDumps] = useState<Shell.CrashDumpInfo[]>([]);
  const reload = useCallback((): void => {
    void ipc.perfLogUsage().then(setLogUsage).catch(() => setLogUsage(null));
    void ipc.perfCrashDumps().then(setDumps).catch(() => setDumps([]));
  }, []);
  useEffect(reload, [reload]);

  // ---- N-35 分身 ----
  const [instances, setInstances] = useState<Shell.InstanceInfo[]>([]);
  const [newInstance, setNewInstance] = useState("");
  const reloadInstances = useCallback((): void => {
    void ipc.perfInstanceList().then(setInstances).catch(() => setInstances([]));
  }, []);
  useEffect(reloadInstances, [reloadInstances]);

  // ---- Z-62 本地统计 ----
  const [statsTop, setStatsTop] = useState<{ action: string; count: number }[]>([]);
  useEffect(() => {
    if (s.perfUsageStats) setStatsTop(topActions(10));
  }, [s.perfUsageStats]);

  const rotateLogs = (): void => {
    void ipc.perfLogRotate().then((r) => {
      pushToast("success", t("pfLogRotated"), `${r.deleted}`);
      reload();
    }).catch((e) => pushToast("error", t("pfLogRotate"), errMessage(e).message));
  };

  const compactDb = async (): Promise<void> => {
    const ok = await askConfirm({ title: t("pfDbCompact"), body: t("pfDbCompactBody") });
    if (!ok) return;
    try {
      const r = await ipc.perfDbCompact();
      pushToast("success", t("pfDbCompact"), `${formatBytes(r.before_bytes)} → ${formatBytes(r.after_bytes)}`);
    } catch (e) {
      pushToast("error", t("pfDbCompact"), errMessage(e).message);
    }
  };

  const createInstance = (): void => {
    const name = newInstance.trim();
    if (!name) return;
    void ipc.perfInstanceCreate(name, false).then(() => {
      setNewInstance("");
      reloadInstances();
    }).catch((e) => pushToast("error", t("pfInstanceCreate"), errMessage(e).message));
  };

  const deleteInstance = (name: string): void => {
    void ipc.perfInstanceDelete(name).then(reloadInstances)
      .catch((e) => pushToast("error", t("pfInstanceDelete"), errMessage(e).message));
  };

  return (
    <div className="settings-section" role="region" aria-label={t("pfTabTitle")}>
      <h3>{t("pfTabTitle")}</h3>

      {/* Z-58 健康面板 */}
      <div className="settings-row">
        <label>
          <input
            type="checkbox"
            checked={s.perfThreadHealth}
            onChange={(e) => set("perfThreadHealth", e.target.checked)}
          />
          {t("pfHealthToggle")}
        </label>
        <p className="hint">{t("pfHealthHint")}</p>
      </div>
      {report && report.samples.length > 0 && (
        <div className="hint">
          {t("pfHealthFps", { n: String(report.avgFps) })} · {t("pfHealthLong", { n: String(report.totalLongTasks) })}
          {health ? ` · heap ${health.heapUsedMb ?? "?"}MB` : ""}
        </div>
      )}

      {/* Z-59 空闲冻结 */}
      <div className="settings-row">
        <label>
          <input
            type="checkbox"
            checked={s.perfIdleFreeze}
            onChange={(e) => set("perfIdleFreeze", e.target.checked)}
          />
          {t("pfIdleFreezeToggle")}
        </label>
        <p className="hint">
          {t("pfIdleFreezeHint")} · {t("pfIdleState", {
            state: frozen === "none" ? t("pfIdleActive") : t("pfIdleFrozen"),
          })}
        </p>
      </div>

      {/* Z-60 内存压力 */}
      <div className="settings-row">
        <p className="hint">
          {t("pfMemTier", { tier: String(tier) })} · {TIER_ACTIONS[tier as 0 | 1 | 2].join(", ") || "—"}
        </p>
        {tier > 0 && (
          <button type="button" className="btn" onClick={() => { manualReset(); setTier(0); }}>
            {t("pfMemReset")}
          </button>
        )}
      </div>

      {/* Z-61 更新通道 */}
      <div className="settings-row">
        <label htmlFor="pf-channel">{t("pfChannel")}</label>
        <select
          id="pf-channel"
          value={s.updateChannel}
          onChange={(e) => {
            const ch = e.target.value === "beta" ? "beta" : "stable";
            setUpdateChannel(ch);
            set("updateChannel", ch);
          }}
        >
          <option value="stable">{lang === "en" ? CHANNEL_LABELS.stable.en : CHANNEL_LABELS.stable.zh}</option>
          <option value="beta">{lang === "en" ? CHANNEL_LABELS.beta.en : CHANNEL_LABELS.beta.zh}</option>
        </select>
        <p className="hint">{t("pfChannelHint", { rollback: channelInfo().rollbackPoint ?? t("pfNoRollback") })}</p>
      </div>

      {/* Z-62 本地统计 */}
      <div className="settings-row">
        <label>
          <input
            type="checkbox"
            checked={s.perfUsageStats}
            onChange={(e) => {
              set("perfUsageStats", e.target.checked);
              if (e.target.checked) setStatsTop(topActions(10));
            }}
          />
          {t("pfStatsToggle")}
        </label>
        <p className="hint">{t("pfStatsHint")}</p>
        {s.perfUsageStats && statsTop.length > 0 && (
          <ul className="hint">
            {statsTop.map((a) => (
              <li key={a.action}>{a.action}: {a.count}</li>
            ))}
          </ul>
        )}
        {s.perfUsageStats && loadStats().days.length > 0 && (
          <p className="hint">{t("pfStatsDaily", { n: String(dailyTotals(7).reduce((a, d) => a + d.total, 0)) })}</p>
        )}
        {s.perfUsageStats && (
          <button type="button" className="btn" onClick={() => { clearStats(); setStatsTop([]); pushToast("success", t("pfStatsCleared")); }}>
            {t("pfStatsClear")}
          </button>
        )}
      </div>

      {/* M-46 日志轮转 */}
      <div className="settings-row">
        <p className="hint">
          {t("pfLogUsage", {
            active: logUsage ? formatBytes(logUsage.active_bytes) : "—",
            archived: logUsage ? `${logUsage.archived_count} / ${formatBytes(logUsage.archived_bytes)}` : "—",
          })}
        </p>
        <button type="button" className="btn" onClick={rotateLogs}>{t("pfLogRotate")}</button>
      </div>

      {/* M-48 DB 紧凑 */}
      <div className="settings-row">
        <p className="hint">{t("pfDbCompactHint")}</p>
        <button type="button" className="btn" onClick={() => void compactDb()}>{t("pfDbCompact")}</button>
      </div>

      {/* M-53 崩溃转储 */}
      <div className="settings-row">
        <p className="hint">{t("pfDumps", { n: String(dumps.length) })}</p>
        {dumps.length > 0 && (
          <ul className="hint">
            {dumps.slice(0, 5).map((d) => (
              <li key={d.file}>{d.file} · {formatBytes(d.size)}</li>
            ))}
          </ul>
        )}
      </div>

      {/* N-35 多环境分身 */}
      <div className="settings-row">
        <h4>{t("pfInstanceTitle")}</h4>
        <p className="hint">{t("pfInstanceHint")}</p>
        <ul className="hint">
          {instances.map((i) => (
            <li key={i.name}>
              {i.name}{i.takes_desktop ? ` · ${t("pfInstanceDesktop")}` : ""}
              {" "}
              <button type="button" className="btn btn-sm" onClick={() => deleteInstance(i.name)}>
                {t("pfInstanceDelete")}
              </button>
            </li>
          ))}
        </ul>
        <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
          <input
            type="text"
            value={newInstance}
            placeholder={t("pfInstanceName")}
            onChange={(e) => setNewInstance(e.target.value)}
            style={{ maxWidth: 220 }}
          />
          <button type="button" className="btn" onClick={createInstance}>{t("pfInstanceCreate")}</button>
        </div>
      </div>

      {/* N-36 接力（诚实口径提示） */}
      <div className="settings-row">
        <h4>{t("pfRelayTitle")}</h4>
        <p className="hint">{t("pfRelayHint")}</p>
      </div>

      {/* M-54 资源公平调度（说明位：入口在硬件面板右键） */}
      <div className="settings-row">
        <h4>{t("pfQuotaTitle")}</h4>
        <p className="hint">{t("pfQuotaHint")}</p>
      </div>
    </div>
  );
}
