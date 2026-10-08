/**
 * UNREAL-X-15000 · AI-38 防线工程（领域10 · 族0371~0376 · X09251~X09400）门禁用例。
 * V 线三族 75 项（security/ai38Checks.ts）+ K 线三族由内核 ktest 覆盖。
 */
import { describe, expect, it } from 'vitest';
import { runAi38VChecks, checkF0372, checkF0373, checkF0374 } from '../ai38Checks';
import { EMERGENCY_LEVELS, ELDER_LEVELS, EDU_TIERS } from '../ai38Models';

describe('AI-38 防线工程 · V 线（族0372~0374）', () => {
  it('各族恰 25 项', () => {
    expect(checkF0372()).toHaveLength(25);
    expect(checkF0373()).toHaveLength(25);
    expect(checkF0374()).toHaveLength(25);
  });

  it('V 线聚合 75 项全绿', () => {
    const { entries, failed } = runAi38VChecks();
    expect(entries).toHaveLength(75);
    expect(failed).toEqual([]);
  });

  it('ID 连续覆盖 X09276~X09350', () => {
    const { entries } = runAi38VChecks();
    const ids = entries.map((e) => e.id);
    for (let x = 9276; x <= 9350; x++) {
      expect(ids).toContain(`X${String(x).padStart(5, '0')}`);
    }
    expect(new Set(ids).size).toBe(75);
  });

  it('三族档位常量五档对齐', () => {
    expect(EMERGENCY_LEVELS).toHaveLength(5);
    expect(ELDER_LEVELS).toHaveLength(5);
    expect(EDU_TIERS).toHaveLength(5);
  });
});
