import { describe, expect, it } from "vitest";
import { __clearMem, memStore } from "../internal/store";
import { H4_TITLES, auditLedgerMatchesTitles, auditThreeSources, buildHDomainCheckpoints, generateOneLineLedger, loadLedger, persistLedger, quarterlyScopeRevision } from "../f400-hDomainClosure";

const H1TOH3 = Array.from({ length: 150 }, (_, i) => ({ item: `F${201 + i}`, title: `占位-${i}` }));

describe("F400 H 域收官登记", () => {
  it("H4 登记册：50 项、编号连续 F351-F400（一处一事实）", () => {
    expect(H4_TITLES).toHaveLength(50);
    expect(H4_TITLES[0]!.item).toBe("F351");
    expect(H4_TITLES[49]!.item).toBe("F400");
    H4_TITLES.forEach((t, i) => expect(t.item).toBe(`F${351 + i}`));
  });

  it("一行账生成与 100% 一致校验（判据）", () => {
    const ledger = generateOneLineLedger(H4_TITLES);
    expect(ledger).toHaveLength(50);
    expect(auditLedgerMatchesTitles(ledger, H4_TITLES)).toEqual({ pass: true, mismatch: [] });
    const tampered = ledger.map((l, i) => (i === 3 ? { ...l, title: "被改的标题" } : l));
    const a = auditLedgerMatchesTitles(tampered, H4_TITLES);
    expect(a.pass).toBe(false);
    expect(a.mismatch[0]).toContain("F354");
  });

  it("总检检查点：200 项基座（H1-H3 注入 150 + H4 内置 50）（判据）", () => {
    const results = [...H1TOH3, ...H4_TITLES].map((t) => ({ item: t.item, passed: true }));
    const r = buildHDomainCheckpoints(H1TOH3, results);
    expect(r.total).toBe(200);
    expect(r.allGreen).toBe(true);
    const oneRed = buildHDomainCheckpoints(H1TOH3, results.slice(0, 199).map((x, i) => (i === 100 ? { ...x, passed: false } : x)));
    expect(oneRed.allGreen).toBe(false);
    const incomplete = buildHDomainCheckpoints(H1TOH3, results.slice(0, 150));
    expect(incomplete.missing).toHaveLength(50);
    expect(incomplete.missing[0]).toBe("F351");
  });

  it("三处同源审计（判据）：200=200=200 才过", () => {
    const fullLedger = Array.from({ length: 200 }, (_, i) => ({ item: `x${i}`, title: "t" }));
    expect(auditThreeSources(200, fullLedger, 200).pass).toBe(true);
    const bad = auditThreeSources(199, fullLedger, 200);
    expect(bad.pass).toBe(false);
    expect(bad.detail).toContain("不一致");
  });

  it("F200 条款修订登记：季度审视范围扩至 F001-F400+I 域（判据）", () => {
    const rev = quarterlyScopeRevision("2026-09-26");
    expect(rev.scope).toContain("F001-F400");
    expect(rev.scope).toContain("I 域");
    expect(rev.clauseRevision).toBe("F200-r2");
  });

  it("一行账快照持久化 round-trip", () => {
    __clearMem();
    const s = memStore();
    const ledger = generateOneLineLedger(H4_TITLES);
    expect(persistLedger(ledger, s)).toBe(true);
    expect(loadLedger(s)).toEqual(ledger);
  });
});
