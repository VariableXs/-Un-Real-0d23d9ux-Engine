/**
 * 车道 E 事件总线自挂载入口：
 * 由 src/entries/desktop/main.tsx 以一行 import 引入（车道领地外唯一改动，
 * 原因：事件监听必须随环境启动注册，而车道禁改 DesktopShell）。
 * 职责：①注册 ai04:open-feature 三个 feature 的懒加载挂载；
 *      ②初始化 N-08 已应用主题的 data-theme 信号同步。
 */
import { mountFeatureOnEvent } from "./mount";
import { initAppliedThemeSync } from "../theme-studio/applied";

let registered = false;

export function registerLaneE(): void {
  if (registered || typeof window === "undefined") return;
  registered = true;
  initAppliedThemeSync();
  mountFeatureOnEvent("theme-studio", () => import("../theme-studio/Studio"));
  mountFeatureOnEvent("widget-board", () => import("./Board"));
  mountFeatureOnEvent("iconpacks", () => import("../iconpacks/Studio"));
}

registerLaneE();