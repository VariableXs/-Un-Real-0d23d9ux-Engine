import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Activity, Ban, Cpu, Gauge, ListRestart, RefreshCw, Search, Wrench, X, ScrollText } from "lucide-react";
import { askConfirm } from "../../components/Modal";
import { pushToast } from "../../state/uiStore";
import { useI18n } from "../../i18n";
import { errMessage, ipc, type AppLogEntry, type Shell } from "../../lib/ipc";
import { listen } from "@tauri-apps/api/event";
import { isTauriRuntime } from "../../entries/runtime";

/**
 * F-3 任务管理器增强（VWM 系统窗口，单实例）：
 * - 进程页：Variable 家族 / 宿主进程双色分类 + CPU/内存排序 + 结束任务护栏
 *   （系统关键进程一律拒绝；宿主进程需显式确认）
 * - 性能页：总 CPU 折线 + 每核占用 + 内存（2s/1s 轮询，sysinfo 差分采样）
 * - 启动项页：注册表 Run 键（HKCU/HKLM）；直跑档只读（如实降级提示）
 * - 服务页：VM 档 PowerShell Get-Service；直跑档如实提示跳宿主管理
 */

type Tab = "proc" | "perf" | "startup" | "service" | "logs";

const TABS: { id: Tab; key: string; icon: typeof Activity }[] = [
  { id: "proc", key: "tmTabProc", icon: Activity },
  { id: "perf", key: "tmTabPerf", icon: Gauge },
  { id: "startup", key: "tmTabStartup", icon: ListRestart },
  { id: "service", key: "tmTabService", icon: Wrench },
  { id: "logs", key: "tmTabLogs", icon: ScrollText },
];

function fmtMem(n: number): string {
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

/** CPU 折线历史（环形缓冲）。 */
const HIST = 90;

export function TaskManApp(): React.ReactElement {
  const { t } = useI18n();
  const [tab, setTab] = useState<Tab>("proc");
  const [procs, setProcs] = useState<Shell.ProcInfo[] | null>(null);
  const [startup, setStartup] = useState<Shell.StartupItem[] | null>(null);
  const [services, setServices] = useState<Shell.ServiceItem[] | null>(null);
  const [svcErr, setSvcErr] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [onlyFamily, setOnlyFamily] = useState(false);
  const [sel, setSel] = useState<number | null>(null);
  const [cpuHist, setCpuHist] = useState<number[]>([]);
  const [cores, setCores] = useState<number[]>([]);
  const [memBrief, setMemBrief] = useState<Shell.SysBrief | null>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);

  // ---- 进程轮询（2s；排序按 CPU→内存，后端已排序） ----
  useEffect(() => {
    if (tab !== "proc") return;
    let alive = true;
    const tick = (): void => {
      void ipc
        .procList()
        .then((r) => {
          if (alive) setProcs(r);
        })
        .catch((e) => {
          if (alive) setProcs([]);
          pushToast("error", t("tmTitle"), errMessage(e).message);
        });
    };
    tick();
    const id = window.setInterval(tick, 2000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, [tab, t]);

  // ---- 性能轮询（1s） ----
  useEffect(() => {
    if (tab !== "perf") return;
    let alive = true;
    const tick = (): void => {
      void ipc
        .perfCpu()
        .then((perCore) => {
          if (!alive) return;
          setCores(perCore);
          const total = perCore.length > 0 ? perCore.reduce((a, b) => a + b, 0) / perCore.length : 0;
          setCpuHist((h) => [...h, total].slice(-HIST));
        })
        .catch(() => {});
      void ipc
        .sysBrief()
        .then((b) => {
          if (alive) setMemBrief(b);
        })
        .catch(() => {});
    };
    tick();
    const id = window.setInterval(tick, 1000);
    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, [tab]);

  // CPU 折线绘制
  useEffect(() => {
    const cv = canvasRef.current;
    if (!cv || tab !== "perf") return;
    const ctx = cv.getContext("2d");
    if (!ctx) return;
    const w = cv.width;
    const h = cv.height;
    ctx.clearRect(0, 0, w, h);
    // 网格线
    ctx.strokeStyle = "rgba(128,128,128,0.2)";
    ctx.lineWidth = 1;
    for (let i = 1; i < 4; i++) {
      ctx.beginPath();
      ctx.moveTo(0, (h / 4) * i);
      ctx.lineTo(w, (h / 4) * i);
      ctx.stroke();
    }
    if (cpuHist.length < 2) return;
    ctx.strokeStyle = "#4f8cff";
    ctx.lineWidth = 2;
    ctx.beginPath();
    cpuHist.forEach((v, i) => {
      const x = (i / (HIST - 1)) * w;
      const y = h - (Math.min(100, v) / 100) * (h - 4) - 2;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    });
    ctx.stroke();
  }, [cpuHist, tab]);

  const loadStartup = useCallback((): void => {
    void ipc
      .startupList()
      .then(setStartup)
      .catch(() => setStartup([]));
  }, []);

  const loadServices = useCallback((): void => {
    setSvcErr(null);
    void ipc
      .serviceList()
      .then(setServices)
      .catch((e) => {
        setServices([]);
        setSvcErr(errMessage(e).message);
      });
  }, []);

  useEffect(() => {
    if (tab === "startup" && startup === null) loadStartup();
    if (tab === "service" && services === null) loadServices();
  }, [tab, startup, services, loadStartup, loadServices]);

  const filtered = useMemo(() => {
    if (!procs) return [];
    const q = query.trim().toLowerCase();
    return procs.filter(
      (p) => (!onlyFamily || p.family === "variable") && (q === "" || p.name.toLowerCase().includes(q)),
    );
  }, [procs, query, onlyFamily]);

  const familyCount = useMemo(() => procs?.filter((p) => p.family === "variable").length ?? 0, [procs]);

  const kill = useCallback(
    async (p: Shell.ProcInfo): Promise<void> => {
      const isHost = p.family === "host";
      const ok = await askConfirm({
        title: t("tmKillTitle"),
        body: isHost
          ? t("tmKillHostMsg").replace("{name}", p.name)
          : t("tmKillFamilyMsg").replace("{name}", p.name),
        danger: true,
        okLabel: t("tmKillConfirm"),
      });
      if (!ok) return;
      try {
        const msg = await ipc.procKill(p.pid, isHost);
        pushToast("success", t("tmTitle"), msg);
      } catch (e) {
        pushToast("error", t("tmTitle"), errMessage(e).message);
      }
    },
    [t],
  );

  const disableStartup = useCallback(
    async (item: Shell.StartupItem): Promise<void> => {
      try {
        await ipc.startupDisable(item);
        pushToast("success", t("tmTabStartup"), `${item.name} ${t("tmStartupDisabled")}`);
        loadStartup();
      } catch (e) {
        pushToast("error", t("tmTabStartup"), errMessage(e).message);
      }
    },
    [t, loadStartup],
  );

  const toggleService = useCallback(
    async (s: Shell.ServiceItem): Promise<void> => {
      const running = s.status === "Running";
      try {
        const msg = await ipc.serviceSet(s.name, !running);
        pushToast("success", t("tmTabService"), msg);
        loadServices();
      } catch (e) {
        pushToast("error", t("tmTabService"), errMessage(e).message);
      }
    },
    [t, loadServices],
  );

  const memPct = memBrief && memBrief.memTotal > 0 ? (memBrief.memUsed / memBrief.memTotal) * 100 : 0;
  const curCpu = cpuHist.length > 0 ? cpuHist[cpuHist.length - 1]! : 0;

  // ---------- 日志页：启动/嵌入实时日志（后端 applog 总线） ----------
  const [logs, setLogs] = useState<AppLogEntry[] | null>(null);
  const logBoxRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (tab !== "logs") return;
    if (logs === null && isTauriRuntime()) {
      void ipc.applogRecent().then(setLogs).catch(() => setLogs([]));
    }
    if (!isTauriRuntime()) return;
    let disposed = false;
    let un: (() => void) | undefined;
    const p = listen<AppLogEntry>("sys://applog", (e) => {
      setLogs((cur) => {
        const next = [...(cur ?? []), e.payload];
        return next.length > 600 ? next.slice(next.length - 600) : next;
      });
    });
    void p
      .then((f) => {
        if (disposed) f();
        else un = f;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      un?.();
    };
  }, [tab, logs]);
  // 新日志到达 → 滚到底部
  useEffect(() => {
    const box = logBoxRef.current;
    if (tab === "logs" && box) box.scrollTop = box.scrollHeight;
  }, [logs, tab]);

  return (
    <div className="tman-app" role="application" aria-label={t("tmTitle")}>
      <div className="tman-tabs" role="tablist">
        {TABS.map((x) => (
          <button
            key={x.id}
            role="tab"
            aria-selected={tab === x.id}
            className={`tman-tab${tab === x.id ? " active" : ""}`}
            onClick={() => setTab(x.id)}
          >
            <x.icon size={14} /> {t(x.key)}
          </button>
        ))}
      </div>

      {tab === "proc" && (
        <div className="tman-proc">
          <div className="tman-toolbar">
            <div className="tman-search">
              <Search size={13} />
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("tmSearch")}
                aria-label={t("tmSearch")}
              />
            </div>
            <label className="tman-filter">
              <input type="checkbox" checked={onlyFamily} onChange={(e) => setOnlyFamily(e.target.checked)} />
              {t("tmOnlyFamily")}（{familyCount}）
            </label>
            <span className="dim small">{t("tmProcCount").replace("{n}", String(filtered.length))}</span>
          </div>
          <div className="tman-table" role="table" aria-label={t("tmTabProc")}>
            <div className="tman-row tman-head" role="row">
              <span role="columnheader">{t("tmColName")}</span>
              <span role="columnheader">{t("tmColPid")}</span>
              <span role="columnheader" className="num">{t("tmColCpu")}</span>
              <span role="columnheader" className="num">{t("tmColMem")}</span>
              <span role="columnheader">{t("tmColKind")}</span>
              <span role="columnheader" />
            </div>
            {procs === null && <p className="dim small" style={{ padding: 16 }}>…</p>}
            {procs !== null && filtered.length === 0 && (
              <p className="dim small" style={{ padding: 16 }}>{t("tmEmpty")}</p>
            )}
            {filtered.map((p) => (
              <div
                key={p.pid}
                role="row"
                className={`tman-row${sel === p.pid ? " sel" : ""}${p.family === "variable" ? " fam" : ""}`}
                onClick={() => setSel(p.pid)}
              >
                <span role="cell" className="ellipsis" title={p.name}>{p.name}</span>
                <span role="cell" className="dim">{p.pid}</span>
                <span role="cell" className="num">{p.cpu.toFixed(1)}%</span>
                <span role="cell" className="num">{fmtMem(p.mem)}</span>
                <span role="cell">
                  <span className={`tman-badge ${p.family === "variable" ? "fam" : "host"}`}>
                    {p.family === "variable" ? t("tmKindFam") : p.critical ? t("tmKindCritical") : t("tmKindHost")}
                  </span>
                </span>
                <span role="cell" className="tman-act">
                  {p.pid !== 0 && (
                    <button
                      type="button"
                      className="icon-btn tiny"
                      title={p.critical ? t("tmKillBlocked") : t("tmKill")}
                      disabled={p.critical}
                      onClick={(ev) => {
                        ev.stopPropagation();
                        void kill(p);
                      }}
                    >
                      <X size={13} />
                    </button>
                  )}
                </span>
              </div>
            ))}
          </div>
          <p className="dim small tman-note">{t("tmGuardNote")}</p>
        </div>
      )}

      {tab === "perf" && (
        <div className="tman-perf">
          <div className="tman-perf-grid">
            <div className="tman-card">
              <h3><Cpu size={14} /> CPU {curCpu.toFixed(0)}%</h3>
              <canvas ref={canvasRef} width={520} height={140} className="tman-chart" aria-label={t("tmColCpu")} />
              <div className="tman-cores">
                {cores.map((v, i) => (
                  <div key={i} className="tman-core" title={`CPU ${i}: ${v.toFixed(0)}%`}>
                    <div className="tman-core-bar" style={{ height: `${Math.min(100, v)}%` }} />
                    <span>{v.toFixed(0)}</span>
                  </div>
                ))}
              </div>
            </div>
            <div className="tman-card">
              <h3><Activity size={14} /> {t("tmMem")}</h3>
              {memBrief ? (
                <>
                  <p className="tman-big">
                    {fmtMem(memBrief.memUsed)} / {fmtMem(memBrief.memTotal)}
                  </p>
                  <div className="tman-membar" role="progressbar" aria-valuenow={Math.round(memPct)}>
                    <div style={{ width: `${memPct}%` }} />
                  </div>
                  <p className="dim small">{t("tmMemPct").replace("{p}", memPct.toFixed(0))}</p>
                  <p className="dim small">{t("tmMode").replace("{m}", memBrief.runtimeMode)}</p>
                  <p className="dim small">{t("tmGpuNa")}</p>
                </>
              ) : (
                <p className="dim small">…</p>
              )}
            </div>
          </div>
        </div>
      )}

      {tab === "startup" && (
        <div className="tman-startup">
          {startup === null && <p className="dim small" style={{ padding: 16 }}>…</p>}
          {startup !== null && startup.length === 0 && (
            <p className="dim small" style={{ padding: 16 }}>{t("tmStartupEmpty")}</p>
          )}
          {startup !== null && startup.length > 0 && (
            <div className="tman-table" role="table" aria-label={t("tmTabStartup")}>
              <div className="tman-row tman-head" role="row">
                <span role="columnheader">{t("tmColName")}</span>
                <span role="columnheader">{t("tmColCmd")}</span>
                <span role="columnheader">{t("tmColHive")}</span>
                <span role="columnheader" />
              </div>
              {startup.map((it) => (
                <div key={`${it.hive}:${it.name}`} role="row" className="tman-row">
                  <span role="cell" className="ellipsis" title={it.name}>{it.name}</span>
                  <span role="cell" className="dim ellipsis" title={it.cmd}>{it.cmd}</span>
                  <span role="cell"><span className="tman-badge host">{it.hive}</span></span>
                  <span role="cell" className="tman-act">
                    <button
                      type="button"
                      className="icon-btn tiny"
                      title={t("tmStartupDisable")}
                      onClick={() => void disableStartup(it)}
                    >
                      <Ban size={13} />
                    </button>
                  </span>
                </div>
              ))}
            </div>
          )}
          <p className="dim small tman-note">{t("tmStartupNote")}</p>
        </div>
      )}

      {tab === "service" && (
        <div className="tman-service">
          {svcErr && <p className="dim small" style={{ padding: 16 }}>{svcErr}</p>}
          {services === null && !svcErr && (
            <div className="skeleton-list" aria-busy="true" style={{ padding: 16, display: "flex", flexDirection: "column", gap: 8 }}>
              {[0, 1, 2, 3].map((i) => (
                <span key={i} className="skeleton" style={{ height: 16, width: `${86 - i * 8}%` }} />
              ))}
            </div>
          )}
          {services !== null && services.length > 0 && (
            <div className="tman-table" role="table" aria-label={t("tmTabService")}>
              <div className="tman-row tman-head" role="row">
                <span role="columnheader">{t("tmColName")}</span>
                <span role="columnheader">{t("tmColDisplay")}</span>
                <span role="columnheader">{t("tmColStatus")}</span>
                <span role="columnheader" />
              </div>
              {services.map((s) => (
                <div key={s.name} role="row" className="tman-row">
                  <span role="cell" className="ellipsis" title={s.name}>{s.name}</span>
                  <span role="cell" className="dim ellipsis" title={s.display}>{s.display}</span>
                  <span role="cell">
                    <span className={`tman-badge ${s.status === "Running" ? "fam" : "host"}`}>{s.status}</span>
                  </span>
                  <span role="cell" className="tman-act">
                    <button
                      type="button"
                      className="icon-btn tiny"
                      title={s.status === "Running" ? t("tmSvcStop") : t("tmSvcStart")}
                      onClick={() => void toggleService(s)}
                    >
                      {s.status === "Running" ? <Ban size={13} /> : <RefreshCw size={13} />}
                    </button>
                  </span>
                </div>
              ))}
            </div>
          )}
          <p className="dim small tman-note">{t("tmServiceNote")}</p>
        </div>
      )}

      {tab === "logs" && (
        <div className="tman-logs">
          <div className="tman-toolbar">
            <span className="dim small">{t("tmLogsHint")}</span>
            <span className="flex-1" />
            <button type="button" className="icon-btn tiny" title={t("tmLogsClear")} onClick={() => setLogs([])}>
              <X size={13} />
            </button>
          </div>
          <div className="tman-logbox" ref={logBoxRef} role="log" aria-label={t("tmTabLogs")}>
            {logs === null && <p className="dim small" style={{ padding: 16 }}>…</p>}
            {logs !== null && logs.length === 0 && (
              <p className="dim small" style={{ padding: 16 }}>{t("tmLogsEmpty")}</p>
            )}
            {logs !== null &&
              logs.map((e, i) => (
                <div key={i} className="tman-logline">
                  <span className="dim">{new Date(e.ts).toLocaleTimeString()}</span>
                  <span className="tman-badge fam">{e.tag}</span>
                  <span className="ellipsis" title={e.msg}>{e.msg}</span>
                </div>
              ))}
          </div>
        </div>
      )}
    </div>
  );
}
