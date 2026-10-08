import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";
import {
  PERF_PROFILES,
  keepAwakeDeadline,
  keepAwakeExpired,
  loadPerfMode,
  matchSchemeGuid,
  savePerfMode,
  type PerfMode,
} from "../perfmode";
import {
  loadWarmList,
  recordLaunch,
  removeWarm,
  warmCandidates,
} from "../predmodel";
import {
  clearMonProfile,
  diffMonProfile,
  loadMonProfile,
  saveMonProfile,
} from "../monprofile";

describe("AI-11 perfmode (U-48 / V-54)", () => {
  it("三档预设含电源计划关键词与保持唤醒位", () => {
    expect(PERF_PROFILES.eco.keepAwake).toBe(false);
    expect(PERF_PROFILES.boost.keepAwake).toBe(true);
    expect(PERF_PROFILES.eco.planHints.length).toBeGreaterThan(0);
  });

  it("按名称关键词匹配系统计划 GUID；匹配不到如实返回 null", () => {
    const schemes = [
      { guid: "a", name: "节能", active: false },
      { guid: "b", name: "平衡", active: true },
      { guid: "c", name: "高性能", active: false },
    ];
    expect(matchSchemeGuid(schemes, "eco")).toBe("a");
    expect(matchSchemeGuid(schemes, "boost")).toBe("c");
    expect(matchSchemeGuid([{ guid: "x", name: "我的方案", active: false }], "eco")).toBeNull();
  });

  it("默认 balanced，save/load 往返（localStorage mock）", () => {
    const store = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    });
    expect(loadPerfMode()).toBe("balanced");
    savePerfMode("boost");
    expect(loadPerfMode()).toBe("boost");
    vi.unstubAllGlobals();
  });

  it("保持唤醒到期计算（V-54）", () => {
    expect(keepAwakeDeadline(1000, 0)).toBeNull();
    expect(keepAwakeDeadline(1000, 1)).toBe(61_000);
    expect(keepAwakeExpired(60_000, 61_000)).toBe(false);
    expect(keepAwakeExpired(61_000, 61_000)).toBe(true);
    expect(keepAwakeExpired(0, null)).toBe(false);
  });
});

describe("AI-11 predmodel (N-24)", () => {
  const store = new Map<string, string>();
  beforeEach(() => {
    store.clear();
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    });
  });
  afterEach(() => vi.unstubAllGlobals());

  it("记录启动 → 候选白名单按计数排序，仅可执行扩展名", () => {
    recordLaunch("C:\\a\\app.exe", 100);
    recordLaunch("C:\\a\\app.exe", 200);
    recordLaunch("C:\\b\\tool.exe", 300);
    recordLaunch("C:\\c\\readme.txt", 400);
    const c = warmCandidates(5);
    expect(c[0]).toBe("C:\\a\\app.exe");
    expect(c).not.toContain("C:\\c\\readme.txt");
  });

  it("移除单条与空列表", () => {
    recordLaunch("C:\\x\\a.exe", 1);
    removeWarm("C:\\x\\a.exe");
    expect(loadWarmList()).toEqual([]);
    expect(warmCandidates()).toEqual([]);
  });
});

describe("AI-11 monprofile (U-43)", () => {
  const store = new Map<string, string>();
  beforeEach(() => {
    store.clear();
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
    });
  });
  afterEach(() => vi.unstubAllGlobals());

  const m1: { device: string; x: number; y: number; w: number; h: number } = {
    device: "\\\\.\\DISPLAY1",
    x: 0,
    y: 0,
    w: 1920,
    h: 1080,
  };
  const m2: { device: string; x: number; y: number; w: number; h: number } = {
    device: "\\\\.\\DISPLAY2",
    x: 1920,
    y: 0,
    w: 1280,
    h: 720,
  };
  const mons = [m1, m2];

  it("保存后一致布局无差异", () => {
    saveMonProfile(mons, 1);
    expect(diffMonProfile(loadMonProfile()!, mons)).toEqual([]);
  });

  it("移动/新增/缺失都能识别", () => {
    saveMonProfile(mons, 1);
    const p = loadMonProfile()!;
    expect(diffMonProfile(p, [{ ...m1, x: 10 }, m2])).toContainEqual({
      device: "\\\\.\\DISPLAY1",
      kind: "moved",
    });
    expect(
      diffMonProfile(p, [m1, m2, { device: "\\\\.\\DISPLAY3", x: 0, y: 0, w: 800, h: 600 }]),
    ).toContainEqual({
      device: "\\\\.\\DISPLAY3",
      kind: "added",
    });
    expect(diffMonProfile(p, [m1])).toContainEqual({ device: "\\\\.\\DISPLAY2", kind: "missing" });
    clearMonProfile();
    expect(loadMonProfile()).toBeNull();
  });
});

// 避免 type PerfMode 未使用告警（预设键遍历已在 perfmode 内保证）
export type _PerfModeAlias = PerfMode;
