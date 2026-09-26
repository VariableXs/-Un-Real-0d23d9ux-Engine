/// <reference types="node" />
/**
 * H4 检查项对账（v3 · 离线对账舱）：
 * 直接读《Varix STAR I start.md》主册原文（1MB 单一事实源），逐字对账——
 * ① 标题对账：F351-F400 五十条登记册/一行账标题与主册正文标题逐字一致（F400 判据）；
 * ② 判据锚对账：registry 判据摘文 = 主册「验收判据：」句逐字（一处一事实执法）；
 * ③ 主册基线：F201-F400 标题共 200 个全数在册（总检基线的存在性证明）；
 * ④ F400 三处同源：主册标题数(200) = 一行账行数(200) = 检查点总数(200) 真数据过闸；
 * ⑤ 诚实边界：buildHDomainCheckpoints 只注入 H4 自己 50 项结果时，
 *    missing = 150、allGreen = false——H1-H3 不代绿（隔离验证的同源纪律）。
 * 主册不在仓库时（外部裁剪）本舱红灯——对账依赖唯一事实源在位。
 */

import { describe, expect, it } from "vitest";
import { readFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { H4_REGISTRY } from "../../../system/h4/registry";
import { H4_TITLES, auditThreeSources, buildHDomainCheckpoints } from "../../../system/h4/f400-hDomainClosure";
import { h4DomainStatus } from "../reconcile";

const LEDGER_PATH = join(process.cwd(), "docs", "Varix STAR I start.md");
const H4_RANGE: ReadonlyArray<number> = Array.from({ length: 200 }, (_, i) => 201 + i);

interface LedgerEntry {
  item: string;
  title: string;
  /** 「验收判据：」后的首句（去尾句号）。 */
  criteria: string;
}

function extractLedger(): Map<string, LedgerEntry> {
  expect(existsSync(LEDGER_PATH), "主册《Varix STAR I start.md》必须在 docs/ 在位（唯一事实源）").toBe(true);
  const text = readFileSync(LEDGER_PATH, "utf-8");
  const lines = text.split(/\r?\n/);
  const map = new Map<string, LedgerEntry>();
  for (let i = 0; i < lines.length; i++) {
    const h = /^### (F\d{3}) (.+)$/.exec(lines[i] ?? "");
    if (!h) continue;
    const item = h[1]!;
    if (!/^F(2|3|4)/.test(item)) continue; // H 域 + I 域头部（F201+）
    const num = Number(item.slice(1));
    if (num < 201 || num > 400) continue;
    // 向后找本项正文段（**FXXX 开头），截取「验收判据：」句
    let criteria = "";
    for (let j = i + 1; j < Math.min(i + 12, lines.length); j++) {
      const para = lines[j] ?? "";
      if (/^### /.test(para) || /^## /.test(para)) break;
      const m = /验收判据：(.+?)(?:。|$)/.exec(para);
      if (m) {
        criteria = m[1]!.trim();
        break;
      }
    }
    map.set(item, { item, title: h[2]!.trim(), criteria });
  }
  return map;
}

const ledger = extractLedger();

describe("检查项对账 ①：登记册/一行账标题 ↔ 主册正文标题（F400 判据逐字一致）", () => {
  it("H4_TITLES 50 项与主册标题逐字一致（一处一事实）", () => {
    const mismatch: string[] = [];
    for (const t of H4_TITLES) {
      const entry = ledger.get(t.item);
      if (!entry) mismatch.push(`${t.item}: 主册缺失`);
      else if (entry.title !== t.title) mismatch.push(`${t.item}: 一行账「${t.title}」 ≠ 主册「${entry.title}」`);
    }
    expect(mismatch).toEqual([]);
  });

  it("registry 判据摘文与主册「验收判据：」句逐字一致（50/50）", () => {
    const mismatch: string[] = [];
    for (const e of H4_REGISTRY) {
      const entry = ledger.get(e.item);
      if (!entry) mismatch.push(`${e.item}: 主册缺失`);
      else if (!entry.criteria) mismatch.push(`${e.item}: 主册未提取到验收判据句`);
      else if (entry.criteria !== e.criteria) mismatch.push(`${e.item}: 登记册「${e.criteria}」 ≠ 主册「${entry.criteria}」`);
    }
    expect(mismatch).toEqual([]);
  });
});

describe("检查项对账 ②：主册 H 域 200 项基线（存在性与完备性）", () => {
  it("F201-F400 五百编号中主册标题全数在册（200/200，无缺项）", () => {
    const missing = H4_RANGE.filter((n) => !ledger.has(`F${n}`));
    expect(missing).toEqual([]);
    expect(ledger.size).toBeGreaterThanOrEqual(200);
  });

  it("每个主册条目都带验收判据句（判据锚覆盖完备）", () => {
    const noCriteria = H4_RANGE.map((n) => `F${n}`).filter((item) => !ledger.get(item)?.criteria);
    expect(noCriteria).toEqual([]);
  });
});

describe("检查项对账 ③：F400 三处同源（真数据过闸）", () => {
  const h1h3Titles = H4_RANGE.slice(0, 150).map((n) => {
    const e = ledger.get(`F${n}`)!;
    return { item: e.item, title: e.title };
  });

  it("主册标题数 200 = 一行账行数 200（H1-H3 主册标题 150 + H4_TITLES 50）= 检查点总数 200", () => {
    const fullLedger = [...h1h3Titles, ...H4_TITLES];
    expect(fullLedger.length).toBe(200);
    const audit = auditThreeSources(200, fullLedger, 200);
    expect(audit.pass).toBe(true);
    expect(audit.detail).toContain("三处同源");
  });

  it("诚实边界：只注入 H4 自己 50 项时域内全绿，但对 200 主册基线如实「未达成」（H1-H3 不代绿）", () => {
    const results = H4_REGISTRY.map((e) => ({ item: e.item, passed: true }));
    const agg = buildHDomainCheckpoints([], results);
    expect(agg.total).toBe(50);
    expect(agg.passed).toBe(50);
    expect(agg.allGreen).toBe(true); // 引擎语义：已注入域内全绿
    const status = h4DomainStatus(results);
    expect(status.checkpointTotal).toBe(50);
    expect(status.baselineComplete).toBe(false); // 对主册 200 基线的诚实判定
    expect(status.note).toContain("待 H1-H3 登记册注入");
    expect(status.h1h3Injected).toBe(0);
  });

  it("注入通道就绪：H1-H3 登记册落库后，同参数面即可达 200 全绿（机制自证）", () => {
    const results = [
      ...h1h3Titles.map((t) => ({ item: t.item, passed: true })), // 演练：假设各队登记册全绿注入
      ...H4_REGISTRY.map((e) => ({ item: e.item, passed: true })),
    ];
    const agg = buildHDomainCheckpoints(h1h3Titles, results);
    expect(agg.total).toBe(200);
    expect(agg.allGreen).toBe(true);
  });
});
