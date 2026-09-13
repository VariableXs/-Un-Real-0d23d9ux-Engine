/**
 * UNREAL-X-15000 · AI-30 系统服务面（领域08 · 族0291~0300 · X07251~X07500）门禁用例。
 * V 线十族 250 项（hardware/ai30Checks.ts）。
 */
import { describe, expect, it } from 'vitest';
import { runAi30Checks, checkF0291, checkF0295, checkF0300 } from '../ai30Checks';

describe('AI-30 系统服务面 · V 线（族0291~0300）', () => {
  it('抽查族恰 25 项', () => {
    expect(checkF0291()).toHaveLength(25);
    expect(checkF0295()).toHaveLength(25);
    expect(checkF0300()).toHaveLength(25);
  });

  it('聚合 250 项全绿', () => {
    const { entries, failed } = runAi30Checks();
    expect(entries).toHaveLength(250);
    expect(failed).toEqual([]);
  });

  it('ID 区间连续无缝（X07251~X07500）', () => {
    const { entries } = runAi30Checks();
    const ids = entries.map((e) => Number(e.id.slice(1))).sort((a, b) => a - b);
    expect(ids[0]).toBe(7251);
    expect(ids[249]).toBe(7500);
    expect(new Set(ids).size).toBe(250);
    for (let i = 1; i < ids.length; i++) expect(ids[i]! - ids[i - 1]!).toBe(1);
  });
});
