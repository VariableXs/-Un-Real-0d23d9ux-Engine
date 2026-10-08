import { beforeEach, describe, expect, it } from "vitest";
import {
  SYSTEM_ITEMS, LOCKED_ITEMS, loadCtxMenuConfig, hideItem,
  restoreItem, hiddenRatioWarning, recordUsage, usageCount, autoSort,
  pruneUninstalledItems, POPUP_BUDGET_MS, USAGE_RING_DAYS,
} from "../ctxmenu";
import {
  loadTaskbarPrefs, saveTaskbarPrefs, effectiveAutoHide, trayShouldFold,
  shouldReveal, startMenuReachableWhenHidden, ICON_SIZE_PX, HEIGHT_PX,
  REVEAL_DELAY_MS, REVEAL_HOTZONE_PX, REARRANGE_MS,
} from "../taskbarprefs";
import { personaStore } from "../store";

beforeEach(() => {
  personaStore.reset();
});

describe("F167 右键菜单自定义", () => {
  it("隐藏-生效-恢复闭环", () => {
    let cfg = loadCtxMenuConfig();
    const r = hideItem(cfg, "open");
    expect(r.ok).toBe(true);
    expect(r.reason).toContain("显示更多选项");
    cfg = r.config;
    expect(cfg.items["open"]?.hidden).toBe(true);
    cfg = restoreItem(cfg, "open").config;
    expect(cfg.items["open"]?.hidden).toBe(false);
  });

  it("关键项锁定不可隐（防自残红线显式）", () => {
    expect([...LOCKED_ITEMS].sort()).toEqual(["delete", "properties", "rename"]);
    const r = hideItem(loadCtxMenuConfig(), "delete");
    expect(r.ok).toBe(false);
    expect(r.reason).toContain("系统关键项");
  });

  it("隐藏 >50% → 提示但允许", () => {
    let cfg = loadCtxMenuConfig();
    for (const id of ["open", "open-with", "cut", "copy", "send-to"]) {
      cfg = hideItem(cfg, id).config;
    }
    const warn = hiddenRatioWarning(cfg);
    expect(warn).toContain("不合群");
    const few = hideItem(cfg, "open").config;
    expect(hiddenRatioWarning({ ...few, items: { ...few.items, open: { ...few.items["open"]!, hidden: false } } })).toBeNull();
  });

  it("使用计数 90 天环形 + 自动排序仅作用应用组（系统组位置不动）", () => {
    expect(USAGE_RING_DAYS).toBe(90);
    let cfg = loadCtxMenuConfig();
    cfg.items["app-a"] = { id: "app-a", appRegistered: true, hidden: false };
    cfg.items["app-b"] = { id: "app-b", appRegistered: true, hidden: false };
    const openWith = cfg.items["open-with"];
    if (openWith) openWith.appRegistered = true;
    const now = Date.now();
    for (let i = 0; i < 5; i++) cfg = recordUsage(cfg, "app-b", now);
    cfg = recordUsage(cfg, "app-a", now);
    expect(usageCount(cfg, "app-b")).toBe(5);
    expect(usageCount(cfg, "app-a")).toBe(1);
    expect(usageCount(cfg, "open")).toBe(0);
    // autoSort 关闭 → 原序
    expect(autoSort({ ...cfg, autoSort: false }, ["open", "copy", "app-b", "app-a"])).toEqual(["open", "copy", "app-b", "app-a"]);
    // 开启 → 系统组保序在前，应用组按计数降序
    expect(autoSort({ ...cfg, autoSort: true }, ["open", "copy", "app-b", "app-a"])).toEqual(["open", "copy", "app-b", "app-a"]);
  });

  it("应用卸载 → 其菜单项自动清（系统组不受影响）", () => {
    let cfg = loadCtxMenuConfig();
    cfg.items["app-x"] = { id: "app-x", appRegistered: true, hidden: false };
    const pruned = pruneUninstalledItems(cfg, new Set<string>());
    expect(pruned.removed).toEqual(["app-x"]);
    expect(pruned.config.items["open"]).toBeTruthy();
  });

  it("弹出延迟红线 100ms 常量在位", () => {
    expect(POPUP_BUDGET_MS).toBe(100);
    expect(SYSTEM_ITEMS.length).toBeGreaterThanOrEqual(8);
  });
});

describe("F168 任务栏个性化", () => {
  it("三选项独立生效互不干扰", () => {
    let p = loadTaskbarPrefs();
    p = { ...p, iconSize: "large" };
    saveTaskbarPrefs(p);
    expect(loadTaskbarPrefs()).toMatchObject({ iconSize: "large", align: "center", autoHide: false });
    saveTaskbarPrefs({ ...loadTaskbarPrefs(), align: "left" });
    expect(loadTaskbarPrefs()).toMatchObject({ iconSize: "large", align: "left", autoHide: false });
    saveTaskbarPrefs({ ...loadTaskbarPrefs(), autoHide: true });
    expect(loadTaskbarPrefs()).toMatchObject({ iconSize: "large", align: "left", autoHide: true });
  });

  it("尺寸档：标准 24/48 大 32/56（乙-1 表增补档）", () => {
    expect(ICON_SIZE_PX).toEqual({ standard: 24, large: 32 });
    expect(HEIGHT_PX).toEqual({ standard: 48, large: 56 });
  });

  it("全屏态强制显示（可交互性优先）", () => {
    const p = loadTaskbarPrefs();
    expect(effectiveAutoHide({ ...p, autoHide: true }, false)).toBe(true);
    expect(effectiveAutoHide({ ...p, autoHide: true }, true)).toBe(false);
  });

  it("隐藏-唤出：热区 6px + 停留 200ms 双条件", () => {
    expect(REVEAL_HOTZONE_PX).toBe(6);
    expect(REVEAL_DELAY_MS).toBe(200);
    expect(shouldReveal(3, 250)).toBe(true);
    expect(shouldReveal(3, 100)).toBe(false);
    expect(shouldReveal(20, 500)).toBe(false);
  });

  it("托盘折叠联动：大图标档阈值自动收紧 75%", () => {
    const p = loadTaskbarPrefs();
    expect(trayShouldFold({ ...p, trayFoldThreshold: 8 }, 9)).toBe(true);
    expect(trayShouldFold({ ...p, trayFoldThreshold: 8 }, 7)).toBe(false);
    // 大图标档：阈值 6（8*0.75）
    const large = { ...p, iconSize: "large" as const, trayFoldThreshold: 8 };
    expect(trayShouldFold(large, 7)).toBe(true);
    expect(trayShouldFold(large, 5)).toBe(false);
  });

  it("隐藏态可连性兜底：Win 键仍弹开始菜单", () => {
    expect(startMenuReachableWhenHidden()).toBe(true);
    expect(REARRANGE_MS).toBe(200);
  });
});
