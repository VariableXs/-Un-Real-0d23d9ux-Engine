/**
 * UNREAL-X-15000 · AI-56 UI 质量收官（领域15 · V 线 6 族 · X13751~X13775/X13776~X13800/
 * X13801~X13825/X13851~X13875/X13901~X13925/X13976~X14000）门禁用例。
 * C 线 4 族（族0554/0556/0558/0559 · 100 项）见 code-analysis/core/src/ai56.rs。
 */
import { describe, expect, it } from 'vitest';
import {
  runAi56Checks,
  checkF0551, checkF0552, checkF0553, checkF0555, checkF0557, checkF0560,
} from '../ai56Checks';

describe('AI-56 UI 质量收官 · V 线（族0551~0553/0555/0557/0560）', () => {
  it('各族恰 25 项', () => {
    expect(checkF0551()).toHaveLength(25);
    expect(checkF0552()).toHaveLength(25);
    expect(checkF0553()).toHaveLength(25);
    expect(checkF0555()).toHaveLength(25);
    expect(checkF0557()).toHaveLength(25);
    expect(checkF0560()).toHaveLength(25);
  });

  it('聚合 150 项全绿', () => {
    const { entries, failed } = runAi56Checks();
    expect(entries).toHaveLength(150);
    expect(failed).toEqual([]);
  });

  it('V 线 ID 区间正确且无重（六族：X13751~13825 · X13851~13875 · X13901~13925 · X13976~14000）', () => {
    const { entries } = runAi56Checks();
    const nums = entries.map((e) => Number(e.id.slice(1)));
    expect(new Set(nums).size).toBe(150);
    const ranges: Array<[number, number]> = [
      [13751, 13775], [13776, 13800], [13801, 13825], [13851, 13875], [13901, 13925], [13976, 14000],
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
