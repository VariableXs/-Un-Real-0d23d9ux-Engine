/**
 * AI-16 单测：N-32 智能通知整理（notifySmart 纯逻辑）+ U-51 排队礼仪。
 */
import { beforeEach, describe, expect, it } from "vitest";
import {
  buildDigests,
  classifySource,
  EMPTY_LEARN,
  learnedStats,
  loadLearn,
  recordAction,
  saveLearn,
  setOverride,
} from "../notifySmart";

describe("classifySource", () => {
  it("无学习数据 = 冷启动即时", () => {
    expect(classifySource("a", undefined)).toBe("instant");
  });
  it("互动不足 8 条不分层", () => {
    expect(classifySource("a", { opened: 1, dismissed: 2, lastAt: 0 })).toBe("instant");
  });
  it("反复划掉不点开 → 静默", () => {
    expect(classifySource("a", { opened: 0, dismissed: 12, lastAt: 0 })).toBe("silent");
  });
  it("点开率 ≥ 0.3 → 即时；否则摘要", () => {
    expect(classifySource("a", { opened: 3, dismissed: 7, lastAt: 0 })).toBe("instant");
    expect(classifySource("a", { opened: 2, dismissed: 8, lastAt: 0 })).toBe("digest");
  });
  it("用户覆盖优先于学习结论", () => {
    const learn = { opened: 5, dismissed: 5, lastAt: 0 };
    expect(classifySource("a", learn, "silent")).toBe("silent");
    expect(classifySource("a", { opened: 0, dismissed: 20, lastAt: 0 }, "instant")).toBe("instant");
  });
});

describe("recordAction / setOverride / learnedStats", () => {
  it("累计点开/划掉并更新时间戳", () => {
    let t = EMPTY_LEARN;
    t = recordAction(t, "x", "opened", 100);
    t = recordAction(t, "x", "opened", 200);
    t = recordAction(t, "x", "dismissed", 300);
    expect(t.sources["x"]).toEqual({ opened: 2, dismissed: 1, lastAt: 300 });
  });
  it("空白来源不记录", () => {
    expect(recordAction(EMPTY_LEARN, "  ", "opened")).toBe(EMPTY_LEARN);
  });
  it("覆盖可设置与清除，learnedStats 透明呈现", () => {
    let t = recordAction(EMPTY_LEARN, "x", "dismissed");
    t = { ...t, sources: { ...t.sources, x: { opened: 0, dismissed: 20, lastAt: t.sources["x"]?.lastAt ?? 0 } } };
    t = setOverride(t, "x", "instant");
    const stats = learnedStats(t);
    expect(stats[0]?.stack).toBe("instant");
    expect(stats[0]?.overridden).toBe(true);
    const cleared = setOverride(t, "x", null);
    expect(learnedStats(cleared)[0]?.stack).toBe("silent");
    expect(learnedStats(cleared)[0]?.overridden).toBe(false);
  });
});

describe("buildDigests", () => {
  it("只聚合 digest 堆与时间窗内的来源", () => {
    const items = [
      { source: "a", title: "t1", ts: 1000 },
      { source: "a", title: "t2", ts: 2000 },
      { source: "a", title: "t3", ts: 3000 },
      { source: "b", title: "t4", ts: 1500 },
      { source: "c", title: "old", ts: 100 },
    ];
    const digests = buildDigests(items, (s) => (s === "a" ? "digest" : s === "b" ? "instant" : "digest"), 500);
    expect(digests).toHaveLength(1);
    expect(digests[0]?.source).toBe("a");
    expect(digests[0]?.count).toBe(3);
    expect(digests[0]?.titles).toEqual(["t1", "t2", "t3"]);
  });
});

describe("localStorage 持久化", () => {
  beforeEach(() => {
    localStorage.clear();
  });
  it("save→load 往返一致", () => {
    const t = recordAction(EMPTY_LEARN, "persist", "opened");
    saveLearn(t);
    const loaded = loadLearn();
    expect(loaded.sources["persist"]?.opened).toBe(1);
  });
  it("损坏数据回退空表", () => {
    localStorage.setItem("variable.notifyLearn.v1", "{broken");
    expect(loadLearn()).toEqual(EMPTY_LEARN);
  });
  it("save 滚动窗口：14 天前的来源被淘汰", () => {
    const old = Date.now() - 20 * 86_400_000;
    const t = {
      sources: { stale: { opened: 1, dismissed: 0, lastAt: old }, fresh: { opened: 0, dismissed: 1, lastAt: Date.now() } },
      overrides: {},
    };
    saveLearn(t);
    const loaded = loadLearn();
    expect(loaded.sources["stale"]).toBeUndefined();
    expect(loaded.sources["fresh"]).toBeDefined();
  });
});
