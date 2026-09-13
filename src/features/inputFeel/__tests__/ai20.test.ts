/**
 * UNREAL-X-15000 · AI-20 输入工程与中文（领域05 · 族0191~0200 · X04751~X05000）门禁用例。
 * V 线四族 100 项（inputFeel/ai20Checks.ts）+ 收官聚合 250 项口径（V 100 + C 150）。
 */
import { describe, expect, it } from 'vitest';
import { runAi20Checks, checkF0191, checkF0196, checkF0197, checkF0198 } from '../ai20Checks';

describe('AI-20 输入工程与中文 · V 线（族0191/0196/0197/0198）', () => {
  it('各族恰 25 项', () => {
    expect(checkF0191()).toHaveLength(25);
    expect(checkF0196()).toHaveLength(25);
    expect(checkF0197()).toHaveLength(25);
    expect(checkF0198()).toHaveLength(25);
  });

  it('V 线聚合 100 项全绿', () => {
    const { entries, failed } = runAi20Checks();
    expect(entries).toHaveLength(100);
    expect(failed).toEqual([]);
  });

  it('ID 区间正确（X04751~X04775 / X04876~X04950）', () => {
    const { entries } = runAi20Checks();
    const ids = entries.map((e) => e.id);
    for (const id of ids.filter((i) => i >= 'X04751' && i <= 'X04775')) {
      expect(Number(id.slice(1))).toBeGreaterThanOrEqual(4751);
      expect(Number(id.slice(1))).toBeLessThanOrEqual(4775);
    }
    expect(ids.some((i) => Number(i.slice(1)) === 4751)).toBe(true);
    expect(ids.some((i) => Number(i.slice(1)) === 4950)).toBe(true);
    expect(new Set(ids).size).toBe(100);
  });
});
describe('AI-20 输入工程与中文 · C 线对齐（code-analysis/core/src/input/ai20.rs 由 cargo test ux_ai20 门禁）', () => {
  it('V 线收官 ID 与 C 线族区间零重叠', () => {
    const { entries } = runAi20Checks();
    const cLineRanges = [[4776, 4800], [4801, 4825], [4826, 4850], [4851, 4875], [4951, 4975], [4976, 5000]];
    const vIds = entries.map((e) => Number(e.id.slice(1)));
    for (const range of cLineRanges) {
      const a = range[0] as number;
      const b = range[1] as number;
      expect(vIds.some((n) => n >= a && n <= b)).toBe(false);
    }
  });
});
