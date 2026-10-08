import { describe, expect, it } from "vitest";
import { RESWAP_BUDGET_MS, RTL_LANGS, coverage, diffTable, directionFor, getLang, resolveText, setLang, snapshotSession, verifyStateKept, withinReswapBudget, type Bundle } from "../f398-languageHotSwap";
import { __clearMem, memStore } from "../internal/store";

const BUNDLES: Partial<Record<string, Bundle>> = {
  "zh-CN": { "app.save": "保存", "app.open": "打开", "app.missing-demo": "" },
  en: { "app.save": "Save", "app.open": "Open", "app.missing-demo": "Missing Demo" },
  ja: { "app.save": "保存" },
};

describe("F398 界面语言热切", () => {
  it("词条解析：命中原文；未命中回退英文并标注（判据）", () => {
    expect(resolveText(BUNDLES, "app.save", "zh-CN")).toEqual({ text: "保存", issue: null });
    const r = resolveText(BUNDLES, "app.missing-demo", "ja");
    expect(r.text).toBe("Missing Demo");
    expect(r.issue).toEqual({ key: "app.missing-demo", lang: "ja", fallbackText: "Missing Demo", annotated: true });
  });

  it("词条覆盖率对账（判据）：日语 1/3、中文 3/3", () => {
    expect(coverage(BUNDLES, "zh-CN")).toEqual({ total: 3, translated: 2, pct: 67 });
    expect(coverage(BUNDLES, "en").pct).toBe(100);
    expect(coverage(BUNDLES, "ja").translated).toBe(1);
  });

  it("差异表（F132 联动）：缺译键全列出且带标注", () => {
    const issues = diffTable(BUNDLES, "ja");
    expect(issues.map((i) => i.key)).toEqual(["app.open", "app.missing-demo"]);
    expect(issues.every((i) => i.annotated)).toBe(true);
  });

  it("状态保持（判据）：窗口与草稿哈希逐项一致；丢窗口/改草稿如实报出", () => {
    const before = snapshotSession(["w1", "w2"], { w1: "hash-a", w2: "hash-b" }, "zh-CN");
    const good = snapshotSession(["w1", "w2", "w3"], { w1: "hash-a", w2: "hash-b" }, "en");
    expect(verifyStateKept(before, good).kept).toBe(true);
    const bad = snapshotSession(["w1"], { w1: "hash-a", w2: "hash-changed" }, "en");
    const r = verifyStateKept(before, bad);
    expect(r.kept).toBe(false);
    expect(r.lostWindows).toEqual(["w2"]);
    expect(r.changedDrafts).toEqual(["w2"]);
  });

  it("换装预算 <1 分钟（判据）", () => {
    expect(RESWAP_BUDGET_MS).toBe(60_000);
    expect(withinReswapBudget(0, 59_999)).toBe(true);
    expect(withinReswapBudget(0, 60_000)).toBe(false);
  });

  it("当前语言持久化：默认 zh-CN、空值拒绝", () => {
    __clearMem();
    const s = memStore();
    expect(getLang(s)).toBe("zh-CN");
    expect(setLang("ja", s)).toBe(true);
    expect(getLang(s)).toBe("ja");
    expect(setLang("", s)).toBe(false);
  });

  it("RTL 接口存在性（代码判据）：阿拉伯语 rtl、中文 ltr", () => {
    expect(RTL_LANGS.has("ar")).toBe(true);
    expect(directionFor("ar-SA")).toBe("rtl");
    expect(directionFor("zh-CN")).toBe("ltr");
    expect(directionFor("he")).toBe("rtl");
  });
});
