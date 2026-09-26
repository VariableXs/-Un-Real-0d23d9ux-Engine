/**
 * J 鼠标域 · v5 深化批次五面板（引擎实验室 / 多屏拓扑 / 滚轮标定向导 /
 * 会话日报 / 对账导出）。
 *
 * 与 MouseJ1Panels 同构（j1Store 订阅 + pushToast + SectionCard 折叠卡），
 * 独立成文件是隔离纪律：v5 批次的引擎消费面单独成块，不与他批次子面板
 * 缠结。每个面板都是对应引擎的真实消费端——引擎不挂 UI 不算交付。
 */

import { useEffect, useState } from "react";
import { SectionCard, MiniButton } from "./MouseJ1Panels";
import { j1Store } from "../mouse/j1store";
import { compareEngines } from "../mouse/oneEuro";
import { solverResidual, scaleConsistency } from "../mouse/gainfield";
import { listMonitorsSafe } from "../mouse/windowRuntime";
import type { MonitorInfo } from "../mouse/screen";
import { seamSegments, topologySummary, toLogical } from "../mouse/topology";
import { describeIdentity } from "../mouse/edid";
import { WheelCalibrationWizard, previewTable, compareWithFactory, type WheelPairSample, type PaceSample } from "../mouse/wheelcal";
import { j1Telemetry } from "../mouse/telemetry";
import { bucketSessions, sessionStats, worstSessions, dailyReport } from "../mouse/session";
import { buildReconcileMarkdown, engineProbes, V5_NEW_ANCHORS } from "../mouse/reconcile";
import { pushToast } from "../../state/uiStore";

function copy(text: string, msg: string): void {
  void navigator.clipboard?.writeText(text).then(() => pushToast("success", msg));
}

/* ------------------------------- 引擎实验室 ------------------------------- */

export function EngineLabPanel(): React.ReactElement {
  const [, tick] = useState(0);
  useEffect(() => j1Store.subscribe(() => tick((v) => v + 1)), []);
  const tremor = j1Store.get("tremor") as { level: "off" | "light" | "strong"; engine?: "iir" | "euro" };
  const engine = tremor.engine ?? "iir";
  const spectrum = compareEngines(1);
  const resid = solverResidual(0.9, 0.1);
  const scaleProbe = scaleConsistency((a) => 1 + Math.min(1, a / 128) * 0.5, 96, 1, 1.25);
  return (
    <div className="j1x-stack">
      <div className="j1x-inline">
        <span className="j1x-hint">滤波引擎</span>
        <MiniButton onClick={() => { j1Store.set("tremor", { engine: "iir" }); pushToast("success", "已切回 IIR 引擎（状态复位）"); }}>IIR</MiniButton>
        <MiniButton onClick={() => { j1Store.set("tremor", { engine: "euro" }); pushToast("success", "已切 One Euro（帧率无关自适应）"); }}>One Euro</MiniButton>
        <span className="j1x-hint">当前：{engine === "euro" ? "One Euro" : "IIR"}</span>
      </div>
      <table className="j1x-table">
        <thead>
          <tr><th>频段</th><th>IIR 残余</th><th>One Euro 残余</th></tr>
        </thead>
        <tbody>
          {spectrum.rows.map((r) => (
            <tr key={r.freq}>
              <td>{r.freq}Hz</td>
              <td>{r.iir.residualRatio}</td>
              <td>{r.euro}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <span className="j1x-hint">
        读法：残余越低吃得越干净。IIR 三频段无差别；One Euro 高频压更狠、低频放更宽（频率选择性）且帧率无关。
      </span>
      <div className="j1x-inline">
        <span className="j1x-hint">贝塞尔反解残差（极端控制点）：{resid.toExponential(1)}（红线 2e-5）{resid < 2e-5 ? "✅" : "❌"}</span>
      </div>
      <div className="j1x-inline">
        <span className="j1x-hint">跨缩放一致性探针：偏差 {scaleProbe.delta}（红线 0.05）{scaleProbe.pass ? "✅" : "❌"}</span>
      </div>
    </div>
  );
}

/* ------------------------------- 多屏拓扑 ------------------------------- */

export function TopologyPanel(): React.ReactElement {
  const [, tick] = useState(0);
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  useEffect(() => {
    j1Store.subscribe(() => tick((v) => v + 1));
    void listMonitorsSafe().then(setMonitors);
  }, []);
  const seams = seamSegments(monitors);
  const summary = topologySummary(monitors);
  const s0 = monitors[0];
  const quant = s0 ? toLogical(monitors, s0.x * s0.scale + 100 * s0.scale, s0.y * s0.scale).residual : 0;
  return (
    <div className="j1x-stack">
      <div className="j1x-inline">
        <span className="j1x-hint">{summary.screens} 屏 · {summary.seams} 条接缝 · {summary.mixedDpi ? "混合 DPI（穿越帧对拍随闸门）" : "统一缩放"} · 桌面 {summary.bounds}</span>
      </div>
      {monitors.map((m) => {
        const id = describeIdentity(m.edidFingerprint);
        return (
          <div key={m.id} className="j1x-inline">
            <span className="j1x-hint">
              <strong>{m.id}</strong> {m.width}×{m.height} @{m.scale}x — {id.vendor}（{id.kind === "edid" ? "EDID 字节身份" : "名称降级身份"}）：{id.note}
            </span>
          </div>
        );
      })}
      {seams.length > 0 && (
        <table className="j1x-table">
          <thead><tr><th>屏对</th><th>轴</th><th>位置</th><th>延伸</th></tr></thead>
          <tbody>
            {seams.map((s, i) => (
              <tr key={i}>
                <td>{s.a} | {s.b}</td>
                <td>{s.axis === "v" ? "竖缝" : "横缝"}</td>
                <td>{Math.round(s.at)}</td>
                <td>{Math.round(s.from)}–{Math.round(s.to)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      <span className="j1x-hint">DPI 量化残差探针（屏1 @100px 逻辑位）：{quant.toFixed(3)}px（&lt;0.5px 即零可见跳变）。</span>
    </div>
  );
}

/* ------------------------------- 滚轮标定向导 ------------------------------- */

const PACE_CHOICES = [1, 3, 6, 10, 15];
const LINES_CHOICES = [3, 5, 8, 12, 18];

export function WheelCalPanel(): React.ReactElement {
  const [wiz] = useState(() => new WheelCalibrationWizard());
  const [, tick] = useState(0);
  const [pace, setPace] = useState(3);
  const [lines, setLines] = useState(8);
  const st = wiz.state;
  const rerender = (): void => tick((v) => v + 1);
  const feedPair = (s: WheelPairSample): void => { wiz.feedLinePair(s); rerender(); };
  const record = (p: PaceSample): void => { wiz.recordPace(p.notchesPerSec, p.lines); rerender(); };
  const gainCfg = j1Store.get("wheelGain") as { calib?: { k: number; b: number } };
  const factory = gainCfg.calib ? compareWithFactory(gainCfg.calib) : null;
  return (
    <div className="j1x-stack">
      {st.step === "idle" && (
        <div className="j1x-inline">
          <MiniButton onClick={() => { wiz.begin(); rerender(); }}>开始标定</MiniButton>
          <span className="j1x-hint">四步：采集 → 拟合 → 预览 → 采纳；随时可弃。</span>
        </div>
      )}
      {st.step === "collect" && (
        <div className="j1x-stack">
          <div className="j1x-inline">
            <span className="j1x-hint">行高估计：{st.lineEst ? `${st.lineEst}px/行` : "采集中（≥3 对样本）"}</span>
            <MiniButton onClick={() => feedPair({ lines: 3, px: 120 })}>注入 3 行/120px 样本</MiniButton>
          </div>
          <div className="j1x-inline">
            <span className="j1x-hint">节奏问答</span>
            <select className="j1x-select" value={pace} onChange={(e) => setPace(Number(e.target.value))}>
              {PACE_CHOICES.map((p) => <option key={p} value={p}>{p} 档/秒</option>)}
            </select>
            <select className="j1x-select" value={lines} onChange={(e) => setLines(Number(e.target.value))}>
              {LINES_CHOICES.map((l) => <option key={l} value={l}>{l} 行/格</option>)}
            </select>
            <MiniButton onClick={() => record({ notchesPerSec: pace, lines })}>记一档（{st.paceSamples.length}/32）</MiniButton>
          </div>
          <div className="j1x-inline">
            <MiniButton onClick={() => { wiz.fitNow(); rerender(); }}>拟合（≥2 档）</MiniButton>
            <MiniButton tone="danger" onClick={() => { wiz.abort(); rerender(); }}>放弃</MiniButton>
          </div>
        </div>
      )}
      {st.step === "preview" && st.fit && (
        <div className="j1x-stack">
          <span className="j1x-hint">{st.fit.note}（k={st.fit.k}, b={st.fit.b}, R²={st.fit.r2}）</span>
          <table className="j1x-table">
            <thead><tr><th>节奏</th><th>预览行数</th></tr></thead>
            <tbody>
              {previewTable(st.fit.k, st.fit.b).map((r) => (
                <tr key={r.pace}><td>{r.pace} 档/秒</td><td>{r.lines} 行</td></tr>
              ))}
            </tbody>
          </table>
          <div className="j1x-inline">
            <MiniButton onClick={() => {
              const adopted = wiz.accept();
              if (!adopted) { pushToast("error", "拟合未达标——回采集补样本"); rerender(); return; }
              j1Store.set("wheelGain", { calib: { k: adopted.k, b: adopted.b } });
              pushToast("success", "标定曲线已生效（wheelGain.calib）");
              rerender();
            }}>采纳并生效</MiniButton>
            <MiniButton tone="danger" onClick={() => { wiz.abort(); rerender(); }}>放弃</MiniButton>
          </div>
        </div>
      )}
      {st.step === "done" && <span className="j1x-hint">标定完成——曲线已在滚轮增益链路生效。</span>}
      {gainCfg.calib && (
        <div className="j1x-inline">
          <span className="j1x-hint">
            当前标定：k={gainCfg.calib.k} b={gainCfg.calib.b} · 3 档→{factory?.at3} 行 / 8 档→{factory?.at8} 行
            {factory?.nearFactory ? "（贴近出厂手感）" : "（与出厂差异较大，属你的手感）"}
          </span>
          <MiniButton tone="danger" onClick={() => { j1Store.set("wheelGain", { calib: undefined }); pushToast("success", "已恢复出厂线性增益"); }}>还原出厂</MiniButton>
        </div>
      )}
    </div>
  );
}

/* ------------------------------- 会话日报 ------------------------------- */

export function SessionReportPanel(): React.ReactElement {
  const [, tick] = useState(0);
  useEffect(() => j1Store.subscribe(() => tick((v) => v + 1)), []);
  const snap = j1Telemetry.snapshot();
  const sessions = bucketSessions(snap.events, snap.frustrations);
  const worst = worstSessions(sessions);
  return (
    <div className="j1x-stack">
      <span className="j1x-hint">{sessions.length} 段会话 · {snap.events.length} 事件 · {snap.frustrations.length} 挫败信号（30 分钟间隔切会话）</span>
      {sessions.length > 0 && (
        <table className="j1x-table">
          <thead><tr><th>会话</th><th>事件</th><th>顺畅率</th><th>P95(ms)</th><th>挫败</th></tr></thead>
          <tbody>
            {sessions.map((s, i) => {
              const st = sessionStats(s);
              const t0 = new Date(s.from).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
              return <tr key={i}><td>{t0}</td><td>{st.events}</td><td>{Math.round(st.smoothRate * 100)}%</td><td>{st.latency.p95}</td><td>{st.frustrationCount}</td></tr>;
            })}
          </tbody>
        </table>
      )}
      {worst.length > 0 && (
        <span className="j1x-hint">最挫败：{worst.map((w) => `密度 ${w.density}（${w.stats.frustrationCount} 信号）`).join(" · ")}</span>
      )}
      <div className="j1x-inline">
        <MiniButton onClick={() => copy(dailyReport(sessions, new Date().toLocaleDateString()), "体验日报已复制（Markdown）")}>生成并复制日报</MiniButton>
      </div>
    </div>
  );
}

/* ------------------------------- 对账导出 ------------------------------- */

export function ReconcileExportPanel(): React.ReactElement {
  const probes = engineProbes();
  return (
    <div className="j1x-stack">
      <table className="j1x-table">
        <thead><tr><th>引擎</th><th>判据锚</th><th>状态</th></tr></thead>
        <tbody>
          {probes.map((p) => {
            let ok = false;
            try { ok = p.probe(); } catch { ok = false; }
            return (
              <tr key={p.engine}>
                <td>{p.engine}</td>
                <td>{p.anchor}</td>
                <td>{ok ? "✅" : "❌"}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      <span className="j1x-hint">v5 新增判据锚 {V5_NEW_ANCHORS.filter((a) => a.state === "green").length} 条机器可判绿 · {V5_NEW_ANCHORS.filter((a) => a.state === "gated").length} 条随闸门</span>
      <div className="j1x-inline">
        <MiniButton onClick={() => copy(buildReconcileMarkdown(), "完整对账表已复制（Markdown）")}>生成并复制完整对账表</MiniButton>
      </div>
      <span className="j1x-hint">对账表 = 二十项元数据 × 十二查 + 引擎探针，机器生成——与 _attic 归档同源。</span>
    </div>
  );
}

/* ------------------------------- 面板注册（SectionCard 组装） ------------------------------- */

export function V5Panels(): React.ReactElement {
  return (
    <>
      <SectionCard title="引擎实验室（滤波双引擎 / 增益反解 / DPI 归一）" f="F601·F611">
        <EngineLabPanel />
      </SectionCard>
      <SectionCard title="多屏拓扑图（接缝 / 身份卡 / DPI 变换）" f="F607·F613">
        <TopologyPanel />
      </SectionCard>
      <SectionCard title="滚轮标定向导（行高实测 / 幂律增益曲线）" f="F605·F612">
        <WheelCalPanel />
      </SectionCard>
      <SectionCard title="体验会话与日报（挫败密度排行）" f="章十三">
        <SessionReportPanel />
      </SectionCard>
      <SectionCard title="检查项对账导出（机器生成）" f="MD3附B">
        <ReconcileExportPanel />
      </SectionCard>
    </>
  );
}
