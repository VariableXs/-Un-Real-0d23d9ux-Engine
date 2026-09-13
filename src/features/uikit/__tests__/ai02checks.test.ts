// UNREAL-X AI-02：族0011~0020「电源状态剧场」断言组全量自检，勿删。
import { describe, expect, it } from 'vitest';
import { runAi02Checks } from '../checks';
import * as X2 from '../groupX2';

describe('UNREAL-X AI-02 断言组（族0011~0020 · X00251~X00500）', () => {
  it('全部通过且无重复 ID', () => {
    const { entries, failed } = runAi02Checks();
    expect(entries.length).toBeGreaterThanOrEqual(50);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });

  it('每族代表性断言 ≥5 条', () => {
    const families = [
      X2.checkX0011, X2.checkX0012, X2.checkX0013, X2.checkX0016, X2.checkX0017,
      X2.checkX0018, X2.checkX0019, X2.checkX0020, X2.checkX0020b,
    ];
    for (const f of families) {
      expect(f().length).toBeGreaterThanOrEqual(5);
    }
  });
});
