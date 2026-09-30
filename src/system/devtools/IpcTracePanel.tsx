import { useEffect, useState } from "react";
import { useHotkey } from "../../lib/keymap/hooks";
import {
  buildWaterfall, clearIpcTrace, filterIpcTrace, subscribeIpcTrace,
  type IpcTraceRecord, IPC_SLOW_MS,
} from "../../lib/ipcTrace";

/**
 * AI-20 M-80：IPC 调用追踪面板（仅 dev 构建挂载；Ctrl+Alt+F12 打开/关闭）。
 * release 构建此组件零引用（App.tsx 以 import.meta.env.DEV 门控），
 * 键位不注册、代码被死代码剔除（dist rg "ipcTrace" 零命中验收）。
 * 瀑布图 + >100ms 标红 + 按命令名过滤。
 */
export function IpcTracePanel(): React.ReactElement | null {
  const [open, setOpen] = useState(false);
  const [records, setRecords] = useState<readonly IpcTraceRecord[]>([]);
  const [query, setQuery] = useState("");

  useHotkey("ctrl+alt+f12", () => setOpen((v) => !v), { scope: "window", priority: 50 });

  useEffect(
    () => subscribeIpcTrace((recs) => setRecords([...recs])),
    [],
  );

  if (!open) return null;
  const filtered = filterIpcTrace(records, query);
  const rows = buildWaterfall(filtered);
  const slow = filtered.filter((r) => r.ms > IPC_SLOW_MS).length;

  return (
    <div className="ipctrace-overlay" data-testid="ipctrace-panel" role="dialog" aria-label="IPC Trace">
      <div className="ipctrace-card">
        <div className="row gap8" style={{ alignItems: "center", marginBottom: 8 }}>
          <strong>IPC Trace View</strong>
          <span className="dim small">{filtered.length} calls · {slow} slow (&gt;{IPC_SLOW_MS}ms)</span>
          <span className="flex-1" />
          <input
            className="text-input"
            style={{ width: 180 }}
            placeholder="filter: cmd…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            aria-label="filter commands"
          />
          <button type="button" className="btn ghost" onClick={() => clearIpcTrace()}>Clear</button>
          <button type="button" className="btn" onClick={() => setOpen(false)}>Esc</button>
        </div>
        <div className="ipctrace-list">
          {rows.length === 0 && <div className="dim small" style={{ padding: 8 }}>（暂无调用记录）</div>}
          {rows.map(({ rec, left, width, slow: isSlow }, i) => (
            <div key={i} className={`ipctrace-row ${isSlow ? "slow" : ""} ${rec.ok ? "" : "fail"}`}>
              <span className="ipctrace-cmd ellipsis" title={rec.cmd}>{rec.cmd}</span>
              <span className="ipctrace-bar-wrap">
                <span className="ipctrace-bar" style={{ left: `${left * 100}%`, width: `${Math.max(width * 100, 1.5)}%` }} />
              </span>
              <span className="ipctrace-ms">{rec.ms.toFixed(1)}ms</span>
              {!rec.ok && <span className="ipctrace-err ellipsis" title={rec.error ?? ""}>{rec.error}</span>}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
