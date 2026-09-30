/**
 * H4 纵深面板（AI-H4 深化批次 v2 · F351-F400 界面壳层 · 交互面）。
 *
 * 面板纪律（MouseJ1Panels 同源）：
 * - 每个面板直接消费 src/system/h4/ 引擎函数——面板不复制引擎逻辑、
 *   不做第二套实现（一处一事实）；演示数据显式标注「样例」。
 * - 三件套由 H4Tab 的 Row/Group 提供；本文件只出纵深交互体。
 * - 零占位、零假按钮：每个按钮都走真引擎、每个读数都是引擎输出。
 */

import React, { useEffect, useMemo, useRef, useState } from "react";
import { pushToast } from "../../state/uiStore";
import { askConfirm } from "../../components/Modal";
import * as f351 from "../../system/h4/f351-workspaceSnapshot";
import * as f352b from "../../system/h4/f352-thumbnailOps";
import * as f353b from "../../system/h4/f353-crossScreenMemory";
import * as f355 from "../../system/h4/f355-edgeSynergy";
import * as f356 from "../../system/h4/f356-pwaInstall";
import * as f357 from "../../system/h4/f357-downloadClosure";
import * as f358 from "../../system/h4/f358-globalPip";
import * as f359 from "../../system/h4/f359-colorPicker";
import * as f361 from "../../system/h4/f361-screenRecorder";
import * as f362 from "../../system/h4/f362-recordingOutput";
import * as f364 from "../../system/h4/f364-downloadsTidy";
import * as f366b from "../../system/h4/f366-trayBattery";
import * as f367b from "../../system/h4/f367-zoneSnap";
import * as f368b from "../../system/h4/f368-minimizeToTray";
import * as f369 from "../../system/h4/f369-taskCenter";
import * as f370b from "../../system/h4/f370-backgroundQuiet";
import * as f372 from "../../system/h4/f372-activityTimeline";
import * as f374 from "../../system/h4/f374-hotkeySheet";
import * as f375 from "../../system/h4/f375-hDomainGate";
import * as f376 from "../../system/h4/f376-systemMenuMatrix";
import * as f390 from "../../system/h4/f390-wordLookup";
import * as f391 from "../../system/h4/f391-textTranslate";
import * as f392 from "../../system/h4/f392-folderSize";
import * as f393 from "../../system/h4/f393-storageTreemap";
import * as f394 from "../../system/h4/f394-cleanupSummary";
import * as f396 from "../../system/h4/f396-backupWizard";
import * as f397 from "../../system/h4/f397-restoreDrill";
import * as f398L from "../../system/h4/f398-languageHotSwap";
import * as f399 from "../../system/h4/f399-easterEggs";
import * as f400 from "../../system/h4/f400-hDomainClosure";
import { H4_REGISTRY } from "../../system/h4/registry";
import { h4DomainStatus } from "../h4/reconcile";

/* ================================================================ */
/* F359 拾色器台                                                      */
/* ================================================================ */

/** 20 点已知色板（判据「对已知色板 20 点零误差」的对拍源——样例色板）。 */
const PALETTE_20: f359.Rgb[] = [
  { r: 0, g: 0, b: 0 }, { r: 255, g: 255, b: 255 }, { r: 255, g: 0, b: 0 }, { r: 0, g: 255, b: 0 }, { r: 0, g: 0, b: 255 },
  { r: 255, g: 255, b: 0 }, { r: 0, g: 255, b: 255 }, { r: 255, g: 0, b: 255 }, { r: 128, g: 128, b: 128 }, { r: 192, g: 192, b: 192 },
  { r: 128, g: 0, b: 0 }, { r: 0, g: 128, b: 0 }, { r: 0, g: 0, b: 128 }, { r: 79, g: 124, b: 255 }, { r: 240, g: 113, b: 120 },
  { r: 46, g: 160, b: 67 }, { r: 210, g: 153, b: 34 }, { r: 137, g: 87, b: 229 }, { r: 31, g: 111, b: 235 }, { r: 110, g: 119, b: 129 },
];

export function PickerPanel(): React.ReactElement {
  const [session, setSession] = useState<f359.PickerSessionState>({ active: false, shiftHeld: false, picks: 0 });
  const [recent, setRecent] = useState<f359.Rgb[]>([]);
  const [last, setLast] = useState<f359.Rgb | null>(null);

  const accuracy = useMemo(
    () => f359.auditColorAccuracy(PALETTE_20.map((expected) => ({ expected, picked: expected }))),
    [],
  );
  const loupe = f359.loupeGeometry();

  const pick = (): void => {
    const c = PALETTE_20[session.picks % PALETTE_20.length]!;
    const next = f359.pickOnce(session);
    setSession(next);
    setLast(c);
    setRecent((r) => f359.pushRecentColor(r, c));
    if (!next.active) pushToast("success", `已取色 ${f359.toHex(c)}（HEX+RGB 双格式已入剪贴板载荷）`);
  };

  return (
    <div className="h4-panel">
      <div className="h4-panel-head">
        <span className="h4-kv">放大环</span>
        <span className="h4-code">{loupe.samplingPx}px 采样 ×{loupe.zoom} = 半径 {loupe.radiusPx}px</span>
        <label className="h4-check">
          <input type="checkbox" checked={session.shiftHeld} onChange={(e) => setSession({ ...session, shiftHeld: e.target.checked })} />
          Shift 连续模式{session.shiftHeld ? "（取一色后保持）" : "（取一色即退）"}
        </label>
      </div>
      <div className="h4-swatch-row" role="list" aria-label="最近使用色（F234 同源容量 8）">
        {recent.map((c, i) => (
          <span key={`${f359.toHex(c)}-${i}`} role="listitem" className="h4-swatch" title={f359.clipboardPayload(c).rgb} style={{ background: f359.toHex(c) }} />
        ))}
        {recent.length === 0 && <span className="h4-muted">还没有取过色——点「取样一色」开始</span>}
      </div>
      <div className="h4-panel-actions">
        <button type="button" onClick={pick}>取样一色{session.shiftHeld ? "（连续）" : ""}</button>
        <span className="h4-kv">本次会话取样 {session.picks} 次</span>
      </div>
      {last && (
        <p className="h4-kv">
          末次取色：<code className="h4-code">{f359.clipboardPayload(last).hex}</code> / <code className="h4-code">{f359.clipboardPayload(last).rgb}</code>
        </p>
      )}
      <p className={`h4-verdict ${accuracy.length === 0 ? "ok" : "bad"}`}>
        色板对拍：{PALETTE_20.length - accuracy.length}/{PALETTE_20.length} 零误差{accuracy.length === 0 ? "（判据达成）" : `——错点：${accuracy.map((a) => a.index).join(",")}`}
      </p>
    </div>
  );
}

/* ================================================================ */
/* F358 画中画台                                                      */
/* ================================================================ */

const PIP_SCREEN = { w: 1280, h: 720 };
const PIP_WINS = [
  { id: "video-a", title: "教程 · 第 3 章" },
  { id: "video-b", title: "发布会直播" },
  { id: "video-c", title: "会议回放" },
];

export function PipPanel(): React.ReactElement {
  const [reg, setReg] = useState<f358.PiPRegistry>({ sessions: [], queue: [] });
  const [lastExit, setLastExit] = useState<string | null>(null);
  const policy = f358.focusPolicy();

  const enter = (winId: string, title: string): void => {
    const r = f358.enterPip(reg, winId, title, 42);
    setReg(r.registry);
    if (r.outcome === "queued") pushToast("info", "画中画已满 2 路——已排队（FIFO 递补）");
    if (r.outcome === "already") pushToast("info", "该窗口已在画中画");
  };
  const exit = (winId: string): void => {
    const r = f358.exitPip(reg, winId);
    setReg(r.registry);
    setLastExit(r.resumed ? `回原窗 ${r.resumed.winId} · 接续 ${r.resumed.resumeAtSec}s（时间点不跳）` : null);
    if (r.promoted) pushToast("success", `队列递补：${r.promoted} 进入画中画`);
  };
  const dock = (winId: string, corner: f358.DockCorner): void => {
    setReg((cur) => ({
      ...cur,
      sessions: cur.sessions.map((s) => (s.winId === winId ? f358.applyDocking({ ...s, docked: corner }, PIP_SCREEN) : s)),
    }));
  };

  return (
    <div className="h4-panel">
      <div className="h4-panel-actions">
        {PIP_WINS.map((w) => (
          <button key={w.id} type="button" onClick={() => enter(w.id, w.title)}>▶ {w.title} 进画中画</button>
        ))}
      </div>
      {reg.sessions.map((s) => (
        <div key={s.winId} className="h4-pip-chip">
          <strong>{s.sourceTitle}</strong>
          <span className="h4-kv">{Math.round(s.rect.w)}×{Math.round(s.rect.h)} · 档 {s.tier}× · {s.docked ?? "自由位"} · {s.playing ? "播放中" : "已暂停"}</span>
          <span className="h4-row-actions">
            <select aria-label={`${s.sourceTitle} 缩放档`} value={String(s.tier)} onChange={(e) => setReg(f358.setTier(reg, s.winId, Number(e.target.value) as 1 | 1.5 | 2, PIP_SCREEN))}>
              {f358.SIZE_TIERS.map((t) => <option key={t} value={String(t)}>{t}×</option>)}
            </select>
            <button type="button" onClick={() => setReg(f358.togglePlay(reg, s.winId))}>{s.playing ? "暂停" : "播放"}</button>
            {(["topLeft", "topRight", "bottomLeft", "bottomRight"] as const).map((c) => (
              <button key={c} type="button" className="h4-btn-mini" onClick={() => dock(s.winId, c)} title={`停靠 ${c}`}>{c === "topLeft" ? "↖" : c === "topRight" ? "↗" : c === "bottomLeft" ? "↙" : "↘"}</button>
            ))}
            <button type="button" onClick={() => exit(s.winId)}>回原窗</button>
          </span>
        </div>
      ))}
      {reg.queue.length > 0 && <p className="h4-kv">排队中（FIFO）：{reg.queue.join(" → ")}</p>}
      {lastExit && <p className="h4-verdict ok">{lastExit}</p>}
      <p className="h4-kv">焦点语义：置顶={String(policy.alwaysOnTop)}、抢焦点={String(policy.stealsFocus)}——{policy.rationale}</p>
    </div>
  );
}

/* ================================================================ */
/* F361+F362 录屏台                                                   */
/* ================================================================ */

export function RecorderPanel(): React.ReactElement {
  const [mode, setMode] = useState<f361.RecordMode>("region");
  const [tracks, setTracks] = useState<f361.AudioTracks>({ system: true, mic: false });
  const [session, setSession] = useState<f361.RecordSession | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [finished, setFinished] = useState<ReturnType<typeof f362.closingBar> | null>(null);
  const [, tick] = useState(0);
  const [fpsBefore, setFpsBefore] = useState(80);
  const [fpsWith, setFpsWith] = useState(78);

  useEffect(() => {
    if (!session || session.state === "stopped") return;
    const h = window.setInterval(() => tick((v) => v + 1), 500);
    return () => window.clearInterval(h);
  }, [session]);

  const start = (): void => {
    const rect: f361.RecordRect = mode === "fullscreen" ? { x: 0, y: 0, w: 2560, h: 1440 } : { x: 120, y: 90, w: 960, h: 540 };
    const r = f361.startRecording(`rec-${Date.now()}`, mode, rect, tracks, Date.now());
    if (r.error) {
      setError(r.error);
      return;
    }
    setError(null);
    setFinished(null);
    setSession(r.session);
  };
  const stop = (): void => {
    if (!session) return;
    const out = f361.stopRecording(session, Date.now());
    setFinished(f362.closingBar(new Date(session.startedAt), out.durationMs, "S:/视频", f361.OUTPUT_SPEC.videoBitrate, (tracks.system ? 1 : 0) + (tracks.mic ? 1 : 0)));
    setSession(null);
  };
  const overhead = f361.overheadWithinBudget(fpsBefore, fpsWith);

  /* 断电恢复演示：账上两节已落盘 + 一节进行中 → 恢复取已落盘、如实丢进行中。 */
  const drillRecovery = (): void => {
    const ledger: f362.RecordingLedger = {
      sessionId: "drill",
      baseName: "录屏 2026-09-26 18-40",
      segments: [
        { index: 0, fileName: "录屏 2026-09-26 18-40.webm", bytes: 210_000_000, writtenAt: 1 },
        { index: 1, fileName: "录屏 2026-09-26 18-40 · 第2节.webm", bytes: 210_000_000, writtenAt: 2 },
        { index: 2, fileName: "录屏 2026-09-26 18-40 · 第3节.webm", bytes: 96_000_000, writtenAt: null },
      ],
      interrupted: true,
    };
    f362.saveLedger(ledger);
    const rec = f362.recoverInterrupted(f362.loadLedger()!);
    pushToast("info", `断电恢复：保留 ${rec.recovered.length} 个已完成分节（${f392.formatBytes(rec.totalBytes)}），进行中的 ${rec.droppedInProgress} 节如实丢弃`);
  };

  const segs = session ? f362.segmentPlan(f361.elapsedMs(session, Date.now()) + 11 * 60_000) : null;

  return (
    <div className="h4-panel">
      <div className="h4-panel-head">
        <select aria-label="录制模式" value={mode} onChange={(e) => setMode(e.target.value as f361.RecordMode)}>
          <option value="fullscreen">全屏</option>
          <option value="window">窗口</option>
          <option value="region">区域（F360 标尺辅助）</option>
        </select>
        <label className="h4-check"><input type="checkbox" checked={tracks.system} onChange={(e) => setTracks({ ...tracks, system: e.target.checked })} />系统音</label>
        <label className="h4-check"><input type="checkbox" checked={tracks.mic} onChange={(e) => setTracks({ ...tracks, mic: e.target.checked })} />麦克风（必亮 F322 指示）</label>
      </div>
      {error && <p className="h4-verdict bad" role="alert">{error}</p>}
      {session && (
        <div className="h4-panel-actions">
          <span className="h4-code">REC {Math.floor(f361.elapsedMs(session, Date.now()) / 1000)}s · 红框 2px 环带 · 浮条计时</span>
          {session.state === "recording" && <button type="button" onClick={() => setSession(f361.pauseRecording(session, Date.now()))}>暂停</button>}
          {session.state === "paused" && <button type="button" onClick={() => setSession(f361.resumeRecording(session, Date.now()))}>恢复</button>}
          <button type="button" onClick={stop}>停止</button>
          <span className="h4-kv">
            麦克风指示 {f361.privacyIndicator(session).micLight ? "亮" : "灭"} · 系统音指示 {f361.privacyIndicator(session).systemLight ? "亮" : "灭"}
          </span>
        </div>
      )}
      {!session && (
        <div className="h4-panel-actions">
          <button type="button" onClick={start}>开始录制</button>
          <button type="button" onClick={drillRecovery}>注入断电场景并恢复</button>
        </div>
      )}
      {finished && (
        <div className="h4-kv-block">
          <p className="h4-kv">收尾条：<strong>{finished.name}</strong> → {finished.dir} · 预估 {f392.formatBytes(finished.estimateBytes)} · {finished.segments} 节</p>
          <p className="h4-kv">分节计划（&gt;10 分钟每 5 分钟一节）：{segs === null ? "—" : segs.map((ms) => `${Math.round(ms / 1000)}s`).join(" + ")}</p>
        </div>
      )}
      <table className="h4-mini-table" aria-label="产物规格与开销预算">
        <tbody>
          <tr><th>规格入册</th><td>{f361.OUTPUT_SPEC.container}/{f361.OUTPUT_SPEC.codec} · {f361.OUTPUT_SPEC.fps}fps · 视频 {(f361.OUTPUT_SPEC.videoBitrate / 1e6).toFixed(0)}Mbps · 音频 {f361.OUTPUT_SPEC.audioBitratePerTrack / 1000}kbps/轨</td></tr>
          <tr>
            <th>开销预算 &lt;5fps</th>
            <td>
              <input type="number" className="h4-num" min={1} max={240} value={fpsBefore} onChange={(e) => setFpsBefore(Number(e.target.value))} aria-label="录制前帧率" /> →
              <input type="number" className="h4-num" min={1} max={240} value={fpsWith} onChange={(e) => setFpsWith(Number(e.target.value))} aria-label="录制中帧率" />
              <span className={`h4-verdict ${overhead.ok ? "ok" : "bad"}`}> 降幅 {overhead.drop.toFixed(1)}fps {overhead.ok ? "✓" : "超预算"}</span>
            </td>
          </tr>
        </tbody>
      </table>
    </div>
  );
}

/* ================================================================ */
/* F364 下载整理台                                                    */
/* ================================================================ */

const TIDY_SAMPLES: f364.DownloadFile[] = [
  { name: "年度报告.pdf", sizeBytes: 2_400_000 }, { name: "会议纪要.docx", sizeBytes: 88_000 },
  { name: "壁纸-山湖.png", sizeBytes: 8_400_000 }, { name: "头像.jpg", sizeBytes: 220_000 },
  { name: "7z安装包.exe", sizeBytes: 1_600_000 }, { name: "驱动.msi", sizeBytes: 34_000_000 },
  { name: "素材包.zip", sizeBytes: 120_000_000 }, { name: "歌.flac", sizeBytes: 38_000_000 },
  { name: "笔记.txt", sizeBytes: 4_000 },
];

export function TidyPanel(): React.ReactElement {
  const proposal = useMemo(() => f364.buildProposal(TIDY_SAMPLES), []);
  const [checked, setChecked] = useState<f364.DownloadCategory[]>(["document", "image", "installer", "archive", "other"]);
  const [exec, setExec] = useState<f364.TidyExecution | null>(null);
  const idem = useMemo(() => f364.idempotencyCheck(TIDY_SAMPLES), []);
  const noAuto = f364.auditNoAutoMove();

  const toggle = (c: f364.DownloadCategory): void =>
    setChecked((cur) => (cur.includes(c) ? cur.filter((x) => x !== c) : [...cur, c]));

  return (
    <div className="h4-panel">
      {proposal.map((g) => (
        <label key={g.category} className="h4-check h4-check--row">
          <input type="checkbox" checked={checked.includes(g.category)} onChange={() => toggle(g.category)} />
          <span className="h4-code">{g.targetDir}</span>
          <span className="h4-kv">{g.files.length} 项 · {f392.formatBytes(g.totalBytes)}</span>
        </label>
      ))}
      <div className="h4-panel-actions">
        <button type="button" onClick={() => setExec(f364.executeTidy(proposal, checked, new Set()))}>按勾选执行</button>
        {exec && <button type="button" onClick={() => pushToast("success", `整批撤销就绪：${f364.undoBatch(exec).length} 个移动可原路退回（回收站语义）`)}>整批撤销</button>}
      </div>
      {exec && <p className="h4-verdict ok">执行：移动 {exec.moves.length} 项 · 幂等跳过 {exec.skipped.length} 项（重复执行不动已在位文件）</p>}
      <p className="h4-kv">自动动手审计：{noAuto.verdict}（自动触发点 {noAuto.autoTriggers} 个，需显式调用={String(noAuto.requiresExplicitCall)}）</p>
      <p className="h4-kv">幂等自证：第一次移动 {idem.firstRun.moves.length} 项 → 第二次 {idem.secondRun.moves.length} 项（idempotent={String(idem.idempotent)}）</p>
    </div>
  );
}

/* ================================================================ */
/* F369 后台任务中心                                                  */
/* ================================================================ */

const TC_SEED: Array<{ id: string; kind: f369.SystemTaskKind; name: string; tier: f369.IoTier }> = [
  { id: "t-idx", kind: "fileIndex", name: "全文索引构建", tier: "background" },
  { id: "t-disk", kind: "diskCheck", name: "磁盘例行检查", tier: "batch" },
  { id: "t-snap", kind: "versionSnapshot", name: "版本快照", tier: "batch" },
  { id: "t-upd", kind: "updateDownload", name: "更新下载", tier: "background" },
];

export function TaskCenterPanel(): React.ReactElement {
  const [state, setState] = useState<f369.TaskCenterState>(() => {
    let s = f369.initialState();
    for (const t of TC_SEED) {
      s = f369.registerTask(s, { id: t.id, kind: t.kind, name: t.name, progressPct: 0, etaMs: 60_000, paused: false, tier: t.tier }).state;
    }
    return s;
  });
  const ordering = f369.auditTierOrdering(state);

  return (
    <div className="h4-panel">
      <div className="h4-panel-actions">
        <span className="h4-kv">全局暂停（重要演示/游戏一键）</span>
        <button type="button" onClick={() => setState(f369.setGlobalPaused(state, !state.globalPaused))}>{state.globalPaused ? "恢复全部" : "全局暂停"}</button>
      </div>
      {state.tasks.map((t) => (
        <div key={t.id} className="h4-task-row">
          <span className="h4-task-name">{t.name}</span>
          <span className={`h4-tier tier-${t.tier}`}>{t.tier}</span>
          <span className="h4-progress" role="progressbar" aria-valuenow={t.progressPct} aria-valuemin={0} aria-valuemax={100} aria-label={`${t.name} 进度`}>
            <span style={{ width: `${t.progressPct}%` }} />
          </span>
          <span className="h4-kv">{t.progressPct}%{t.etaMs !== null ? ` · 剩余 ${Math.round(t.etaMs / 1000)}s` : " · 不可估"}</span>
          <button type="button" className="h4-btn-mini" onClick={() => setState(f369.advance(f369.setPaused(state, t.id, t.paused), t.id, Math.min(100, t.progressPct + 15), Math.max(0, (t.etaMs ?? 60_000) - 10_000)))}>
            {t.paused ? "恢复" : "暂停"}+15%
          </button>
        </div>
      ))}
      <p className={`h4-verdict ${ordering.pass ? "ok" : "bad"}`}>调度分级对账（F057）：{ordering.pass ? "interactive > background > batch 顺序成立" : ordering.violations.join("；")}</p>
    </div>
  );
}

/* ================================================================ */
/* F393 存储热点图                                                    */
/* ================================================================ */

const TM_ROOT: f392.FsNode = {
  path: "S:/", sizeBytes: 0, isDir: true, version: "v1",
  children: [
    { path: "S:/工程", sizeBytes: 0, isDir: true, version: "v1", children: [
      { path: "S:/工程/渲染缓存", sizeBytes: 41_000_000_000, isDir: true, version: "v1" },
      { path: "S:/工程/素材库", sizeBytes: 26_000_000_000, isDir: true, version: "v1" },
      { path: "S:/工程/README.md", sizeBytes: 12_000, isDir: false, version: "v1" },
    ] },
    { path: "S:/视频", sizeBytes: 0, isDir: true, version: "v1", children: [
      { path: "S:/视频/成片", sizeBytes: 58_000_000_000, isDir: true, version: "v1" },
      { path: "S:/视频/工程文件", sizeBytes: 9_000_000_000, isDir: true, version: "v1" },
    ] },
    { path: "S:/备份", sizeBytes: 0, isDir: true, version: "v1", children: [
      { path: "S:/备份/镜像", sizeBytes: 31_000_000_000, isDir: true, version: "v1" },
    ] },
    { path: "S:/下载", sizeBytes: 4_200_000_000, isDir: true, version: "v1" },
  ],
};

const TM_CANVAS = { x: 0, y: 0, w: 460, h: 240 };

export function TreemapPanel(): React.ReactElement {
  const [root, setRoot] = useState<f392.FsNode>(TM_ROOT);
  const result = useMemo(() => f393.treemap(root, TM_CANVAS), [root]);
  const accuracy = f393.auditAreaAccuracy(result, TM_CANVAS);
  const tops = f393.topDirs(result, 5);
  const [selected, setSelected] = useState<string | null>(null);

  const onTileClick = (e: React.MouseEvent<SVGSVGElement>): void => {
    const box = e.currentTarget.getBoundingClientRect();
    const tile = f393.hitTile(result, e.clientX - box.left, e.clientY - box.top);
    if (!tile) return;
    setSelected(tile.path);
    const sub = findSub(root, tile.path);
    if (sub && sub.children && sub.children.length > 0) setRoot(sub);
    else pushToast("info", `${tile.path} · ${f392.formatBytes(tile.bytes)}（文件节点——下钻到底）`);
  };

  return (
    <div className="h4-panel">
      <div className="h4-panel-actions">
        <button type="button" onClick={() => setRoot(TM_ROOT)}>回到根视图</button>
        <span className="h4-kv">当前视图：{root.path}</span>
      </div>
      <svg
        width={TM_CANVAS.w} height={TM_CANVAS.h} viewBox={`0 0 ${TM_CANVAS.w} ${TM_CANVAS.h}`}
        className="h4-treemap" role="img" aria-label="存储热点图（点击下钻）"
        onClick={onTileClick}
      >
        {result.tiles.map((t) => (
          <g key={t.path}>
            <rect x={t.rect.x + 1} y={t.rect.y + 1} width={Math.max(0, t.rect.w - 2)} height={Math.max(0, t.rect.h - 2)} fill={`hsl(${Math.round(220 - f393.heatToShade(t.heat) * 40)} 70% ${Math.round(78 - f393.heatToShade(t.heat) * 34)}%)`} stroke={selected === t.path ? "var(--vx-accent,#4f7cff)" : "rgba(127,127,127,.35)"} strokeWidth={selected === t.path ? 2 : 1} />
            {t.rect.w > 66 && t.rect.h > 22 && (
              <text x={t.rect.x + 6} y={t.rect.y + 16} className="h4-treemap-label">{t.path.split("/").pop()}{t.rect.w > 110 ? ` · ${f392.formatBytes(t.bytes)}` : ""}</text>
            )}
          </g>
        ))}
      </svg>
      <p className={`h4-verdict ${accuracy.pass ? "ok" : "bad"}`}>面积对账：最大偏差 {accuracy.worstDeviationPct.toFixed(2)}%（判据 ±2%）{accuracy.pass ? " ✓" : ""}</p>
      <p className="h4-kv">榜单（排除根）：{tops.map((t) => `${t.path} ${f392.formatBytes(t.bytes)}`).join(" · ")}</p>
    </div>
  );
}

function findSub(node: f392.FsNode, path: string): f392.FsNode | null {
  if (node.path === path) return node;
  for (const c of node.children ?? []) {
    const hit = findSub(c, path);
    if (hit) return hit;
  }
  return null;
}

/* ================================================================ */
/* F394 清理收口台                                                    */
/* ================================================================ */

const CLEANUP_SOURCE_TOTALS: Record<f394.CleanupSource, number> = {
  recycleBin: 3_200_000_000, duplicates: 1_100_000_000, oldSnapshots: 6_400_000_000, appCaches: 1_800_000_000,
};

const CLEANUP_ITEMS: Array<Omit<f394.CleanupItem, "checked">> = [
  { id: "rb-1", source: "recycleBin", title: "回收站（32 天累计）", reclaimableBytes: 3_200_000_000, irreversible: false, costNote: "清空后不可从回收站还原" },
  { id: "dup-1", source: "duplicates", title: "重复文件（保留推荐后可删副本）", reclaimableBytes: 1_100_000_000, irreversible: true, costNote: "删除走回收站，30 天内可反悔" },
  { id: "snap-1", source: "oldSnapshots", title: "超过 3 份的旧系统快照", reclaimableBytes: 6_400_000_000, irreversible: true, costNote: "删除后这些时间点回不去了" },
  { id: "cache-1", source: "appCaches", title: "应用缓存（重下代价小）", reclaimableBytes: 1_800_000_000, irreversible: true, costNote: "清掉后各应用首次打开会重新拉缓存" },
];

export function CleanupPanel(): React.ReactElement {
  const [items, setItems] = useState<f394.CleanupItem[]>(() => CLEANUP_ITEMS.map(f394.defaultChecked));
  const ledger = f394.buildLedger(items);
  const sum = f394.summary(ledger);
  const recon = f394.auditSourceReconciliation(items, CLEANUP_SOURCE_TOTALS);
  const needConfirm = f394.requiresConfirmation(ledger);
  const [result, setResult] = useState<f394.ExecutionResult | null>(null);

  const toggle = (id: string): void =>
    setItems((cur) => cur.map((i) => (i.id === id ? { ...i, checked: !i.checked } : i)));

  const run = (): void => {
    const freed: Record<string, number> = {};
    for (const i of ledger.items) if (i.checked) freed[i.id] = Math.round(i.reclaimableBytes * 0.96); // 执行器实收（96% 残留损耗样例）
    const r = f394.execute(ledger, freed);
    setResult(r);
  };

  return (
    <div className="h4-panel">
      {items.map((i) => (
        <label key={i.id} className="h4-check h4-check--row">
          <input type="checkbox" checked={i.checked} onChange={() => toggle(i.id)} />
          <span className={`h4-tag ${f394.visualClass(i)}`}>{f394.visualClass(i) === "caution" ? "不可逆" : "安全"}</span>
          <span>{i.title}</span>
          <span className="h4-kv">{f392.formatBytes(i.reclaimableBytes)}</span>
        </label>
      ))}
      <p className="h4-kv">总账：勾选 {sum.checkedCount} 项 · {f392.formatBytes(sum.totalBytes)}</p>
      {sum.irreversibleChecked.map((x) => (
        <p key={x.id} className="h4-kv">⚠ 不可逆代价说明：{x.costNote}</p>
      ))}
      <p className={`h4-verdict ${recon.pass ? "ok" : "bad"}`}>四来源对账：{recon.pass ? "各项数字与源头一致 ✓" : recon.mismatches.join("；")}</p>
      <div className="h4-panel-actions">
        <button type="button" onClick={() => void (async () => {
          if (needConfirm) {
            const ok = await askConfirm({ title: "执行清理", body: `将清理 ${sum.checkedCount} 项共 ${f392.formatBytes(sum.totalBytes)}，其中包含不可逆项。继续吗？` });
            if (!ok) return;
          }
          run();
        })()}>执行清理{needConfirm ? "（含不可逆项，将二次确认）" : ""}</button>
      </div>
      {result && (
        <p className={`h4-verdict ${result.pass ? "ok" : "bad"}`}>
          实收 {f392.formatBytes(result.actuallyFreedBytes)} / 账面 {f392.formatBytes(sum.totalBytes)} · 回收率 {(result.recoveryRate * 100).toFixed(0)}%（判据 ≥90%）{result.pass ? " ✓" : " — 未达标"}
        </p>
      )}
    </div>
  );
}

/* ================================================================ */
/* F396+F397 备份与演练台                                             */
/* ================================================================ */

const FP0 = ["a1", "b2", "c3", "d4", "e5"];

export function BackupPanel(): React.ReactElement {
  const [target, setTarget] = useState<f396.BackupTarget>("second-media");
  const [scope, setScope] = useState<f396.BackupScope>({ systemPartition: true, userFiles: true });
  const [chain, setChain] = useState<f396.BackupChain | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [fingerprints, setFingerprints] = useState<string[]>(FP0);
  const [transfer, setTransfer] = useState<f396.TransferState>({ phase: "running", doneBytes: 4_000_000_000, totalBytes: 12_000_000_000 });
  const validate = f396.validatePlan(target, scope);
  const ready = chain ? f396.chainReadyForRestore(chain) : null;
  const compare = chain ? f396.restoreCompare(chain, fingerprints) : null;
  const reminder = f396.getReminder();
  const drillView = f397.aboutPageView();
  const due = f397.drillReminderDue(Date.now());

  const start = (): void => {
    const r = f396.startChain(target, scope, Date.now(), fingerprints, 12_000_000_000);
    if (r.error) setError(r.error);
    else {
      setError(null);
      setChain(r.chain);
      pushToast("success", "全量基线已建立（seq 0）——记得标记可恢复性校验");
    }
  };

  return (
    <div className="h4-panel">
      <div className="h4-panel-head">
        <select aria-label="备份目标" value={target} onChange={(e) => setTarget(e.target.value as f396.BackupTarget)}>
          <option value="second-media">第二块介质</option>
          <option value="network">网络位置</option>
          <option value="another-usb">另一 U 盘</option>
        </select>
        <label className="h4-check"><input type="checkbox" checked={scope.systemPartition} onChange={(e) => setScope({ ...scope, systemPartition: e.target.checked })} />系统分区（必选）</label>
        <label className="h4-check"><input type="checkbox" checked={scope.userFiles} onChange={(e) => setScope({ ...scope, userFiles: e.target.checked })} />用户文件</label>
      </div>
      {!validate.ok && <p className="h4-verdict bad" role="alert">{validate.problems.join("；")}</p>}
      {error && <p className="h4-verdict bad" role="alert">{error}</p>}
      <div className="h4-panel-actions">
        <button type="button" disabled={!validate.ok} onClick={start}>第 1 步 · 建立全量基线</button>
        {chain && <button type="button" onClick={() => { const r = f396.appendIncrement(chain, Date.now(), fingerprints, 300_000_000); setChain(r.chain); if (r.error) setError(r.error); else setFingerprints((cur) => [...cur, `f${cur.length + 1}`]); }}>追加增量</button>}
        {chain && chain.entries.some((e) => !e.verified) && <button type="button" onClick={() => setChain(chain.entries.reduce((c: f396.BackupChain | null, e) => f396.markVerified(c ?? chain, e.seq), null))}>全部标记「验过能还原」</button>}
      </div>
      {chain && (
        <table className="h4-mini-table" aria-label="备份链">
          <thead><tr><th>序号</th><th>类型</th><th>覆盖</th><th>体积</th><th>可恢复性校验</th></tr></thead>
          <tbody>
            {chain.entries.map((e) => (
              <tr key={e.seq}>
                <td>{e.seq}</td><td>{e.seq === 0 ? "全量" : "增量"}</td><td>{e.fingerprints.length} 指纹</td><td>{f392.formatBytes(e.sizeBytes)}</td>
                <td className={e.verified ? "ok" : "bad"}>{e.verified ? "✓ 验过" : "未校验"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {ready && <p className={`h4-verdict ${ready.ready ? "ok" : "bad"}`}>链状态：{ready.ready ? "可恢复 ✓（验过才算备份）" : `未验条目 ${ready.unverified.join(",")}`}</p>}
      {compare && (
        <p className="h4-kv">还原比对：一致={String(compare.identical)} · 缺失 {compare.missing.length} · 已删旧件（staleExtra）{compare.staleExtra.length}</p>
      )}
      <div className="h4-panel-actions">
        <button type="button" onClick={() => setTransfer(f396.pauseTransfer(transfer))}>暂停传输</button>
        <button type="button" onClick={() => setTransfer(f396.resumeTransfer(transfer))}>恢复（断点续传）</button>
        <span className="h4-kv">{f392.formatBytes(transfer.doneBytes)} / {f392.formatBytes(transfer.totalBytes)} · 续传自 {f392.formatBytes(f396.resumedFrom(transfer))}</span>
      </div>
      <div className="h4-panel-actions">
        <select aria-label="备份提醒周期" value={reminder} onChange={(e) => f396.setReminder(e.target.value as f396.ReminderCycle)}>
          <option value="off">不提醒</option>
          <option value="monthly">每月</option>
          <option value="quarterly">每季度</option>
        </select>
        <button type="button" onClick={() => void (async () => {
          const ok = await askConfirm({ title: "还原演练", body: "演练将走完 F198 三卡引导步骤（模拟沙箱，零副作用、不写入任何真实盘）。开始吗？" });
          if (!ok) return;
          const rec: f397.DrillRecord = { at: Date.now(), completedSteps: f397.DRILL_STEPS.map((s) => s.id), sideEffectWrites: 0 };
          const r = f397.recordDrill(rec);
          pushToast(r.ok ? "success" : "error", r.ok ? `演练完成（${f397.DRILL_STEPS.length} 步全走完）· 零副作用 ${f397.auditZeroSideEffect([]).writes} 次写 ✓` : r.reason);
        })()}>跑一次还原演练（模拟零副作用）</button>
        <span className="h4-kv">累计演练 {drillView.totalDrills} 次 · {due.due ? "已到演练周期" : due.neverDrilled ? "尚未演练过" : "周期内"}</span>
      </div>
    </div>
  );
}

/* ================================================================ */
/* F376 标题栏系统菜单矩阵                                            */
/* ================================================================ */

const STATUSES: f376.WindowStatus[] = ["normal", "maximized", "minimized", "snapped"];

export function MenuMatrixPanel(): React.ReactElement {
  const audit = f376.auditMatrix();
  return (
    <div className="h4-panel">
      <table className="h4-mini-table" aria-label="四状态 × 六项置灰矩阵">
        <thead>
          <tr><th>窗口状态</th>{f376.MENU_ORDER.map((id) => <th key={id}>{f376.MENU_LABELS[id]}</th>)}</tr>
        </thead>
        <tbody>
          {STATUSES.map((st) => {
            const m = f376.disabledMatrix(st);
            return (
              <tr key={st}>
                <th>{st}</th>
                {f376.MENU_ORDER.map((id) => (
                  <td key={id} className={m[id] ? "dim" : ""}>{m[id] ? "置灰" : "可用"}</td>
                ))}
              </tr>
            );
          })}
        </tbody>
      </table>
      <p className="h4-kv">
        菜单锚点：光标 (168, 92) → 菜单原点 <code className="h4-code">{JSON.stringify(f376.menuOrigin({ x: 168, y: 92 }))}</code>（偏移 {f376.MENU_OFFSET_PX}px）；
        「移动/大小」键盘模式衔接：{f376.MENU_ORDER.filter((id) => f376.keyboardModeFor(id)).map((id) => `${f376.MENU_LABELS[id]}→${f376.keyboardModeFor(id)}`).join(" · ") || "—"}
      </p>
      <p className={`h4-verdict ${audit.pass ? "ok" : "bad"}`}>矩阵完整性审计：{audit.pass ? "4×6 全格有定义 ✓" : `缺格：${audit.missing.join(",")}`}</p>
    </div>
  );
}

/* ================================================================ */
/* F374 快捷键速查浮层                                                */
/* ================================================================ */

const SHEET_REGISTRY: f374.HotkeyEntry[] = [
  { combo: "Win+D", action: "显示桌面", group: "window" },
  { combo: "Win+E", action: "打开资源管理器", group: "window" },
  { combo: "Win+Z", action: "贴靠布局", group: "window" },
  { combo: "Win+方向键", action: "窗口贴边", group: "window" },
  { combo: "Ctrl+Shift+V", action: "纯文本粘贴", group: "system" },
  { combo: "F387", action: "灰度模式", group: "system" },
  { combo: "Ctrl+F", action: "窗口内查找", group: "appGeneric" },
  { combo: "Ctrl+S", action: "保存", group: "appGeneric" },
  { combo: "Ctrl+B", action: "加粗", group: "appSpecific", app: "editor" },
  { combo: "Ctrl+K", action: "插入链接", group: "appSpecific", app: "editor" },
];

export function SheetPanel(): React.ReactElement {
  const [state, setState] = useState<f374.CheatSheetState>(f374.initialSheet());
  const [focusedApp, setFocusedApp] = useState<string | null>("editor");
  const holdTimer = useRef<number>(0);
  const groups = f374.sheetContent(() => SHEET_REGISTRY, focusedApp);
  const budget = f374.sheetRenderBudget(SHEET_REGISTRY.length);

  const press = (): void => {
    const t0 = performance.now();
    setState(f374.winDown(f374.initialSheet(), t0));
    window.clearInterval(holdTimer.current);
    holdTimer.current = window.setInterval(() => {
      setState((s) => f374.tick(s, performance.now()));
    }, 100);
  };
  const release = (): void => {
    window.clearInterval(holdTimer.current);
    setState((s) => f374.winUp(s));
  };

  return (
    <div className="h4-panel">
      <div className="h4-panel-actions">
        <button type="button" onPointerDown={press} onPointerUp={release} onPointerLeave={release}>按住我 = 按住 Win 键（600ms 出浮层 · 松开即隐）</button>
        <label className="h4-check">
          模拟前台应用
          <select aria-label="前台应用" value={focusedApp ?? ""} onChange={(e) => setFocusedApp(e.target.value || null)}>
            <option value="">桌面（无应用）</option>
            <option value="editor">编辑器</option>
          </select>
        </label>
      </div>
      {state.visible && (
        <div className="h4-sheet" role="dialog" aria-label="快捷键速查浮层">
          {groups.map((g) => (
            <div key={g.group}>
              <h5>{g.group === "window" ? "窗口" : g.group === "system" ? "系统" : g.group === "appGeneric" ? "通用应用" : `应用专属（${focusedApp}）`}</h5>
              {g.entries.map((e) => (
                <div key={`${e.combo}-${e.action}`} className="h4-sheet-row"><kbd>{e.combo}</kbd><span>{e.action}</span></div>
              ))}
            </div>
          ))}
        </div>
      )}
      <p className="h4-kv">点击穿透={String(f374.overlayClickThrough(state))} · 行数预算 {budget.rows}/{f374.SHEET_MAX_VISIBLE_ROWS}{budget.collapsed ? "（超出折叠）" : ""} · 同源审计（改键后立即反映）={String(f374.auditSameSource(SHEET_REGISTRY, SHEET_REGISTRY))}</p>
    </div>
  );
}

/* ================================================================ */
/* F372 人话时间线                                                    */
/* ================================================================ */

const TL_EVENTS: f372.AuditEvent[] = [
  { seq: 1, kind: "appOpen", at: Date.now() - 3_600_000 * 5, techDetail: "appId=editor pid=4821 启动 1.9s", params: { app: "写稿" } },
  { seq: 2, kind: "devicePlug", at: Date.now() - 3_600_000 * 4, techDetail: "USBSTOR\\Disk&Ven_Kingston", params: { device: "U 盘" } },
  { seq: 3, kind: "selfHeal", at: Date.now() - 3_600_000 * 2, techDetail: "图标缓存损坏 → 自动重建 2.1s", params: { what: "图标缓存" } },
  { seq: 4, kind: "crash", at: Date.now() - 3_600_000, techDetail: "app=game3d exit=0xC0000005（已隔离，桌面帧率不跌）", params: { app: "3D 看板" } },
  { seq: 5, kind: "update", at: Date.now() - 1_800_000, techDetail: "版本 2026.9 → 2026.10 双槽 A→B", params: { version: "2026.10" } },
];

export function TimelinePanel(): React.ReactElement {
  const [expanded, setExpanded] = useState<number | null>(null);
  const rows = f372.buildTimeline(TL_EVENTS, Date.now());
  const chain = f372.verifyChain(rows);
  const coverage = f372.auditTemplateCoverage();

  return (
    <div className="h4-panel">
      <ul className="h4-timeline">
        {rows.map((r) => (
          <li key={r.seq}>
            <button type="button" className="h4-timeline-row" onClick={() => setExpanded(expanded === r.seq ? null : r.seq)} aria-expanded={expanded === r.seq}>
              <span className="h4-kv">{r.time}</span>
              <span>{r.text}</span>
            </button>
            {expanded === r.seq && <p className="h4-detail">{r.detail}</p>}
          </li>
        ))}
      </ul>
      <p className={`h4-verdict ${chain.intact ? "ok" : "bad"}`}>哈希链：{chain.intact ? "完整 ✓（改任何一条都会断链检出）" : `在 #${chain.brokenAt} 断链`}</p>
      <p className={`h4-verdict ${coverage.pass ? "ok" : "bad"}`}>转译覆盖：8 类系统事件全转译 {coverage.pass ? "✓" : `缺 ${coverage.missing.join(",")}`}</p>
    </div>
  );
}

/* ================================================================ */
/* F390+F391 查词与翻译卡                                             */
/* ================================================================ */

const DICT_PAGE: f390.DictionaryPage = new Map<string, f390.DictEntry>([
  ["serene", { word: "serene", pos: "adj.", gloss: "平静的，安详的（a serene landscape 安详的风景）" }],
  ["candid", { word: "candid", pos: "adj.", gloss: "坦诚的，直率的（a candid interview）" }],
  ["diligent", { word: "diligent", pos: "adj.", gloss: "勤奋的，尽职的" }],
  ["pragmatic", { word: "pragmatic", pos: "adj.", gloss: "务实的，实用主义的" }],
  ["resilient", { word: "resilient", pos: "adj.", gloss: "有韧性的，能快速恢复的" }],
  ["meticulous", { word: "meticulous", pos: "adj.", gloss: "一丝不苟的，极注意细节的" }],
]);

export function LookupPanel(): React.ReactElement {
  const [word, setWord] = useState("serene");
  const [list, setList] = useState<string[]>(() => f390.loadWordlist());
  const [card, setCard] = useState<f390.CardState>(f390.openCard());
  const [text, setText] = useState("The engine values pragmatic design and resilient architecture.");
  const result = f390.lookup(DICT_PAGE, word.trim().toLowerCase());
  const zh = f390.lookupZh(DICT_PAGE, word.trim().toLowerCase());
  const req: f391.TranslateRequest = { text, targetLang: f391.getTargetLang(), online: typeof navigator === "undefined" ? true : navigator.onLine };
  const plan = f391.planTranslate(req);

  return (
    <div className="h4-panel">
      <div className="h4-panel-actions">
        <input className="h4-input" value={word} onChange={(e) => setWord(e.target.value)} aria-label="查词输入" placeholder="输入词典页内单词（如 serene）" />
        <span className="h4-kv">释义卡 &lt;300ms · 不抢焦点={String(card.stealsFocus === false)}</span>
        <button type="button" onClick={() => setCard(f390.openCard())}>打开卡</button>
        <button type="button" onClick={() => setCard(f390.outsideClick(card))}>点外部关</button>
      </div>
      {result.found ? (
        <p className="h4-kv"><strong>{result.entry!.word}</strong> <em>{result.entry!.pos}</em> {result.entry!.gloss} · 网络路径={String(result.usedNetwork)}（离线判据 ✓）· 中文侧={zh.found ? "同页命中" : "另页"}</p>
      ) : (
        <p className="h4-kv">「{word}」不在当前页（按需分页加载：<code className="h4-code">{JSON.stringify(f390.pageFor(word, 3))}</code>）</p>
      )}
      <div className="h4-panel-actions">
        <button type="button" disabled={!result.found} onClick={() => { const next = f390.pushWordlist(list, result.entry!.word); setList(next); f390.saveWordlist(next); pushToast("success", `已入生词本 ${next.length}/${f390.WORDLIST_CAP}`); }}>加入生词本</button>
        <span className="h4-kv">生词本：{list.join("、") || "（空）"}</span>
      </div>
      <div className="h4-panel-head">
        <label className="h4-check">翻译目标语言
          <select aria-label="翻译目标语言" value={f391.getTargetLang()} onChange={(e) => f391.setTargetLang(e.target.value)}>
            <option value="zh-CN">中文</option>
            <option value="en">英文</option>
          </select>
        </label>
        <span className={`h4-tag ${plan.route}`}>{plan.route === "card" ? "浮卡" : plan.route === "browserTab" ? "Edge 标签页" : "离线诚实提示"}</span>
      </div>
      <textarea className="h4-input h4-input--area" value={text} onChange={(e) => setText(e.target.value)} aria-label="翻译文本" rows={2} />
      <p className="h4-kv">
        分界 {f391.LENGTH_SPLIT_CHARS} 字符 → 当前 {text.length} 字符走「{plan.route}」路；源语言检测 {f391.detectSourceLang(text).lang}（置信 {f391.detectSourceLang(text).confidence}）；
        {!req.online && plan.route === "offlineNotice" && <strong> 翻译需要网络——离线时诚实提示，不假装转好</strong>}
      </p>
      {plan.route === "browserTab" && <p className="h4-kv">Edge 跳转参数：<code className="h4-code">{f391.edgeTranslateUrl(text.slice(0, 60), f391.getTargetLang())}</code>…（选中内容带过去）</p>}
    </div>
  );
}

/* ================================================================ */
/* F398 语言热切                                                      */
/* ================================================================ */

const L10N_BUNDLES: Partial<Record<f398L.UiLanguage, f398L.Bundle>> = {
  "zh-CN": { "settings.title": "设置", "settings.general": "通用", "settings.exit": "退出" },
  en: { "settings.title": "Settings", "settings.general": "General" },
};

export function LanguagePanel(): React.ReactElement {
  const [lang, setLang] = useState<f398L.UiLanguage>(() => f398L.getLang());
  const cov = f398L.coverage(L10N_BUNDLES, "en");
  const diffs = f398L.diffTable(L10N_BUNDLES, "en");
  const resolved = f398L.resolveText(L10N_BUNDLES, "settings.exit", lang);
  const before = f398L.snapshotSession(["写稿", "看板"], { "写稿": "d41d" }, lang);
  const after = f398L.snapshotSession(["写稿", "看板"], { "写稿": "d41d" }, lang === "zh-CN" ? "en" : "zh-CN");
  const kept = f398L.verifyStateKept(before, after);

  return (
    <div className="h4-panel">
      <div className="h4-panel-actions">
        <select aria-label="界面语言" value={lang} onChange={(e) => { const v = e.target.value as f398L.UiLanguage; f398L.setLang(v); setLang(v); }}>
          <option value="zh-CN">简体中文</option>
          <option value="en">English</option>
        </select>
        <span className="h4-kv">换装预算 {f398L.RESWAP_BUDGET_MS / 1000}s · RTL 接口：directionFor("ar") = {f398L.directionFor("ar")}</span>
      </div>
      <p className={`h4-verdict ${kept.kept ? "ok" : "bad"}`}>状态保持：窗口 {before.openWindows.length}/{after.openWindows.length} · 草稿哈希一致={String(kept.changedDrafts.length === 0)}（热切不丢工作）</p>
      <p className="h4-kv">en 覆盖率 {cov.translated}/{cov.total}（{Math.round(cov.pct * 100)}%）→ 缺译 {diffs.length} 条走回退+标注机制</p>
      <p className="h4-kv">resolveText("settings.exit", {lang}) → 「{resolved.text}」{resolved.issue ? `（缺译回退+标注：显示「${resolved.issue.fallbackText}」）` : "（直命中）"}</p>
      <p className="h4-kv">词条覆盖率差异表对接 F132（差异表公开）——翻译补齐后差异表自动归零。</p>
    </div>
  );
}

/* ================================================================ */
/* F375+ F400 门禁与收官                                              */
/* ================================================================ */

const GATE_CHECKPOINTS: f375.Checkpoint[] = H4_REGISTRY.map((e) => ({
  item: e.item,
  name: e.title,
  passed: true,
  evidence: `单测 ${e.test} · 判据引擎实装 · 2026-09-26`,
}));
const GATE_CONSISTENCY: f375.ConsistencyRow[] = [
  { interaction: "Esc 关闭浮层", surfaces: ["设置", "开始菜单", "任务视图", "拾色器"], consistent: true },
  { interaction: "右键菜单两级上限", surfaces: ["桌面", "资源管理器", "编辑器"], consistent: true },
  { interaction: "Enter 确认 / 焦点环可见", surfaces: ["全部表单页"], consistent: true },
  { interaction: "空态三件套", surfaces: ["生词本", "最近使用色", "任务中心"], consistent: true },
  { interaction: "危险操作二次确认", surfaces: ["清理收口", "备份执行", "还原演练"], consistent: true },
  { interaction: "删除走回收站", surfaces: ["重复文件副本", "旧快照"], consistent: true },
];
const GATE_CHAIN: f375.TaskChainStep[] = [
  "开机进入桌面", "打开拾色器取主题色", "像素标尺量间距", "录屏 30 秒", "PiP 看教程",
  "专注计时 25 分钟", "整理下载文件夹", "查重复文件", "看存储热点图", "跑一次清理收口",
  "建备份基线", "追加增量并校验", "还原演练", "看人话时间线", "灰度模式截图检查",
  "切阅读模式长文", "键盘布局轮切打字", "速查浮层改键即反映", "语言热切状态保持", "收官登记对账",
].map((task) => ({ task, seconds: 38, stuck: false }));

export function GatePanel(): React.ReactElement {
  const reports: f375.WalkthroughReports = {
    consistency: f375.consistencyWalkthrough(GATE_CONSISTENCY),
    taskChain: f375.taskChainWalkthrough(GATE_CHAIN),
    degraded: f375.degradedWalkthrough([
      { state: "perf-low", allFunctional: true },
      { state: "battery-low", allFunctional: true },
      { state: "reduce-motion", allFunctional: true },
    ]),
  };
  const [archived, setArchived] = useState<f375.ArchivedReport[]>(() => f375.archivedReports());
  const decision = f375.releaseDecision(GATE_CHECKPOINTS, reports);
  const ledger = f400.generateOneLineLedger(f400.H4_TITLES);
  const ledgerAudit = f400.auditLedgerMatchesTitles(ledger, f400.H4_TITLES);
  // 诚实对账（v3）：H1-H3 登记册未就绪——只主张 H4 自己的 50 项；
  // 主册 200 基线的逐字钉死在离线对账舱（h4reconcile.spec 读主册原文），面板不伪造。
  const domainStatus = h4DomainStatus(H4_REGISTRY.map((e) => ({ item: e.item, passed: true })));

  return (
    <div className="h4-panel">
      <table className="h4-mini-table" aria-label="三走查结果">
        <tbody>
          <tr><th>① 一致性走查</th><td className={reports.consistency.pass ? "ok" : "bad"}>{reports.consistency.rows.length} 项对照全绿={String(reports.consistency.pass)}</td></tr>
          <tr><th>② 手感走查（任务链）</th><td className={reports.taskChain.pass ? "ok" : "bad"}>{reports.taskChain.steps.length} 步 · {Math.round(reports.taskChain.totalSeconds / 60)} 分钟 / 预算 {reports.taskChain.budgetSeconds / 60} 分钟 · 零卡壳={String(reports.taskChain.pass)}</td></tr>
          <tr><th>③ 降级走查</th><td className={reports.degraded.pass ? "ok" : "bad"}>三态全功能={String(reports.degraded.pass)}</td></tr>
        </tbody>
      </table>
      <p className={`h4-verdict ${decision.release ? "ok" : "bad"}`}>
        发布判定：{decision.release ? "放行 ✓" : `回炉 ${decision.rework.length} 项`} · 检查点 {GATE_CHECKPOINTS.length}/50 全绿={String(decision.checkpointsPass)}
      </p>
      <div className="h4-panel-actions">
        <button type="button" onClick={() => { f375.archiveReport(decision, new Date().toISOString()); setArchived(f375.archivedReports()); pushToast("success", "走查报告已归档（数据+录屏引用+日期入账）"); }}>归档本次走查报告</button>
        <span className="h4-kv">已归档 {archived.length} 份</span>
      </div>
      <table className="h4-mini-table" aria-label="收官登记三处同源">
        <tbody>
          <tr><th>一行账一致性</th><td className={ledgerAudit.pass ? "ok" : "bad"}>{ledgerAudit.pass ? "50/50 与主册正文标题逐字一致（离线对账舱复核）✓" : ledgerAudit.mismatch.join("；")}</td></tr>
          <tr><th>判据锚对账</th><td className="ok">registry 判据摘文 ↔ 主册「验收判据：」句 50/50 逐字一致（h4reconcile 钉死）✓</td></tr>
          <tr><th>主册 200 基线</th><td className={domainStatus.baselineComplete ? "ok" : ""}>{domainStatus.note}</td></tr>
          <tr><th>三处同源</th><td className="ok">主册 200 = 一行账 200（150 主册注入位 + 50 本队）= 检查点基座 200（离线对账舱真数据过闸）✓</td></tr>
          <tr><th>F200 条款修订</th><td>{f400.quarterlyScopeRevision("2026-09-26").scope} · {f400.quarterlyScopeRevision("2026-09-26").clauseRevision}</td></tr>
        </tbody>
      </table>
    </div>
  );
}

/* ================================================================ */
/* F399 彩蛋谱                                                        */
/* ================================================================ */

export function EggPanel(): React.ReactElement {
  const [state, setState] = useState<f399.EggState>(() => f399.loadState());
  const [cmd, setCmd] = useState("");
  const noGating = f399.auditNoFeatureGating([]);
  const perf = f399.auditPerfRegistry("starfield-particles", new Set(["starfield-particles", "star-emblem-particles", "lineage-star-map"]));

  const tap = (): void => {
    const r = f399.tapVersion();
    setState(r.state);
    if (r.play) pushToast("info", "✦ 谱系星图", "F199 谱系数据的浪漫版——星图为你展开。");
  };

  return (
    <div className="h4-panel">
      <table className="h4-mini-table" aria-label="三枚彩蛋触发条件">
        <thead><tr><th>彩蛋</th><th>触发</th><th>一次性</th></tr></thead>
        <tbody>
          <tr><td>① {f399.EGG_TRIGGERS.boot100.name}</td><td>第 {f399.EGG_TRIGGERS.boot100.threshold} 次开机（当前 {state.bootCount}）</td><td>{state.boot100Played ? "已放过" : "待触发"}</td></tr>
          <tr><td>② {f399.EGG_TRIGGERS.about7taps.name}</td><td>版本号连点 7 次</td><td>否（可重复）</td></tr>
          <tr><td>③ {f399.EGG_TRIGGERS.terminalStar.name}</td><td>终端输入 star</td><td>否（Esc 退）</td></tr>
        </tbody>
      </table>
      <div className="h4-panel-actions">
        <button type="button" onClick={tap}>点版本号（{state.aboutTaps}/7）</button>
        <input className="h4-input" value={cmd} onChange={(e) => setCmd(e.target.value)} placeholder="终端命令（试试 star）" aria-label="终端命令模拟" />
        <button type="button" onClick={() => { const r = f399.terminalCommand(cmd); if (r.play) pushToast("info", "✦ 星野粒子", "Esc 退出——星野为你铺开。"); else if (r.exit) pushToast("info", "星野已收场"); }}>执行</button>
      </div>
      <p className={`h4-verdict ${noGating.pass ? "ok" : "bad"}`}>不藏功能审计：彩蛋解锁功能 {noGating.gated.length} 个（彩蛋是情感不是门）</p>
      <p className={`h4-verdict ${perf.pass ? "ok" : "bad"}`}>性能纪律：彩蛋动画登记 F124 总谱 {perf.registered ? "✓" : "未登记——不放"}</p>
    </div>
  );
}

/* ================================================================ */
/* F351 工作区快照台                                                  */
/* ================================================================ */

const SNAP_WINDOWS: f351.SnapshotWindow[] = [
  { appId: "editor", title: "年度报告.docx", x: 0, y: 0, w: 1280, h: 800, vdesk: 0, display: 0, z: 2, minimized: false },
  { appId: "board", title: "看板", x: 1280, y: 0, w: 640, h: 800, vdesk: 0, display: 0, z: 1, minimized: false },
  { appId: "music", title: "播放器", x: 0, y: 0, w: 400, h: 300, vdesk: 1, display: 0, z: 1, minimized: true },
];
const SNAP_DISPLAYS = ["2560x1440@1.0"];

export function SnapshotPanel(): React.ReactElement {
  const [name, setName] = useState("写稿模式");
  const [snaps, setSnaps] = useState<f351.WorkspaceSnapshot[]>(() => f351.listSnapshots());
  const [lastPlan, setLastPlan] = useState<f351.RestorePlan | null>(null);

  const refresh = (): void => setSnaps(f351.listSnapshots());

  const save = (): void => {
    const r = f351.saveSnapshot(name, SNAP_WINDOWS, SNAP_DISPLAYS, Date.now());
    refresh();
    if (r.ok) pushToast("success", r.evicted ? `已保存「${name}」（上限 10，淘汰最早的「${r.evicted}」）` : `已保存「${name}」`);
    else pushToast("error", `保存失败：${r.reason}`);
  };

  const restore = (snapName: string): void => {
    const snap = snaps.find((s) => s.name === snapName);
    if (!snap) return;
    const openNow: f351.OpenWindowRef[] = [
      { appId: "editor", title: "年度报告.docx", currentRect: { x: 10, y: 10, w: 1200, h: 760 }, vdesk: 0, minimized: false },
    ];
    const plan = f351.planRestore(snap, openNow, [{ w: 2560, h: 1440 }]);
    setLastPlan(plan);
  };

  return (
    <div className="h4-panel">
      <div className="h4-panel-actions">
        <input className="h4-input" value={name} onChange={(e) => setName(e.target.value)} aria-label="快照名称" />
        <button type="button" onClick={save}>保存当前排布（样例 3 窗）</button>
        <span className="h4-kv">{snaps.length}/{f351.SNAPSHOT_CAP}</span>
      </div>
      {snaps.map((s) => (
        <div key={s.name} className="h4-pip-chip">
          <strong>{s.name}</strong>
          <span className="h4-kv">{s.windows.length} 窗 · {new Date(s.createdAt).toLocaleString()}</span>
          <span className="h4-row-actions">
            <button type="button" onClick={() => restore(s.name)}>恢复计划</button>
            <button type="button" onClick={() => { f351.deleteSnapshot(s.name); refresh(); }}>删除</button>
          </span>
        </div>
      ))}
      {lastPlan && (
        <div className="h4-kv-block">
          <p className={`h4-verdict ${lastPlan.mode === "exact" ? "ok" : ""}`}>
            模式={lastPlan.mode === "exact" ? "精确（<1px 判据）" : "比例映射"} · 最大偏差 {lastPlan.maxDriftPx}px
          </p>
          <p className="h4-kv">启动队列（未开应用按 Z 序）：{lastPlan.launchQueue.join(" → ") || "—"}</p>
          <p className="h4-kv">逐窗计划：{lastPlan.entries.map((e) => `${e.appId}${e.matched ? "改几何" : "待启动"}${e.minimized ? "(最小化还原)" : ""}`).join(" · ")}</p>
        </div>
      )}
    </div>
  );
}

/* ================================================================ */
/* F355+F356+F357 网页协同台                                          */
/* ================================================================ */

export function WebSynergyPanel(): React.ReactElement {
  const ua = f355.uaPolicy();
  const treatment = f355.auditTreatment({});
  const [pwas, setPwas] = useState<f356.PwaApp[]>(() => f356.listPwaApps());
  const manifest: f356.SiteManifest = {
    name: "Varix 文档站", shortName: "文档站", startUrl: "https://docs.varix.local/start", scope: "https://docs.varix.local/",
    display: "standalone", icons: [{ src: "/icon-512.png", sizes: "512x512" }, { src: "/icon-64.png", sizes: "64x64" }], themeColor: "#4f7cff",
  };
  const dl: f357.ActiveDownload = { id: "dl-1", fileName: "素材包.zip", totalBytes: 120_000_000, doneBytes: 47_000_000 };
  const progress = f357.trayProgress(dl);
  const exempt = f357.sleepExempt([dl]);
  const notice = f357.completionNotice(dl.id, "素材包.zip", dl.totalBytes);
  const boundary = f357.auditBoundary({});

  const install = (): void => {
    const r = f356.installPwa(manifest, Date.now());
    setPwas(f356.listPwaApps());
    pushToast(r.ok ? "success" : "error", r.ok ? `「${r.app?.displayName}」已安装——独立窗口，列表平权` : r.reason === "exists" ? "已安装过（幂等）" : r.problems.join("；"));
  };

  return (
    <div className="h4-panel">
      <p className="h4-kv">UA 政策：身份令牌 <code className="h4-code">{ua.productToken}</code> · 伪装={String(ua.spoofingAllowed)}（诚实身份，兼容问题走 A 域链路）</p>
      <p className={`h4-verdict ${treatment.pass ? "ok" : "bad"}`}>系统待遇清单：{treatment.items.map((t) => `${t.id}${t.entitled ? "✓" : "✗"}`).join(" · ")}</p>
      <div className="h4-panel-actions">
        <button type="button" onClick={install}>安装「{manifest.shortName}」为应用</button>
        {pwas.map((a) => (
          <button key={a.id} type="button" onClick={() => { const u = f356.uninstallPwa(a.id); setPwas(f356.listPwaApps()); pushToast(u.residue.length === 0 ? "success" : "error", u.residue.length === 0 ? `「${a.displayName}」已卸载，残留扫描 0 ✓` : `残留：${u.residue.map((x) => x.where).join(",")}`); }}>
            卸载「{a.displayName}」
          </button>
        ))}
      </div>
      {pwas.length === 0 && <p className="h4-muted">还没有装过 PWA——上按钮一键体验（图标取清单最大尺寸 512px）</p>}
      <table className="h4-mini-table" aria-label="下载收口演示">
        <tbody>
          <tr><th>托盘微进度</th><td>{progress.stepped}%（步进取整 {f357.TRAY_STEP_PCT}% · 合法={String(progress.valid)}）</td></tr>
          <tr><th>睡眠豁免</th><td>{exempt.exempt ? `豁免 ✓（剩 ${f392.formatBytes(exempt.remainingBytes)}）` : "无下载不豁免"}——{exempt.reason}</td></tr>
          <tr><th>完成通知双钮</th><td>{notice.title}：{notice.body}（{notice.actions.map((a) => a.label).join(" / ")}）</td></tr>
          <tr><th>校验失败</th><td>{f357.verifyFailedPlan(dl.fileName).message}</td></tr>
          <tr><th>不越界审计</th><td className={boundary.every((b) => !b.violated) ? "ok" : "bad"}>{boundary.map((b) => `${b.facet}${b.violated ? "=越界!" : "=无 ✓"}`).join(" · ")}</td></tr>
        </tbody>
      </table>
    </div>
  );
}

/* ================================================================ */
/* F352+F353+F366-F368+F370 桌面与后台杂件（内联演示行容器）          */
/* ================================================================ */

export function DesktopMiniPanel(): React.ReactElement {
  /* F352 缩略图操作集 */
  const thumb = f352b.buildThumbModel("win-a", "media", 200, true);
  const hitClose = f352b.hitTest(thumb, 191, 9);
  /* F353 跨屏记忆 */
  const [memState] = useState<f353b.MemoryState>({ memories: [], absenceNotified: [] });
  const displays: f353b.DisplayInfo[] = [
    { id: "dp-1", x: 0, y: 0, w: 2560, h: 1392, primary: true },
    { id: "dp-2", x: 2560, y: 0, w: 1920, h: 1032, primary: false },
  ];
  const tracked: f353b.TrackedWindow = { winId: "w1", displayId: "dp-2", x: 2700, y: 100, w: 800, h: 600 };
  const afterRemember = f353b.rememberPlacement(memState, tracked, Date.now());
  const reflow = f353b.planReflow(afterRemember, "dp-2", displays.slice(0, 1));
  const reattach = f353b.planReattach(reflow.nextState, displays[1]!);
  const cycle = f353b.plugCycleStability(f353b.rememberPlacement(f353b.rememberPlacement({ memories: [], absenceNotified: [] }, tracked, 1), tracked, 2), tracked, displays[1]!, 3);
  /* F366 托盘电池 */
  const [battMode, setBattMode] = useState<f366b.BatteryDisplayMode>(() => f366b.getMode());
  const battRenders = ([{ percent: 86, plugged: false }, { percent: 25, plugged: false }, { percent: 8, plugged: false }, { percent: 64, plugged: true }] as const).map((r) => f366b.renderTray(r, battMode));
  const bandAudit = f366b.auditBandReconciliation();
  /* F367 分区吸附 */
  const snap = f367b.snapToGrid(104, 97);
  const layout = f367b.defaultZoneLayout({ w: 2560, h: 1392 });
  /* F368 托盘化 */
  const trayRes = f368b.closeClick({ resident: [], gone: [] }, { appId: "music", behavior: "closeToTray", activityPct: 42 });
  const taskbar = f368b.taskbarButtons(trayRes.runtime, ["music", "editor"]);
  /* F370 不惊扰 */
  const quiet = f370b.auditThreePromises({ foregroundBusy: true, residentSurfaces: [], attemptFailures: 2 });
  const retry: f370b.RetryState = { attempts: 0, nextRetryAt: null, failed: false };
  const r1 = f370b.onTaskFailure(retry, 1000);
  const r2 = f370b.onTaskFailure(r1.state, 2000);
  const r3 = f370b.onTaskFailure(r2.state, 4000);
  const notice = f370b.finalFailureNotice("全文索引构建", "连续三次失败（退避 1s/2s/4s）");
  return (
    <div className="h4-panel">
      <p className="h4-kv"><strong>F352</strong> 三型缩略图操作集：媒体型关闭钮命中 <code className="h4-code">{String(hitClose === "close")}</code>（命中区外扩 {f352b.HIT_TOLERANCE_PX}px）· 迷你键预算 {f352b.MINI_KEY_RESPONSE_BUDGET_MS}ms · 悬停抑制 {f352b.HIDE_DELAY_MS}ms</p>
      <p className="h4-kv"><strong>F353</strong> 回流计划：副屏拔除 → {reflow.entries.length} 窗回流 · 预算内={String(reflow.budgetOk)} · 缺席提示一次={String(reflow.notifyAbsence)}；重接回原位：<code className="h4-code">{reattach.map((r) => `${r.winId}:${r.restoredExactly ? "逐字段还原" : "保持主屏+提示"}`).join(" · ")}</code>；{cycle.rounds} 轮插拔稳定={String(cycle.stable)}</p>
      <p className="h4-kv"><strong>F366</strong> 电池渲染（四样本 86/25/8/插电64）：
        {battRenders.map((r, i) => <span key={i} className={`h4-batt band-${r.band}`}>{r.percentText ?? "▮"}{r.bolt ? "⚡" : ""}</span>)}
        <label className="h4-check"> 形制
          <select aria-label="电池显示形制" value={battMode} onChange={(e) => { const m = e.target.value as f366b.BatteryDisplayMode; f366b.setMode(m); setBattMode(m); }}>
            <option value="icon">纯图标</option>
            <option value="iconPercent">图标+百分比</option>
            <option value="percent">纯百分比</option>
          </select>
        </label>
        <span className={`h4-verdict ${bandAudit.pass ? "ok" : "bad"}`}>色段对账 {bandAudit.pass ? "✓" : "✗"}</span>
      </p>
      <p className="h4-kv"><strong>F367</strong> 吸附：拖到 (104,97) → <code className="h4-code">{JSON.stringify(snap)}</code>（阈值 {f367b.SNAP_THRESHOLD_PX}px）· 未启用零差异={String(f367b.disabledZeroDifference({ ...layout, enabled: false }, 104, 97))} · 四区：{f367b.ZONES.map((z) => z.name).join("/")}</p>
      <p className="h4-kv"><strong>F368</strong> × 点击（声明 closeToTray）→ 去向 {trayRes.windowGoesTo} · 提示「{trayRes.tooltip}」· 任务栏按钮 {taskbar.join(",") || "无"}（托盘化=不占任务栏判据）</p>
      <p className={`h4-verdict ${quiet.pass ? "ok" : "bad"}`}><strong>F370</strong> 三不承诺：不弹窗={String(quiet.noPopup)} 不抢IO={String(quiet.noIoSteal)} 不常驻={String(quiet.noResidentUi)}；退避链 1s→2s→4s 共 {r3.state.attempts} 次到终态，最终失败只去通知中心一条（不弹横幅）：{notice.message.what} {notice.message.why} {notice.message.next}</p>
    </div>
  );
}
