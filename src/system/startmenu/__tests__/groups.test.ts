import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  HIGH_FREQ_TOP_N,
  highFreqEnabled,
  recentGridId,
  recentlyAdded,
  RECENT_DAYS,
  RECENT_WINDOW_MS,
  setHighFreqEnabled,
  topUsedItems,
} from "../groups";
import type { RecentEntry } from "../recent";

/**
 * 化境 V-12 回归：最近添加 / 高频分组 ——
 * - 「最近添加」= 第三方 addedAt 近 14 天 ∪ recent 近 14 天，按 gridId 去重
 *   （tp 同时命中保留 addedAt 版本），按时间降序；
 * - 「高频」= counts 过滤 count>0 且网格存在，按次数降序、同数按 id 稳定，取前 N；
 * - 开关持久化 localStorage（variable:start:hfreq:v1），默认关。
 * now 显式传入 → 纯函数测试不依赖系统时钟。
 */

const NOW = 1_000_000_000_000;
const DAY = 24 * 60 * 60 * 1000;

function shimStorage(): void {
  const m = new Map<string, string>();
  (globalThis as { localStorage?: unknown }).localStorage = {
    getItem: (k: string) => (m.has(k) ? (m.get(k) as string) : null),
    setItem: (k: string, v: string) => void m.set(k, v),
    removeItem: (k: string) => void m.delete(k),
    clear: () => void m.clear(),
  };
}

beforeEach(() => {
  vi.resetModules();
  shimStorage();
});

describe("recentGridId（kind → 网格项 id）", () => {
  it("app/tp/sys 三种映射，sys 含 tool- 特例", () => {
    expect(recentGridId("app", "write")).toBe("app-write");
    expect(recentGridId("tp", "x1")).toBe("tp-x1");
    expect(recentGridId("sys", "tool-calc")).toBe("tool-calc");
    expect(recentGridId("sys", "explorer")).toBe("sys-explorer");
  });
});

describe("recentlyAdded（V-12 最近 14 天过滤）", () => {
  it("14 天内收录，边界恰好 14 天也收录，15 天排除", () => {
    const thirds = [
      { id: "in14", name: "恰好14天", addedAt: NOW - RECENT_WINDOW_MS },
      { id: "in1", name: "昨天装的", addedAt: NOW - DAY },
      { id: "old", name: "15天前", addedAt: NOW - RECENT_WINDOW_MS - 1 },
    ];
    const out = recentlyAdded(thirds, [], NOW);
    expect(out.map((e) => e.gridId)).toEqual(["tp-in1", "tp-in14"]);
  });

  it("recents 近 14 天收录（app-*），更早排除，未来时间戳排除", () => {
    const recents: RecentEntry[] = [
      { kind: "app", id: "write", name: "写", ts: NOW - 2 * DAY },
      { kind: "app", id: "ancient", name: "远古", ts: NOW - 20 * DAY },
      { kind: "sys", id: "future", name: "未来", ts: NOW + DAY },
    ];
    const out = recentlyAdded([], recents, NOW);
    expect(out.map((e) => e.gridId)).toEqual(["app-write"]);
  });

  it("tp 同时命中 addedAt 与 recent → 保留 addedAt 版本（去重）", () => {
    const thirds = [{ id: "x", name: "装的时间", addedAt: NOW - 3 * DAY }];
    const recents: RecentEntry[] = [{ kind: "tp", id: "x", name: "用的时间", ts: NOW - DAY }];
    const out = recentlyAdded(thirds, recents, NOW);
    expect(out).toHaveLength(1);
    expect(out[0]?.label).toBe("装的时间");
    expect(out[0]?.ts).toBe(NOW - 3 * DAY);
  });

  it("按 ts 降序排列；空输入 → 空输出", () => {
    const thirds = [
      { id: "a", name: "A", addedAt: NOW - 5 * DAY },
      { id: "b", name: "B", addedAt: NOW - DAY },
    ];
    const recents: RecentEntry[] = [{ kind: "app", id: "c", name: "C", ts: NOW - 2 * DAY }];
    const out = recentlyAdded(thirds, recents, NOW);
    expect(out.map((e) => e.gridId)).toEqual(["tp-b", "app-c", "tp-a"]);
    expect(recentlyAdded([], [], NOW)).toEqual([]);
  });

  it("窗口参数可注入（自定义 windowMs）", () => {
    const thirds = [{ id: "a", name: "A", addedAt: NOW - 2 * DAY }];
    expect(recentlyAdded(thirds, [], NOW, DAY)).toEqual([]);
    expect(recentlyAdded(thirds, [], NOW, 3 * DAY)).toHaveLength(1);
  });
});

describe("topUsedItems（V-12 高频 TopN）", () => {
  const items = [{ id: "a" }, { id: "b" }, { id: "c" }];

  it("count>0 且网格存在才收录，按次数降序", () => {
    const counts = { a: 2, b: 3, c: 1, "ghost": 9 };
    const out = topUsedItems(items, 8, counts);
    expect(out.map((e) => e.item.id)).toEqual(["b", "a", "c"]);
    expect(out[0]?.count).toBe(3);
  });

  it("同次数按 id 稳定排序；n 截断与 n=0", () => {
    const counts = { a: 1, b: 1 };
    expect(topUsedItems(items, 8, counts).map((e) => e.item.id)).toEqual(["a", "b"]);
    expect(topUsedItems(items, 2, { a: 3, b: 2, c: 1 }).map((e) => e.item.id)).toEqual(["a", "b"]);
    expect(topUsedItems(items, 0, counts)).toEqual([]);
  });

  it("count=0 不出现", () => {
    expect(topUsedItems(items, 8, { a: 0, b: 1 })).toEqual([{ item: items[1], count: 1 }]);
  });
});

describe("高频分组开关（默认关 + localStorage 持久化）", () => {
  it("默认关；setHighFreqEnabled(true) 后开", () => {
    expect(highFreqEnabled()).toBe(false);
    setHighFreqEnabled(true);
    expect(highFreqEnabled()).toBe(true);
  });

  it("写入 localStorage 键 variable:start:hfreq:v1（前缀约定）", () => {
    setHighFreqEnabled(true);
    expect(globalThis.localStorage.getItem("variable:start:hfreq:v1")).toBe("1");
    setHighFreqEnabled(false);
    expect(globalThis.localStorage.getItem("variable:start:hfreq:v1")).toBe("0");
  });

  it("常量：RECENT_DAYS=14 / HIGH_FREQ_TOP_N=8", () => {
    expect(RECENT_DAYS).toBe(14);
    expect(RECENT_WINDOW_MS).toBe(14 * DAY);
    expect(HIGH_FREQ_TOP_N).toBe(8);
  });
});