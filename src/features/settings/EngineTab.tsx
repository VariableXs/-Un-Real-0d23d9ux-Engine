/**
 * 阶段 6.11（任务 53/57/61UI/64UI）EngineTab.tsx — 引擎组设置页：
 * ① 引擎开关（默认关，可整段禁用）② 自动休眠阈值 ③ 画质三档（办公/均衡/游戏，
 * 参数化+用户自定义）④ 资源配额 ⑤ 延迟实测面板（真实样本，无样本时如实显示）
 * ⑥ 申请式授权（任务 61UI：Wine/引擎进程能力申请卡）⑦ 诚实声明（任务 64UI：
 * 对策+残余风险双栏；「内核 VMX 未实现、现用 Hyper-V 底座」显式落点）。
 */
import { useEffect, useState } from "react";
import type { EngineQualityTier, Settings } from "../../lib/settings";
import { pushToast } from "../../state/uiStore";
import {
  qualityParamsFor,
  latencyP95,
  latencyTotal,
  type LatencySegments,
} from "../../system/engine/engineModel";
import {
  capabilityStore,
  decideCapabilityRequest,
  pushCapabilityRequest,
  useCapabilityLatency,
  useCapabilityRequests,
  type CapabilityScope,
} from "../security/capabilityRequest";
import { HonestyDeclareCard } from "../security/HonestyDeclareCard";
import { errMessage, ipc, type RamCacheStatsT } from "../../lib/ipc";

const QUALITY_LABELS: Record<EngineQualityTier, string> = {
  office: "办公（24fps / 4 Mbps）",
  balanced: "均衡（30fps / 8 Mbps）",
  gaming: "游戏（60fps / 20 Mbps）",
};

/** 单条延迟样本展示。 */
function LatencyRow(props: { seg: LatencySegments; at: number }): React.ReactElement {
  const s = props.seg;
  return (
    <tr>
      <td style={{ textAlign: "right" }}>{latencyTotal(s).toFixed(1)}</td>
      <td>{s.captureMs.toFixed(1)}</td>
      <td>{s.encodeMs.toFixed(1)}</td>
      <td>{s.transmitMs.toFixed(1)}</td>
      <td>{s.decodeMs.toFixed(1)}</td>
      <td>{s.composeMs.toFixed(1)}</td>
      <td className="dim small">{new Date(props.at).toLocaleTimeString()}</td>
    </tr>
  );
}

export function EngineTab(props: {
  settings: Settings;
  onPatch: (patch: Partial<Settings>) => void;
}): React.ReactElement {
  const s = props.settings;
  const set = <K extends keyof Settings>(key: K, value: Settings[K]): void =>
    props.onPatch({ [key]: value } as Partial<Settings>);
  const requests = useCapabilityRequests();
  const pending = requests.filter((r) => r.status === "pending");
  const [checkedVmx, setCheckedVmx] = useState(false);
  const [cache, setCache] = useState<RamCacheStatsT | null>(null);

  const refreshCache = (): void => {
    void ipc
      .ramcacheStats()
      .then((s) => setCache(s))
      .catch((e) => pushToast("error", errMessage(e).message));
  };
  const clearCache = (): void => {
    void ipc
      .ramcacheClear()
      .then((s) => {
        setCache(s);
        pushToast("info", "缓存已全清（只缓不落盘，清空即消失）");
      })
      .catch((e) => pushToast("error", errMessage(e).message));
  };

  // 延迟实测面板：读会话内真实样本（引擎代理经 engine://state 上报；无样本如实显示）。
  const latencyLog = useCapabilityLatency();
  const samples = latencyLog.slice(-8);
  const p95 = latencyP95(samples.map((x) => latencyTotal(x.seg)));

  const effectiveParams = qualityParamsFor(s.engineQuality, s.engineQualityCustom as never);

  // S3.8 公示：ramcache 统计（挂载即查；刷新/清空显式动作）。
  useEffect(() => {
    refreshCache();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div className="tab-body">
      <p className="dim small">
        隐形 Windows 引擎：需要原生兼容性时 Windows 作为后台引擎被拉起，应用窗口无缝出现在
        Variable 桌面。默认关闭；纯 Wine 用户可整段禁用本通道。
      </p>

      {/* ① 开关 + 休眠 + 配额 */}
      <label className="field" style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <input
          type="checkbox"
          checked={s.engineEnabled}
          onChange={(e) => set("engineEnabled", e.target.checked)}
        />
        <span>
          启用隐形 Windows 引擎
          <span className="dim small"> · 关闭时在跑应用会先提示保存再优雅关闭，Wine 通道不受影响</span>
        </span>
      </label>
      <div className="field">
        <span className="field-label">
          自动休眠阈值
          <span className="dim small"> · 空闲 N 分钟后内存快照写差分盘并休眠（0 = 从不）</span>
        </span>
        <select
          value={s.engineHibernateIdleMin}
          onChange={(e) => set("engineHibernateIdleMin", Number(e.target.value))}
          aria-label="引擎自动休眠阈值（分钟）"
          style={{ width: "100%" }}
        >
          {[0, 5, 15, 30, 60].map((m) => (
            <option key={m} value={m}>{m === 0 ? "从不自动休眠" : `${m} 分钟`}</option>
          ))}
        </select>
      </div>
      <div className="field">
        <span className="field-label">
          CPU 配额
          <span className="dim small"> · 0 = 自动（物理核一半，保底 2 核）</span>
        </span>
        <select
          value={s.engineQuotaCores}
          onChange={(e) => set("engineQuotaCores", Number(e.target.value))}
          aria-label="引擎 CPU 配额（核）"
          style={{ width: "100%" }}
        >
          <option value={0}>自动</option>
          {[2, 4, 6, 8].map((n) => (
            <option key={n} value={n}>{n} 核</option>
          ))}
        </select>
      </div>

      {/* ③ 画质三档 + 自定义 */}
      <div className="field">
        <span className="field-label">
          画质档位
          <span className="dim small"> · 切换即时生效不重连（码率参数运行时下发）</span>
        </span>
        <select
          value={s.engineQuality}
          onChange={(e) => set("engineQuality", e.target.value as EngineQualityTier)}
          aria-label="引擎画质档位"
          style={{ width: "100%" }}
        >
          {(Object.keys(QUALITY_LABELS) as EngineQualityTier[]).map((k) => (
            <option key={k} value={k}>{QUALITY_LABELS[k]}</option>
          ))}
        </select>
      </div>
      <div className="field">
        <span className="field-label">
          自定义参数覆盖
          <span className="dim small"> · 留空 = 纯预设；填了才覆盖对应项（当前生效：{" "}
            {effectiveParams.fps} fps / {effectiveParams.bitrateKbps} Kbps / {effectiveParams.codecPreset}）
          </span>
        </span>
        <select
          value={s.engineQualityCustom ? "custom" : "preset"}
          onChange={(e) =>
            set(
              "engineQualityCustom",
              e.target.value === "custom"
                ? { fps: effectiveParams.fps, bitrateKbps: effectiveParams.bitrateKbps, codecPreset: effectiveParams.codecPreset, captureIntervalMs: effectiveParams.captureIntervalMs }
                : null,
            )
          }
          aria-label="画质自定义覆盖开关"
          style={{ width: "100%" }}
        >
          <option value="preset">跟随预设</option>
          <option value="custom">自定义（锁定当前值后手改 JSON）</option>
        </select>
      </div>

      {/* ⑤ 延迟实测面板 */}
      <h3 className="w11-sec-title">延迟实测（端到端五段拆解）</h3>
      {samples.length === 0 ? (
        <p className="dim small">
          暂无实测样本 —— 延迟样本由引擎代理在会话中实测上报（采集/编码/传输/解码/合成五段），
          办公档目标 ≤50 ms；实机走查与三次方差 ≤5% 校验移交终局联验归档。
        </p>
      ) : (
        <table className="engine-latency-table" style={{ fontSize: 12, borderCollapse: "collapse" }}>
          <thead>
            <tr>
              <th>合计 ms</th><th>采集</th><th>编码</th><th>传输</th><th>解码</th><th>合成</th><th>时刻</th>
            </tr>
          </thead>
          <tbody>
            {samples.map((x, i) => <LatencyRow key={i} seg={x.seg} at={x.at} />)}
          </tbody>
          <tfoot>
            <tr>
              <td><strong>P95 {p95.toFixed(1)} ms</strong></td>
              <td colSpan={6} className="dim small">
                {p95 > 0 && p95 <= 50 ? "达标（办公档目标 ≤50 ms）" : `目标 ≤50 ms（当前档位 ${QUALITY_LABELS[s.engineQuality]}）`}
              </td>
            </tr>
          </tfoot>
        </table>
      )}

      {/* ⑥ 申请式授权（任务 61UI） */}
      <h3 className="w11-sec-title">能力申请（申请式授权）</h3>
      <p className="dim small">
        Wine / 引擎进程默认无网络、无宿主盘；越权能力必须在这里逐条申请，默认拒绝。
      </p>
      {pending.length === 0 ? (
        <p className="dim small">当前没有待审批的能力申请。</p>
      ) : (
        pending.map((r) => (
          <div key={r.id} className="field" style={{ border: "1px solid rgb(255 255 255 / 0.12)", borderRadius: 8, padding: 10 }}>
            <span className="field-label">
              {r.subject} 申请 <strong>{r.scope}</strong>
              <span className="dim small"> · {r.reason}</span>
            </span>
            <div className="vwm-tp-card-actions" style={{ marginTop: 6 }}>
              <button
                type="button"
                className="btn primary"
                onClick={() => capabilityStore.setState({ requests: decideCapabilityRequest(capabilityStore.getState().requests, r.id, "granted") })}
              >
                本次允许
              </button>
              <button
                type="button"
                className="btn"
                onClick={() => capabilityStore.setState({ requests: decideCapabilityRequest(capabilityStore.getState().requests, r.id, "denied") })}
              >
                拒绝
              </button>
            </div>
          </div>
        ))
      )}
      <div className="field">
        <span className="field-label">
          模拟能力申请（验收演示）
          <span className="dim small"> · 正式申请由内核/Wine 隔离链（任务 61K）经系统事件发起</span>
        </span>
        <button
          type="button"
          className="btn"
          onClick={() => {
            const scope: CapabilityScope = "net";
            pushCapabilityRequest({ subject: "wine:notepad.exe", scope, reason: "应用内检查更新需要访问网络" });
          }}
        >
          发起示例申请（wine:notepad.exe · net）
        </button>
      </div>

      {/* ⑦ 诚实声明（任务 64UI；设置页落点） */}
      <h3 className="w11-sec-title">引擎热数据缓存（ramcache）</h3>
      <p className="dim small">
        只缓不落盘：数据只存在内存，关机/拔盘即消失，U 盘零残留（构造性保证）。
        盘上文件被另一系统改写后自动失效重读。
      </p>
      {cache ? (
        <p className="dim small">
          条目 {cache.entries} · 占用 {(cache.bytes / 1024 / 1024).toFixed(1)} /{" "}
          {(cache.budgetBytes / 1024 / 1024).toFixed(0)} MiB · 命中率{" "}
          {(cache.hitRate * 100).toFixed(0)}%（查询 {cache.hits + cache.misses} 次，逐出{" "}
          {cache.evictions} 条）
        </p>
      ) : (
        <p className="dim small">尚未读取统计（点刷新）。</p>
      )}
      <div className="vwm-tp-card-actions" style={{ marginTop: 6 }}>
        <button type="button" className="btn" onClick={refreshCache}>
          刷新统计
        </button>
        <button type="button" className="btn" onClick={clearCache}>
          全部清空
        </button>
      </div>

      <h3 className="w11-sec-title">诚实声明</h3>
      <label className="field" style={{ display: "flex", gap: 8, alignItems: "center" }}>
        <input type="checkbox" checked={checkedVmx} onChange={(e) => setCheckedVmx(e.target.checked)} />
        <span className="dim small">展开威胁对策与残余风险双栏（含 VM 底座现状声明）</span>
      </label>
      {checkedVmx && <HonestyDeclareCard />}
    </div>
  );
}
