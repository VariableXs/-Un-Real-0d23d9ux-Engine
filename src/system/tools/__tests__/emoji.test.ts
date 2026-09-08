import { beforeEach, describe, expect, it } from "vitest";
import {
  ALL_ITEMS,
  EMOJI_CATEGORIES,
  RECENT_KEY,
  RECENT_LIMIT,
  SYMBOL_GROUPS,
  readRecent,
  searchEmoji,
  writeRecent,
} from "../emojiData";

// 测试隔离：recent 走 localStorage（node 环境由 setup 注入内存兜底），每例前清空。
beforeEach(() => {
  localStorage.removeItem(RECENT_KEY);
});

describe("emoji 数据集完整性", () => {
  it("精选数据集总量 ≥ 400", () => {
    expect(ALL_ITEMS.length).toBeGreaterThanOrEqual(400);
  });

  it("九大分类齐全且非空", () => {
    expect(EMOJI_CATEGORIES.map((c) => c.id)).toEqual([
      "smileys",
      "gestures",
      "animals",
      "food",
      "activities",
      "travel",
      "objects",
      "symbols",
      "flags",
    ]);
    for (const c of EMOJI_CATEGORIES) {
      expect(c.items.length).toBeGreaterThan(0);
      for (const item of c.items) {
        expect(item.ch.length).toBeGreaterThan(0);
        expect(item.zh.length).toBeGreaterThan(0);
        expect(item.en.length).toBeGreaterThan(0);
      }
    }
  });

  it("emoji 本体全局不重复", () => {
    const seen = new Set<string>();
    for (const item of ALL_ITEMS) {
      expect(seen.has(item.ch), `重复字符: ${item.ch}`).toBe(false);
      seen.add(item.ch);
    }
  });
});

describe("searchEmoji 搜索", () => {
  it("中文「笑」命中 😄", () => {
    const hits = searchEmoji("笑", "zh");
    expect(hits.length).toBeGreaterThan(0);
    expect(hits.some((h) => h.ch === "😄")).toBe(true);
  });

  it("英文 smile（大小写不敏感）命中 😄", () => {
    for (const q of ["smile", "SMILE", "Smile"]) {
      const hits = searchEmoji(q, "en");
      expect(hits.some((h) => h.ch === "😄"), `查询 ${q} 应命中 😄`).toBe(true);
    }
  });

  it("emoji 本体命中（任意语言）", () => {
    expect(searchEmoji("🚀", "zh").some((h) => h.ch === "🚀")).toBe(true);
    expect(searchEmoji("🚀", "en").some((h) => h.ch === "🚀")).toBe(true);
  });

  it("符号区内容可被搜索（中文「箭头」/ 英文 arrow）", () => {
    expect(searchEmoji("左箭头", "zh").some((h) => h.ch === "←")).toBe(true);
    expect(searchEmoji("left arrow", "en").some((h) => h.ch === "←")).toBe(true);
  });

  it("无结果返回空数组；空查询同样为空", () => {
    expect(searchEmoji("不存在的词zzzz", "zh")).toEqual([]);
    expect(searchEmoji("", "zh")).toEqual([]);
    expect(searchEmoji("   ", "en")).toEqual([]);
  });
});

describe("recent 最近使用（上限 24）", () => {
  it("初始为空，写入后可读回", () => {
    expect(readRecent()).toEqual([]);
    const next = writeRecent("😄");
    expect(next).toEqual(["😄"]);
    expect(readRecent()).toEqual(["😄"]);
  });

  it("重复写入去重并置顶", () => {
    writeRecent("😀");
    writeRecent("😄");
    writeRecent("😀");
    expect(readRecent()).toEqual(["😀", "😄"]);
  });

  it("超过 24 个淘汰最旧", () => {
    // 26 个不同字符写入 → 只保留最近 24 个，最旧两个被淘汰
    const chars = Array.from({ length: 26 }, (_, i) => String.fromCodePoint(0x1f600 + i));
    for (const c of chars) writeRecent(c);
    const recent = readRecent();
    expect(recent).toHaveLength(RECENT_LIMIT);
    expect(recent[0]).toBe(chars[25]); // 最新在首
    expect(recent).not.toContain(chars[0]); // 最旧被淘汰
    expect(recent).not.toContain(chars[1]);
    expect(recent).toContain(chars[2]); // 第 3 个起保留
  });

  it("损坏的存储静默视为空", () => {
    localStorage.setItem(RECENT_KEY, "{不是json");
    expect(readRecent()).toEqual([]);
    // writeRecent 应从空列表重建（不被脏数据卡死）
    expect(writeRecent("😅")).toEqual(["😅"]);
  });
});

describe("符号区完整性（数学 / 货币 / 箭头 / 制表）", () => {
  it("四组齐全且非空", () => {
    expect(SYMBOL_GROUPS.map((g) => g.id)).toEqual(["math", "currency", "arrow", "box"]);
    for (const g of SYMBOL_GROUPS) {
      expect(g.items.length).toBeGreaterThanOrEqual(10);
      for (const item of g.items) {
        expect(item.ch.length).toBeGreaterThan(0);
        expect(item.zh.length).toBeGreaterThan(0);
        expect(item.en.length).toBeGreaterThan(0);
      }
    }
  });

  it("各组关键符号在场", () => {
    const math = SYMBOL_GROUPS.find((g) => g.id === "math")!.items.map((i) => i.ch);
    expect(math).toContain("×");
    expect(math).toContain("÷");
    expect(math).toContain("√");
    const currency = SYMBOL_GROUPS.find((g) => g.id === "currency")!.items.map((i) => i.ch);
    expect(currency).toContain("$");
    expect(currency).toContain("¥");
    expect(currency).toContain("€");
    const arrow = SYMBOL_GROUPS.find((g) => g.id === "arrow")!.items.map((i) => i.ch);
    expect(arrow).toContain("←");
    expect(arrow).toContain("→");
    const box = SYMBOL_GROUPS.find((g) => g.id === "box")!.items.map((i) => i.ch);
    expect(box).toContain("─");
    expect(box).toContain("┼");
  });
});
