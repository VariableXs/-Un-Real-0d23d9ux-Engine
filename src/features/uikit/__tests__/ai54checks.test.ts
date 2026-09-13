// UNREAL-X AI-54：族0531~0540「kit 系统件与系统范式」断言组全量自检，勿删。
import { describe, expect, it } from 'vitest';
import { runAi54Checks } from '../checks';
import * as J from '../groupJ';

const FAMILIES = [531, 532, 533, 534, 535, 536, 537, 538, 539, 540];

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

describe('UNREAL-X AI-54 断言组（族0531~0540 · X13251~X13500）', () => {
  it('10 族全部通过且无重复 ID', () => {
    const { entries, failed } = runAi54Checks();
    expect(entries.length).toBeGreaterThanOrEqual(50);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });

  it('每族满编 25 项', () => {
    const { entries, failed } = runAi54Checks();
    const byFam = groupByFamily(entries.map((e) => e.id));
    for (const fam of FAMILIES) {
      expect(byFam.get(fam)?.size ?? 0, `族${fam} 应满编 25 项`).toBe(25);
    }
    expect(failed).toEqual([]);
  });

  it('每族断言函数可独立求值', () => {
    const fns = [
      J.checkX0531, J.checkX0532, J.checkX0533, J.checkX0534, J.checkX0535,
      J.checkX0536, J.checkX0537, J.checkX0538, J.checkX0539, J.checkX0540,
    ];
    for (const f of fns) {
      const list = f();
      expect(list.length).toBeGreaterThanOrEqual(5);
      for (const e of list) expect(e.check(), `${e.id} ${e.name}`).toBe(true);
    }
  });
});
