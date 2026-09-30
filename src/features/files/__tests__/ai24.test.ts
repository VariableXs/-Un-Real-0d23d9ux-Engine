/**
 * UNREAL-X-15000 · AI-24 数据智能与收官（领域06 · 族0231~0240 · X05751~X06000）门禁用例。
 * V 线四族 100 项（files/ai24Checks.ts）+ C 线六族 150 项（code-analysis fs/ai24.rs 由 cargo test ux_ai24 门禁）。
 */
import { describe, expect, it } from 'vitest';
import { runAi24Checks, checkF0233, checkF0234, checkF0237, checkF0238 } from '../ai24Checks';

describe('AI-24 数据智能与收官 · V 线（族0233/0234/0237/0238）', () => {
  it('各族恰 25 项', () => {
    expect(checkF0233()).toHaveLength(25);
    expect(checkF0234()).toHaveLength(25);
    expect(checkF0237()).toHaveLength(25);
    expect(checkF0238()).toHaveLength(25);
  });

  it('V 线聚合 100 项全绿', () => {
    const { entries, failed } = runAi24Checks();
    expect(entries).toHaveLength(100);
    expect(failed).toEqual([]);
  });

  it('ID 区间正确（X05801~X05850 / X05901~X05950）', () => {
    const { entries } = runAi24Checks();
    const ids = entries.map((e) => e.id);
    expect(new Set(ids).size).toBe(100);
    expect(ids.some((i) => Number(i.slice(1)) === 5801)).toBe(true);
    expect(ids.some((i) => Number(i.slice(1)) === 5950)).toBe(true);
  });
});

describe('AI-24 数据智能与收官 · C 线对齐（code-analysis/core/src/fs/ai24.rs 由 cargo test ux_ai24 门禁）', () => {
  it('V 线 ID 与 C 线族区间零重叠', () => {
    const { entries } = runAi24Checks();
    const cLineRanges = [[5751, 5800], [5851, 5900], [5951, 6000]];
    const vIds = entries.map((e) => Number(e.id.slice(1)));
    for (const range of cLineRanges) {
      const a = range[0] as number;
      const b = range[1] as number;
      expect(vIds.some((x) => x >= a && x <= b)).toBe(false);
    }
  });
});
