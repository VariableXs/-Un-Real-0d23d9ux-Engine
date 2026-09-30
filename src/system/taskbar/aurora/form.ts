/**
 * AURORA-10000 领域04 · 族0076 任务栏形态 + 族0080 任务栏行为（AI-16 批次，勿删）。
 * 纯函数布局决策：位置/岛形/高度/标签/显隐行为，供 Taskbar 消费。
 */
import { getD4 } from "./prefs";

export type TaskbarPos = "bottom-center" | "bottom-left" | "top" | "left" | "right";
export type TaskbarHeight = "slim" | "standard" | "tall";
export type TaskbarLabel = "icon" | "labeled" | "compact";
export type TaskbarPlate = "glass" | "solid" | "gradient";

export interface TaskbarForm {
  pos: TaskbarPos;
  /** 浮动岛（F01882）。 */
  island: boolean;
  /** 高度档（F01894）：px。 */
  height: number;
  label: TaskbarLabel;
  plate: TaskbarPlate;
  /** 屏幕边距（F01990）。 */
  margin: number;
  /** 投影（F01991）。 */
  shadow: boolean;
  /** 显示策略（F01981）。 */
  multiScreen: "primary" | "all";
}

const HEIGHTS: Record<TaskbarHeight, number> = { slim: 40, standard: 52, tall: 64 };

/** 组装当前任务栏形态（F01876~F01900 / F01981 / F01990 / F01991）。 */
export function taskbarForm(): TaskbarForm {
  const pos = (getD4<TaskbarPos>("F01876") ?? "bottom-center");
  const h = getD4<TaskbarHeight>("F01894") ?? "standard";
  return {
    pos,
    island: getD4<boolean>("F01882") ?? true,
    height: HEIGHTS[h],
    label: getD4<TaskbarLabel>("F01890") ?? "icon",
    plate: getD4<TaskbarPlate>("F01897") ?? "glass",
    margin: getD4<number>("F01990") ?? 0,
    shadow: getD4<boolean>("F01991") ?? true,
    multiScreen: getD4<TaskbarForm["multiScreen"]>("F01981") ?? "primary",
  };
}

export interface VisibilityInput {
  fullscreen: boolean;
  focused: boolean;
  pointerNearEdge: boolean;
  now: number;
}

export interface VisibilityDecision {
  visible: boolean;
  /** 让位原因（全屏/失焦隐藏），供遥测与教学提示。 */
  reason: "normal" | "fullscreen-yield" | "autohide" | "peek";
}

/** 显隐决策（F01886 失焦隐藏 / F01887 智能显隐 / F01976 全屏延迟 / F01977）。 */
export function visibilityDecision(input: VisibilityInput, now = Date.now()): VisibilityDecision {
  const autohide = getD4<boolean>("F01886") ?? false;
  const smart = getD4<boolean>("F01887") ?? true;
  const delay = getD4<number>("F01976") ?? 300;
  if (input.fullscreen && smart) {
    // 全屏让位：让位优先，绝不接管（守卫 §15.5）。
    return { visible: input.pointerNearEdge, reason: input.pointerNearEdge ? "peek" : "fullscreen-yield" };
  }
  if (autohide || getD4<boolean>("F01977")) {
    if (!input.focused && !input.pointerNearEdge) {
      return { visible: now - (visibilityDecision.lastHide ?? 0) > delay, reason: "autohide" };
    }
    return { visible: true, reason: "peek" };
  }
  visibilityDecision.lastHide = 0;
  return { visible: true, reason: "normal" };
}
visibilityDecision.lastHide = 0;

/** 夜间透明（F01985）与自动配色（F01984）：返回浮层不透明度。 */
export function plateOpacity(hour: number): number {
  const base = 0.92;
  const night = getD4<boolean>("F01985") && (hour >= 23 || hour < 6);
  return night ? base - 0.25 : base;
}

/** 性能模式（F01986）：关动效降级。 */
export function motionEnabled(): boolean { return !(getD4<boolean>("F01986") ?? false); }
