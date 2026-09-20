/**
 * i18n 词条拆分后的契约测试。
 *
 * 为什么必须有这个测试：英文词条（3122 行）是从 dictionaries.ts **机械切分**出去的
 * （脚本 `_attic/tools/split-i18n-en.py`）。机械操作最怕两件事——切漏了、切坏了。
 * 而语言包出问题的表现很隐蔽：不是报错，是界面上某个词 suddenly 变成 key 原文
 * 或者空字符串。这类问题靠肉眼验收根本扫不完（161+ 词条 × 3 语言）。
 *
 * 这里钉死三条不可退让的契约：
 *   1. **完整性**：en 的词条键集合必须与 zh 完全一致（拆分没丢、没多）
 *   2. **回退**：en 未加载时 translate 必须回退到中文，**绝不返回 key 或空串**
 *   3. **可用性**：ensureEnDict 之后英文真正生效，且幂等
 */
import { describe, expect, it } from "vitest";
import { dictionaries, ensureEnDict, isEnReady, translate } from "../dictionaries";
import { en } from "../dict-en";

describe("i18n · 拆分完整性", () => {
  it("英文词条键集合与中文完全一致（机械拆分没丢项、没多项）", () => {
    const zhKeys = Object.keys(dictionaries.zh).sort();
    const enKeys = Object.keys(en).sort();
    expect(zhKeys.length).toBeGreaterThan(100); // 规模 sanity check
    expect(enKeys).toEqual(zhKeys);
  });

  it("缺失与多余都能被定位（按名指出，便于修复）", () => {
    const zhKeys = new Set(Object.keys(dictionaries.zh));
    const enKeys = new Set(Object.keys(en));
    const missing = [...zhKeys].filter((k) => !enKeys.has(k));
    const extra = [...enKeys].filter((k) => !zhKeys.has(k));
    expect(missing).toEqual([]);
    expect(extra).toEqual([]);
  });

  it("每个英文词条都有非空内容（空串会让界面出现空白按钮）", () => {
    const empties = Object.entries(en)
      .filter(([, v]) => typeof v !== "string" || v.trim() === "")
      .map(([k]) => k);
    expect(empties).toEqual([]);
  });

  it("中文与繁体是静态内置的（首屏无需等待任何异步加载）", () => {
    expect(Object.keys(dictionaries.zh).length).toBeGreaterThan(0);
    expect(Object.keys(dictionaries["zh-TW"]).length).toBeGreaterThan(0);
  });
});

describe("i18n · 英文按需加载的回退行为", () => {
  it("字典导出结构保持三语言键位（删键会让 dictionaries[lang] 变 undefined）", () => {
    expect(Object.keys(dictionaries).sort()).toEqual(["en", "zh", "zh-TW"]);
  });

  it("中文/繁体直接可用，不依赖任何加载", () => {
    expect(translate("zh", "appName")).toBe("Variable");
    expect(translate("zh-TW", "appName")).toBe("Variable");
  });

  it("英文未加载时回退到中文，绝不返回 key 原文或空串", () => {
    // 取一个确定存在的键，模拟「用户选了英文但词条还没到」的窗口期。
    const key = "appName";
    const s = translate("en", key);
    expect(s).not.toBe(key); // 不能把 key 原文显示给用户
    expect(s.trim()).not.toBe(""); // 不能是空白
    expect(s).toBe(translate("zh", key)); // 应当就是中文值
  });

  it("ensureEnDict 之后英文真正生效", async () => {
    await ensureEnDict();
    expect(isEnReady()).toBe(true);
    // appSubtitle 中英不同，能真验证「拿到的是英文而不是回退的中文」。
    const zhVal = translate("zh", "appSubtitle");
    const enVal = translate("en", "appSubtitle");
    expect(zhVal).not.toBe(enVal);
    expect(enVal).toBe(en.appSubtitle);
  });

  it("ensureEnDict 幂等（重复调用不报错、结果稳定）", async () => {
    await ensureEnDict();
    const first = translate("en", "appName");
    await ensureEnDict();
    expect(translate("en", "appName")).toBe(first);
  });
});

describe("i18n · 参数插值与伪本地化未被拆分破坏", () => {
  it("带参数的词条仍能正确插值", async () => {
    await ensureEnDict();
    // 找一个含 {n} 占位的词条（perfSamples）
    const out = translate("en", "perfSamples", { n: 42 });
    expect(out).toContain("42");
    expect(out).not.toContain("{n}");
  });

  it("未知 key 返回 key 本身（保持既有行为，不静默变空）", () => {
    expect(translate("zh", "__definitely_missing_key__")).toBe("__definitely_missing_key__");
  });
});
