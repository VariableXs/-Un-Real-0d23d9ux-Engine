/**
 * V-27 嵌入窗口焦点联动（化境 · AI-2 窗口与键位路）：
 * 点击嵌入（L1）窗口内部时，宿主 VWM 框标题栏同步进入激活态视觉
 * （与直接聚焦宿主一致），明确「我在操作谁」。
 * - 仅视觉联动：绝不调用 pointerFocusVwm / embed_focus，不改系统焦点语义
 *   （不抢真实焦点，IME/键盘行为零变化——化境纪律）；
 * - 真实 VWM 焦点变化时视觉态让位（避免出现两条「激活」标题栏）；
 * - 联动可关（默认开，localStorage 持久化）；
 * - 不做宿主标题栏显示嵌入进程名（L4 hint 徽标已有该职责）。
 */

import { createStore, useStore } from "../../lib/store";

const KEY = "variable:vwm:embedfocuslink";

/** 联动总开关（默认开）。 */
export function embedFocusLinkEnabled(): boolean {
  try {
    return localStorage.getItem(KEY) !== "0";
  } catch {
    return true;
  }
}

export function setEmbedFocusLinkEnabled(v: boolean): void {
  try {
    localStorage.setItem(KEY, v ? "1" : "0");
  } catch {
    /* storage blocked */
  }
}

export const embedFocusLinkStore = createStore<{ visualActiveId: string | null }>({
  visualActiveId: null,
});

/** 观测到嵌入窗口内部交互（点击/OS 窗口重新聚焦）→ 宿主标题栏进入激活态视觉。 */
export function noteEmbedInteraction(winId: string): void {
  if (!embedFocusLinkEnabled()) return;
  embedFocusLinkStore.setState({ visualActiveId: winId });
}

/** 真实 VWM 焦点变化 → 视觉态让位（真实焦点标题栏本就走激活令牌）。 */
export function noteVwmFocusChange(focusedId: string | null): void {
  const cur = embedFocusLinkStore.getState().visualActiveId;
  if (cur === null) return;
  if (focusedId !== cur) embedFocusLinkStore.setState({ visualActiveId: null });
}

/** 查询某宿主窗口是否处于「嵌入视觉激活」（真实聚焦时不适用——真实态优先）。 */
export function embedVisualActive(winId: string): boolean {
  return embedFocusLinkStore.getState().visualActiveId === winId;
}

export function useEmbedVisualActive(winId: string): boolean {
  return useStore(embedFocusLinkStore, (s) => s.visualActiveId === winId);
}

/** 重置（测试 / 环境重载用）。 */
export function resetEmbedFocusLink(): void {
  embedFocusLinkStore.setState({ visualActiveId: null });
}
