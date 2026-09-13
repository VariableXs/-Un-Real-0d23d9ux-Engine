/**
 * UNREAL-X-15000 · AI-33 兼容性防线·第 1 组（领域09 · 族0321~0330 · X08001~X08250）门禁用例。
 * 10 族 250 项：V 线（族0321~0325/0327）+ K 线（族0326/0328~0330）。
 */
import { describe, expect, it } from 'vitest';
import {
  runAi33Checks,
  checkF0321, checkF0322, checkF0323, checkF0324, checkF0325,
  checkF0326, checkF0327, checkF0328, checkF0329, checkF0330,
} from '../ai33Checks';

describe('AI-33 兼容性防线 · 全线（族0321~0330）', () => {
  it('各族恰 25 项', () => {
    expect(checkF0321()).toHaveLength(25);
    expect(checkF0322()).toHaveLength(25);
    expect(checkF0323()).toHaveLength(25);
    expect(checkF0324()).toHaveLength(25);
    expect(checkF0325()).toHaveLength(25);
    expect(checkF0326()).toHaveLength(25);
    expect(checkF0327()).toHaveLength(25);
    expect(checkF0328()).toHaveLength(25);
    expect(checkF0329()).toHaveLength(25);
    expect(checkF0330()).toHaveLength(25);
  });

  it('聚合 250 项全绿', () => {
    const { entries, failed } = runAi33Checks();
    expect(entries).toHaveLength(250);
    expect(failed).toEqual([]);
  });

  it('ID 区间连续无重（X08001~X08250）', () => {
    const { entries } = runAi33Checks();
    const nums = entries.map((e) => Number(e.id.slice(1))).sort((a, b) => a - b);
    expect(new Set(nums).size).toBe(250);
    expect(nums[0]).toBe(8001);
    expect(nums[249]).toBe(8250);
    for (let i = 1; i < nums.length; i++) {
      expect(nums[i]).toBe((nums[i - 1] as number) + 1);
    }
  });
});
