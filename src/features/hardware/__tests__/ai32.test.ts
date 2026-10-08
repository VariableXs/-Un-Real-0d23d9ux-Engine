/**
 * UNREAL-X-15000 · AI-32 设备场景与收官（领域08 · 族0311~0320 · X07751~X08000）门禁用例。
 * 10 族 250 项：V 线（族0311~0314/0318）+ C 线对齐（族0315~0317）+ 三方（族0319~0320）。
 */
import { describe, expect, it } from 'vitest';
import {
  runAi32Checks,
  checkF0311, checkF0312, checkF0313, checkF0314, checkF0315,
  checkF0316, checkF0317, checkF0318, checkF0319, checkF0320,
} from '../ai32Checks';

describe('AI-32 设备场景与收官 · 全线（族0311~0320）', () => {
  it('各族恰 25 项', () => {
    expect(checkF0311()).toHaveLength(25);
    expect(checkF0312()).toHaveLength(25);
    expect(checkF0313()).toHaveLength(25);
    expect(checkF0314()).toHaveLength(25);
    expect(checkF0315()).toHaveLength(25);
    expect(checkF0316()).toHaveLength(25);
    expect(checkF0317()).toHaveLength(25);
    expect(checkF0318()).toHaveLength(25);
    expect(checkF0319()).toHaveLength(25);
    expect(checkF0320()).toHaveLength(25);
  });

  it('聚合 250 项全绿', () => {
    const { entries, failed } = runAi32Checks();
    expect(entries).toHaveLength(250);
    expect(failed).toEqual([]);
  });

  it('ID 区间连续无重（X07751~X08000）', () => {
    const { entries } = runAi32Checks();
    const nums = entries.map((e) => Number(e.id.slice(1))).sort((a, b) => a - b);
    expect(new Set(nums).size).toBe(250);
    expect(nums[0]).toBe(7751);
    expect(nums[249]).toBe(8000);
    for (let i = 1; i < nums.length; i++) {
      expect(nums[i]).toBe((nums[i - 1] as number) + 1);
    }
  });
});
