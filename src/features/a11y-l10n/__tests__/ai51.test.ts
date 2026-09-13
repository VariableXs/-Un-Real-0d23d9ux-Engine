/**
 * UNREAL-X-15000 · AI-51 无障碍与本地化·第2组（领域14 · 族0501~0510 · X12501~X12750）门禁用例。
 * 10 族 250 项：V 线（族0501/0504/0506~0510）+ 三方（族0502/0505）+ C 线镜像（族0503 → ai51.rs）。
 */
import { describe, expect, it } from 'vitest';
import { runAi51Checks, checkF0501, checkF0502, checkF0503, checkF0504, checkF0505, checkF0506, checkF0507, checkF0508, checkF0509, checkF0510 } from '../ai51Checks';

describe('AI-51 无障碍与本地化·第2组 · 全线（族0501~0510）', () => {
  it('各族恰 25 项', () => {
    expect(checkF0501()).toHaveLength(25);
    expect(checkF0502()).toHaveLength(25);
    expect(checkF0503()).toHaveLength(25);
    expect(checkF0504()).toHaveLength(25);
    expect(checkF0505()).toHaveLength(25);
    expect(checkF0506()).toHaveLength(25);
    expect(checkF0507()).toHaveLength(25);
    expect(checkF0508()).toHaveLength(25);
    expect(checkF0509()).toHaveLength(25);
    expect(checkF0510()).toHaveLength(25);
  });

  it('聚合 250 项全绿', () => {
    const { entries, failed } = runAi51Checks();
    expect(entries).toHaveLength(250);
    expect(failed).toEqual([]);
  });

  it('ID 区间连续无重（X12501~X12750）', () => {
    const { entries } = runAi51Checks();
    const nums = entries.map((e) => Number(e.id.slice(1))).sort((a, b) => a - b);
    expect(new Set(nums).size).toBe(250);
    expect(nums[0]).toBe(12501);
    expect(nums[249]).toBe(12750);
    for (let i = 1; i < nums.length; i++) {
      expect(nums[i]).toBe((nums[i - 1] as number) + 1);
    }
  });
});
