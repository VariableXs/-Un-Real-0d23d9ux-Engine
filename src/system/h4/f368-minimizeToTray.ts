/**
 * F368 最小化到托盘（H 域 · AI-H4）：
 * 长驻应用（下载器/同步类）可「关闭到托盘」：点 × 实际最小化到托盘（应用声明此行为时
 * × 悬停提示「最小化到托盘」——不骗人），真退出走托盘右键「退出」；托盘图标带活动徽标
 * （下载中显示微进度）；托盘化应用不占任务栏按钮。
 * 判据（主册 F368）：声明行为与悬停提示；徽标实时性；真退出路径；任务栏不显示判据；
 * 托盘溢出收纳兼容（F075 语义）。
 * 依赖锚点：F075 托盘系统。
 */

import { defaultStore, h4Key, readJson, type KvStore, writeJson } from "./internal/store";

/** 应用声明的关闭行为。 */
export type CloseBehavior = "normal" | "closeToTray";

export interface TrayApp {
  appId: string;
  behavior: CloseBehavior;
  /** 活动徽标（0-100 进度；null=无活动）。 */
  activityPct: number | null;
}

export interface TrayRuntime {
  /** 托盘化中的应用（× 之后住在这里）。 */
  resident: TrayApp[];
  /** 真退出的应用（托盘右键「退出」后出清）。 */
  gone: string[];
}

export interface CloseClickResult {
  runtime: TrayRuntime;
  /** 窗口去向：taskbar=正常关；tray=进托盘。 */
  windowGoesTo: "taskbar" | "tray";
  /** × 悬停提示文案（声明行为时必给——不骗人判据）。 */
  tooltip: string | null;
}

/** 点 ×：应用声明 closeToTray → 最小化到托盘并给预告提示；未声明 → 真关。 */
export function closeClick(runtime: TrayRuntime, app: TrayApp): CloseClickResult {
  if (app.behavior !== "closeToTray") {
    return { runtime: { ...runtime, gone: [...runtime.gone, app.appId] }, windowGoesTo: "taskbar", tooltip: null };
  }
  const resident = runtime.resident.some((r) => r.appId === app.appId) ? runtime.resident : [...runtime.resident, app];
  return {
    runtime: { ...runtime, resident },
    windowGoesTo: "tray",
    tooltip: "最小化到托盘",
  };
}

/** 徽标实时性：活动进度更新（0-100 钳制；完成置 null）。 */
export function updateActivity(runtime: TrayRuntime, appId: string, pct: number | null): TrayRuntime {
  return {
    ...runtime,
    resident: runtime.resident.map((r) =>
      r.appId === appId ? { ...r, activityPct: pct === null ? null : Math.min(100, Math.max(0, Math.round(pct))) } : r,
    ),
  };
}

/** 真退出路径（托盘右键「退出」）：出托盘 + 记 gone（真退出判据）。 */
export function quitFromTray(runtime: TrayRuntime, appId: string): TrayRuntime {
  return {
    resident: runtime.resident.filter((r) => r.appId !== appId),
    gone: runtime.gone.includes(appId) ? runtime.gone : [...runtime.gone, appId],
  };
}

/** 任务栏不显示判据：托盘化应用不得出现在任务栏按钮表。 */
export function taskbarButtons(runtime: TrayRuntime, allRunningAppIds: string[]): string[] {
  const resident = new Set(runtime.resident.map((r) => r.appId));
  return allRunningAppIds.filter((id) => !resident.has(id));
}

/** 托盘溢出收纳兼容（F075 语义）：容量外 Resident 图标进溢出抽屉而非消失。 */
export function overflowCompat(runtime: TrayRuntime, trayCapacity: number): { visible: string[]; overflowed: string[] } {
  const ids = runtime.resident.map((r) => r.appId);
  return { visible: ids.slice(0, Math.max(0, trayCapacity)), overflowed: ids.slice(Math.max(0, trayCapacity)) };
}

/* ---------- 行为声明的持久化（应用偏好记住） ---------- */

const KEY = h4Key("f368", "behavior");

type BehaviorPrefs = Record<string, CloseBehavior>;

function isPrefs(v: unknown): v is BehaviorPrefs {
  return typeof v === "object" && v !== null && !Array.isArray(v);
}

export function setBehavior(appId: string, behavior: CloseBehavior, store: KvStore = defaultStore()): boolean {
  const prefs = readJson<BehaviorPrefs>(store, KEY, {}, isPrefs);
  return writeJson(store, KEY, { ...prefs, [appId]: behavior });
}

export function declaredBehavior(appId: string, store: KvStore = defaultStore()): CloseBehavior {
  const prefs = readJson<BehaviorPrefs>(store, KEY, {}, isPrefs);
  return prefs[appId] ?? "normal";
}
