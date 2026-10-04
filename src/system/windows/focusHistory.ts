/**
 * V-25 焦点历史回溯（化境 · AI-2 窗口与键位路）：
 * `Ctrl+Alt+[` / `Ctrl+Alt+]`（经键位注册表仲裁后生效）在最近聚焦窗口历史中
 * 后退/前进，如同编辑器的「上一个光标位置」。
 * - 历史栈深 20，重复聚焦去重（连续同窗口只记一次）；
 * - 与 Alt+Tab 互不干扰（独立栈，Alt+Tab 走 vwm.cycleVwmFocus）；
 * - 栈底/栈顶时无动作不循环（行为可预期，调用方可据此给轻震动反馈）。
 */

const DEPTH = 20;

let backStack: string[] = [];
let forwardStack: string[] = [];
let current: string | null = null;

/** 重置（测试 / 环境重载用）。 */
export function resetFocusHistory(): void {
  backStack = [];
  forwardStack = [];
  current = null;
}

/** 聚焦变化时记录（去重：与当前相同不记）。重复聚焦去重 + 栈深裁剪。 */
export function recordFocus(id: string | null): void {
  if (id === null || id === current) return;
  if (current !== null) {
    backStack.push(current);
    if (backStack.length > DEPTH) backStack.shift();
    forwardStack = []; // 新聚焦截断前进分支（编辑器语义）
  }
  current = id;
}

export function focusHistoryCounts(): { back: number; forward: number } {
  return { back: backStack.length, forward: forwardStack.length };
}

/** 后退一步：返回目标窗口 id；栈底返回 null（无动作）。 */
export function focusBack(): string | null {
  const prev = backStack.pop();
  if (!prev) return null;
  if (current !== null) forwardStack.push(current);
  current = prev;
  return prev;
}

/** 前进一步：返回目标窗口 id；栈顶返回 null（无动作）。 */
export function focusForward(): string | null {
  const next = forwardStack.pop();
  if (!next) return null;
  if (current !== null) backStack.push(current);
  current = next;
  return next;
}
