/**
 * UNREAL-X-15000 · AI-29 硬件体验面（领域08 · 族0281~0290 · X07001~X07250）门禁用例。
 * V 线八族 200 项（hardware/ai29Checks.ts）+ K 线两族 50 项（kernel ai29k）。
 */
import { describe, expect, it } from 'vitest';
import { runAi29Checks, checkF0281, checkF0285, checkF0289 } from '../ai29Checks';

describe('AI-29 硬件体验面 · V 线（族0281~0285/0287~0289）', () => {
  it('抽查族恰 25 项', () => {
    expect(checkF0281()).toHaveLength(25);
    expect(checkF0285()).toHaveLength(25);
    expect(checkF0289()).toHaveLength(25);
  });

  it('聚合 200 项全绿', () => {
    const { entries, failed } = runAi29Checks();
    expect(entries).toHaveLength(200);
    expect(failed).toEqual([]);
  });

  it('ID 区间断点无缝（X07001~X07125 + X07151~X07225）', () => {
    const { entries } = runAi29Checks();
    const ids = entries.map((e) => Number(e.id.slice(1))).sort((a, b) => a - b);
    expect(new Set(ids).size).toBe(200);
    expect(ids[0]).toBe(7001);
    expect(ids[124]).toBe(7125);
    expect(ids[125]).toBe(7151);
    expect(ids[199]).toBe(7225);
    for (let i = 1; i <= 124; i++) expect(ids[i]! - ids[i - 1]!).toBe(1);
    for (let i = 126; i < ids.length; i++) expect(ids[i]! - ids[i - 1]!).toBe(1);
  });
});
