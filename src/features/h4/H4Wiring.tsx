/**
 * H4 接线总览面板（v6 · 深化批次六）：
 * F351-F400 五十项 × 桌面表面的接线矩阵——每行一项（表面徽标 + 事件进出 + 人话说明），
 * 顶部审计汇总条（命名规范/挂载唯一/表面覆盖/登记册对齐四道审计的实时灯）。
 * 数据唯一来源 = consumers.ts BINDINGS（面板零自研逻辑——逻辑在契约层，壳在这里）。
 */

import React from "react";
import { BINDINGS, SURFACES, wiringReport, bindingFor, type Surface } from "./consumers";

const SURFACE_LABELS: Record<Surface, string> = {
  taskbar: "任务栏",
  tray: "托盘",
  "window-frame": "窗口框",
  "start-menu": "开始菜单",
  settings: "设置中心",
  "notify-center": "通知中心",
  desktop: "桌面",
  explorer: "资源管理器",
  terminal: "终端",
  "quick-settings": "快速设置",
  "alt-tab": "Alt+Tab",
  "task-view": "任务视图",
  "about-page": "关于页",
  "recovery-env": "恢复环境",
};

export function H4WiringPanel(): React.ReactElement {
  const report = wiringReport();
  const [filter, setFilter] = React.useState<string>("");
  const [selected, setSelected] = React.useState<string | null>(null);

  const rows = React.useMemo(() => {
    const q = filter.trim().toLowerCase();
    if (!q) return BINDINGS;
    return BINDINGS.filter((b) => b.item.toLowerCase().includes(q) || b.note.toLowerCase().includes(q) || b.surfaces.some((s) => SURFACE_LABELS[s].includes(q)));
  }, [filter]);

  const detail = selected ? bindingFor(selected) : null;

  return (
    <div className="h4-wiring">
      <div className="h4-wiring-audit" role="status">
        <span className={report.allGreen ? "h4-badge-ok" : "h4-badge-bad"}>{report.allGreen ? "✓ 接线审计全绿" : "✗ 接线审计有红"}</span>
        {report.audits.map((a) => (
          <span key={a.problems.join("|") || Math.random()} className={a.pass ? "h4-audit-chip ok" : "h4-audit-chip bad"}>
            {a.pass ? "✓" : "✗"} {a.problems.length === 0 ? "过" : `${a.problems.length} 项问题`}
          </span>
        ))}
        <span className="h4-audit-chip">
          {report.totalItems} 项 · 入 {report.totalEventsIn} / 出 {report.totalEventsOut} 事件
        </span>
      </div>

      <div className="h4-wiring-surfaces" role="list" aria-label="表面挂载统计">
        {report.perSurface.map((p) => (
          <span key={p.surface} className="h4-surface-chip" role="listitem">
            {SURFACE_LABELS[p.surface]} <b>{p.count}</b>
          </span>
        ))}
      </div>

      <div className="h4-wiring-toolbar">
        <input
          type="search"
          className="h4-wiring-search"
          placeholder="筛选项号 / 表面 / 说明……"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
          aria-label="筛选接线项"
        />
        <span className="h4-readonly">
          {rows.length}/{BINDINGS.length} 项
        </span>
      </div>

      <div className="h4-wiring-matrix" role="table" aria-label="五十项接线矩阵">
        {rows.map((b) => (
          <button
            key={b.item}
            type="button"
            className={selected === b.item ? "h4-wiring-row selected" : "h4-wiring-row"}
            onClick={() => setSelected(selected === b.item ? null : b.item)}
            aria-expanded={selected === b.item}
          >
            <span className="h4-wiring-item">{b.item}</span>
            <span className="h4-wiring-surfaces-mini">
              {SURFACES.filter((s) => b.surfaces.includes(s)).map((s) => (
                <i key={s} className="h4-wiring-surface-dot" title={SURFACE_LABELS[s]}>
                  {SURFACE_LABELS[s]}
                </i>
              ))}
            </span>
            <span className="h4-wiring-note">{b.note}</span>
            <span className="h4-wiring-ev">
              ↓{b.eventsIn.length} ↑{b.eventsOut.length}
            </span>
          </button>
        ))}
      </div>

      {detail && (
        <div className="h4-wiring-detail" role="region" aria-label={`${detail.item} 接线详情`}>
          <p className="h4-wiring-detail-title">
            {detail.item} · {detail.mountId}
          </p>
          <p className="h4-wiring-detail-line">入向（桌面 → 功能）：{detail.eventsIn.join("、") || "（无）"}</p>
          <p className="h4-wiring-detail-line">出向（功能 → 桌面）：{detail.eventsOut.join("、") || "（无）"}</p>
          <p className="h4-wiring-detail-line">{detail.note}</p>
        </div>
      )}
    </div>
  );
}
