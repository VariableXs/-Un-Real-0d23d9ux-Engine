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

  it('每族满编 25 项', () => {
    // 族0011 由 checkX0011（1~10、16~25）+ checkX0022（氛围侧 11~15）合成，
    // 族0014/0020 分别落在 checkX0014/checkX0021，其余一族一函数。
    const perFamily: Array<[number, Array<() => { id: string }[]>]> = [
      [11, [X2.checkX0011, X2.checkX0022]],
      [12, [X2.checkX0012]],
      [13, [X2.checkX0013]],
      [14, [X2.checkX0014]],
      [15, [X2.checkX0016]],
      [16, [X2.checkX0017]],
      [17, [X2.checkX0018]],
      [18, [X2.checkX0019]],
      [19, [X2.checkX0020]],
      [20, [X2.checkX0021]],
    ];
    for (const [fam, fns] of perFamily) {
      const ids = fns.flatMap((f) => f().map((e) => e.id));
      const uniq = new Set(ids);
      expect(uniq.size, `族${fam} 去重后应为 25 项`).toBe(25);
      expect(ids.length, `族${fam} 不应有重复 ID`).toBe(uniq.size);
    }
  });
});
