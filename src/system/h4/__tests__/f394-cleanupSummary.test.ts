import { describe, expect, it } from "vitest";
import { CLEANUP_SOURCES, auditSourceReconciliation, buildLedger, defaultChecked, execute, requiresConfirmation, summary, visualClass, type CleanupItem } from "../f394-cleanupSummary";

function item(id: string, source: CleanupItem["source"], bytes: number, irreversible = false): Omit<CleanupItem, "checked"> {
  return { id, source, title: id, reclaimableBytes: bytes, irreversible, costNote: irreversible ? "清了要重下" : null };
}

const RAW = [
  item("rb1", "recycleBin", 1000),
  item("dup1", "duplicates", 2000),
  item("snap1", "oldSnapshots", 4000, true),
  item("cache1", "appCaches", 500, true),
];

describe("F394 清理建议收口页", () => {
  it("四来源汇总对账：页面数字与源头逐项一致（判据）", () => {
    expect(CLEANUP_SOURCES).toEqual(["recycleBin", "duplicates", "oldSnapshots", "appCaches"]);
    const ledger = buildLedger(RAW.map((r) => ({ ...r, checked: false })));
    const a = auditSourceReconciliation(ledger.items, { recycleBin: 1000, duplicates: 2000, oldSnapshots: 4000, appCaches: 500 });
    expect(a.pass).toBe(true);
    const bad = auditSourceReconciliation(ledger.items, { recycleBin: 999, duplicates: 2000, oldSnapshots: 4000, appCaches: 500 });
    expect(bad.pass).toBe(false);
    expect(bad.mismatches[0]).toContain("recycleBin");
  });

  it("总账准确：勾选项之和 + 不可逆单独列示（判据）", () => {
    const ledger = buildLedger([
      { ...RAW[0]!, checked: true },
      { ...RAW[2]!, checked: true },
    ]);
    const s = summary(ledger);
    expect(s.totalBytes).toBe(5000);
    expect(s.checkedCount).toBe(2);
    expect(s.irreversibleChecked).toEqual([{ id: "snap1", costNote: "清了要重下" }]);
  });

  it("不可逆项必须带代价说明；缺说明在总账里显式暴露（零静默）", () => {
    const ledger = buildLedger([{ ...RAW[2]!, checked: true, costNote: null }]);
    expect(summary(ledger).irreversibleChecked[0]!.costNote).toContain("缺代价说明");
  });

  it("不可逆项二次确认（判据）：勾了不可逆必须确认才可执行", () => {
    expect(requiresConfirmation(buildLedger([{ ...RAW[0]!, checked: true }]))).toBe(false);
    expect(requiresConfirmation(buildLedger([{ ...RAW[3]!, checked: true }]))).toBe(true);
  });

  it("执行回收率 ≥90% 判据", () => {
    const ledger = buildLedger([
      { ...RAW[0]!, checked: true },
      { ...RAW[1]!, checked: true },
    ]);
    const good = execute(ledger, { rb1: 950, dup1: 2000 });
    expect(good.pass).toBe(true);
    expect(good.recoveryRate).toBeGreaterThanOrEqual(0.9);
    const bad = execute(ledger, { rb1: 500, dup1: 500 });
    expect(bad.pass).toBe(false);
    expect(execute(buildLedger([]), {}).pass).toBe(true); // 零勾选视为平凡通过
  });

  it("默认勾选策略与分色标注：可逆默认勾（safe）、不可逆默认不勾（caution）", () => {
    const safe = defaultChecked(item("rb", "recycleBin", 1));
    const caution = defaultChecked(item("snap", "oldSnapshots", 1, true));
    expect(safe.checked).toBe(true);
    expect(visualClass(safe)).toBe("safe");
    expect(caution.checked).toBe(false);
    expect(visualClass(caution)).toBe("caution");
  });
});
