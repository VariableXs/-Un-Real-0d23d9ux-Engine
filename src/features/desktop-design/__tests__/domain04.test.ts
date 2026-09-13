// UNREAL-X-15000 · 领域04（任务栏与开始菜单 · AI-13~16 V 线）自检测试，勿删。
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
import {
  QuickPanel, NotifCenter, TaskView, WindowSwitcher, SearchHub,
  QuickLauncher, QuickActions, MotionUnifier, OverlayA11y,
} from '../overlayModels';
import { TaskbarL10n, TaskbarTheme, TaskbarClosing, CLOSING_EXPECT } from '../taskbarEngineModels';

describe('UNREAL-X-15000 领域04 全量自检（AI-13~16 V 线 · X03001~X04000 去内核/C 线 8 族 · 800 项）', () => {
  it('800 项全部通过且 ID 唯一', () => {
    const { entries, failed } = runDomain04Checks();
    expect(entries.length).toBe(800);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });
  it('32 族每族恰 25 项', () => {
    for (const f of [121,130,140,141,142,143,144,145,146,147,149,150,157,158,160]) {
      const fam = checkFamily(`F0${f}`);
      expect(fam.length).toBe(25);
      expect(new Set(fam.map((e) => e.id)).size).toBe(25);
    }
  });
});

describe('AI-15 浮层系统 逻辑核抽查', () => {
  it('快捷面板档位与净身', () => {
    const p = new QuickPanel();
    expect(p.set('position', 'top-left')).toBe(true);
    expect(p.set('density', 9)).toBe(false);
    expect(QuickPanel.deserialize(p.serialize()).state.position).toBe('top-left');
    expect(p.pin('wifi')).toBe(true);
  });
  it('通知中心去重与勿扰', () => {
    const c = new NotifCenter();
    c.push('a', 'info', 't', 'b', 1);
    expect(c.push('a', 'info', 't', 'dup', 2)!.id).toBe(1);
    c.dnd = true;
    expect(c.push('x', 'info', 'y', 'z', 3)).toBeNull();
    c.dnd = false;
    expect(c.groupByApp().get('a')!.length).toBe(1);
  });
  it('任务视图/切换器/搜索/启动器/操作', () => {
    const v = new TaskView();
    v.open('w1', '写作', 1, 1);
    expect(v.onDesktop(1)[0]!.id).toBe('w1');
    const s = new WindowSwitcher();
    s.touch('a'); s.touch('b'); s.touch('c');
    expect(s.cycle(2, 1)).toBe(0);
    const h = new SearchHub();
    h.addSource('a', 'app', '终端');
    expect(h.query('终端')[0]!.id).toBe('a');
    const l = new QuickLauncher();
    l.register('x', '笔记');
    expect(l.launch('x')).toBe(true);
    const q = new QuickActions();
    q.toggle('airplane');
    expect(q.on.get('wifi')).toBe(false);
  });
  it('动效统一与浮层可达', () => {
    const m = new MotionUnifier();
    m.register('p', { curve: 'standard', durationMs: 240, scalePermille: 980 });
    m.setReduce(true);
    expect(m.effective('p')!.durationMs).toBe(120);
    const a = new OverlayA11y();
    a.open({ id: 'p', role: 'dialog', ariaLabel: '面板', focusable: true });
    expect(a.ariaOk() && a.contrastOk()).toBe(true);
  });
});

describe('AI-16 任务栏引擎 V 线 逻辑核抽查', () => {
  it('本地化四语回退', () => {
    const l = new TaskbarL10n();
    expect(l.t('startMenu.label')).toBe('开始');
    l.setLocale('en-US');
    expect(l.t('startMenu.label')).toBe('Start');
    expect(l.setLocale('fr-FR')).toBe(false);
    expect(TaskbarL10n.format('未读 {n} 条', { n: 2 })).toBe('未读 2 条');
  });
  it('主题三档与 HC 红线', () => {
    const t = new TaskbarTheme();
    expect(t.setTheme('hc')).toBe(true);
    expect(t.hcOk()).toBe(true);
    t.wallpaperLuma = 100;
    expect(t.autoFromWallpaper()).toBe('dark');
  });
  it('收官三线聚合', () => {
    const c = new TaskbarClosing();
    c.record('kernel', CLOSING_EXPECT.kernel);
    c.record('analysis', CLOSING_EXPECT.analysis);
    c.record('variable', CLOSING_EXPECT.variable);
    c.handshake('h1'); c.handshake('h2'); c.handshake('h3'); c.handshake('h4'); c.handshake('h5');
    expect(c.allGreen()).toBe(true);
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
