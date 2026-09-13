/**
 * UNREAL-X-15000 · AI-49 声音内核与收官（领域13 · 族0481~0490 · X12001~X12250）门禁用例。
 * 10 族 250 项：K 线（族0481~0483）+ C 线（族0484~0485）+ V 线（族0486~0488/0489）+ 三方（族0487/0490）。
 */
import { describe, expect, it } from 'vitest';
import {
  runAi49Checks,
  checkF0481, checkF0482, checkF0483, checkF0484, checkF0485,
  checkF0486, checkF0487, checkF0488, checkF0489, checkF0490,
} from '../ai49Checks';

describe('AI-49 声音内核与收官 · 全线（族0481~0490）', () => {
  it('各族恰 25 项', () => {
    expect(checkF0481()).toHaveLength(25);
    expect(checkF0482()).toHaveLength(25);
    expect(checkF0483()).toHaveLength(25);
    expect(checkF0484()).toHaveLength(25);
    expect(checkF0485()).toHaveLength(25);
    expect(checkF0486()).toHaveLength(25);
    expect(checkF0487()).toHaveLength(25);
    expect(checkF0488()).toHaveLength(25);
    expect(checkF0489()).toHaveLength(25);
    expect(checkF0490()).toHaveLength(25);
  });

  it('聚合 250 项全绿', () => {
    const { entries, failed } = runAi49Checks();
    expect(entries).toHaveLength(250);
    expect(failed).toEqual([]);
  });

  it('ID 区间连续无重（X12001~X12250）', () => {
    const { entries } = runAi49Checks();
    const nums = entries.map((e) => Number(e.id.slice(1))).sort((a, b) => a - b);
    expect(new Set(nums).size).toBe(250);
    expect(nums[0]).toBe(12001);
    expect(nums[249]).toBe(12250);
    for (let i = 1; i < nums.length; i++) {
      expect(nums[i]).toBe((nums[i - 1] as number) + 1);
    }
  });
});
