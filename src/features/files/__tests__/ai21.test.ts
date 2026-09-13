/**
 * UNREAL-X-15000 · AI-21 文件管理面（领域06 · 族0201~0210 · X05001~X05250）门禁用例。
 * V 线十族 250 项（files/ai21Checks.ts）。
 */
import { describe, expect, it } from 'vitest';
import { runAi21Checks, checkF0201, checkF0202, checkF0203, checkF0204, checkF0205, checkF0206, checkF0207, checkF0208, checkF0209, checkF0210 } from '../ai21Checks';

const families: [string, () => ReturnType<typeof checkF0201>][] = [
  ['族0201 管理器核心', checkF0201],
  ['族0202 文件预览', checkF0202],
  ['族0203 文件搜索', checkF0203],
  ['族0204 文件元数据', checkF0204],
  ['族0205 文件操作进阶', checkF0205],
  ['族0206 回收站与恢复', checkF0206],
  ['族0207 磁盘与空间', checkF0207],
  ['族0208 文件组织哲学', checkF0208],
  ['族0209 拖拽数据', checkF0209],
  ['族0210 文件管理性能', checkF0210],
];

describe('AI-21 文件管理面 · V 线（族0201~0210）', () => {
  it('各族恰 25 项', () => {
    for (const [name, f] of families) expect(f(), name).toHaveLength(25);
  });

  it('V 线聚合 250 项全绿', () => {
    const { entries, failed } = runAi21Checks();
    expect(entries).toHaveLength(250);
    expect(failed).toEqual([]);
  });

  it('ID 区间恰为 X05001~X05250 且唯一', () => {
    const { entries } = runAi21Checks();
    const ids = entries.map((e) => Number(e.id.slice(1)));
    expect(new Set(ids).size).toBe(250);
    expect(Math.min(...ids)).toBe(5001);
    expect(Math.max(...ids)).toBe(5250);
  });
});
