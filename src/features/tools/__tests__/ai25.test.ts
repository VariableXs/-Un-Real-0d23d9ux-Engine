/**
 * UNREAL-X-15000 · AI-25 效率与工具中枢（领域07 · 族0241~0250 · X06001~X06250）门禁用例。
 * V 线十族 250 项（tools/ai25Checks.ts），含 TODO_GANTT_RESERVED 预留位实装标记。
 */
import { describe, expect, it } from 'vitest';
import { runAi25Checks, checkF0241, checkF0242, checkF0243, checkF0244, checkF0245, checkF0246, checkF0247, checkF0248, checkF0249, checkF0250 } from '../ai25Checks';
import { TODO_GANTT_X2 } from '../ai25Models';

describe('AI-25 效率与工具中枢 · V 线（族0241~0250）', () => {
  it('各族恰 25 项', () => {
    expect(checkF0241()).toHaveLength(25);
    expect(checkF0242()).toHaveLength(25);
    expect(checkF0243()).toHaveLength(25);
    expect(checkF0244()).toHaveLength(25);
    expect(checkF0245()).toHaveLength(25);
    expect(checkF0246()).toHaveLength(25);
    expect(checkF0247()).toHaveLength(25);
    expect(checkF0248()).toHaveLength(25);
    expect(checkF0249()).toHaveLength(25);
    expect(checkF0250()).toHaveLength(25);
  });

  it('V 线聚合 250 项全绿', () => {
    const { entries, failed } = runAi25Checks();
    expect(entries).toHaveLength(250);
    expect(failed).toEqual([]);
  });

  it('ID 连续覆盖 X06001~X06250', () => {
    const { entries } = runAi25Checks();
    const ids = entries.map((e) => Number(e.id.slice(1))).sort((a, b) => a - b);
    expect(new Set(ids).size).toBe(250);
    expect(ids[0]).toBe(6001);
    expect(ids.at(-1)).toBe(6250);
  });

  it('甘特图预留位已实装', () => {
    expect(TODO_GANTT_X2).toBe(true);
  });
});
