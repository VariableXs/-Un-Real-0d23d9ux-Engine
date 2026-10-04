/**
 * UNREAL-X-15000 · AI-41 生态治理（领域11 · 族0401~0410 · X10001~X10250）门禁用例。
 * 10 族 250 项：三方/V/K/C 线。
 */
import { describe, expect, it } from 'vitest';
import {
  runAi41Checks,
  checkF0401, checkF0402, checkF0403, checkF0404, checkF0405,
  checkF0406, checkF0407, checkF0408, checkF0409, checkF0410,
} from '../ai41Checks';

describe('AI-41 生态治理 · 全线（族0401~0410）', () => {
  it('各族恰 25 项', () => {
    expect(checkF0401()).toHaveLength(25);
    expect(checkF0402()).toHaveLength(25);
    expect(checkF0403()).toHaveLength(25);
    expect(checkF0404()).toHaveLength(25);
    expect(checkF0405()).toHaveLength(25);
    expect(checkF0406()).toHaveLength(25);
    expect(checkF0407()).toHaveLength(25);
    expect(checkF0408()).toHaveLength(25);
    expect(checkF0409()).toHaveLength(25);
    expect(checkF0410()).toHaveLength(25);
  });

  it('聚合 250 项全绿', () => {
    const { entries, failed } = runAi41Checks();
    expect(entries).toHaveLength(250);
    expect(failed).toEqual([]);
  });

  it('ID 区间连续无重（X10001~X10250）', () => {
    const { entries } = runAi41Checks();
    const nums = entries.map((e) => Number(e.id.slice(1))).sort((a, b) => a - b);
    expect(new Set(nums).size).toBe(250);
    expect(nums[0]).toBe(10001);
    expect(nums[249]).toBe(10250);
    for (let i = 1; i < nums.length; i++) {
      expect(nums[i]).toBe((nums[i - 1] as number) + 1);
    }
  });
});
