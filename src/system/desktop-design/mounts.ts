/**
 * AURORA-10000 · AI-11~AI-15 车道 · 自挂载装配（activate.ts 引入一次即全部生效）。
 * feature 注册表：
 * - design-center ：设计中心面板（25 族全量开关/参数档）
 * - design-theater：剧场模式全屏 overlay（族0074）
 * - design-ritual ：仪式卡 overlay（族0075，RitualRunner 派发）
 */
import { mountOnEvent, installCloseHandler } from "../wallpaper/mount";

export function installDesignOverlays(target: EventTarget = window): void {
  void mountOnEvent(target, "design-center", async () => ({ Overlay: (await import("./DesignCenter")).DesignCenter }));
  void installCloseHandler(target, "design-center");
  void mountOnEvent(target, "design-theater", async () => ({ Overlay: (await import("./TheaterOverlay")).TheaterOverlay }));
  void installCloseHandler(target, "design-theater");
  void mountOnEvent(target, "design-ritual", async () => ({ Overlay: (await import("./RitualOverlay")).RitualOverlay }));
  void installCloseHandler(target, "design-ritual");
}
