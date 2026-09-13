// UNREAL-X AI-17/AI-18：输入手感面 + 输入智能（族0161~0180 · X04001~X04500）测试。
// 断言组全绿 + 每族非空 + ID 口径（X04001~X04500 连续段）。

import { describe, expect, it } from "vitest";
import { runAi17Checks, runAi18Checks } from "../checks";

describe("UNREAL-X AI-17 输入手感面（族0161~0170）", () => {
  const r = runAi17Checks();

  it("断言组全绿", () => {
    expect(r.failed).toEqual([]);
  });

  it("断言条数 ≥ 50（每族 ≥5）", () => {
    expect(r.entries.length).toBeGreaterThanOrEqual(50);
  });

  it("ID 落在 X04001~X04250", () => {
    for (const e of r.entries) {
      const n = Number(e.id.slice(1));
      expect(n).toBeGreaterThanOrEqual(4001);
      expect(n).toBeLessThanOrEqual(4250);
    }
  });

  it("20 族每个 X 段至少命中一条", () => {
    const hits = new Set(r.entries.map((e) => Math.floor((Number(e.id.slice(1)) - 1) / 25)));
    for (let fam = 160; fam <= 169; fam++) expect(hits.has(fam)).toBe(true);
  });
});

describe("UNREAL-X AI-18 输入智能（族0171~0180）", () => {
  const r = runAi18Checks();

  it("断言组全绿", () => {
    expect(r.failed).toEqual([]);
  });

  it("断言条数 ≥ 50（每族 ≥5）", () => {
    expect(r.entries.length).toBeGreaterThanOrEqual(50);
  });

  it("ID 落在 X04251~X04500", () => {
    for (const e of r.entries) {
      const n = Number(e.id.slice(1));
      expect(n).toBeGreaterThanOrEqual(4251);
      expect(n).toBeLessThanOrEqual(4500);
    }
  });

  it("20 族每个 X 段至少命中一条", () => {
    const hits = new Set(r.entries.map((e) => Math.floor((Number(e.id.slice(1)) - 1) / 25)));
    for (let fam = 170; fam <= 179; fam++) expect(hits.has(fam)).toBe(true);
  });
});
