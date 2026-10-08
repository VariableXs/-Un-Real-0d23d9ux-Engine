/**
 * UNREAL-X-15000 · AI-27 工具智能与联动（领域07 · 族0261~0270 · X06501~X06750）门禁用例。
 * V 线五族 125 项（tools/ai27Checks.ts）；族0263（X06551~X06575）与族0267~0270（X06651~X06750）为 C/K 线，零重叠。
 */
import { describe, expect, it } from 'vitest';
import { runAi27VChecks, checkF0261, checkF0262, checkF0264, checkF0265, checkF0266 } from '../ai27Checks';

const V_RANGES: [number, number][] = [[6501, 6525], [6526, 6550], [6576, 6600], [6601, 6625], [6626, 6650]];
const CK_RANGES: [number, number][] = [[6551, 6575], [6651, 6750]];

describe('AI-27 工具智能与联动 · V 线（族0261/0262/0264/0265/0266）', () => {
  it('各族恰 25 项', () => {
    expect(checkF0261()).toHaveLength(25);
    expect(checkF0262()).toHaveLength(25);
    expect(checkF0264()).toHaveLength(25);
    expect(checkF0265()).toHaveLength(25);
    expect(checkF0266()).toHaveLength(25);
  });

  it('V 线聚合 125 项全绿', () => {
    const { entries, failed } = runAi27VChecks();
    expect(entries).toHaveLength(125);
    expect(failed).toEqual([]);
  });

  it('ID 唯一且全部落在 V 线区间，与 C/K 线零重叠', () => {
    const { entries } = runAi27VChecks();
    const ids = entries.map((e) => Number(e.id.slice(1)));
    expect(new Set(ids).size).toBe(125);
    for (const n of ids) {
      expect(V_RANGES.some(([a, b]) => n >= a && n <= b)).toBe(true);
    }
    for (const [a, b] of V_RANGES) {
      expect(ids.includes(a)).toBe(true);
      expect(ids.includes(b)).toBe(true);
    }
    for (const [a, b] of CK_RANGES) {
      expect(ids.some((n) => n >= a && n <= b)).toBe(false);
    }
  });
});
