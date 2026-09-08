import type { ComponentType } from "react";
import { MiniCalculator } from "./MiniCalculator";
import { MiniCountdown } from "./MiniCountdown";
import { MiniNotes } from "./MiniNotes";
import { MiniPomodoro } from "./MiniPomodoro";
import { MiniWorldClock } from "./MiniWorldClock";

/**
 * U-18 迷你应用框架：注册协议。
 * - MiniAppDef：id + 标题 i18n key + 窗口宽（280–360 胶囊尺寸约束）
 * - registerMiniApp：开放注册入口（供后续插件生态；本计划仅内置五件）
 *   拒绝条件：width 越界 / id 重复 / render 缺失 —— 返回 false，不抛错
 * - MINI_APPS：注册表快照数组（引用稳定，随注册增长）
 * - getMiniApp(id)：查询（未注册返回 null）
 */

export interface MiniAppDef {
  id: string;
  /** 窗口标题 i18n key（dictionaries 缺 key 回退 key 本身，先上线后补文案）。 */
  titleKey: string;
  /** 窗口宽（像素，280–360）。 */
  width: number;
}

export interface MiniApp extends MiniAppDef {
  /** 渲染组件（无 props——迷你应用自包含，数据自取）。 */
  render: ComponentType;
}

/** 窗口宽度约束（含边界）。 */
export const MINI_WIDTH_MIN = 280;
export const MINI_WIDTH_MAX = 360;

const byId = new Map<string, MiniApp>();

/** 注册表快照（内置五件 + 后续插件注册项；数组引用稳定，只 push 不重建）。 */
export const MINI_APPS: MiniApp[] = [];

/** 开放注册：非法 def / 越界 width / 重复 id → false（静默拒绝）。 */
export function registerMiniApp(def: MiniAppDef, render: ComponentType): boolean {
  if (!def || !def.id || typeof def.width !== "number") return false;
  if (def.width < MINI_WIDTH_MIN || def.width > MINI_WIDTH_MAX) return false;
  if (typeof render !== "function") return false;
  if (byId.has(def.id)) return false;
  const app: MiniApp = { id: def.id, titleKey: def.titleKey, width: def.width, render };
  byId.set(def.id, app);
  MINI_APPS.push(app);
  return true;
}

/** 按 id 查询（未注册 → null）。 */
export function getMiniApp(id: string): MiniApp | null {
  return byId.get(id) ?? null;
}

// ---------- 内置五件 ----------

registerMiniApp({ id: "mini-worldclock", titleKey: "miniWorldClockTitle", width: 340 }, MiniWorldClock);
registerMiniApp({ id: "mini-pomodoro", titleKey: "miniPomodoroTitle", width: 300 }, MiniPomodoro);
registerMiniApp({ id: "mini-calculator", titleKey: "miniCalculatorTitle", width: 300 }, MiniCalculator);
registerMiniApp({ id: "mini-notes", titleKey: "miniNotesTitle", width: 320 }, MiniNotes);
registerMiniApp({ id: "mini-countdown", titleKey: "miniCountdownTitle", width: 300 }, MiniCountdown);
