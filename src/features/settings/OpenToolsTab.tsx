/**
 * AI-15 开放工具组 — 设置页「开放工具」标签。
 * 覆盖：M-57 出站桥 / M-59 嵌入声明 / M-63 资源包安全扫描 /
 * V-81 开放安装器 / V-82 环境变量编辑器 / V-83 计划任务工坊 / V-86 启动延迟编排 /
 * V-84 关联快照 / V-85 卸载善后 / V-87 服务依赖图 / V-89 配置对比 / V-90 沙盒试用，
 * 以及 M-55/56/60/61/62 工具链入口说明。
 * 红线：出站桥默认关（仅环回/内网）；winget 操作逐次确认；计划任务动作白名单；
 * 卸载善后仅报告 + 勾选删除（入回收站）；不做系统级关联（HKLM）操作。
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { open as openFileDialog } from "@tauri-apps/plugin-dialog";
import { useI18n } from "../../i18n";
import { ipc, type Shell } from "../../lib/ipc";
import {
  diffKindClass,
  fmtBytes,
  fmtTime,
  isSafeStartupMirror,
  nextFireTime,
  splitPathRows,
  triggerSummary,
} from "../../lib/openTools";
import { askConfirm } from "../../components/Modal";
import { pushToast } from "../../state/uiStore";

/** 出站桥事件白名单（与 opentools.rs KNOWN_EVENTS 镜像）。 */
const WEBHOOK_EVENTS = [
  "boot.ready",
  "window.embedded",
  "theme.changed",
  "wallpaper.changed",
  "schedule.fired",
  "pack.installed",
  "clipboard.pin",
  "shutdown.clean",
] as const;

/** 计划任务动作白名单（与 workshop.rs SCHED_ACTIONS 镜像；后端强制）。 */
const SCHED_ACTIONS = [
  "theme.set",
  "wallpaper.set",
  "perf.set",
  "notify.remind",
  "notes.review",
  "palette.run",
  "sound.chime",
] as const;

function Result(props: { text: string | null }): React.ReactElement | null {
  if (props.text === null) return null;
  return <div className="ot-result" data-testid="ot-result">{props.text}</div>;
}

export function OpenToolsTab(): React.ReactElement {
  const { t } = useI18n();
  const [busy, setBusy] = useState(false);
  const wrap = useCallback(async (fn: () => Promise<void>): Promise<void> => {
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      pushToast("error", t("otOpFail"), String(e));
    } finally {
      setBusy(false);
    }
  }, [t]);

  // ============================== M-57 出站桥 ==============================
  const [whCfg, setWhCfg] = useState<Shell.WebhookConfigView | null>(null);
  const [whLog, setWhLog] = useState<string | null>(null);
  const loadWh = useCallback((): void => {
    void ipc.webhookRulesGet().then(setWhCfg).catch(() => setWhCfg(null));
  }, []);
  useEffect(loadWh, [loadWh]);

  const patchWhRule = (idx: number, p: Partial<Shell.WebhookRuleView>): void => {
    if (!whCfg) return;
    const rules = whCfg.rules.map((r, i) => (i === idx ? { ...r, ...p } : r));
    setWhCfg({ ...whCfg, rules });
  };
  const addWhRule = (): void => {
    if (!whCfg) return;
    setWhCfg({ ...whCfg, rules: [...whCfg.rules, { id: "", event: "theme.changed", url: "http://127.0.0.1:9000/hook", enabled: false }] });
  };
  const saveWh = (): void => {
    if (!whCfg) return;
    void wrap(async () => {
      const saved = await ipc.webhookRulesSet(whCfg);
      setWhCfg(saved);
      pushToast("success", t("otWhSaved"));
    });
  };
  const testWh = (url: string): void => {
    void wrap(async () => {
      const ok = await ipc.webhookTest(url);
      pushToast(ok ? "success" : "error", ok ? t("otWhTestOk") : t("otWhTestFail"));
    });
  };

  // ===================== M-63 .vxs 安全扫描 / M-59 嵌入声明 =====================
  const [scanOut, setScanOut] = useState<string | null>(null);
  const doVxsScan = async (): Promise<void> => {
    const p = await openFileDialog({ multiple: false, filters: [{ name: "VXS Pack", extensions: ["vxs"] }] });
    if (typeof p !== "string") return;
    await wrap(async () => {
      const r = await ipc.vxsScan(p);
      const dist = r.typeDist.map(([k, n]) => `.${k} ×${n}`).join(", ");
      setScanOut(
        [
          `${r.path}`,
          `${t("otScanFiles")}: ${r.totalFiles} · ${fmtBytes(r.totalBytes)}`,
          `${t("otScanTypes")}: ${dist || "—"}`,
          r.safe ? `✓ ${t("otScanSafe")}` : `✕ ${t("otScanBlocked")}:`,
          ...r.blocked.map((b) => `  ✕ ${b}`),
          ...r.warnings.map((w) => `  ⚠ ${w}`),
        ].join("\n"),
      );
      if (!r.safe) pushToast("error", t("otScanBlocked"), r.blocked[0] ?? "");
    });
  };
  const [embPath, setEmbPath] = useState("");
  const [embOut, setEmbOut] = useState<string | null>(null);
  const doEmbScan = (): void => {
    if (!embPath.trim()) return;
    void wrap(async () => {
      const m = await ipc.embedManifestScan(embPath.trim());
      if (!m) {
        setEmbOut(t("otEmbNone"));
        return;
      }
      setEmbOut(
        [
          `${t("otEmbVersion")}: ${m.version}`,
          `titleMatch: ${m.titleMatch || "—"}`,
          `min: ${m.minWidth}×${m.minHeight}`,
          `multiInstance: ${m.multiInstance || "—"}`,
          `waitMs: ${m.waitMs}`,
        ].join("\n"),
      );
    });
  };

  // ============================== V-81 winget ==============================
  const [wgStatus, setWgStatus] = useState<Shell.WingetStatusView | null>(null);
  const [wgQuery, setWgQuery] = useState("");
  const [wgPkgs, setWgPkgs] = useState<Shell.WingetPkgView[] | null>(null);
  const [wgOut, setWgOut] = useState<string | null>(null);
  useEffect(() => {
    void ipc.wingetStatus().then(setWgStatus).catch(() => setWgStatus(null));
  }, []);

  const wgOp = (kind: "install" | "upgrade" | "uninstall", id: string, name: string): void => {
    void wrap(async () => {
      const label = t(kind === "install" ? "otWgInstall" : kind === "upgrade" ? "otWgUpgrade" : "otWgUninstall");
      const ok = await askConfirm({
        title: t("otWgConfirmTitle"),
        body: t("otWgConfirmBody", { op: label, name, id }),
        danger: kind === "uninstall",
      });
      if (!ok) return;
      const r =
        kind === "install" ? await ipc.wingetInstall(id, true)
          : kind === "upgrade" ? await ipc.wingetUpgradeOne(id)
            : await ipc.wingetUninstall(id);
      setWgOut(`${label} ${id}: ${r.ok ? "OK" : `exit=${r.exitCode}`}`);
    });
  };

  // ============================== V-82 环境变量 ==============================
  const [env, setEnv] = useState<Shell.EnvOverviewView | null>(null);
  const [envBackups, setEnvBackups] = useState<Shell.EnvBackupView[]>([]);
  const [envSel, setEnvSel] = useState<{ name: string; value: string; expand: boolean } | null>(null);
  const loadEnv = useCallback((): void => {
    void ipc.envOverview().then(setEnv).catch(() => setEnv(null));
    void ipc.envBackupList().then(setEnvBackups).catch(() => setEnvBackups([]));
  }, []);
  useEffect(loadEnv, [loadEnv]);

  const saveEnvVar = (): void => {
    if (!envSel) return;
    void wrap(async () => {
      await ipc.envVarSet(envSel.name, envSel.value, envSel.expand);
      pushToast("success", t("otEnvSaved"));
      setEnvSel(null);
      loadEnv();
    });
  };
  const delEnvVar = (name: string): void => {
    void wrap(async () => {
      const ok = await askConfirm({ title: t("otEnvDelTitle"), body: t("otEnvDelBody", { name }), danger: true });
      if (!ok) return;
      await ipc.envVarDelete(name);
      pushToast("success", t("otEnvSaved"));
      loadEnv();
    });
  };
  const restoreEnvBackup = (b: Shell.EnvBackupView): void => {
    void wrap(async () => {
      const ok = await askConfirm({
        title: t("otEnvRestoreTitle"),
        body: t("otEnvRestoreBody", { n: String(b.vars.length), time: fmtTime(b.createdAt) }),
      });
      if (!ok) return;
      const n = await ipc.envRestoreBackup(b.id);
      pushToast("success", t("otEnvRestored", { n: String(n) }));
      loadEnv();
    });
  };

  // ============================== V-83 计划任务工坊 ==============================
  const [sched, setSched] = useState<Shell.SchedTaskView[]>([]);
  const [schedLog, setSchedLog] = useState<string | null>(null);
  const [schedForm, setSchedForm] = useState<{ name: string; action: string; arg: string; ttype: string; hour: number; minute: number; secs: number }>({
    name: "", action: "theme.set", arg: "", ttype: "at", hour: 22, minute: 0, secs: 3600,
  });
  const loadSched = useCallback((): void => {
    void ipc.schedList().then(setSched).catch(() => setSched([]));
  }, []);
  useEffect(loadSched, [loadSched]);

  const buildTrigger = (): Shell.SchedTriggerView => {
    switch (schedForm.ttype) {
      case "at": return { type: "at", hour: schedForm.hour, minute: schedForm.minute };
      case "idle": return { type: "idle", secs: schedForm.secs };
      case "login": return { type: "login", secs: schedForm.secs };
      default: return { type: "interval", secs: schedForm.secs };
    }
  };
  const upsertSched = (): void => {
    if (!schedForm.name.trim()) {
      pushToast("error", t("otSchedNameEmpty"));
      return;
    }
    void wrap(async () => {
      const task: Shell.SchedTaskView = {
        id: `st-${Date.now().toString(36)}`,
        name: schedForm.name.trim(),
        action: schedForm.action,
        arg: schedForm.arg,
        trigger: buildTrigger(),
        enabled: true,
        lastFired: 0,
      };
      const list = await ipc.schedUpsert(task);
      setSched(list);
      setSchedForm({ ...schedForm, name: "", arg: "" });
      pushToast("success", t("otSchedSaved"));
    });
  };
  const toggleSched = (id: string, enabled: boolean): void => {
    void wrap(async () => {
      setSched(await ipc.schedToggle(id, enabled));
    });
  };
  const removeSched = (id: string, name: string): void => {
    void wrap(async () => {
      const ok = await askConfirm({ title: t("otSchedDelTitle"), body: t("otSchedDelBody", { name }), danger: true });
      if (!ok) return;
      setSched(await ipc.schedRemove(id));
    });
  };
  const runSchedNow = (id: string): void => {
    void wrap(async () => {
      await ipc.schedRunNow(id);
      pushToast("success", t("otSchedRan"));
    });
  };
  const showSchedLog = (): void => {
    void wrap(async () => {
      const logs = await ipc.schedLogList();
      setSchedLog(
        logs.length === 0
          ? t("otSchedLogEmpty")
          : logs.map((l) => `${fmtTime(l.ts)} ${l.name} ${l.ok ? "✓" : "✕"} ${l.detail}`).join("\n"),
      );
    });
  };

  // ============================== V-86 启动延迟 ==============================
  const [sdCfg, setSdCfg] = useState<Shell.StartDelayConfigView | null>(null);
  const [sdTimeline, setSdTimeline] = useState<string | null>(null);
  useEffect(() => {
    void ipc.startdelayGet().then(setSdCfg).catch(() => setSdCfg(null));
  }, []);
  const setSdDelay = (name: string, delaySecs: number): void => {
    if (!sdCfg) return;
    const delays = sdCfg.delays.map((d) => (d.name === name ? { ...d, delaySecs } : d));
    setSdCfg({ ...sdCfg, delays });
  };
  const saveSd = (): void => {
    if (!sdCfg) return;
    void wrap(async () => {
      setSdCfg(await ipc.startdelaySet(sdCfg));
      pushToast("success", t("otSdSaved"));
    });
  };
  const showSdTimeline = (): void => {
    void wrap(async () => {
      const tl = await ipc.startdelayTimeline();
      setSdTimeline(
        tl.length === 0
          ? t("otSdTimelineEmpty")
          : tl.map((e) => `${fmtTime(e.bootMs)} +${e.delaySecs}s ${e.name} → ${fmtTime(e.launchedMs)}（${t("otSdTheoretical")}: ${fmtTime(e.theoreticalMs)}, ${e.via}）`).join("\n"),
      );
    });
  };

  // ============================== V-84 关联快照 ==============================
  const [snaps, setSnaps] = useState<Shell.AssocSnapshotView[]>([]);
  const [snapOut, setSnapOut] = useState<string | null>(null);
  const [snapName, setSnapName] = useState("");
  const loadSnaps = useCallback((): void => {
    void ipc.assocSnapshotList().then(setSnaps).catch(() => setSnaps([]));
  }, []);
  useEffect(loadSnaps, [loadSnaps]);

  const takeSnap = (): void => {
    void wrap(async () => {
      const s = await ipc.assocSnapshotTake(snapName.trim() || `snap-${Date.now().toString(36)}`);
      setSnapName("");
      pushToast("success", t("otSnapTaken", { n: String(s.entries.length) }));
      loadSnaps();
    });
  };
  const diffSnaps = (): void => {
    void wrap(async () => {
      const [a, b] = snaps;
      if (!a || !b) return;
      const d = await ipc.assocSnapshotDiff(a.id, b.id);
      setSnapOut(
        d.length === 0
          ? t("otSnapDiffEmpty")
          : d.map((e) => `${e.kind === "add" ? "+" : e.kind === "del" ? "-" : "~"} ${e.ext}: ${e.aProg || "—"} → ${e.bProg || "—"}`).join("\n"),
      );
    });
  };
  const restoreSnap = (s: Shell.AssocSnapshotView): void => {
    void wrap(async () => {
      const [a, b] = snaps;
      let preview = "";
      if (a && b) {
        const d = await ipc.assocSnapshotDiff(s.id, snaps.find((x) => x.id !== s.id)?.id ?? s.id).catch(() => []);
        preview = d.slice(0, 20).map((e) => `${e.ext}: → ${e.bProg}`).join("\n");
      }
      const ok = await askConfirm({
        title: t("otSnapRestoreTitle"),
        body: `${t("otSnapRestoreBody", { name: s.name, n: String(s.entries.length) })}${preview ? `\n${preview}` : ""}`,
      });
      if (!ok) return;
      const [applied] = await ipc.assocSnapshotRestore(s.id);
      pushToast("success", t("otSnapRestored", { n: String(applied) }));
    });
  };
  const removeSnap = (id: string): void => {
    void wrap(async () => {
      await ipc.assocSnapshotRemove(id);
      loadSnaps();
    });
  };

  // ============================== V-85 卸载善后 ==============================
  const [resApp, setResApp] = useState("");
  const [resReport, setResReport] = useState<Shell.ResidueReportView | null>(null);
  const [resChecked, setResChecked] = useState<Set<string>>(new Set());
  const doResScan = (): void => {
    if (!resApp.trim()) return;
    void wrap(async () => {
      const r = await ipc.residueScanApp(resApp.trim());
      setResReport(r);
      setResChecked(new Set());
    });
  };
  const toggleRes = (path: string): void => {
    setResChecked((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };
  const doResDelete = (): void => {
    if (resChecked.size === 0) return;
    void wrap(async () => {
      const paths = [...resChecked];
      const ok = await askConfirm({
        title: t("otResDelTitle"),
        body: t("otResDelBody", { n: String(paths.length) }),
        danger: true,
      });
      if (!ok) return;
      const failed = await ipc.residueDelete(paths);
      pushToast(failed.length === 0 ? "success" : "error", failed.length === 0 ? t("otResDeleted") : t("otResDeleteFail", { n: String(failed.length) }));
      setResChecked(new Set());
      const r = await ipc.residueScanApp(resApp.trim());
      setResReport(r);
    });
  };

  // ============================== V-87 服务依赖图 ==============================
  const [svcNodes, setSvcNodes] = useState<Shell.SvcNodeView[] | null>(null);
  const [svcSel, setSvcSel] = useState("");
  const [svcOut, setSvcOut] = useState<string | null>(null);
  const loadSvc = (): void => {
    void wrap(async () => {
      setSvcNodes(await ipc.svcGraph());
    });
  };
  const showSvcImpact = (name: string): void => {
    setSvcSel(name);
    void wrap(async () => {
      const r = await ipc.svcImpact(name);
      setSvcOut(
        [
          `${t("otSvcTarget")}: ${r.target}`,
          `${t("otSvcDependents")}: ${r.directDependents.join(", ") || "—"}`,
          `${t("otSvcTransitive")}: ${r.transitiveDependents.join(", ") || "—"}`,
          `${t("otSvcDependsOn")}: ${r.targetDependsOn.join(", ") || "—"}`,
          `${t("otSvcStopOrder")}: ${r.suggestedStopOrder.join(" → ") || "—"}`,
        ].join("\n"),
      );
    });
  };

  // ============================== V-89 配置对比 ==============================
  const [cfgA, setCfgA] = useState("{}");
  const [cfgB, setCfgB] = useState("{}");
  const [diffOut, setDiffOut] = useState<Shell.CfgDiffEntryView[] | null>(null);
  const doCfgDiff = (): void => {
    void wrap(async () => {
      setDiffOut(await ipc.cfgDiff(cfgA, cfgB));
    });
  };

  // ============================== V-90 沙盒试用 ==============================
  const [trials, setTrials] = useState<Shell.SandboxTrialView[]>([]);
  const loadTrials = useCallback((): void => {
    void ipc.sandboxTrialList().then(setTrials).catch(() => setTrials([]));
  }, []);
  useEffect(loadTrials, [loadTrials]);
  const endTrial = (kind: string, id: string): void => {
    void wrap(async () => {
      setTrials(await ipc.sandboxTrialEnd(kind, id));
      pushToast("success", t("otTrialEnded"));
    });
  };

  // ============================== 渲染 ==============================
  const envUser = useMemo(() => env?.user ?? [], [env]);
  const pathRows = useMemo(
    () => splitPathRows(envUser.find((v) => v.name === "Path")?.value ?? ""),
    [envUser],
  );

  return (
    <div className="ot-tab" data-testid="ot-tab">
      <p className="ot-desc">{t("otDesc")}</p>

      {/* ---------- M-57 出站桥 ---------- */}
      <section className="ot-group">
        <h4>{t("otWhTitle")}</h4>
        <label className="ot-row">
          <input
            type="checkbox"
            disabled={!whCfg || busy}
            checked={whCfg?.enabled ?? false}
            onChange={(e) => setWhCfg(whCfg ? { ...whCfg, enabled: e.target.checked } : whCfg)}
          />
          <span>
            <span className="ot-label">{t("otWhToggle")}</span>
            <span className="ot-hint">{t("otWhHint")}</span>
          </span>
        </label>
        {whCfg?.rules.map((r, i) => (
          <div key={r.id || i} className="ot-row" data-testid="ot-wh-rule">
            <select className="ot-input" style={{ maxWidth: 170 }} value={r.event} onChange={(e) => patchWhRule(i, { event: e.target.value })}>
              {WEBHOOK_EVENTS.map((ev) => <option key={ev} value={ev}>{ev}</option>)}
            </select>
            <input className="ot-input" value={r.url} onChange={(e) => patchWhRule(i, { url: e.target.value })} placeholder="http://127.0.0.1:9000/hook" />
            <label className="ot-check"><input type="checkbox" checked={r.enabled} onChange={(e) => patchWhRule(i, { enabled: e.target.checked })} />on</label>
            <button type="button" disabled={busy} onClick={() => testWh(r.url)}>{t("otWhTest")}</button>
            <button type="button" disabled={busy} onClick={() => setWhCfg({ ...whCfg, rules: whCfg.rules.filter((_, j) => j !== i) })}>✕</button>
          </div>
        ))}
        <div className="ot-actions">
          <button type="button" disabled={!whCfg || busy} onClick={addWhRule}>{t("otWhAdd")}</button>
          <button type="button" disabled={!whCfg || busy} onClick={saveWh}>{t("otWhSave")}</button>
          <button type="button" disabled={busy} onClick={() => void wrap(async () => {
            const logs = await ipc.webhookLogList(20);
            setWhLog(logs.length === 0 ? t("otWhLogEmpty") : logs.map((l) => `${fmtTime(Number(l.ts))} ${String(l.event)} ${l.ok ? "✓" : "✕"} ${String(l.detail ?? "")}`).join("\n"));
          })}>{t("otWhLog")}</button>
        </div>
        <Result text={whLog} />
      </section>

      {/* ---------- M-63 / M-59 ---------- */}
      <section className="ot-group">
        <h4>{t("otScanTitle")}</h4>
        <div className="ot-actions">
          <button type="button" disabled={busy} onClick={() => void doVxsScan()}>{t("otScanPick")}</button>
        </div>
        <Result text={scanOut} />
        <h4>{t("otEmbTitle")}</h4>
        <span className="ot-hint">{t("otEmbHint")}</span>
        <div className="ot-actions">
          <input className="ot-input" style={{ maxWidth: 340 }} value={embPath} onChange={(e) => setEmbPath(e.target.value)} placeholder="C:\Apps\MyApp\MyApp.exe" />
          <button type="button" disabled={busy || !embPath.trim()} onClick={doEmbScan}>{t("otEmbScan")}</button>
        </div>
        <Result text={embOut} />
      </section>

      {/* ---------- V-81 winget ---------- */}
      <section className="ot-group">
        <h4>{t("otWgTitle")}</h4>
        {wgStatus && !wgStatus.available && <span className="ot-hint">{t("otWgMissing")}</span>}
        {wgStatus?.available && (
          <>
            <div className="ot-actions">
              <input className="ot-input" style={{ maxWidth: 200 }} value={wgQuery} onChange={(e) => setWgQuery(e.target.value)} placeholder={t("otWgQueryPh")} />
              <button type="button" disabled={busy || !wgQuery.trim()} onClick={() => void wrap(async () => {
                setWgPkgs(await ipc.wingetSearch(wgQuery.trim()));
              })}>{t("otWgSearch")}</button>
              <button type="button" disabled={busy} onClick={() => void wrap(async () => {
                setWgPkgs(await ipc.wingetUpgradeList());
              })}>{t("otWgUpgradable")}</button>
              <button type="button" disabled={busy} onClick={() => void wrap(async () => {
                setWgPkgs(await ipc.wingetListInstalled());
              })}>{t("otWgInstalled")}</button>
            </div>
            {wgPkgs && wgPkgs.length > 0 && (
              <div className="ot-list">
                {wgPkgs.slice(0, 50).map((p) => (
                  <div key={`${p.id}-${p.version}`} className="ot-item">
                    <span>
                      {p.name}
                      <span className="ot-sub">{p.id} · {p.version}{p.available && p.available !== p.version ? ` → ${p.available}` : ""} · {p.source}</span>
                    </span>
                    <span className="ot-actions">
                      <button type="button" disabled={busy} onClick={() => wgOp("install", p.id, p.name)}>{t("otWgInstall")}</button>
                      {p.available && p.available !== p.version && (
                        <button type="button" disabled={busy} onClick={() => wgOp("upgrade", p.id, p.name)}>{t("otWgUpgrade")}</button>
                      )}
                      <button type="button" disabled={busy} onClick={() => wgOp("uninstall", p.id, p.name)}>{t("otWgUninstall")}</button>
                    </span>
                  </div>
                ))}
              </div>
            )}
            {wgPkgs && wgPkgs.length === 0 && <span className="ot-hint">{t("otWgEmpty")}</span>}
            <Result text={wgOut} />
          </>
        )}
      </section>

      {/* ---------- V-82 环境变量 ---------- */}
      <section className="ot-group">
        <h4>{t("otEnvTitle")}</h4>
        <span className="ot-hint">{t("otEnvHint")}</span>
        <div className="ot-actions">
          <button type="button" disabled={busy} onClick={() => setEnvSel({ name: "", value: "", expand: false })}>{t("otEnvAdd")}</button>
          <button type="button" disabled={busy} onClick={loadEnv}>{t("otEnvReload")}</button>
        </div>
        {envSel && (
          <div className="ot-actions">
            <input className="ot-input" style={{ maxWidth: 180 }} value={envSel.name} onChange={(e) => setEnvSel({ ...envSel, name: e.target.value })} placeholder="NAME" />
            <input className="ot-input" value={envSel.value} onChange={(e) => setEnvSel({ ...envSel, value: e.target.value })} placeholder="%USERPROFILE%\bin" />
            <label className="ot-check"><input type="checkbox" checked={envSel.expand} onChange={(e) => setEnvSel({ ...envSel, expand: e.target.checked })} />REG_EXPAND_SZ</label>
            <button type="button" disabled={busy || !envSel.name.trim()} onClick={saveEnvVar}>{t("otEnvSave")}</button>
            <button type="button" onClick={() => setEnvSel(null)}>✕</button>
          </div>
        )}
        {pathRows.length > 0 && (
          <div className="ot-list" data-testid="ot-env-path">
            {pathRows.map((row, i) => (
              <div key={`${row}-${i}`} className="ot-item"><span className="ot-mono">{row}</span></div>
            ))}
          </div>
        )}
        {envUser.length > 0 && (
          <div className="ot-list">
            {envUser.slice(0, 80).map((v) => (
              <div key={v.name} className="ot-item">
                <span>
                  {v.name}
                  <span className="ot-sub ot-mono">{v.value}</span>
                </span>
                <span className="ot-actions">
                  <button type="button" disabled={busy} onClick={() => setEnvSel({ name: v.name, value: v.value, expand: v.expand })}>{t("otEnvEdit")}</button>
                  <button type="button" disabled={busy} onClick={() => delEnvVar(v.name)}>{t("otEnvDel")}</button>
                </span>
              </div>
            ))}
          </div>
        )}
        {envBackups.length > 0 && (
          <>
            <span className="ot-hint">{t("otEnvBackups")}</span>
            <div className="ot-list">
              {envBackups.map((b) => (
                <div key={b.id} className="ot-item">
                  <span>{fmtTime(b.createdAt)}<span className="ot-sub">{b.vars.length} vars</span></span>
                  <button type="button" disabled={busy} onClick={() => restoreEnvBackup(b)}>{t("otEnvRestore")}</button>
                </div>
              ))}
            </div>
          </>
        )}
      </section>

      {/* ---------- V-83 计划任务工坊 ---------- */}
      <section className="ot-group">
        <h4>{t("otSchedTitle")}</h4>
        <span className="ot-hint">{t("otSchedHint")}</span>
        <div className="ot-actions">
          <input className="ot-input" style={{ maxWidth: 140 }} value={schedForm.name} onChange={(e) => setSchedForm({ ...schedForm, name: e.target.value })} placeholder={t("otSchedNamePh")} />
          <select className="ot-input" style={{ maxWidth: 140 }} value={schedForm.action} onChange={(e) => setSchedForm({ ...schedForm, action: e.target.value })}>
            {SCHED_ACTIONS.map((a) => <option key={a} value={a}>{a}</option>)}
          </select>
          <input className="ot-input" style={{ maxWidth: 140 }} value={schedForm.arg} onChange={(e) => setSchedForm({ ...schedForm, arg: e.target.value })} placeholder={t("otSchedArgPh")} />
          <select className="ot-input" style={{ maxWidth: 110 }} value={schedForm.ttype} onChange={(e) => setSchedForm({ ...schedForm, ttype: e.target.value })}>
            <option value="at">{t("otSchedTAt")}</option>
            <option value="interval">{t("otSchedTInterval")}</option>
            <option value="idle">{t("otSchedTIdle")}</option>
            <option value="login">{t("otSchedTLogin")}</option>
          </select>
          {schedForm.ttype === "at" ? (
            <>
              <input className="ot-input" style={{ maxWidth: 60 }} type="number" min={0} max={23} value={schedForm.hour} onChange={(e) => setSchedForm({ ...schedForm, hour: Number(e.target.value) })} />
              <span>:</span>
              <input className="ot-input" style={{ maxWidth: 60 }} type="number" min={0} max={59} value={schedForm.minute} onChange={(e) => setSchedForm({ ...schedForm, minute: Number(e.target.value) })} />
            </>
          ) : (
            <input className="ot-input" style={{ maxWidth: 90 }} type="number" min={0} value={schedForm.secs} onChange={(e) => setSchedForm({ ...schedForm, secs: Number(e.target.value) })} placeholder="s" />
          )}
          <button type="button" disabled={busy} onClick={upsertSched}>{t("otSchedAdd")}</button>
        </div>
        {sched.length > 0 && (
          <div className="ot-list">
            {sched.map((task) => (
              <div key={task.id} className="ot-item">
                <span>
                  {task.name}
                  <span className="ot-sub">{task.action} {task.arg} · {triggerSummary(task.trigger)} · {t("otSchedNext")}: {fmtTime(nextFireTime(task.trigger, Date.now(), task.lastFired))}</span>
                </span>
                <span className="ot-actions">
                  <label className="ot-check"><input type="checkbox" checked={task.enabled} onChange={(e) => toggleSched(task.id, e.target.checked)} /></label>
                  <button type="button" disabled={busy} onClick={() => runSchedNow(task.id)}>{t("otSchedRun")}</button>
                  <button type="button" disabled={busy} onClick={() => removeSched(task.id, task.name)}>✕</button>
                </span>
              </div>
            ))}
          </div>
        )}
        <div className="ot-actions">
          <button type="button" disabled={busy} onClick={showSchedLog}>{t("otSchedLog")}</button>
        </div>
        <Result text={schedLog} />
      </section>

      {/* ---------- V-86 启动延迟 ---------- */}
      <section className="ot-group">
        <h4>{t("otSdTitle")}</h4>
        <span className="ot-hint">{t("otSdHint")}</span>
        {sdCfg && (
          <>
            {sdCfg.delays.length > 0 && (
              <div className="ot-list">
                {sdCfg.delays.map((d) => (
                  <div key={d.name} className="ot-item">
                    <span className="ot-mono">{d.name}</span>
                    <span className="ot-actions">
                      <input
                        className="ot-input"
                        style={{ maxWidth: 80 }}
                        type="number"
                        min={0}
                        max={120}
                        value={d.delaySecs}
                        disabled={isSafeStartupMirror(d.name, d.name)}
                        onChange={(e) => setSdDelay(d.name, Math.max(0, Math.min(120, Number(e.target.value))))}
                      />
                      <span className="ot-hint">s</span>
                    </span>
                  </div>
                ))}
              </div>
            )}
            <div className="ot-actions">
              <button type="button" disabled={busy} onClick={saveSd}>{t("otSdSave")}</button>
              <button type="button" disabled={busy} onClick={showSdTimeline}>{t("otSdTimeline")}</button>
            </div>
          </>
        )}
        <Result text={sdTimeline} />
      </section>

      {/* ---------- V-84 / V-85 ---------- */}
      <section className="ot-group">
        <h4>{t("otSnapTitle")}</h4>
        <span className="ot-hint">{t("otSnapHint")}</span>
        <div className="ot-actions">
          <input className="ot-input" style={{ maxWidth: 160 }} value={snapName} onChange={(e) => setSnapName(e.target.value)} placeholder={t("otSnapNamePh")} />
          <button type="button" disabled={busy} onClick={takeSnap}>{t("otSnapTake")}</button>
          <button type="button" disabled={busy || snaps.length < 2} onClick={diffSnaps}>{t("otSnapDiff")}</button>
        </div>
        {snaps.length > 0 && (
          <div className="ot-list">
            {snaps.map((s) => (
              <div key={s.id} className="ot-item">
                <span>{s.name}<span className="ot-sub">{fmtTime(s.createdAt)} · {s.entries.length} {t("otSnapEntries")}</span></span>
                <span className="ot-actions">
                  <button type="button" disabled={busy} onClick={() => restoreSnap(s)}>{t("otSnapRestore")}</button>
                  <button type="button" disabled={busy} onClick={() => removeSnap(s.id)}>✕</button>
                </span>
              </div>
            ))}
          </div>
        )}
        <Result text={snapOut} />
        <h4>{t("otResTitle")}</h4>
        <span className="ot-hint">{t("otResHint")}</span>
        <div className="ot-actions">
          <input className="ot-input" style={{ maxWidth: 200 }} value={resApp} onChange={(e) => setResApp(e.target.value)} placeholder={t("otResAppPh")} />
          <button type="button" disabled={busy || !resApp.trim()} onClick={doResScan}>{t("otResScan")}</button>
          <button type="button" disabled={busy || resChecked.size === 0} onClick={doResDelete}>{t("otResDelete", { n: String(resChecked.size) })}</button>
        </div>
        {resReport && (
          <div className="ot-list">
            <span className="ot-hint">{t("otResTotal")}: {fmtBytes(resReport.totalBytes)} · {resReport.scannedRoots.join(", ")}</span>
            {resReport.hits.map((h) => (
              <label key={h.path} className="ot-item ot-check">
                <span>
                  <input type="checkbox" checked={resChecked.has(h.path)} onChange={() => toggleRes(h.path)} />
                  <span className="ot-mono">{h.path}</span>
                  <span className="ot-sub">{h.kind} · {fmtBytes(h.bytes)}</span>
                </span>
              </label>
            ))}
          </div>
        )}
      </section>

      {/* ---------- V-87 服务依赖图 ---------- */}
      <section className="ot-group">
        <h4>{t("otSvcTitle")}</h4>
        <div className="ot-actions">
          <button type="button" disabled={busy} onClick={loadSvc}>{t("otSvcLoad")}</button>
          {svcNodes && svcNodes.length > 0 && (
            <select className="ot-input" style={{ maxWidth: 260 }} value={svcSel} onChange={(e) => showSvcImpact(e.target.value)}>
              <option value="">—</option>
              {svcNodes.slice(0, 300).map((n) => <option key={n.name} value={n.name}>{n.display}</option>)}
            </select>
          )}
        </div>
        {svcNodes && svcOut === null && (
          <div className="ot-list">
            {svcNodes.filter((n) => n.dependsOn.length > 0).slice(0, 30).map((n) => (
              <div key={n.name} className="ot-item">
                <span>{n.display}<span className="ot-sub">{n.name} → {n.dependsOn.join(", ")}</span></span>
              </div>
            ))}
          </div>
        )}
        <Result text={svcOut} />
      </section>

      {/* ---------- V-89 / V-90 ---------- */}
      <section className="ot-group">
        <h4>{t("otDiffTitle")}</h4>
        <span className="ot-hint">{t("otDiffHint")}</span>
        <div className="ot-actions">
          <textarea className="ot-input" style={{ maxWidth: 320, height: 72 }} value={cfgA} onChange={(e) => setCfgA(e.target.value)} />
          <textarea className="ot-input" style={{ maxWidth: 320, height: 72 }} value={cfgB} onChange={(e) => setCfgB(e.target.value)} />
          <button type="button" disabled={busy} onClick={doCfgDiff}>{t("otDiffRun")}</button>
        </div>
        {diffOut && (
          <div className="ot-list" data-testid="ot-diff-list">
            {diffOut.length === 0 && <span className="ot-hint">{t("otDiffEmpty")}</span>}
            {diffOut.map((d, i) => (
              <div key={`${d.path}-${i}`} className="ot-item">
                <span className={diffKindClass(d.kind)}>{d.kind === "add" ? "+" : d.kind === "del" ? "-" : "~"} {d.path}</span>
                <span className="ot-sub ot-mono">{JSON.stringify(d.a)} → {JSON.stringify(d.b)}</span>
              </div>
            ))}
          </div>
        )}
        <h4>{t("otTrialTitle")}</h4>
        <span className="ot-hint">{t("otTrialHint")}</span>
        {trials.length > 0 && (
          <div className="ot-list">
            {trials.map((tr) => (
              <div key={`${tr.kind}-${tr.id}`} className="ot-item">
                <span>{tr.kind}: {tr.id}<span className="ot-sub">{fmtTime(tr.startedAt)} · prev={tr.prevValue || "—"}</span></span>
                <button type="button" disabled={busy} onClick={() => endTrial(tr.kind, tr.id)}>{t("otTrialApply")}</button>
              </div>
            ))}
          </div>
        )}
        {trials.length === 0 && <span className="ot-hint">{t("otTrialEmpty")}</span>}
      </section>

      {/* ---------- M-55/56/60/61/62 工具链 ---------- */}
      <section className="ot-group">
        <h4>{t("otDocTitle")}</h4>
        <span className="ot-hint">{t("otDocHint")}</span>
      </section>
    </div>
  );
}
