// UNREAL-X-15000 · 领域04（任务栏与开始菜单 · AI-13/AI-14）自检测试，勿删。
import { describe, expect, it } from 'vitest';
import { runDomain04Checks, checkFamily } from '../domain04';
import {
  TaskbarShape, TaskbarInteraction, TrayArea, WidgetStrip,
  PreviewCard, ProgressFusion, TaskbarGrouping, TaskbarMultiScreen,
} from '../taskbarModels';
import {
  StartMenuStructure, TilesEcosystem, StartSearch, StartPersonalization,
  StartMenuBehavior, RecommendationEngine, AppCatalogography, MenuMotion, MenuA11y, MenuPerf,
} from '../startMenuModels';

describe('UNREAL-X-15000 领域04 全量自检（X03001~X03500 · 500 项）', () => {
  it('500 项全部通过且 ID 唯一', () => {
    const { entries, failed } = runDomain04Checks();
    expect(entries.length).toBe(500);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });
  it('20 族每族恰 25 项', () => {
    for (let f = 121; f <= 140; f++) {
      const fam = checkFamily(`F0${f}`);
      expect(fam.length).toBe(25);
      expect(new Set(fam.map((e) => e.id)).size).toBe(25);
    }
  });
});

describe('AI-13 任务栏 逻辑核抽查', () => {
  it('形态档位矩阵与快照迁移', () => {
    const t = new TaskbarShape();
    expect(t.set('position', 'left')).toBe(true);
    expect(t.set('position', 'diagonal' as never)).toBe(false);
    const t2 = TaskbarShape.deserialize(t.serialize());
    expect(t2.state.position).toBe('left');
    expect(t.height()).toBe(48);
  });
  it('交互开合与防抖', () => {
    const x = new TaskbarInteraction();
    x.add('a');
    expect(x.click('a', 'left', 0)).toBe('focus');
    expect(x.click('a', 'left', 10)).toBe('ignored');
    expect(x.click('a', 'left', 500)).toBe('minimize');
    expect(x.click('a', 'middle', 600)).toBe('new-instance');
  });
  it('托盘溢出与徽标 99+', () => {
    const tray = new TrayArea();
    tray.register('a', '', true);
    for (let i = 0; i < 7; i++) tray.register(`t${i}`);
    expect(tray.layout().visible.length).toBe(5);
    tray.setBadge('a', 150);
    expect(tray.badgeLabel('a')).toBe('99+');
  });
  it('小组件、行为、预览卡、进度、分组、多屏、性能', () => {
    const w = new WidgetStrip();
    expect(w.add('clock')!.id).toBe('clock-0');
    expect(w.add('b')).not.toBeNull();
    expect(w.add('c')).not.toBeNull();
    expect(w.add('d')).not.toBeNull();
    expect(w.add('e')).toBeNull();
    const p = new PreviewCard();
    p.cacheThumb('w1', 't');
    expect(p.title('一'.repeat(40)).length).toBe(32);
    const f = new ProgressFusion();
    f.set('w', 150);
    expect(f.of('w').value).toBe(100);
    const g = new TaskbarGrouping();
    expect(g.group([{ winId: '1', app: 'a', order: 0 }, { winId: '2', app: 'a', order: 1 }, { winId: '3', app: 'a', order: 2 }]).length).toBe(0);
    const m = new TaskbarMultiScreen();
    m.setMonitors([
      { id: 'm1', primary: true, bounds: { x: 0, y: 0, w: 1920, h: 1080 } },
      { id: 'm2', primary: false, bounds: { x: 1920, y: 0, w: 1920, h: 1080 } },
    ]);
    expect(m.monitorOf(2000, 0)).toBe('m2');
  });
});

describe('AI-14 开始菜单 逻辑核抽查', () => {
  it('结构四区与磁贴生态', () => {
    const s = new StartMenuStructure();
    expect(s.visibleIds().join()).toBe('search,pinned,recommended,power');
    expect(s.pinnedCapacity).toBe(18);
    const t = new TilesEcosystem();
    t.add('a', 'medium', 'g1');
    t.move('a', 0, 'g2');
    expect(t.inGroup('g2').length).toBe(1);
  });
  it('搜索排序与个性化钳制', () => {
    const s = new StartSearch();
    s.index([
      { name: '记事本', kind: 'app', hot: 10 },
      { name: '记事回放', kind: 'file', hot: 99 },
    ]);
    expect(s.query('记事')[0]!.name).toBe('记事本');
    const p = new StartPersonalization();
    expect(p.set('accent', 'red')).toBe(false);
    expect(p.set('accent', '#ff8800')).toBe(true);
  });
  it('行为状态机、推荐引擎、目录学', () => {
    const b = new StartMenuBehavior();
    b.toggle();
    expect(b.initialFocus()).toBe('search');
    expect(b.dismiss('escape')).toBe(true);
    expect(b.dismiss('escape')).toBe(false);
    const r = new RecommendationEngine();
    r.record('app', r.now);
    r.record('app', r.now - r.halfLifeMs);
    expect(r.score('app')).toBe(1.5);
    r.reject('app');
    expect(r.score('app')).toBe(0);
    const c = new AppCatalogography();
    expect(c.register('a', '甲', '生产力')).toBe(true);
    expect(c.register('a', '甲', '游戏')).toBe(false);
    expect(c.register('b', '乙', '不存在的类')).toBe(false);
  });
  it('动效降级、可达性、性能预算', () => {
    expect(MenuMotion.reduced({ durMs: 250, ease: 'standard' })).toEqual({ durMs: 100, ease: 'linear' });
    const a = new MenuA11y();
    a.setFocusables(['x', 'y']);
    expect(a.nav('ArrowDown')).toBe('y');
    expect(a.nav('ArrowDown')).toBe('x');
    expect(MenuA11y.contrast([255, 255, 255], [0, 0, 0])).toBeGreaterThan(20);
    const p = new MenuPerf();
    expect(p.inBudget('open', 100)).toBe(true);
    expect(p.inBudget('open', 101)).toBe(false);
    expect(p.renderPage('p', 5)).toBe(5);
    expect(p.renderPage('p', 9)).toBe(5);
  });
});
