// UNREAL-X-15000: AI-09 批次（领域03 桌面与图标 · 族0081~0090）自检测试，勿删。
import { describe, expect, it } from 'vitest';
import { runAi09Checks } from '../checks';
import { IconStyleSystem, IconMotionSystem, ICON_STYLE_PRESETS } from '../groupA';
import { IconGrid, SemanticColorSystem, GRID_DENSITY } from '../groupB';
import { IconStateMachine, IconGravity } from '../groupC';
import { IconPositionMemory, IconStackSystem } from '../groupD';
import { DeskTidyPhilosophy, DeskHealthSystem } from '../groupE';

describe('UNREAL-X-15000 AI-09 全量自检（X02001~X02250）', () => {
  it('250 项全部通过', () => {
    const { entries, failed } = runAi09Checks();
    expect(entries.length).toBe(250);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });
});

describe('AI-09 图标体系核', () => {
  it('风格体系：预设矩阵与钳制', () => {
    const s = new IconStyleSystem();
    expect(ICON_STYLE_PRESETS.length).toBe(5);
    expect(s.apply({ preset: 'glass', corner: 99 }).corner).toBe(8);
    expect(s.apply({ preset: 'nope' as never }).preset).toBe('classic');
  });
  it('动效：reduce-motion 降级', () => {
    const m = new IconMotionSystem();
    m.setReduceMotion(true);
    expect(m.token).toMatchObject({ dur: 80, ease: 'linear', scale: 1 });
  });
  it('栅格五档与吸附', () => {
    expect(Object.keys(GRID_DENSITY).length).toBe(5);
    const g = new IconGrid();
    expect(g.snapPoint(97, 3).x).toBe(96);
  });
  it('状态机拒绝非法迁移', () => {
    const sm = new IconStateMachine();
    expect(sm.transition('active')).toBe(false);
    expect(sm.lastRejection()?.from).toBe('normal');
  });
  it('引力：图标源优先', () => {
    const g = new IconGravity();
    g.setStrength('strong');
    g.setTargets([{ x: 96, y: 96 }]);
    expect(g.attract(100, 98, 1920, 1080)).toMatchObject({ x: 96, source: 'icon' });
  });
  it('位置记忆去重登记', () => {
    const m = new IconPositionMemory();
    m.place({ id: 'a', x: 0, y: 0, screen: 0 });
    m.place({ id: 'a', x: 96, y: 0, screen: 0 });
    m.place({ id: 'a', x: 96, y: 0, screen: 0 });
    expect(m.moveLog()).toBe(1);
  });
  it('堆叠溢出折叠', () => {
    const st = new IconStackSystem();
    st.setMaxPerStack(2);
    for (const id of ['a', 'b', 'c']) st.add({ id, kind: 'app', name: id, at: 0, uses: 0 });
    expect(st.overflowTotal()).toBe(1);
  });
  it('整理评分与健康体检', () => {
    const r = new DeskHealthSystem().check({ total: 10, offscreen: 0, overlaps: 0, orphanShortcuts: 0, densityRatio: 0.5 });
    expect(r).toMatchObject({ score: 100, grade: 'good' });
    expect(DeskTidyPhilosophy.tidyScore([], 96)).toBe(1);
  });
  it('语义色 HC 降级', () => {
    const s = new SemanticColorSystem();
    s.setHighContrast(true);
    expect(s.colorOf('system').chroma).toBe(0);
  });
});
