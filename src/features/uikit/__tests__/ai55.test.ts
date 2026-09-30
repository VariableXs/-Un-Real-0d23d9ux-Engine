/**
 * UNREAL-X-15000 · AI-55 主题流水线与视觉回归（领域15 · V 线 6 族 · X13526~X13550/X13551~X13575/
 * X13626~X13650/X13651~X13675/X13676~X13700/X13726~X13750）门禁用例。
 * C 线 4 族（族0541/0544/0545/0549 · 100 项）见 code-analysis/core/src/ai55.rs。
 */
import { describe, expect, it } from 'vitest';
import {
  runAi55Checks,
  checkF0542, checkF0543, checkF0546, checkF0547, checkF0548, checkF0550,
} from '../ai55Checks';

describe('AI-55 主题流水线与视觉回归 · V 线（族0542/0543/0546~0548/0550）', () => {
  it('各族恰 25 项', () => {
    expect(checkF0542()).toHaveLength(25);
    expect(checkF0543()).toHaveLength(25);
    expect(checkF0546()).toHaveLength(25);
    expect(checkF0547()).toHaveLength(25);
    expect(checkF0548()).toHaveLength(25);
    expect(checkF0550()).toHaveLength(25);
  });

  it('聚合 150 项全绿', () => {
    const { entries, failed } = runAi55Checks();
    expect(entries).toHaveLength(150);
    expect(failed).toEqual([]);
  });

  it('V 线 ID 区间正确且无重（六族：X13526~13575 · X13626~13700 · X13726~13750）', () => {
    const { entries } = runAi55Checks();
    const nums = entries.map((e) => Number(e.id.slice(1)));
    expect(new Set(nums).size).toBe(150);
    const ranges: Array<[number, number]> = [
      [13526, 13550], [13551, 13575], [13626, 13650], [13651, 13675], [13676, 13700], [13726, 13750],
    ];
    for (const [lo, hi] of ranges) {
      const inRange = nums.filter((n) => n >= lo && n <= hi);
      expect(inRange).toHaveLength(25);
      inRange.sort((a, b) => a - b);
      for (let i = 1; i < inRange.length; i++) {
        expect(inRange[i]).toBe((inRange[i - 1] as number) + 1);
      }
    }
  });
});
