import { describe, it, expect, vi, beforeEach } from "vitest";
import { crossFadeTheme, resetThemeFade } from "../themeFade";
import { beginDragClarity, isDragClarityActive } from "../dragClarity";
import {
  DEFAULT_HOTSPOT_CONFIG, hitEdge, actionFor, loadHotspotConfig, saveHotspotConfig, resetHotspotConfig,
} from "../hotspots";

const hasDom = typeof document !== "undefined";
const itDom = hasDom ? it : it.skip;

describe("AI-17 themeFade（Z-68）", () => {
  beforeEach(() => resetThemeFade());

  it("连续快速切换防抖：只有最后一次生效", () => {
    vi.useFakeTimers();
    const applied: string[] = [];
    crossFadeTheme("light", { apply: (t) => applied.push(t), debounceMs: 50 });
    crossFadeTheme("dark", { apply: (t) => applied.push(t), debounceMs: 50 });
    vi.advanceTimersByTime(120);
    expect(applied).toEqual(["dark"]);
    vi.useRealTimers();
  });

  it("cancel 取消未决切换", () => {
    vi.useFakeTimers();
    const applied: string[] = [];
    const cancel = crossFadeTheme("light", { apply: (t) => applied.push(t), debounceMs: 50 });
    cancel();
    vi.advanceTimersByTime(120);
    expect(applied).toEqual([]);
    vi.useRealTimers();
  });

  itDom("切换应用后为主题添加交叉淡入类（双帧）", async () => {
    const host = document.createElement("div");
    crossFadeTheme("light", { apply: () => {}, host, debounceMs: 0 });
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    expect(host.classList.contains("theme-fade-root")).toBe(true);
  });
});

describe("AI-17 dragClarity（Z-65）", () => {
  it("进入/退出拖动降质态（body class）", () => {
    const exit = beginDragClarity("medium");
    expect(typeof exit).toBe("function");
    if (hasDom) expect(isDragClarityActive()).toBe(true);
    exit();
    expect(isDragClarityActive()).toBe(false);
  });

  it("high 档全程满清：不进入降质态", () => {
    const exit = beginDragClarity("high");
    expect(isDragClarityActive()).toBe(false);
    exit();
  });
});

describe("AI-17 hotspots（Z-69）", () => {
  beforeEach(() => resetHotspotConfig());

  it("默认仅左下角=开始菜单，其余完全无行为", () => {
    const cfg = loadHotspotConfig();
    expect(cfg.positions.bl).toBe("start-menu");
    for (const edge of ["tl", "tr", "br", "top", "left", "right"] as const) {
      expect(actionFor(cfg, edge)).toBe("none");
    }
    expect(actionFor(cfg, "bl")).toBe("start-menu");
  });

  it("hitEdge 命中四角与三边", () => {
    expect(hitEdge(2, 2, 1920, 1080, 6)).toBe("tl");
    expect(hitEdge(1918, 2, 1920, 1080, 6)).toBe("tr");
    expect(hitEdge(2, 1078, 1920, 1080, 6)).toBe("bl");
    expect(hitEdge(1918, 1078, 1920, 1080, 6)).toBe("br");
    expect(hitEdge(960, 2, 1920, 1080, 6)).toBe("top");
    expect(hitEdge(2, 540, 1920, 1080, 6)).toBe("left");
    expect(hitEdge(1918, 540, 1920, 1080, 6)).toBe("right");
    expect(hitEdge(960, 540, 1920, 1080, 6)).toBeNull();
  });

  it("disabled 时全部热区无响应", () => {
    const cfg = { ...DEFAULT_HOTSPOT_CONFIG, enabled: false };
    expect(actionFor(cfg, "bl")).toBe("none");
  });

  it("save/load round-trip 持久化", () => {
    const cfg = { ...DEFAULT_HOTSPOT_CONFIG, positions: { tr: "quick-panel" as const }, hoverDwellMs: 400 };
    saveHotspotConfig(cfg);
    const loaded = loadHotspotConfig();
    expect(loaded.positions.tr).toBe("quick-panel");
    expect(loaded.hoverDwellMs).toBe(400);
    resetHotspotConfig();
    expect(loadHotspotConfig().positions.tr).toBeUndefined();
  });
});
