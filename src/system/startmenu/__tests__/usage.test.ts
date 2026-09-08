import { beforeEach, describe, expect, it, vi } from "vitest";
import { bumpUsage, topUsed, usageCount, usageCounts } from "../usage";

/**
 * 化境 V-12 回归：高频使用本地计数 ——
 * - 只在开始菜单点击时累加（bumpUsage），localStorage 持久化（variable:start:usage:v1）；
 * - topUsed 取前 N（count>0，次数降序、同数按 id 稳定）；
 * - 测试环境无 localStorage：装一个 Map 垫片 + resetModules 后动态导入。
 */

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

describe("usage（V-12 本地计数）", () => {
  it("bumpUsage 累加、usageCount/usageCounts 读取", async () => {
    const u = await import("../usage");
    expect(u.usageCount("app-write")).toBe(0);
    u.bumpUsage("app-write");
    u.bumpUsage("app-write");
    u.bumpUsage("tp-x");
    expect(u.usageCount("app-write")).toBe(2);
    expect(u.usageCount("tp-x")).toBe(1);
    expect(u.usageCounts()["app-write"]).toBe(2);
  });

  it("topUsed：按次数降序取前 N，count=0 不出现，同数按 id 稳定", async () => {
    const u = await import("../usage");
    u.bumpUsage("b");
    u.bumpUsage("b");
    u.bumpUsage("b");
    u.bumpUsage("a");
    u.bumpUsage("a");
    u.bumpUsage("c");
    u.bumpUsage("d"); // 与 c 同数（1），id 稳定序 c < d
    expect(u.topUsed(3).map((e) => e.id)).toEqual(["b", "a", "c"]);
    expect(u.topUsed(3)[2]?.count).toBe(1);
    expect(u.topUsed(0)).toEqual([]);
  });

  it("localStorage 持久化：重载模块后计数仍在", async () => {
    const u1 = await import("../usage");
    u1.bumpUsage("tp-persist");
    u1.bumpUsage("tp-persist");
    vi.resetModules();
    const u2 = await import("../usage");
    expect(u2.usageCount("tp-persist")).toBe(2);
  });

  it("空 id 忽略（防误伤）", async () => {
    const u = await import("../usage");
    u.bumpUsage("");
    expect(Object.keys(u.usageCounts())).toHaveLength(0);
  });

  it("导出面完整（bumpUsage/topUsed/usageCount/usageCounts 可用）", async () => {
    const mod = await import("../usage");
    expect(typeof mod.bumpUsage).toBe("function");
    expect(typeof mod.topUsed).toBe("function");
    expect(typeof mod.usageCount).toBe("function");
    expect(typeof mod.usageCounts).toBe("function");
  });

  it("usageCounts 返回快照（改快照不影响内部态）", async () => {
    const mod = await import("../usage");
    mod.bumpUsage("snap");
    const snap = mod.usageCounts();
    snap["snap"] = 999;
    expect(mod.usageCount("snap")).toBe(1);
  });
});
