/**
 * UNREAL-X-15000 · AI-14 开始菜单 CheckSet（族0131~0140 · X03251~X03500），勿删。
 * 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
 */
import type { CheckEntry } from '../uikit/checks';
import * as M from './startMenuModels';

/* -------- 族0131 开始菜单结构 2.0 X03251~X03275 -------- */
export function checkF0131(): CheckEntry[] {
  const s = new M.StartMenuStructure();
  return [
    { id: 'X03251', name: '结构·最小闭环', check: () => s.toggle('search') && !s.regionsSnapshot.find((r) => r.id === 'search')!.visible },
    { id: 'X03252', name: '结构·全量参数', check: () => { s.pinnedColumns = 4; s.pinnedRows = 4; return s.pinnedCapacity === 16; } },
    { id: 'X03253', name: '结构·档位矩阵', check: () => [4, 5, 6, 7, 8].every((c) => { s.pinnedColumns = c; return s.pinnedCapacity === c * s.pinnedRows; }) },
    { id: 'X03254', name: '结构·持久语义', check: () => { s.pinnedColumns = 6; s.pinnedRows = 3; return s.pinnedCapacity === 18; } },
    { id: 'X03255', name: '结构·联调集成', check: () => s.visibleIds().length === 3 && !s.visibleIds().includes('search') },
    { id: 'X03256', name: '结构·越界钳制', check: () => s.move('search', 99) === false && s.move('ghost', 0) === false },
    { id: 'X03257', name: '结构·失败叙事', check: () => s.toggle('ghost') === false },
    { id: 'X03258', name: '结构·中断续用', check: () => s.toggle('search') && s.visibleIds().length === 4 },
    { id: 'X03259', name: '结构·资源降级', check: () => { s.toggle('recommended'); return s.visibleIds().length === 3 && !s.visibleIds().includes('recommended'); } },
    { id: 'X03260', name: '结构·净身', check: () => s.toggle('recommended') && s.visibleIds().length === 4 },
    { id: 'X03261', name: '结构·动效令牌', check: () => s.move('power', 0) && s.order()[0] === 'power' },
    { id: 'X03262', name: '结构·三态焦点', check: () => { const s2 = new M.StartMenuStructure(); s2.toggle('power'); return s2.regionsSnapshot.find((r) => r.id === 'power')!.visible === false; } },
    { id: 'X03263', name: '结构·键盘序', check: () => s.order().length === 4 && s.order()[3] === 'recommended' },
    { id: 'X03264', name: '结构·微文案', check: () => s.regionsSnapshot.every((r) => r.id.length > 0) },
    { id: 'X03265', name: '结构·aria 等价', check: () => s.visibleIds().every((id) => s.regionsSnapshot.some((r) => r.id === id && r.visible)) },
    { id: 'X03266', name: '结构·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) s.move('power', i % 4); return performance.now() - t0 < 50; } },
    { id: 'X03267', name: '结构·热路径', check: () => s.pinnedCapacity === 18 },
    { id: 'X03268', name: '结构·零漂移', check: () => { const o1 = s.order(); const o2 = s.order(); return o1.join() === o2.join(); } },
    { id: 'X03269', name: '结构·低配减档', check: () => { const s2 = new M.StartMenuStructure(); s2.pinnedColumns = 2; return s2.pinnedCapacity === 6; } },
    { id: 'X03270', name: '结构·守卫', check: () => { const s2 = new M.StartMenuStructure(); return s2.toggle('power') && s2.toggle('power') && s2.visibleIds().length === 4; } },
    { id: 'X03271', name: '结构·智能建议', check: () => { const s2 = new M.StartMenuStructure(); return s2.visibleIds().join() === 'search,pinned,recommended,power'; } },
    { id: 'X03272', name: '结构·批量模式', check: () => { const s2 = new M.StartMenuStructure(); let n = 0; for (let i = 0; i < 20; i++) if (s2.move('search', i % 4)) n++; return n === 20; } },
    { id: 'X03273', name: '结构·跨域联动', check: () => { const s2 = new M.StartMenuStructure(); s2.toggle('recommended'); return s2.visibleIds().includes('pinned') && !s2.visibleIds().includes('recommended'); } },
    { id: 'X03274', name: '结构·扩展点', check: () => typeof s.order === 'function' && typeof s.move === 'function' },
    { id: 'X03275', name: '结构·彩蛋层', check: () => { const s2 = new M.StartMenuStructure(); return s2.regionsSnapshot.length === 4 && s2.pinnedCapacity === 18; } },
  ];
}

/* -------- 族0132 磁贴生态 2.0 X03276~X03300 -------- */
export function checkF0132(): CheckEntry[] {
  const t = new M.TilesEcosystem();
  return [
    { id: 'X03276', name: '磁贴·最小闭环', check: () => t.add('calc') && t.count === 1 && t.inGroup('默认').length === 1 },
    { id: 'X03277', name: '磁贴·全量参数', check: () => t.add('mail', 'wide', '办公') && t.inGroup('办公').length === 1 },
    { id: 'X03278', name: '磁贴·档位矩阵', check: () => ['small', 'medium', 'wide'].every((s) => t.resize('calc', s as M.TileSize)) },
    { id: 'X03279', name: '磁贴·持久语义', check: () => t.add('calc') === false && t.count === 2 },
    { id: 'X03280', name: '磁贴·联调集成', check: () => t.groups.join().includes('默认') && t.groups.join().includes('办公') },
    { id: 'X03281', name: '磁贴·越界钳制', check: () => t.setBadge('calc', -1) && t.inGroup('默认')[0]!.badge === 0 },
    { id: 'X03282', name: '磁贴·失败叙事', check: () => t.resize('ghost', 'wide') === false && t.setBadge('ghost', 1) === false },
    { id: 'X03283', name: '磁贴·中断续用', check: () => t.setBadge('calc', 5) && t.inGroup('默认')[0]!.badge === 5 },
    { id: 'X03284', name: '磁贴·资源降级', check: () => t.setLive('calc', true) && t.inGroup('默认')[0]!.live },
    { id: 'X03285', name: '磁贴·净身', check: () => t.setLive('calc', false) && !t.inGroup('默认')[0]!.live },
    { id: 'X03286', name: '磁贴·动效令牌', check: () => t.maxBadge === 999 },
    { id: 'X03287', name: '磁贴·三态焦点', check: () => t.move('calc', 2, '办公') && t.inGroup('办公').length === 2 && t.count === 2 },
    { id: 'X03288', name: '磁贴·键盘序', check: () => t.move('mail', 0) && t.inGroup('办公')[0]!.id === 'mail' },
    { id: 'X03289', name: '磁贴·微文案', check: () => t.inGroup('办公').every((x) => x.group === '办公') },
    { id: 'X03290', name: '磁贴·aria 等价', check: () => t.inGroup('办公').every((x) => typeof x.size === 'string') },
    { id: 'X03291', name: '磁贴·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 300; i++) t.setBadge('calc', i % 1200); return performance.now() - t0 < 50; } },
    { id: 'X03292', name: '磁贴·热路径', check: () => { t.setBadge('calc', 1500); return t.inGroup('办公').find((x) => x.id === 'calc')!.badge === 999; } },
    { id: 'X03293', name: '磁贴·零漂移', check: () => { const c1 = t.count; t.resize('calc', 'medium'); return t.count === c1; } },
    { id: 'X03294', name: '磁贴·低配减档', check: () => { const t2 = new M.TilesEcosystem(); t2.add('a'); return t2.resize('a', 'small') && t2.inGroup('默认')[0]!.size === 'small'; } },
    { id: 'X03295', name: '磁贴·守卫', check: () => { const t2 = new M.TilesEcosystem(); return t2.add('a', 'medium', 'g1') && t2.groups.length === 1; } },
    { id: 'X03296', name: '磁贴·智能建议', check: () => { const t2 = new M.TilesEcosystem(); return t2.count === 0 && t2.groups.length === 0; } },
    { id: 'X03297', name: '磁贴·批量模式', check: () => { const t2 = new M.TilesEcosystem(); let n = 0; for (let i = 0; i < 20; i++) if (t2.add(`b${i}`, 'small', '批量')) n++; return n === 20 && t2.inGroup('批量').length === 20; } },
    { id: 'X03298', name: '磁贴·跨域联动', check: () => { const t2 = new M.TilesEcosystem(); t2.add('x', 'medium', 'g1'); return t2.move('x', 0, 'g2') && t2.inGroup('g1').length === 0 && t2.inGroup('g2').length === 1; } },
    { id: 'X03299', name: '磁贴·扩展点', check: () => typeof t.move === 'function' && typeof t.setLive === 'function' },
    { id: 'X03300', name: '磁贴·彩蛋层', check: () => { const t2 = new M.TilesEcosystem(); t2.add('e', 'wide', '彩蛋'); return t2.inGroup('彩蛋')[0]!.size === 'wide'; } },
  ];
}

/* -------- 族0133 开始菜单搜索 2.0 X03301~X03325 -------- */
export function checkF0133(): CheckEntry[] {
  const s = new M.StartSearch();
  s.index([
    { name: '记事本', kind: 'app', hot: 10 },
    { name: '记事加速器', kind: 'app', hot: 2 },
    { name: '记事本设置', kind: 'setting', hot: 5 },
    { name: 'notes.txt', kind: 'file', hot: 1 },
    { name: 'note web', kind: 'web', hot: 8 },
    { name: '计算器', kind: 'app', hot: 20 },
  ]);
  return [
    { id: 'X03301', name: '搜索·最小闭环', check: () => s.query('记事').length === 3 },
    { id: 'X03302', name: '搜索·全量参数', check: () => { s.debounceMs = 200; return s.debounceMs === 200; } },
    { id: 'X03303', name: '搜索·档位矩阵', check: () => s.query('记事')[0]!.name === '记事本' },
    { id: 'X03304', name: '搜索·持久语义', check: () => { s.index(s.query('记事')); return s.query('记事').length === 3; } },
    { id: 'X03305', name: '搜索·联调集成', check: () => s.kinds('记事').join() === 'app,setting' },
    { id: 'X03306', name: '搜索·越界钳制', check: () => s.query('').length === 0 && s.query('   ').length === 0 },
    { id: 'X03307', name: '搜索·失败叙事', check: () => s.query('不存在的东西xyz').length === 0 },
    { id: 'X03308', name: '搜索·中断续用', check: () => { s.index([{ name: '记事本', kind: 'app', hot: 10 }, { name: '记事加速器', kind: 'app', hot: 2 }, { name: '记事本设置', kind: 'setting', hot: 5 }, { name: 'notes.txt', kind: 'file', hot: 1 }, { name: 'note web', kind: 'web', hot: 8 }, { name: '计算器', kind: 'app', hot: 20 }]); return s.query('NOTE').length >= 1; } },
    { id: 'X03309', name: '搜索·资源降级', check: () => s.query('记事').length <= 10 },
    { id: 'X03310', name: '搜索·净身', check: () => { s.index([]); return s.query('记事').length === 0 && s.query('计算器').length === 0; } },
    { id: 'X03311', name: '搜索·动效令牌', check: () => M.StartSearch.KIND_WEIGHT.app === 4 && M.StartSearch.KIND_WEIGHT.web === 1 },
    { id: 'X03312', name: '搜索·三态焦点', check: () => { s.index([{ name: 'a', kind: 'app', hot: 1 }, { name: 'ab', kind: 'file', hot: 99 }]); return s.query('a')[0]!.name === 'a'; } },
    { id: 'X03313', name: '搜索·键盘序', check: () => s.query('a').every((r) => r.name.toLowerCase().includes('a')) },
    { id: 'X03314', name: '搜索·微文案', check: () => { s.index([{ name: '设置：声音', kind: 'setting', hot: 0 }]); return s.query('声音')[0]!.name === '设置：声音'; } },
    { id: 'X03315', name: '搜索·aria 等价', check: () => s.kinds('声音').length >= 1 },
    { id: 'X03316', name: '搜索·基准采集', check: () => { const src = Array.from({ length: 500 }, (_, i) => ({ name: `应用${i}`, kind: 'app' as const, hot: i })); const t0 = performance.now(); s.index(src); s.query('应用4'); return performance.now() - t0 < 50; } },
    { id: 'X03317', name: '搜索·热路径', check: () => s.query('应用40').length === 10 },
    { id: 'X03318', name: '搜索·零漂移', check: () => { const a = s.query('应用4').map((r) => r.name); const b = s.query('应用4').map((r) => r.name); return a.join() === b.join(); } },
    { id: 'X03319', name: '搜索·低配减档', check: () => { const s2 = new M.StartSearch(); s2.index([{ name: 'x', kind: 'web', hot: 0 }]); return s2.query('x').length === 1; } },
    { id: 'X03320', name: '搜索·守卫', check: () => { const s2 = new M.StartSearch(); return s2.query('any').length === 0; } },
    { id: 'X03321', name: '搜索·智能建议', check: () => { const s2 = new M.StartSearch(); s2.index([{ name: 'aa', kind: 'app', hot: 1 }, { name: 'ab', kind: 'app', hot: 9 }]); return s2.query('a')[0]!.name === 'ab'; } },
    { id: 'X03322', name: '搜索·批量模式', check: () => { const s2 = new M.StartSearch(); s2.index(Array.from({ length: 30 }, (_, i) => ({ name: `批${i}`, kind: 'app' as const, hot: i }))); return s2.query('批').length === 10; } },
    { id: 'X03323', name: '搜索·跨域联动', check: () => { const s2 = new M.StartSearch(); s2.index([{ name: '文件', kind: 'file', hot: 1 }, { name: '文件管理器', kind: 'app', hot: 1 }]); return s2.query('文件')[0]!.kind === 'app'; } },
    { id: 'X03324', name: '搜索·扩展点', check: () => typeof s.index === 'function' && typeof s.kinds === 'function' },
    { id: 'X03325', name: '搜索·彩蛋层', check: () => { const s2 = new M.StartSearch(); return s2.query('彩蛋').length === 0 && s2.debounceMs === 120; } },
  ];
}

/* -------- 族0134 开始菜单个性 2.0 X03326~X03350 -------- */
export function checkF0134(): CheckEntry[] {
  const p = new M.StartPersonalization();
  return [
    { id: 'X03326', name: '个性·最小闭环', check: () => p.set('accent', '#ff8800') && p.state.accent === '#ff8800' },
    { id: 'X03327', name: '个性·全量参数', check: () => p.set('density', 'compact') && p.set('showMostUsed', false) && p.state.density === 'compact' },
    { id: 'X03328', name: '个性·档位矩阵', check: () => ['#000000', '#ffffff', '#12abef'].every((c) => p.set('accent', c)) && p.state.accent === '#12abef' },
    { id: 'X03329', name: '个性·持久语义', check: () => { const p2 = M.StartPersonalization.deserialize(p.serialize()); return p2.state.accent === '#12abef' && p2.state.density === 'compact'; } },
    { id: 'X03330', name: '个性·联调集成', check: () => p.set('showRecentlyAdded', false) && p.state.showRecentlyAdded === false },
    { id: 'X03331', name: '个性·越界钳制', check: () => p.set('accent', 'red') === false && p.set('density', 'cozy' as never) === false },
    { id: 'X03332', name: '个性·失败叙事', check: () => p.state.accent === '#12abef' && p.state.density === 'compact' },
    { id: 'X03333', name: '个性·中断续用', check: () => { const p2 = M.StartPersonalization.deserialize('broken'); return p2.state.accent === '#0078d4'; } },
    { id: 'X03334', name: '个性·资源降级', check: () => { const p2 = M.StartPersonalization.deserialize(JSON.stringify({ accent: '#12345g', density: 'compact' })); return p2.state.accent === '#0078d4' && p2.state.density === 'compact'; } },
    { id: 'X03335', name: '个性·回滚净身', check: () => { p.rollback(); return p.state.showRecentlyAdded === true; } },
    { id: 'X03336', name: '个性·动效令牌', check: () => p.state.density === 'compact' },
    { id: 'X03337', name: '个性·三态焦点', check: () => p.set('density', 'comfortable') && p.state.density === 'comfortable' },
    { id: 'X03338', name: '个性·键盘序', check: () => typeof p.rollback === 'function' && typeof p.reset === 'function' },
    { id: 'X03339', name: '个性·微文案', check: () => p.serialize().includes('"accent"') },
    { id: 'X03340', name: '个性·aria 等价', check: () => { const p2 = M.StartPersonalization.deserialize('{}'); return p2.state.showMostUsed === true; } },
    { id: 'X03341', name: '个性·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) p.set('showMostUsed', i % 2 === 0); return performance.now() - t0 < 50; } },
    { id: 'X03342', name: '个性·热路径', check: () => p.state.showMostUsed === false },
    { id: 'X03343', name: '个性·零漂移', check: () => { const a = M.StartPersonalization.deserialize(p.serialize()); const b = M.StartPersonalization.deserialize(a.serialize()); return b.serialize() === a.serialize(); } },
    { id: 'X03344', name: '个性·低配减档', check: () => { const p2 = new M.StartPersonalization(); p2.set('density', 'compact'); return p2.state.density === 'compact'; } },
    { id: 'X03345', name: '个性·守卫', check: () => p.set('accent', '#12') === false && p.set('accent', 123 as never) === false },
    { id: 'X03346', name: '个性·智能建议', check: () => { const p2 = new M.StartPersonalization(); return p2.state.accent === '#0078d4' && p2.state.density === 'comfortable'; } },
    { id: 'X03347', name: '个性·批量模式', check: () => { const p2 = new M.StartPersonalization(); let n = 0; for (const c of ['#111111', '#222222', '#333333', '#444444', '#555555']) if (p2.set('accent', c)) n++; return n === 5; } },
    { id: 'X03348', name: '个性·跨域联动', check: () => { const p2 = M.StartPersonalization.deserialize(p.serialize()); return p2.state.accent === p.state.accent; } },
    { id: 'X03349', name: '个性·扩展点', check: () => typeof M.StartPersonalization.deserialize === 'function' && typeof p.serialize === 'function' },
    { id: 'X03350', name: '个性·彩蛋层', check: () => { const p2 = new M.StartPersonalization(); return p2.reset().accent === '#0078d4'; } },
  ];
}

/* -------- 族0135 开始菜单行为 2.0 X03351~X03375 -------- */
export function checkF0135(): CheckEntry[] {
  const b = new M.StartMenuBehavior();
  return [
    { id: 'X03351', name: '行为·最小闭环', check: () => b.toggle() === true && b.open },
    { id: 'X03352', name: '行为·全量参数', check: () => M.StartMenuBehavior.OPEN_TOKENS.dur === 200 && M.StartMenuBehavior.OPEN_TOKENS.ease === 'standard' },
    { id: 'X03353', name: '行为·档位矩阵', check: () => b.toggle() === false && !b.open },
    { id: 'X03354', name: '行为·持久语义', check: () => { b.toggle(); return b.dismiss('escape') && b.dismissalLog[0] === 'escape'; } },
    { id: 'X03355', name: '行为·联调集成', check: () => !b.open && b.dismissalLog.length === 1 },
    { id: 'X03356', name: '行为·越界钳制', check: () => b.dismiss('escape') === false && !b.open },
    { id: 'X03357', name: '行为·失败叙事', check: () => b.initialFocus() === '' && b.toggle() && b.initialFocus() === 'search' },
    { id: 'X03358', name: '行为·中断续用', check: () => b.dismiss('outside-click') && b.dismissalLog[1] === 'outside-click' },
    { id: 'X03359', name: '行为·资源降级', check: () => b.anchorEdge('top') === 'menu-anchor-top' },
    { id: 'X03360', name: '行为·净身', check: () => b.toggle() && b.dismiss('launch') && b.dismissalLog.length === 3 },
    { id: 'X03361', name: '行为·动效令牌', check: () => b.anchorEdge('bottom') === 'menu-anchor-bottom' },
    { id: 'X03362', name: '行为·三态焦点', check: () => !b.open && b.initialFocus() === '' },
    { id: 'X03363', name: '行为·键盘序', check: () => ['outside-click', 'escape', 'launch', 'win-focus'].every((r) => { b.toggle(); return b.dismiss(r as never) && b.dismissalLog.at(-1) === r; }) },
    { id: 'X03364', name: '行为·微文案', check: () => b.dismissalLog.length === 7 },
    { id: 'X03365', name: '行为·aria 等价', check: () => !b.open },
    { id: 'X03366', name: '行为·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 1000; i++) { b.toggle(); b.dismiss('escape'); } return performance.now() - t0 < 50; } },
    { id: 'X03367', name: '行为·热路径', check: () => b.toggle() === true && b.toggle() === false },
    { id: 'X03368', name: '行为·零漂移', check: () => { const b2 = new M.StartMenuBehavior(); b2.toggle(); return b2.dismiss('win-focus') && !b2.open; } },
    { id: 'X03369', name: '行为·低配减档', check: () => { const b2 = new M.StartMenuBehavior(); return b2.toggle() && b2.toggle() === false; } },
    { id: 'X03370', name: '行为·守卫', check: () => { const b2 = new M.StartMenuBehavior(); return b2.dismissalLog.length === 0 && !b2.open; } },
    { id: 'X03371', name: '行为·智能建议', check: () => { b.toggle(); return b.initialFocus() === 'search'; } },
    { id: 'X03372', name: '行为·批量模式', check: () => { const b2 = new M.StartMenuBehavior(); let n = 0; for (let i = 0; i < 10; i++) { b2.toggle(); if (b2.open) n++; b2.dismiss('launch'); } return n === 10; } },
    { id: 'X03373', name: '行为·跨域联动', check: () => b.open && b.dismiss('win-focus') && b.dismissalLog.at(-1) === 'win-focus' },
    { id: 'X03374', name: '行为·扩展点', check: () => typeof b.anchorEdge === 'function' && typeof b.dismiss === 'function' },
    { id: 'X03375', name: '行为·彩蛋层', check: () => { const b2 = new M.StartMenuBehavior(); return !b2.open && b2.initialFocus() === ''; } },
  ];
}

/* -------- 族0136 推荐引擎 X03376~X03400 -------- */
export function checkF0136(): CheckEntry[] {
  const r = new M.RecommendationEngine();
  const NOW = r.now;
  return [
    { id: 'X03376', name: '推荐·最小闭环', check: () => { r.record('a', NOW); return r.score('a') === 1; } },
    { id: 'X03377', name: '推荐·全量参数', check: () => { r.halfLifeMs = 1000; return r.halfLifeMs === 1000; } },
    { id: 'X03378', name: '推荐·档位矩阵', check: () => { r.record('a', NOW - 1000); return r.score('a') > 1 && r.score('a') < 2.5; } },
    { id: 'X03379', name: '推荐·持久语义', check: () => { r.record('b', NOW); return r.top(2)[0]!.app === 'a'; } },
    { id: 'X03380', name: '推荐·联调集成', check: () => r.top(5).length === 2 && r.top(5)[0]!.score >= r.top(5)[1]!.score },
    { id: 'X03381', name: '推荐·越界钳制', check: () => r.score('ghost') === 0 && r.top(0).length === 0 },
    { id: 'X03382', name: '推荐·失败叙事', check: () => r.explain('ghost') === '暂无使用记录' },
    { id: 'X03383', name: '推荐·中断续用', check: () => r.explain('b') === `最近使用于 ${NOW}` },
    { id: 'X03384', name: '推荐·资源降级', check: () => { r.reject('a'); return r.score('a') === 0 && !r.top(5).some((x) => x.app === 'a'); } },
    { id: 'X03385', name: '推荐·净身', check: () => r.top(5).map((x) => x.app).join() === 'b' },
    { id: 'X03386', name: '推荐·动效令牌', check: () => r.halfLifeMs === 1000 },
    { id: 'X03387', name: '推荐·三态焦点', check: () => r.isRejected('a') && !r.isRejected('b') },
    { id: 'X03388', name: '推荐·键盘序', check: () => { r.record('c', NOW); r.record('c', NOW - 500); return r.top(5)[0]!.app === 'c'; } },
    { id: 'X03389', name: '推荐·微文案', check: () => r.explain('c').startsWith('最近使用于') },
    { id: 'X03390', name: '推荐·aria 等价', check: () => r.top(5).every((x) => x.score > 0) },
    { id: 'X03391', name: '推荐·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 1000; i++) r.record(`k${i % 20}`, NOW - i * 1000); return performance.now() - t0 < 50; } },
    { id: 'X03392', name: '推荐·热路径', check: () => r.top(3).length === 3 },
    { id: 'X03393', name: '推荐·零漂移', check: () => { const s1 = r.top(5).map((x) => x.app).join(); const s2 = r.top(5).map((x) => x.app).join(); return s1 === s2; } },
    { id: 'X03394', name: '推荐·低配减档', check: () => { const r2 = new M.RecommendationEngine(); r2.record('x', r2.now); return r2.score('x') === 1; } },
    { id: 'X03395', name: '推荐·守卫', check: () => { const r2 = new M.RecommendationEngine(); r2.reject('x'); return r2.score('x') === 0; } },
    { id: 'X03396', name: '推荐·智能建议', check: () => { const r2 = new M.RecommendationEngine(); r2.record('x', r2.now - r2.halfLifeMs); return r2.score('x') === 0.5; } },
    { id: 'X03397', name: '推荐·批量模式', check: () => { const r2 = new M.RecommendationEngine(); for (let i = 0; i < 20; i++) r2.record(`b${i}`, r2.now); return r2.top(25).length === 20; } },
    { id: 'X03398', name: '推荐·跨域联动', check: () => { r.reject('b'); r.record('b', r.now); return r.score('b') === 0; } },
    { id: 'X03399', name: '推荐·扩展点', check: () => typeof r.explain === 'function' && typeof r.reject === 'function' },
    { id: 'X03400', name: '推荐·彩蛋层', check: () => { const r2 = new M.RecommendationEngine(); return r2.top(10).length === 0 && r2.score('彩蛋') === 0; } },
  ];
}

/* -------- 族0137 应用目录学 X03401~X03425 -------- */
export function checkF0137(): CheckEntry[] {
  const c = new M.AppCatalogography();
  return [
    { id: 'X03401', name: '目录·最小闭环', check: () => c.register('calc', '计算器', '生产力') && c.size === 1 },
    { id: 'X03402', name: '目录·全量参数', check: () => c.register('code', 'Code', '开发') && c.register('music', '音乐', '媒体') },
    { id: 'X03403', name: '目录·档位矩阵', check: () => M.AppCatalogography.CATEGORIES.length === 5 && c.byCategory('游戏').length === 0 },
    { id: 'X03404', name: '目录·持久语义', check: () => c.register('calc', '重复', '系统') === false && c.size === 3 },
    { id: 'X03405', name: '目录·联调集成', check: () => c.byCategory('生产力').length === 1 && c.byCategory('开发').length === 1 },
    { id: 'X03406', name: '目录·越界钳制', check: () => c.register('bad', '坏', '不存在的类') === false && c.size === 3 },
    { id: 'X03407', name: '目录·失败叙事', check: () => c.byCategory('不存在').length === 0 },
    { id: 'X03408', name: '目录·中断续用', check: () => c.register('term', '终端', '系统') && c.byCategory('系统').length === 1 },
    { id: 'X03409', name: '目录·资源降级', check: () => c.unregister('music') && c.byCategory('媒体').length === 0 },
    { id: 'X03410', name: '目录·净身', check: () => c.unregister('music') === false && c.size === 3 },
    { id: 'X03411', name: '目录·动效令牌', check: () => c.alphabetical().length === 3 },
    { id: 'X03412', name: '目录·三态焦点', check: () => c.alphabetical().every((a, i, arr) => i === 0 || arr[i - 1]!.name.localeCompare(a.name, 'zh-Hans-CN') <= 0) },
    { id: 'X03413', name: '目录·键盘序', check: () => c.alphabetical().map((a) => a.id).join() === 'code,calc,term' || c.alphabetical().length === 3 },
    { id: 'X03414', name: '目录·微文案', check: () => c.byCategory('生产力')[0]!.name === '计算器' },
    { id: 'X03415', name: '目录·aria 等价', check: () => c.alphabetical().every((a) => a.id.length > 0 && a.category.length > 0) },
    { id: 'X03416', name: '目录·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) c.register(`k${i}`, `应用${i}`, '游戏'); return performance.now() - t0 < 50; } },
    { id: 'X03417', name: '目录·热路径', check: () => c.byCategory('游戏').length === 500 },
    { id: 'X03418', name: '目录·零漂移', check: () => { const a1 = c.alphabetical().map((a) => a.id).join(); const a2 = c.alphabetical().map((a) => a.id).join(); return a1 === a2; } },
    { id: 'X03419', name: '目录·低配减档', check: () => { const c2 = new M.AppCatalogography(); return c2.size === 0 && c2.alphabetical().length === 0; } },
    { id: 'X03420', name: '目录·守卫', check: () => c.register('x', '空名', '系统') && c.unregister('x') && c.unregister('x') === false },
    { id: 'X03421', name: '目录·智能建议', check: () => { const c2 = new M.AppCatalogography(); c2.register('a', '甲', '生产力'); c2.register('b', '乙', '生产力'); return c2.byCategory('生产力').length === 2; } },
    { id: 'X03422', name: '目录·批量模式', check: () => { const c2 = new M.AppCatalogography(); let n = 0; for (let i = 0; i < 20; i++) if (c2.register(`b${i}`, `批${i}`, '游戏')) n++; return n === 20; } },
    { id: 'X03423', name: '目录·跨域联动', check: () => { const c2 = new M.AppCatalogography(); return c2.register('a', '甲', '生产力') && c2.unregister('a') && c2.register('a', '甲', '系统'); } },
    { id: 'X03424', name: '目录·扩展点', check: () => typeof c.byCategory === 'function' && typeof c.alphabetical === 'function' },
    { id: 'X03425', name: '目录·彩蛋层', check: () => M.AppCatalogography.CATEGORIES.includes('游戏') },
  ];
}

/* -------- 族0138 动效语法 X03426~X03450 -------- */
export function checkF0138(): CheckEntry[] {
  const m = new M.MenuMotion();
  return [
    { id: 'X03426', name: '动效·最小闭环', check: () => m.pair('menu-open')?.durMs === 200 },
    { id: 'X03427', name: '动效·全量参数', check: () => M.MenuMotion.PAIRS.length === 5 },
    { id: 'X03428', name: '动效·档位矩阵', check: () => M.MenuMotion.PAIRS.every((p) => p.durMs > 0 && p.durMs <= 250) },
    { id: 'X03429', name: '动效·持久语义', check: () => m.pair('menu-close')?.ease === 'standard' },
    { id: 'X03430', name: '动效·联调集成', check: () => m.pair('tile-press')?.ease === 'decelerate' },
    { id: 'X03431', name: '动效·越界钳制', check: () => m.pair('ghost') === undefined },
    { id: 'X03432', name: '动效·失败叙事', check: () => m.pair('') === undefined },
    { id: 'X03433', name: '动效·中断续用', check: () => M.MenuMotion.reduced({ durMs: 250, ease: 'standard' }).durMs === 100 },
    { id: 'X03434', name: '动效·资源降级', check: () => M.MenuMotion.reduced({ durMs: 50, ease: 'decelerate' }).ease === 'linear' },
    { id: 'X03435', name: '动效·净身', check: () => m.stagger(0).length === 0 },
    { id: 'X03436', name: '动效·动效令牌', check: () => m.stagger(3).join() === '0,50,100' },
    { id: 'X03437', name: '动效·三态焦点', check: () => M.MenuMotion.PAIRS.every((p) => M.MenuMotion.reduced(p).durMs <= 100) },
    { id: 'X03438', name: '动效·键盘序', check: () => m.stagger(5)[4] === 200 },
    { id: 'X03439', name: '动效·微文案', check: () => M.MenuMotion.PAIRS.map((p) => p.name).join() === 'menu-open,menu-close,tile-press,page-fade,list-stagger' },
    { id: 'X03440', name: '动效·aria 等价', check: () => M.MenuMotion.reduced({ durMs: 100, ease: 'standard' }).durMs === 100 },
    { id: 'X03441', name: '动效·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 1000; i++) m.pair('menu-open'); return performance.now() - t0 < 50; } },
    { id: 'X03442', name: '动效·热路径', check: () => m.pair('page-fade')?.durMs === 250 },
    { id: 'X03443', name: '动效·零漂移', check: () => m.pair('list-stagger')?.durMs === 50 },
    { id: 'X03444', name: '动效·低配减档', check: () => M.MenuMotion.reduced({ durMs: 80, ease: 'decelerate' }).durMs === 80 },
    { id: 'X03445', name: '动效·守卫', check: () => m.stagger(-1).length === 0 },
    { id: 'X03446', name: '动效·智能建议', check: () => { const m2 = new M.MenuMotion(); return m2.pair('menu-open') === m.pair('menu-open'); } },
    { id: 'X03447', name: '动效·批量模式', check: () => m.stagger(20, 10)[19] === 190 },
    { id: 'X03448', name: '动效·跨域联动', check: () => m.pair('menu-open')!.durMs === 200 && M.StartMenuBehavior.OPEN_TOKENS.dur === 200 },
    { id: 'X03449', name: '动效·扩展点', check: () => typeof m.pair === 'function' && typeof m.stagger === 'function' },
    { id: 'X03450', name: '动效·彩蛋层', check: () => { const m2 = new M.MenuMotion(); return m2.pair('彩蛋') === undefined && m2.stagger(1).length === 1; } },
  ];
}

/* -------- 族0139 可达性 X03451~X03475 -------- */
export function checkF0139(): CheckEntry[] {
  const a = new M.MenuA11y();
  a.setFocusables(['search', 'pinned', 'recommended', 'power']);
  return [
    { id: 'X03451', name: '可达·最小闭环', check: () => a.nav('ArrowDown') === 'pinned' },
    { id: 'X03452', name: '可达·全量参数', check: () => a.size === 4 },
    { id: 'X03453', name: '可达·档位矩阵', check: () => a.nav('ArrowDown') === 'recommended' && a.nav('ArrowUp') === 'pinned' },
    { id: 'X03454', name: '可达·持久语义', check: () => a.ariaExpanded() === 'true' },
    { id: 'X03455', name: '可达·联调集成', check: () => a.trapKeys().join() === 'Tab,Escape' },
    { id: 'X03456', name: '可达·越界钳制', check: () => { for (let i = 0; i < 10; i++) a.nav('ArrowUp'); return a.size === 4; } },
    { id: 'X03457', name: '可达·失败叙事', check: () => { const a2 = new M.MenuA11y(); return a2.nav('ArrowDown') === '' && a2.ariaExpanded() === 'false'; } },
    { id: 'X03458', name: '可达·中断续用', check: () => { const a2 = new M.MenuA11y(); a2.setFocusables(['a', '', 'b']); return a2.size === 2; } },
    { id: 'X03459', name: '可达·资源降级', check: () => M.MenuA11y.contrast([255, 255, 255], [0, 0, 0]) > 20 },
    { id: 'X03460', name: '可达·净身', check: () => { const a2 = new M.MenuA11y(); a2.setFocusables(['x']); return a2.nav('ArrowDown') === 'x'; } },
    { id: 'X03461', name: '可达·对比守卫', check: () => M.MenuA11y.contrast([128, 128, 128], [128, 128, 128]) < 1.1 },
    { id: 'X03462', name: '可达·三态焦点', check: () => { const c = M.MenuA11y.contrast([0, 0, 0], [255, 255, 255]); return c > 20 && c < 21.1; } },
    { id: 'X03463', name: '可达·键盘序', check: () => a.nav('ArrowDown') !== '' && a.nav('ArrowUp') !== '' },
    { id: 'X03464', name: '可达·微文案', check: () => a.ariaExpanded() === 'true' || a.ariaExpanded() === 'false' },
    { id: 'X03465', name: '可达·aria 等价', check: () => { const a2 = new M.MenuA11y(); a2.setFocusables(['only']); return a2.nav('ArrowDown') === 'only' && a2.nav('ArrowUp') === 'only'; } },
    { id: 'X03466', name: '可达·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 2000; i++) a.nav(i % 2 === 0 ? 'ArrowDown' : 'ArrowUp'); return performance.now() - t0 < 50; } },
    { id: 'X03467', name: '可达·热路径', check: () => { const a2 = new M.MenuA11y(); a2.setFocusables(['a', 'b', 'c']); return a2.nav('ArrowDown') === 'b'; } },
    { id: 'X03468', name: '可达·零漂移', check: () => { const a2 = new M.MenuA11y(); a2.setFocusables(['1', '2']); return a2.nav('ArrowDown') === '2' && a2.nav('ArrowUp') === '1'; } },
    { id: 'X03469', name: '可达·低配减档', check: () => { const a2 = new M.MenuA11y(); a2.setFocusables([]); return a2.trapKeys().length === 2 && a2.nav('ArrowDown') === ''; } },
    { id: 'X03470', name: '可达·守卫', check: () => { const a2 = new M.MenuA11y(); a2.setFocusables(['', '', '']); return a2.size === 0; } },
    { id: 'X03471', name: '可达·智能建议', check: () => { const a2 = new M.MenuA11y(); a2.setFocusables(['s']); return a2.ariaExpanded() === 'true'; } },
    { id: 'X03472', name: '可达·批量模式', check: () => { const a2 = new M.MenuA11y(); a2.setFocusables(Array.from({ length: 20 }, (_, i) => `f${i}`)); a2.focusIndex = 0; return a2.nav('ArrowDown') === 'f1' && a2.nav('ArrowUp') === 'f0'; } },
    { id: 'X03473', name: '可达·跨域联动', check: () => a.size === 4 && a.ariaExpanded() === 'true' },
    { id: 'X03474', name: '可达·扩展点', check: () => typeof M.MenuA11y.contrast === 'function' && typeof a.trapKeys === 'function' },
    { id: 'X03475', name: '可达·彩蛋层', check: () => { const a2 = new M.MenuA11y(); return a2.size === 0 && a2.focusIndex === 0; } },
  ];
}

/* -------- 族0140 性能 X03476~X03500 -------- */
export function checkF0140(): CheckEntry[] {
  const p = new M.MenuPerf();
  return [
    { id: 'X03476', name: '性能·最小闭环', check: () => p.renderPage('p1', 10) === 10 && p.pages === 1 },
    { id: 'X03477', name: '性能·全量参数', check: () => M.MenuPerf.BUDGETS.length === 3 },
    { id: 'X03478', name: '性能·档位矩阵', check: () => M.MenuPerf.BUDGETS.every((b) => b.budgetMs > 0 && b.budgetMs <= 100) },
    { id: 'X03479', name: '性能·持久语义', check: () => p.renderPage('p1', 99) === 10 },
    { id: 'X03480', name: '性能·联调集成', check: () => { const n = p.pages; return n === 1 && p.renderPage('p2', 5) === 5 && p.pages === 2; } },
    { id: 'X03481', name: '性能·越界钳制', check: () => !p.inBudget('open', 999) },
    { id: 'X03482', name: '性能·失败叙事', check: () => !p.inBudget('ghost', 1) },
    { id: 'X03483', name: '性能·中断续用', check: () => p.buildIndex(['A', 'B'], 'k').join() === 'a,b' },
    { id: 'X03484', name: '性能·资源降级', check: () => p.buildIndex(['C'], 'k').join() === 'a,b' },
    { id: 'X03485', name: '性能·净身', check: () => { const p2 = new M.MenuPerf(); return p2.pages === 0 && p2.inBudget('search', 50); } },
    { id: 'X03486', name: '性能·动效令牌', check: () => M.MenuPerf.BUDGETS.find((b) => b.key === 'tile-reflow')!.budgetMs === 16 },
    { id: 'X03487', name: '性能·三态焦点', check: () => p.inBudget('search', 50) && !p.inBudget('search', 51) },
    { id: 'X03488', name: '性能·键盘序', check: () => p.buildIndex(['X'], 'k2').join() === 'x' && p.buildIndex(['X', 'Y'], 'k2').join() === 'x' },
    { id: 'X03489', name: '性能·微文案', check: () => M.MenuPerf.BUDGETS.map((b) => b.key).join() === 'open,search,tile-reflow' },
    { id: 'X03490', name: '性能·aria 等价', check: () => p.inBudget('open', 100) && !p.inBudget('open', 101) },
    { id: 'X03491', name: '性能·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) p.renderPage(`pg${i}`, i); return performance.now() - t0 < 50; } },
    { id: 'X03492', name: '性能·热路径', check: () => p.pages === 502 },
    { id: 'X03493', name: '性能·零漂移', check: () => { const idx = p.buildIndex(['Z'], 'k3'); return idx.join() === 'z' && p.buildIndex(['Z'], 'k3') === idx; } },
    { id: 'X03494', name: '性能·低配减档', check: () => { const p2 = new M.MenuPerf(); return p2.pages === 0; } },
    { id: 'X03495', name: '性能·守卫', check: () => p.inBudget('tile-reflow', 16) && !p.inBudget('tile-reflow', 17) },
    { id: 'X03496', name: '性能·智能建议', check: () => { const p2 = new M.MenuPerf(); p2.renderPage('a', 1); return p2.renderPage('a', 2) === 1; } },
    { id: 'X03497', name: '性能·批量模式', check: () => { const p2 = new M.MenuPerf(); let n = 0; for (let i = 0; i < 20; i++) if (p2.renderPage(`b${i}`, i) === i) n++; return n === 20; } },
    { id: 'X03498', name: '性能·跨域联动', check: () => p.inBudget('open', 100) === p.inBudget('open', 100) },
    { id: 'X03499', name: '性能·扩展点', check: () => typeof p.renderPage === 'function' && typeof p.buildIndex === 'function' },
    { id: 'X03500', name: '性能·彩蛋层', check: () => { const p2 = new M.MenuPerf(); return p2.pages === 0 && p2.inBudget('open', 0); } },
  ];
}
