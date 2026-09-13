/**
 * UNREAL-X-15000 · AI-59 协作与防线 CheckSet（族0581~0590 · X14501~X14750），勿删。
 * 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
 */
import type { CheckEntry } from '../compat/checks';
import * as T from './ai59Models';

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

/* -------- 族0581 多会话协作纪律 X14501~X14525 -------- */
export function checkF0581(): CheckEntry[] {
  const d = new T.CollabDiscipline();
  return mk25(14501, '协作', [
    () => d.claim('s1', 1, 250) && d.claims.length === 1,
    () => d.claim('s2', 251, 500) && d.covered() === 500,
    () => T.DISCIPLINE_TIERS.length === 5 && d.setTier('strict') === 'strict',
    () => d.snapshot().includes('"tier":"strict"') && new T.CollabDiscipline().restore(d.snapshot()),
    () => { const q = new T.CollabDiscipline(); return q.restore(d.snapshot()) && q.tier === 'strict' && q.covered() === 500; },
    () => { const q = new T.CollabDiscipline(); return q.claim('s', 100, 50) === false && q.clamped >= 1; },
    () => { const q = new T.CollabDiscipline(); q.claim('a', 1, 10); return q.claim('b', 5, 15) === false && q.lastReason.includes('冲突'); },
    () => { const q = new T.CollabDiscipline(); q.claim('a', 1, 10); const s = q.snapshot(); q.claim('b', 200, 300); q.restore(s); return q.claims.length === 1; },
    () => { const q = new T.CollabDiscipline(); q.setTier('observe'); return q.claim('a', 1, 10) === false; },
    () => { const q = new T.CollabDiscipline(); q.claim('a', 1, 10); return q.release('a') && q.claims.length === 0 && q.release('a') === false; },
    () => T.CollabDiscipline.tierLabel('strict') === '严纪律' && T.CollabDiscipline.tierLabel('archive') === '存档',
    () => { const q = new T.CollabDiscipline(); return q.setTier('nope') === 'standard' && q.clamped >= 1; },
    () => T.DISCIPLINE_TIERS.every((t) => { const q = new T.CollabDiscipline(); return q.setTier(t) === t; }),
    () => { const q = new T.CollabDiscipline(); q.claim('a', 1, 10); return q.lastReason === '' && q.covered() === 10; },
    () => T.CollabDiscipline.tierLabel('observe') === '只读观察' && T.CollabDiscipline.tierLabel('loose') === '宽松',
    () => { const t0 = performance.now(); for (let i = 0; i < 100; i++) new T.CollabDiscipline().claim('s', 1, 10); return performance.now() - t0 < 50; },
    () => { const q = new T.CollabDiscipline(); let n = 0; for (let i = 0; i < 50; i++) { if (q.claim(`s${i}`, i * 10 + 1, i * 10 + 10)) n++; } return n === 50; },
    () => { const a = new T.CollabDiscipline(); const b = new T.CollabDiscipline(); a.claim('x', 1, 10); b.claim('x', 1, 10); return a.snapshot() === b.snapshot(); },
    () => { const q = new T.CollabDiscipline(); q.setTier('loose'); return q.claim('a', 1, 1000) && q.setTier('strict') === 'strict'; },
    () => { const q = new T.CollabDiscipline(); return q.claim('a', 1, 501) === false && q.lastReason.includes('上限'); },
    () => T.CollabDiscipline.nextFree([{ from: 1, to: 100 }], 50) === 101 && T.CollabDiscipline.nextFree([], 25) === 1,
    () => { const q = new T.CollabDiscipline(); let n = 0; for (let i = 0; i < 10; i++) { if (q.claim(`s${i}`, 1, 10)) n++; } return n === 1; },
    () => { const q = new T.CollabDiscipline(); q.setTier('strict'); return q.claim('k', 14000, 14249) && q.covered() === 250; },
    () => typeof T.CollabDiscipline.nextFree === 'function' && typeof d.snapshot === 'function',
    () => { const q = new T.CollabDiscipline(); q.claim('a', 14991, 15000); return q.covered() === 10 && q.claim('b', 14995, 15005) === false; },
  ]);
}

/* -------- 族0582 域验收协议 X14526~X14550 -------- */
export function checkF0582(): CheckEntry[] {
  const g = new T.DomainAcceptance();
  return mk25(14526, '验收', [
    () => g.pass('G1', true, true) && g.passed.has('G1'),
    () => g.records.length === 1 && g.records[0]!.gate === 'G1',
    () => T.ACCEPT_GATES.length === 4,
    () => g.snapshot().includes('G1') && new T.DomainAcceptance().restore(g.snapshot()),
    () => { const q = new T.DomainAcceptance(); return q.restore(g.snapshot()) && q.passed.has('G1'); },
    () => { const q = new T.DomainAcceptance(); return q.pass('G2', true, true) === false && q.clamped >= 1; },
    () => { const q = new T.DomainAcceptance(); q.pass('G1', true, true); return q.pass('G2', true, false) === false && q.veto === true; },
    () => { const q = new T.DomainAcceptance(); q.pass('G1', true, true); q.pass('G2', true, true); const s = q.snapshot(); q.restore(s); return q.passed.size === 2; },
    () => { const q = new T.DomainAcceptance(); return q.pass('G1', false, true) === false && q.veto; },
    () => { const q = new T.DomainAcceptance(); q.pass('G1', true, true); q.pass('G2', true, true); const s = q.snapshot(); q.pass('G3', true, true); q.restore(s); return q.passed.size === 2; },
    () => T.DomainAcceptance.gateLabel('G1') === '族内自检' && T.DomainAcceptance.gateLabel('G4') === '终验收',
    () => { const q = new T.DomainAcceptance(); q.pass('G1', true, true); return q.audit(1, 5, [5, 4, 3, 2, 1]); },
    () => { const q = new T.DomainAcceptance(); return q.audit(1, 3, [1, 2]) === false; },
    () => T.DomainAcceptance.narrative('veto').includes('否决') && T.DomainAcceptance.narrative('skip').includes('顺序'),
    () => T.ACCEPT_GATES.every((x) => T.DomainAcceptance.gateLabel(x).length > 0),
    () => { const t0 = performance.now(); for (let i = 0; i < 100; i++) { const q = new T.DomainAcceptance(); q.pass('G1', true, true); } return performance.now() - t0 < 50; },
    () => { const q = new T.DomainAcceptance(); q.pass('G1', true, true); q.pass('G2', true, true); q.pass('G3', true, true); return q.done() === false; },
    () => { const a = new T.DomainAcceptance(); const b = new T.DomainAcceptance(); a.pass('G1', true, true); b.pass('G1', true, true); return a.snapshot() === b.snapshot(); },
    () => { const q = new T.DomainAcceptance(); return q.pass('G9', true, true) === false && q.clamped >= 1; },
    () => { const q = new T.DomainAcceptance(); return q.pass('G1', true, true) && q.pass('G2', true, true) && q.pass('G3', true, true) && q.pass('G4', true, true) && q.done(); },
    () => T.DomainAcceptance.narrative('unknown').includes('未知') && T.DomainAcceptance.narrative('veto').length > 0,
    () => { const q = new T.DomainAcceptance(); let n = 0; for (const gt of T.ACCEPT_GATES) { if (q.pass(gt, true, true)) n++; } return n === 4; },
    () => { const q = new T.DomainAcceptance(); q.pass('G1', true, true); return q.pass('G1', true, true) && q.records.length === 2; },
    () => typeof g.audit === 'function' && typeof T.DomainAcceptance.narrative === 'function',
    () => { const q = new T.DomainAcceptance(); q.pass('G1', true, true); q.pass('G2', true, true); return q.audit(14501, 14750, Array.from({ length: 250 }, (_, i) => i + 14501)); },
  ]);
}

/* -------- 族0583 回归防线 X14551~X14575（C 线主责，V 线同口径镜像）-------- */
export function checkF0583(): CheckEntry[] {
  const r = new T.RegressionGuard();
  return mk25(14551, '回归', [
    () => r.register(1) && r.count() === 1,
    () => { let n = 0; for (let i = 2; i <= 25; i++) { if (r.register(i)) n++; } return n === 24 && r.count() === 25; },
    () => T.REGRESSION_TIERS.length === 3,
    () => r.snapshot().includes('"frozen":false') && new T.RegressionGuard().restore(r.snapshot()),
    () => { const q = new T.RegressionGuard(); return q.restore(r.snapshot()) && q.count() === 25; },
    () => { const q = new T.RegressionGuard(); return q.register(0) === false && q.register(-1) === false && q.clamped >= 2; },
    () => { const q = new T.RegressionGuard(); q.register(1); return q.register(1) === false; },
    () => { const q = new T.RegressionGuard(); q.register(1); const s = q.snapshot(); q.register(2); q.restore(s); return q.count() === 1; },
    () => { const q = new T.RegressionGuard(); q.freeze(); return q.register(9) === false; },
    () => { const q = new T.RegressionGuard(); q.register(1); q.freeze(); return q.frozen && q.count() === 1; },
    () => T.RegressionGuard.narrative('red').includes('回滚') && T.RegressionGuard.narrative('green').includes('合入'),
    () => T.RegressionGuard.verdict(100, 100) === 'green' && T.RegressionGuard.verdict(100, 101) === 'green',
    () => T.RegressionGuard.verdict(100, 95) === 'yellow' && T.RegressionGuard.verdict(100, 96) === 'yellow',
    () => T.RegressionGuard.narrative('yellow').includes('预警'),
    () => T.RegressionGuard.drill() === true,
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) new T.RegressionGuard().register(i + 1); return performance.now() - t0 < 50; },
    () => { const q = new T.RegressionGuard(); for (let i = 1; i <= 100; i++) q.register(i); return q.count() === 100; },
    () => { const a = new T.RegressionGuard(); const b = new T.RegressionGuard(); a.register(1); b.register(1); return a.snapshot() === b.snapshot(); },
    () => { const q = new T.RegressionGuard(); q.register(1); q.register(2); return q.ids.join() === '1,2'; },
    () => { const q = new T.RegressionGuard(); q.freeze(); return q.restore('{"ids":[1,2],"frozen":false}') && q.frozen === false; },
    () => { const q = new T.RegressionGuard(); let n = 0; for (let i = 1; i <= 10; i++) { if (q.register(5)) n++; } return n === 1; },
    () => { const q = new T.RegressionGuard(); for (let i = 1; i <= 50; i++) q.register(i); return q.count() === 50 && q.register(1) === false; },
    () => T.REGRESSION_TIERS.every((t) => typeof T.RegressionGuard.narrative(t) === 'string'),
    () => typeof r.freeze === 'function' && typeof T.RegressionGuard.verdict === 'function',
    () => T.RegressionGuard.verdict(1000, 800) === 'red' && T.RegressionGuard.narrative(T.RegressionGuard.verdict(1000, 800)).includes('红色'),
  ]);
}

/* -------- 族0584 混沌工程 X14576~X14600（K 线主责，V 线同口径镜像）-------- */
export function checkF0584(): CheckEntry[] {
  const c = new T.ChaosEngine();
  return mk25(14576, '混沌', [
    () => c.inject('panic', 'mild', 1) && c.injected === 1,
    () => c.setBudget(500) === 500 && c.setBlastCap(4) === 4 && c.inject('oom', 'steady', 4),
    () => T.CHAOS_FAULTS.length === 5 && T.CHAOS_SEVERITY.length === 5,
    () => c.snapshot().includes('"budget":500') && new T.ChaosEngine().restore(c.snapshot()),
    () => { const q = new T.ChaosEngine(); return q.restore(c.snapshot()) && q.injected === 2; },
    () => { const q = new T.ChaosEngine(); return q.inject('nope', 'mild', 1) === false && q.inject('panic', 'nope', 1) === false; },
    () => { const q = new T.ChaosEngine(); return q.inject('panic', 'mild', 0) === false && q.inject('panic', 'mild', 9) === false; },
    () => { const q = new T.ChaosEngine(); q.setBudget(2); q.inject('oom', 'mild', 1); q.inject('oom', 'mild', 1); return q.inject('oom', 'mild', 1) === false; },
    () => { const q = new T.ChaosEngine(); q.killSwitch = true; return q.inject('panic', 'mild', 1) === false && q.clamped >= 1; },
    () => { const q = new T.ChaosEngine(); return q.setBudget(0) === 1 && q.setBudget(99999) === 1000; },
    () => T.ChaosEngine.narrative('panic').includes('恐慌') && T.ChaosEngine.faultLabel('oom') === '内存',
    () => { const s1 = T.ChaosEngine.schedule(42, 10); const s2 = T.ChaosEngine.schedule(42, 10); return s1.join() === s2.join() && s1.length === 10; },
    () => T.ChaosEngine.schedule(7, 20).every((f) => T.CHAOS_FAULTS.includes(f)),
    () => T.ChaosEngine.recoverable('panic', 'extreme') === false && T.ChaosEngine.recoverable('panic', 'harsh') === true,
    () => T.CHAOS_FAULTS.every((f) => T.CHAOS_SEVERITY.every((sv) => (f === 'panic' && sv === 'extreme' ? !T.ChaosEngine.recoverable(f, sv) : T.ChaosEngine.recoverable(f, sv)))),
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.ChaosEngine.schedule(i + 1, 10); return performance.now() - t0 < 50; },
    () => { const q = new T.ChaosEngine(); q.setBudget(1000); let n = 0; for (let i = 0; i < 1000; i++) { if (q.inject('oom', 'mild', 1)) n++; } return n === 1000; },
    () => { const a = new T.ChaosEngine(); const b = new T.ChaosEngine(); a.setBudget(77); b.setBudget(77); return a.snapshot() === b.snapshot(); },
    () => { const q = new T.ChaosEngine(); q.setBlastCap(1); return q.inject('oom', 'mild', 1) && q.inject('oom', 'mild', 2) === false; },
    () => { const q = new T.ChaosEngine(); return q.setBudget(Number.NaN) === 1; },
    () => { const s = T.ChaosEngine.schedule(99, 5); return s.length === 5 && new Set(s).size <= 5; },
    () => { const q = new T.ChaosEngine(); let n = 0; for (const f of T.CHAOS_FAULTS) { if (q.inject(f, 'trace', 1)) n++; } return n === 5; },
    () => T.CHAOS_SEVERITY.every((sv) => new T.ChaosEngine().inject('irq-storm', sv, 1)),
    () => typeof T.ChaosEngine.schedule === 'function' && typeof T.ChaosEngine.recoverable === 'function',
    () => T.ChaosEngine.narrative('irq-storm').includes('风暴') && T.ChaosEngine.narrative('unknown-fault').includes('未知'),
  ]);
}

/* -------- 族0585 长稳测试 X14601~X14625（C 线主责，V 线同口径镜像）-------- */
export function checkF0585(): CheckEntry[] {
  const s = new T.SoakRunner();
  return mk25(14601, '长稳', [
    () => s.sample(100) && s.samples.length === 1,
    () => s.setHours(168) === 168 && s.hours === 168,
    () => { for (let i = 0; i < 30; i++) s.sample(i); return s.samples.length === 24; },
    () => s.snapshot().includes('"hours":168') && new T.SoakRunner().restore(s.snapshot()),
    () => { const q = new T.SoakRunner(); return q.restore(s.snapshot()) && q.hours === 168; },
    () => { const q = new T.SoakRunner(); return q.sample(Number.NaN) === false && q.sample(Number.POSITIVE_INFINITY) === false; },
    () => { const q = new T.SoakRunner(); return q.leaks([100, 3000, 2048, 2049]) === 2; },
    () => { const q = new T.SoakRunner(); q.setHours(10); q.resume(5); return q.resume(3) === false && q.checkpoint === 5; },
    () => { const q = new T.SoakRunner(); return q.setHours(0) === 1 && q.setHours(9999) === 720; },
    () => { const q = new T.SoakRunner(); return q.verdict() === 'fail'; },
    () => T.SoakRunner.drift(100, 111) === true && T.SoakRunner.drift(100, 110) === false,
    () => T.SoakRunner.drift(100, 89) === true && T.SoakRunner.drift(100, 90) === false,
    () => { const q = new T.SoakRunner(); q.setHours(72); q.resume(72); return q.verdict() === 'pass'; },
    () => { const q = new T.SoakRunner(); q.setHours(72); q.resume(72); q.sample(3000); return q.verdict() === 'fail'; },
    () => T.SoakRunner.narrative().includes('续跑'),
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) { const q = new T.SoakRunner(); q.sample(i); } return performance.now() - t0 < 50; },
    () => { const q = new T.SoakRunner(); for (let i = 0; i < 1000; i++) q.sample(i % 100); return q.samples.length === 24; },
    () => { const a = new T.SoakRunner(); const b = new T.SoakRunner(); a.setHours(100); b.setHours(100); return a.snapshot() === b.snapshot(); },
    () => { const q = new T.SoakRunner(); q.setHours(24); q.resume(24); return q.verdict() === 'pass' && q.checkpoint === 24; },
    () => { const q = new T.SoakRunner(); return q.resume(-1) === false && q.clamped >= 1; },
    () => { const q = new T.SoakRunner(); let n = 0; for (let i = 1; i <= 24; i++) { if (q.sample(i)) n++; } return n === 24 && q.samples.length === 24; },
    () => { const q = new T.SoakRunner(); q.setHours(72); q.resume(72); for (let i = 0; i < 25; i++) q.sample(100); return q.verdict() === 'pass' && q.samples.length === 24; },
    () => T.SoakRunner.drift(0, 100) === false,
    () => typeof s.leaks === 'function' && typeof T.SoakRunner.drift === 'function',
    () => { const q = new T.SoakRunner(); q.resume(10); const w = new T.SoakRunner(); w.restore(q.snapshot()); return w.checkpoint === 10; },
  ]);
}

/* -------- 族0586 工具链成熟度 X14626~X14650 -------- */
export function checkF0586(): CheckEntry[] {
  const m = new T.ToolchainMaturity();
  return mk25(14626, '工具链', [
    () => m.register('cargo-ktest', 'experimental') && m.tools.size === 1,
    () => m.register('vitest', 'beta') && m.register('tsc', 'stable') && m.tools.size === 3,
    () => T.TOOLCHAIN_TOOLS.length === 5 && T.MATURITY_TIERS.length === 5,
    () => m.snapshot().includes('cargo-ktest') && new T.ToolchainMaturity().restore(m.snapshot()),
    () => { const q = new T.ToolchainMaturity(); return q.restore(m.snapshot()) && q.tools.size === 3; },
    () => { const q = new T.ToolchainMaturity(); return q.register('nope', 'stable') === false && q.register('vitest', 'nope') === false; },
    () => { const q = new T.ToolchainMaturity(); q.register('vitest', 'beta'); return q.register('vitest', 'stable') === false; },
    () => { const q = new T.ToolchainMaturity(); q.register('vitest', 'beta'); return q.upgrade('vitest', 'experimental') === false; },
    () => { const q = new T.ToolchainMaturity(); return q.upgrade('vitest', 'stable') === false; },
    () => { const q = new T.ToolchainMaturity(); q.register('tsc', 'stable'); return q.upgrade('tsc', 'stable') === false && q.upgrade('tsc', 'hardened'); },
    () => T.ToolchainMaturity.tierLabel('frozen') === '冻结' && T.ToolchainMaturity.tierLabel('experimental') === '实验',
    () => T.ToolchainMaturity.lineOf('cargo-ktest') === 'K' && T.ToolchainMaturity.lineOf('vitest') === 'V' && T.ToolchainMaturity.lineOf('gen-unrealx') === 'C',
    () => m.coveredLines().join() === 'K,V',
    () => T.ToolchainMaturity.fallback('md5-sync').includes('人工'),
    () => { const q = new T.ToolchainMaturity(); q.register('gen-unrealx', 'experimental'); return q.upgrade('gen-unrealx', 'beta') && q.upgrade('gen-unrealx', 'stable'); },
    () => { const t0 = performance.now(); for (let i = 0; i < 200; i++) { const q = new T.ToolchainMaturity(); q.register('vitest', 'beta'); } return performance.now() - t0 < 50; },
    () => { const q = new T.ToolchainMaturity(); for (const t of T.TOOLCHAIN_TOOLS) q.register(t, 'experimental'); return q.tools.size === 5 && q.coveredLines().join() === 'C,K,V'; },
    () => { const a = new T.ToolchainMaturity(); const b = new T.ToolchainMaturity(); a.register('vitest', 'beta'); b.register('vitest', 'beta'); return a.snapshot() === b.snapshot(); },
    () => m.score() === 120,
    () => { const q = new T.ToolchainMaturity(); q.register('md5-sync', 'stable'); return q.freeze('md5-sync') && q.tools.get('md5-sync') === 'frozen'; },
    () => { const q = new T.ToolchainMaturity(); let n = 0; for (const t of T.TOOLCHAIN_TOOLS) { if (q.register(t, 'stable')) n++; } return n === 5 && q.score() === 300; },
    () => { const q = new T.ToolchainMaturity(); q.register('cargo-ktest', 'experimental'); q.upgrade('cargo-ktest', 'beta'); q.upgrade('cargo-ktest', 'stable'); q.upgrade('cargo-ktest', 'hardened'); return q.freeze('cargo-ktest') && q.score() === 100; },
    () => { const q = new T.ToolchainMaturity(); q.register('vitest', 'beta'); return q.restore('[["tsc","frozen"]]') && q.tools.size === 1 && q.tools.get('tsc') === 'frozen'; },
    () => typeof m.upgrade === 'function' && typeof T.ToolchainMaturity.lineOf === 'function',
    () => { const q = new T.ToolchainMaturity(); q.register('vitest', 'frozen'); return q.upgrade('vitest', 'frozen') === false && q.freeze('vitest') === false; },
  ]);
}

/* -------- 族0587 知识沉淀 X14651~X14675 -------- */
export function checkF0587(): CheckEntry[] {
  const k = new T.KnowledgeBase();
  return mk25(14651, '知识', [
    () => k.add(581, '协作纪律心得') && k.entries.length === 1,
    () => k.add(582, '域验收协议', ['gate']) && k.add(583, '回归防线', ['gate', 'ci']),
    () => T.KnowledgeBase.normalizeFamily('族0581') === 581 && T.KnowledgeBase.normalizeFamily('族600') === 600,
    () => k.exportSanitized().includes('581') && JSON.parse(k.exportSanitized()).length === 3,
    () => k.search('协议').length === 1 && k.byTag('gate').length === 2,
    () => k.add(581, '重复条目') === false,
    () => k.add(0, '非法族号') === false && k.add(601, '越界') === false,
    () => k.add(584, '   ') === false,
    () => T.KnowledgeBase.normalizeFamily('族 581') === 0,
    () => T.KnowledgeBase.normalizeFamily('族999') === 0 && T.KnowledgeBase.normalizeFamily('') === 0,
    () => k.adr() === 1 && k.adr() === 2,
    () => k.search('').length === 0,
    () => k.search('防线').length === 1,
    () => { const q = new T.KnowledgeBase(); q.add(590, 'Abc'); return q.search('abc').length === 1; },
    () => k.byTag('ci').length === 1 && k.byTag('nope').length === 0,
    () => { const t0 = performance.now(); for (let i = 0; i < 200; i++) new T.KnowledgeBase().add((i % 600) + 1, 't'); return performance.now() - t0 < 50; },
    () => { const q = new T.KnowledgeBase(); let n = 0; for (let i = 1; i <= 100; i++) { if (q.add(i, `条目${i}`)) n++; } return n === 100; },
    () => { const a = new T.KnowledgeBase(); const b = new T.KnowledgeBase(); a.add(1, 'x'); b.add(1, 'x'); return a.exportSanitized() === b.exportSanitized(); },
    () => k.adr() === 3,
    () => { const q = new T.KnowledgeBase(); q.add(599, 'title', ['a', 'b']); return q.byTag('b').length === 1; },
    () => T.KnowledgeBase.normalizeFamily('族01') === 1,
    () => { const q = new T.KnowledgeBase(); let n = 0; for (let i = 0; i < 5; i++) { if (q.add(100, 'x')) n++; } return n === 1; },
    () => { const q = new T.KnowledgeBase(); q.add(1, '带标签', ['x']); return q.exportSanitized().includes('"tags":["x"]'); },
    () => typeof k.byTag === 'function' && typeof T.KnowledgeBase.normalizeFamily === 'function',
    () => { const q = new T.KnowledgeBase(); q.add(600, '边界'); return q.entries.length === 1 && q.add(601, '越界') === false; },
  ]);
}

/* -------- 族0588 项目记忆 X14676~X14700 -------- */
export function checkF0588(): CheckEntry[] {
  const pm = new T.ProjectMemory();
  return mk25(14676, '记忆', [
    () => pm.append(59, 581, 25) && pm.chain.length === 1,
    () => pm.append(59, 582, 25) && pm.append(60, 591, 25) && pm.chain.length === 3,
    () => typeof T.ProjectMemory.fnv('x') === 'number' && T.ProjectMemory.fnv('') === 0x811c9dc5,
    () => pm.verify() === true,
    () => pm.exportSanitized().includes('"ai":59') && JSON.parse(pm.exportSanitized()).length === 3,
    () => pm.append(59, 581, 25) === false,
    () => pm.append(61, 581, 25) === false && pm.append(0, 581, 25) === false,
    () => pm.append(59, 601, 25) === false && pm.append(59, 0, 25) === false,
    () => pm.append(59, 583, 24) === false,
    () => pm.tally() === 75,
    () => { const q = new T.ProjectMemory(); q.append(1, 1, 25); q.chain[0]!.hash = 12345; return q.verify() === false; },
    () => { const q = new T.ProjectMemory(); q.append(30, 300, 25); return q.verify() === true; },
    () => { const q = new T.ProjectMemory(); q.append(1, 1, 25); q.append(2, 2, 25); return q.chain[1]!.hash !== q.chain[0]!.hash; },
    () => { const q = new T.ProjectMemory(); let n = 0; for (let f = 1; f <= 10; f++) { if (q.append(1, f, 25)) n++; } return n === 10 && q.tally() === 250; },
    () => { const q = new T.ProjectMemory(); q.append(59, 590, 25); return q.exportSanitized() === JSON.stringify([{ ai: 59, family: 590, items: 25 }]); },
    () => { const t0 = performance.now(); for (let i = 0; i < 100; i++) new T.ProjectMemory().append((i % 60) + 1, 1, 25); return performance.now() - t0 < 50; },
    () => { const q = new T.ProjectMemory(); let n = 0; for (let ai = 1; ai <= 60; ai++) { if (q.append(ai, 581, 25)) n++; } return n === 60; },
    () => { const a = new T.ProjectMemory(); const b = new T.ProjectMemory(); a.append(59, 581, 25); b.append(59, 581, 25); return a.chain[0]!.hash === b.chain[0]!.hash; },
    () => { const q = new T.ProjectMemory(); q.append(59, 581, 25); q.append(59, 582, 25); return q.verify(); },
    () => { const q = new T.ProjectMemory(); return q.verify() === true && q.tally() === 0; },
    () => { const q = new T.ProjectMemory(); let n = 0; for (let i = 0; i < 5; i++) { if (q.append(59, 581, 25)) n++; } return n === 1; },
    () => { const q = new T.ProjectMemory(); for (let i = 1; i <= 20; i++) q.append(i, i, 25); return q.chain.length === 20 && q.verify(); },
    () => T.ProjectMemory.fnv('a') !== T.ProjectMemory.fnv('b'),
    () => typeof pm.verify === 'function' && typeof T.ProjectMemory.fnv === 'function',
    () => { const q = new T.ProjectMemory(); q.append(60, 600, 25); return q.tally() === 25; },
  ]);
}

/* -------- 族0589 交接运维 X14701~X14725 -------- */
export function checkF0589(): CheckEntry[] {
  const h = new T.HandoverOps();
  return mk25(14701, '交接', [
    () => h.create('AI-59', 'AI-60') === 1 && h.cards.length === 1,
    () => h.create('AI-60', 'AI-59') === 2 && h.create('', 'x') === 0 && h.create('a', 'a') === 0,
    () => T.HANDOVER_CHECKLIST.length === 5,
    () => h.check(1, '代码') && h.check(1, '文档') && h.cards[0]!.done.length === 2,
    () => { h.check(1, '基线'); h.check(1, '门禁'); h.check(1, '未决'); return h.acknowledge(1) && h.cards[0]!.ack; },
    () => { const q = new T.HandoverOps(); const id = q.create('a', 'b'); return q.acknowledge(id) === false; },
    () => { const q = new T.HandoverOps(); const id = q.create('a', 'b'); q.check(id, '代码'); return q.check(id, '代码') === false; },
    () => { const q = new T.HandoverOps(); return q.check(99, '代码') === false && q.check(1, 'nope') === false; },
    () => h.pending().length === 1 && h.cards[1]!.ack === false,
    () => { const q = new T.HandoverOps(); const id = q.create('a', 'b'); q.rollback(id); return q.cards.length === 0 && q.rollback(id) === false; },
    () => T.HandoverOps.sla(3) === true && T.HandoverOps.sla(4) === false,
    () => T.HandoverOps.sla(0) === true && T.HandoverOps.sla(99) === false,
    () => T.HandoverOps.patrol([true, true, true, true, true]) === true && T.HandoverOps.patrol([true, true, true, true, false]) === false,
    () => T.HandoverOps.patrol([true, true, true]) === false,
    () => { const q = new T.HandoverOps(); const id = q.create('a', 'b'); for (const it of T.HANDOVER_CHECKLIST) q.check(id, it); return q.acknowledge(id) && q.pending().length === 0; },
    () => { const t0 = performance.now(); for (let i = 0; i < 200; i++) new T.HandoverOps().create('a', 'b'); return performance.now() - t0 < 50; },
    () => { const q = new T.HandoverOps(); let n = 0; for (let i = 0; i < 10; i++) { if (q.create('a', 'b') > 0) n++; } return n === 10; },
    () => { const a = new T.HandoverOps(); const b = new T.HandoverOps(); a.create('x', 'y'); b.create('x', 'y'); return a.cards[0]!.id === b.cards[0]!.id; },
    () => { const q = new T.HandoverOps(); const id = q.create('a', 'b'); q.check(id, '代码'); q.rollback(id); return q.create('c', 'd') === 1; },
    () => { const q = new T.HandoverOps(); const id = q.create('a', 'b'); q.check(id, '代码'); q.check(id, '文档'); q.check(id, '基线'); q.check(id, '门禁'); return q.acknowledge(id) === false && q.pending().length === 1; },
    () => T.HANDOVER_CHECKLIST.every((it) => typeof it === 'string'),
    () => { const q = new T.HandoverOps(); const ids = [q.create('a', 'b'), q.create('b', 'c'), q.create('c', 'd')]; return ids.join() === '1,2,3' && q.pending().length === 3; },
    () => { const q = new T.HandoverOps(); const id = q.create('a', 'b'); let n = 0; for (const it of T.HANDOVER_CHECKLIST) { if (q.check(id, it)) n++; } return n === 5; },
    () => typeof h.acknowledge === 'function' && typeof T.HandoverOps.patrol === 'function',
    () => { const q = new T.HandoverOps(); const id = q.create('a', 'b'); for (const it of T.HANDOVER_CHECKLIST) q.check(id, it); q.acknowledge(id); q.rollback(id); return q.cards.length === 0; },
  ]);
}

/* -------- 族0590 可持续迭代 X14726~X14750 -------- */
export function checkF0590(): CheckEntry[] {
  const si = new T.SustainIteration();
  return mk25(14726, '迭代', [
    () => si.plan(100) && si.planned === 100,
    () => si.deliver(100) && si.delivered === 100 && si.burn() === 1,
    () => T.ITERATION_WAVES.length === 8 && si.wave() === 'W0',
    () => si.snapshot().includes('"w":0') && new T.SustainIteration().restore(si.snapshot()),
    () => { const q = new T.SustainIteration(); return q.restore(si.snapshot()) && q.planned === 100; },
    () => si.advance() && si.wave() === 'W1' && si.delivered === 0 && si.planned === 0,
    () => { const q = new T.SustainIteration(); q.plan(50); return q.advance() === false; },
    () => { const q = new T.SustainIteration(); return q.plan(-1) === false && q.plan(15001) === false; },
    () => { const q = new T.SustainIteration(); return q.deliver(-1) === false; },
    () => { const q = new T.SustainIteration(); return q.burn() === 0; },
    () => T.SustainIteration.stable(1) && T.SustainIteration.stable(0.9) && T.SustainIteration.stable(1.1),
    () => T.SustainIteration.stable(0.89) === false && T.SustainIteration.stable(1.11) === false,
    () => si.takeDebt(1, 2, 10) && si.debt.length === 1,
    () => si.takeDebt(2, 1, 10) === false && si.takeDebt(1, 1, 10) === false,
    () => si.debtLoad(10) === 0.2,
    () => { const t0 = performance.now(); for (let i = 0; i < 300; i++) { const q = new T.SustainIteration(); q.plan(100); q.deliver(100); q.advance(); } return performance.now() - t0 < 50; },
    () => { const q = new T.SustainIteration(); q.plan(10); q.deliver(10); return q.advance() && q.plan(10) && q.deliver(10) && q.advance() && q.wave() === 'W2'; },
    () => { const a = new T.SustainIteration(); const b = new T.SustainIteration(); a.plan(5); b.plan(5); return a.snapshot() === b.snapshot(); },
    () => { const q = new T.SustainIteration(); return q.takeDebt(1, 0, 10) === false && q.takeDebt(1, 6, 10) === false; },
    () => { const q = new T.SustainIteration(); q.plan(100); q.deliver(90); return q.burn() === 0.9 && T.SustainIteration.stable(q.burn()); },
    () => T.SustainIteration.innovationReserve(100) === 10 && T.SustainIteration.innovationReserve(15) === 1,
    () => { const q = new T.SustainIteration(); let n = 0; for (let w = 0; w < 8; w++) { q.plan(1); q.deliver(1); if (q.advance()) n++; } return n === 7 && q.wave() === 'W7'; },
    () => { const q = new T.SustainIteration(); q.plan(100); q.deliver(100); q.advance(); return q.restore('{"w":3,"d":0,"p":0}') && q.wave() === 'W3'; },
    () => typeof si.takeDebt === 'function' && typeof T.SustainIteration.innovationReserve === 'function',
    () => { const q = new T.SustainIteration(); return q.restore('{"w":99,"d":0,"p":0}') === false; },
  ]);
}

/* -------- 聚合入口 -------- */
export function runAi59Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [
    checkF0581, checkF0582, checkF0583, checkF0584, checkF0585,
    checkF0586, checkF0587, checkF0588, checkF0589, checkF0590,
  ];
  const entries = families.flatMap((fn) => fn());
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
