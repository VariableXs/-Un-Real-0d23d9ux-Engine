// AURORA-10000: AI-71~AI-75 批次（领域15 UI 设计与优化）自检测试，勿删。
import { describe, expect, it } from 'vitest';
import { runDomain15Checks } from '../checks';
import { COMPONENT_CATALOG, SliderModel, niceTicks } from '../groupA';
import { LoadMachine, EMPTY_STATE_CATALOG, densityTokens } from '../groupB';
import { NAV_SECTIONS, virtualWindow, AuditLedger } from '../groupC';
import { RovingTabindex, TokenRegistry } from '../groupD';
import { readingMinutes, diskSortByLetter, UI_FINALE_CHECKLIST } from '../groupE';

describe('AURORA-10000 领域15 全量自检（F08751~F09375）', () => {
  it('625 项全部通过', () => {
    const { entries, failed } = runDomain15Checks();
    expect(entries.length).toBe(625);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });
});

describe('AI-71 逻辑核抽查', () => {
  it('组件目录 25 个且滑杆可调', () => {
    expect(COMPONENT_CATALOG.length).toBe(25);
    const s = new SliderModel(0, 10, 1, 0);
    expect(s.set(11)).toBe(10);
  });
  it('图表刻度 nice 化', () => {
    expect(niceTicks(0, 10, 5)[1]).toBe(2);
  });
});

describe('AI-72 逻辑核抽查', () => {
  it('加载状态机与空态目录', () => {
    const m = new LoadMachine();
    expect(m.start()).toBe('loading');
    expect(EMPTY_STATE_CATALOG.length).toBe(10);
  });
  it('密度令牌三档', () => {
    expect([densityTokens('compact').row, densityTokens('spacious').row]).toEqual([32, 48]);
  });
});

describe('AI-73 逻辑核抽查', () => {
  it('导航分区与虚拟列表', () => {
    expect(NAV_SECTIONS.length).toBe(4);
    expect(virtualWindow(0, 100, 50, 10).end).toBeLessThanOrEqual(10);
  });
  it('审计台账闭环', () => {
    const l = new AuditLedger();
    l.add({ rule: 'r', severity: 'error', target: 't' });
    expect(l.open().length).toBe(1);
    l.resolve('r', 't');
    expect(l.open().length).toBe(0);
  });
});

describe('AI-74 逻辑核抽查', () => {
  it('键盘 roving 循环', () => {
    const r = new RovingTabindex(3);
    expect(r.move('ArrowDown')).toBe(1);
    expect(r.move('ArrowDown')).toBe(2);
    expect(r.move('ArrowDown')).toBe(0);
  });
  it('令牌注册与治理', () => {
    const reg = new TokenRegistry();
    reg.register({ name: '--aurora-x', value: '1', category: 'color' });
    expect(reg.get('--aurora-x')!.value).toBe('1');
  });
});

describe('AI-75 逻辑核抽查（W0 八项口径）', () => {
  it('BUG-01 阅读时长与 BUG-03 盘符排序', () => {
    expect(readingMinutes(0)).toBe(0);
    expect(diskSortByLetter([{ letter: 'D', usedPct: 1 }, { letter: 'C', usedPct: 2 }]).map((d) => d.letter)).toEqual(['C', 'D']);
  });
  it('收官核对单 25 项', () => {
    expect(UI_FINALE_CHECKLIST.length).toBe(25);
  });
});
