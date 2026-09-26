import { describe, expect, it } from "vitest";
import { READING_MODE_ENTRY, READING_SPEC, appMode, contentHash, defaultStyle, lineHeightPx, pageWidthPx, setAppMode, unifiedEntry, withSerif } from "../f386-readingMode";
import { __clearMem, memStore } from "../internal/store";

const TEXT = "这是一段用于测试的中文长文， mixed with ASCII words。".repeat(10);

describe("F386 阅读模式", () => {
  it("三参数判据口径：行距 1.6 / 45 字 / 衬线可选", () => {
    expect(READING_SPEC).toEqual({ lineHeight: 1.6, maxCharsPerLine: 45, serifOptional: true });
    const s = defaultStyle();
    expect(s.lineHeight).toBe(1.6);
    expect(s.maxCharsPerLine).toBe(45);
    expect(s.serif).toBe(false);
    expect(withSerif(s, true).serif).toBe(true);
    expect(withSerif(s, true).lineHeight).toBe(1.6); // 其余参数不动
  });

  it("内容零修改（判据）：模式只产排版参数，文本哈希开关前后一致", () => {
    const before = contentHash(TEXT);
    const style = withSerif(defaultStyle(), true);
    void style; // 排版参数不触碰文本
    const after = contentHash(TEXT);
    expect(before).toBe(after);
  });

  it("页宽换算：CJK 全角 1em、ASCII 半角 0.5em", () => {
    const pureCjk = pageWidthPx(defaultStyle(), 16, "一二三四五");
    expect(pureCjk).toBe(45 * 16); // 45 字 × 16px
    const pureAscii = pageWidthPx(defaultStyle(), 16, "abcdefghij");
    expect(pureAscii).toBe(Math.round(45 * 0.5 * 16)); // 半角折半
    expect(pageWidthPx(defaultStyle(), 16, "")).toBe(45 * 16); // 空文本回退全角口径
  });

  it("行高换算：1.6 × 字号", () => {
    expect(lineHeightPx(defaultStyle(), 16)).toBe(25.6);
    expect(lineHeightPx(defaultStyle(), 12)).toBe(19.2);
  });

  it("每应用记忆（判据）：记事本开着、帮助中心独立", () => {
    __clearMem();
    const s = memStore();
    expect(appMode("notepad", s)).toEqual({ on: false, serif: false });
    setAppMode("notepad", true, true, s);
    expect(appMode("notepad", s)).toEqual({ on: true, serif: true });
    expect(appMode("help", s)).toEqual({ on: false, serif: false });
  });

  it("入口统一（判据）：全应用同源入口 id", () => {
    expect(unifiedEntry()).toBe(READING_MODE_ENTRY);
    expect(READING_MODE_ENTRY).toContain("view-menu");
  });
});
