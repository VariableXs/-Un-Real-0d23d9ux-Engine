/**
 * AURORA-10000 · 仪式触发总线（族0075）。
 * open-feature 动态 import 存在异步间隙，entryId 先落总线、overlay 挂载即取。
 */
let pendingRitual: string | null = null;

export function fireRitual(entryId: string): void {
  pendingRitual = entryId;
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent("ai04:open-feature", { detail: { feature: "design-ritual" } }));
  }
}

export function takePendingRitual(): string | null {
  const v = pendingRitual;
  pendingRitual = null;
  return v;
}
