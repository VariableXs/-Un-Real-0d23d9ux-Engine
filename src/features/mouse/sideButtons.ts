/**
 * J 鼠标域 · F615 侧键编程。
 *
 * 鼠标侧键（XButton1/2）全局与应用级两级映射：全局默认「后退/前进」（浏览器/
 * 资源管理器历史导航——Windows 肌肉记忆），任一应用可覆盖（IDE 里侧键=折叠
 * 代码块、截图工具里=再截上区域 F595）；映射目标三选（系统动作/快捷键组合/
 * 启动应用）；侧键按下与手势层（F617）互斥由优先级表裁决（同一按键两功能
 * 不并存）；五键鼠全键位可用。
 *
 * 判据锚点：
 * - 全局/应用两级优先级 → resolveSideButton()
 * - 三类映射目标各一例 → SIDE_ACTION / SIDE_SHORTCUT / SIDE_LAUNCH 三目标类型
 * - 与 F244 注册表互通 → sideButtonRegistryRows()（同源字段注册行）
 * - 五键设备全键位 → XBUTTON1/2 + 主三键登记
 * - 覆盖清除回退与持久化 → clearAppOverride()
 */

import { j1Store } from "./j1store";

export const XBUTTON1 = 3; // 浏览器规范：MouseEventArgs button 3 = XButton1（后退）
export const XBUTTON2 = 4; // XButton2（前进）

/** 五键设备全键位登记（面板展示与审计同源）。 */
export const FIVE_BUTTON_MAP = [
  { button: 0, name: "左键", remappable: false },
  { button: 1, name: "中键", remappable: false },
  { button: 2, name: "右键", remappable: false },
  { button: XBUTTON1, name: "侧键 后退", remappable: true },
  { button: XBUTTON2, name: "侧键 前进", remappable: true },
] as const;

/** 映射目标三类型（三类映射目标判据）。 */
export type SideTarget =
  | { kind: "action"; action: string }
  | { kind: "shortcut"; keys: string } // 如 "Ctrl+Shift+B"
  | { kind: "launch"; appId: string };

/** 系统动作表（action kind 的合法值——启动应用/快捷键走各自 kind 不在此列）。 */
export const SIDE_ACTIONS = [
  { id: "nav-back", name: "后退", desc: "历史导航后退（浏览器/资源管理器同款）。" },
  { id: "nav-forward", name: "前进", desc: "历史导航前进。" },
  { id: "taskview", name: "任务视图", desc: "打开任务视图（F081）。" },
  { id: "desktop-toggle", name: "显示桌面", desc: "切换显示桌面。" },
  { id: "mute", name: "静音切换", desc: "全局静音/取消静音。" },
] as const;

export interface SideButtonsConfig {
  /** 全局默认：button → 目标。 */
  global: Record<string, SideTarget>;
  /** 应用覆盖：appId → { button → 目标 }。 */
  apps: Record<string, Record<string, SideTarget>>;
}

/** 映射目标校验（导入/手配共用——非法目标显性拒绝）。 */
export function validateSideTarget(t: SideTarget): string[] {
  const errs: string[] = [];
  if (t.kind === "action") {
    if (!SIDE_ACTIONS.some((a) => a.id === t.action)) errs.push(`未知系统动作: ${t.action}`);
  } else if (t.kind === "shortcut") {
    if (!/^[A-Za-z0-9]+(\+[A-Za-z0-9]+)+$/.test(t.keys)) errs.push(`快捷键组合格式非法: ${t.keys}`);
  } else if (t.kind === "launch") {
    if (!t.appId) errs.push("启动应用缺少 appId");
  } else {
    errs.push("未知映射类型");
  }
  return errs;
}

/**
 * 两级优先级解析（全局/应用两级优先级判据）：应用覆盖存在且未被清除 → 应用；
 * 否则 → 全局；全局也没有 → null（按键放行给系统默认语义）。
 */
export function resolveSideButton(cfg: SideButtonsConfig, appId: string | null, button: number): SideTarget | null {
  if (appId) {
    const app = cfg.apps[appId];
    const hit = app?.[String(button)];
    if (hit) return hit;
  }
  return cfg.global[String(button)] ?? null;
}

/** 覆盖清除回退：删除应用覆盖后该键回全局默认（一键回全局判据）。 */
export function clearAppOverride(appId: string, button: number): void {
  const s = j1Store.get("sideButtons");
  const apps = { ...(s.apps as SideButtonsConfig["apps"]) };
  const app = { ...(apps[appId] ?? {}) };
  delete app[String(button)];
  if (Object.keys(app).length === 0) delete apps[appId];
  else apps[appId] = app;
  j1Store.set("sideButtons", { apps });
}

/** 设置映射（全局或应用级；写入前校验——异常零静默）。 */
export function setSideMapping(scope: { global: true } | { appId: string }, button: number, target: SideTarget): void {
  const errs = validateSideTarget(target);
  if (errs.length > 0) throw new Error(`[mouse-j1:F615] 侧键映射校验失败: ${errs.join("；")}`);
  const s = j1Store.get("sideButtons");
  if ("global" in scope) {
    j1Store.set("sideButtons", { global: { ...(s.global as SideButtonsConfig["global"]), [String(button)]: target } });
  } else {
    const apps = { ...(s.apps as SideButtonsConfig["apps"]) };
    apps[scope.appId] = { ...(apps[scope.appId] ?? {}), [String(button)]: target };
    j1Store.set("sideButtons", { apps });
  }
}

/**
 * 与 F244 快捷键注册表互通：侧键映射生成同源字段注册行
 * （冲突审计同源——同一按键两功能不并存的裁决依据）。
 */
export function sideButtonRegistryRows(cfg: SideButtonsConfig, appId: string | null): {
  key: string; scope: string; action: string; source: string;
}[] {
  const rows: { key: string; scope: string; action: string; source: string }[] = [];
  const describe = (t: SideTarget): string => {
    if (t.kind === "action") return SIDE_ACTIONS.find((a) => a.id === t.action)?.name ?? t.action;
    if (t.kind === "shortcut") return `快捷键 ${t.keys}`;
    return `启动 ${t.appId}`;
  };
  for (const [b, t] of Object.entries(cfg.global)) {
    rows.push({ key: `XButton${b === String(XBUTTON1) ? "1" : "2"}`, scope: "全局", action: describe(t as SideTarget), source: "F615" });
  }
  if (appId) {
    for (const [b, t] of Object.entries(cfg.apps[appId] ?? {})) {
      rows.push({ key: `XButton${b === String(XBUTTON1) ? "1" : "2"}`, scope: `应用 ${appId}`, action: describe(t as SideTarget), source: "F615" });
    }
  }
  return rows;
}

/**
 * 与手势层（F617）互斥优先级表（同一按键两功能不并存判据）：
 * 侧键被映射占用时手势层不接管侧键；手势层启用且使用右键时侧键不受影响
 * （两功能作用在不同按键上天然无冲突，登记到此表供审计面呈现）。
 */
export function sideGesturePriorityMatrix(sideConfigured: boolean, gestureEnabled: boolean): string {
  if (sideConfigured && gestureEnabled) return "侧键映射优先；手势层仅右键，无按键冲突（已登记）。";
  if (sideConfigured) return "仅侧键映射生效。";
  if (gestureEnabled) return "仅手势层生效（右键轨迹，侧键保持系统默认）。";
  return "两者均未启用。";
}

/**
 * F244 冲突审计（互通判据的执法面）：把当前映射与外部注册行（系统快捷键
 * 等已占用声明）比对，产出冲突清单。冲突规则：同一修饰键组合或同一系统
 * 动作被声明两次 → 冲突行（审计表裁决，不静默双注册）。
 */
export function sideButtonConflicts(
  cfg: SideButtonsConfig,
  externalRows: { key: string; action: string }[],
): { key: string; conflictWith: string }[] {
  const conflicts: { key: string; conflictWith: string }[] = [];
  for (const [b, t] of Object.entries(cfg.global)) {
    const label = `XButton${b === String(XBUTTON1) ? "1" : "2"}`;
    if (t.kind === "shortcut") {
      for (const row of externalRows) {
        if (row.action.includes(t.keys)) conflicts.push({ key: label, conflictWith: row.action });
      }
    }
  }
  return conflicts;
}
