/**
 * UNREAL-X-15000 · AI-22 数据能力面（领域06 · 族0211~0220 · X05251~X05500）门禁用例。
 * V 线十族 250 项（files/ai22Checks.ts）。
 */
import { describe, expect, it } from 'vitest';
import { runAi22Checks, checkF0211, checkF0212, checkF0213, checkF0214, checkF0215, checkF0216, checkF0217, checkF0218, checkF0219, checkF0220 } from '../ai22Checks';

const families: [string, () => ReturnType<typeof checkF0211>][] = [
  ['族0211 同步备份', checkF0211],
  ['族0212 完整性', checkF0212],
  ['族0213 加密文件', checkF0213],
  ['族0214 安全删除', checkF0214],
  ['族0215 文档处理', checkF0215],
  ['族0216 图片工具', checkF0216],
  ['族0217 音视频工具', checkF0217],
  ['族0218 压缩中心', checkF0218],
  ['族0219 文件监视', checkF0219],
  ['族0220 数据互操作', checkF0220],
];

describe('AI-22 数据能力面 · V 线（族0211~0220）', () => {
  it('各族恰 25 项', () => {
    for (const [name, f] of families) expect(f(), name).toHaveLength(25);
  });

  it('V 线聚合 250 项全绿', () => {
    const { entries, failed } = runAi22Checks();
    expect(entries).toHaveLength(250);
    expect(failed).toEqual([]);
  });

  it('ID 区间恰为 X05251~X05500 且唯一', () => {
    const { entries } = runAi22Checks();
    const ids = entries.map((e) => Number(e.id.slice(1)));
    expect(new Set(ids).size).toBe(250);
    expect(Math.min(...ids)).toBe(5251);
    expect(Math.max(...ids)).toBe(5500);
  });
});
