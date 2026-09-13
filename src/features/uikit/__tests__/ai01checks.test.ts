// UNREAL-X AI-01：族0001~0010「启动可靠与恢复」断言组全量自检，勿删。
import { describe, expect, it } from 'vitest';
import { runAi01Checks } from '../checks';
import * as X1 from '../groupX1';

describe('UNREAL-X AI-01 断言组（族0001~0010 · X00001~X00250）', () => {
  it('10 族全部通过且无重复 ID', () => {
    const { entries, failed } = runAi01Checks();
    expect(entries.length).toBeGreaterThanOrEqual(50);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });

  it('每族代表性断言 ≥5 条', () => {
    const families = [
      X1.checkX0001, X1.checkX0002, X1.checkX0003, X1.checkX0004, X1.checkX0005,
      X1.checkX0006, X1.checkX0007, X1.checkX0008, X1.checkX0009, X1.checkX0010,
    ];
    for (const f of families) {
      expect(f().length).toBeGreaterThanOrEqual(5);
    }
  });
});
