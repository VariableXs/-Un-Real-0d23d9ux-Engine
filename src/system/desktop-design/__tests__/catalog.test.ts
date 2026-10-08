/**
 * AURORA-10000 · AI-11~AI-15 车道 · 目录完整性测试。
 * 红线：F01251~F01875 唯一、连续、无跳号；每族恰 25 项；
 * 与 docs/AURORA-10000-功能全景图.md 的行号逐一对应（行首 "- F0xxxx "，?raw 注入）。
 */
import panoramaRaw from "../../../../docs/AURORA-10000-功能全景图.md?raw";
import splitRaw from "../../../../docs/AURORA-10000-AI分工完成图.md?raw";
import { describe, expect, it } from "vitest";
import { FAMILIES, ALL_ENTRIES, LANE } from "../catalog";

describe("AURORA-10000 领域03 目录（AI-11~AI-15）", () => {
  it("共 25 族 × 25 项 = 625", () => {
    expect(FAMILIES).toHaveLength(25);
    for (const f of FAMILIES) expect(f.entries).toHaveLength(25);
    expect(ALL_ENTRIES).toHaveLength(625);
  });

  it("ID 唯一、连续、覆盖 F01251~F01875", () => {
    const ids = ALL_ENTRIES.map((e) => parseInt(e.id.slice(1), 10));
    expect(new Set(ids).size).toBe(625);
    for (let i = 1251; i <= 1875; i++) expect(ids).toContain(i);
    expect(Math.min(...ids)).toBe(LANE.range[0]);
    expect(Math.max(...ids)).toBe(LANE.range[1]);
  });

  it("族区间与条目一一对应", () => {
    for (const f of FAMILIES) {
      for (const e of f.entries) {
        const n = parseInt(e.id.slice(1), 10);
        expect(n).toBeGreaterThanOrEqual(f.range[0]);
        expect(n).toBeLessThanOrEqual(f.range[1]);
        expect(e.family).toBe(f.id);
      }
    }
  });

  it("每条目双语标签齐全、kind 合法", () => {
    for (const e of ALL_ENTRIES) {
      expect(e.label.zh.length).toBeGreaterThan(0);
      expect(e.label.en.length).toBeGreaterThan(0);
      expect(["preset", "switch", "reserved"]).toContain(e.kind);
    }
  });

  it("与全景图文档逐条对齐", () => {
    const docLines = new Map<string, string>();
    for (const m of panoramaRaw.matchAll(/^- (F\d{5}) (.+)$/gm)) docLines.set(m[1] ?? "", m[2] ?? "");
    for (const e of ALL_ENTRIES) {
      expect(docLines.has(e.id), `${e.id} 不在全景图`).toBe(true);
      const name = e.label.zh.split(" — ")[0]?.split(" ·")[0]?.replace(/\s*（.*）\s*$/, "") ?? "";
      expect(docLines.get(e.id) ?? "", `${e.id} 名称漂移：${name}`).toContain(name.slice(0, 2));
    }
  });

  it("与分工完成图区间一致（AI-11~AI-15 → F01251~F01875）", () => {
    expect(splitRaw).toContain("### AI-11 ");
    expect(splitRaw).toContain("### AI-15 ");
    expect(splitRaw).toContain("F01251~F01375");
    expect(splitRaw).toContain("F01751~F01875");
  });
});
