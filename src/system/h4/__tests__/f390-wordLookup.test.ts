import { describe, expect, it } from "vitest";
import { CARD_BUDGET_MS, WORDLIST_CAP, loadWordlist, lookup, lookupZh, openCard, outsideClick, pageFor, pushWordlist, saveWordlist, withinCardBudget, type DictionaryPage } from "../f390-wordLookup";
import { __clearMem, memStore } from "../internal/store";

function page(): DictionaryPage {
  return new Map([
    ["serendipity", { word: "serendipity", pos: "n.", gloss: "意外发现珍宝的运气；机缘巧合" }],
    ["ubiquitous", { word: "ubiquitous", pos: "adj.", gloss: "无处不在的" }],
  ]);
}

describe("F390 选中文本查词", () => {
  it("离线查词：命中返回词性+释义、零网络路径（判据）", () => {
    const r = lookup(page(), "Serendipity ");
    expect(r.found).toBe(true);
    expect(r.entry!.gloss).toContain("机缘巧合");
    expect(r.usedNetwork).toBe(false);
  });

  it("未命中如实返回（零编造释义）；空词拒绝", () => {
    expect(lookup(page(), "zzz").found).toBe(false);
    expect(lookup(page(), "zzz").entry).toBeNull();
    expect(lookup(page(), "  ").found).toBe(false);
  });

  it("生词本 20 条：去重提首、容量钳制（判据）", () => {
    let list: string[] = [];
    for (let i = 0; i < WORDLIST_CAP + 5; i++) list = pushWordlist(list, `word${i}`);
    expect(list).toHaveLength(WORDLIST_CAP);
    expect(list[0]).toBe("word24");
    expect(list).not.toContain("word0");
    const bumped = pushWordlist(list, "word10");
    expect(bumped[0]).toBe("word10");
    expect(bumped).toHaveLength(WORDLIST_CAP);
    expect(pushWordlist(list, "  ")).toBe(list); // 空词不入本
  });

  it("生词本持久化 round-trip", () => {
    __clearMem();
    const s = memStore();
    saveWordlist(["a", "b"], s);
    expect(loadWordlist(s)).toEqual(["a", "b"]);
  });

  it("浮卡不抢焦点、点外即关（判据）", () => {
    const card = openCard();
    expect(card.visible).toBe(true);
    expect(card.stealsFocus).toBe(false);
    expect(outsideClick(card).visible).toBe(false);
  });

  it("释义卡时序 <300ms（判据）", () => {
    expect(CARD_BUDGET_MS).toBe(300);
    expect(withinCardBudget(0, 299)).toBe(true);
    expect(withinCardBudget(0, 300)).toBe(false);
  });

  it("词典分页加载：命中页定位、越界如实", () => {
    const p = pageFor("ubiquitous", 4);
    expect(p.inRange).toBe(true);
    expect(p.page).toBeLessThan(4);
    expect(pageFor("ubiquitous", 4).page).toBe(pageFor("ubiquitous", 4).page); // 确定性
    expect(pageFor("", 4).inRange).toBe(false);
  });

  it("双库：汉英向同引擎独立入口（判据「英汉/汉英双库」）", () => {
    const zh: DictionaryPage = new Map([["机缘巧合", { word: "机缘巧合", pos: "n.", gloss: "serendipity" }]]);
    expect(lookupZh(zh, "机缘巧合").entry!.gloss).toBe("serendipity");
  });
});
