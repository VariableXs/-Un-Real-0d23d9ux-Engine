/**
 * 菜单与对话框七件（AI-U1 · F419 任务栏右键 / F420 时钟右键 / F424 Esc
 * 层级栈 / F433 Shift+F10 / F434 对话框键位 / F435 下拉框 / F436 滑杆）。
 *
 * 判据唯一源（主册摘文，与 kernel/varix/src/uni1/ 同参数）：
 * - F419「两形制清单；最近 3 条与 F074 引擎同源；关闭所有确认链；固定项
 *   语义；菜单项数 ≤8 审计」。
 * - F420「两直达落地页；立即同步执行与留痕（F295 事件）；左键行为不变；
 *   菜单项数；键盘可达」。
 * - F424「语义层级表（四层优先级）；逐层剥离用例；无副作用桌面态；响应
 *   <100ms」。
 * - F433「呼出位置与焦点判定；方向键/首字母/Enter/Esc 全链；子菜单键盘
 *   进入与退出；与 F215/F207 语义一致」。
 * - F434「四键行为矩阵（三类控件×四键）；焦点陷阱联动；热键功能性（虽无
 *   下划线仍生效）；默认按钮判定（F207 复用）」。
 * - F435「三招行为矩阵；跳选循环；预览代值与 Esc 恢复；滚动跟随；展开
 *   收起时序 <100ms」。
 * - F436「五招行为；步进值定义表（每滑杆登记最小刻度）；气泡读数；连发
 *   节奏一致性；焦点环（F206）在滑杆上的形态」。
 */

import { u1Store } from "./u1store";

/* ------------------------------- F419 任务栏右键菜单 ------------------------------- */

/** 两形制：未固定应用（最近 3 条）与已固定应用（固定项语义）。 */
export type TaskbarMenuForm = "running-unpinned" | "pinned";

/** 菜单项数 ≤8 审计（判据原文）。 */
export function taskbarMenuItemCountOk(n: number): boolean {
  return n <= 8;
}

/** 最近 3 条（与 F074 引擎同源——只取前 3）。 */
export function recentJumps(items: string[]): string[] {
  return items.slice(0, 3);
}

/* ------------------------------- F420 时钟右键 ------------------------------- */

/** 两直达落地页（判据：两直达——调整日期/时间 + 通知设置）。 */
export const CLOCK_CTX_PAGES = ["datetime", "notify"] as const;

/** 菜单项数（判据：菜单项数——两直达 + 立即同步 = 3）。 */
export const CLOCK_CTX_ITEMS = ["datetime", "notify", "sync-now"] as const;

/* ------------------------------- F424 Esc 层级栈 ------------------------------- */

/** 四层优先级（与内核 escstack 同表：数字越大越浮在上）。 */
export const ESC_TIERS = ["popup", "panel", "flyout", "modal"] as const;
export type EscTier = (typeof ESC_TIERS)[number];

/** 每次按键只剥一层（判据：逐层剥离——一次 Esc 不许全关）。 */
export function escPeelOne(stack: EscTier[]): EscTier | null {
  if (stack.length === 0) return null; // 桌面态无副作用
  return stack[stack.length - 1] ?? null;
}

/** 层级不变量：栈从底到顶 tier 序必须递增（浮在上者更大）。 */
export function escStackInvariant(stack: EscTier[]): boolean {
  const ranks = stack.map((t) => ESC_TIERS.indexOf(t));
  return ranks.every((r, i) => {
    if (i === 0) return true;
    const prev = ranks[i - 1];
    return prev !== undefined && r >= prev;
  });
}

export const ESC_BUDGET_MS = 100;

/* ------------------------------- F433 Shift+F10 键盘右键 ------------------------------- */

/** 呼出位置：焦点元素几何中心（无焦点 → 视口中心）。 */
export function keyboardMenuAnchor(focusRect: { x: number; y: number; w: number; h: number } | null, viewport: { w: number; h: number }): { x: number; y: number } {
  if (!focusRect) return { x: viewport.w / 2, y: viewport.h / 2 };
  return { x: focusRect.x + focusRect.w / 2, y: focusRect.y + focusRect.h / 2 };
}

/** 菜单翻转边界（F215 同源）：右缘/下缘溢出时向内收。 */
export function flipMenu(pos: { x: number; y: number }, size: { w: number; h: number }, screen: { w: number; h: number }): { x: number; y: number } {
  return {
    x: Math.min(pos.x, screen.w - size.w - 8),
    y: Math.min(pos.y, screen.h - size.h - 8),
  };
}

/* ------------------------------- F434 对话框控件键位 ------------------------------- */

/** 默认按钮判定（F207 复用）：有显式 default 用之；否则取首个安全按钮。 */
export function defaultButton(buttons: { label: string; kind: "primary" | "normal" | "danger"; isDefault?: boolean }[]): number {
  const d = buttons.findIndex((b) => b.isDefault);
  if (d >= 0) return d;
  return buttons.findIndex((b) => b.kind === "primary");
}

/** 删除类对话框：Esc=取消（判据：删除类对话框 Esc 行为 10 例全为取消）。 */
export function dialogEscKind(hasDanger: boolean): "cancel" {
  void hasDanger;
  return "cancel";
}

/* ------------------------------- F435 下拉框键盘 ------------------------------- */

export const DROPDOWN_TOGGLE_BUDGET_MS = 100;

/** 跳选循环：从预览位起扫一圈，命中同首字母项（与内核 dropdown 同语义）。 */
export function jumpLetter(options: string[], from: number, ch: string): number | null {
  const n = options.length;
  if (n === 0) return null;
  for (let step = 1; step <= n; step++) {
    const i = (from + step) % n;
    const opt = options[i];
    if (opt && opt.charAt(0).toUpperCase() === ch.toUpperCase()) return i;
  }
  return null;
}

/** 滚动跟随：预览位出视窗 → 顶贴齐（与内核 follow 同式）。 */
export function dropdownFollow(preview: number, scrollTop: number, rows: number): number {
  if (preview < scrollTop) return preview;
  if (preview >= scrollTop + rows) return preview + 1 - rows;
  return scrollTop;
}

/* ------------------------------- F436 滑杆键盘 ------------------------------- */

/** 步进定义表（判据：每滑杆登记最小刻度——与内核 registry 同表）。 */
export const SLIDER_REGISTRY = [
  { name: "音量", min: 0, max: 100, step: 2 },
  { name: "亮度", min: 10, max: 100, step: 5 },  // 亮度下限 10（F239 同源）
  { name: "缩放", min: 100, max: 200, step: 25 },
] as const;

/** 五招统一：方向 ±1 步 / PgUp/PgDn ±10 步 / Home/End 极值。 */
export function sliderStep(spec: { min: number; max: number; step: number }, value: number, steps: number): number {
  return Math.min(spec.max, Math.max(spec.min, value + steps * spec.step));
}

/** 连发节奏（F240 同源）：首牙 400ms、阶梯 60ms/发。 */
export const REPEAT_FIRST_DELAY_MS = 400;
export const REPEAT_STEP_MS = 60;

export function repeatDue(heldMs: number): number {
  if (heldMs < REPEAT_FIRST_DELAY_MS) return 0;
  return Math.floor((heldMs - REPEAT_FIRST_DELAY_MS) / REPEAT_STEP_MS) + 1;
}

/* ------------------------------- 面板读数 ------------------------------- */

/** Esc 预算读数（消费 u1Store——即时生效）。 */
export function escBudgetMs(): number {
  const cfg = u1Store.get<{ budgetMs?: number }>("escStack");
  return cfg.budgetMs ?? ESC_BUDGET_MS;
}
