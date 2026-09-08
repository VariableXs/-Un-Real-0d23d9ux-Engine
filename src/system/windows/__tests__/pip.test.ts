import { beforeEach, describe, expect, it } from "vitest";
import {
  PIP_OPACITY_LEVELS,
  isPip,
  loadPipMemory,
  magnetSnap,
  nextOpacity,
  pipEnter,
  pipExit,
  pipExitAll,
  pipList,
  pipMove,
  savePipMemory,
} from "../pip";
import type { VwmRect } from "../vwm";

const WA: VwmRect = { x: 0, y: 0, w: 1920, h: 1080 };
const rect: VwmRect = { x: 100, y: 100, w: 800, h: 600 };

beforeEach(() => {
  localStorage.clear();
  pipExitAll();
});

describe("N-04 pipEnter", () => {
  it("L4 窗口禁入 PiP（拒绝并给出原因，验收边界）", () => {
    const r = pipEnter({ winId: "w1", app: "game", tier: 4, rect }, WA);
    expect(r.ok).toBe(false);
    expect(r.reason).toContain("L4");
    expect(pipList()).toHaveLength(0);
  });

  it("进入 PiP：默认小窗贴右下角，记录还原几何", () => {
    const r = pipEnter({ winId: "w1", app: "write", tier: 1, rect }, WA);
    expect(r.ok).toBe(true);
    expect(r.state?.rect.w).toBe(420);
    expect(r.state?.rect.x).toBeGreaterThan(1200);
    expect(r.state?.restore).toEqual(rect);
    expect(isPip("w1")).toBe(true);
  });

  it("重复进入幂等（返回现状，不产生第二个 PiP）", () => {
    pipEnter({ winId: "w1", app: "write", tier: 1, rect }, WA);
    const again = pipEnter({ winId: "w1", app: "write", tier: 1, rect: { ...rect, x: 500 } }, WA);
    expect(again.state?.restore.x).toBe(100); // 保留第一次的还原几何
    expect(pipList()).toHaveLength(1);
  });

  it("尺寸记忆：同一应用第二次进入用上次尺寸", () => {
    savePipMemory("write", { w: 500, h: 300, opacity: 50 });
    expect(loadPipMemory("write")).toEqual({ w: 500, h: 300, opacity: 50 });
    const r = pipEnter({ winId: "w2", app: "write", tier: 1, rect }, WA);
    expect(r.state?.rect).toMatchObject({ w: 500, h: 300 });
    expect(r.state?.opacity).toBe(50);
  });
});

describe("N-04 磁吸与透明度", () => {
  it("距边 ≤8px 吸附贴边；超出不吸附", () => {
    expect(magnetSnap(3, 500, 420, 264, WA)).toEqual({ x: 0, y: 500 });
    expect(magnetSync_safe(500, 820, WA)).toEqual({ x: 500, y: 1080 - 264 });
    expect(magnetSnap(500, 500, 420, 264, WA)).toEqual({ x: 500, y: 500 });
  });

  it("透明度四档循环 85→70→50→30→85", () => {
    expect(PIP_OPACITY_LEVELS).toEqual([85, 70, 50, 30]);
    expect(nextOpacity(85)).toBe(70);
    expect(nextOpacity(30)).toBe(85);
  });
});

// 辅助：把 y 放到贴近下缘的位置
function magnetSync_safe(x: number, y: number, wa: VwmRect): { x: number; y: number } {
  return magnetSnap(x, y, 420, 264, wa);
}

describe("N-04 退出与回收", () => {
  it("退出回原位（回原位 + 不卡中间态）", () => {
    pipEnter({ winId: "w1", app: "write", tier: 1, rect }, WA);
    const restore = pipExit("w1");
    expect(restore).toEqual(rect);
    expect(isPip("w1")).toBe(false);
    expect(pipExit("w1")).toBeNull();
  });

  it("一键回收全部（托盘聚合动作）", () => {
    pipEnter({ winId: "w1", app: "write", tier: 1, rect }, WA);
    pipEnter({ winId: "w2", app: "code", tier: 2, rect: { ...rect, x: 200 } }, WA);
    const all = pipExitAll();
    expect(Object.keys(all).sort()).toEqual(["w1", "w2"]);
    expect(pipList()).toHaveLength(0);
  });

  it("拖动磁吸并回写尺寸记忆", () => {
    pipEnter({ winId: "vwm-write-abc1", app: "write", tier: 1, rect }, WA);
    const st = pipMove("vwm-write-abc1", 2, 300, WA);
    expect(st?.rect.x).toBe(0); // 磁吸到左缘
    expect(loadPipMemory("write-abc1") ?? loadPipMemory("write")).toBeTruthy();
  });
});

