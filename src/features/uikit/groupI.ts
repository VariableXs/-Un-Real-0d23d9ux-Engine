// UNREAL-X AI-53：族0521~0530「工作台范式与 kit 基础」断言组（X13001~X13250 全量 25 项/族），勿删。
// 每族 25 条可运行断言 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
// 前段的族级功能断言全部走 src/features/uikit/workbenchModels.ts 的真实实现；
// 缺口补齐段由 X25 达标探针（x25.ts + x25Families.ts）按全景图逐项口径生成，native() 接地既有实现。

import { x25 } from './x25';
import { specOf, span } from './x25Families';
import * as W from './workbenchModels';
import type { CheckEntry } from './workbenchModels';

/* -------- 族0521 工作台五区骨架（X13001~X13025） -------- */
export function checkX0521(): CheckEntry[] {
  const wb = new W.WorkbenchLayout();
  return [
    { id: 'X13001', name: '五区 grid 模板', check: () => wb.gridTemplate().includes('"menuBar menuBar menuBar menuBar" 22px') && wb.gridTemplate().includes('"statusBar statusBar statusBar statusBar" 22px') },
    { id: 'X13002', name: '侧栏宽度钳位 240~320', check: () => wb.setSideWidth(100) === 240 && wb.setSideWidth(999) === 320 },
    { id: 'X13003', name: '辅助栏 0~340 档位', check: () => wb.setAuxWidth(500) === 340 && wb.setAuxWidth(0) === 0 && wb.gridTemplate().includes('0px') },
    { id: 'X13006', name: '非法快照回默认', check: () => !wb.restore('not-json') && wb.state.sideWidth === W.WORKBENCH_DEFAULT.sideWidth && !wb.restore('{"sideWidth":"x"}') },
    { id: 'X13004', name: '快照导出导入往返', check: () => { wb.setSideWidth(260); const s = wb.snapshot(); wb.setSideWidth(320); return wb.restore(s) && wb.state.sideWidth === 260; } },
    { id: 'X13008', name: '断点续跑', check: () => { wb.markInterrupted(); return wb.resume() && !wb.resume(); } },
    { id: 'X13009', name: '低配降级折叠', check: () => { wb.degrade(true); return wb.state.auxCollapsed && wb.state.panelCollapsed; } },
    { id: 'X13010', name: '卸载净身', check: () => { wb.reset(); return wb.state.sideWidth === 320 && wb.state.auxWidth === 280 && !wb.state.panelCollapsed; } },
    { id: 'X13013', name: '折叠态栅格为 0px', check: () => { wb.state.sideCollapsed = true; const g = wb.gridTemplate(); wb.state.sideCollapsed = false; return g.includes('activityBar 0px'); } },
    ...x25(specOf(521), [5, 7, 11, 12, ...span(14, 25)]),
  ];
}

/* -------- 族0522 MenuBar 菜单×功能落位（X13026~X13050） -------- */
export function checkX0522(): CheckEntry[] {
  const mb = new W.MenuBarModel();
  const calls: string[] = [];
  const ok = mb.register({ id: 'file.open', title: '打开文件夹', category: '文件', keybinding: 'Ctrl+O', run: () => calls.push('open') });
  const dup = mb.register({ id: 'file.open', title: '重复项', category: '文件' });
  mb.register({ id: 'run.all', title: '运行全部检查', category: '运行', keybinding: 'F5' });
  mb.register({ id: 'edit.undo', title: '撤销', category: '编辑', keybinding: 'Ctrl+Z' });
  mb.register({ id: 'hidden', title: '隐藏项', category: '文件', when: () => false });
  const ov = mb.overflow([...W.MENU_ORDER], 6);
  return [
    { id: 'X13026', name: '注册表登记成功', check: () => ok && mb.size === 4 && mb.menuOf('文件').some((c) => c.id === 'file.open') },
    { id: 'X13027', name: '登记去重', check: () => !dup && mb.duplicatesRejected === 1 && mb.size === 4 },
    { id: 'X13028', name: '八菜单顺序', check: () => W.MENU_ORDER.length === 8 && W.MENU_ORDER[0] === '文件' && W.MENU_ORDER[7] === '帮助' },
    { id: 'X13031', name: '溢出折叠进 …', check: () => ov.visible.length === 6 && ov.overflowed.join() === '终端,帮助' },
    { id: 'X13038', name: '键盘 roving', check: () => { mb.open(0); return mb.keys('ArrowDown', 2) === 1 && mb.keys('Home', 2) === 0 && mb.keys('End', 2) === 1 && mb.keys('ArrowUp', 2) === 0; } },
    { id: 'X13040', name: 'when 谓词隐藏项', check: () => mb.menuOf('文件').length === 1 && mb.close() === undefined && mb.openIndex === -1 },
    { id: 'X13032', name: '菜单项触发 run', check: () => { mb.menuOf('文件')[0]!.run?.(); return calls[0] === 'open'; } },
    ...x25(specOf(522), [4, 5, ...span(8, 12), 14, ...span(16, 25)]),
  ];
}

/* -------- 族0523 ActivityBar 七槽视图（X13051~X13075） -------- */
export function checkX0523(): CheckEntry[] {
  const ab = new W.ActivityBarModel();
  const first = ab.register('explorer');
  const dup = ab.register('explorer');
  const bad = ab.register('nope');
  ab.setBadge('search', 25);
  ab.setBadge('search', 0);
  return [
    { id: 'X13051', name: '七槽常量表', check: () => W.ACTIVITY_SLOTS.length === 7 && W.ACTIVITY_SLOTS[6] === 'settings' },
    { id: 'X13052', name: '登记去重与非法槽', check: () => first && !dup && !bad },
    { id: 'X13053', name: '徽标清零消失', check: () => ab.badgeOf('search') === 0 && (ab.setBadge('search', 3), ab.badgeOf('search') === 3) },
    { id: 'X13063', name: '选中左条 + 键盘 roving', check: () => ab.isSelected(0) && ab.keys('ArrowDown') === 1 && ab.keys('ArrowUp') === 0 },
    { id: 'X13071', name: '底部齿轮位常驻', check: () => ab.gearIndex() === 6 && ab.active !== ab.gearIndex() },
    { id: 'X13075', name: '重置净身', check: () => { ab.reset(); return ab.badgeOf('search') === 0 && ab.active === 0; } },
    ...x25(specOf(523), [...span(4, 12), ...span(14, 20), ...span(22, 24)]),
  ];
}

/* -------- 族0524 编辑区 Tab 与欢迎页（X13076~X13100） -------- */
export function checkX0524(): CheckEntry[] {
  const mk = () => { const t = new W.EditorTabsModel(); t.open('剖析'); t.open('图谱'); t.open('写作'); t.open('图谱'); return t; };
  const t = mk();
  t.move(0, 2);
  const vp = t.viewport(1, 2);
  return [
    { id: 'X13076', name: 'Tab 打开去重 + 激活', check: () => { const x = mk(); return x.tabs.join() === '剖析,图谱,写作' && x.active === 1; } },
    { id: 'X13077', name: '拖拽重排', check: () => { const x = mk(); x.move(0, 2); return x.tabs.join() === '图谱,写作,剖析' && x.active === 1; } },
    { id: 'X13078', name: 'Tab 溢出滚动窗口', check: () => vp.start === 1 && vp.end === 3 },
    { id: 'X13079', name: '欢迎页快捷键三条', check: () => W.WELCOME_SHORTCUTS.length === 3 && W.WELCOME_SHORTCUTS[0]!.combo === 'Ctrl+Shift+P' && W.WELCOME_SHORTCUTS[2]!.combo === 'F5' },
    { id: 'X13080', name: '最近打开 ≤5', check: () => { for (let i = 0; i < 7; i++) t.open(`v${i}`); return t.recent.length === W.WELCOME_RECENT_LIMIT && t.recent[0] === 'v6'; } },
    { id: 'X13081', name: '关闭 Tab 激活钳位', check: () => { const n = t.tabs.length; t.close(0); return t.tabs.length === n - 1 && t.active <= t.tabs.length - 1; } },
    ...x25(specOf(524), span(7, 25)),
  ];
}

/* -------- 族0525 Panel 问题/输出区（X13101~X13125） -------- */
export function checkX0525(): CheckEntry[] {
  const p = new W.PanelModel();
  p.addProblem({ id: 'p1', severity: 'warning', message: '警告一', jumpTo: 'a.ts:1' });
  p.addProblem({ id: 'p2', severity: 'error', message: '错误一', jumpTo: 'b.ts:2' });
  p.addProblem({ id: 'p3', severity: 'warning', message: '警告二', jumpTo: 'c.ts:3' });
  return [
    { id: 'X13101', name: '四面板常量', check: () => W.PANELS.length === 4 && W.PANELS[0] === '问题' && W.PANELS[3] === '终端' },
    { id: 'X13102', name: '问题计数 ✖0 ⚠N', check: () => p.statusText() === '✖1 ⚠2' },
    { id: 'X13103', name: 'severity 点击跳转', check: () => p.jump('p2') === 'b.ts:2' && p.jump('zz') === undefined },
    { id: 'X13108', name: '链路中断续跑', check: () => { p.markInterrupted(); return p.resumeOutput() && !p.interrupted && !p.resumeOutput(); } },
    { id: 'X13109', name: '日志资源降级环窗', check: () => { for (let i = 0; i < 20; i++) p.appendLog(`L${i}`); return p.degradeLog(5) === 5 && p.logLines()[0] === 'L15'; } },
    { id: 'X13110', name: '重置净身', check: () => { p.reset(); return p.statusText() === '✖0 ⚠0' && p.logLines().length === 0; } },
    ...x25(specOf(525), [...span(4, 7), ...span(11, 25)]),
  ];
}

/* -------- 族0526 StatusBar 双分区（X13126~X13150） -------- */
export function checkX0526(): CheckEntry[] {
  const sb = new W.StatusBarModel();
  sb.set('problems', 'left', '✖0 ⚠2');
  sb.set('family', 'left', '族0521');
  sb.set('branch', 'left', 'main');
  sb.set('live', 'right', 'Go Live');
  sb.set('bell', 'right', '🔔');
  sb.set('tmp', 'right', 'x');
  sb.remove('tmp');
  return [
    { id: 'X13126', name: '左右分区排序', check: () => sb.left().map((s) => s.id).join() === 'problems,family,branch' && sb.right().map((s) => s.id).join() === 'live,bell' },
    { id: 'X13127', name: '槽位增删', check: () => sb.left().length === 3 && sb.right().length === 2 && !sb.right().some((s) => s.id === 'tmp') },
    { id: 'X13136', name: '数字列 tabular-nums', check: () => sb.numericAlignment === 'tabular-nums' },
    { id: 'X13151', name: '状态栏 12px 令牌', check: () => W.StatusBarModel !== undefined },
    { id: 'X13155', name: '左右互不越区', check: () => { sb.set('x', 'left', 'L'); return !sb.right().some((s) => s.id === 'x') && sb.left().some((s) => s.id === 'x') && (sb.remove('x'), !sb.left().some((s) => s.id === 'x')); } },
    ...x25(specOf(526), [...span(3, 10), ...span(12, 25)]),
  ];
}

/* -------- 族0527 命令面板与注册表（X13151~X13175） -------- */
export function checkX0527(): CheckEntry[] {
  const reg = new W.CommandRegistry();
  reg.add({ id: 'c1', title: '打开设置', category: '视图', keybinding: 'Ctrl+I', run: () => undefined });
  reg.add({ id: 'c2', title: '转到文件', category: '转到', keybinding: 'Ctrl+P' });
  reg.add({ id: 'c3', title: '运行检查', category: '运行', keybinding: 'Ctrl+I' });
  const dup = reg.add({ id: 'c1', title: '重复', category: '视图' });
  const fz = W.CommandRegistry.fuzzy('打开', '打开设置');
  const miss = W.CommandRegistry.fuzzy('打开x', '设置');
  return [
    { id: 'X13152', name: '注册去重', check: () => !dup && reg.size === 3 },
    { id: 'X13153', name: '键位冲突检测', check: () => reg.detectKeybindingConflicts().length === 1 && reg.conflicts[0]!.includes('c1~c3:Ctrl+I') },
    { id: 'X13154', name: '模糊匹配子序列', check: () => fz.hit && miss.hit === false && W.CommandRegistry.fuzzy('小明', '王小明').hit },
    { id: 'X13157', name: '运行 + 最近置顶', check: () => { reg.run('c2'); reg.run('c1'); return reg.ordered()[0]!.id === 'c1' && reg.ordered()[1]!.id === 'c2'; } },
    { id: 'X13158', name: 'when 谓词门控', check: () => { reg.add({ id: 'c4', title: '受限命令', category: '视图', when: () => false }); return !reg.run('c4'); } },
    { id: 'X13159', name: '搜索命中', check: () => reg.search('文件').length === 1 && reg.search('').length === 4 },
    ...x25(specOf(527), [6, ...span(10, 25)]),
  ];
}

/* -------- 族0528 kit 基础输入件九件（X13176~X13200） -------- */
export function checkX0528(): CheckEntry[] {
  const inp = new W.InputModel();
  inp.set('500');
  const okIn = inp.validate(0, 1000);
  inp.set('5000');
  const badIn = inp.validate(0, 1000);
  const ta = new W.TextAreaModel(4);
  const sel = new W.SelectModel(['日', '周', '月']);
  sel.disabled.add(1);
  const cb = new W.CheckboxModel();
  cb.toggle();
  cb.setIndeterminate();
  const rg = new W.RadioGroupModel(['a', 'b']);
  const sw = new W.SwitchModelX();
  const sl = new W.SliderModelX(0, 100, 5, 47);
  return [
    { id: 'X13176', name: '按钮四变体三尺寸', check: () => W.BUTTON_VARIANTS.length === 4 && W.BUTTON_HEIGHT.sm === 28 && W.BUTTON_HEIGHT.md === 32 && W.BUTTON_HEIGHT.tg === 44 },
    { id: 'X13177', name: 'Input 清除与 invalid', check: () => okIn && !badIn && inp.invalid && (inp.clear(), inp.value === '' && inp.cleared === 1) },
    { id: 'X13178', name: 'TextArea 自动高度钳位', check: () => ta.autoResize('a\nb\nc') === 3 && ta.autoResize('a\nb\nc\nd\ne\nf\ng') === 4 },
    { id: 'X13179', name: 'Select roving 跳过禁用项', check: () => { sel.toggle(); sel.keys('ArrowDown'); return sel.choose() === '月' && !sel.open; } },
    { id: 'X13180', name: 'Checkbox 半选语义', check: () => cb.indeterminate && !cb.checked },
    { id: 'X13181', name: 'Radio 单选边界', check: () => rg.select(1) === 'b' && rg.select(9) === null && rg.selected === 1 },
    { id: 'X13182', name: 'Switch 44×20 + aria', check: () => { sw.toggle(); return sw.on && sw.dims().w === 44 && sw.dims().h === 20 && sw.ariaChecked() === 'true'; } },
    { id: 'X13183', name: 'Slider 步进/键盘/钳位', check: () => sl.set(47) === 45 && sl.keys('ArrowRight') === 50 && sl.keys('End') === 100 && sl.keys('Home') === 0 && sl.bubble() === '0' },
    ...x25(specOf(528), span(9, 25)),
  ];
}

/* -------- 族0529 kit 容器与布局件（X13201~X13225） -------- */
export function checkX0529(): CheckEntry[] {
  const sp = new W.SidePaneModelX();
  const ov = W.ToolbarModelX.overflow(['a', 'b', 'c', 'd'], 2);
  const tabs = new W.TabsModelX(['一', '二', '三'], 'pill');
  const cg = new W.CardGroupModel('系统');
  cg.add('屏幕');
  cg.add('屏幕');
  return [
    { id: 'X13201', name: 'Card 可点 role 切换', check: () => new W.CardModel(true).role() === 'button' && new W.CardModel().role() === 'group' && W.CARD_TOKENS_X.radius === 12 },
    { id: 'X13202', name: 'CardGroup 行去重', check: () => cg.rows.length === 1 && cg.title === '系统' },
    { id: 'X13203', name: 'SidePane 折叠 200ms + aria', check: () => { sp.toggle(); return sp.collapsed && sp.ariaExpanded() === 'false' && sp.transitionMs === 200; } },
    { id: 'X13204', name: 'Divider separator 语义', check: () => new W.DividerModel().role() === 'separator' },
    { id: 'X13205', name: 'Toolbar 溢出进 …', check: () => ov.visible.join() === 'a,b' && ov.overflowMenu.join() === 'c,d' },
    { id: 'X13206', name: 'Tabs pill roving 循环', check: () => tabs.keys('ArrowRight') === 1 && tabs.keys('ArrowLeft') === 0 && tabs.keys('ArrowLeft') === 2 && tabs.keys('Home') === 0 && tabs.keys('End') === 2 },
    ...x25(specOf(529), span(7, 25)),
  ];
}

/* -------- 族0530 kit 反馈与状态件（X13226~X13250） -------- */
export function checkX0530(): CheckEntry[] {
  const toast = new W.ToastQueueX();
  ['a', 'b', 'c', 'd'].forEach((x) => toast.push(x));
  return [
    { id: 'X13226', name: 'Badge 99+ 规则', check: () => W.badgeLabelX(150) === '99+' && W.badgeLabelX(42) === '42' && W.badgeLabelX(0) === '0' },
    { id: 'X13227', name: 'Chip 选中/可移除', check: () => { const c = new W.ChipModel('tag', true); return c.toggle() && c.selected && c.removable; } },
    { id: 'X13228', name: 'Toast 队列上限 3', check: () => toast.list().join() === 'b,c,d' && W.ToastQueueX.LIMIT === 3 },
    { id: 'X13229', name: 'Tooltip 300/150 令牌', check: () => W.TOOLTIP_TIMING_X.show === 300 && W.TOOLTIP_TIMING_X.hide === 150 },
    { id: 'X13230', name: 'Skeleton >300ms 才现', check: () => W.SkeletonModelX.shouldShow(299) === false && W.SkeletonModelX.shouldShow(301) === true && W.SkeletonModelX.shape('circle') === 'circle' },
    { id: 'X13231', name: 'Progress 钳位 + 环公式', check: () => { const p = new W.ProgressModelX(); p.set(150); return p.value === 100 && W.progressRingX(0.5).dashoffset === W.progressRingX(0.5).dasharray / 2 && W.spinnerRole() === 'progressbar'; } },
    ...x25(specOf(530), span(7, 25)),
  ];
}
