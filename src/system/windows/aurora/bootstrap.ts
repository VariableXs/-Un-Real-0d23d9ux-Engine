import { applyAllSelections, effectiveSelections, loadSelections, saveSelections } from "./engine";

/**
 * AURORA-10000 · 领域02 窗口与空间自举（AI-06~AI-10 · W1）。
 * 由 src/entries/desktop/main.tsx 调用一次：加载 25 族选择（缺失回退族内首项），
 * 把参数档以 CSS 变量写入 documentElement（--aurora-ws-* 令牌），供窗口/
 * 标题栏/边缘/光影/材质/微手感各渲染层消费。无 UI、无键盘接管、无遥测。
 */
export function bootstrapWindowSpace(): void {
  if (typeof document === "undefined") return; // 非 DOM 环境（单测/SSR）：跳过
  const saved = loadSelections();
  const sel = effectiveSelections(saved);
  // 缺省项补写，保证下次启动读到的表完整
  saveSelections(sel);
  applyAllSelections(sel, document.documentElement);
}
