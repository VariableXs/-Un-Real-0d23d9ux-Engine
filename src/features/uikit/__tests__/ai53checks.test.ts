// UNREAL-X AI-53：族0521~0530「工作台范式与 kit 基础」断言组全量自检，勿删。
import { describe, expect, it } from 'vitest';
import { runAi53Checks } from '../checks';
import * as I from '../groupI';

const FAMILIES = [521, 522, 523, 524, 525, 526, 527, 528, 529, 530];

/** 按族号归集：族 N 覆盖 X((N-1)*25+1) ~ X(N*25)。 */
function groupByFamily(ids: string[]): Map<number, Set<string>> {
  const map = new Map<number, Set<string>>();
  for (const id of ids) {
    const n = Number(id.slice(1));
    const fam = Math.floor((n - 1) / 25) + 1;
    const set = map.get(fam) ?? new Set<string>();
    set.add(id);
    map.set(fam, set);
  }
  return map;
}

describe('UNREAL-X AI-53 断言组（族0521~0530 · X13001~X13250）', () => {
  it('10 族全部通过且无重复 ID', () => {
    const { entries, failed } = runAi53Checks();
    expect(entries.length).toBeGreaterThanOrEqual(50);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });

  it('每族满编 25 项（族0527 跨函数合成亦算）', () => {
    const { entries, failed } = runAi53Checks();
    const byFam = groupByFamily(entries.map((e) => e.id));
    for (const fam of FAMILIES) {
      expect(byFam.get(fam)?.size ?? 0, `族${fam} 应满编 25 项`).toBe(25);
    }
    expect(failed).toEqual([]);
  });

  it('每族断言函数可独立求值', () => {
    const fns = [
      I.checkX0521, I.checkX0522, I.checkX0523, I.checkX0524, I.checkX0525,
      I.checkX0526, I.checkX0527, I.checkX0528, I.checkX0529, I.checkX0530,
    ];
    for (const f of fns) {
      const list = f();
      expect(list.length).toBeGreaterThanOrEqual(5);
      for (const e of list) expect(e.check(), `${e.id} ${e.name}`).toBe(true);
    }
  });
});
