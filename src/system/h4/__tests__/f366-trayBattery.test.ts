import { describe, expect, it } from "vitest";
import { __clearMem, memStore } from "../internal/store";
import { auditBandReconciliation, colorBand, getMode, lowLevel, renderTray, setMode } from "../f366-trayBattery";

describe("F366 托盘电池显示选项", () => {
  it("三档持久化：默认 icon、切换即时、round-trip 保真", () => {
    __clearMem();
    const s = memStore();
    expect(getMode(s)).toBe("icon");
    expect(setMode("percent", s)).toBe(true);
    expect(getMode(s)).toBe("percent");
  });

  it("色段四档阈值对账（判据逐点）", () => {
    const a = auditBandReconciliation();
    expect(a.pass).toBe(true);
    expect(colorBand(15)).toBe("red");
    expect(colorBand(30)).toBe("yellow");
    expect(colorBand(60)).toBe("normal");
    expect(colorBand(61)).toBe("green");
  });

  it("低电三态独立视觉：notice/warning=黄、critical=红+闪烁；插电全消", () => {
    expect(lowLevel(29, false)).toBe("notice");
    expect(lowLevel(19, false)).toBe("warning");
    expect(lowLevel(10, false)).toBe("critical");
    expect(lowLevel(31, false)).toBe("none");
    expect(lowLevel(5, true)).toBe("none"); // 插电不惊扰
    const crit = renderTray({ percent: 8, plugged: false }, "icon");
    expect(crit.low).toEqual({ level: "critical", color: "red", blink: true });
  });

  it("三档渲染：纯图标无数字；图标+百分比；纯百分比无图标", () => {
    const r = { percent: 42, plugged: false };
    expect(renderTray(r, "icon")).toMatchObject({ icon: true, percentText: null, band: "normal" });
    const ip = renderTray(r, "iconPercent");
    expect(ip.percentText).toBe("42%");
    expect(ip.icon).toBe(true);
    const p = renderTray(r, "percent");
    expect(p.icon).toBe(false);
    expect(p.percentText).toBe("42%");
  });

  it("插电闪电标记三档通用", () => {
    for (const mode of ["icon", "iconPercent", "percent"] as const) {
      expect(renderTray({ percent: 50, plugged: true }, mode).bolt).toBe(true);
      expect(renderTray({ percent: 50, plugged: false }, mode).bolt).toBe(false);
    }
  });

  it("非法电量读数显式钳制（零 NaN/越界渲染）", () => {
    expect(renderTray({ percent: Number.NaN, plugged: false }, "percent").percentText).toBe("0%");
    expect(renderTray({ percent: 150, plugged: false }, "percent").percentText).toBe("100%");
    expect(renderTray({ percent: -3, plugged: false }, "percent").percentText).toBe("0%");
  });
});
