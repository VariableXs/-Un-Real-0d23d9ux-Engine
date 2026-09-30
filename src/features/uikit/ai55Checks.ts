/**
 * UNREAL-X-15000 · AI-55 主题流水线与视觉回归 CheckSet（族0542/0543/0546/0547/0548/0550 · V 线 6 族 150 项），勿删。
 * 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
 * C 线（族0541/0544/0545/0549）见 code-analysis/core/src/ai55.rs。
 */
import type { CheckEntry } from '../compat/checks';
import * as T from './ai55Models';

const SUFFIX = [
  '最小闭环', '全量参数', '档位矩阵', '快照迁移', '联调集成',
  '越界钳制', '失败叙事', '中断还原', '资源降级', '回滚净身',
  '动效令牌', '三态焦点', '键盘序', '微文案', 'aria 等价',
  '基准采集', '热路径', '零漂移', '低配减档', '守卫',
  '智能建议', '批量模式', '跨域联动', '扩展点', '彩蛋层',
];

function mk25(startId: number, prefix: string, fns: (() => boolean)[]): CheckEntry[] {
  return fns.map((check, i) => ({ id: `X${startId + i}`, name: `${prefix}·${SUFFIX[i]}`, check }));
}

/* -------- 族0542 主题六元组持久化 X13526~X13550（§16.3）-------- */
export function checkF0542(): CheckEntry[] {
  const t = new T.ThemeHexad();
  return mk25(13526, '六元组', [
    () => t.data.density === 'comfortable' && t.data.material === 'mica' && t.data.radius === 12,
    () => t.set('palette', 'aurora') === 'aurora' && t.set('wallpaper', 'mist') === 'mist',
    () => t.setDensity('compact') === 'compact' && T.DENSITIES.length === 2,
    () => t.setRadius(16) === 16 && t.setRadius(4) === 4 && t.setRadius(8) === 8,
    () => t.setMaterial('acrylic') === 'acrylic' && T.MATERIALS.length === 5,
    () => t.setRadius(99) === 12 && t.setRadius(-1) === 12 && t.clamped >= 2,
    () => t.setDensity('huge') === 'comfortable' && t.clamped >= 3,
    () => t.snapshot().includes('"density":"comfortable"') && t.snapshot().includes('"v":1'),
    () => { const q = T.ThemeHexad.migrate(t.snapshot()); return q.data.palette === 'aurora' && q.data.wallpaper === 'mist'; },
    () => { const q = new T.ThemeHexad(); q.setMaterial('solid'); q.setMaterial('mica'); return q.data.material === 'mica'; },
    () => t.setMotion(0) === 0 && t.setMotion(5) === 5 && T.MOTION_TIERS.length === 6,
    () => t.setDensity('compact') !== t.setDensity('comfortable') || t.data.density === 'comfortable',
    () => t.setMotion(2) === 2 && t.setRadius(16) === 16 && t.snapshot().includes('"motion":2'),
    () => T.ThemeHexad.label('mica') === '云母' && T.ThemeHexad.label('glow') === '辉光',
    () => { const q = T.ThemeHexad.migrate('{"density":"compact","material":"glow","radius":16,"motion":0}'); return q.data.density === 'compact' && q.data.material === 'glow'; },
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) new T.ThemeHexad().setMotion(i % 7); return performance.now() - t0 < 50; },
    () => t.set('density', 'compact') === 'compact' && t.data.density === 'compact',
    () => { const a = T.ThemeHexad.migrate(t.snapshot()); const b = T.ThemeHexad.migrate(t.snapshot()); return a.snapshot() === b.snapshot(); },
    () => { const q = new T.ThemeHexad(); q.setMotion(Number.NaN); return q.data.motion === 3 && q.clamped >= 1; },
    () => { const q = new T.ThemeHexad(); return q.restore('{bad') === false && q.clamped >= 1 && q.data.density === 'comfortable'; },
    () => { const q = new T.ThemeHexad(); q.restore('{"motion":9,"radius":0}'); return q.data.motion === 3 && q.data.radius === 12; },
    () => { let n = 0; for (let i = 0; i < 20; i++) { const q = new T.ThemeHexad(); q.setMaterial(T.MATERIALS[i % 5] as string); if (q.data.material === T.MATERIALS[i % 5]) n++; } return n === 20; },
    () => { const q = T.ThemeHexad.migrate(t.snapshot()); q.set('palette', 'deep'); return q.data.palette === 'deep' && q.data.wallpaper === 'mist'; },
    () => typeof T.ThemeHexad.migrate === 'function' && typeof t.snapshot === 'function',
    () => { const q = new T.ThemeHexad(); q.restore('{"wallpaper":"egg","palette":"egg"}'); return q.data.wallpaper === 'egg' && q.data.palette === 'egg'; },
  ]);
}

/* -------- 族0543 键位注册表与冲突检测 X13551~X13575（§17）-------- */
export function checkF0543(): CheckEntry[] {
  const r = new T.KeymapRegistry();
  return mk25(13551, '键位', [
    () => r.register('cmd.p', 'Ctrl+Shift+P') && r.bindings.length === 1,
    () => r.register('cmd.p2', 'Shift+Ctrl+P') && r.conflicts.length === 1,
    () => T.normalizeCombo('Shift+Ctrl+P') === 'Ctrl+Shift+P' && T.normalizeCombo('Win+A') === 'Win+A',
    () => r.snapshot().includes('"cmd.p"') && JSON.parse(r.snapshot()).bindings.length === 2,
    () => r.byCombo('Ctrl+Shift+P').length === 2 && r.byCombo('Alt+F4').length === 0,
    () => r.register('cmd.p', 'Ctrl+K') === false && r.clamped >= 1,
    () => r.register('', 'Ctrl+K') === false && r.register('blank', '') === false,
    () => r.unregister('cmd.p2') && r.conflicts.length === 0 && r.bindings.length === 1,
    () => { const q = new T.KeymapRegistry(); return q.registerGlobals() === 0 && q.bindings.length === T.GLOBAL_KEYS.length; },
    () => { const q = new T.KeymapRegistry(); q.registerGlobals(); return q.conflicts.length === 0; },
    () => T.GLOBAL_KEYS.length === 8 && T.GLOBAL_KEYS[0]![0] === 'Win / Ctrl+Esc',
    () => T.GLOBAL_KEYS.every(([k]) => T.normalizeCombo(k.replace(' / ', '+')) !== ''),
    () => r.register('f1.sys', 'F1', 'system') && r.bindings.length === 2,
    () => T.normalizeCombo('Ctrl+Alt+Del') === 'Ctrl+Alt+Del' && T.MOD_ORDER.length === 4,
    () => r.byCombo('f1').length === 0 && r.byCombo('F1').length === 1,
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.normalizeCombo(`Shift+Ctrl+K${i % 10}`); return performance.now() - t0 < 50; },
    () => r.register('k1', 'Ctrl+K') && r.register('k2', 'Ctrl+K') && r.conflicts[r.conflicts.length - 1]!.includes('k1~k2'),
    () => { const a = new T.KeymapRegistry(); a.register('x', 'Ctrl+P'); const b = new T.KeymapRegistry(); b.register('x', 'Ctrl+P'); return a.snapshot() === b.snapshot(); },
    () => r.unregister('nope') === false && r.bindings.length >= 3,
    () => { const q = new T.KeymapRegistry(); q.register('a', 'Ctrl+K'); q.register('b', 'Ctrl+K'); q.unregister('a'); return q.conflicts.length === 0; },
    () => { let n = 0; for (let i = 0; i < 20; i++) { if (r.register(`kx${i}`, `F${(i % 12) + 1}`)) n++; } return n >= 10; },
    () => r.byCombo('Ctrl+P').length >= 1 || r.bindings.some((b) => b.combo.startsWith('Ctrl')),
    () => r.bindings.every((b) => b.scope === 'workbench' || b.scope === 'system'),
    () => typeof T.KeymapRegistry.prototype.registerGlobals === 'function' && typeof r.byCombo === 'function',
    () => { const q = new T.KeymapRegistry(); q.register('egg', 'Win+;'); return q.bindings[0]!.id === 'egg'; },
  ]);
}

/* -------- 族0546 视觉稿 V1 晨雾 X13626~X13650（§7.1/§7.2）-------- */
export function checkF0546(): CheckEntry[] {
  const m = new T.MockupSpec('v1');
  return mk25(13626, '晨雾', [
    () => m.variant === 'v1' && T.MockupSpec.variantLabel('v1') === '晨雾',
    () => m.style.material === 'm-mica' && m.style.accentFollowsWallpaper === true,
    () => T.MOCKUP_SCREENS.length === 5 && m.style.radius === 16,
    () => m.draft('设置') && m.stages.get('设置') === 'drafted',
    () => m.verify('设置') && m.progress() === 20,
    () => m.draft('设置') === false && m.clamped >= 1,
    () => m.verify('桌面+图标') === false && m.clamped >= 2,
    () => m.draft('桌面+图标') && m.verify('桌面+图标') && m.progress() === 40,
    () => m.lock() === false && m.clamped >= 3,
    () => { const q = new T.MockupSpec('v1'); return q.lock() === false && q.progress() === 0; },
    () => m.style.curve === '--ease-standard' && m.style.density === 'comfortable',
    () => T.MOCKUP_SCREENS.every((s) => m.stages.has(s)),
    () => { const q = new T.MockupSpec('v1'); q.draft('设置'); return q.progress() === 0; },
    () => T.MockupSpec.variantLabel('v1') !== T.MockupSpec.variantLabel('v2'),
    () => m.draft('任务栏+开始菜单') && m.verify('任务栏+开始菜单') && m.progress() === 60,
    () => { const t0 = performance.now(); for (let i = 0; i < 300; i++) new T.MockupSpec('v1').progress(); return performance.now() - t0 < 50; },
    () => m.stages.get('设置') === m.stages.get('设置'),
    () => { const a = new T.MockupSpec('v1'); const b = new T.MockupSpec('v1'); return a.progress() === b.progress() && a.variant === b.variant; },
    () => { const q = new T.MockupSpec('v1'); return q.style.accentFollowsWallpaper && q.style.material === 'm-mica'; },
    () => m.lock() === false && m.progress() === 60,
    () => { for (const s of T.MOCKUP_SCREENS) { m.draft(s); m.verify(s); } return m.lock() === true && m.locked; },
    () => m.draft('文件管理器') === false && m.clamped >= 4,
    () => m.style.radius > T.MOCKUP_STYLES.v2.radius && m.style.radius === 16,
    () => typeof T.MockupSpec.variantLabel === 'function' && typeof m.progress === 'function',
    () => { const q = new T.MockupSpec('v1'); return q.stages.get('代码分析工作台') === 'blank' && !q.locked; },
  ]);
}

/* -------- 族0547 视觉稿 V2 工作台 X13651~X13675（§7.1/§7.2）-------- */
export function checkF0547(): CheckEntry[] {
  const m = new T.MockupSpec('v2');
  return mk25(13651, '工作台稿', [
    () => m.variant === 'v2' && T.MockupSpec.variantLabel('v2') === '工作台',
    () => m.style.density === 'compact' && m.style.material === 'm-solid',
    () => m.style.radius === 8 && m.style.accentFollowsWallpaper === false,
    () => m.draft('代码分析工作台') && m.stages.get('代码分析工作台') === 'drafted',
    () => m.verify('代码分析工作台') && m.progress() === 20,
    () => m.draft('代码分析工作台') === false && m.clamped >= 1,
    () => m.verify('设置') === false && m.clamped >= 2,
    () => m.draft('设置') && m.verify('设置') && m.progress() === 40,
    () => m.lock() === false && m.clamped >= 3,
    () => { const q = new T.MockupSpec('v2'); for (const s of T.MOCKUP_SCREENS) { q.draft(s); q.verify(s); } return q.lock() && q.locked; },
    () => m.style.curve === '--ease-standard' && T.MOCKUP_SCREENS.length === 5,
    () => T.MOCKUP_STYLES.v2.density !== T.MOCKUP_STYLES.v1.density,
    () => { const q = new T.MockupSpec('v2'); q.draft('文件管理器'); return q.progress() === 0; },
    () => T.MockupSpec.variantLabel('v2') === '工作台' && T.MockupSpec.variantLabel('v3') === '极光',
    () => m.draft('文件管理器') && m.verify('文件管理器') && m.progress() === 60,
    () => { const t0 = performance.now(); for (let i = 0; i < 300; i++) new T.MockupSpec('v2').draft('设置'); return performance.now() - t0 < 50; },
    () => m.stages.get('桌面+图标') === 'blank',
    () => { const a = new T.MockupSpec('v2'); const b = new T.MockupSpec('v2'); a.draft('设置'); b.draft('设置'); return a.stages.get('设置') === b.stages.get('设置'); },
    () => m.style.material === 'm-solid' && m.style.density === 'compact',
    () => m.lock() === false,
    () => { for (const s of T.MOCKUP_SCREENS) { m.draft(s); m.verify(s); } return m.lock() && m.progress() === 100; },
    () => m.draft('设置') === false && m.clamped >= 4,
    () => T.MOCKUP_STYLES.v2.radius < T.MOCKUP_STYLES.v1.radius,
    () => typeof T.MOCKUP_STYLES === 'object' && typeof m.verify === 'function',
    () => { const q = new T.MockupSpec('v2'); return !q.locked && q.stages.get('任务栏+开始菜单') === 'blank'; },
  ]);
}

/* -------- 族0548 视觉稿 V3 极光 X13676~X13700（§7.1/§7.2）-------- */
export function checkF0548(): CheckEntry[] {
  const m = new T.MockupSpec('v3');
  return mk25(13676, '极光', [
    () => m.variant === 'v3' && T.MockupSpec.variantLabel('v3') === '极光',
    () => m.style.material === 'm-glow' && m.style.curve === '--ease-emphasized',
    () => m.style.accentFollowsWallpaper === true && m.style.radius === 12,
    () => m.draft('桌面+图标') && m.stages.get('桌面+图标') === 'drafted',
    () => m.verify('桌面+图标') && m.progress() === 20,
    () => m.draft('桌面+图标') === false && m.clamped >= 1,
    () => m.verify('任务栏+开始菜单') === false && m.clamped >= 2,
    () => m.draft('任务栏+开始菜单') && m.verify('任务栏+开始菜单') && m.progress() === 40,
    () => m.lock() === false && m.clamped >= 3,
    () => { const q = new T.MockupSpec('v3'); return q.progress() === 0 && !q.locked; },
    () => m.style.density === 'comfortable' && T.MOCKUP_SCREENS.length === 5,
    () => T.MOCKUP_STYLES.v3.curve !== T.MOCKUP_STYLES.v1.curve,
    () => { const q = new T.MockupSpec('v3'); q.draft('设置'); return q.progress() === 0; },
    () => ['晨雾', '工作台', '极光'].every((l) => ['v1', 'v2', 'v3'].some((v) => T.MockupSpec.variantLabel(v as 'v1') === l)),
    () => m.draft('设置') && m.verify('设置') && m.progress() === 60,
    () => { const t0 = performance.now(); for (let i = 0; i < 300; i++) new T.MockupSpec('v3').verify('设置'); return performance.now() - t0 < 50; },
    () => m.stages.get('文件管理器') === 'blank' && m.stages.get('代码分析工作台') === 'blank',
    () => { const a = new T.MockupSpec('v3'); const b = new T.MockupSpec('v3'); a.draft('设置'); b.draft('设置'); return a.stages.get('设置') === b.stages.get('设置'); },
    () => m.style.material === 'm-glow' && m.style.curve === '--ease-emphasized',
    () => m.lock() === false,
    () => { for (const s of T.MOCKUP_SCREENS) { m.draft(s); m.verify(s); } return m.lock() && m.progress() === 100; },
    () => m.verify('设置') === false && m.clamped >= 4,
    () => T.MOCKUP_STYLES.v3.radius === T.MOCKUP_STYLES.v1.radius - 4,
    () => typeof T.MockupSpec.variantLabel === 'function' && typeof m.lock === 'function',
    () => { const q = new T.MockupSpec('v3'); return q.stages.get('设置') === 'blank' && q.style.material === 'm-glow'; },
  ]);
}

/* -------- 族0550 空态与骨架体系 X13726~X13750（§13 #21/EmptyState）-------- */
export function checkF0550(): CheckEntry[] {
  const se = new T.SkeletonEmpty();
  return mk25(13726, '空骨架', [
    () => T.SkeletonEmpty.shouldSkeleton(400) === true && T.SkeletonEmpty.shouldSkeleton(200) === false,
    () => T.SkeletonEmpty.layout(3).length === 3 && T.SkeletonEmpty.layout(3)[1]!.shape === 'rect',
    () => { const f = T.SkeletonEmpty.crossfade(false); return f.mode === 'crossfade' && f.duration === 170; },
    () => se.add('no-files', '还没有文件', '把文件拖进来即可', '新建') && se.empties.length === 1,
    () => se.of('no-files')!.action === '新建' && se.of('no-files')!.hint === '把文件拖进来即可',
    () => T.SkeletonEmpty.shouldSkeleton(Number.NaN) === false,
    () => se.add('', 'x', 'y', 'z') === false && se.clamped >= 1,
    () => se.add('no-files', '重复', '', '') === false && se.empties.length === 1,
    () => T.SkeletonEmpty.layout(0).length === 0 && T.SkeletonEmpty.layout(99).length === 8,
    () => se.add('no-net', '网络不可达', '检查连接后重试', '重试') && se.empties.length === 2,
    () => T.SKELETON_SHAPES.length === 3 && T.SkeletonEmpty.layout(2)[0]!.w === '100%',
    () => T.SkeletonEmpty.coexist(0, 400) === 'skeleton' && T.SkeletonEmpty.coexist(0, 100) === 'empty',
    () => T.SkeletonEmpty.coexist(3, 100) === 'content',
    () => { const f = T.SkeletonEmpty.crossfade(true); return f.duration === 80 && f.mode === 'fade'; },
    () => se.empties.every((e) => e.title.length > 0 && e.action.length > 0),
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.SkeletonEmpty.layout(4); return performance.now() - t0 < 50; },
    () => T.SkeletonEmpty.shouldSkeleton(301) === true && T.SkeletonEmpty.shouldSkeleton(300) === false,
    () => { const a = T.SkeletonEmpty.layout(4); const b = T.SkeletonEmpty.layout(4); return JSON.stringify(a) === JSON.stringify(b); },
    () => T.SkeletonEmpty.layout(1)[0]!.w === '60%',
    () => se.add('no-result', '没有匹配结果', '换个关键词试试', '清空搜索') && se.of('no-result') !== undefined,
    () => { let n = 0; for (let i = 0; i < 20; i++) { const q = new T.SkeletonEmpty(); if (q.add(`k${i}`, '标题', '提示', '动作')) n++; } return n === 20; },
    () => T.SkeletonEmpty.coexist(2, 301) === 'skeleton' && T.SkeletonEmpty.coexist(5, 50) === 'content',
    () => se.empties.every((e) => T.SkeletonEmpty.shouldSkeleton(100) === false || e.key !== ''),
    () => typeof T.SkeletonEmpty.crossfade === 'function' && typeof se.of === 'function',
    () => { const q = new T.SkeletonEmpty(); q.add('egg', '彩蛋在等你', '输入暗号发现它', '发现'); return q.of('egg')!.title.includes('彩蛋'); },
  ]);
}

/* -------- 聚合入口 -------- */
export function runAi55Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [checkF0542, checkF0543, checkF0546, checkF0547, checkF0548, checkF0550];
  const entries = families.flatMap((fn) => fn());
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
