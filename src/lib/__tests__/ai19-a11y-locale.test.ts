/**
 * AI-19 无障碍与本地化组单元测试（U-40/U-41/M-73…M-78 纯逻辑部分）。
 * DOM 相关（announce/focus 通告、CVD 滤镜注入）由浏览器人工/aria-audit 覆盖。
 */
import { describe, it, expect } from "vitest";
import { StickyModifiers, withStickyModifiers, CVD_FILTERS } from "../a11y";
import { setS2tUserLexicon, getS2tUserLexicon, s2tLexiconEmpty, applyUserLexicon, s2tFull, s2t, s2tLexiconConflict, S2T_LEXICON_MAX } from "../../i18n/s2t";
import { systemLocale, formatNumberLocale, formatBytesLocale, formatTimeLocale, systemHour12 } from "../localeFormat";

describe("AI-19 a11y：粘滞键序列累积器（M-73/U-40）", () => {
  it("分步 Ctrl→Alt→E 等价 Ctrl+Alt+E", () => {
    const st = new StickyModifiers();
    expect(st.feed("Control")).toBe(true);
    expect(st.feed("Alt")).toBe(true);
    expect(st.feed("KeyE")).toBe(false);
    const mods = st.take({ ctrlKey: false, altKey: false, shiftKey: false, metaKey: false });
    expect([...mods].sort()).toEqual(["alt", "ctrl"]);
    // 取走后清空
    expect(st.current().size).toBe(0);
  });

  it("锁存 ∪ 实时事件修饰键；reset 显式清空", () => {
    const st = new StickyModifiers();
    st.feed("Shift");
    const mods = st.take({ ctrlKey: true, altKey: false, shiftKey: false, metaKey: false });
    expect(mods.has("shift")).toBe(true);
    expect(mods.has("ctrl")).toBe(true);

    st.feed("Control");
    st.reset();
    expect(st.take({ ctrlKey: false, altKey: false, shiftKey: false, metaKey: false }).size).toBe(0);
  });

  it("withStickyModifiers：修饰键喂入返回事件态，普通键返回合并态", () => {
    const st = new StickyModifiers();
    const r1 = withStickyModifiers({ key: "Control", ctrlKey: true } as KeyboardEvent, st);
    expect(r1.ctrl).toBe(true);
    const r2 = withStickyModifiers({ key: "KeyK", ctrlKey: false } as KeyboardEvent, st);
    expect(r2.ctrl).toBe(true); // 来自锁存
    const r3 = withStickyModifiers({ key: "KeyK", ctrlKey: false } as KeyboardEvent, st);
    expect(r3.ctrl).toBe(false); // 锁存已清空
  });

  it("CVD 滤镜四类 + off 均有定义", () => {
    expect(CVD_FILTERS.off).toBe("none");
    expect(CVD_FILTERS.protanopia).toContain("cvd-protanopia");
    expect(CVD_FILTERS.deuteranopia).toContain("cvd-deuteranopia");
    expect(CVD_FILTERS.tritanopia).toContain("cvd-tritanopia");
    expect(CVD_FILTERS.achromatopsia).toBe("grayscale(1)");
  });
});

describe("AI-19 简繁转换用户词表（M-77）", () => {
  it("安装/读取/空判定", () => {
    expect(s2tLexiconEmpty()).toBe(true);
    const n = setS2tUserLexicon({ 内存: "記憶體", " 空 ": "", "": "x" });
    expect(n).toBe(1);
    expect(getS2tUserLexicon()).toEqual({ 内存: "記憶體" });
    expect(s2tLexiconEmpty()).toBe(false);
    setS2tUserLexicon({});
    expect(s2tLexiconEmpty()).toBe(true);
  });

  it("用户词覆盖默认转换（简体键与繁体形式都命中）", () => {
    setS2tUserLexicon({ 内存: "記憶體" });
    expect(s2tFull("查看内存占用")).toBe("查看記憶體占用");
    // 直接对已完成字符级转换的文本生效
    expect(applyUserLexicon("查看記憶體占用")).toBe("查看記憶體占用");
    setS2tUserLexicon({});
  });

  it("专有名词不转换（值=键本身）", () => {
    setS2tUserLexicon({ 软件名: "软件名" });
    expect(s2tFull("软件名文档")).toBe("软件名文檔"); // 键本身保留简体，其余正常转换
    setS2tUserLexicon({});
  });

  it("冲突检测：同键异值返回已有值", () => {
    expect(s2tLexiconConflict("内存", "記憶體", {})).toBeNull();
    expect(s2tLexiconConflict("内存", "記憶體", { 内存: "内存" })).toBe("内存");
  });

  it("上限截断", () => {
    const big: Record<string, string> = {};
    for (let i = 0; i < S2T_LEXICON_MAX + 10; i++) big[`词${i}`] = `詞${i}`;
    expect(setS2tUserLexicon(big)).toBe(S2T_LEXICON_MAX);
    setS2tUserLexicon({});
  });

  it("s2t 基础字符级转换可用", () => {
    expect(s2t("简体")).toContain("簡");
    expect(s2t("设置")).toContain("設");
  });
});

describe("AI-19 区域格式跟随（M-78）", () => {
  it("systemLocale 有兜底", () => {
    expect(typeof systemLocale()).toBe("string");
    expect(systemLocale().length).toBeGreaterThan(0);
  });

  it("数字千分位跟随 locale（en 与 de 分隔符不同）", () => {
    const en = formatNumberLocale(1234567.89, "en-US");
    const de = formatNumberLocale(1234567.89, "de-DE");
    expect(en).toMatch(/1,234,567/);
    expect(de).toMatch(/1\.234\.567/);
  });

  it("容量：二进制 KiB/MiB 与十进制 KB/MB 口径", () => {
    expect(formatBytesLocale(1536, "binary", "en-US")).toBe("1.5 KiB");
    expect(formatBytesLocale(1536, "decimal", "en-US")).toBe("1.54 KB");
    expect(formatBytesLocale(512 * 1024 * 1024, "auto", "en-US")).toContain("MiB");
    expect(formatBytesLocale(-5, "auto")).toBe("0 B");
    expect(formatBytesLocale(42, "auto", "en-US")).toBe("42 B");
  });

  it("时间：强制 12/24 小时制", () => {
    const ts = new Date(2026, 8, 8, 14, 5).getTime();
    const h24 = formatTimeLocale(ts, "en-US", false);
    const h12 = formatTimeLocale(ts, "en-US", true);
    expect(h24).toMatch(/14:05|14:05:00/);
    expect(h12).toMatch(/2:05 PM|02:05 PM/i);
    expect(typeof systemHour12("en-US")).toBe("boolean");
  });

  it("无效 locale 回退不抛异常", () => {
    expect(() => formatNumberLocale(1, "xx-XX")).not.toThrow();
    expect(() => formatTimeLocale(Date.now(), "xx-XX")).not.toThrow();
  });
});
