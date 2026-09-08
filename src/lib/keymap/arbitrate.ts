/**
 * Z-08/Z-10 — 作用域栈裁决（arbitrate）。
 *
 * 规则：
 * - 同一 combo 可能被多个 scope 登记；输入焦点在可编辑元素内时
 *   global 层自动降级（让位给文本输入），window/context 层照常；
 * - context 栈后进先出：最深的上下文优先消费；
 * - 无消费者时返回 null，事件继续冒泡（让位协议由 Z-09 处理）。
 */

import type { KeyBinding, KeyScope } from "./types";
import { snapshot } from "./registry";

/** 可编辑元素：输入框、文本域、contentEditable（global 键位在这些元素内让位）。 */
export function isEditableTarget(target: EventTarget | null): boolean {
  if (typeof HTMLElement === "undefined") return false; // 非 DOM 环境（单测/worker）不降级
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  return (
    tag === "INPUT" ||
    tag === "TEXTAREA" ||
    tag === "SELECT" ||
    target.isContentEditable
  );
}

/** 上下文栈：后进先出。每项是一个 context scope 的 id（如 "panel:clipboard"）。 */
const contextStack: string[] = [];

export function pushContext(id: string): void {
  if (!contextStack.includes(id)) contextStack.push(id);
}

export function popContext(id: string): void {
  const i = contextStack.lastIndexOf(id);
  if (i >= 0) contextStack.splice(i, 1);
}

export function contextTop(): string | null {
  return contextStack.length > 0 ? (contextStack[contextStack.length - 1] ?? null) : null;
}

export interface ArbitrationInput {
  combo: string;
  /** 事件目标（用于可编辑元素降级）。 */
  target: EventTarget | null;
  /** 当前窗口级 scope id（null = 桌面层）。 */
  windowScope?: string | null;
}

export interface ArbitrationResult {
  binding: KeyBinding | null;
  /** 让位原因（调试/遥测用）。 */
  yielded?: "editable" | "no-binding" | "context-mismatch";
}

/** 裁决一个按键事件归属。返回 null 表示无消费者。 */
export function arbitrate(input: ArbitrationInput): ArbitrationResult {
  const all = snapshot().filter((b) => b.combo === input.combo);
  if (all.length === 0) return { binding: null, yielded: "no-binding" };

  const byScope = (s: KeyScope): KeyBinding[] =>
    all.filter((b) => b.scope === s).sort((a, b) => b.priority - a.priority);

  // 1) context 栈顶优先
  const top = contextTop();
  if (top) {
    const ctx = byScope("context").find((b) => b.id === top || b.source === top);
    if (ctx) return { binding: ctx };
  }
  // 2) window 层
  const win = byScope("window");
  if (win.length > 0) {
    if (!input.windowScope || win.some((b) => b.source === input.windowScope) || input.windowScope === null) {
      const first = win[0];
      if (first) return { binding: first };
    }
  }
  // 3) global 层：可编辑元素内自动降级（Z-10 联动）
  if (isEditableTarget(input.target)) {
    return { binding: null, yielded: "editable" };
  }
  const glob = byScope("global");
  const g0 = glob[0];
  return g0 ? { binding: g0 } : { binding: null, yielded: "no-binding" };
}
