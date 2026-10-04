/**
 * U3-v7 ClockPanelPane 活体件（AI-U3 · 批次七 · clockpanel 引擎演示面）。
 *
 * 两层日历分工活体：悬停轻层（F549 Tooltip 三要素一行）+ 点击重层
 * （月历面板：6×7 网格、农历逐日、F560 节假日红灰双色、今日高亮、
 * 翻页跨月跨年、点灰显格翻月选中）。
 *
 * 词典纪律（十章）：面板浮层出路四条齐全——dictwalk 账本 u3-clock-panel
 * 条目；打开焦点落入、关闭焦点归还时钟槽（四章归还链）。
 */

import React, { useCallback, useRef, useState } from "react";
import { SectionCard } from "../settings/MouseJ1Panels";
import {
  buildMonthGrid, panelInit, panelToggle, panelShiftMonth, panelSelect, panelTitle,
  holidayCoverage, hoverDateLine, GRID_COLS, type CalendarCell,
} from "./labapi";
import { WEEKDAY_NAMES } from "./clockcal";

/** 演示锚定「今天」（与 ShellBar 演示时刻同日——2026-09-27 现场一致）。 */
const DEMO_TODAY: readonly [number, number, number] = [2026, 9, 27];

export function U3ClockPanelSection(): React.ReactElement {
  const [ps, setPs] = useState(() => panelInit(DEMO_TODAY));
  const [msg, setMsg] = useState("点时钟开面板（点击重层）——悬停按钮看轻层 Tooltip");
  const [hoverTip, setHoverTip] = useState(false);
  const clockRef = useRef<HTMLButtonElement | null>(null);

  const grid = buildMonthGrid(ps.viewYear, ps.viewMonth, DEMO_TODAY);
  const cov = holidayCoverage(ps.viewYear);

  const open = useCallback(() => {
    setPs((s) => panelToggle(s, DEMO_TODAY));
    setMsg("面板开——焦点已落入（Esc/点外/再点时钟/失焦四种出路）");
  }, []);
  const close = useCallback(() => {
    setPs((s) => ({ ...s, open: false }));
    setMsg("面板关——焦点归还时钟槽（四章归还链）");
    clockRef.current?.focus();
  }, []);

  const clickCell = useCallback((c: CalendarCell) => {
    setPs((s) => panelSelect(s, c));
    const h = c.holiday;
    const lunar = c.lunarText ? ` · 农历${c.lunarText}` : "";
    setMsg(`选中 ${c.y}-${c.m}-${c.d}${lunar}${h ? ` · ${h.kind === "holiday" ? "🔴" : "⚪"} ${h.name}` : ""}`);
  }, []);

  return (
    <SectionCard title="时钟面板装配区" f="F549/F560·v7">
      <div className="u3-lab-intro">
        两层日历活体：F549 悬停轻层（Tooltip 三要素一行 500ms）与点击重层（月历面板）各司其职；F560 节假日红灰双色（法定红、补班灰——标注克制）。农历逐日与 Tooltip 同一数据源（clockcal→内核同源，零漂移）。
      </div>

      {/* 缩比时钟槽 + 面板 */}
      <div
        className="u3-clock-stage"
        onClick={(e) => { if (!(e.target as HTMLElement).closest("[data-u3-float]")) { if (ps.open) close(); } }}
        onKeyDown={(e) => { if (e.key === "Escape" && ps.open) close(); }}
        tabIndex={-1}
      >
        <button
          ref={clockRef}
          type="button"
          className="u3-clock-slot"
          aria-expanded={ps.open}
          onClick={open}
          onMouseEnter={() => setHoverTip(true)}
          onMouseLeave={() => setHoverTip(false)}
        >
          09:41 · 2026-09-27
        </button>

        {/* 悬停轻层（F549：500ms、三要素一行、跟随锚点） */}
        {hoverTip && !ps.open && (
          <div className="u3-clock-tip" role="tooltip" data-u3-float="tip">
            {hoverDateLine(DEMO_TODAY[0], DEMO_TODAY[1], DEMO_TODAY[2])}
          </div>
        )}

        {/* 点击重层（月历面板——出路四条齐全） */}
        {ps.open && (
          <div className="u3-clock-panel" data-u3-float="panel" role="dialog" aria-label="日历面板">
            <div className="u3-clock-panel-head">
              <button type="button" className="j1x-btn" onClick={() => setPs((s) => panelShiftMonth(s, -1))} aria-label="上一月">◂</button>
              <span className="u3-clock-panel-title">{panelTitle(ps)}</span>
              <button type="button" className="j1x-btn" onClick={() => setPs((s) => panelShiftMonth(s, 1))} aria-label="下一月">▸</button>
            </div>
            <div className="u3-clock-grid" style={{ gridTemplateColumns: `repeat(${GRID_COLS}, 1fr)` }} role="grid">
              {WEEKDAY_NAMES.map((w, i) => (
                <span key={w} className={`u3-clock-wd ${i >= 5 ? "is-weekend" : ""}`} role="columnheader">周{w}</span>
              ))}
              {grid.map((c) => {
                const h = c.holiday;
                const isSel = ps.selected?.join("-") === `${c.y}-${c.m}-${c.d}`;
                return (
                  <button
                    key={`${c.y}-${c.m}-${c.d}-${c.inMonth ? "in" : "out"}`}
                    type="button"
                    role="gridcell"
                    className={[
                      "u3-clock-cell",
                      c.inMonth ? "" : "is-out",
                      c.isToday ? "is-today" : "",
                      isSel ? "is-selected" : "",
                      h?.kind === "holiday" ? "is-holiday" : "",
                      h?.kind === "workday" ? "is-workday" : "",
                    ].join(" ")}
                    onClick={() => clickCell(c)}
                    title={h ? `${h.name}${h.kind === "workday" ? "（补班）" : ""}` : c.lunarText ?? undefined}
                  >
                    <span className="u3-clock-day">{c.d}</span>
                    {c.lunarText && <span className={`u3-clock-lunar ${h ? "u3-clock-lunar-h" : ""}`}>{c.lunarText}</span>}
                  </button>
                );
              })}
            </div>
            <div className="u3-clock-panel-foot">
              <span className="u3-stat">🔴 法定 <b>{cov.covered}</b> 锚 · {cov.pendingNote}</span>
              <button type="button" className="j1x-btn" onClick={close}>关闭（Esc 同效）</button>
            </div>
          </div>
        )}
      </div>

      <div className="u3-row">
        <div>
          <div className="u3-name"><span className="fno">F549</span>两层分工</div>
          <div className="u3-desc">悬停轻（不点击即见）· 点击重（月历可翻可选）——两层互不干扰，数据同源</div>
        </div>
        <div className="u3-ctl" style={{ gridColumn: "2 / span 2" }}>
          <span className="u3-stat" role="status">{msg}</span>
        </div>
      </div>
      <div className="u3-row">
        <div>
          <div className="u3-name"><span className="fno">F560</span>节假日红灰</div>
          <div className="u3-desc">2026 七锚在册（元旦/春节/清明/劳动/端午/中秋/国庆——与农历两源互证）；2027 春节已锚，其余随 F122 更新</div>
        </div>
        <div className="u3-ctl" style={{ gridColumn: "2 / span 2" }}>
          {["2026-01-01 元旦", "2026-02-17 春节", "2026-04-05 清明", "2026-05-01 劳动节", "2026-06-19 端午", "2026-09-25 中秋", "2026-10-01 国庆", "2027-02-06 春节"].map((t) => (
            <span key={t} className="u3-badge ok">🔴 {t}</span>
          ))}
        </div>
      </div>
    </SectionCard>
  );
}
