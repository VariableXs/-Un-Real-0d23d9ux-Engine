/** AI-07 · P-1/N-13 命令注册表单测（注册/查询/权重/条件可见性/钉选/学习排序）。 */
import { describe, expect, it, beforeEach } from "vitest";
import {
  __resetForTests,
  executeCommand,
  learnedWeight,
  loadPinned,
  loadStats,
  matchScore,
  pinCommand,
  PIN_CAP,
  queryCommands,
  recordUsage,
  registerCommand,
  reorderPinned,
  serializePinned,
  serializeStats,
  unpinCommand,
  unregisterCommand,
} from "../commands/registry";
import type { Command } from "../commands/types";

function mk(id: string, title: string, category: Command["category"], keywords: string[] = []): Command {
  return { id, titleKey: title, category, keywords, action: () => true, source: "test" };
}

const resolve = (c: Command) => c.titleKey;

beforeEach(() => {
  __resetForTests();
});

describe("P-1 命令注册表", () => {
  it("注册 / 查询 / 重复注册覆盖", () => {
    registerCommand(mk("palette.open", "命令面板", "tool"));
    registerCommand(mk("snap.left", "贴靠左半屏", "action"));
    expect(queryCommands("", { resolveTitle: resolve })).toHaveLength(2);
    registerCommand(mk("palette.open", "命令面板", "tool"));
    expect(queryCommands("", { resolveTitle: resolve })).toHaveLength(2); // 覆盖不重复
    unregisterCommand("palette.open");
    expect(queryCommands("", { resolveTitle: resolve })).toHaveLength(1);
  });

  it("非法声明拒绝", () => {
    // @ts-expect-error 故意缺 action
    expect(() => registerCommand({ id: "x", titleKey: "t", category: "tool" })).toThrow();
  });

  it("模糊匹配：前缀 > 连续子串 > 子序列", () => {
    const c1 = mk("a.b", "settings center", "settings");
    const c2 = mk("a.c", "open system settings", "settings");
    registerCommand(c1);
    registerCommand(c2);
    const s1 = matchScore(c1, "settings", "settings center");
    const s2 = matchScore(c2, "settings", "open system settings");
    expect(s1).toBeGreaterThan(s2); // 前缀命中 > 尾部子串
    expect(matchScore(c1, "zzz", "settings center")).toBe(-1);
    expect(matchScore(c1, "stce", "settings center")).toBeGreaterThan(0); // 子序列
  });

  it("拼音匹配（zh 高频）", () => {
    const c = mk("a.mll", "贴靠左半屏", "action");
    expect(matchScore(c, "tkz", "贴靠左半屏")).toBeGreaterThan(0); // 首字母
  });

  it("条件可见性 when()", () => {
    registerCommand({ ...mk("a.hidden", "隐藏命令", "tool"), when: () => false });
    registerCommand(mk("a.shown", "可见命令", "tool"));
    const hits = queryCommands("", { resolveTitle: resolve });
    expect(hits.map((h) => h.command.id)).toEqual(["a.shown"]);
  });

  it("模式过滤：`>` 命令模式排除 app/settings；`?` 仅 settings", () => {
    registerCommand(mk("app.notes", "便签", "app"));
    registerCommand(mk("set.theme", "主题设置", "settings"));
    registerCommand(mk("act.snap", "贴靠", "action"));
    const cmdMode = queryCommands("", { mode: "commands", resolveTitle: resolve });
    expect(cmdMode.map((h) => h.command.id).sort()).toEqual(["act.snap"]);
    const setMode = queryCommands("", { mode: "settings", resolveTitle: resolve });
    expect(setMode.map((h) => h.command.id)).toEqual(["set.theme"]);
  });
});

describe("学习排序（频次 + 新近度，纯本地）", () => {
  it("统计持久化往返", () => {
    recordUsage("x", 1000);
    recordUsage("x", 2000);
    const saved = serializeStats();
    loadStats(saved);
    expect(learnedWeight("x", 2000)).toBeGreaterThan(0);
    loadStats(null);
    expect(learnedWeight("x")).toBe(0);
  });

  it("损坏统计如实从零开始", () => {
    loadStats("{{not-json");
    expect(serializeStats()).toBe("{}");
  });

  it("高频命令排序靠前", () => {
    registerCommand(mk("a.fresh", "常用", "tool"));
    registerCommand(mk("a.cold", "冷门", "tool"));
    recordUsage("a.fresh", Date.now());
    recordUsage("a.fresh", Date.now());
    const hits = queryCommands("", { resolveTitle: resolve });
    expect(hits[0]!.command.id).toBe("a.fresh");
  });

  it("新近度半衰：老命令权重低于新命令", () => {
    const now = Date.now();
    recordUsage("a.old", now - 30 * 24 * 3600 * 1000);
    recordUsage("a.new", now);
    expect(learnedWeight("a.new", now)).toBeGreaterThan(learnedWeight("a.old", now));
  });
});

describe("钉选（V-45 同款钉选位）", () => {
  it("钉选/取消/持久化往返", () => {
    expect(pinCommand("m1")).toBe(true);
    expect(pinCommand("m2")).toBe(true);
    const saved = serializePinned();
    loadPinned(saved);
    const hits = queryCommands("", { resolveTitle: resolve }); // 空注册表也行
    expect(hits).toHaveLength(0);
    unpinCommand("m1");
    expect(serializePinned()).toBe(JSON.stringify({ ids: ["m2"], cap: PIN_CAP }));
  });

  it("超上限如实失败", () => {
    for (let i = 0; i < PIN_CAP; i++) expect(pinCommand(`m${i}`)).toBe(true);
    expect(pinCommand("overflow")).toBe(false);
  });

  it("拖拽排序保持缺失项", () => {
    pinCommand("a");
    pinCommand("b");
    pinCommand("c");
    reorderPinned(["c", "a"]);
    expect(JSON.parse(serializePinned()).ids).toEqual(["c", "a", "b"]);
  });
});

describe("执行", () => {
  it("执行并记录统计；when() 拦截返回 false", async () => {
    let ran = 0;
    registerCommand({ ...mk("a.run", "执行", "tool"), action: () => void ran++ });
    await executeCommand("a.run");
    expect(ran).toBe(1);
    expect(JSON.parse(serializeStats())["a.run"].count).toBe(1);
    registerCommand({ ...mk("a.gated", "门控", "tool"), when: () => false });
    expect(await executeCommand("a.gated")).toBe(false);
    await expect(executeCommand("nope")).rejects.toThrow();
  });
});
