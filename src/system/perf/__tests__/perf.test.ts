/**
 * AI-13 性能与长跑组 — 前端单测
 * （M-50 eventShed / M-49 lruCache / U-21 framebudget / Z-60 memPressure /
 *   Z-62 usageStats / Z-57 safeBoot / U-23 sessionNarrative / U-19+M-52 bootTimeline）
 */

import { describe, it, expect, beforeEach, vi, afterEach } from "vitest";
import { shedChannel, shedEmit, shedStats, shedReset } from "../eventShed";
import { LruByteCache, hitRate } from "../lruCache";
import {
  setFrameTier, getFrameTier, frameBudgetMs, chargeFrame, avgCost, overBudget, downgradeTier,
} from "../framebudget";
import {
  applyTier, memTier, tierFromHeap, tierFromLoad, manualReset, TIER_ACTIONS, resetMemPressure, onMemPressure,
} from "../memPressure";
import {
  recordAction, topActions, dailyTotals, clearStats, resetStatsForTest, dayKey, recordAppTime, loadStats,
} from "../usageStats";
import { detectSafeBoot, suggestSafeBoot, safeModeAllowedOps, SAFE_SKIPPED_LOADERS } from "../safeBoot";
import {
  markBootStage, bootMarks, bootDurations, bootSnapshot, bootDiff, deferrableAfterReady, resetBootTimeline,
} from "../bootTimeline";
import {
  takePreviousNarrative, previousSessionCrashed, previousNarrative, stopSessionNarrative,
} from "../sessionNarrative";
import { deltaBudgetOk, markRollback, initChannel, loadRollbackTestHelper } from "../updateChannel";

beforeEach(() => {
  vi.useFakeTimers();
  shedReset();
  resetMemPressure();
  resetStatsForTest();
  clearStats();
  resetBootTimeline();
  localStorage.clear();
});

afterEach(() => {
  vi.useRealTimers();
  stopSessionNarrative();
});

// ---- M-50 事件削峰 ----
describe("M-50 eventShed", () => {
  it("latest 策略：1000 事件/秒风暴下最新必达", () => {
    const got: number[] = [];
    shedChannel<number>("storm", { strategy: "latest", windowMs: 50 }, (v) => got.push(v));
    for (let i = 0; i < 1000; i += 1) shedEmit("storm", i);
    vi.advanceTimersByTime(60);
    expect(got).toEqual([999]); // 最新必达 100%
    const stats = shedStats("storm")!;
    expect(stats.received).toBe(1000);
    expect(stats.delivered).toBe(1); // 回调次数收敛
  });

  it("merge 策略：同窗事件合并为单负载", () => {
    const got: number[] = [];
    shedChannel<number>("merge", { strategy: "merge", windowMs: 40, merge: (b) => b.reduce((a, c) => a + c, 0) }, (v) => got.push(v));
    for (let i = 1; i <= 10; i += 1) shedEmit("merge", i);
    vi.advanceTimersByTime(50);
    expect(got).toEqual([55]);
  });

  it("queue 策略：保序 + 队列上限丢最旧、保留最新", () => {
    const got: number[] = [];
    shedChannel<number>("queue", { strategy: "queue", windowMs: 30, maxQueue: 3 }, (v) => got.push(v));
    for (let i = 0; i < 10; i += 1) shedEmit("queue", i);
    vi.advanceTimersByTime(40);
    expect(got).toEqual([7, 8, 9]);
  });

  it("未注册频道不泄漏、直接丢弃", () => {
    expect(shedStats("ghost")).toBeNull();
    expect(() => shedEmit("ghost", 1)).not.toThrow();
  });
});

// ---- M-49 图标缓存 LRU ----
describe("M-49 lruCache", () => {
  it("淘汰顺序 = last_used 最旧优先", () => {
    const c = new LruByteCache<string>(3, 1024);
    c.set("a", "1", 10);
    c.set("b", "2", 10);
    c.set("c", "3", 10);
    c.get("a"); // a 变新
    c.set("d", "4", 10); // 应淘汰 b
    expect(c.has("a")).toBe(true);
    expect(c.has("b")).toBe(false);
    expect(c.has("c")).toBe(true);
    expect(c.has("d")).toBe(true);
  });

  it("字节预算淘汰 + 单条超预算拒收", () => {
    const c = new LruByteCache<string>(100, 100);
    c.set("big", "x", 200); // 超预算拒收
    expect(c.has("big")).toBe(false);
    c.set("a", "x", 60);
    c.set("b", "y", 50); // 总量 110 > 100 → 淘汰 a
    expect(c.has("a")).toBe(false);
    expect(c.stats().bytes).toBe(50);
    expect(c.stats().evicted).toBeGreaterThanOrEqual(1);
  });

  it("分批淘汰每批 ≤32 且命中率统计正确", () => {
    const c = new LruByteCache<string>(10, 10_000_000);
    for (let i = 0; i < 45; i += 1) c.set(`k${i}`, "v", 1);
    // 全部写入后 map 超过 maxEntries 的部分由 evictBatch 分批收
    let batches = 0;
    while (c.stats().count > 10 && batches < 100) {
      const n = c.evictBatch(32);
      expect(n).toBeLessThanOrEqual(32);
      batches += 1;
    }
    expect(c.stats().count).toBe(10);
    const s = c.stats();
    expect(hitRate(s)).toBe(0); // 全 miss（未 get）
    c.get("k44");
    expect(hitRate(c.stats())).toBeGreaterThan(0);
  });
});

// ---- U-21 帧预算器 ----
describe("U-21 framebudget", () => {
  it("三档预算映射", () => {
    setFrameTier("full");
    expect(frameBudgetMs()).toBeCloseTo(16.6);
    setFrameTier("half");
    expect(frameBudgetMs()).toBe(33);
    setFrameTier("floor");
    expect(frameBudgetMs()).toBe(50);
    expect(getFrameTier()).toBe("floor");
  });

  it("成本账本与超预算判定", () => {
    setFrameTier("full");
    for (let i = 0; i < 40; i += 1) chargeFrame("starfield", 20); // 20ms > 16.6
    expect(avgCost("starfield")).toBeCloseTo(20);
    expect(overBudget("starfield")).toBe(true);
    expect(overBudget("idle-site")).toBe(false); // 未登记
  });

  it("降档只降不升（full→half→floor）", () => {
    expect(downgradeTier("full")).toBe("half");
    expect(downgradeTier("half")).toBe("floor");
    expect(downgradeTier("floor")).toBe("floor");
  });
});

// ---- Z-60 内存压力自适应 ----
describe("Z-60 memPressure", () => {
  it("水位三档推导", () => {
    expect(tierFromHeap(80, 100)).toBe(0);
    expect(tierFromHeap(86, 100)).toBe(1);
    expect(tierFromHeap(94, 100)).toBe(2);
    expect(tierFromLoad(95)).toBe(2);
    expect(tierFromLoad(85)).toBe(1);
    expect(tierFromLoad(50)).toBe(0);
  });

  it("只降不升：恢复不反向自动升级，manualReset 显式复位", () => {
    const events: number[] = [];
    onMemPressure((e) => events.push(e.tier));
    applyTier(1, "heap");
    applyTier(2, "system");
    expect(memTier()).toBe(2);
    applyTier(1, "system"); // 反向被抑制
    applyTier(0, "system");
    expect(memTier()).toBe(2);
    expect(events).toEqual([1, 2]);
    manualReset();
    expect(memTier()).toBe(0);
  });

  it("降档动作表契约形状先约", () => {
    expect(TIER_ACTIONS[0]).toEqual([]);
    expect(TIER_ACTIONS[1]).toContain("clear-caches");
    expect(TIER_ACTIONS[2]).toContain("floor-frame-tier");
    expect(TIER_ACTIONS[2]).toContain("notify-user");
  });
});

// ---- Z-62 本地统计 ----
describe("Z-62 usageStats", () => {
  it("记数 + Top 聚合 + 每日总量", () => {
    recordAction("save", 3);
    recordAction("save", 2);
    recordAction("undo");
    recordAppTime("writer", 120);
    const top = topActions(20);
    expect(top[0]).toEqual({ action: "save", count: 5 });
    expect(loadStats().appSeconds["writer"]).toBe(120);
    expect(dailyTotals(7)[0]!.total).toBe(6);
    expect(dailyTotals(7)[0]!.day).toBe(dayKey());
  });

  it("一键清除彻底（零残留）", () => {
    recordAction("a");
    clearStats();
    expect(topActions(20)).toEqual([]);
    expect(localStorage.getItem("vxs:usageStats")).toBeNull();
  });
});

// ---- Z-57 安全模式 ----
describe("Z-57 safeBoot", () => {
  it("参数 > 设置 > 崩溃连击 的判定顺序", () => {
    expect(detectSafeBoot("?safe=1", false, false).source).toBe("param");
    expect(detectSafeBoot("", true, false).source).toBe("setting");
    expect(detectSafeBoot("", false, true).source).toBe("crash-streak");
    expect(detectSafeBoot("", false, false).safe).toBe(false);
  });

  it("跳过清单覆盖第三方资源且只读白名单守住不改配置红线", () => {
    const r = detectSafeBoot("?safe=1", false);
    expect(r.skipped).toEqual([...SAFE_SKIPPED_LOADERS]);
    expect(safeModeAllowedOps("read-settings")).toBe(true);
    expect(safeModeAllowedOps("write-user-config")).toBe(false);
  });

  it("连续 2 次异常退出 → 建议安全模式", () => {
    expect(suggestSafeBoot(2).suggest).toBe(true);
    expect(suggestSafeBoot(1).suggest).toBe(false);
  });
});

// ---- U-23 崩溃叙事 ----
describe("U-23 sessionNarrative", () => {
  it("异常叙事写入 → 下次启动恢复卡片一次性取走", () => {
    localStorage.setItem("vxs:sessionNarrative", JSON.stringify({
      sessionStart: 1, lastAlive: 2,
      events: [{ kind: "error", message: "boom", site: "desktop", ts: 2 }],
    }));
    expect(previousSessionCrashed()).toBe(true);
    const n = takePreviousNarrative();
    expect(n?.events[0]!.message).toBe("boom");
    expect(localStorage.getItem("vxs:sessionNarrative")).toBeNull(); // 一次性
  });

  it("正常会话（无事件）不算崩溃", () => {
    previousNarrative(); // 空
    expect(previousSessionCrashed()).toBe(false);
  });
});

// ---- U-19 + M-52 启动时间线 ----
describe("U-19/M-52 bootTimeline", () => {
  it("阶段标记 / 耗时 / P1+P2 可延迟清单", () => {
    markBootStage("app-shell", 0);
    markBootStage("settings-load", 1);
    markBootStage("icons-warm", 2);
    const marks = bootMarks();
    expect(marks.length).toBe(3);
    expect(marks[0]!.name).toBe("app-shell");
    const durs = bootDurations();
    expect(durs[0]!.name).toBe("app-shell");
    expect(deferrableAfterReady().map((m) => m.name)).toEqual(["settings-load", "icons-warm"]);
  });

  it("bootDiff：同名阶段回归 >10% 标红", () => {
    const base = [
      { name: "a", priority: 0 as const, atMs: 0 },
      { name: "b", priority: 1 as const, atMs: 100 },
      { name: "c", priority: 2 as const, atMs: 150 },
    ];
    const cur = [
      { name: "a", priority: 0 as const, atMs: 0 },
      { name: "b", priority: 1 as const, atMs: 80 },  // 改善
      { name: "c", priority: 2 as const, atMs: 150 }, // 70 vs 50 → +40% 回归
    ];
    const diff = bootDiff(base, cur);
    const a = diff.find((d) => d.name === "a")!;
    const b = diff.find((d) => d.name === "b")!;
    expect(a.regressed).toBe(false); // 100 → 80 改善
    expect(b.regressed).toBe(true); // 50 → 70 回归 +40%
  });

  it("快照 JSON 形状（boot-<version>.json）", () => {
    markBootStage("app-shell", 0);
    const snap = bootSnapshot("1.5.9");
    expect(snap.version).toBe("1.5.9");
    expect(snap.marks[0]!.name).toBe("app-shell");
  });
});

// ---- Z-61 更新通道 ----
describe("Z-61 updateChannel", () => {
  it("增量包预算：diff < 30% 目标才合格", () => {
    expect(deltaBudgetOk(25, 100)).toBe(true);
    expect(deltaBudgetOk(30, 100)).toBe(false);
    expect(deltaBudgetOk(10, 0)).toBe(false);
  });

  it("回滚点登记（双版本回滚）", () => {
    initChannel("1.5.9", "stable");
    markRollback("1.5.8");
    expect(loadRollbackTestHelper()).toBe("1.5.8");
  });
});
