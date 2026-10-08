/**
 * UNREAL-X-15000 · AI-26 生活工具面（领域07 · 族0251~0260 · X06251~X06500）门禁用例。
 * V 线十族 250 项（tools/ai26Checks.ts）。
 */
import { describe, expect, it } from 'vitest';
import { runAi26Checks, checkF0251, checkF0252, checkF0253, checkF0254, checkF0255, checkF0256, checkF0257, checkF0258, checkF0259, checkF0260 } from '../ai26Checks';

const families: [string, () => ReturnType<typeof checkF0251>][] = [
  ['族0251 录音音频工具', checkF0251],
  ['族0252 演示白板', checkF0252],
  ['族0253 阅读器', checkF0253],
  ['族0254 媒体播放器', checkF0254],
  ['族0255 图片查看器', checkF0255],
  ['族0256 打印中心', checkF0256],
  ['族0257 通讯录人脉', checkF0257],
  ['族0258 密码管理器', checkF0258],
  ['族0259 天气出行', checkF0259],
  ['族0260 地图位置', checkF0260],
];

describe('AI-26 生活工具面 · V 线（族0251~0260）', () => {
  it('各族恰 25 项', () => {
    for (const [name, f] of families) expect(f(), name).toHaveLength(25);
  });

  it('V 线聚合 250 项全绿', () => {
    const { entries, failed } = runAi26Checks();
    expect(entries).toHaveLength(250);
    expect(failed).toEqual([]);
  });

  it('ID 区间恰为 X06251~X06500 且唯一', () => {
    const { entries } = runAi26Checks();
    const ids = entries.map((e) => Number(e.id.slice(1)));
    expect(new Set(ids).size).toBe(250);
    expect(Math.min(...ids)).toBe(6251);
    expect(Math.max(...ids)).toBe(6500);
  });
});
