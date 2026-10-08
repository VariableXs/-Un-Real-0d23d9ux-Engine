import { describe, expect, it } from "vitest";
import type { ComponentType } from "react";
import {
  getMiniApp,
  MINI_APPS,
  MINI_WIDTH_MAX,
  MINI_WIDTH_MIN,
  registerMiniApp,
} from "../registry";
import { MiniCalculator } from "../MiniCalculator";
import { MiniCountdown } from "../MiniCountdown";
import { MiniNotes } from "../MiniNotes";
import { MiniPomodoro } from "../MiniPomodoro";
import { MiniWorldClock } from "../MiniWorldClock";

const BUILT_IN = ["mini-worldclock", "mini-pomodoro", "mini-calculator", "mini-notes", "mini-countdown"];

describe("U-18 迷你应用注册表", () => {
  it("内置五件注册齐全（首个断言：此前的注册表未被本文件污染）", () => {
    expect(MINI_APPS.length).toBe(5);
    expect(MINI_APPS.map((a) => a.id).sort()).toEqual([...BUILT_IN].sort());
    for (const id of BUILT_IN) {
      const app = getMiniApp(id);
      expect(app).not.toBeNull();
      expect(app?.titleKey).toBeTypeOf("string");
      expect(app?.render).toBeTypeOf("function");
    }
    // 内置渲染器与各组件一一对应
    expect(getMiniApp("mini-worldclock")?.render).toBe(MiniWorldClock);
    expect(getMiniApp("mini-pomodoro")?.render).toBe(MiniPomodoro);
    expect(getMiniApp("mini-calculator")?.render).toBe(MiniCalculator);
    expect(getMiniApp("mini-notes")?.render).toBe(MiniNotes);
    expect(getMiniApp("mini-countdown")?.render).toBe(MiniCountdown);
  });

  it("内置 width 均在 280–360 范围内", () => {
    for (const app of MINI_APPS) {
      expect(app.width).toBeGreaterThanOrEqual(MINI_WIDTH_MIN);
      expect(app.width).toBeLessThanOrEqual(MINI_WIDTH_MAX);
    }
    expect(MINI_WIDTH_MIN).toBe(280);
    expect(MINI_WIDTH_MAX).toBe(360);
  });

  it("width 范围约束：越界拒绝（279 / 361），边界接受（280 / 360）", () => {
    const noop: ComponentType = (): null => null;
    expect(registerMiniApp({ id: "t-279", titleKey: "k", width: MINI_WIDTH_MIN - 1 }, noop)).toBe(false);
    expect(registerMiniApp({ id: "t-361", titleKey: "k", width: MINI_WIDTH_MAX + 1 }, noop)).toBe(false);
    expect(getMiniApp("t-279")).toBeNull();
    expect(getMiniApp("t-361")).toBeNull();

    expect(registerMiniApp({ id: "t-280", titleKey: "k", width: 280 }, noop)).toBe(true);
    expect(registerMiniApp({ id: "t-360", titleKey: "k", width: 360 }, noop)).toBe(true);
    expect(getMiniApp("t-280")?.width).toBe(280);
    expect(getMiniApp("t-360")?.width).toBe(360);
  });

  it("重复 id 拒绝：内置 id 与已注册新 id 均不可覆盖", () => {
    const noop: ComponentType = (): null => null;
    const before = getMiniApp("mini-notes");
    expect(registerMiniApp({ id: "mini-notes", titleKey: "hijack", width: 300 }, noop)).toBe(false);
    expect(getMiniApp("mini-notes")).toBe(before); // 原注册项未被替换
    expect(getMiniApp("mini-notes")?.titleKey).toBe("miniNotesTitle");

    expect(registerMiniApp({ id: "t-dup", titleKey: "k", width: 300 }, noop)).toBe(true);
    expect(registerMiniApp({ id: "t-dup", titleKey: "k2", width: 320 }, noop)).toBe(false);
    expect(getMiniApp("t-dup")?.titleKey).toBe("k");
  });

  it("render 缺失拒绝", () => {
    expect(registerMiniApp({ id: "t-norender", titleKey: "k", width: 300 }, undefined as unknown as ComponentType)).toBe(
      false,
    );
    expect(getMiniApp("t-norender")).toBeNull();
  });

  it("getMiniApp 查询：未知 id → null；注册表快照随注册增长", () => {
    expect(getMiniApp("mini-not-exist")).toBeNull();
    const len = MINI_APPS.length;
    const noop: ComponentType = (): null => null;
    expect(registerMiniApp({ id: "t-query", titleKey: "q", width: 300 }, noop)).toBe(true);
    expect(MINI_APPS.length).toBe(len + 1);
    expect(MINI_APPS.some((a) => a.id === "t-query")).toBe(true);
    expect(getMiniApp("t-query")).toEqual({ id: "t-query", titleKey: "q", width: 300, render: noop });
  });
});
