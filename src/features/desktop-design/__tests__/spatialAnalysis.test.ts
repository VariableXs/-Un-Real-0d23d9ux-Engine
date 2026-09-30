// UNREAL-X-15000 · AI-08 空间分析【V】消费端 UI 测试（族0071~0080），勿删。
import { describe, expect, it } from 'vitest';
import {
  parseProfileSnapshot,
  cellsToView,
  messNarrative,
  layoutAdvice,
  buildReport,
} from '../spatialAnalysis';

describe('AI-08 空间分析消费端（X01751~X02000 视图层）', () => {
  it('画像快照解析为 Top-K 条目', () => {
    const snap = 'UX71;3;1000\nb 600 2\na 300 1\nc 100 5';
    expect(parseProfileSnapshot(snap, 2)).toEqual([
      { app: 'b', focusMs: 600, switches: 2 },
      { app: 'a', focusMs: 300, switches: 1 },
    ]);
    expect(parseProfileSnapshot('BAD', 3)).toEqual([]);
  });

  it('热力格子映射三档热级', () => {
    const cells = [0, 100, 500, 900];
    expect(cellsToView(cells, 4)).toEqual([
      { gx: 1, gy: 0, level: 0 },
      { gx: 2, gy: 0, level: 1 },
      { gx: 3, gy: 0, level: 2 },
    ]);
  });

  it('混乱度叙事五档与边界', () => {
    expect(messNarrative(0)).toBe('整洁：无需整理');
    expect(messNarrative(100)).toBe('轻微：可随手归位');
    expect(messNarrative(300)).toBe('中等：建议开启整理助手');
    expect(messNarrative(550)).toBe('混乱：建议一键收纳');
    expect(messNarrative(750)).toBe('严重：建议重置为推荐布局');
    expect(messNarrative(-1)).toBe('无效评分');
  });

  it('布局建议超容量降级', () => {
    expect(layoutAdvice('quarter-grid', 4, '四分网格')).toEqual({ layout: 'quarter-grid', reason: '四分网格' });
    expect(layoutAdvice('half-split', 3, 'x')).toBeNull();
    expect(layoutAdvice('unknown', 1, 'x')).toBeNull();
  });

  it('聚合报告：全字段与缺省字段', () => {
    const full = buildReport({
      profileSnapshot: 'UX71;1;10\na 10 0',
      heatmapCells: [0, 800, 0, 0],
      gridW: 2,
      messScore: 600,
      recommendedLayout: 'masonry',
      recommendReason: '瀑布网格',
      windowCount: 6,
      switchAvgMs: 250,
    });
    expect(full.topApps[0]?.app).toBe('a');
    expect(full.hotspots).toEqual([{ gx: 1, gy: 0, level: 2 }]);
    expect(full.messNarrative).toBe('混乱：建议一键收纳');
    expect(full.advice).toEqual({ layout: 'masonry', reason: '瀑布网格' });
    expect(full.switchAvgMs).toBe(250);

    const empty = buildReport({});
    expect(empty.topApps).toEqual([]);
    expect(empty.hotspots).toEqual([]);
    expect(empty.messNarrative).toBe('整洁：无需整理');
    expect(empty.advice).toBeNull();
    expect(empty.switchAvgMs).toBe(0);
  });
});
