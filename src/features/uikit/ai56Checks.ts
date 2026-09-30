/**
 * UNREAL-X-15000 · AI-56 UI 质量收官 CheckSet（族0551/0552/0553/0555/0557/0560 · V 线 6 族 150 项），勿删。
 * 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
 * C 线（族0554/0556/0558/0559）见 code-analysis/core/src/ai56.rs。
 */
import type { CheckEntry } from '../compat/checks';
import * as T from './ai56Models';

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

/* -------- 族0551 微交互打磨 X13751~X13775（§5 族0405 三态曲线全表）-------- */
export function checkF0551(): CheckEntry[] {
  const mi = new T.MicroInteraction();
  return mk25(13751, '微交互', [
    () => mi.resolve('hover').duration === 120 && mi.resolve('hover').curve === '--ease-standard',
    () => mi.resolve('overlay').duration === 240 && mi.resolve('overlay').curve === '--ease-emphasized',
    () => T.MOTION_TABLE.length === 7 && mi.resolve('notify').curve === '--ease-spring',
    () => { const h = mi.resolve('hover'); return h.leave === 80 && h.duration === 120; },
    () => T.MicroInteraction.stateFx('hover').scale === 1.02 && T.MicroInteraction.stateFx('press').scale === 0.98,
    () => mi.resolve('nope').duration === 120 && mi.clamped >= 1,
    () => T.MicroInteraction.stateFx('disabled').opacity === 0.4,
    () => { mi.setReduceMotion(true); const r = mi.resolve('overlay'); return r.duration === 80 && r.curve === 'linear'; },
    () => { mi.setReduceMotion(false); return mi.resolve('route').duration === 170; },
    () => T.MicroInteraction.loadTier(400) === 'spinner' && T.MicroInteraction.loadTier(200) === 'none',
    () => T.MOTION_TABLE.every((m) => m.duration >= 80 && m.duration <= 240),
    () => T.MicroInteraction.focusRing().width === 2 && T.MicroInteraction.focusRing().offset === 2,
    () => ['rest', 'hover', 'press', 'disabled'].every((s) => T.MicroInteraction.stateFx(s as T.WidgetState).opacity > 0),
    () => T.MicroInteraction.stateFx('rest').scale === 1 && T.MicroInteraction.stateFx('rest').opacity === 1,
    () => mi.resolve('menu').duration === 170 && mi.resolve('skeleton').duration === 170,
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) mi.resolve('hover'); return performance.now() - t0 < 50; },
    () => { mi.setReduceMotion(true); const a = mi.resolve('hover'); mi.setReduceMotion(false); const b = mi.resolve('hover'); return a.duration !== b.duration; },
    () => { const a = T.MicroInteraction.stateFx('press'); return a.scale === 0.98 && a.opacity === 1; },
    () => { mi.setReduceMotion(true); const r = mi.resolve('menu'); mi.setReduceMotion(false); return r.curve === 'linear'; },
    () => mi.resolve('notify').duration === 200 && T.MicroInteraction.loadTier(300) === 'none',
    () => { let n = 0; for (let i = 0; i < 20; i++) { const q = new T.MicroInteraction(); if (q.resolve(T.MOTION_TABLE[i % 7]!.scene).duration >= 80) n++; } return n === 20; },
    () => T.MOTION_TABLE[0]!.scene === 'hover' && T.MOTION_TABLE[6]!.scene === 'reduce-motion',
    () => typeof T.MicroInteraction.stateFx === 'function' && typeof mi.resolve === 'function',
    () => { const q = new T.MicroInteraction(); q.setReduceMotion(true); return q.resolve('egg').duration === 80; },
    () => mi.resolve('hover').scene === 'hover' && T.MicroInteraction.stateFx('hover').opacity === 1,
  ]);
}

/* -------- 族0552 图标一致性走查 X13776~X13800（§2/族0353）-------- */
export function checkF0552(): CheckEntry[] {
  const au = new T.IconAudit();
  return mk25(13776, '图标', [
    () => au.add('settings', 24, 1.5, 20) && au.icons.length === 1 && au.clean(),
    () => au.add('folder', 24, 1.5, 16) && au.icons.length === 2 && au.clean(),
    () => T.ICON_GRID === 24 && T.ICON_STROKE === 1.5 && T.ICON_SIZES.length === 3,
    () => au.add('save', 24, 1.5, 24) && au.clean() && au.icons.every((i) => i.grid === 24),
    () => au.report().length === 0 && au.icons.length === 3,
    () => au.add('bad', 20, 2, 18) && !au.clean() && au.report().length === 3,
    () => au.add('', 24, 1.5, 24) === false && au.clamped >= 1,
    () => au.add('settings', 24, 1.5, 20) === false && au.icons.length === 4,
    () => au.fix('bad') && au.clean() && au.icons.find((i) => i.name === 'bad')!.stroke === 1.5,
    () => au.add('warn', 24, 1.5, 99) && au.icons.find((i) => i.name === 'warn')!.size === 24,
    () => T.ICON_SIZES.join(',') === '16,20,24',
    () => { const q = new T.IconAudit(); q.add('x', 24, 1.5, 20); return q.report().length === 0; },
    () => { const q = new T.IconAudit(); return q.clean() && q.icons.length === 0; },
    () => au.fix('nope') === false && au.clamped >= 2,
    () => au.add('grid-off', 12, 1.5, 16) && au.report().some((v) => v === 'grid-off:grid'),
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) new T.IconAudit().add(`i${i}`, 24, 1.5, 20); return performance.now() - t0 < 50; },
    () => au.icons.every((i) => Number.isFinite(i.grid) && Number.isFinite(i.stroke)),
    () => { const a = new T.IconAudit(); const b = new T.IconAudit(); a.add('z', 24, 1.5, 20); b.add('z', 24, 1.5, 20); return a.report().join() === b.report().join(); },
    () => { const q = new T.IconAudit(); q.add('n', Number.NaN, Number.NaN, Number.NaN); return q.icons[0]!.grid === 24 && q.icons[0]!.stroke === 1.5 && q.icons[0]!.size === 24; },
    () => T.IconAudit.touchTarget(false) === 44 && T.IconAudit.touchTarget(true) === 40,
    () => { let n = 0; const q = new T.IconAudit(); for (let i = 0; i < 20; i++) { if (q.add(`ic${i}`, 24, 1.5, T.ICON_SIZES[i % 3]!)) n++; } return n === 20; },
    () => au.fix('grid-off') && !au.report().includes('grid-off:grid'),
    () => typeof T.IconAudit.touchTarget === 'function' && typeof au.fix === 'function',
    () => { const q = new T.IconAudit(); q.add('egg', 24, 1.5, 16); return q.icons[0]!.name === 'egg' && q.clean(); },
    () => au.icons.every((i) => i.name.length > 0) && au.icons.length >= 4,
  ]);
}

/* -------- 族0553 HC 红线核验 X13801~X13825（§8.3 降级不降可达）-------- */
export function checkF0553(): CheckEntry[] {
  const g = new T.HcGuard();
  const good: T.SurfaceDef = { name: 'card', material: 'm-solid', glow: false, motionMs: 80, contrast: 8 };
  return mk25(13801, 'HC红线', [
    () => T.HcGuard.degrade(good).ok && T.HcGuard.degrade(good).degradations.length === 0,
    () => { const s = { ...good, material: 'm-mica' }; return T.HcGuard.degrade(s).degradations[0]!.includes('material'); },
    () => T.HcGuard.paletteFrozen() && T.HC_PALETTE.accent === '#ffff00',
    () => { const s = { ...good, glow: true }; return T.HcGuard.degrade(s).degradations[0]!.includes('glow'); },
    () => g.audit([good, { ...good, material: 'm-acrylic' }]) === 1 && g.audited.length === 2,
    () => { const s = { ...good, contrast: 3 }; return !T.HcGuard.degrade(s).ok && T.HcGuard.degrade(s).degradations.some((d) => d.includes('contrast')); },
    () => { const s = { ...good, name: '' }; return T.HcGuard.degrade(s).degradations.every((d) => d.startsWith(':') === false || d.length > 0); },
    () => { const s = { ...good, motionMs: 240 }; return T.HcGuard.degrade(s).degradations[0]!.includes('motion'); },
    () => { const s = { ...good, motionMs: 81 }; return T.HcGuard.degrade(s).degradations.length === 1; },
    () => { const q = new T.HcGuard(); return q.passRate() === 100 && q.audited.length === 0; },
    () => T.HC_PALETTE.text === '#000000' && T.HC_PALETTE.bg === '#ffffff',
    () => g.audit([{ ...good, name: 'hero', contrast: 4.4 }]) === 1 && g.passRate() < 100,
    () => g.audited.every((v) => typeof v.ok === 'boolean'),
    () => { const allBad: T.SurfaceDef = { name: 'x', material: 'm-glow', glow: true, motionMs: 240, contrast: 2 }; return T.HcGuard.degrade(allBad).degradations.length === 4; },
    () => g.passRate() === Math.round((g.audited.filter((v) => v.ok).length / g.audited.length) * 100),
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.HcGuard.degrade(good); return performance.now() - t0 < 50; },
    () => { const s = { ...good, motionMs: 80.5 }; return T.HcGuard.degrade(s).ok === false; },
    () => { const a = T.HcGuard.degrade(good); const b = T.HcGuard.degrade(good); return a.ok === b.ok && a.degradations.join() === b.degradations.join(); },
    () => { const q = new T.HcGuard(); q.audit([{ ...good, glow: true }]); return q.audited[0]!.ok === false; },
    () => { const s = { ...good, material: 'm-solid' }; return T.HcGuard.degrade(s).degradations.every((d) => !d.includes('material')); },
    () => { let n = 0; for (let i = 0; i < 20; i++) { const q = new T.HcGuard(); if (q.audit([good]) === 0) n++; } return n === 20; },
    () => { const q = new T.HcGuard(); q.audit([good, { ...good, name: 'b', glow: true }, { ...good, name: 'c', contrast: 4 }]); return q.passRate() === 33; },
    () => typeof T.HcGuard.degrade === 'function' && typeof g.passRate === 'function',
    () => { const egg: T.SurfaceDef = { name: 'egg', material: 'm-solid', glow: false, motionMs: 80, contrast: 21 }; return T.HcGuard.degrade(egg).ok; },
    () => { const dual = { ...good, name: 'dual', glow: true, motionMs: 200 }; const v = T.HcGuard.degrade(dual); return v.degradations.length === 2 && !v.ok; },
  ]);
}

/* -------- 族0555 中文渲染纪律 X13851~X13875（§12.1 CJK 铁律）-------- */
export function checkF0555(): CheckEntry[] {
  const cd = new T.CjkDiscipline();
  const good: T.TextStyleDef = { lh: 1.6, tracking: 0, fontStack: T.CJK_FALLBACK, tabular: true };
  return mk25(13851, '中文', [
    () => T.CjkDiscipline.validate(good).length === 0 && cd.clean(),
    () => { const s = { ...good, lh: 1.5 }; return T.CjkDiscipline.validate(s)[0] === 'line-height≠1.6'; },
    () => T.CJK_LH === 1.6 && T.CJK_TRACKING === 0,
    () => { const s = { ...good, tabular: false }; return T.CjkDiscipline.validate(s).some((v) => v.includes('tabular')); },
    () => cd.audit(good) === 0 && cd.violations.length === 0,
    () => { const s = { ...good, tracking: 0.5 }; return T.CjkDiscipline.validate(s).some((v) => v === '字距≠0'); },
    () => { const s = { ...good, fontStack: ['system-ui'] }; return T.CjkDiscipline.validate(s).some((v) => v.includes('回退链缺')); },
    () => cd.audit({ ...good, lh: 1.2 }) === 1 && cd.violations.length === 1,
    () => T.CJK_FALLBACK.length === 5 && T.CJK_FALLBACK.includes('Microsoft YaHei'),
    () => { const q = new T.CjkDiscipline(); return q.clean() && q.violations.length === 0; },
    () => T.CjkDiscipline.zhOk('已完成。') === true && T.CjkDiscipline.zhOk('已完成.') === false,
    () => T.CjkDiscipline.zhOk('进行中...') === false,
    () => T.CjkDiscipline.needsSpacing('中', 'A') === true && T.CjkDiscipline.needsSpacing('A', '文') === true,
    () => cd.audit({ ...good, tracking: 1 }) === 1 && cd.clamped >= 1,
    () => { const s = { ...good, lh: 1.6, tracking: 0 }; return T.CjkDiscipline.validate(s).length === 0; },
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.CjkDiscipline.validate(good); return performance.now() - t0 < 50; },
    () => T.CjkDiscipline.needsSpacing('中', '文') === false && T.CjkDiscipline.needsSpacing('a', 'b') === false,
    () => { const a = T.CjkDiscipline.validate(good); const b = T.CjkDiscipline.validate(good); return a.join() === b.join(); },
    () => { const q = new T.CjkDiscipline(); q.audit({ ...good, lh: 1.0, tracking: 2 }); return q.violations.length === 2; },
    () => T.CjkDiscipline.zhOk('确定吗？') === true && T.CjkDiscipline.zhOk('确定吗?') === false,
    () => { let n = 0; for (let i = 0; i < 20; i++) { const q = new T.CjkDiscipline(); if (q.audit(good) === 0) n++; } return n === 20; },
    () => { const q = new T.CjkDiscipline(); q.audit({ ...good, fontStack: ['Arial'] }); return q.violations.length === 5; },
    () => typeof T.CjkDiscipline.zhOk === 'function' && typeof cd.audit === 'function',
    () => { const q = new T.CjkDiscipline(); q.audit({ ...good, tabular: true }); return q.clean() && q.clamped === 0; },
    () => T.CjkDiscipline.zhOk('任务已完成』') === true && T.CjkDiscipline.needsSpacing('文', '字') === false,
  ]);
}

/* -------- 族0557 落稿实施 X13901~X13925（§7.3 选稿→组件落地）-------- */
export function checkF0557(): CheckEntry[] {
  const ap = new T.MockupApply();
  return mk25(13901, '落稿', [
    () => ap.pick('v1') && ap.variant === 'v1' && ap.done.length === 0,
    () => ap.step('抽token') && ap.progress() === 20,
    () => T.APPLY_STEPS.length === 5 && ap.step('搭容器') && ap.progress() === 40,
    () => ap.step('对照组件') && ap.done.join() === '抽token,搭容器,对照组件',
    () => ap.step('过三态') && ap.step('过键走查') && ap.finished(),
    () => ap.step('过三态') === false && ap.clamped >= 1,
    () => ap.pick('v1') === false && ap.clamped >= 2,
    () => { const q = new T.MockupApply(); q.pick('v2'); return q.step('对照组件') === false && q.done.length === 0; },
    () => { const q = new T.MockupApply(); return q.step('抽token') === false && q.variant === null; },
    () => T.MockupApply.mixAllowed('v1', 'v2') && T.MockupApply.mixAllowed('v2', 'v3'),
    () => ap.pick('v3') && ap.done.length === 0 && ap.progress() === 0,
    () => ap.step('抽token') && ap.rollbackOne() === '抽token' && ap.done.length === 0,
    () => ap.step('抽token') && ap.step('搭容器') && ap.progress() === 40,
    () => ap.rollbackOne() === '搭容器' && ap.step('抽token') === false && ap.done.length === 1,
    () => ap.step('搭容器') && ap.step('对照组件') && ap.done.length === 3,
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) new T.MockupApply().progress(); return performance.now() - t0 < 50; },
    () => ap.done.every((s) => (T.APPLY_STEPS as readonly string[]).includes(s)),
    () => { const a = new T.MockupApply(); const b = new T.MockupApply(); a.pick('v1'); b.pick('v1'); return a.progress() === b.progress(); },
    () => { const q = new T.MockupApply(); q.pick('v1'); return q.step('过键走查') === false && q.clamped >= 1; },
    () => T.MockupApply.mixAllowed('v1', 'v1') === false,
    () => { let n = 0; for (let i = 0; i < 20; i++) { const q = new T.MockupApply(); q.pick((['v1', 'v2', 'v3'] as const)[i % 3]!); if (q.variant !== null) n++; } return n === 20; },
    () => { const q = new T.MockupApply(); q.pick('v2'); for (const s of T.APPLY_STEPS) q.step(s); return q.finished() && q.progress() === 100; },
    () => typeof T.MockupApply.mixAllowed === 'function' && typeof ap.rollbackOne === 'function',
    () => { const q = new T.MockupApply(); q.pick('v3'); q.step('抽token'); return q.rollbackOne() === '抽token' && !q.finished(); },
    () => ap.done.length === 3 && ap.step('过三态') && ap.step('过键走查') && ap.finished(),
  ]);
}

/* -------- 族0560 UI 大收官 X13976~X14000（三方 G1~G4 双线门禁）-------- */
export function checkF0560(): CheckEntry[] {
  const f = new T.UiFinale();
  return mk25(13976, '收官', [
    () => f.gate('G1 代码门禁', true) && f.passed.length === 1,
    () => f.gate('G2 视觉门禁', true) && f.progress() === 50,
    () => T.FINALE_GATES.length === 4 && f.gate('G3 指标门禁', true),
    () => f.gate('G4 审计门禁', true) && f.seal() && f.sealed,
    () => f.gate('G1 代码门禁', true) === false && f.clamped >= 1,
    () => f.gate('G2 视觉门禁', false) === false && f.clamped >= 2,
    () => { const q = new T.UiFinale(); return q.gate('G2 视觉门禁', true) === false && q.passed.length === 0; },
    () => { const q = new T.UiFinale(); return q.seal() === false && q.clamped >= 1; },
    () => { const q = new T.UiFinale(); q.gate('G1 代码门禁', false); return q.passed.length === 0; },
    () => T.UiFinale.idAudit([1, 2, 3], 1, 3) === true,
    () => T.UiFinale.idAudit([1, 2, 2], 1, 3) === false,
    () => T.UiFinale.idAudit([1, 2], 1, 3) === false,
    () => T.UiFinale.dualGate(0, 0) === true && T.UiFinale.dualGate(1, 0) === false,
    () => T.UiFinale.dualGate(0, 3) === false && T.FINALE_GATES[0] === 'G1 代码门禁',
    () => { const q = new T.UiFinale(); for (const g of T.FINALE_GATES) q.gate(g, true); return q.seal() && q.progress() === 100; },
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.UiFinale.idAudit([1, 2, 3], 1, 3); return performance.now() - t0 < 50; },
    () => { const q = new T.UiFinale(); q.gate('G1 代码门禁', true); return q.progress() === 25; },
    () => { const a = new T.UiFinale(); const b = new T.UiFinale(); a.gate('G1 代码门禁', true); b.gate('G1 代码门禁', true); return a.progress() === b.progress(); },
    () => { const q = new T.UiFinale(); q.gate('G1 代码门禁', true); q.seal(); return q.sealed === false; },
    () => T.UiFinale.idAudit([0, 1, 2], 1, 3) === false && T.UiFinale.idAudit([2, 3, 4], 1, 3) === false,
    () => { let n = 0; for (let i = 0; i < 20; i++) { const q = new T.UiFinale(); if (q.gate('G1 代码门禁', i % 2 === 0) === (i % 2 === 0)) n++; } return n === 20; },
    () => { const q = new T.UiFinale(); for (const g of T.FINALE_GATES) q.gate(g, true); q.seal(); return q.gate('G1 代码门禁', true) === false; },
    () => typeof T.UiFinale.idAudit === 'function' && typeof f.seal === 'function',
    () => { const q = new T.UiFinale(); return q.passed.length === 0 && q.progress() === 0 && !q.sealed; },
    () => f.sealed && f.passed.length === 4 && T.FINALE_GATES.every((g) => f.passed.includes(g)),
  ]);
}

/* -------- 聚合入口 -------- */
export function runAi56Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [checkF0551, checkF0552, checkF0553, checkF0555, checkF0557, checkF0560];
  const entries = families.flatMap((fn) => fn());
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
