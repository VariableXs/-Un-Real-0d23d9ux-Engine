import { useCallback, useEffect, useState } from "react";
import { Activity, RefreshCw } from "lucide-react";
import { useI18n } from "../../i18n";
import { ipc, errMessage } from "../../lib/ipc";
import type { Shell } from "../../lib/ipc";
import { ipc11 } from "../../lib/ipc11";
import type {
  BatteryHealth,
  CheckItem,
  EventRow,
  FileHit,
  KeepAwakeState,
  MonitorDto,
  PortRow,
  PowerLossReport,
  PowerScheme,
  StartupProc,
  UptimeDto,
} from "../../lib/ipc11";
import { isTauriRuntime } from "../../entries/runtime";
import { pushToast } from "../../state/uiStore";
import { formatBytes } from "../../lib/format";
import {
  PERF_PROFILES,
  keepAwakeDeadline,
  loadPerfMode,
  matchSchemeGuid,
  savePerfMode,
  type PerfMode,
} from "./syshub/perfmode";
import { loadMonProfile, saveMonProfile } from "./syshub/monprofile";
import { hudEnabled, setHudEnabled } from "./syshub/PerfHud";
import "./../../styles/ai11-syshub.css";

/**
 * AI-11 系统中枢（VWM 工具应用）：
 * U-43 多显示器编排 / U-46 色彩与时辰 / U-48 性能模式 / N-20 电源与电池 /
 * N-21 网络指挥台 / N-22 存储健康 / N-25 健康自愈 / V-52 可靠性时间线 /
 * V-53 端口侦探 / V-54 保持唤醒 / V-55 大文件雷达 / V-56 运行时长 /
 * V-57 优先级预设 / V-59 断电自检 / V-60 启动归因 / V-51 亮度音量微步进 /
 * U-45/U-47 音频·无线只读视图。
 *
 * 红线：探针只读；代理/电源计划/gamma/优先级等写操作一律显式按钮确认；
 * 失败如实标注，不编造数据；退出还原由后端 restore_on_exit 兜底。
 */

type TabId = "overview" | "display" | "power" | "network" | "storage" | "system" | "periph";

function fmtUptime(secs: number): string {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return d > 0 ? `${d}d ${h}h ${m}m` : `${h}h ${m}m`;
}

const STATUS_DOT: Record<string, string> = { ok: "ok", warn: "warn", fail: "fail" };

export function SysHubApp(_props: { winId: string }): React.ReactElement {
  const { t } = useI18n();
  const [tab, setTab] = useState<TabId>("overview");
  const [busy, setBusy] = useState(false);

  // ---- overview：uptime + 性能模式 + 保持唤醒 + 电池 ----
  const [uptime, setUptime] = useState<UptimeDto | null>(null);
  const [mode, setMode] = useState<PerfMode>(() => loadPerfMode());
  const [schemes, setSchemes] = useState<PowerScheme[] | null>(null);
  const [awake, setAwake] = useState<KeepAwakeState | null>(null);
  const [awakeMin, setAwakeMin] = useState(60);
  const [awakeDeadline, setAwakeDeadline] = useState<number | null>(null);
  const [battery, setBattery] = useState<Shell.BatteryState | null>(null);
  const [wear, setWear] = useState<BatteryHealth | null>(null);
  const [brightness, setBrightness] = useState<Shell.BrightnessState | null>(null);
  const [gammaK, setGammaK] = useState(6500);
  const [hud, setHud] = useState(() => hudEnabled());

  // ---- display ----
  const [mons, setMons] = useState<MonitorDto[] | null>(null);
  const [monCount, setMonCount] = useState<number | null>(null);

  // ---- network ----
  const [ports, setPorts] = useState<PortRow[] | null>(null);
  const [pidNames, setPidNames] = useState<Record<number, string>>({});
  const [pingHost, setPingHost] = useState("www.baidu.com");
  const [pingRes, setPingRes] = useState<string | null>(null);
  const [proxyState, setProxyState] = useState<Awaited<ReturnType<typeof ipc11.proxyGet>> | null>(null);
  const [proxyServer, setProxyServer] = useState("127.0.0.1:7890");

  // ---- storage ----
  const [bigRoot, setBigRoot] = useState("C:\\Users");
  const [bigMinMb, setBigMinMb] = useState(500);
  const [bigDays, setBigDays] = useState(30);
  const [bigHits, setBigHits] = useState<FileHit[] | null>(null);

  // ---- system ----
  const [events, setEvents] = useState<EventRow[] | null>(null);
  const [procs, setProcs] = useState<StartupProc[] | null>(null);
  const [checks, setChecks] = useState<CheckItem[] | null>(null);
  const [pwrLoss, setPwrLoss] = useState<PowerLossReport | null>(null);

  // ---- periph（U-45/U-47 走既有 hardware IPC；U-44 档案为只读视图） ----
  const [audioDev, setAudioDev] = useState<Shell.AudioDeviceInfo[] | null>(null);
  const [wifi, setWifi] = useState<Shell.WifiState | null>(null);
  const [wifiNets, setWifiNets] = useState<Shell.WifiNetwork[] | null>(null);
  const [bt, setBt] = useState<Shell.BluetoothState | null>(null);
  const [btDevs, setBtDevs] = useState<Shell.BtDevice[] | null>(null);

  const errOf = (e: unknown): string => errMessage(e).message;

  const refreshOverview = useCallback((): void => {
    if (!isTauriRuntime()) return;
    ipc11.sysUptime().then(setUptime).catch(() => setUptime(null));
    ipc11.keepawakeGet().then(setAwake).catch(() => setAwake(null));
    ipc.batteryGet().then(setBattery).catch(() => setBattery(null));
    ipc11.batteryHealth().then(setWear).catch(() => setWear(null));
    ipc.brightnessGet().then(setBrightness).catch(() => setBrightness(null));
  }, []);

  useEffect(() => {
    refreshOverview();
  }, [refreshOverview]);

  // 保持唤醒到期自动解除（V-54 前端定时器；进程退出后端兜底）
  useEffect(() => {
    if (awakeDeadline === null) return undefined;
    const id = window.setInterval(() => {
      if (Date.now() >= awakeDeadline) {
        setAwakeDeadline(null);
        ipc11.keepawakeSet(false, false).catch(() => {});
        ipc11.keepawakeGet().then(setAwake).catch(() => {});
      }
    }, 5_000);
    return () => window.clearInterval(id);
  }, [awakeDeadline]);

  const refreshTab = useCallback(
    (which: TabId): void => {
      if (!isTauriRuntime()) return;
      setBusy(true);
      const done = (): void => setBusy(false);
      if (which === "overview") {
        refreshOverview();
        done();
      } else if (which === "display") {
        ipc11
          .monitorList()
          .then((list: MonitorDto[]) => {
            setMons(list);
            const prof = loadMonProfile();
            setMonCount(prof ? Object.keys(prof.rects).length : null);
          })
          .catch((e: unknown) => pushToast("error", t("toolSyshub"), errOf(e)))
          .finally(done);
      } else if (which === "power") {
        ipc11.powerSchemesList().then(setSchemes).catch(() => setSchemes(null));
        done();
      } else if (which === "network") {
        ipc11
          .portTable()
          .then(async (rows: PortRow[]) => {
            setPorts(rows);
            // PID → 进程名（V-53 归因；与任务管理器同源 proc_list）
            try {
              const pl = await ipc.procList();
              const map: Record<number, string> = {};
              for (const p of pl as unknown as { pid: number; name: string }[]) map[p.pid] = p.name;
              setPidNames(map);
            } catch {
              /* 归因失败如实留 PID */
            }
          })
          .catch((e: unknown) => pushToast("error", t("toolSyshub"), errOf(e)));
        ipc11.proxyGet().then(setProxyState).catch(() => setProxyState(null));
        done();
      } else if (which === "storage") {
        done();
      } else if (which === "system") {
        ipc11
          .eventlogRecent("System", 30)
          .then(setEvents)
          .catch(() => setEvents(null));
        ipc11
          .startupProcs()
          .then((list: StartupProc[]) => {
            list.sort((a, b) => a.bootOffsetMs - b.bootOffsetMs);
            setProcs(list.slice(0, 30));
          })
          .catch(() => setProcs(null));
        ipc11.selfhealChecks().then(setChecks).catch(() => setChecks(null));
        ipc11.pwrlossCheck().then(setPwrLoss).catch(() => setPwrLoss(null));
        done();
      } else if (which === "periph") {
        ipc.audioDevices().then(setAudioDev).catch(() => setAudioDev(null));
        ipc.wifiGet().then(setWifi).catch(() => setWifi(null));
        ipc.bluetoothGet().then(setBt).catch(() => setBt(null));
        ipc.btDevices().then(setBtDevs).catch(() => setBtDevs(null));
        done();
      } else {
        done();
      }
    },
    [refreshOverview, t],
  );

  useEffect(() => {
    refreshTab(tab);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab]);

  // ---- actions（全部显式触发） ----

  const applyMode = async (m: PerfMode): Promise<void> => {
    setMode(m);
    savePerfMode(m);
    if (!isTauriRuntime()) return;
    try {
      const list = schemes ?? (await ipc11.powerSchemesList());
      setSchemes(list);
      const guid = matchSchemeGuid(list, m);
      if (guid) await ipc11.powerSchemeSet(guid);
      if (PERF_PROFILES[m].keepAwake) await ipc11.keepawakeSet(true, true);
      else if (awake?.on) await ipc11.keepawakeSet(false, false);
      refreshOverview();
      pushToast("success", t("shPowerPlan"), t("shApplied"));
    } catch (e) {
      pushToast("error", t("shPowerPlan"), errOf(e));
    }
  };

  const toggleAwake = async (): Promise<void> => {
    try {
      const next = !(awake?.on ?? false);
      await ipc11.keepawakeSet(next, next);
      setAwakeDeadline(next ? keepAwakeDeadline(Date.now(), awakeMin) : null);
      refreshOverview();
    } catch (e) {
      pushToast("error", t("shKeepAwake"), errOf(e));
    }
  };

  const runPing = async (): Promise<void> => {
    setPingRes("…");
    try {
      const r = await ipc11.netPing(pingHost);
      setPingRes(
        r.avgMs !== null ? `${r.host}: ${r.avgMs.toFixed(1)} ms（丢包 ${r.lostPct ?? "?"}%）` : r.rawExcerpt,
      );
    } catch (e) {
      setPingRes(errOf(e));
    }
  };

  const runBigScan = async (): Promise<void> => {
    setBusy(true);
    try {
      setBigHits(await ipc11.bigfileScan(bigRoot, bigMinMb, bigDays));
    } catch (e) {
      pushToast("error", t("shBigFiles"), errOf(e));
    } finally {
      setBusy(false);
    }
  };

  const runHeal = async (id: string): Promise<void> => {
    try {
      const msg = await ipc11.healRun(id);
      pushToast("success", t("shHeal"), msg);
      refreshTab("system");
    } catch (e) {
      pushToast("error", t("shHeal"), errOf(e));
    }
  };

  const setPriority = async (pid: number, klass: string): Promise<void> => {
    try {
      await ipc11.procPrioritySet(pid, klass);
      pushToast("success", t("shPriority"), `${pid} → ${klass}`);
    } catch (e) {
      pushToast("error", t("shPriority"), errOf(e));
    }
  };

  const TABS: { id: TabId; label: string }[] = [
    { id: "overview", label: t("shOverview") },
    { id: "display", label: t("shDisplays") },
    { id: "power", label: t("shPower") },
    { id: "network", label: t("shNetwork") },
    { id: "storage", label: t("shStorage") },
    { id: "system", label: t("shSystem") },
    { id: "periph", label: t("shPeriph") },
  ];

  return (
    <div className="syshub">
      <div className="syshub-tabs">
        {TABS.map((x) => (
          <button
            key={x.id}
            type="button"
            className={`syshub-tab${tab === x.id ? " on" : ""}`}
            onClick={() => setTab(x.id)}
          >
            {x.label}
          </button>
        ))}
        <button
          type="button"
          className="syshub-refresh"
          title={t("shRefresh")}
          onClick={() => refreshTab(tab)}
          disabled={busy}
        >
          <RefreshCw size={14} className={busy ? "spin" : ""} />
        </button>
      </div>

      <div className="syshub-body">
        {tab === "overview" && (
          <div className="syshub-sec">
            <div className="syshub-cards">
              <div className="syshub-card">
                <div className="k">{t("shSysUptime")}</div>
                <div className="v">{uptime ? fmtUptime(uptime.sysSecs) : "—"}</div>
              </div>
              <div className="syshub-card">
                <div className="k">{t("shEnvUptime")}</div>
                <div className="v">{uptime ? fmtUptime(uptime.envSecs) : "—"}</div>
              </div>
              <div className="syshub-card">
                <div className="k">{t("shBattery")}</div>
                <div className="v">
                  {battery?.hasBattery ? `${battery.percent ?? "?"}%${battery.acOnline ? " ⚡" : ""}` : "—"}
                </div>
              </div>
              <div className="syshub-card">
                <div className="k">{t("shWear")}</div>
                <div className="v">{wear?.wearPct != null ? `${wear.wearPct}%` : "—"}</div>
              </div>
            </div>
            <div className="syshub-row">
              <span className="k">{t("shPowerPlan")}</span>
              {(Object.keys(PERF_PROFILES) as PerfMode[]).map((m) => (
                <button key={m} type="button" className={`syshub-btn${mode === m ? " on" : ""}`} onClick={() => void applyMode(m)}>
                  {m === "eco" ? t("shModeEco") : m === "balanced" ? t("shModeBalanced") : t("shModeBoost")}
                </button>
              ))}
            </div>
            <div className="syshub-row">
              <span className="k">{t("shKeepAwake")}</span>
              <input
                type="number"
                min={1}
                max={720}
                value={awakeMin}
                onChange={(e) => setAwakeMin(Number(e.target.value) || 60)}
                className="syshub-input num"
              />
              <span className="dim">{t("shMinutes")}</span>
              <button type="button" className={`syshub-btn${awake?.on ? " on" : ""}`} onClick={() => void toggleAwake()}>
                {awake?.on ? t("shKeepOn") : t("shKeepOff")}
              </button>
              {awakeDeadline !== null && (
                <span className="dim small">{t("shUntil")} {new Date(awakeDeadline).toLocaleTimeString()}</span>
              )}
            </div>
            <div className="syshub-row">
              <span className="k">N-19 {t("shPerfHud")}</span>
              <button
                type="button"
                className={`syshub-btn${hud ? " on" : ""}`}
                onClick={() => {
                  const next = !hud;
                  setHudEnabled(next);
                  setHud(next);
                  window.dispatchEvent(new Event("variable:ai11:hud-changed"));
                }}
              >
                {hud ? t("shKeepOn") : t("shKeepOff")}
              </button>
              <span className="dim small">{t("shHudHint")}</span>
            </div>
            <div className="syshub-row">
              <span className="k">{t("shGamma")}</span>
              <input
                type="range"
                min={2800}
                max={6500}
                step={100}
                value={gammaK}
                onChange={(e) => setGammaK(Number(e.target.value))}
              />
              <span className="dim">{gammaK}K</span>
              <button type="button" className="syshub-btn" onClick={() => void ipc11.gammaSet(gammaK).catch((e) => pushToast("error", t("shGamma"), errOf(e)))}>
                {t("shApply")}
              </button>
              <button type="button" className="syshub-btn" onClick={() => void ipc11.gammaRestore().catch((e) => pushToast("error", t("shGamma"), errOf(e)))}>
                {t("shGammaRestore")}
              </button>
            </div>
            <div className="syshub-row">
              <span className="k">{t("shBrightness")}</span>
              {brightness?.supported ? (
                <>
                  <input
                    type="range"
                    min={0}
                    max={100}
                    value={brightness.level}
                    onChange={(e) => {
                      const v = Number(e.target.value);
                      setBrightness({ ...brightness, level: v });
                    }}
                    onMouseUp={(e) => void ipc.brightnessSet(Number((e.target as HTMLInputElement).value)).catch(() => {})}
                  />
                  <span className="dim">{brightness.level}%</span>
                </>
              ) : (
                <span className="dim">{t("shUnsupported")}</span>
              )}
            </div>
            <div className="dim small">{t("shReadonly")}</div>
          </div>
        )}

        {tab === "display" && (
          <div className="syshub-sec">
            <div className="syshub-row">
              <span className="k">U-43 {t("shDisplays")}</span>
              <button
                type="button"
                className="syshub-btn"
                onClick={() => {
                  if (mons) {
                    saveMonProfile(mons, Date.now());
                    setMonCount(mons.length);
                    pushToast("success", t("shDisplays"), t("shMemSaved"));
                  }
                }}
              >
                {t("shMemSave")}
              </button>
              {monCount !== null && <span className="dim small">{t("shMemSaved")}（{monCount}）</span>}
            </div>
            {(mons ?? []).map((m) => (
              <div key={m.device} className="syshub-line">
                <Activity size={13} />
                <b>{m.device}</b>
                <span className="dim">
                  {m.w}×{m.h} @ ({m.x},{m.y}) {m.primary ? `· ${t("shPrimary")}` : ""}
                </span>
              </div>
            ))}
            {mons && mons.length === 0 && <div className="dim">{t("shErr")}</div>}
          </div>
        )}

        {tab === "power" && (
          <div className="syshub-sec">
            <div className="syshub-row">
              <span className="k">{t("shPowerPlan")}</span>
              {(schemes ?? []).map((s) => (
                <button
                  key={s.guid}
                  type="button"
                  className={`syshub-btn${s.active ? " on" : ""}`}
                  onClick={() =>
                    void ipc11
                      .powerSchemeSet(s.guid)
                      .then(() => ipc11.powerSchemesList().then(setSchemes))
                      .catch((e) => pushToast("error", t("shPowerPlan"), errOf(e)))
                  }
                >
                  {s.name}
                </button>
              ))}
            </div>
            <div className="syshub-row">
              <span className="k">{t("shBattery")}</span>
              <span className="dim">
                {wear?.designMwh ? `设计 ${Math.round(wear.designMwh / 1000)} mWh · ` : ""}
                {wear?.fullMwh ? `满充 ${Math.round(wear.fullMwh / 1000)} mWh · ` : ""}
                {wear?.cycleCount ? `${t("shCycles")} ${wear.cycleCount} · ` : ""}
                {wear?.wearPct != null ? `${t("shWear")} ${wear.wearPct}%` : "—"}
              </span>
            </div>
            <div className="syshub-row">
              <span className="k">{t("shKeepAwake")}</span>
              <button type="button" className={`syshub-btn${awake?.on ? " on" : ""}`} onClick={() => void toggleAwake()}>
                {awake?.on ? t("shKeepOn") : t("shKeepOff")}
              </button>
              {awake?.display ? <span className="dim small">{t("shDisplay")}</span> : null}
            </div>
          </div>
        )}

        {tab === "network" && (
          <div className="syshub-sec">
            <div className="syshub-row">
              <span className="k">N-21 {t("shPing")}</span>
              <input className="syshub-input" value={pingHost} onChange={(e) => setPingHost(e.target.value)} />
              <button type="button" className="syshub-btn" onClick={() => void runPing()}>
                {t("shPingGo")}
              </button>
              {pingRes && <span className="dim small">{pingRes}</span>}
            </div>
            <div className="syshub-row">
              <span className="k">{t("shProxy")}</span>
              {proxyState && (
                <span className="dim small">
                  {proxyState.enabled ? `ON ${proxyState.server}` : "OFF"}
                  {proxyState.autoConfigUrl ? ` · PAC ${proxyState.autoConfigUrl}` : ""}
                </span>
              )}
              <input className="syshub-input" value={proxyServer} onChange={(e) => setProxyServer(e.target.value)} />
              <button
                type="button"
                className="syshub-btn"
                onClick={() =>
                  void ipc11
                    .proxySet(true, proxyServer)
                    .then(() => ipc11.proxyGet().then(setProxyState))
                    .catch((e) => pushToast("error", t("shProxy"), errOf(e)))
                }
              >
                {t("shProxyOn")}
              </button>
              <button
                type="button"
                className="syshub-btn"
                onClick={() =>
                  void ipc11
                    .proxySet(false, "")
                    .then(() => ipc11.proxyGet().then(setProxyState))
                    .catch((e) => pushToast("error", t("shProxy"), errOf(e)))
                }
              >
                {t("shProxyOff")}
              </button>
            </div>
            <div className="syshub-k">V-53 {t("shPorts")}（{(ports ?? []).length}）</div>
            <div className="syshub-table">
              <div className="h">
                <span>proto</span>
                <span>local</span>
                <span>remote</span>
                <span>state</span>
                <span>pid</span>
                <span>{t("shPriority")}</span>
              </div>
              {(ports ?? []).slice(0, 60).map((r, i) => (
                <div className="r" key={`${r.proto}-${r.local}-${i}`}>
                  <span>{r.proto}</span>
                  <span className="mono">{r.local}</span>
                  <span className="mono">{r.remote}</span>
                  <span>{r.state}</span>
                  <span className="mono" title={pidNames[r.pid] ?? ""}>
                    {pidNames[r.pid] ?? r.pid}
                  </span>
                  <span>
                    {["above", "normal", "below"].map((k) => (
                      <button key={k} type="button" className="syshub-mini" onClick={() => void setPriority(r.pid, k)}>
                        {k}
                      </button>
                    ))}
                  </span>
                </div>
              ))}
            </div>
            <div className="dim small">{t("shNoKill")}</div>
          </div>
        )}

        {tab === "storage" && (
          <div className="syshub-sec">
            <div className="syshub-row">
              <span className="k">V-55 {t("shBigFiles")}</span>
              <input className="syshub-input" value={bigRoot} onChange={(e) => setBigRoot(e.target.value)} />
              <input
                className="syshub-input num"
                type="number"
                min={1}
                value={bigMinMb}
                onChange={(e) => setBigMinMb(Number(e.target.value) || 100)}
              />
              <span className="dim">MB</span>
              <input
                className="syshub-input num"
                type="number"
                min={1}
                value={bigDays}
                onChange={(e) => setBigDays(Number(e.target.value) || 30)}
              />
              <span className="dim">{t("shDays")}</span>
              <button type="button" className="syshub-btn" onClick={() => void runBigScan()} disabled={busy}>
                {t("shScan")}
              </button>
            </div>
            <div className="syshub-table">
              {(bigHits ?? []).map((f) => (
                <div className="r" key={f.path}>
                  <span className="mono grow" title={f.path}>
                    {f.path}
                  </span>
                  <span className="mono">{formatBytes(f.size)}</span>
                </div>
              ))}
            </div>
            <div className="dim small">{t("shReadonly")}</div>
          </div>
        )}

        {tab === "system" && (
          <div className="syshub-sec">
            <div className="syshub-row">
              <span className="k">
                V-56 {t("shUptime")}：{uptime ? `${fmtUptime(uptime.sysSecs)} / ${t("shEnvUptime")} ${fmtUptime(uptime.envSecs)}` : "—"}
              </span>
              {uptime && uptime.sysSecs > 7 * 86400 && <span className="warn small">⚠ {t("shRestartHint")}</span>}
            </div>
            <div className="syshub-row">
              <span className="k">V-59 {t("shPwrLoss")}</span>
              {pwrLoss && (
                <span className={pwrLoss.dirty ? "warn small" : "ok small"}>
                  {pwrLoss.dirty ? `⚠ ${t("shDirty")}` : t("shClean")} · {pwrLoss.checks.filter((c) => c.status === "ok").length}/{pwrLoss.checks.length}
                </span>
              )}
              {pwrLoss?.fixed.length ? <span className="ok small">{t("shFixed")}: {pwrLoss.fixed.join(", ")}</span> : null}
            </div>
            <div className="syshub-row">
              <span className="k">N-25 {t("shChecks")}</span>
              {(checks ?? []).map((c) => (
                <span key={c.id} className={`dot ${STATUS_DOT[c.status] ?? ""}`} title={c.detail}>
                  {c.name}
                </span>
              ))}
              {["trash", "toolData"].map((id) => (
                <button key={id} type="button" className="syshub-btn" onClick={() => void runHeal(id)}>
                  {t("shHeal")}:{id}
                </button>
              ))}
            </div>
            <div className="syshub-k">V-60 {t("shStartup")}</div>
            <div className="syshub-table">
              {(procs ?? []).map((p) => (
                <div className="r" key={`${p.pid}-${p.name}`}>
                  <span className="mono">{(p.bootOffsetMs / 1000).toFixed(1)}s</span>
                  <span>{p.name}</span>
                  <span className="mono dim grow" title={p.path}>
                    {p.path || "—"}
                  </span>
                </div>
              ))}
            </div>
            <div className="syshub-k">V-52 {t("shEvents")}</div>
            <div className="syshub-table">
              {(events ?? []).slice(0, 20).map((e, i) => (
                <div className="r" key={i}>
                  <span className="mono dim">{e.time}</span>
                  <span className={`lv ${e.level.toLowerCase()}`}>{e.level}</span>
                  <span className="grow" title={e.message}>
                    {e.provider} #{e.id} — {e.message.slice(0, 90)}
                  </span>
                </div>
              ))}
            </div>
            <div className="dim small">{t("shNoDeleteEvents")}</div>
          </div>
        )}

        {tab === "periph" && (
          <div className="syshub-sec">
            <div className="syshub-k">U-45 {t("shAudio")}</div>
            {(audioDev ?? []).map((d) => (
              <div className="syshub-line" key={d.id}>
                <b>{d.name}</b>
                <span className="dim">{d.kind}{d.default ? " · default" : ""}</span>
                {d.kind === "render" && !d.default && (
                  <button type="button" className="syshub-mini" onClick={() => void ipc.audioSetDefault(d.id).catch((e) => pushToast("error", t("shAudio"), errOf(e)))}>
                    {t("shSetDefault")}
                  </button>
                )}
              </div>
            ))}
            <div className="syshub-k">U-47 {t("shWifi")}</div>
            {wifi && (
              <div className="syshub-line">
                <span className="dim">
                  {wifi.connected ? `${wifi.ssid ?? ""} · ${wifi.signal ?? "?"}%` : t("shDisconnected")}
                  {wifi.radio_on === false ? " · radio off" : ""}
                </span>
                <button
                  type="button"
                  className="syshub-btn"
                  onClick={() =>
                    void ipc
                      .wifiScan()
                      .then(setWifiNets)
                      .catch((e) => pushToast("error", t("shWifi"), errOf(e)))
                  }
                >
                  {t("shScan")}
                </button>
              </div>
            )}
            {(wifiNets ?? []).slice(0, 10).map((n) => (
              <div className="syshub-line" key={n.ssid}>
                <span>{n.ssid}</span>
                <span className="dim">{n.signal}%{n.secured ? " 🔒" : ""}</span>
              </div>
            ))}
            <div className="syshub-k">{t("shBluetooth")}</div>
            {bt && (
              <div className="dim small">
                {bt.available ? (bt.enabled ? "ON" : "OFF") : t("shUnsupported")}
              </div>
            )}
            {(btDevs ?? []).map((d) => (
              <div className="syshub-line" key={d.id}>
                <span>{d.name}</span>
                <span className="dim">{d.connected ? "connected" : ""}</span>
              </div>
            ))}
            <div className="dim small">{t("shReadonly")}</div>
          </div>
        )}
      </div>
    </div>
  );
}
