import { describe, expect, it } from "vitest";
import { auditLabels, extractLabelRefs, auditLabelRefs, findHardcodedCopy, migrationPriority } from "../labels-audit";
import { BATCH_LEDGER, generateChangelog, generateApiDoc, auditDelivery, standardDeliveryChecks } from "../delivery-docs";
import { estimateTextWidth, fitText, windowTier, layoutForTier, worstCaseCheck } from "../layout-robust";

// ---------- labels-audit ----------

describe("labels-audit · 双语机检（十二查 13 深化）", () => {
  const zh = { a: "甲", b: "乙", c: "丙" };
  const en = { a: "A", b: "B" };

  it("键集一致性：zhOnly/enOnly 双向列出（漏翻逐条）", () => {
    const r = auditLabels(zh, en);
    expect(r.zhOnly).toEqual(["c"]);
    expect(r.enOnly).toEqual([]);
    expect(r.ok).toBe(false);
    expect(auditLabels({ a: "甲" }, { a: "A" }).ok).toBe(true);
  });

  it("空值键 = 未翻（占位符即缺陷）", () => {
    const r = auditLabels({ a: "甲", b: "  " }, { a: "A", b: "B" });
    expect(r.emptyValues).toEqual(["b"]);
  });

  it("t() 引用完整性：缺键逐条定位到文件行号", () => {
    const refs = extractLabelRefs({
      "p.tsx": '<Row label={t("a")}>\n<Row label={t("ghost")}>',
    });
    expect(refs).toHaveLength(2);
    const r = auditLabelRefs(refs, zh, en);
    expect(r.ok).toBe(false);
    expect(r.missing[0]!.key).toBe("ghost");
    expect(r.missing[0]!.line).toBe(2);
  });

  it("硬编码字面量：中文字面量属性逐条带槽位分类、优先级排序", () => {
    const { findings, total } = findHardcodedCopy({
      "p.tsx": '<Row label="手工标签">\n<Card title="手工标题">',
    });
    expect(total).toBe(2);
    expect(findings[0]!.slot).toBe("label");
    const prio = migrationPriority(findings);
    expect(prio[0]!.priority).toBe("P1"); // 状态类优先。
    expect(findHardcodedCopy({ "clean.tsx": '<Row label={t("a")}>' }).total).toBe(0);
  });
});

// ---------- delivery-docs ----------

describe("delivery-docs · 交付物生成器（十九章 19 深化）", () => {
  it("台账七批齐全（v1-v8 结构化数据源）", () => {
    expect(BATCH_LEDGER).toHaveLength(7);
    expect(BATCH_LEDGER[BATCH_LEDGER.length - 1]!.cumulativeLines).toBe(18577);
  });

  it("CHANGELOG 从台账直出：批次倒序 + 测试/行数逐批入账", () => {
    const md = generateChangelog(20800);
    expect(md).toContain("## [Unreleased]");
    expect(md).toContain("### v8");
    expect(md).toContain("89%"); // 18577/20800 = 89.3% → 89。
    // 倒序：v8 在 v3 之前（v1-v2 是合并批次标题）。
    expect(md.indexOf("### v8")).toBeLessThan(md.indexOf("### v3"));
  });

  it("接口文档按模块分组（签名+说明——与实现同步的文档面）", () => {
    const md = generateApiDoc([
      { module: "m1", export: "f", signature: "f(x): y", doc: "做 y" },
      { module: "m2", export: "g", signature: "g(): void", doc: "做无" },
    ]);
    expect(md).toContain("## m1");
    expect(md).toContain("`f(x): y` — 做 y");
  });

  it("交付清单核验：真机走查项显性『随闸门』不装绿（诚实不包装）", () => {
    const checks = standardDeliveryChecks({ tests: 393, lines: 18577, targetLines: 20800 });
    const a = auditDelivery(checks);
    expect(a.ok).toBe(false); // 真机走查未做——不装绿。
    expect(a.missing).toEqual(["真机走查（四档 DPI——随闸门补测登记）"]);
    expect(checks.find((c) => c.item.includes("隔离验证"))!.present).toBe(true);
  });
});

// ---------- layout-robust ----------

describe("layout-robust · 布局鲁棒性（七章超小/超大字号深化）", () => {
  it("文本度量：CJK 全角 1.0 / Latin 0.55（确定性估算）", () => {
    expect(estimateTextWidth("令牌", 15)).toBe(30); // 2 全角。
    expect(estimateTextWidth("Tokens", 15)).toBe(Math.ceil(6 * 0.55 * 15)); // 50。
  });

  it("适配四动作：放行/缩字号/换行/截断+tooltip（信息不丢纪律）", () => {
    expect(fitText("短", 15, 100, false).action).toBe("fit");
    expect(fitText("长标题测试数据八", 15, 120, false).action).toBe("shrink-font"); // 15px 放不下（122>120）、12px 放得下（98≤120）。
    expect(fitText("允许换行的长标题内容很多很多", 15, 100, true).action).toBe("wrap");
    const t = fitText("绝不换行的超长单行标题测试用例数据", 15, 80, false);
    expect(t.action).toBe("truncate-tooltip"); // 截断必须配 tooltip。
  });

  it("窗口三档断点：compact/standard/expansive 布局逐档成立", () => {
    expect(windowTier(600)).toBe("compact");
    expect(windowTier(900)).toBe("standard");
    expect(windowTier(1600)).toBe("expansive");
    expect(layoutForTier("compact").sidePanel).toBe(false);
    expect(layoutForTier("expansive").columns).toBe(3);
  });

  it("最恶劣组合：200% 字体 × 最小窗口——正文换行兜底必须成立", () => {
    const w = worstCaseCheck("中文长标题内容示例数据", 15, 400, 2.0);
    expect(w.effectiveFontPx).toBe(30);
    expect(w.ok).toBe(true); // 允许换行 → 恶劣组合不破版。
  });
});
