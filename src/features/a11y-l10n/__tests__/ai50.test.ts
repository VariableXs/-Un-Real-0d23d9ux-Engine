/**
 * UNREAL-X-15000 · AI-50 无障碍五域（领域14 · 族0491~0500 · X12251~X12500）门禁用例。
 * 10 族 250 项：V 线（族0491~0499）+ C 线（族0500）。
 */
import { describe, expect, it } from 'vitest';
import {
  runAi50Checks,
  checkF0491, checkF0492, checkF0493, checkF0494, checkF0495,
  checkF0496, checkF0497, checkF0498, checkF0499, checkF0500,
} from '../ai50Checks';

describe('AI-50 无障碍五域 · 全线（族0491~0500）', () => {
  it('各族恰 25 项', () => {
    expect(checkF0491()).toHaveLength(25);
    expect(checkF0492()).toHaveLength(25);
    expect(checkF0493()).toHaveLength(25);
    expect(checkF0494()).toHaveLength(25);
    expect(checkF0495()).toHaveLength(25);
    expect(checkF0496()).toHaveLength(25);
    expect(checkF0497()).toHaveLength(25);
    expect(checkF0498()).toHaveLength(25);
    expect(checkF0499()).toHaveLength(25);
    expect(checkF0500()).toHaveLength(25);
  });

  it('聚合 250 项全绿', () => {
    const { entries, failed } = runAi50Checks();
    expect(entries).toHaveLength(250);
    expect(failed).toEqual([]);
  });

  it('ID 区间连续无重（X12251~X12500）', () => {
    const { entries } = runAi50Checks();
    const nums = entries.map((e) => Number(e.id.slice(1))).sort((a, b) => a - b);
    expect(new Set(nums).size).toBe(250);
    expect(nums[0]).toBe(12251);
    expect(nums[249]).toBe(12500);
    for (let i = 1; i < nums.length; i++) {
      expect(nums[i]).toBe((nums[i - 1] as number) + 1);
    }
  });
});
