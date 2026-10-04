/**
 * UNREAL-X-15000 · AI-40 生态面（领域11 · 族0391~0400 · X09751~X10000）门禁用例。
 * 10 族 250 项：V/三方线。
 */
import { describe, expect, it } from 'vitest';
import {
  runAi40Checks,
  checkF0391, checkF0392, checkF0393, checkF0394, checkF0395,
  checkF0396, checkF0397, checkF0398, checkF0399, checkF0400,
} from '../ai40Checks';

describe('AI-40 生态面 · 全线（族0391~0400）', () => {
  it('各族恰 25 项', () => {
    expect(checkF0391()).toHaveLength(25);
    expect(checkF0392()).toHaveLength(25);
    expect(checkF0393()).toHaveLength(25);
    expect(checkF0394()).toHaveLength(25);
    expect(checkF0395()).toHaveLength(25);
    expect(checkF0396()).toHaveLength(25);
    expect(checkF0397()).toHaveLength(25);
    expect(checkF0398()).toHaveLength(25);
    expect(checkF0399()).toHaveLength(25);
    expect(checkF0400()).toHaveLength(25);
  });

  it('聚合 250 项全绿', () => {
    const { entries, failed } = runAi40Checks();
    expect(entries).toHaveLength(250);
    expect(failed).toEqual([]);
  });

  it('ID 区间连续无重（X09751~X10000）', () => {
    const { entries } = runAi40Checks();
    const nums = entries.map((e) => Number(e.id.slice(1))).sort((a, b) => a - b);
    expect(new Set(nums).size).toBe(250);
    expect(nums[0]).toBe(9751);
    expect(nums[249]).toBe(10000);
    for (let i = 1; i < nums.length; i++) {
      expect(nums[i]).toBe((nums[i - 1] as number) + 1);
    }
  });
});
