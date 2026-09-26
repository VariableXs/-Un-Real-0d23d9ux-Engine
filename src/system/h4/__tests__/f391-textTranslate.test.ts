import { describe, expect, it } from "vitest";
import { LENGTH_SPLIT_CHARS, detectSourceLang, edgeTranslateUrl, getTargetLang, planTranslate, reuseCard, reuseOutsideClick, setTargetLang } from "../f391-textTranslate";
import { __clearMem, memStore } from "../internal/store";

describe("F391 选中文本翻译", () => {
  it("长短分界：短文本走浮卡、长文本直达标签（判据两路）", () => {
    const short = planTranslate({ text: "hello world", targetLang: "zh", online: true });
    expect(short.route).toBe("card");
    const long = planTranslate({ text: "a".repeat(LENGTH_SPLIT_CHARS + 1), targetLang: "zh", online: true });
    expect(long.route).toBe("browserTab");
    const boundary = planTranslate({ text: "a".repeat(LENGTH_SPLIT_CHARS), targetLang: "zh", online: true });
    expect(boundary.route).toBe("card"); // ≤500 算短
    expect(LENGTH_SPLIT_CHARS).toBe(500);
  });

  it("Edge 跳转参数：选中内容带过去、目标语言参数、URL 编码安全", () => {
    const url = edgeTranslateUrl("你好 世界", "en");
    expect(url).toContain("https://www.bing.com/translator/");
    expect(url).toContain(`text=${encodeURIComponent("你好 世界")}`);
    expect(url).toContain("to=en");
  });

  it("离线诚实提示：不假装会翻（判据）", () => {
    const off = planTranslate({ text: "hello", targetLang: "zh", online: false });
    expect(off.route).toBe("offlineNotice");
    expect(off.offlineMessage).toContain("翻译需要网络");
    expect(off.edgeUrl).toBeNull();
  });

  it("源语言自动检测：中英判向、空文本如实 unknown", () => {
    expect(detectSourceLang("这是一段中文")).toEqual({ lang: "zh", confidence: "high" });
    expect(detectSourceLang("hello world")).toEqual({ lang: "en", confidence: "high" });
    expect(detectSourceLang("   ")).toEqual({ lang: "unknown", confidence: "low" });
  });

  it("目标语言设置可改（判据）：默认 en、空值拒绝", () => {
    __clearMem();
    const s = memStore();
    expect(getTargetLang(s)).toBe("en");
    expect(setTargetLang("ja", s)).toBe(true);
    expect(getTargetLang(s)).toBe("ja");
    expect(setTargetLang("", s)).toBe(false);
  });

  it("浮卡同形制复用 F390（判据）：同一状态机、点外即关", () => {
    const card = reuseCard();
    expect(card.visible).toBe(true);
    expect(card.stealsFocus).toBe(false);
    expect(reuseOutsideClick(card).visible).toBe(false);
  });
});
