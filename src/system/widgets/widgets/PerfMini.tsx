/**
 * N-09 性能迷你条：接现有只读命令 ipc.perfCpu()（perf_cpu → number[]，逐核占用%）。
 * 诚实边界：命令失败（非 Tauri 环境/权限缺失）时渲染禁用卡并注明需要 N-19 数据源。
 */
import { useState } from "react";
import { LABELS, useLaneLang } from "../labels";
import { useRefresh } from "./common";
import { ipc } from "../../../lib/ipc";

export default function PerfMini({ paused }: { paused: boolean }): React.ReactElement {
  const lang = useLaneLang();
  const t = LABELS[lang];
  const [cores, setCores] = useState<number[] | null>(null);
  const [failed, setFailed] = useState(false);

  useRefresh(() => {
    void ipc
      .perfCpu()
      .then((v) => {
        setCores(v);
        setFailed(false);
      })
      .catch(() => setFailed(true));
  }, 3000, paused);

  if (failed) {
    return <div className="wgt-perf off">{t.perfNoData}</div>;
  }
  const list = cores ?? [];
  const avg = list.length > 0 ? Math.round(list.reduce((a, b) => a + (Number(b) || 0), 0) / list.length) : null;
  return (
    <div className="wgt-perf">
      <div className="wgt-perf-head">
        <span>{t.perf}</span>
        <strong>{avg === null ? "…" : `${avg}%`}</strong>
      </div>
      <div className="wgt-perf-bars">
        {list.slice(0, 8).map((v, i) => (
          <span key={i} className="wgt-perf-bar" data-level={v > 85 ? "hot" : v > 50 ? "warm" : "cool"} style={{ height: `${Math.min(100, Math.max(4, v))}%` }} />
        ))}
      </div>
    </div>
  );
}