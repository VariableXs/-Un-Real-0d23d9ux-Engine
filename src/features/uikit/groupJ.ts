// UNREAL-X AI-54：族0531~0540「kit 系统件与系统范式」断言组（X13251~X13500 全量 25 项/族），勿删。
// 每族 25 条可运行断言 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
// 前段的族级功能断言全部走 src/features/uikit/systemModels.ts 的真实实现；
// 缺口补齐段由 X25 达标探针（x25.ts + x25Families.ts）按全景图逐项口径生成，native() 接地既有实现。

import { x25 } from './x25';
import { specOf, span } from './x25Families';
import * as S from './systemModels';
import type { CheckEntry } from './systemModels';

/* -------- 族0531 kit 数据展示件（X13251~X13275） -------- */
export function checkX0531(): CheckEntry[] {
  const av = new S.AvatarModelX(null, '王小明');
  const avImg = new S.AvatarModelX('u.png', 'John Smith');
  return [
    { id: 'X13251', name: 'Avatar 字/图两态', check: () => !av.showsImage() && av.label() === '小明' && avImg.showsImage() && avImg.label() === 'John Smith' },
    { id: 'X13252', name: 'Avatar 英文缩写', check: () => S.avatarInitialsX('John Smith') === 'JS' && S.avatarInitialsX('王小明') === '小明' },
    { id: 'X13253', name: 'Breadcrumb 中段省略', check: () => S.breadcrumbX(['此电脑', 'D', '项目']).length === 3 && S.breadcrumbX(['此电脑', 'D', 'a', 'b', '项目']).join('/') === '此电脑/…/项目' },
    { id: 'X13254', name: 'Kbd 键帽拆分与规格', check: () => { const k = new S.KbdModelX('Ctrl+Shift+P'); return k.keys().length === 3 && k.dims().h === 22 && k.dims().radius === 4; } },
    { id: 'X13255', name: '状态点四态常量', check: () => { const a = new S.AvatarModelX(null, 'x'); a.status = 'busy'; return a.status === 'busy'; } },
    ...x25(specOf(531), span(6, 25)),
  ];
}

/* -------- 族0532 kit 导航与系统★件（X13276~X13300） -------- */
export function checkX0532(): CheckEntry[] {
  const nav = new S.NavRailModel([
    { id: 'home', label: '主页', icon: '房子' },
    { id: 'system', label: '系统', icon: '显示器' },
    { id: 'update', label: '系统更新', icon: '循环箭头' },
  ]);
  const hero = new S.HeroCardModel();
  hero.setLinks([{ id: '1', label: '365' }, { id: '2', label: '云' }, { id: '3', label: '更新' }, { id: '4', label: '多' }]);
  const row = new S.SettingsRowModel('屏幕', '亮度与缩放');
  const trow = new S.SettingsRowModel('通知', '应用通知', true);
  const tb = new S.TaskbarItemModel();
  return [
    { id: 'X13276', name: 'NavRail 选中整行 raised+3px 条', check: () => { const s = nav.selectionSpec(); return s.bg === 'var(--bg-raised)' && s.bar === 'var(--accent)' && s.barWidth === 3 && nav.rowHeight() === 44; } },
    { id: 'X13277', name: 'NavRail 键盘 roving', check: () => nav.keys('ArrowDown') === 1 && nav.isActive(1) && nav.keys('ArrowUp') === 0 },
    { id: 'X13278', name: 'SearchPill focus 展开 ESC 收起', check: () => { const p = new S.SearchPillModel(); p.focus(); return p.focused && p.escape() === false && !p.focused && (p.focus(), p.query = 'a', p.escape() === true && p.query === '' && p.focused); } },
    { id: 'X13279', name: 'HeroCard 链接 ≤3 超出折叠', check: () => hero.links.length === 3 && hero.overflow.length === 1 && hero.overflowLabel() === '…' && hero.thumbRatio === '16:9' && hero.height === 96 },
    { id: 'X13280', name: 'SettingsRow 整行语义 h≥72', check: () => row.role() === 'button' && row.minHeight() === 72 && trow.role() === 'row-with-switch' },
    { id: 'X13281', name: 'TaskbarItem 运行指示短点/长条', check: () => { tb.running = true; const short = tb.indicator(); tb.active = true; const long = tb.indicator(); tb.running = false; const none = tb.indicator(); return short === 'short' && long === 'long' && none === 'none'; } },
    { id: 'X13282', name: 'CommandPalette 语义与空态', check: () => { const cp = new S.CommandPaletteModel(); cp.pick('a'); cp.pick('b'); cp.pick('a'); const r = cp.roles(); return r.combo === 'combobox' && r.listbox === 'listbox' && r.empty.includes('试试'); } },
    ...x25(specOf(532), span(8, 25)),
  ];
}

/* -------- 族0533 材质五档参数（X13301~X13325） -------- */
export function checkX0533(): CheckEntry[] {
  const m = new S.MaterialSystem();
  return [
    { id: 'X13301', name: '五档 ×5 级表完整', check: () => S.MATERIAL_TABLE['m-frosted'].length === 5 && S.MATERIAL_TABLE['m-acrylic'].length === 5 && S.MATERIAL_TABLE['m-mica'].length === 5 },
    { id: 'X13302', name: 'frosted 1~5 级数值', check: () => { const r = S.MATERIAL_TABLE['m-frosted']; return r[0]!.blurPx === 8 && r[4]!.blurPx === 28 && r[0]!.bgAlpha === 0.55 && r[4]!.bgAlpha === 0.75 && r[2]!.saturate === 1 && r[0]!.noiseOpacity === 0; } },
    { id: 'X13303', name: 'acrylic 噪点/饱和', check: () => { const r = S.MATERIAL_TABLE['m-acrylic']; return r[0]!.saturate === 1.2 && r[0]!.noiseOpacity === 0.02 && r[4]!.noiseOpacity === 0.05; } },
    { id: 'X13304', name: 'mica 不糊内容 blur=0', check: () => S.MATERIAL_TABLE['m-mica'].every((l) => l.blurPx === 0) && S.MATERIAL_TABLE['m-mica'][0]!.saturate === 1.1 },
    { id: 'X13305', name: '强度钳位 1~5', check: () => { m.apply('m-frosted', 9); const hi = m.level; m.apply('m-mica', -1); const lo = m.level; return hi === 5 && lo === 1 && m.params()!.blurPx === 0; } },
    { id: 'X13306', name: 'HC 全降级 solid', check: () => { m.apply('m-acrylic'); return m.downgradeHighContrast() === 'm-solid'; } },
    { id: 'X13307', name: '低配降级链两级', check: () => { const a = new S.MaterialSystem(); a.apply('m-acrylic'); const d1 = a.degradeLowPerf(4, false); const d2 = a.degradeLowPerf(4, false); const s = new S.MaterialSystem(); s.apply('m-acrylic'); const d3 = s.degradeLowPerf(8, true); return d1 === 'm-frosted' && d2 === 'm-solid' && d3 === 'm-solid'; } },
    { id: 'X13308', name: 'reduce-motion 辉光静态', check: () => m.glowStatic(true) === true && m.glowStatic(false) === false },
    ...x25(specOf(533), span(9, 25)),
  ];
}

/* -------- 族0534 数值规范令牌对稿（X13326~X13350） -------- */
export function checkX0534(): CheckEntry[] {
  return [
    { id: 'X13326', name: '字阶七档', check: () => S.TYPE_SCALE_X.pageTitle.size === 28 && S.TYPE_SCALE_X.windowTitle.size === 13 && S.TYPE_SCALE_X.rowTitle.size === 15 && S.TYPE_SCALE_X.rowSecondary.size === 12 && S.TYPE_SCALE_X.menuItem.size === 13 && S.TYPE_SCALE_X.statusBar.size === 12 },
    { id: 'X13327', name: 'CJK 行高 1.6 / 标题 1.3', check: () => S.TYPE_SCALE_X.rowTitle.lineHeight === 1.6 && S.TYPE_SCALE_X.pageTitle.lineHeight === 1.3 && S.TYPE_SCALE_X.statusBar.lineHeight === 1 },
    { id: 'X13328', name: '色彩语义 oklch 基准', check: () => S.COLOR_SEMANTICS_X.bgCanvas === 'oklch(0.14 0.02 262)' && S.COLOR_SEMANTICS_X.accent === 'oklch(0.68 0.09 262)' && S.COLOR_SEMANTICS_X.accentSoft.includes('0.16') },
    { id: 'X13329', name: '圆角语义不混用 16>12>8>4', check: () => S.RADII_X.window === 16 && S.RADII_X.card === 12 && S.RADII_X.control === 8 && S.RADII_X.chip === 4 },
    { id: 'X13330', name: '海拔 0~6 七档', check: () => S.ELEVATION_X.length === 7 && S.ELEVATION_X[6] === 'elev-6' },
    { id: 'X13331', name: '控件三密度 28/32/44', check: () => S.CONTROL_HEIGHTS_X.compact === 28 && S.CONTROL_HEIGHTS_X.standard === 32 && S.CONTROL_HEIGHTS_X.touch === 44 },
    { id: 'X13332', name: '对比度 AA 红线 4.5:1', check: () => S.aaTextOk([0, 0, 0], [255, 255, 255]) && S.aaTextOk([255, 255, 255], [128, 128, 128]) === false && S.contrastX([0, 0, 0], [255, 255, 255]) > 20 },
    ...x25(specOf(534), span(8, 25)),
  ];
}

/* -------- 族0535 设置 11 页收敛（X13351~X13375） -------- */
export function checkX0535(): CheckEntry[] {
  const nav = new S.SettingsNavModel();
  const cov = nav.coverage();
  const hits = nav.search(
    [
      { title: '屏幕亮度', desc: '调整显示', page: 'system' },
      { title: '音量合成器', desc: '应用音量', page: 'system' },
      { title: '深色模式', desc: '主题外观', page: 'personalize' },
    ],
    '音量',
  );
  return [
    { id: 'X13351', name: '导航 11 页', check: () => S.SETTINGS_PAGES.length === 11 && S.SETTINGS_PAGES[0]!.id === 'home' && S.SETTINGS_PAGES[10]!.id === 'update' },
    { id: 'X13352', name: '27 Tab 全部归位（主页为 Hero 卡无 Tab）', check: () => cov.mapped === Object.keys(S.TAB_TO_PAGE).length && cov.uncovered.length === 0 && cov.emptyPages.join() === 'home' },
    { id: 'X13353', name: 'Tab→页映射正确', check: () => nav.pageOf('Vision')?.id === 'personalize' && nav.pageOf('Perf')?.id === 'update' && nav.pageOf('A11y')?.id === 'a11y' },
    { id: 'X13354', name: '设置搜索命中跳页', check: () => hits.join() === 'system' && nav.search([{ title: 'x', desc: 'y', page: 'home' }], '').length === 0 },
    { id: 'X13355', name: '页名 28px/600 令牌', check: () => nav.pageTitleStyle().size === 28 && nav.pageTitleStyle().weight === 600 },
    ...x25(specOf(535), span(6, 25)),
  ];
}

/* -------- 族0536 开始菜单与任务栏皮（X13376~X13400） -------- */
export function checkX0536(): CheckEntry[] {
  const sm = new S.StartMenuModel();
  sm.setPinned(Array.from({ length: 30 }, (_, i) => `app${i}`));
  const tb = new S.TaskbarModel();
  tb.pin('a');
  tb.pin('a');
  tb.pin('b');
  tb.setRunning('a', true);
  tb.setRunning('b', true, true);
  return [
    { id: 'X13376', name: '已固定去重 + 分页 6×4', check: () => sm.pinned.length === 30 && sm.perPage === 24 && sm.pages === 2 },
    { id: 'X13377', name: '页指示器翻页循环', check: () => sm.pageItems().length === 24 && (sm.nextPage(), sm.page === 1 && sm.pageItems().length === 6) && (sm.nextPage(), sm.page === 0) },
    { id: 'X13378', name: '推荐区最近 ≤6', check: () => sm.recent(['1', '2', '3', '4', '5', '6', '7']).length === 6 },
    { id: 'X13379', name: '打开动效 emphasized/dur-5/锚点', check: () => { const a = sm.openAnim(); return a.ease === 'var(--ease-emphasized)' && a.dur === 'var(--dur-5)' && a.origin === 'taskbar'; } },
    { id: 'X13380', name: '任务栏居中组去重 + 托盘六件', check: () => tb.centered().join() === 'a,b' && S.TASKBAR_TRAY.length === 6 && S.TASKBAR_TRAY[5] === 'showDesktop' },
    { id: 'X13381', name: '运行指示 短点/长条', check: () => tb.indicatorOf('a') === 'short' && tb.indicatorOf('b') === 'long' },
    ...x25(specOf(536), span(7, 25)),
  ];
}

/* -------- 族0537 快捷面板与通知中心（X13401~X13425） -------- */
export function checkX0537(): CheckEntry[] {
  const qp = new S.QuickPanelModel();
  const nc = new S.NotificationCenterModel();
  nc.push({ id: 'n1', app: '邮件', title: '新邮件', body: 'x' });
  nc.push({ id: 'n2', app: '日历', title: '提醒', body: 'y' });
  nc.push({ id: 'n3', app: '邮件', title: '新邮件2', body: 'z' });
  return [
    { id: 'X13401', name: '六磁贴常量', check: () => S.QUICK_TILES.length === 6 && S.QUICK_TILES[0] === 'wifi' && S.QUICK_TILES[4] === 'dnd' },
    { id: 'X13402', name: '磁贴点亮切换', check: () => qp.toggle('wifi') === true && qp.toggle('wifi') === false },
    { id: 'X13403', name: '亮度/音量钳位 0~100', check: () => qp.setBrightness(150) === 100 && qp.setBrightness(-5) === 0 && qp.setVolume(40.6) === 41 && qp.setVolume(-1) === 0 },
    { id: 'X13404', name: '通知按应用分组', check: () => nc.groups().length === 2 && nc.groups()[0]!.notices.length === 2 },
    { id: 'X13405', name: '勿扰拦截 + 全部清除', check: () => { nc.dnd = true; const blocked = !nc.push({ id: 'n4', app: 'x', title: 'y', body: 'z' }); nc.dnd = false; return blocked && nc.suppressedByDnd === 1 && nc.clearAll() === 3 && nc.groups().length === 0; } },
    { id: 'X13406', name: '下半月历天数', check: () => nc.secondHalfDays(31) === 16 && nc.secondHalfDays(28) === 14 },
    ...x25(specOf(537), span(7, 25)),
  ];
}

/* -------- 族0538 文件管理器与桌面层（X13426~X13450） -------- */
export function checkX0538(): CheckEntry[] {
  const tb = new S.ExplorerToolbarModel();
  const addr = new S.ExplorerAddressBar();
  addr.enter(['此电脑', 'D', '项目']);
  const st = new S.ExplorerStatusModel();
  st.selected = 3;
  const flat = S.contextMenuFlat();
  return [
    { id: 'X13426', name: '工具栏九动作', check: () => S.ExplorerToolbarModel.actions().length === 9 && S.ExplorerToolbarModel.actions()[0] === 'new' && S.ExplorerToolbarModel.actions()[8] === 'more' },
    { id: 'X13427', name: '三视图切换', check: () => { tb.setView('largeIcons'); const big: string = tb.view; tb.setView('details'); const det: string = tb.view; return big === 'largeIcons' && det === 'details'; } },
    { id: 'X13428', name: '排序同键反转', check: () => { tb.sortBy('name'); const flip = tb.sortKey === 'name' && tb.asc === false; tb.sortBy('size'); return flip && tb.sortKey === 'size' && tb.asc === true; } },
    { id: 'X13429', name: '地址栏面包屑每段可下拉', check: () => addr.dropdown(1) === 'D' && addr.dropdown(9) === null && addr.breadcrumb().join('>') === '此电脑>D>项目' },
    { id: 'X13430', name: '状态栏选中数 + 容量条', check: () => st.text() === '已选 3 项' && Math.abs(st.capacityRatio() - 0.25) < 1e-9 },
    { id: 'X13431', name: '桌面右键分组 Divider', check: () => S.DESKTOP_CONTEXT_GROUPS.length === 3 && flat.filter((x) => x === '---').length === 2 && flat[flat.length - 1] === '个性化' },
    ...x25(specOf(538), span(7, 25)),
  ];
}

/* -------- 族0539 动效编排表（X13451~X13475） -------- */
export function checkX0539(): CheckEntry[] {
  return [
    { id: 'X13451', name: '七场景编排表完整', check: () => S.MOTION_SCENES.length === 7 && S.motionOf('hover') !== undefined && S.motionOf('reduceMotion') !== undefined },
    { id: 'X13452', name: 'hover 120ms 标准曲线', check: () => S.motionOf('hover')!.dur === '--dur-2' && S.motionOf('hover')!.ease === '--ease-standard' },
    { id: 'X13453', name: '浮层 emphasized+dur-5 锚点', check: () => S.motionOf('overlay')!.ease === '--ease-emphasized' && S.motionOf('overlay')!.dur === '--dur-5' },
    { id: 'X13454', name: '通知 spring 右缘 5s 消散', check: () => S.motionOf('notice')!.ease === '--ease-spring' && S.motionOf('notice')!.orchestration.includes('5s') },
    { id: 'X13455', name: 'reduce-motion 降级 80ms 线性', check: () => { const r = S.reduceMotionFallback(); return r.dur === '--dur-1' && r.ease === 'linear'; } },
    { id: 'X13456', name: '骨架换场交叉溶解禁跳变', check: () => S.motionOf('skeletonSwap')!.orchestration.includes('禁跳变') },
    ...x25(specOf(539), span(7, 25)),
  ];
}

/* -------- 族0540 手势语言（X13476~X13500） -------- */
export function checkX0540(): CheckEntry[] {
  return [
    { id: 'X13476', name: '五手势常量', check: () => S.GESTURES.length === 5 && S.GESTURES[2]!.action === '虚拟桌面切换' },
    { id: 'X13477', name: '边缘右滑通知中心', check: () => S.GESTURES.some((g) => g.id === 'edgeRightSwipe' && g.action === '通知中心') },
    { id: 'X13478', name: '触控 44px 红线', check: () => S.touchTarget('comfortable').min === 44 && !S.touchTarget('comfortable').exemption },
    { id: 'X13479', name: 'compact 豁免 40px', check: () => S.touchTarget('compact').min === 40 && S.touchTarget('compact').exemption },
    { id: 'X13480', name: '捏合缩放域限定', check: () => S.GESTURES.find((g) => g.id === 'pinchZoom')!.action.includes('缩放') },
    ...x25(specOf(540), span(6, 25)),
  ];
}
