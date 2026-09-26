/**
 * 系统快捷键六件（AI-U1 · F402 任务管理器 / F403 Win+L / F405 Alt+F4 /
 * F407 Win+I / F408 Win+X / F416 Win 键开始菜单）——前端生效面。
 *
 * 判据唯一源（主册摘文，与 kernel/varix/src/uni1/ v1 语义核同参数）：
 * - F402「三入口快捷键注册（F244）；刷新 1s±0.1s；排序/结束任务用例；
 *   三处数据对账（同进程同读数误差 <3%）」。
 * - F403「锁定 <300ms 实测；媒体暂停续播；窗口状态保持；锁屏摘要只
 *   计数判据；与 F316 闲置锁屏同一入口殊途同归」。
 * - F405「焦点判定（窗/桌面/模态三场景）；桌面电源菜单三选项与默认值；
 *   三问联动；关闭语义与 × 一致性」。
 * - F407「三场景（未开/已开/已开在搜索）行为；单例聚焦；搜索框焦点与
 *   光标就绪（直接可打字）；注册表登记」。
 * - F408「九项清单与落地页对照表；首字母快捷；子菜单二级；菜单键位
 *   注册；各项打开时长 <1s」。
 * - F416「开合时序（<150ms）；焦点落搜索框；打字冲突零丢失；Esc/外点
 *   关闭；连按稳定性」。
 */

import { u1Store } from "./u1store";

/* ------------------------------- F402 任务管理器 ------------------------------- */

/** 任务管理器三入口（判据：三入口快捷键注册——F244 同表）。 */
export const TASKMGR_ENTRIES = ["Ctrl+Shift+Esc", "Ctrl+Alt+Del 菜单", "任务栏右键"] as const;

/** 刷新节奏：1s±0.1s 判据（刷新间隔在 900–1100ms 内视为达标）。 */
export function refreshWithinBudget(ms: number): boolean {
  return ms >= 900 && ms <= 1100;
}

/** 三处数据对账：同进程读数误差 <3% 判据（相对基准值）。 */
export function reconcileReadings(base: number, ...others: number[]): boolean {
  return others.every((v) => Math.abs(v - base) / Math.max(base, 1) < 0.03);
}

/* ------------------------------- F403 Win+L 锁屏 ------------------------------- */

/** 锁定预算（ms）——判据 <300ms；超线诚实记账不静默。 */
export const LOCK_BUDGET_MS = 300;

export function lockLatencyVerdict(ms: number): "ok" | "over" {
  return ms <= LOCK_BUDGET_MS ? "ok" : "over";
}

/** 媒体暂停续播：锁定时暂停、解锁恢复（判据原文）。 */
export function mediaPauseResume(locked: boolean, playing: boolean): "keep" | "pause" | "resume" {
  if (locked && playing) return "pause";
  if (!locked && !playing) return "resume";
  return "keep";
}

/* ------------------------------- F405 Alt+F4 与关机菜单 ------------------------------- */

export type FocusScene = "window" | "desktop" | "modal";

/** Alt+F4 焦点判定：模态优先关模态、窗口关窗口、桌面呼出电源菜单。 */
export function altF4Resolve(scene: FocusScene, dirty: boolean, confirmEnabled: boolean): "close-window" | "power-menu" | "triple-ask" | "noop" {
  if (scene === "modal") return dirty && confirmEnabled ? "triple-ask" : "close-window";
  if (scene === "window") return dirty && confirmEnabled ? "triple-ask" : "close-window";
  return "power-menu";
}

/** 桌面电源菜单三选项与默认值（判据：三选项与默认值——默认关机）。 */
export const POWER_MENU = [
  { id: "shutdown", label: "关机", isDefault: true },
  { id: "restart", label: "重启", isDefault: false },
  { id: "sleep", label: "睡眠", isDefault: false },
] as const;

/* ------------------------------- F407 Win+I 设置 ------------------------------- */

export type SettingsScene = "closed" | "open" | "open-on-search";

/** 三场景行为：未开→开；已开→单例聚焦；已开在搜索→聚焦搜索框不重置。 */
export function winIResolve(scene: SettingsScene): "open-focus-page" | "focus-window" | "focus-search-keep" {
  if (scene === "closed") return "open-focus-page";
  if (scene === "open") return "focus-window";
  return "focus-search-keep";
}

/* ------------------------------- F408 Win+X 快捷菜单 ------------------------------- */

/** 九项清单与落地页对照表（判据：九项——与内核 winxmenu 同表）。 */
export const WINX_ITEMS = [
  { letter: "t", label: "任务管理器", page: "taskmgr" },
  { letter: "s", label: "设置", page: "settings" },
  { letter: "e", label: "资源管理器", page: "explorer" },
  { letter: "d", label: "桌面", page: "desktop" },
  { letter: "n", label: "网络", page: "network" },
  { letter: "p", label: "电源", page: "power" },
  { letter: "v", label: "显示", page: "display" },
  { letter: "b", label: "蓝牙", page: "bluetooth" },
  { letter: "x", label: "终端", page: "terminal" },
] as const;

/** 首字母快捷：命中唯一项直达；未命中不动作（内核 f408-letter 判据同源）。 */
export function winXLetterJump(key: string): (typeof WINX_ITEMS)[number] | null {
  const hits = WINX_ITEMS.filter((i) => i.letter === key.toLowerCase());
  const hit = hits[0];
  return hits.length === 1 && hit ? hit : null;
}

/** 各项打开时长 <1s 预算。 */
export const WINX_OPEN_BUDGET_MS = 1000;

/* ------------------------------- F416 Win 键开始菜单 ------------------------------- */

export const START_OPEN_BUDGET_MS = 150;

export type StartScene = { open: boolean; pressedAtMs: number | null };

/** 连按稳定性：第二次按压 200ms 内 = 关闭而非重开（去抖）。 */
export function winKeyToggle(state: StartScene, nowMs: number): StartScene {
  if (state.open) return { open: false, pressedAtMs: nowMs };
  if (state.pressedAtMs !== null && nowMs - state.pressedAtMs < 200) return state;
  return { open: true, pressedAtMs: nowMs };
}

/** 打字零丢失：菜单打开期间键入直通搜索框（IME 组合期不拦截——B-904）。 */
export function typingDuringOpen(imeComposing: boolean): boolean {
  return !imeComposing; // 组合期让路 IME（吞键=0 判据的姊妹条款）
}

/* ------------------------------- 面板读数 ------------------------------- */

/** 三入口当前注册态（消费 u1Store——即时生效）。 */
export function taskmgrEntryEnabled(): string {
  const cfg = u1Store.get<{ hotkey?: string }>("taskmgrHot");
  return cfg.hotkey ?? "Ctrl+Shift+Esc";
}
