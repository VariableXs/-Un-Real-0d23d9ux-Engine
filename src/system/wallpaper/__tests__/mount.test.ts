import { describe, expect, it, vi } from "vitest";
import { installFeatureListener, isFeatureEvent, mountOnEvent, OPEN_EVENT } from "../mount";

function featureEvent(feature: string): CustomEvent {
  return new CustomEvent(OPEN_EVENT, { detail: { feature } });
}

describe("N-07 overlay 自挂载器（mountOnEvent 事件契约）", () => {
  it("isFeatureEvent：detail.feature 精确匹配；非 CustomEvent 拒绝", () => {
    expect(isFeatureEvent(featureEvent("lockscreen"), "lockscreen")).toBe(true);
    expect(isFeatureEvent(featureEvent("wallpaper-studio"), "lockscreen")).toBe(false);
    expect(isFeatureEvent(new Event("x"), "lockscreen")).toBe(false);
    expect(isFeatureEvent(new CustomEvent(OPEN_EVENT, { detail: {} }), "lockscreen")).toBe(false);
  });

  it("installFeatureListener：命中触发 / 不命中忽略 / 反注册后不再触发", () => {
    const target = new EventTarget();
    const onOpen = vi.fn();
    const dispose = installFeatureListener(target, "lockscreen", onOpen);
    target.dispatchEvent(featureEvent("wallpaper-studio"));
    expect(onOpen).not.toHaveBeenCalled();
    target.dispatchEvent(featureEvent("lockscreen"));
    expect(onOpen).toHaveBeenCalledTimes(1);
    dispose();
    target.dispatchEvent(featureEvent("lockscreen"));
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("mountOnEvent：命中时加载模块；node 无 DOM 环境下安全 no-op 不抛错", async () => {
    const target = new EventTarget();
    const loader = vi.fn(async () => ({ Overlay: () => null }));
    const dispose = mountOnEvent(target, "scene-settings", loader);
    expect(() => target.dispatchEvent(featureEvent("scene-settings"))).not.toThrow();
    await Promise.resolve();
    await Promise.resolve();
    expect(loader).toHaveBeenCalledTimes(1);
    dispose();
    target.dispatchEvent(featureEvent("scene-settings"));
    expect(loader).toHaveBeenCalledTimes(1);
  });
});