// UNREAL-X AI-01：族0001~0010「启动可靠与恢复」断言组全量自检，勿删。
import { describe, expect, it } from 'vitest';
import { runAi01Checks } from '../checks';
import * as X1 from '../groupX1';

const FAMILIES = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

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

describe('UNREAL-X AI-01 断言组（族0001~0010 · X00001~X00250）', () => {
  it('10 族全部通过且无重复 ID', () => {
    const { entries, failed } = runAi01Checks();
    expect(entries.length).toBeGreaterThanOrEqual(50);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });

  it('每族满编 25 项且逐项通过', () => {
    const { entries, failed } = runAi01Checks();
    const byFam = groupByFamily(entries.map((e) => e.id));
    for (const fam of FAMILIES) {
      expect(byFam.get(fam)?.size ?? 0, `族${String(fam).padStart(4, '0')} 应满编 25 项`).toBe(25);
    }
    expect(failed).toEqual([]);
  });

  it('每族断言函数可独立求值', () => {
    const fns = [
      X1.checkX0001, X1.checkX0002, X1.checkX0003, X1.checkX0004, X1.checkX0005,
      X1.checkX0006, X1.checkX0007, X1.checkX0008, X1.checkX0009, X1.checkX0010,
    ];
    for (const f of fns) {
      const list = f();
      expect(list.length).toBeGreaterThanOrEqual(5);
      for (const e of list) expect(e.check(), `${e.id} ${e.name}`).toBe(true);
    }
  });
});
