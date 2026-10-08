/**
 * H4 任务栏挂载层——渲染壳（v8）：
 * 把 taskbarLayer.ts 的芯片模型渲染成任务栏右侧的 H4 芯片组。
 * 数据源全部真实：专注态/布局态走 h4Bus 契约事件 + 引擎持久化；电池走 navigator
 * Battery API（读不到就不出芯片——零编造）；任务徽标走 F369 引擎持久化。
 * 点击 = h4Bus.emit 契约事件（契约执法总线拒绝未登记事件——零静默）。
 */

import React from "react";
import * as f369 from "../../system/h4/f369-taskCenter";
import * as f373 from "../../system/h4/f373-keyboardLayouts";
import type { FocusRun } from "../../system/h4/f363-focusTimer";
import type { BatteryReading } from "../../system/h4/f366-trayBattery";
import { h4Bus } from "./bus";
import { xlog } from "./xlog";
import { buildTaskbarChips, type H4Chip } from "./taskbarLayer";

interface BatteryManagerLike {
  level: number;
  charging: boolean;
  addEventListener?(type: string, cb: () => void): void;
  removeEventListener?(type: string, cb: () => void): void;
}

function readBattery(cb: (b: BatteryReading | null) => void): void {
  const nav = navigator as Navigator & { getBattery?: () => Promise<BatteryManagerLike> };
  if (typeof nav.getBattery !== "function") {
    cb(null); // 无真实源——零编造，不出芯片
    return;
  }
  nav
    .getBattery()
    .then((b) => {
      const push = () => cb({ percent: Math.round(b.level * 100), plugged: b.charging });
      push();
      b.addEventListener?.("levelchange", push);
      b.addEventListener?.("chargingchange", push);
    })
    .catch(() => cb(null));
}

export function H4TaskbarLayer(): React.ReactElement | null {
  const [focusRun, setFocusRun] = React.useState<FocusRun | null>(null);
  const [battery, setBattery] = React.useState<BatteryReading | null>(null);
  const [tick, setTick] = React.useState(0);

  /* 专注态：契约事件驱动（focus.start / focus.abandon —— 桌面壳或面板 emit） */
  React.useEffect(() => {
    const offStart = h4Bus.on("focus.start", (p) => {
      const pl = p as { minutes?: number } | null;
      const minutes = typeof pl?.minutes === "number" ? pl.minutes : 25;
      setFocusRun({ day: new Date().toISOString().slice(0, 10), plannedMinutes: minutes, startedAt: Date.now(), endedAt: null, outcome: "running" });
    });
    const offAbandon = h4Bus.on("focus.abandon", () => setFocusRun((r) => (r ? { ...r, outcome: "abandoned", endedAt: Date.now() } : r)));
    return () => {
      offStart();
      offAbandon();
    };
  }, []);

  /* 电池：真实 API（读不到不出芯片） */
  React.useEffect(() => {
    readBattery(setBattery);
  }, []);

  /* 徽标秒级刷新（专注徽标实时性判据——1s 一拍） */
  React.useEffect(() => {
    if (!focusRun) return;
    const t = window.setInterval(() => setTick((v) => v + 1), 1000);
    return () => window.clearInterval(t);
  }, [focusRun]);

  const layout = f373.activeLayout(f373.loadState());
  const taskState = f369.loadPersisted();
  const taskBadge = taskState.tasks.filter((t) => f369.effectivelyRunning(t, taskState)).length;
  void tick; // tick 只驱动重渲染（徽标 mm:ss 逐秒走字）

  const chips: H4Chip[] = buildTaskbarChips({
    focusRun,
    now: Date.now(),
    layoutName: layout.name,
    battery,
    batteryMode: "iconPercent",
    taskBadge,
  });

  if (chips.length === 0) return null;

  const onChip = (chip: H4Chip): void => {
    if (!chip.clickEvent) return;
    const v = h4Bus.emit(chip.clickEvent, { from: "taskbar-layer" });
    if (!v.ok) xlog.log("warn", "h4-taskbar", "click-event-rejected", v.reason);
  };

  return (
    <div className="h4-taskbar-layer" role="group" aria-label="H4 任务栏状态芯片">
      {chips.map((c) => (
        <button
          key={c.id}
          type="button"
          className={`h4-tb-chip${c.tone !== "none" ? ` tone-${c.tone}` : ""}${c.blink ? " blink" : ""}`}
          title={c.title}
          onClick={() => onChip(c)}
          aria-label={c.title}
        >
          {c.text}
        </button>
      ))}
    </div>
  );
}
