// UNREAL-X AI-04：startupTelemetry 逻辑核测试（X00751~X01000 V 线），勿删。
import { describe, expect, it } from 'vitest';
import {
  AI34_SCOPE,
  Archive,
  BootTelemetry,
  FailureCluster,
  GRADUATION_GATES,
  Graduation,
  Memoir,
  TELEMETRY_CAP,
  TELEMETRY_NAME_MAX,
  defaultBaseline,
  docChapter,
  docToc,
  finale,
} from '../startupTelemetry';

describe('族0031 启动遥测（X00751~X00775）', () => {
  it('环形缓冲 16 满载挤最旧并计 dropped', () => {
    const t = new BootTelemetry();
    for (let i = 0; i < TELEMETRY_CAP; i++) t.record(`e${i}`, i);
    expect(t.size).toBe(TELEMETRY_CAP);
    t.record('overflow', 99);
    expect(t.size).toBe(TELEMETRY_CAP);
    expect(t.dropped).toBe(1);
    expect(t.timeline()[0]!.name).toBe('e1');
    expect(t.timeline().at(-1)!.ms).toBe(99);
  });
  it('事件名脱敏截断 ≤8 字节、时长非负', () => {
    const t = new BootTelemetry();
    t.record('verylongname', -5);
    expect(t.timeline()[0]!.name.length).toBe(TELEMETRY_NAME_MAX);
    expect(t.timeline()[0]!.ms).toBe(0);
  });
  it('span/worst/percentile', () => {
    const t = new BootTelemetry();
    t.record('fw', 40);
    t.record('kernel', 120);
    t.record('shell', 200);
    expect(t.spanMs()).toBe(360);
    expect(t.worstMs()).toBe(200);
    expect(t.percentile(50)).toBe(120);
    expect(t.percentile(100)).toBe(200);
    expect(new BootTelemetry().percentile(50)).toBe(0);
  });
});

describe('族0032 失败学习（X00776~X00800）', () => {
  it('聚合、热点阈值 ≥3、top 平局字典序', () => {
    const f = new FailureCluster();
    f.record('E-BT-01');
    f.record('E-BT-02');
    f.record('E-BT-02');
    expect(f.hotspots()).toEqual([]);
    f.record('E-BT-02');
    expect(f.hotspots()).toEqual(['E-BT-02']);
    expect(f.total()).toBe(4);
    expect(f.top()).toEqual({ code: 'E-BT-02', count: 3 });
    const g = new FailureCluster();
    g.record('B');
    g.record('B');
    g.record('A');
    g.record('A');
    expect(g.top()!.code).toBe('A');
  });
});

describe('族0033 基线库（X00801~X00825）', () => {
  it('容差判定、只增不删、快照', () => {
    const lib = defaultBaseline();
    expect(lib.regressed('boot_ms', 400)).toBe(false);
    expect(lib.regressed('boot_ms', 401)).toBe(true);
    expect(lib.improved('boot_ms', 300)).toBe(true);
    expect(lib.regressed('nope', 999)).toBe(false);
    expect(lib.regressed('fps', 61)).toBe(true);
    expect(lib.define({ name: 'boot_ms', base: 1, tol: 1 })).toBe(false);
    expect(lib.snapshot()).toHaveLength(3);
  });
});

describe('族0035 文档剧场（X00851~X00875）', () => {
  it('双语章节、越界走附录、目录有序', () => {
    expect(docChapter(0).zh).toContain('冷启动');
    expect(docChapter(0).en).toContain('Ch.1');
    expect(docChapter(9).zh.startsWith('附录')).toBe(true);
    expect(docToc([0, 1])).toEqual(['第一章 · 冷启动链路', '第二章 · 品牌剧场']);
    expect(docToc([])).toEqual([]);
  });
});

describe('族0037 回忆录（X00901~X00925）', () => {
  it('去重、按天排序、D0 可记', () => {
    const m = new Memoir();
    m.add(3, '品牌剧场完成');
    m.add(1, '首次点亮');
    m.add(1, '首次点亮');
    m.add(0, '立项');
    expect(m.size).toBe(3);
    expect(m.timeline()[0]).toBe('D0: 立项');
    expect(m.timeline()[2]).toBe('D3: 品牌剧场完成');
  });
});

describe('族0038 毕业礼（X00926~X00950）', () => {
  it('四门禁齐 → 毕业词；未齐 → 鼓励词', () => {
    expect(GRADUATION_GATES).toHaveLength(4);
    const g = new Graduation();
    expect(g.ready).toBe(false);
    expect(g.message()).toContain('门禁未齐');
    ([0, 1, 2, 3] as const).forEach((i) => g.pass(i));
    expect(g.passed).toBe(4);
    expect(g.ready).toBe(true);
    expect(g.message()).toContain('毕业');
  });
});

describe('族0039 档案馆（X00951~X00975）', () => {
  it('归档去重、跨架检索、总数守恒', () => {
    const a = new Archive();
    a.file('brand', 501);
    a.file('brand', 501);
    a.file('telemetry', 751);
    expect(a.total).toBe(2);
    expect(a.find(501)).toBe('brand');
    expect(a.find(751)).toBe('telemetry');
    expect(a.find(9999)).toBeUndefined();
    expect(a.shelfNames()).toEqual(['brand', 'telemetry']);
  });
});

describe('族0040 大收官（X00976~X01000）', () => {
  it('AI-03/04 区间 500 项核算与判词', () => {
    expect(AI34_SCOPE.total).toBe(500);
    expect(AI34_SCOPE.to - AI34_SCOPE.from + 1).toBe(500);
    expect(finale(500, 500)).toEqual({ done: 500, percent: 100, verdict: '15000 全绿，基线冻结' });
    expect(finale(250, 500).verdict).toContain('过半');
    expect(finale(600, 500).done).toBe(500);
    expect(finale(0, 0).verdict).toBe('无计划');
  });
});
