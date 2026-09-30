/**
 * UNREAL-X-15000 · AI-60 大收官（领域16 · 族0591~0600 · X14751~X15000）门禁用例。
 * 10 族 250 项：V 线（族0591/0592/0595~0600）+ C 线镜像（族0593/0594）。
 */
import { describe, expect, it } from 'vitest';
import {
  runAi60Checks,
  checkF0591, checkF0592, checkF0593, checkF0594, checkF0595,
  checkF0596, checkF0597, checkF0598, checkF0599, checkF0600,
} from '../ai60Checks';

describe('AI-60 大收官 · 全线（族0591~0600）', () => {
  it('各族恰 25 项', () => {
    expect(checkF0591()).toHaveLength(25);
    expect(checkF0592()).toHaveLength(25);
    expect(checkF0593()).toHaveLength(25);
    expect(checkF0594()).toHaveLength(25);
    expect(checkF0595()).toHaveLength(25);
    expect(checkF0596()).toHaveLength(25);
    expect(checkF0597()).toHaveLength(25);
    expect(checkF0598()).toHaveLength(25);
    expect(checkF0599()).toHaveLength(25);
    expect(checkF0600()).toHaveLength(25);
  });

  it('聚合 250 项全绿', () => {
    const { entries, failed } = runAi60Checks();
    expect(entries).toHaveLength(250);
    expect(failed).toEqual([]);
  });

  it('ID 区间连续无重（X14751~X15000）', () => {
    const { entries } = runAi60Checks();
    const nums = entries.map((e) => Number(e.id.slice(1))).sort((a, b) => a - b);
    expect(new Set(nums).size).toBe(250);
    expect(nums[0]).toBe(14751);
    expect(nums[249]).toBe(15000);
    for (let i = 1; i < nums.length; i++) {
      expect(nums[i]).toBe((nums[i - 1] as number) + 1);
    }
  });
});
