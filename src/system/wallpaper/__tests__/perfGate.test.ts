import { describe, expect, it } from "vitest";
import { isDegradeActive, isTypingRecent, TYPING_WINDOW_MS } from "../perfGate";

const OFF = { typing: false, fullscreen: false, battery: false };

describe("N-07 perfGate 性能联动决策", () => {
  it("无信号 → 全引擎正常（shader 60fps，不暂停）", () => {
    const d = isDegradeActive(OFF);
    expect(d.degraded).toBe(false);
    expect(d.shaderFps).toBe(60);
    expect(d.videoPaused).toBe(false);
    expect(d.generativePaused).toBe(false);
    expect(d.particlesStatic).toBe(false);
    expect(d.reasons).toEqual([]);
  });

  it("全屏 → shader 降帧 10fps、粒子静止（L817）", () => {
    const d = isDegradeActive({ ...OFF, fullscreen: true });
    expect(d.shaderFps).toBe(10);
    expect(d.particlesStatic).toBe(true);
    expect(d.videoPaused).toBe(false);
    expect(d.reasons).toEqual(["fullscreen"]);
  });

  it("电池 → 视频暂停（L817 电池红线），shader 帧率不动", () => {
    const d = isDegradeActive({ ...OFF, battery: true });
    expect(d.videoPaused).toBe(true);
    expect(d.shaderFps).toBe(60);
    expect(d.reasons).toEqual(["battery"]);
  });

  it("打字 → 全引擎降级（扩展到全部引擎）", () => {
    const d = isDegradeActive({ ...OFF, typing: true });
    expect(d.degraded).toBe(true);
    expect(d.videoPaused).toBe(true);
    expect(d.generativePaused).toBe(true);
    expect(d.particlesStatic).toBe(true);
    expect(d.shaderFps).toBe(12);
  });

  it("多信号叠加原因齐全", () => {
    const d = isDegradeActive({ typing: true, fullscreen: true, battery: true });
    expect(d.reasons).toEqual(["typing", "fullscreen", "battery"]);
    expect(d.shaderFps).toBe(10); // 全屏优先级最高
  });

  it("isTypingRecent：最近 1s 窗口判定（0 = 从未按键）", () => {
    expect(isTypingRecent(1000, 1000 + TYPING_WINDOW_MS - 1)).toBe(true);
    expect(isTypingRecent(1000, 1000 + TYPING_WINDOW_MS)).toBe(false);
    expect(isTypingRecent(0, 5000)).toBe(false);
  });
});