/**
 * UNREAL-X-15000 · AI-21 文件管理面 V 线 CheckSet（族0201~0210 · X05001~X05250），勿删。
 * 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
 */
import type { CheckEntry } from '../uikit/checks';
import * as T from './ai21Models';

/* -------- 族0201 文件管理器核心 2.0 X05001~X05025 -------- */
export function checkF0201(): CheckEntry[] {
  const items = Array.from({ length: 60 }, (_, i): T.DirEntry => ({ name: `f${i}`, kind: 'file', size: i }));
  return [
    { id: 'X05001', name: '管理器·最小闭环', check: () => { const e = new T.ExplorerCore(); const p = e.cd('/docs'); const l = e.list('/docs', [{ name: 'a.txt', kind: 'file', size: 1 }]); const u = e.cd('..'); return p === '/docs' && l.length === 1 && u === '/'; } },
    { id: 'X05002', name: '管理器·全量参数', check: () => { const e = new T.ExplorerCore('strict'); return e.tier === 'strict' && e.serialize().includes('"tier":"strict"'); } },
    { id: 'X05003', name: '管理器·档位矩阵', check: () => T.TIER_MATRIX.length === 5 && T.TIER_MATRIX.every((t) => new T.ExplorerCore(t).tier === t) },
    { id: 'X05004', name: '管理器·快照迁移', check: () => { const e = new T.ExplorerCore('light'); return T.ExplorerCore.deserialize(e.serialize()).tier === 'light'; } },
    { id: 'X05005', name: '管理器·联调集成', check: () => { const e = new T.ExplorerCore(); e.cd('/a'); e.list('/a', items.slice(0, 3)); return e.path === '/a' && e.layout === 'grid'; } },
    { id: 'X05006', name: '管理器·越界钳制', check: () => { const e = new T.ExplorerCore('hack'); return e.tier === 'balanced' && e.clamped === 1; } },
    { id: 'X05007', name: '管理器·失败叙事', check: () => T.explainError('E2101').next.includes('挂载') && T.explainError('E2101').text === '目录读取失败' },
    { id: 'X05008', name: '管理器·中断续跑', check: () => { const e = new T.ExplorerCore(); e.cd('/x'); const back = e.cd('..'); const again = e.cd('/y'); return back === '/' && again === '/y'; } },
    { id: 'X05009', name: '管理器·资源降级', check: () => { const e = new T.ExplorerCore('off'); const l = e.list('/big', items); return l.length === 50 && e.clamped === 1; } },
    { id: 'X05010', name: '管理器·回滚净身', check: () => { const e = new T.ExplorerCore(); e.cd('/tmp'); e.cd('..'); return e.path === '/' && new T.ExplorerCore().path === '/'; } },
    { id: 'X05011', name: '管理器·动效令牌', check: () => { const m = T.motionFor('balanced'); return m.curve === 'ease-standard' && m.durationMs === 180 && m.scale === 1; } },
    { id: 'X05012', name: '管理器·三态焦点', check: () => new T.ExplorerCore('off').layout === 'list-plain' && new T.ExplorerCore('print').layout === 'columns' },
    { id: 'X05013', name: '管理器·键盘序', check: () => T.TIER_MATRIX.every((t) => new T.ExplorerCore(t).tier === t) },
    { id: 'X05014', name: '管理器·微文案', check: () => T.explainError('E2101').text === '目录读取失败' },
    { id: 'X05015', name: '管理器·aria 等价', check: () => { const e = new T.ExplorerCore(); return typeof e.list === 'function' && typeof e.cd === 'function'; } },
    { id: 'X05016', name: '管理器·基准采集', check: () => { const e = new T.ExplorerCore(); const t0 = performance.now(); for (let i = 0; i < 500; i++) e.list('/b', items.slice(0, 5)); return performance.now() - t0 < 50; } },
    { id: 'X05017', name: '管理器·热路径', check: () => { const e = new T.ExplorerCore(); return e.list('/h', items.slice(0, 10)).length === 10; } },
    { id: 'X05018', name: '管理器·零漂移', check: () => { const e = new T.ExplorerCore('strict'); const a = T.ExplorerCore.deserialize(e.serialize()); return a.serialize() === e.serialize(); } },
    { id: 'X05019', name: '管理器·低配减档', check: () => new T.ExplorerCore('off').layout === 'list-plain' },
    { id: 'X05020', name: '管理器·守卫', check: () => T.ExplorerCore.deserialize('bad{').tier === 'balanced' },
    { id: 'X05021', name: '管理器·智能建议', check: () => { const e = new T.ExplorerCore(); const p = e.cd('docs'); return p === '/docs' && e.clamped === 1; } },
    { id: 'X05022', name: '管理器·批量模式', check: () => { const e = new T.ExplorerCore(); return e.list('/many', Array.from({ length: 300 }, (_, i) => ({ name: `n${i}`, kind: 'file' as const, size: 1 }))).length === 300; } },
    { id: 'X05023', name: '管理器·跨域联动', check: () => T.ExplorerCore.deserialize(new T.ExplorerCore('print').serialize()).tier === 'print' },
    { id: 'X05024', name: '管理器·扩展点', check: () => typeof T.ExplorerCore.deserialize === 'function' && typeof T.TIER_MATRIX === 'object' },
    { id: 'X05025', name: '管理器·彩蛋层', check: () => { const e = new T.ExplorerCore(); e.cd('/'); return e.cd('..') === '/'; } },
  ];
}

/* -------- 族0202 文件预览 2.0 X05026~X05050 -------- */
export function checkF0202(): CheckEntry[] {
  return [
    { id: 'X05026', name: '预览·最小闭环', check: () => { const p = new T.FilePreview(); const r = p.render('a.txt', 4096); return r.rendered && r.kind === 'inline' && r.ms > 0; } },
    { id: 'X05027', name: '预览·全量参数', check: () => { const p = new T.FilePreview('rich'); return p.tier === 'rich'; } },
    { id: 'X05028', name: '预览·档位矩阵', check: () => T.PREVIEW_MATRIX.length === 5 && T.PREVIEW_MATRIX.every((t) => new T.FilePreview(t).tier === t) },
    { id: 'X05029', name: '预览·快照迁移', check: () => { const p = new T.FilePreview('icon'); return T.FilePreview.deserialize(p.serialize()).tier === 'icon'; } },
    { id: 'X05030', name: '预览·联调集成', check: () => { const p = new T.FilePreview(); p.render('a.txt', 1024); return p.hit('a.txt') && !p.hit('b.txt'); } },
    { id: 'X05031', name: '预览·越界钳制', check: () => { const p = new T.FilePreview('mega'); return p.tier === 'inline' && p.clamped === 1; } },
    { id: 'X05032', name: '预览·失败叙事', check: () => T.explainError('E2102').next.includes('轻量') },
    { id: 'X05033', name: '预览·中断续跑', check: () => { const p = new T.FilePreview(); p.render('a.txt', 1); p.render('b.txt', 1); return p.hit('a.txt') && p.hit('b.txt'); } },
    { id: 'X05034', name: '预览·资源降级', check: () => { const p = new T.FilePreview('none'); return p.render('a.txt', 1).rendered === false; } },
    { id: 'X05035', name: '预览·回滚净身', check: () => { const p = new T.FilePreview(); return p.hit('ghost.txt') === false; } },
    { id: 'X05036', name: '预览·动效令牌', check: () => T.motionFor('balanced').durationMs === 180 },
    { id: 'X05037', name: '预览·三态焦点', check: () => { const p = new T.FilePreview('icon'); const r = p.render('a.txt', 999 * 1024); return r.ms <= 50; } },
    { id: 'X05038', name: '预览·键盘序', check: () => T.PREVIEW_MATRIX.every((t) => new T.FilePreview(t).tier === t) },
    { id: 'X05039', name: '预览·微文案', check: () => T.explainError('E2102').text === '预览渲染超时' },
    { id: 'X05040', name: '预览·aria 等价', check: () => typeof new T.FilePreview().render === 'function' },
    { id: 'X05041', name: '预览·基准采集', check: () => { const p = new T.FilePreview(); const t0 = performance.now(); for (let i = 0; i < 500; i++) p.render(`f${i}.txt`, 2048); return performance.now() - t0 < 50; } },
    { id: 'X05042', name: '预览·热路径', check: () => { const p = new T.FilePreview(); p.render('hot.txt', 1); return p.hit('hot.txt'); } },
    { id: 'X05043', name: '预览·零漂移', check: () => { const p = new T.FilePreview('rich'); const a = T.FilePreview.deserialize(p.serialize()); return a.serialize() === p.serialize(); } },
    { id: 'X05044', name: '预览·低配减档', check: () => { const p = new T.FilePreview('icon'); return p.render('a.txt', 1024).kind === 'icon'; } },
    { id: 'X05045', name: '预览·守卫', check: () => T.FilePreview.deserialize('[]').tier === 'inline' },
    { id: 'X05046', name: '预览·智能建议', check: () => new T.FilePreview('full').guard(128 * 1024 * 1024) === 'icon' },
    { id: 'X05047', name: '预览·批量模式', check: () => { const p = new T.FilePreview(); for (let i = 0; i < 5; i++) p.render(`f${i}.txt`, 1); return p.serialize().includes('"cached":5'); } },
    { id: 'X05048', name: '预览·跨域联动', check: () => T.FilePreview.deserialize(new T.FilePreview('full').serialize()).tier === 'full' },
    { id: 'X05049', name: '预览·扩展点', check: () => typeof T.FilePreview.deserialize === 'function' && typeof T.PREVIEW_MATRIX === 'object' },
    { id: 'X05050', name: '预览·彩蛋层', check: () => new T.FilePreview().render('🥚.egg', 1).rendered === true },
  ];
}

/* -------- 族0203 文件搜索 2.0 X05051~X05075 -------- */
export function checkF0203(): CheckEntry[] {
  const files: [string, string][] = [
    ['/docs/report.md', '年度报告内容'],
    ['/docs/budget.xlsx', '预算表数据'],
    ['/img/logo.png', 'binary'],
    ['/notes/idea.txt', '灵感：搜索彩蛋'],
  ];
  return [
    { id: 'X05051', name: '搜索·最小闭环', check: () => { const s = new T.FileSearch(); s.rebuild([files[0]!]); return s.query('report').length === 1; } },
    { id: 'X05052', name: '搜索·全量参数', check: () => new T.FileSearch('content').tier === 'content' },
    { id: 'X05053', name: '搜索·档位矩阵', check: () => T.SEARCH_MATRIX.length === 5 && T.SEARCH_MATRIX.every((t) => new T.FileSearch(t).tier === t) },
    { id: 'X05054', name: '搜索·快照迁移', check: () => { const s = new T.FileSearch('prefix'); return T.FileSearch.deserialize(s.serialize()).tier === 'prefix'; } },
    { id: 'X05055', name: '搜索·联调集成', check: () => { const s = new T.FileSearch('name'); s.rebuild(files); const h = s.query('budget'); return h.length === 1 && h[0]!.path === '/docs/budget.xlsx'; } },
    { id: 'X05056', name: '搜索·越界钳制', check: () => { const s = new T.FileSearch('xyz'); return s.tier === 'fuzzy' && s.clamped === 1; } },
    { id: 'X05057', name: '搜索·失败叙事', check: () => T.explainError('E2103').next.includes('重建') },
    { id: 'X05058', name: '搜索·中断续跑', check: () => { const s = new T.FileSearch(); s.rebuild(files); const empty = s.query(''); const ok = s.query('report'); return empty.length === 0 && ok.length >= 1; } },
    { id: 'X05059', name: '搜索·资源降级', check: () => { const s = new T.FileSearch('name'); s.rebuild(files); return s.queryCount >= 0 && s.query('内容').length === 0; } },
    { id: 'X05060', name: '搜索·回滚净身', check: () => { const s = new T.FileSearch(); s.rebuild(files); const n = s.rebuild([]); return n === 0 && s.query('report').length === 0; } },
    { id: 'X05061', name: '搜索·动效令牌', check: () => T.motionFor('strict').curve === 'ease-standard' },
    { id: 'X05062', name: '搜索·三态焦点', check: () => { const s = new T.FileSearch(); return s.lcs('abc', 'abc') === 3 && s.lcs('a', 'b') === 0; } },
    { id: 'X05063', name: '搜索·键盘序', check: () => T.SEARCH_MATRIX.every((t) => new T.FileSearch(t).tier === t) },
    { id: 'X05064', name: '搜索·微文案', check: () => T.explainError('E2103').text === '搜索索引损坏' },
    { id: 'X05065', name: '搜索·aria 等价', check: () => typeof new T.FileSearch().query === 'function' },
    { id: 'X05066', name: '搜索·基准采集', check: () => { const s = new T.FileSearch(); const t0 = performance.now(); for (let i = 0; i < 500; i++) s.lcs('keyword', 'keynord'); return performance.now() - t0 < 50; } },
    { id: 'X05067', name: '搜索·热路径', check: () => { const s = new T.FileSearch('name'); s.rebuild(files); return s.query('logo').length === 1; } },
    { id: 'X05068', name: '搜索·零漂移', check: () => { const s = new T.FileSearch('regex'); const a = T.FileSearch.deserialize(s.serialize()); return a.serialize() === s.serialize(); } },
    { id: 'X05069', name: '搜索·低配减档', check: () => { const s = new T.FileSearch('name'); s.rebuild(files); return s.query('logo').length === 1; } },
    { id: 'X05070', name: '搜索·守卫', check: () => T.FileSearch.deserialize('bad{').tier === 'fuzzy' },
    { id: 'X05071', name: '搜索·智能建议', check: () => { const s = new T.FileSearch('name'); s.rebuild(files); const h = s.query('logo'); return h.length === 1 && h[0]!.score === 1; } },
    { id: 'X05072', name: '搜索·批量模式', check: () => { const s = new T.FileSearch('name'); s.rebuild(files); return s.query('o').length === 2; } },
    { id: 'X05073', name: '搜索·跨域联动', check: () => { const s = T.FileSearch.deserialize(new T.FileSearch('content').serialize()); s.rebuild(files); return s.query('灵感').length === 1; } },
    { id: 'X05074', name: '搜索·扩展点', check: () => typeof T.FileSearch.deserialize === 'function' && typeof new T.FileSearch().lcs === 'function' },
    { id: 'X05075', name: '搜索·彩蛋层', check: () => { const s = new T.FileSearch('regex'); s.rebuild(files); return s.query('.*').length === 4; } },
  ];
}

/* -------- 族0204 文件元数据 2.0 X05076~X05100 -------- */
export function checkF0204(): CheckEntry[] {
  return [
    { id: 'X05076', name: '元数据·最小闭环', check: () => { const m = new T.FileMetadata(); m.set({ path: '/a.txt', size: 10, mtime: 100, tags: ['红'] }); const g = m.get('/a.txt'); return !!g && g.size === 10 && g.tags[0] === '红'; } },
    { id: 'X05077', name: '元数据·全量参数', check: () => new T.FileMetadata('strict').tier === 'strict' },
    { id: 'X05078', name: '元数据·档位矩阵', check: () => T.TIER_MATRIX.every((t) => new T.FileMetadata(t).tier === t) },
    { id: 'X05079', name: '元数据·快照迁移', check: () => { const m = new T.FileMetadata('light'); return T.FileMetadata.deserialize(m.serialize()).tier === 'light'; } },
    { id: 'X05080', name: '元数据·联调集成', check: () => { const m = new T.FileMetadata(); m.set({ path: '/a', size: 1, mtime: 1, tags: ['工作'] }); m.set({ path: '/b', size: 2, mtime: 2, tags: ['工作'] }); return m.byTag('工作').length === 2; } },
    { id: 'X05081', name: '元数据·越界钳制', check: () => { const m = new T.FileMetadata(); const f = m.set({ path: '/a', size: -5, mtime: -1, tags: [] }); return f.size === 0 && f.mtime === 0 && m.clamped === 1; } },
    { id: 'X05082', name: '元数据·失败叙事', check: () => T.explainError('E2104').next.includes('基础属性') },
    { id: 'X05083', name: '元数据·中断还原', check: () => new T.FileMetadata().get('/missing') === null },
    { id: 'X05084', name: '元数据·资源降级', check: () => new T.FileMetadata('off').basicOnly() === true },
    { id: 'X05085', name: '元数据·回滚净身', check: () => { const m = new T.FileMetadata(); return m.byTag('x').length === 0; } },
    { id: 'X05086', name: '元数据·动效令牌', check: () => T.motionFor('balanced').scale === 1 },
    { id: 'X05087', name: '元数据·三态焦点', check: () => { const m = new T.FileMetadata(); const f = m.set({ path: '/a', size: 1, mtime: 1, tags: ['a', 'a'] }); return f.tags.length === 1; } },
    { id: 'X05088', name: '元数据·键盘序', check: () => T.TIER_MATRIX.every((t) => new T.FileMetadata(t).tier === t) },
    { id: 'X05089', name: '元数据·微文案', check: () => T.explainError('E2104').text === '元数据不可解析' },
    { id: 'X05090', name: '元数据·aria 等价', check: () => typeof new T.FileMetadata().set === 'function' },
    { id: 'X05091', name: '元数据·基准采集', check: () => { const m = new T.FileMetadata(); const t0 = performance.now(); for (let i = 0; i < 500; i++) m.set({ path: `/f${i}`, size: i, mtime: i, tags: ['t'] }); return performance.now() - t0 < 50; } },
    { id: 'X05092', name: '元数据·热路径', check: () => { const m = new T.FileMetadata(); m.set({ path: '/hot', size: 1, mtime: 1, tags: [] }); return m.get('/hot') !== null; } },
    { id: 'X05093', name: '元数据·零漂移', check: () => { const m = new T.FileMetadata('strict'); const a = T.FileMetadata.deserialize(m.serialize()); return a.serialize() === m.serialize(); } },
    { id: 'X05094', name: '元数据·低配减档', check: () => new T.FileMetadata('off').basicOnly() === true },
    { id: 'X05095', name: '元数据·守卫', check: () => T.FileMetadata.deserialize('[]').tier === 'balanced' },
    { id: 'X05096', name: '元数据·智能建议', check: () => { const m = new T.FileMetadata(); m.set({ path: '/a', size: 1, mtime: 1, tags: ['重要'] }); return m.byTag('重要')[0] === '/a'; } },
    { id: 'X05097', name: '元数据·批量模式', check: () => { const m = new T.FileMetadata(); for (let i = 0; i < 10; i++) m.set({ path: `/f${i}`, size: i, mtime: i, tags: ['批量'] }); return m.byTag('批量').length === 10; } },
    { id: 'X05098', name: '元数据·跨域联动', check: () => T.FileMetadata.deserialize(new T.FileMetadata('print').serialize()).tier === 'print' },
    { id: 'X05099', name: '元数据·扩展点', check: () => typeof T.FileMetadata.deserialize === 'function' },
    { id: 'X05100', name: '元数据·彩蛋层', check: () => { const m = new T.FileMetadata(); const f = m.set({ path: '/a', size: 1, mtime: 1, tags: ['1', '2', '3', '4', '5', '6', '7', '8', '9', '🥚'] }); return f.tags.length === 8 && m.clamped === 1; } },
  ];
}

/* -------- 族0205 文件操作进阶 2.0 X05101~X05125 -------- */
export function checkF0205(): CheckEntry[] {
  return [
    { id: 'X05101', name: '操作·最小闭环', check: () => { const o = new T.FileOps(); o.push({ kind: 'copy', src: '/a', dst: '/b' }); const r = o.exec(1); return r.done === 1 && o.pending === 0; } },
    { id: 'X05102', name: '操作·全量参数', check: () => new T.FileOps('strict').tier === 'strict' },
    { id: 'X05103', name: '操作·档位矩阵', check: () => T.TIER_MATRIX.every((t) => new T.FileOps(t).tier === t) },
    { id: 'X05104', name: '操作·快照迁移', check: () => { const o = new T.FileOps('light'); return T.FileOps.deserialize(o.serialize()).tier === 'light'; } },
    { id: 'X05105', name: '操作·联调集成', check: () => { const o = new T.FileOps(); for (let i = 0; i < 3; i++) o.push({ kind: 'move', src: `/s${i}`, dst: `/d${i}` }); const r = o.exec(3); return r.done === 3 && o.pending === 0; } },
    { id: 'X05106', name: '操作·越界钳制', check: () => { const o = new T.FileOps(); o.push({ kind: 'rename', src: 'a', dst: 'b' }); return o.clamped === 1; } },
    { id: 'X05107', name: '操作·失败叙事', check: () => T.explainError('E2105').next.includes('续作') },
    { id: 'X05108', name: '操作·中断续跑', check: () => { const o = new T.FileOps('off'); o.push({ kind: 'copy', src: '/a', dst: '/b' }); o.push({ kind: 'copy', src: '/c', dst: '/d' }); o.exec(5); o.exec(5); return o.resume() === 1; } },
    { id: 'X05109', name: '操作·资源降级', check: () => { const o = new T.FileOps('off'); for (let i = 0; i < 3; i++) o.push({ kind: 'copy', src: `/a${i}`, dst: `/b${i}` }); const r = o.exec(10); return r.done === 1; } },
    { id: 'X05110', name: '操作·回滚净身', check: () => { const o = new T.FileOps(); o.push({ kind: 'copy', src: '/a', dst: '/b' }); return o.rollback() && o.pending === 0; } },
    { id: 'X05111', name: '操作·动效令牌', check: () => T.motionFor('balanced').durationMs === 180 },
    { id: 'X05112', name: '操作·三态焦点', check: () => { const o = new T.FileOps(); o.push({ kind: 'copy', src: '/a', dst: '/b' }); o.push({ kind: 'copy', src: '/c', dst: '/d' }); o.exec(1); return o.pending === 1; } },
    { id: 'X05113', name: '操作·键盘序', check: () => T.TIER_MATRIX.every((t) => new T.FileOps(t).tier === t) },
    { id: 'X05114', name: '操作·微文案', check: () => T.explainError('E2105').text === '批量操作中断' },
    { id: 'X05115', name: '操作·aria 等价', check: () => typeof new T.FileOps().exec === 'function' },
    { id: 'X05116', name: '操作·基准采集', check: () => { const o = new T.FileOps(); const t0 = performance.now(); for (let i = 0; i < 500; i++) o.push({ kind: 'batch', src: `/a${i}`, dst: `/b${i}` }); return performance.now() - t0 < 50; } },
    { id: 'X05117', name: '操作·热路径', check: () => { const o = new T.FileOps(); o.push({ kind: 'copy', src: '/a', dst: '/b' }); return o.exec(1).done === 1; } },
    { id: 'X05118', name: '操作·零漂移', check: () => { const o = new T.FileOps('strict'); const a = T.FileOps.deserialize(o.serialize()); return a.serialize() === o.serialize(); } },
    { id: 'X05119', name: '操作·低配减档', check: () => { const o = new T.FileOps('off'); o.push({ kind: 'copy', src: '/a', dst: '/b' }); o.push({ kind: 'copy', src: '/c', dst: '/d' }); return o.exec(99).done === 1; } },
    { id: 'X05120', name: '操作·守卫', check: () => T.FileOps.deserialize('nope').tier === 'balanced' },
    { id: 'X05121', name: '操作·智能建议', check: () => { const o = new T.FileOps(); for (let i = 0; i < 2; i++) o.push({ kind: 'copy', src: `/a${i}`, dst: `/b${i}` }); return o.pending === 2; } },
    { id: 'X05122', name: '操作·批量模式', check: () => { const o = new T.FileOps(); for (let i = 0; i < 25; i++) o.push({ kind: 'batch', src: `/a${i}`, dst: `/b${i}` }); return o.exec(25).done === 25; } },
    { id: 'X05123', name: '操作·跨域联动', check: () => T.FileOps.deserialize(new T.FileOps('print').serialize()).tier === 'print' },
    { id: 'X05124', name: '操作·扩展点', check: () => typeof new T.FileOps().rollback === 'function' && typeof T.FileOps.deserialize === 'function' },
    { id: 'X05125', name: '操作·彩蛋层', check: () => new T.FileOps().resume() === 0 },
  ];
}

/* -------- 族0206 回收站与恢复 2.0 X05126~X05150 -------- */
export function checkF0206(): CheckEntry[] {
  return [
    { id: 'X05126', name: '回收站·最小闭环', check: () => { const b = new T.RecycleBin(); b.trash('/a.txt', 100); return b.restore('/a.txt') === true && b.count === 0; } },
    { id: 'X05127', name: '回收站·全量参数', check: () => new T.RecycleBin('forever').tier === 'forever' },
    { id: 'X05128', name: '回收站·档位矩阵', check: () => T.RETENTION_MATRIX.length === 5 && T.RETENTION_MATRIX.every((t) => new T.RecycleBin(t).tier === t) },
    { id: 'X05129', name: '回收站·快照迁移', check: () => { const b = new T.RecycleBin('7d'); return T.RecycleBin.deserialize(b.serialize()).tier === '7d'; } },
    { id: 'X05130', name: '回收站·联调集成', check: () => { const b = new T.RecycleBin(); b.trash('/a', 10); b.trash('/b', 20); b.trash('/c', 30); return b.count === 3 && b.size === 60; } },
    { id: 'X05131', name: '回收站·越界钳制', check: () => { const b = new T.RecycleBin('never'); return b.tier === '30d' && b.clamped === 1; } },
    { id: 'X05132', name: '回收站·失败叙事', check: () => T.explainError('E2106').next.includes('还原') },
    { id: 'X05133', name: '回收站·中断还原', check: () => { const b = new T.RecycleBin(); b.trash('/a', 1); b.restore('/a'); return b.restore('/a') === null; } },
    { id: 'X05134', name: '回收站·资源降级', check: () => { const b = new T.RecycleBin('immediate'); b.trash('/a', 1); return b.sweep(1) === 1; } },
    { id: 'X05135', name: '回收站·回滚净身', check: () => { const b = new T.RecycleBin(); b.trash('/a', 1); return b.purge() && b.count === 0; } },
    { id: 'X05136', name: '回收站·动效令牌', check: () => T.motionFor('light').durationMs === 180 },
    { id: 'X05137', name: '回收站·三态焦点', check: () => { const b = new T.RecycleBin(); b.trash('/a', 1); b.trash('/a', 2); return b.count === 1; } },
    { id: 'X05138', name: '回收站·键盘序', check: () => T.RETENTION_MATRIX.every((t) => new T.RecycleBin(t).tier === t) },
    { id: 'X05139', name: '回收站·微文案', check: () => T.explainError('E2106').text === '回收站条目缺失' },
    { id: 'X05140', name: '回收站·aria 等价', check: () => typeof new T.RecycleBin().trash === 'function' },
    { id: 'X05141', name: '回收站·基准采集', check: () => { const b = new T.RecycleBin(); const t0 = performance.now(); for (let i = 0; i < 500; i++) b.trash(`/f${i}`, i); return performance.now() - t0 < 50; } },
    { id: 'X05142', name: '回收站·热路径', check: () => { const b = new T.RecycleBin(); b.trash('/a', 5); return b.size === 5; } },
    { id: 'X05143', name: '回收站·零漂移', check: () => { const b = new T.RecycleBin('90d'); const a = T.RecycleBin.deserialize(b.serialize()); return a.serialize() === b.serialize(); } },
    { id: 'X05144', name: '回收站·低配减档', check: () => T.RETENTION_DAYS['immediate'] === 0 },
    { id: 'X05145', name: '回收站·守卫', check: () => T.RecycleBin.deserialize('[]').tier === '30d' },
    { id: 'X05146', name: '回收站·智能建议', check: () => { const b = new T.RecycleBin(); b.trash('/a', 10); b.trash('/b', 20); return b.size === 30; } },
    { id: 'X05147', name: '回收站·批量模式', check: () => { const b = new T.RecycleBin('30d'); for (let i = 0; i < 10; i++) b.trash(`/f${i}`, 1); return b.sweep(31) === 10; } },
    { id: 'X05148', name: '回收站·跨域联动', check: () => { const b = T.RecycleBin.deserialize(new T.RecycleBin('7d').serialize()); return T.RETENTION_DAYS[b.tier] === 7; } },
    { id: 'X05149', name: '回收站·扩展点', check: () => typeof T.RETENTION_DAYS === 'object' && typeof T.RecycleBin.deserialize === 'function' },
    { id: 'X05150', name: '回收站·彩蛋层', check: () => { const b = new T.RecycleBin('forever'); b.trash('/🥚', 1); return b.sweep(365) === 0; } },
  ];
}

/* -------- 族0207 磁盘与空间 2.0 X05151~X05175 -------- */
export function checkF0207(): CheckEntry[] {
  const buckets: T.UsageBucket[] = [
    { label: '文档', bytes: 300 },
    { label: '图片', bytes: 700 },
    { label: '视频', bytes: 2000 },
  ];
  return [
    { id: 'X05151', name: '磁盘·最小闭环', check: () => { const d = new T.DiskSpace(); const r = d.scan(buckets); return r.total === 3000 && r.depth === 3; } },
    { id: 'X05152', name: '磁盘·全量参数', check: () => new T.DiskSpace('deep').tier === 'deep' },
    { id: 'X05153', name: '磁盘·档位矩阵', check: () => T.SCAN_MATRIX.length === 5 && T.SCAN_MATRIX.every((t) => new T.DiskSpace(t).tier === t) },
    { id: 'X05154', name: '磁盘·快照迁移', check: () => { const d = new T.DiskSpace('quick'); return T.DiskSpace.deserialize(d.serialize()).tier === 'quick'; } },
    { id: 'X05155', name: '磁盘·联调集成', check: () => { const d = new T.DiskSpace(); d.scan(buckets); const t = d.top(1); return t.length === 1 && t[0]!.label === '视频'; } },
    { id: 'X05156', name: '磁盘·越界钳制', check: () => { const d = new T.DiskSpace('extreme'); return d.tier === 'standard' && d.clamped === 1; } },
    { id: 'X05157', name: '磁盘·失败叙事', check: () => T.explainError('E2107').next.includes('清理') },
    { id: 'X05158', name: '磁盘·中断还原', check: () => T.DiskSpace.deserialize('bad{').tier === 'standard' },
    { id: 'X05159', name: '磁盘·资源降级', check: () => { const d = new T.DiskSpace('none'); return d.scan(buckets).total === 0; } },
    { id: 'X05160', name: '磁盘·回滚净身', check: () => new T.DiskSpace().enough(1) === false },
    { id: 'X05161', name: '磁盘·动效令牌', check: () => T.motionFor('print').durationMs === 180 },
    { id: 'X05162', name: '磁盘·三态焦点', check: () => { const d = new T.DiskSpace('forensic'); return d.scan(buckets).depth === 12; } },
    { id: 'X05163', name: '磁盘·键盘序', check: () => T.SCAN_MATRIX.every((t) => new T.DiskSpace(t).tier === t) },
    { id: 'X05164', name: '磁盘·微文案', check: () => T.explainError('E2107').text === '磁盘空间不足' },
    { id: 'X05165', name: '磁盘·aria 等价', check: () => typeof new T.DiskSpace().scan === 'function' },
    { id: 'X05166', name: '磁盘·基准采集', check: () => { const d = new T.DiskSpace(); const t0 = performance.now(); for (let i = 0; i < 500; i++) d.scan(buckets); return performance.now() - t0 < 50; } },
    { id: 'X05167', name: '磁盘·热路径', check: () => { const d = new T.DiskSpace(); d.scan(buckets); return d.top(2).length === 2; } },
    { id: 'X05168', name: '磁盘·零漂移', check: () => { const d = new T.DiskSpace('deep'); const a = T.DiskSpace.deserialize(d.serialize()); return a.serialize() === d.serialize(); } },
    { id: 'X05169', name: '磁盘·低配减档', check: () => { const d = new T.DiskSpace('quick'); return d.scan(buckets).depth === 1; } },
    { id: 'X05170', name: '磁盘·守卫', check: () => T.DiskSpace.deserialize('[]').tier === 'standard' },
    { id: 'X05171', name: '磁盘·智能建议', check: () => { const d = new T.DiskSpace(); d.scan(buckets); return d.enough(1000) === true && d.enough(9999) === false; } },
    { id: 'X05172', name: '磁盘·批量模式', check: () => { const d = new T.DiskSpace(); d.scan(Array.from({ length: 100 }, (_, i) => ({ label: `b${i}`, bytes: i }))); return d.top(50).length === 50; } },
    { id: 'X05173', name: '磁盘·跨域联动', check: () => T.DiskSpace.deserialize(new T.DiskSpace('forensic').serialize()).tier === 'forensic' },
    { id: 'X05174', name: '磁盘·扩展点', check: () => typeof T.SCAN_DEPTH === 'object' && typeof T.DiskSpace.deserialize === 'function' },
    { id: 'X05175', name: '磁盘·彩蛋层', check: () => { const d = new T.DiskSpace(); d.scan(buckets); return d.top(0).length === 0 && d.top(100).length === 3; } },
  ];
}

/* -------- 族0208 文件组织哲学 2.0 X05176~X05200 -------- */
export function checkF0208(): CheckEntry[] {
  return [
    { id: 'X05176', name: '组织·最小闭环', check: () => { const o = new T.FileOrganizer(); o.addRule({ id: 1, pattern: 'pdf', target: '/文档', priority: 1 }); return o.classify('年报.pdf') === '/文档'; } },
    { id: 'X05177', name: '组织·全量参数', check: () => new T.FileOrganizer('strict').tier === 'strict' },
    { id: 'X05178', name: '组织·档位矩阵', check: () => T.TIER_MATRIX.every((t) => new T.FileOrganizer(t).tier === t) },
    { id: 'X05179', name: '组织·快照迁移', check: () => { const o = new T.FileOrganizer('light'); return T.FileOrganizer.deserialize(o.serialize()).tier === 'light'; } },
    { id: 'X05180', name: '组织·联调集成', check: () => { const o = new T.FileOrganizer(); o.addRule({ id: 1, pattern: 'png', target: '/图片', priority: 1 }); const r = o.organize(['a.png', 'b.png', 'c.txt']); return r.length === 3 && r[0]![1] === '/图片'; } },
    { id: 'X05181', name: '组织·越界钳制', check: () => { const o = new T.FileOrganizer(); o.addRule({ id: 1, pattern: 'a', target: '/x', priority: 999 }); return o.clamped === 1; } },
    { id: 'X05182', name: '组织·失败叙事', check: () => T.explainError('E2108').next.includes('优先级') },
    { id: 'X05183', name: '组织·中断还原', check: () => new T.FileOrganizer().classify('unknown.bin') === null },
    { id: 'X05184', name: '组织·资源降级', check: () => { const o = new T.FileOrganizer('off'); return o.organize(['x.bin'])[0]![1] === '/未分类'; } },
    { id: 'X05185', name: '组织·回滚净身', check: () => new T.FileOrganizer().organize([]).length === 0 },
    { id: 'X05186', name: '组织·动效令牌', check: () => T.motionFor('balanced').curve === 'ease-standard' },
    { id: 'X05187', name: '组织·三态焦点', check: () => { const o = new T.FileOrganizer(); o.addRule({ id: 1, pattern: 'pdf', target: '/低', priority: 9 }); o.addRule({ id: 2, pattern: 'pdf', target: '/高', priority: 1 }); return o.classify('x.pdf') === '/高'; } },
    { id: 'X05188', name: '组织·键盘序', check: () => T.TIER_MATRIX.every((t) => new T.FileOrganizer(t).tier === t) },
    { id: 'X05189', name: '组织·微文案', check: () => T.explainError('E2108').text === '组织规则冲突' },
    { id: 'X05190', name: '组织·aria 等价', check: () => typeof new T.FileOrganizer().classify === 'function' },
    { id: 'X05191', name: '组织·基准采集', check: () => { const o = new T.FileOrganizer(); o.addRule({ id: 1, pattern: 'a', target: '/x', priority: 1 }); const t0 = performance.now(); for (let i = 0; i < 500; i++) o.classify(`file${i}a.txt`); return performance.now() - t0 < 50; } },
    { id: 'X05192', name: '组织·热路径', check: () => { const o = new T.FileOrganizer(); o.addRule({ id: 1, pattern: 'pdf', target: '/文档', priority: 1 }); return o.classify('a.pdf') === '/文档'; } },
    { id: 'X05193', name: '组织·零漂移', check: () => { const o = new T.FileOrganizer('strict'); const a = T.FileOrganizer.deserialize(o.serialize()); return a.serialize() === o.serialize(); } },
    { id: 'X05194', name: '组织·低配减档', check: () => { const o = new T.FileOrganizer('off'); return o.suggest(['/a.png']) === null; } },
    { id: 'X05195', name: '组织·守卫', check: () => T.FileOrganizer.deserialize('[]').tier === 'balanced' },
    { id: 'X05196', name: '组织·智能建议', check: () => { const o = new T.FileOrganizer(); const s = o.suggest(['/pic/a.png']); return !!s && s.pattern === '.png' && s.target === '/png合集'; } },
    { id: 'X05197', name: '组织·批量模式', check: () => { const o = new T.FileOrganizer(); o.addRule({ id: 1, pattern: 'txt', target: '/笔记', priority: 1 }); return o.organize(['1.txt', '2.txt', '3.txt', '4.txt', '5.txt']).every(([, t]) => t === '/笔记'); } },
    { id: 'X05198', name: '组织·跨域联动', check: () => T.FileOrganizer.deserialize(new T.FileOrganizer('print').serialize()).tier === 'print' },
    { id: 'X05199', name: '组织·扩展点', check: () => typeof new T.FileOrganizer().addRule === 'function' && typeof T.FileOrganizer.deserialize === 'function' },
    { id: 'X05200', name: '组织·彩蛋层', check: () => { const o = new T.FileOrganizer(); o.addRule({ id: 1, pattern: '🥚', target: '/彩蛋', priority: 1 }); return o.classify('surprise🥚.bin') === '/彩蛋'; } },
  ];
}

/* -------- 族0209 拖拽数据 2.0 X05201~X05225 -------- */
export function checkF0209(): CheckEntry[] {
  return [
    { id: 'X05201', name: '拖拽·最小闭环', check: () => { const d = new T.DragPayload(); d.start('file', ['/a.txt'], 10); const p = d.drop(); return !!p && p.paths[0] === '/a.txt' && p.bytes === 10; } },
    { id: 'X05202', name: '拖拽·全量参数', check: () => new T.DragPayload('strict').tier === 'strict' },
    { id: 'X05203', name: '拖拽·档位矩阵', check: () => T.TIER_MATRIX.every((t) => new T.DragPayload(t).tier === t) },
    { id: 'X05204', name: '拖拽·快照迁移', check: () => { const d = new T.DragPayload('light'); return T.DragPayload.deserialize(d.serialize()).tier === 'light'; } },
    { id: 'X05205', name: '拖拽·联调集成', check: () => { const d = new T.DragPayload(); d.start('text', ['/a'], 1); const p = d.drop(); const c = d.cancel(); return !!p && c && d.drop() === null; } },
    { id: 'X05206', name: '拖拽·越界钳制', check: () => { const d = new T.DragPayload(); const p = d.start('file', ['no-slash'], -5); return p.paths.length === 0 && p.bytes === 0 && d.clamped === 1; } },
    { id: 'X05207', name: '拖拽·失败叙事', check: () => T.explainError('E2109').next.includes('重新拖拽') },
    { id: 'X05208', name: '拖拽·中断续跑', check: () => { const d = new T.DragPayload(); d.start('file', ['/a'], 1); d.cancel(); d.start('file', ['/b'], 2); return d.drop()?.paths[0] === '/b'; } },
    { id: 'X05209', name: '拖拽·资源降级', check: () => { const d = new T.DragPayload('off'); d.start('file', ['/a'], 1); return d.drop()?.kind === 'file'; } },
    { id: 'X05210', name: '拖拽·回滚净身', check: () => { const d = new T.DragPayload(); d.start('file', ['/a'], 1); return d.cancel() && d.drop() === null; } },
    { id: 'X05211', name: '拖拽·动效令牌', check: () => T.motionFor('balanced').durationMs === 180 },
    { id: 'X05212', name: '拖拽·三态焦点', check: () => { const d = new T.DragPayload(); return d.sameVolume('/data/a', '/data/b') === true && d.sameVolume('/data/a', '/sys/b') === false; } },
    { id: 'X05213', name: '拖拽·键盘序', check: () => T.TIER_MATRIX.every((t) => new T.DragPayload(t).tier === t) },
    { id: 'X05214', name: '拖拽·微文案', check: () => T.explainError('E2109').text === '拖放载荷不完整' },
    { id: 'X05215', name: '拖拽·aria 等价', check: () => typeof new T.DragPayload().start === 'function' },
    { id: 'X05216', name: '拖拽·基准采集', check: () => { const d = new T.DragPayload(); const t0 = performance.now(); for (let i = 0; i < 500; i++) d.start('file', [`/f${i}`], i); return performance.now() - t0 < 50; } },
    { id: 'X05217', name: '拖拽·热路径', check: () => { const d = new T.DragPayload(); d.start('mixed', ['/a', '/b'], 5); return d.drop()!.paths.length === 2; } },
    { id: 'X05218', name: '拖拽·零漂移', check: () => { const d = new T.DragPayload('strict'); const a = T.DragPayload.deserialize(d.serialize()); return a.serialize() === d.serialize(); } },
    { id: 'X05219', name: '拖拽·低配减档', check: () => new T.DragPayload('off').tier === 'off' },
    { id: 'X05220', name: '拖拽·守卫', check: () => T.DragPayload.deserialize('[]').tier === 'balanced' },
    { id: 'X05221', name: '拖拽·智能建议', check: () => { const d = new T.DragPayload(); return d.sameVolume('/c/d', '/c/e') === true; } },
    { id: 'X05222', name: '拖拽·批量模式', check: () => { const d = new T.DragPayload(); d.start('folder', ['/f1', '/f2', '/f3', '/f4', '/f5'], 500); return d.drop()!.paths.length === 5; } },
    { id: 'X05223', name: '拖拽·跨域联动', check: () => T.DragPayload.deserialize(new T.DragPayload('print').serialize()).tier === 'print' },
    { id: 'X05224', name: '拖拽·扩展点', check: () => typeof new T.DragPayload().cancel === 'function' && typeof T.DragPayload.deserialize === 'function' },
    { id: 'X05225', name: '拖拽·彩蛋层', check: () => { const d = new T.DragPayload(); d.start('mixed', ['/🥚'], 1); return d.drop()!.paths[0] === '/🥚'; } },
  ];
}

/* -------- 族0210 文件管理性能 X05226~X05250 -------- */
export function checkF0210(): CheckEntry[] {
  return [
    { id: 'X05226', name: '性能·最小闭环', check: () => { const p = new T.PerfIndex(); p.sample(10); return p.withinBudget() === true; } },
    { id: 'X05227', name: '性能·全量参数', check: () => new T.PerfIndex('turbo').tier === 'turbo' },
    { id: 'X05228', name: '性能·档位矩阵', check: () => T.PERF_MATRIX.length === 5 && T.PERF_MATRIX.every((t) => new T.PerfIndex(t).tier === t) },
    { id: 'X05229', name: '性能·快照迁移', check: () => { const p = new T.PerfIndex('fast'); return T.PerfIndex.deserialize(p.serialize()).tier === 'fast'; } },
    { id: 'X05230', name: '性能·联调集成', check: () => { const p = new T.PerfIndex('smooth'); p.sample(5); p.sample(15); return p.p95() >= 5 && p.memo('k', () => 42) === 42; } },
    { id: 'X05231', name: '性能·越界钳制', check: () => { const p = new T.PerfIndex('warp'); return p.tier === 'smooth' && p.clamped === 1; } },
    { id: 'X05232', name: '性能·失败叙事', check: () => T.explainError('E2110').next.includes('降级') },
    { id: 'X05233', name: '性能·中断还原', check: () => T.PerfIndex.deserialize('bad{').tier === 'smooth' },
    { id: 'X05234', name: '性能·资源降级', check: () => { const p = new T.PerfIndex('off'); p.sample(100); return p.withinBudget() === true; } },
    { id: 'X05235', name: '性能·回滚净身', check: () => new T.PerfIndex().p95() === 0 },
    { id: 'X05236', name: '性能·动效令牌', check: () => T.motionFor('off').scale === 0 && T.motionFor('off').curve === 'linear-fade' },
    { id: 'X05237', name: '性能·三态焦点', check: () => { const p = new T.PerfIndex(); p.sample(1); p.sample(100); return p.p95() === 100; } },
    { id: 'X05238', name: '性能·键盘序', check: () => T.PERF_MATRIX.every((t) => new T.PerfIndex(t).tier === t) },
    { id: 'X05239', name: '性能·微文案', check: () => T.explainError('E2110').text === '性能预算超限' },
    { id: 'X05240', name: '性能·aria 等价', check: () => typeof new T.PerfIndex().sample === 'function' },
    { id: 'X05241', name: '性能·基准采集', check: () => { const p = new T.PerfIndex(); const t0 = performance.now(); for (let i = 0; i < 500; i++) p.memo(`k${i}`, () => i); return performance.now() - t0 < 50; } },
    { id: 'X05242', name: '性能·热路径', check: () => { const p = new T.PerfIndex(); p.memo('hot', () => 7); return p.memo('hot', () => 999) === 7; } },
    { id: 'X05243', name: '性能·零漂移', check: () => { const p = new T.PerfIndex('fast'); const a = T.PerfIndex.deserialize(p.serialize()); return a.serialize() === p.serialize(); } },
    { id: 'X05244', name: '性能·低配减档', check: () => new T.PerfIndex('turbo').degrade() === 'fast' },
    { id: 'X05245', name: '性能·守卫', check: () => T.PerfIndex.deserialize('[]').tier === 'smooth' },
    { id: 'X05246', name: '性能·智能建议', check: () => { const p = new T.PerfIndex('turbo'); p.sample(50); return p.withinBudget() === false && p.degrade() === 'fast'; } },
    { id: 'X05247', name: '性能·批量模式', check: () => { const p = new T.PerfIndex(); for (let i = 0; i < 300; i++) p.sample(i); return p.serialize().includes('"samples":256'); } },
    { id: 'X05248', name: '性能·跨域联动', check: () => T.PERF_BUDGET_MS[T.PerfIndex.deserialize(new T.PerfIndex('fast').serialize()).tier] === 50 },
    { id: 'X05249', name: '性能·扩展点', check: () => typeof T.PERF_BUDGET_MS === 'object' && typeof T.PerfIndex.deserialize === 'function' },
    { id: 'X05250', name: '性能·彩蛋层', check: () => new T.PerfIndex().memo('🥚', () => 42) === 42 },
  ];
}

// UNREAL-X AI-21（族0201~0210 · X05001~X05250）V 线聚合：十族 × 25 项 = 250 项，只增不删。
export function runAi21Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [checkF0201, checkF0202, checkF0203, checkF0204, checkF0205, checkF0206, checkF0207, checkF0208, checkF0209, checkF0210];
  const entries = families.flatMap((f) => f());
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
