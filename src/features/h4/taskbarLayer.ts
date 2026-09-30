/**
 * H4 任务栏挂载层——纯模型（v8 · 深化批次八）：
 * H4 功能在真实任务栏上的芯片建模：专注徽标（F363）/ 任务中心徽标（F369）/
 * 布局指示（F373）/ 电池（F366）。渲染顺序固定（交互词典）、无数据源不显示
 * （零编造：没有真实读数就不出芯片——不摆假读数充门面）。
 * 判据锚定：F363 徽标实时性 / F373 任务栏语言指示 / F366 托盘电池三档 / F369 中心徽标。
 */

import * as f363 from "../../system/h4/f363-focusTimer";
import * as f366 from "../../system/h4/f366-trayBattery";
import type { FocusRun } from "../../system/h4/f363-focusTimer";
import type { BatteryReading, BatteryDisplayMode } from "../../system/h4/f366-trayBattery";

export type H4ChipId = "focus" | "tasks" | "layout" | "battery";

export interface H4Chip {
  id: H4ChipId;
  text: string;
  /** 悬停提示（三件套之一：名称 + 读数——不假装可点的东西不配提示）。 */
  title: string;
  /** 点击发出的总线事件（必须已在 BINDINGS 登记——契约执法总线会拒绝未登记事件）。 */
  clickEvent: string | null;
  /** 低电独立视觉（F366 判据：黄/红/闪烁三态）。 */
  tone: "none" | "yellow" | "red";
  blink: boolean;
}

export interface TaskbarChipInput {
  /** 进行中的专注时段（null = 无——不出芯片）。 */
  focusRun: FocusRun | null;
  now: number;
  /** 当前键盘布局名（F373 任务栏指示恒显）。 */
  layoutName: string;
  /** 电池读数（null = 无真实源——零编造，不出芯片）。 */
  battery: BatteryReading | null;
  batteryMode: BatteryDisplayMode;
  /** 任务中心运行数（0 = 不出芯片）。 */
  taskBadge: number;
}

/** 芯片顺序固定（交互词典：专注 → 任务 → 布局 → 电池）。 */
export function buildTaskbarChips(input: TaskbarChipInput): H4Chip[] {
  const chips: H4Chip[] = [];
  if (input.focusRun && input.focusRun.outcome === "running") {
    chips.push({
      id: "focus",
      text: f363.badgeText(input.focusRun, input.now),
      title: `专注中（${input.focusRun.plannedMinutes} 分钟档）——点击放弃本时段`,
      clickEvent: "focus.abandon",
      tone: "none",
      blink: false,
    });
  }
  if (input.taskBadge > 0) {
    chips.push({
      id: "tasks",
      text: String(input.taskBadge),
      title: `${input.taskBadge} 个后台任务进行中——任务视图侧栏可管`,
      clickEvent: "task.global-pause",
      tone: "none",
      blink: false,
    });
  }
  chips.push({
    id: "layout",
    text: input.layoutName,
    title: "键盘布局——点击轮切（Win+空格 同义）",
    clickEvent: "hotkey.win-space",
    tone: "none",
    blink: false,
  });
  if (input.battery) {
    const r = f366.renderTray(input.battery, input.batteryMode);
    chips.push({
      id: "battery",
      text: r.percentText ?? `${Math.round(f366.clampPercent(input.battery.percent))}%`,
      title: `电量 ${f366.clampPercent(input.battery.percent)}%${r.bolt ? " · 插电中" : ""}`,
      clickEvent: null, // 电量不可点（没有登记的打开事件——不假装可点）
      tone: r.low.color === "none" ? "none" : r.low.color,
      blink: r.low.blink,
    });
  }
  return chips;
}

/** 专注放弃的确认语义（破坏性？否——放弃有真实时长记账，不需二次确认但要有反馈）。 */
export function focusAbandonConfirm(run: FocusRun, now: number): { needConfirm: boolean; reason: string } {
  const used = f363.actualMinutes({ ...run, outcome: "abandoned", endedAt: now }, now);
  return used >= 5
    ? { needConfirm: false, reason: `已专注 ${used} 分钟将如实记账` }
    : { needConfirm: false, reason: "不足 5 分钟按 1 分钟记账（有始有终但不虚报）" };
}
