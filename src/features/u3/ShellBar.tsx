/**
 * U3-v6 ShellBar 活体件（AI-U3 · 批次六 · shellbar 引擎的演示面）。
 *
 * 高密度纵深（F302 乙线）：一个 720px 缩比任务栏真排布——开始钮、
 * Win+数字槽位、托盘（溢出箭头）、时钟槽（F549 完整日期 tooltip）、
 * 通知中心抽屉（时间线三段）。全部交互走 shellbar 引擎纯函数，状态
 * 即时可见（三章 100ms 反馈红线）。
 *
 * 词典纪律（十章）：本件所有浮层出路四条齐全——登记于 dictwalk 账本
 * u3-tray-overflow / u3-action-center / u3-clock-tooltip 条目。
 */

import React, { useCallback, useMemo, useState } from "react";
import { SectionCard } from "../settings/MouseJ1Panels";
import {
  trayLayout, winNumberTarget, taskbarFocusNext, alwaysOnTopVerdict,
  actionCenterRect, timelineBand, clockTooltipRect, clockFullDateLine,
  TASKBAR_HEIGHT_DEFAULT_PX, type TaskbarSlot, type TrayItem, type NotifEntry,
} from "./engines";
import { U3_LAB_LABELS, u3Label } from "./labels";

/** 演示任务栏宽度（缩比：真 4K 任务栏的等比样本）。 */
const DEMO_TASKBAR_W = 720;
/** 演示屏幕（缩比几何同一套函数跑真预算）。 */
const DEMO_SCREEN = { w: 720, h: 240 };

const DEMO_TRAY: ReadonlyArray<TrayItem> = [
  { id: "net",   w: 16, pinned: true },
  { id: "vol",   w: 16, pinned: true },
  { id: "bat",   w: 16, pinned: false },
  { id: "ime",   w: 16, pinned: false },
  { id: "shield",w: 16, pinned: false },
  { id: "sync",  w: 16, pinned: false },
  { id: "upd",   w: 16, pinned: false },
];

const DEMO_SLOTS: ReadonlyArray<TaskbarSlot> = [
  { id: "files",  x: 56,  w: 36, instances: 1 },
  { id: "browse", x: 96,  w: 36, instances: 3 },
  { id: "notes",  x: 136, w: 36, instances: 0 },
  { id: "term",   x: 176, w: 36, instances: 2 },
  { id: "music",  x: 216, w: 36, instances: 1 },
];

const DEMO_NOTIFS: ReadonlyArray<NotifEntry> = [
  { id: "n1", atMs: Date.now() - 5 * 60_000,        kind: "banner" },
  { id: "n2", atMs: Date.now() - 3 * 3600_000,      kind: "quiet" },
  { id: "n3", atMs: Date.now() - 20 * 3600_000,     kind: "capture-block" },
  { id: "n4", atMs: Date.now() - 30 * 3600_000,     kind: "banner" },
];

const TRAY_GLYPH: Readonly<Record<string, string>> = {
  net: "⌥", vol: "🔊", bat: "🔋", ime: "文", shield: "🛡", sync: "↻", upd: "↑",
};

/** 缩比演示时刻（真实时钟槽的 F549 行）。 */
const DEMO_GREG = "2026-09-27";
const DEMO_WEEKDAY = "周日";
const DEMO_LUNAR = "八月十七";

export function U3ShellBarSection(): React.ReactElement {
  const [lang] = useState<"zh" | "en">("zh");
  const [focusedSlot, setFocusedSlot] = useState<string | null>(null);
  const [trayOpen, setTrayOpen] = useState(false);
  const [centerOpen, setCenterOpen] = useState(false);
  const [tipOpen, setTipOpen] = useState(false);
  const [lastMsg, setLastMsg] = useState("点任务栏交互——Win+数字/Win+T/托盘/时钟全部接引擎活跑");

  const tray = useMemo(() => trayLayout(DEMO_TRAY, DEMO_TASKBAR_W), []);
  const ac = useMemo(() => actionCenterRect(DEMO_SCREEN, TASKBAR_HEIGHT_DEFAULT_PX), []);
  const tip = useMemo(
    () => clockTooltipRect({ x: DEMO_TASKBAR_W - 88, w: 80, y: DEMO_SCREEN.h - TASKBAR_HEIGHT_DEFAULT_PX }, DEMO_SCREEN, 220),
    [],
  );
  const topVerdict = useMemo(() => alwaysOnTopVerdict("normal", false), []);

  /** 统一收浮层（十章：同一套关闭规则——开一个关其余）。 */
  const closeAll = useCallback((keep: "tray" | "center" | "tip" | null) => {
    setTrayOpen(keep === "tray");
    setCenterOpen(keep === "center");
    setTipOpen(keep === "tip");
  }, []);

  const pressWinNumber = useCallback((n: number) => {
    const t = winNumberTarget(DEMO_SLOTS, n);
    if (!t) { setLastMsg(`槽位 ${n} 不存在——空手（显性）`); return; }
    setLastMsg(t.action === "launch" ? `Win+${n} → 「${t.slot.id}」无实例 → 启动`
      : t.action === "focus" ? `Win+${n} → 「${t.slot.id}」单实例 → 聚焦`
      : `Win+${n} → 「${t.slot.id}」${t.slot.instances} 实例 → 列窗清单`);
  }, []);

  const pressWinT = useCallback((backwards: boolean) => {
    const next = taskbarFocusNext(DEMO_SLOTS, focusedSlot, backwards);
    setFocusedSlot(next);
    setLastMsg(`Win+T${backwards ? "（反向）" : ""} → 焦点环至「${next ?? "无"}」`);
  }, [focusedSlot]);

  const bands = useMemo(() => {
    const now = Date.now();
    return DEMO_NOTIFS.map((n) => ({ ...n, band: timelineBand(n.atMs, now) }));
  }, []);

  return (
    <SectionCard title={u3Label("shellTitle", lang, U3_LAB_LABELS)} f="F516/F535/F536/F548/F549·v6">
      <div className="u3-lab-intro">
        shell 层活体：托盘溢出预算、Win+数字槽位裁决、Win+T 焦点环、置顶三态、通知中心时间线、F549 完整日期——全部 shellbar 引擎纯函数驱动。
      </div>

      {/* 缩比任务栏真排布 */}
      <div className="u3-shell-stage" role="img" aria-label="任务栏活体演示">
        <div className="u3-shell-taskbar" style={{ width: DEMO_TASKBAR_W, height: TASKBAR_HEIGHT_DEFAULT_PX }}>
          <span className="u3-shell-start">⊞</span>
          {DEMO_SLOTS.map((s, i) => (
            <button
              key={s.id}
              type="button"
              className={`u3-shell-slot ${focusedSlot === s.id ? "is-focus" : ""}`}
              style={{ left: s.x, width: s.w }}
              onClick={() => pressWinNumber(i + 1)}
              title={`槽位 ${i + 1} · ${s.instances} 实例`}
            >
              {s.id.slice(0, 2)}
              {s.instances > 1 && <span className="u3-shell-dots">{"•".repeat(Math.min(s.instances, 3))}</span>}
            </button>
          ))}
          <span className="u3-shell-tray">
            {tray.visible.map((t) => <span key={t.id} className="u3-shell-tray-icon">{TRAY_GLYPH[t.id] ?? "·"}</span>)}
            {tray.overflow.length > 0 && (
              <button
                type="button"
                className={`u3-shell-tray-arrow ${trayOpen ? "is-open" : ""}`}
                aria-expanded={trayOpen}
                onClick={() => closeAll(trayOpen ? null : "tray")}
              >▴</button>
            )}
          </span>
          <button
            type="button"
            className="u3-shell-clock"
            onClick={() => closeAll(tipOpen ? null : "tip")}
          >{DEMO_GREG}</button>
        </div>

        {/* 托盘溢出浮层（出路四条齐全——dictwalk 账本条目） */}
        {trayOpen && (
          <div className="u3-shell-flyout" style={{ right: 60, bottom: TASKBAR_HEIGHT_DEFAULT_PX + 4 }} role="menu">
            <div className="u3-shell-flyout-head">托盘溢出（{tray.overflow.length}）</div>
            {tray.overflow.map((t) => <div key={t.id} className="u3-shell-flyout-row">{TRAY_GLYPH[t.id] ?? "·"} {t.id}</div>)}
          </div>
        )}

        {/* F549 完整日期 tooltip */}
        {tipOpen && (
          <div className="u3-shell-flyout u3-shell-tip" style={{ left: tip.x, bottom: DEMO_SCREEN.h - tip.y + 4 }}>
            {clockFullDateLine(DEMO_GREG, DEMO_WEEKDAY, DEMO_LUNAR)}
          </div>
        )}

        {/* 通知中心抽屉（时间线三段） */}
        {centerOpen && (
          <div className="u3-shell-flyout u3-shell-center" style={{ left: ac.x, bottom: TASKBAR_HEIGHT_DEFAULT_PX + 8, width: ac.w }} role="dialog">
            <div className="u3-shell-flyout-head">通知中心</div>
            {(["today", "morning", "earlier"] as const).map((band) => {
              const rows = bands.filter((b) => b.band === band);
              if (rows.length === 0) return null;
              const bandName = band === "today" ? "今日" : band === "morning" ? "今晨" : "更早";
              return (
                <div key={band} className="u3-shell-band">
                  <div className="u3-shell-band-name">{bandName}</div>
                  {rows.map((r) => <div key={r.id} className="u3-shell-flyout-row">{r.kind === "capture-block" ? "🛡" : r.kind === "quiet" ? "🌙" : "🔔"} {r.kind}</div>)}
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* 控制行 */}
      <div className="u3-row">
        <div>
          <div className="u3-name"><span className="fno">F535/536</span>Win 键族活跑</div>
          <div className="u3-desc">槽位裁决三态（启动/聚焦/列窗）+ 焦点环循环——越界空手显性</div>
        </div>
        <div className="u3-ctl" style={{ gridColumn: "2 / span 2" }}>
          <button type="button" className="j1x-btn" onClick={() => pressWinNumber(2)}>Win+2</button>
          <button type="button" className="j1x-btn" onClick={() => pressWinNumber(3)}>Win+3（无实例）</button>
          <button type="button" className="j1x-btn" onClick={() => pressWinT(false)}>Win+T</button>
          <button type="button" className="j1x-btn" onClick={() => pressWinT(true)}>Shift+Win+T</button>
        </div>
      </div>
      <div className="u3-row">
        <div>
          <div className="u3-name"><span className="fno">F516/F548</span>通知与置顶</div>
          <div className="u3-desc">通知中心时间线三段 + 任务管理器置顶裁决（{topVerdict.allow ? "允许" : "拒绝"}）</div>
        </div>
        <div className="u3-ctl" style={{ gridColumn: "2 / span 2" }}>
          <button type="button" className="j1x-btn" onClick={() => closeAll(centerOpen ? null : "center")}>通知中心</button>
          <span className="u3-stat" role="status">{lastMsg}</span>
        </div>
      </div>
    </SectionCard>
  );
}
