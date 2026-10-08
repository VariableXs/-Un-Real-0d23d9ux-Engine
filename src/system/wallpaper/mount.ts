/**
 * AI04 车道 W/E 通用：overlay 自挂载器（不改 DesktopShell）。
 *
 * 模式（计划 §车道 W）：SPA 的 React 树只有 DesktopShell 一个挂载点，
 * 组件树外无法常驻 —— 故采用「事件触发时动态 import + createPortal」：
 *   mountOnEvent(window, "wallpaper-studio", () => import("./Workshop"));
 * 各 overlay 模块在模块加载即调用 mountOnEvent（见各文件顶部）；
 * 主控集成阶段只需 `void import("<模块>")` 一次即可激活监听。
 *
 * 事件契约：
 * - window CustomEvent "ai04:open-feature"  { detail: { feature } } → 打开 overlay；
 * - window CustomEvent "ai04:close-feature" { detail: { feature } } → 关闭（overlay 自身/Esc 派发）。
 */

import { createElement } from "react";
import { createRoot, type Root } from "react-dom/client";

export interface OverlayModule {
  /** 每个可挂载 overlay 模块都导出名为 Overlay 的组件。 */
  Overlay: React.ComponentType;
}

export const OPEN_EVENT = "ai04:open-feature";
export const CLOSE_EVENT = "ai04:close-feature";

interface LiveEntry {
  root: Root;
  host: HTMLElement;
}

const live = new Map<string, LiveEntry>();

/** 纯判断：CustomEvent 的 detail.feature 是否命中目标 feature。 */
export function isFeatureEvent(e: Event, feature: string): boolean {
  if (!(e instanceof CustomEvent)) return false;
  const detail = e.detail as { feature?: unknown } | undefined;
  return typeof detail?.feature === "string" && detail.feature === feature;
}

/** 注册 feature 监听（可测试的最小单元；返回反注册函数）。 */
export function installFeatureListener(
  target: EventTarget,
  feature: string,
  onOpen: () => void,
): () => void {
  const handler = (e: Event): void => {
    if (isFeatureEvent(e, feature)) onOpen();
  };
  target.addEventListener(OPEN_EVENT, handler);
  return () => target.removeEventListener(OPEN_EVENT, handler);
}

/**
 * 监听 open 事件并动态加载 overlay 模块，以 body 直挂 portal 渲染。
 * - 幂等：同一 feature 重复 open 不叠加；
 * - 监听 close 事件卸载；
 * - 非 DOM 环境（vitest node）下 onOpen 不触碰 document，安全 no-op。
 */
export function mountOnEvent(
  target: EventTarget,
  feature: string,
  loader: () => Promise<OverlayModule>,
): () => void {
  return installFeatureListener(target, feature, () => {
    void loader().then((mod) => openOverlay(feature, mod.Overlay)).catch(() => {
      /* 动态加载失败：诚实静默（入口可重试），不阻塞桌面 */
    });
  });
}

/** 打开（或复用）一个 overlay portal。 */
export function openOverlay(feature: string, Overlay: React.ComponentType): void {
  if (typeof document === "undefined" || live.has(feature)) return;
  const host = document.createElement("div");
  host.dataset.ai04Overlay = feature;
  document.body.appendChild(host);
  const root = createRoot(host);
  root.render(createElement(Overlay));
  live.set(feature, { root, host });
}

/** 关闭并卸载 overlay（供 close 事件与 overlay 内部关闭按钮使用）。 */
export function closeOverlay(feature: string): void {
  const entry = live.get(feature);
  if (!entry) return;
  live.delete(feature);
  entry.root.unmount();
  entry.host.remove();
}

/** overlay 组件内部关闭用：派发 close 事件（mountOnEvent 统一接住卸载）。 */
export function dispatchClose(feature: string): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent(CLOSE_EVENT, { detail: { feature } }));
}

/**
 * 安装全局 close 监听（每个 overlay 模块自挂载时调用一次；
 * 与 mountOnEvent 成对，保证 close 事件在模块加载后始终有人接）。
 */
export function installCloseHandler(target: EventTarget, feature: string): () => void {
  const handler = (e: Event): void => {
    if (isFeatureEvent(e, feature)) closeOverlay(feature);
  };
  target.addEventListener(CLOSE_EVENT, handler);
  return () => target.removeEventListener(CLOSE_EVENT, handler);
}