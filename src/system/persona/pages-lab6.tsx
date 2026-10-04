/**
 * E 域页组⑪ · 深化实验室六（批次九收官批）：交付物机检三面板。
 */
import { useMemo, useState } from "react";
import { Card, Row, PButton, Notice } from "./ui";
import { auditLabels, extractLabelRefs, auditLabelRefs, findHardcodedCopy, migrationPriority } from "./labels-audit";
import { generateChangelog, generateApiDoc, auditDelivery, standardDeliveryChecks } from "./delivery-docs";
import { estimateTextWidth, fitText, windowTier, layoutForTier, worstCaseCheck } from "./layout-robust";

// ---------- labels 机检面板 ----------

export function LabelsAuditCard(): React.ReactNode {
  const demo = useMemo(() => {
    // labels 双语表喂入（规模：取实际 labels 模块的键数抽样口径——demo 用截断表）。
    const zh = { run: "运行", copy: "复制", tokens: "令牌表", assetTitle: "素材管线" };
    const en = { run: "Run", copy: "Copy", tokens: "Tokens", assetTitle: "Asset pipeline" };
    const labelsAudit = auditLabels(zh, en);
    // 引用完整性：页面源码喂入（demo 含一个坏键）。
    const refs = extractLabelRefs({
      "pages-demo.tsx": [
        '<PButton>{t("run")}</PButton>',
        '<Row label={t("tokens")}>',
        '<Row label={t("missing-key")}>',
      ].join("\n"),
    });
    const refAudit = auditLabelRefs(refs, zh, en);
    // 硬编码字面量迁移清单。
    const hardcoded = findHardcodedCopy({
      "pages-legacy.tsx": [
        '<Row label="手工写的中文标签">',
        '<Card title="手工标题">',
        '<Row label={t("run")}>',
      ].join("\n"),
    });
    return { labelsAudit, refAudit, hardcoded: hardcoded.total, priority: migrationPriority(hardcoded.findings).slice(0, 3) };
  }, []);
  return (
    <Card title="labels 双语机检（十二查 13）">
      <Row label={`键集一致性: zhOnly ${demo.labelsAudit.zhOnly.length} / enOnly ${demo.labelsAudit.enOnly.length} / 空值 ${demo.labelsAudit.emptyValues.length}`} sub={demo.labelsAudit.ok ? "双语键集一致、无空值——发布口径达标" : "键集不齐（漏翻逐条列出）"}>
        <span style={{ color: demo.labelsAudit.ok ? "var(--p-success)" : "var(--p-danger)" }}>{demo.labelsAudit.ok ? "一致" : "有缺口"}</span>
      </Row>
      <Row label={`t() 引用完整性: 检查 ${demo.refAudit.checked} 处`} sub={demo.refAudit.ok ? "全部引用存在（引用不存在的键 = 运行期 undefined 上屏）" : `缺失: ${demo.refAudit.missing.map((m) => m.key).join(",")}`}>
        <span style={{ color: demo.refAudit.ok ? "var(--p-success)" : "var(--p-danger)" }}>{demo.refAudit.ok ? "干净" : "有坏引用"}</span>
      </Row>
      <Row label={`硬编码字面量迁移清单: ${demo.hardcoded} 处`} sub={demo.priority.map((p) => `${p.priority} ×${p.count}: ${p.snippet}`).join(" · ") + "（P1 状态类优先、P2 装饰类——收尾冲刺顺序表）"}>
        <span />
      </Row>
    </Card>
  );
}

// ---------- 交付物面板 ----------

export function DeliveryDocsCard(): React.ReactNode {
  const [tab, setTab] = useState<"changelog" | "api" | "checklist">("changelog");
  const changelog = useMemo(() => generateChangelog(20800), []);
  const api = useMemo(() => generateApiDoc([
    { module: "png-encode", export: "encodePng", signature: "encodePng(bmp: RgbaBitmap): Uint8Array", doc: "RGBA8 → PNG 字节（真彩色+alpha，字节即预算）" },
    { module: "motion-curve", export: "advance", signature: "advance(cur, dtMs, curve, durationMs): InterruptibleState", doc: "可逆动画推进（位置/值分离——打断回退精确归零）" },
    { module: "verdict-report", export: "diffRuns", signature: "diffRuns(prev, curr): DiffResult", doc: "跨轮回归四分法（新红/修复/持续红/稳定）" },
  ]), []);
  const checks = useMemo(() => auditDelivery(standardDeliveryChecks({ tests: 393, lines: 18577, targetLines: 20800 })), []);
  return (
    <Card title="交付物生成器（十九章 19 · 文档与实现一致的机械面）">
      <Row label="文档三件套" sub="CHANGELOG 与接口文档从批次台账直出（手工文档必然腐化——文档即代码产物）">
        <span style={{ display: "flex", gap: 4 }}>
          {(["changelog", "api", "checklist"] as const).map((k) => (
            <PButton key={k} kind={tab === k ? "primary" : "ghost"} onClick={() => setTab(k)}>{k}</PButton>
          ))}
        </span>
      </Row>
      {tab === "changelog" ? (
        <Row label={`CHANGELOG（${changelog.split("\n").length} 行 · 台账直出）`} sub={changelog.split("\n").filter((l) => l.startsWith("### ")).slice(-3).join(" · ")}>
          <span />
        </Row>
      ) : null}
      {tab === "api" ? (
        <Row label="接口文档（签名 + 与代码注释同源的说明）" sub={api.split("\n").filter((l) => l.startsWith("- `")).join(" ")}>
          <span />
        </Row>
      ) : null}
      {tab === "checklist" ? (
        <>
          {checks.table.map((c) => (
            <Row key={c.item} label={c.item} sub={c.where}>
              <span style={{ color: c.present ? "var(--p-success)" : "var(--p-warn)" }}>{c.present ? "✓" : "随闸门"}</span>
            </Row>
          ))}
          <Notice tone={checks.ok ? "ok" : "info"}>{checks.ok ? "交付清单全绿" : `缺 ${checks.missing.length} 项（已登记随闸门补测——诚实不装绿）`}</Notice>
        </>
      ) : null}
    </Card>
  );
}

// ---------- 布局鲁棒性面板 ----------

export function LayoutRobustCard(): React.ReactNode {
  const demo = useMemo(() => {
    const cjk = estimateTextWidth("令牌表设置", 15);
    const latin = estimateTextWidth("Tokens", 15);
    // 最恶劣组合：200% 字体 × 最小窗口 500px 预算。
    const worst = worstCaseCheck("强调色决定全系统的可交互语义色", 15, 500, 2.0);
    const tierCompact = layoutForTier(windowTier(600));
    const tierWide = layoutForTier(windowTier(1600));
    return {
      cjk, latin,
      fitOk: fitText("短标题", 15, 100, false).action,
      fitShrink: fitText("一个相当长的中文标题在这里", 15, 120, false).action,
      fitWrap: fitText("更长的标题允许换行显示完整", 15, 120, true).action,
      fitTruncate: fitText("超长单行标题绝不换行时截断配提示", 15, 100, false).action,
      worst: `${worst.ok ? "成立" : "破版"} · ${worst.effectiveFontPx}px · ${worst.action}`,
      tiers: `600px→${windowTier(600)}(${tierCompact.columns}列/${tierCompact.density}) · 1600px→${windowTier(1600)}(${tierWide.columns}列/${tierWide.density})`,
    };
  }, []);
  return (
    <Card title="布局鲁棒性（七章超小/超大字号）">
      <Row label="文本度量估算（CJK 全角 1.0 / Latin 0.55）" sub={`"令牌表设置"@15px = ${demo.cjk}px · "Tokens"@15px = ${demo.latin}px`}>
        <span />
      </Row>
      <Row label="适配四动作" sub={`放行 ${demo.fitOk} · 缩字号 ${demo.fitShrink} · 换行 ${demo.fitWrap} · 截断+tooltip ${demo.fitTruncate}`}>
        <span />
      </Row>
      <Row label="最恶劣组合（200% 字体 × 最小窗口）" sub={`正文换行兜底 → ${demo.worst}`}>
        <span />
      </Row>
      <Row label="窗口三档断点" sub={demo.tiers}>
        <span />
      </Row>
    </Card>
  );
}
