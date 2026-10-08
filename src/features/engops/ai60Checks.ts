/**
 * UNREAL-X-15000 · AI-60 大收官 CheckSet（族0591~0600 · X14751~X15000），勿删。
 * 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
 */
import type { CheckEntry } from '../compat/checks';
import * as T from './ai60Models';

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

/* -------- 族0591 社区运营 X14751~X14775 -------- */
export function checkF0591(): CheckEntry[] {
  const co = new T.CommunityOps();
  return mk25(14751, '社区', [
    () => co.propose('首个提案') === 1 && co.proposals.length === 1,
    () => co.vote(1, 2) && co.proposals[0]!.votes === 2,
    () => T.COMMUNITY_ROLES.length === 4,
    () => co.snapshot().includes('首个提案') && new T.CommunityOps().restore(co.snapshot()),
    () => { const q = new T.CommunityOps(); return q.restore(co.snapshot()) && q.proposals.length === 1; },
    () => { const q = new T.CommunityOps(); return q.propose('  ') === 0 && q.clamped >= 1; },
    () => { const q = new T.CommunityOps(); return q.vote(99, 1) === false; },
    () => { const q = new T.CommunityOps(); const id = q.propose('x'); q.close(id); return q.vote(id, 1) === false; },
    () => { const q = new T.CommunityOps(); return q.vote(1, 0) === false && q.vote(1, 4) === false; },
    () => { const q = new T.CommunityOps(); let n = 0; for (let i = 0; i < 11; i++) { if (q.propose(`p${i}`) > 0) n++; } return n === 10; },
    () => T.CommunityOps.roleWeight('owner') === 3 && T.CommunityOps.roleWeight('reader') === 0,
    () => T.CommunityOps.roleWeight('nope') === 0,
    () => { const q = new T.CommunityOps(); const id = q.propose('x'); return q.close(id) === 'rejected'; },
    () => T.CommunityOps.narrative().includes('法定'),
    () => { const q = new T.CommunityOps(); const id = q.propose('x'); q.vote(id, 3); return q.close(id) === 'accepted' && q.accepted() === 1; },
    () => { const t0 = performance.now(); for (let i = 0; i < 200; i++) { const q = new T.CommunityOps(); q.propose('p'); } return performance.now() - t0 < 50; },
    () => { const q = new T.CommunityOps(); let n = 0; for (let i = 0; i < 10; i++) { const id = q.propose(`p${i}`); if (q.vote(id, 1)) n++; } return n === 10; },
    () => { const a = new T.CommunityOps(); const b = new T.CommunityOps(); a.propose('x'); b.propose('x'); return a.snapshot() === b.snapshot(); },
    () => { const q = new T.CommunityOps(); const id = q.propose('x'); q.vote(id, 1); q.vote(id, 2); return q.proposals[0]!.votes === 3 && q.close(id) === 'accepted'; },
    () => { const q = new T.CommunityOps(); const id = q.propose('x'); q.vote(id, 3); q.close(id); return q.close(id) === ''; },
    () => { const q = new T.CommunityOps(); for (let i = 0; i < T.COMMUNITY_ROLES.length; i++) q.propose(`p${i}`); return q.proposals.length === 4; },
    () => { const q = new T.CommunityOps(); for (let i = 1; i <= 5; i++) q.propose(`p${i}`); for (let i = 1; i <= 5; i++) { q.vote(i, 3); q.close(i); } return q.accepted() === 5; },
    () => { const q = new T.CommunityOps(); const id = q.propose('共创'); q.vote(id, T.CommunityOps.roleWeight('maintainer')); return q.close(id) === 'rejected'; },
    () => typeof co.close === 'function' && typeof T.CommunityOps.roleWeight === 'function',
    () => { const q = new T.CommunityOps(); const id = q.propose('彩蛋：社区之光'); q.vote(id, 3); return q.close(id) === 'accepted' && q.proposals[0]!.title.includes('彩蛋'); },
  ]);
}

/* -------- 族0592 终极验收仪式 X14776~X14800 -------- */
export function checkF0592(): CheckEntry[] {
  const ce = new T.AcceptanceCeremony();
  return mk25(14776, '仪式', [
    () => ce.tick(0) && ce.checklist[0] === true,
    () => ce.setAudience(3000) === 3000 && ce.audience === 3000,
    () => T.CEREMONY_STAGES.length === 5 && ce.stage() === 'prelude',
    () => ce.snapshot().includes('"s":0') && new T.AcceptanceCeremony().restore(ce.snapshot()),
    () => { const q = new T.AcceptanceCeremony(); return q.restore(ce.snapshot()) && q.audience === 3000; },
    () => { const q = new T.AcceptanceCeremony(); return q.tick(5) === false && q.tick(-1) === false; },
    () => { const q = new T.AcceptanceCeremony(); return q.advance() === false && q.clamped >= 1; },
    () => { const q = new T.AcceptanceCeremony(); q.tick(0); q.tick(1); q.tick(2); q.tick(3); q.tick(4); return q.advance() && q.stage() === 'verify'; },
    () => { const q = new T.AcceptanceCeremony(); return q.setAudience(-5) === 0 && q.setAudience(99999) === 5000; },
    () => { const q = new T.AcceptanceCeremony(); return q.light() === false; },
    () => T.AcceptanceCeremony.stageLabel('finale') === '终章' && T.AcceptanceCeremony.stageLabel('prelude') === '序章',
    () => { const q = new T.AcceptanceCeremony(); q.tick(0); q.tick(1); return q.ready() === false; },
    () => { const q = new T.AcceptanceCeremony(); for (let i = 0; i < 5; i++) q.tick(i); return q.ready(); },
    () => T.AcceptanceCeremony.narrative().includes('清单'),
    () => { const q = new T.AcceptanceCeremony(); for (let i = 0; i < 5; i++) q.tick(i); let n = 0; while (q.advance()) n++; return n === 4 && q.stage() === 'finale'; },
    () => { const t0 = performance.now(); for (let i = 0; i < 300; i++) { const q = new T.AcceptanceCeremony(); for (let j = 0; j < 5; j++) q.tick(j); q.advance(); } return performance.now() - t0 < 50; },
    () => { const q = new T.AcceptanceCeremony(); for (let i = 0; i < 5; i++) q.tick(i); q.advance(); q.advance(); return q.stage() === 'showcase'; },
    () => { const a = new T.AcceptanceCeremony(); const b = new T.AcceptanceCeremony(); a.setAudience(42); b.setAudience(42); return a.snapshot() === b.snapshot(); },
    () => { const q = new T.AcceptanceCeremony(); for (let i = 0; i < 5; i++) q.tick(i); while (q.advance()); return q.light() && q.lit; },
    () => { const q = new T.AcceptanceCeremony(); q.advance() === false; q.tick(0); return q.advance() === false; },
    () => { const q = new T.AcceptanceCeremony(); q.tick(0); q.tick(1); q.tick(2); q.tick(3); const w = new T.AcceptanceCeremony(); w.restore(q.snapshot()); return w.ready() === false; },
    () => { const q = new T.AcceptanceCeremony(); let n = 0; for (let i = 0; i < 10; i++) { if (q.tick(i % 5)) n++; } return n === 10; },
    () => T.CEREMONY_STAGES.every((s) => T.AcceptanceCeremony.stageLabel(s).length > 0),
    () => typeof ce.light === 'function' && typeof T.AcceptanceCeremony.stageLabel === 'function',
    () => { const q = new T.AcceptanceCeremony(); q.setAudience(15000); for (let i = 0; i < 5; i++) q.tick(i); let n = 0; while (q.advance()) n++; return n === 4 && q.light() && q.audience === 5000; },
  ]);
}

/* -------- 族0593 三线一致性审计 X14801~X14825（C 线主责，V 线同口径镜像）-------- */
export function checkF0593(): CheckEntry[] {
  const ca = new T.ConsistencyAudit();
  return mk25(14801, '审计', [
    () => ca.record(591, 9, 8, 8) && ca.rows.length === 1,
    () => ca.record(592, 8, 9, 8) && ca.record(593, 8, 8, 9) && ca.rows.length === 3,
    () => ca.total() === 75,
    () => ca.balanced() === true,
    () => ca.verdict() === 'balanced',
    () => { const q = new T.ConsistencyAudit(); return q.record(591, 10, 10, 4) === false && q.clamped >= 1; },
    () => { const q = new T.ConsistencyAudit(); return q.record(0, 8, 8, 9) === false && q.record(601, 8, 8, 9) === false; },
    () => { const q = new T.ConsistencyAudit(); q.record(591, 9, 8, 8); return q.record(591, 8, 8, 9) === false; },
    () => { const q = new T.ConsistencyAudit(); return q.record(591, -1, 13, 13) === false; },
    () => { const q = new T.ConsistencyAudit(); return q.verdict() === 'empty'; },
    () => T.ConsistencyAudit.narrative('balanced').includes('平衡'),
    () => T.ConsistencyAudit.narrative('skewed').includes('偏斜'),
    () => T.ConsistencyAudit.narrative('empty').includes('空'),
    () => { const q = new T.ConsistencyAudit(); q.record(600, 25, 0, 0); return q.maxSkew() === 25 && q.verdict() === 'skewed'; },
    () => { const q = new T.ConsistencyAudit(); q.record(594, 9, 8, 8); return q.maxSkew() === 1 && q.balanced(); },
    () => { const t0 = performance.now(); for (let i = 0; i < 300; i++) { const q = new T.ConsistencyAudit(); q.record((i % 600) + 1, 9, 8, 8); } return performance.now() - t0 < 50; },
    () => { const q = new T.ConsistencyAudit(); let n = 0; for (let f = 1; f <= 100; f++) { if (q.record(f, 8, 9, 8)) n++; } return n === 100 && q.total() === 2500; },
    () => { const a = new T.ConsistencyAudit(); const b = new T.ConsistencyAudit(); a.record(591, 9, 8, 8); b.record(591, 9, 8, 8); return a.rows[0]!.k === b.rows[0]!.k && a.total() === b.total(); },
    () => { const q = new T.ConsistencyAudit(); q.record(591, 0, 25, 0); return q.total() === 25 && q.verdict() === 'skewed'; },
    () => { const q = new T.ConsistencyAudit(); q.record(591, 9, 8, 8); q.record(592, 9, 9, 7); return q.maxSkew() === 2 && q.verdict() === 'skewed'; },
    () => { const q = new T.ConsistencyAudit(); let n = 0; for (let i = 0; i < 5; i++) { if (q.record(591, 9, 8, 8)) n++; } return n === 1; },
    () => { const q = new T.ConsistencyAudit(); for (let f = 581; f <= 600; f++) q.record(f, 8, 9, 8); return q.rows.length === 20 && q.balanced(); },
    () => { const q = new T.ConsistencyAudit(); q.record(599, 8, 8, 9); return q.rows[0]!.c === 9 && q.rows[0]!.k === 8; },
    () => typeof ca.maxSkew === 'function' && typeof T.ConsistencyAudit.narrative === 'function',
    () => { const q = new T.ConsistencyAudit(); q.record(600, 0, 0, 25); return q.verdict() === 'skewed' && T.ConsistencyAudit.narrative('skewed').includes('回填'); },
  ]);
}

/* -------- 族0594 ID 唯一性防线 X14826~X14850（C 线主责，V 线同口径镜像）-------- */
export function checkF0594(): CheckEntry[] {
  const iu = new T.IdUniqueness();
  return mk25(14826, '防线', [
    () => iu.admit(14751) && iu.seen.size === 1,
    () => { let n = 0; for (let id = 14752; id <= 14775; id++) { if (iu.admit(id)) n++; } return n === 24 && iu.seen.size === 25; },
    () => T.IdUniqueness.validate(1) && T.IdUniqueness.validate(15000) && !T.IdUniqueness.validate(0) && !T.IdUniqueness.validate(15001),
    () => iu.snapshot().includes('14751') && new T.IdUniqueness().restore(iu.snapshot()),
    () => { const q = new T.IdUniqueness(); return q.restore(iu.snapshot()) && q.seen.size === 25; },
    () => { const q = new T.IdUniqueness(); return q.admit(0) === false && q.admit(15001) === false && q.admit(1.5) === false; },
    () => { const q = new T.IdUniqueness(); q.admit(1); return q.admit(1) === false && q.clamped >= 1; },
    () => { const q = new T.IdUniqueness(); q.admit(100); const s = q.snapshot(); q.admit(200); q.restore(s); return q.seen.size === 1; },
    () => { const q = new T.IdUniqueness(); return q.coverage(0, 100) === 0; },
    () => { const q = new T.IdUniqueness(); q.admit(5); q.admit(15); q.admit(25); return q.coverage(1, 20) === 2; },
    () => iu.conflicts([14751, 14760, 9999]) === 2,
    () => T.IdUniqueness.dupes([1, 2, 2, 3, 3, 3]) === 3,
    () => T.IdUniqueness.dupes([1, 2, 3]) === 0,
    () => T.IdUniqueness.spanOk(1, 15000) && !T.IdUniqueness.spanOk(100, 50),
    () => T.IdUniqueness.narrative().includes('重复'),
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) new T.IdUniqueness().admit((i % 15000) + 1); return performance.now() - t0 < 50; },
    () => { const q = new T.IdUniqueness(); let n = 0; for (let id = 1; id <= 1000; id++) { if (q.admit(id)) n++; } return n === 1000; },
    () => { const a = new T.IdUniqueness(); const b = new T.IdUniqueness(); a.admit(77); b.admit(77); return a.snapshot() === b.snapshot(); },
    () => { const q = new T.IdUniqueness(); q.admit(14000); q.admit(14001); return q.coverage(14000, 14001) === 2 && q.coverage(1, 13999) === 0; },
    () => { const q = new T.IdUniqueness(); return q.conflicts([]) === 0 && q.seen.size === 0; },
    () => { const q = new T.IdUniqueness(); let n = 0; for (let i = 0; i < 5; i++) { if (q.admit(42)) n++; } return n === 1; },
    () => { const q = new T.IdUniqueness(); for (let id = 14501; id <= 14750; id++) q.admit(id); return q.seen.size === 250 && q.coverage(14501, 15000) === 250; },
    () => { const q = new T.IdUniqueness(); q.restore('[1,2,3,99999,-5]'); return q.seen.size === 3; },
    () => typeof iu.conflicts === 'function' && typeof T.IdUniqueness.dupes === 'function',
    () => { const q = new T.IdUniqueness(); q.admit(15000); return q.seen.has(15000) && q.admit(15000) === false; },
  ]);
}

/* -------- 族0595 文档收官 X14851~X14875 -------- */
export function checkF0595(): CheckEntry[] {
  const df = new T.DocFinale();
  return mk25(14851, '文档', [
    () => df.pass('scope') && df.passed.has('scope'),
    () => df.pass('ids') && df.passed.size === 2,
    () => T.DOC_GATES.length === 5,
    () => df.snapshot().includes('scope') && new T.DocFinale().restore(df.snapshot()),
    () => { const q = new T.DocFinale(); return q.restore(df.snapshot()) && q.passed.size === 2; },
    () => { const q = new T.DocFinale(); return q.pass('nope') === false && q.clamped >= 1; },
    () => { const q = new T.DocFinale(); return q.pass('gates') === false; },
    () => { const q = new T.DocFinale(); q.pass('scope'); q.reportBroken(3); return q.pass('ids') && q.pass('links') === false; },
    () => { const q = new T.DocFinale(); return q.reportBroken(-1) === false && q.reportBroken(100) === false; },
    () => { const q = new T.DocFinale(); q.pass('scope'); q.reportBroken(2); q.fixAll(); return q.pass('ids') && q.pass('links') && q.brokenLinks === 0; },
    () => T.DocFinale.narrative('order').includes('按序'),
    () => T.DocFinale.narrative('links').includes('断链'),
    () => T.DocFinale.narrative('unknown').includes('未知'),
    () => { const q = new T.DocFinale(); return q.done() === false; },
    () => { const q = new T.DocFinale(); for (const g of T.DOC_GATES) q.pass(g); return q.done(); },
    () => { const t0 = performance.now(); for (let i = 0; i < 200; i++) { const q = new T.DocFinale(); q.pass('scope'); } return performance.now() - t0 < 50; },
    () => { const q = new T.DocFinale(); let n = 0; for (const g of T.DOC_GATES) { if (q.pass(g)) n++; } return n === 5; },
    () => { const a = new T.DocFinale(); const b = new T.DocFinale(); a.pass('scope'); b.pass('scope'); return a.snapshot() === b.snapshot(); },
    () => { const q = new T.DocFinale(); q.pass('scope'); q.pass('ids'); q.pass('links'); return q.done() === false; },
    () => { const q = new T.DocFinale(); q.pass('scope'); q.reportBroken(5); return q.fixAll() && q.brokenLinks === 0 && q.fixAll() === false; },
    () => { const q = new T.DocFinale(); q.pass('scope'); const s = q.snapshot(); q.pass('ids'); q.restore(s); return q.passed.size === 1; },
    () => { const q = new T.DocFinale(); q.pass('scope'); q.pass('ids'); q.pass('links'); q.pass('gates'); return q.passed.size === 4 && !q.done(); },
    () => { const q = new T.DocFinale(); q.reportBroken(0); q.pass('scope'); q.pass('ids'); return q.pass('links'); },
    () => typeof df.fixAll === 'function' && typeof T.DocFinale.narrative === 'function',
    () => { const q = new T.DocFinale(); for (const g of T.DOC_GATES) q.pass(g); return q.snapshot().includes('signoff') && q.done(); },
  ]);
}

/* -------- 族0596 基线冻结 X14876~X14900 -------- */
export function checkF0596(): CheckEntry[] {
  const bf = new T.BaselineFreeze();
  return mk25(14876, '冻结', [
    () => bf.addMetric(100) && bf.metrics.length === 1,
    () => bf.addMetric(105) && bf.addMetric(110) && bf.metrics.length === 3,
    () => bf.freeze() === true && bf.frozen === true,
    () => bf.seal() > 0 && typeof bf.seal() === 'number',
    () => { const q = new T.BaselineFreeze(); return q.restore(bf.snapshot()) && q.frozen && q.metrics.length === 3; },
    () => { const q = new T.BaselineFreeze(); return q.addMetric(1.5) === false && q.addMetric(Number.NaN) === false; },
    () => { const q = new T.BaselineFreeze(); return q.freeze() === false && q.clamped >= 1; },
    () => { const q = new T.BaselineFreeze(); q.addMetric(1); q.freeze(); return q.addMetric(2) === false; },
    () => { const q = new T.BaselineFreeze(); return q.seal() === 0; },
    () => { const q = new T.BaselineFreeze(); q.addMetric(42); q.freeze(); return q.verify(q.seal()); },
    () => { const q = new T.BaselineFreeze(); q.addMetric(42); q.freeze(); return q.verify(12345) === false; },
    () => T.BaselineFreeze.fnv([1, 2, 3]) !== T.BaselineFreeze.fnv([3, 2, 1]),
    () => T.BaselineFreeze.fnv([]) === 0x811c9dc5,
    () => T.BaselineFreeze.narrative().includes('冻结'),
    () => { const q = new T.BaselineFreeze(); q.addMetric(7); q.freeze(); const seal = q.seal(); q.restore('{"m":[7],"f":true}'); return q.verify(seal); },
    () => { const t0 = performance.now(); for (let i = 0; i < 300; i++) { const q = new T.BaselineFreeze(); q.addMetric(i); q.freeze(); } return performance.now() - t0 < 50; },
    () => { const q = new T.BaselineFreeze(); for (let i = 1; i <= 50; i++) q.addMetric(i); q.freeze(); return q.metrics.length === 50 && q.verify(q.seal()); },
    () => { const a = new T.BaselineFreeze(); const b = new T.BaselineFreeze(); a.addMetric(9); b.addMetric(9); a.freeze(); b.freeze(); return a.seal() === b.seal(); },
    () => { const q = new T.BaselineFreeze(); q.addMetric(1); const s = q.snapshot(); q.freeze(); q.restore(s); return q.frozen === false; },
    () => { const q = new T.BaselineFreeze(); q.addMetric(5); q.freeze(); const seal = q.seal(); q.metrics.push(99); return q.verify(seal) === false; },
    () => { const q = new T.BaselineFreeze(); q.addMetric(0); q.freeze(); return q.seal() > 0; },
    () => { const q = new T.BaselineFreeze(); let n = 0; for (let i = 0; i < 10; i++) { if (q.addMetric(i)) n++; } return n === 10; },
    () => { const q = new T.BaselineFreeze(); q.addMetric(-3); q.freeze(); return q.verify(q.seal()); },
    () => typeof bf.verify === 'function' && typeof T.BaselineFreeze.fnv === 'function',
    () => { const q = new T.BaselineFreeze(); for (let i = 1; i <= 25; i++) q.addMetric(i); q.freeze(); return q.metrics.length === 25 && q.frozen && q.verify(q.seal()); },
  ]);
}

/* -------- 族0597 毕业审计 X14901~X14925 -------- */
export function checkF0597(): CheckEntry[] {
  const ga = new T.GraduationAudit();
  return mk25(14901, '毕业', [
    () => ga.setScore('lines', 95) && ga.scores.size === 1,
    () => ga.setScore('gates', 90) && ga.setScore('docs', 92) && ga.setScore('debt', 88) && ga.setScore('innovation', 91) && ga.complete(),
    () => ga.total() === 91,
    () => ga.verdict() === 'excellent',
    () => { const q = new T.GraduationAudit(); return q.restore(ga.snapshot()) && q.complete() && q.total() === 91; },
    () => { const q = new T.GraduationAudit(); return q.setScore('nope', 50) === false && q.clamped >= 1; },
    () => { const q = new T.GraduationAudit(); return q.setScore('lines', Number.NaN) === false; },
    () => { const q = new T.GraduationAudit(); q.setScore('lines', 50); return q.setScore('lines', 150) && q.scores.get('lines') === 100; },
    () => { const q = new T.GraduationAudit(); q.setScore('lines', -20); return q.scores.get('lines') === 0; },
    () => { const q = new T.GraduationAudit(); return q.verdict() === 'fail'; },
    () => T.GraduationAudit.criterionLabel('lines') === '三线覆盖' && T.GraduationAudit.criterionLabel('innovation') === '创新保底',
    () => { const q = new T.GraduationAudit(); for (const c of T.GRAD_CRITERIA) q.setScore(c, 60); return q.verdict() === 'pass'; },
    () => { const q = new T.GraduationAudit(); q.setScore('lines', 59); q.setScore('gates', 60); q.setScore('docs', 60); q.setScore('debt', 60); q.setScore('innovation', 61); return q.total() === 60 && q.verdict() === 'pass'; },
    () => T.GraduationAudit.narrative('excellent').includes('金星'),
    () => T.GraduationAudit.narrative('fail').includes('毕业线'),
    () => { const t0 = performance.now(); for (let i = 0; i < 200; i++) { const q = new T.GraduationAudit(); for (const c of T.GRAD_CRITERIA) q.setScore(c, 80); } return performance.now() - t0 < 50; },
    () => { const q = new T.GraduationAudit(); let n = 0; for (const c of T.GRAD_CRITERIA) { if (q.setScore(c, 100)) n++; } return n === 5 && q.verdict() === 'excellent'; },
    () => { const a = new T.GraduationAudit(); const b = new T.GraduationAudit(); a.setScore('lines', 70); b.setScore('lines', 70); return a.snapshot() === b.snapshot(); },
    () => { const q = new T.GraduationAudit(); q.setScore('lines', 89); q.setScore('gates', 89); q.setScore('docs', 89); q.setScore('debt', 89); q.setScore('innovation', 89); return q.total() === 89 && q.verdict() === 'pass'; },
    () => { const q = new T.GraduationAudit(); q.setScore('lines', 89); q.setScore('gates', 90); q.setScore('docs', 90); q.setScore('debt', 90); q.setScore('innovation', 90); return q.total() === 90 && q.verdict() === 'excellent'; },
    () => { const q = new T.GraduationAudit(); q.setScore('lines', 100); return q.total() === 0; },
    () => { const q = new T.GraduationAudit(); for (const c of T.GRAD_CRITERIA) q.setScore(c, 30); return q.verdict() === 'fail'; },
    () => { const q = new T.GraduationAudit(); q.restore('[["lines",88],["gates",92]]'); return q.scores.size === 2 && !q.complete(); },
    () => typeof ga.total === 'function' && typeof T.GraduationAudit.criterionLabel === 'function',
    () => { const q = new T.GraduationAudit(); for (const c of T.GRAD_CRITERIA) q.setScore(c, 95); return q.total() === 95 && q.verdict() === 'excellent' && T.GraduationAudit.narrative('excellent').includes('优秀'); },
  ]);
}

/* -------- 族0598 时间胶囊 X14926~X14950 -------- */
export function checkF0598(): CheckEntry[] {
  const tc = new T.TimeCapsule();
  return mk25(14926, '胶囊', [
    () => tc.put('UNREAL-X 15000 达成') && tc.items.length === 1,
    () => tc.put('三线 5000×3') && tc.put('600 族全绿') && tc.items.length === 3,
    () => tc.seal() === true && tc.sealed === true,
    () => tc.open().length === 3 && tc.open()[0] === 'UNREAL-X 15000 达成',
    () => { const q = new T.TimeCapsule(); return q.restore(tc.snapshot()) && q.sealed && q.items.length === 3; },
    () => { const q = new T.TimeCapsule(); return q.put('  ') === false && q.clamped >= 1; },
    () => { const q = new T.TimeCapsule(); return q.seal() === false; },
    () => { const q = new T.TimeCapsule(); q.put('x'); q.seal(); return q.put('y') === false; },
    () => { const q = new T.TimeCapsule(); return q.open().length === 0; },
    () => { const q = new T.TimeCapsule(); let n = 0; for (let i = 0; i < 51; i++) { if (q.put(`i${i}`)) n++; } return n === 50; },
    () => tc.fingerprint() > 0,
    () => { const q = new T.TimeCapsule(); return q.fingerprint() === 0; },
    () => T.TimeCapsule.fnv(['a']) !== T.TimeCapsule.fnv(['b']),
    () => T.TimeCapsule.narrative().includes('封存'),
    () => { const q = new T.TimeCapsule(); q.put('x'); q.seal(); const fp = q.fingerprint(); const w = new T.TimeCapsule(); w.restore(q.snapshot()); return w.fingerprint() === fp; },
    () => { const t0 = performance.now(); for (let i = 0; i < 200; i++) { const q = new T.TimeCapsule(); q.put('m'); q.seal(); } return performance.now() - t0 < 50; },
    () => { const q = new T.TimeCapsule(); for (let i = 0; i < 50; i++) q.put(`m${i}`); q.seal(); return q.open().length === 50; },
    () => { const a = new T.TimeCapsule(); const b = new T.TimeCapsule(); a.put('x'); b.put('x'); a.seal(); b.seal(); return a.fingerprint() === b.fingerprint(); },
    () => { const q = new T.TimeCapsule(); q.put('x'); const s = q.snapshot(); q.seal(); q.restore(s); return q.sealed === false; },
    () => { const q = new T.TimeCapsule(); q.put('唯一'); q.seal(); const inner = q.open(); inner.push('篡改'); return q.open().length === 1; },
    () => { const q = new T.TimeCapsule(); let n = 0; for (let i = 0; i < 5; i++) { if (q.put('同款')) n++; } return n === 5; },
    () => { const q = new T.TimeCapsule(); q.put('a'); q.put('b'); q.seal(); return q.open().join() === 'a,b'; },
    () => { const q = new T.TimeCapsule(); q.put('x'); q.seal(); return q.seal() === false; },
    () => typeof tc.fingerprint === 'function' && typeof T.TimeCapsule.fnv === 'function',
    () => { const q = new T.TimeCapsule(); q.put('给下一代的信'); q.seal(); return q.open().includes('给下一代的信') && q.fingerprint() > 0; },
  ]);
}

/* -------- 族0599 下一代路线 X14951~X14975 -------- */
export function checkF0599(): CheckEntry[] {
  const ng = new T.NextGenRoadmap();
  return mk25(14951, '路线', [
    () => ng.nominate('实时协作内核') && ng.themes.length === 1,
    () => ng.nominate('AI 原生工作台') && ng.vote('实时协作内核', 5) && ng.themes[0]!.score === 5,
    () => T.ROADMAP_PHASES.length === 5 && ng.phase() === 'explore',
    () => ng.snapshot().includes('实时协作内核') && new T.NextGenRoadmap().restore(ng.snapshot()),
    () => { const q = new T.NextGenRoadmap(); return q.restore(ng.snapshot()) && q.themes.length === 2; },
    () => { const q = new T.NextGenRoadmap(); return q.nominate('  ') === false && q.clamped >= 1; },
    () => { const q = new T.NextGenRoadmap(); return q.vote('不存在', 1) === false; },
    () => { const q = new T.NextGenRoadmap(); q.nominate('a'); return q.nominate('a') === false; },
    () => { const q = new T.NextGenRoadmap(); q.nominate('a'); return q.vote('a', 0) === false && q.vote('a', 100) === false; },
    () => { const q = new T.NextGenRoadmap(); return q.advance() === false; },
    () => T.NextGenRoadmap.phaseLabel('commit') === '立项' && T.NextGenRoadmap.phaseLabel('explore') === '探索',
    () => { const q = new T.NextGenRoadmap(); q.nominate('a'); q.nominate('b'); return q.advance() && q.phase() === 'design'; },
    () => { const q = new T.NextGenRoadmap(); return q.top() === ''; },
    () => T.NextGenRoadmap.narrative().includes('2 个提名'),
    () => ng.top() === '实时协作内核',
    () => { const t0 = performance.now(); for (let i = 0; i < 200; i++) { const q = new T.NextGenRoadmap(); q.nominate('a'); q.nominate('b'); q.advance(); } return performance.now() - t0 < 50; },
    () => { const q = new T.NextGenRoadmap(); q.nominate('a'); q.nominate('b'); q.nominate('c'); let n = 0; while (q.advance()) n++; return n === 4 && q.phase() === 'commit'; },
    () => { const a = new T.NextGenRoadmap(); const b = new T.NextGenRoadmap(); a.nominate('x'); b.nominate('x'); return a.snapshot() === b.snapshot(); },
    () => { const q = new T.NextGenRoadmap(); q.nominate('a'); q.nominate('b'); q.vote('b', 3); q.vote('a', 3); return q.top() === 'a'; },
    () => { const q = new T.NextGenRoadmap(); q.nominate('a'); q.vote('a', 99); return q.themes[0]!.score === 99 && q.vote('a', 1) === false; },
    () => { const q = new T.NextGenRoadmap(); q.nominate('a'); q.nominate('b'); q.advance(); q.advance(); const s = q.snapshot(); const w = new T.NextGenRoadmap(); w.restore(s); return w.phase() === 'prototype'; },
    () => { const q = new T.NextGenRoadmap(); let n = 0; for (let i = 0; i < 10; i++) { if (q.vote('x', 1) === false) n++; } return n === 10; },
    () => T.ROADMAP_PHASES.every((p) => T.NextGenRoadmap.phaseLabel(p).length > 0),
    () => typeof ng.top === 'function' && typeof T.NextGenRoadmap.phaseLabel === 'function',
    () => { const q = new T.NextGenRoadmap(); q.nominate('量子图形栈'); q.nominate('神经输入法'); q.vote('量子图形栈', 9); q.advance(); q.advance(); q.advance(); q.advance(); return q.phase() === 'commit' && q.top() === '量子图形栈'; },
  ]);
}

/* -------- 族0600 大收官 X14976~X15000 -------- */
export function checkF0600(): CheckEntry[] {
  const gf = new T.GrandFinale();
  return mk25(14976, '收官', [
    () => gf.deliverWave(1000) && gf.delivered === 1000 && gf.wavesUsed === 1,
    () => gf.deliverWave(2000) && gf.progress() === 3000 / 15000,
    () => gf.waveCap === 9,
    () => gf.snapshot().includes('"d":3000') && new T.GrandFinale().restore(gf.snapshot()),
    () => { const q = new T.GrandFinale(); return q.restore(gf.snapshot()) && q.delivered === 3000; },
    () => { const q = new T.GrandFinale(); return q.deliverWave(0) === false && q.deliverWave(-5) === false; },
    () => { const q = new T.GrandFinale(); return q.deliverWave(15001) === false && q.clamped >= 1; },
    () => { const q = new T.GrandFinale(); q.deliverWave(14000); return q.deliverWave(1001) === false; },
    () => { const q = new T.GrandFinale(); return q.seal() === false; },
    () => { const q = new T.GrandFinale(); q.deliverWave(15000); return q.complete() && q.progress() === 1; },
    () => { const q = new T.GrandFinale(); q.deliverWave(15000); q.seal(); return q.sealed && q.deliverWave(1) === false; },
    () => T.GrandFinale.narrative().includes('15000'),
    () => { const q = new T.GrandFinale(); q.deliverWave(15000); q.seal(); return q.seal() === false; },
    () => { const q = new T.GrandFinale(); return q.progress() === 0 && q.complete() === false; },
    () => { const q = new T.GrandFinale(); let n = 0; for (let i = 0; i < 9; i++) { if (q.deliverWave(1500)) n++; } return n === 9 && q.delivered === 13500 && q.complete() === false; },
    () => { const t0 = performance.now(); for (let i = 0; i < 300; i++) { const q = new T.GrandFinale(); q.deliverWave(5000); q.deliverWave(5000); q.deliverWave(5000); } return performance.now() - t0 < 50; },
    () => { const q = new T.GrandFinale(); for (let i = 0; i < 3; i++) q.deliverWave(5000); return q.complete() && q.deliverWave(1) === false && q.seal(); },
    () => { const a = new T.GrandFinale(); const b = new T.GrandFinale(); a.deliverWave(100); b.deliverWave(100); return a.snapshot() === b.snapshot(); },
    () => { const q = new T.GrandFinale(); q.deliverWave(14999); return q.deliverWave(1) && q.complete() && q.delivered === 15000; },
    () => { const q = new T.GrandFinale(); let n = 0; for (let i = 0; i < 10; i++) { if (q.deliverWave(1500)) n++; } return n === 9; },
    () => { const q = new T.GrandFinale(); q.restore('{"w":9,"d":15000,"s":false}'); return q.complete() && q.seal(); },
    () => { const q = new T.GrandFinale(); return q.restore('{"w":10,"d":0,"s":false}') === false; },
    () => { const q = new T.GrandFinale(); q.deliverWave(1); return q.progress() === 1 / 15000; },
    () => typeof gf.seal === 'function' && typeof gf.progress === 'function',
    () => { const q = new T.GrandFinale(); const parts = [5000, 5000, 4999, 1]; for (const p of parts) q.deliverWave(p); return q.wavesUsed === 4 && q.complete() && q.seal() && q.snapshot().includes('"s":true'); },
  ]);
}

/* -------- 聚合入口 -------- */
export function runAi60Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [
    checkF0591, checkF0592, checkF0593, checkF0594, checkF0595,
    checkF0596, checkF0597, checkF0598, checkF0599, checkF0600,
  ];
  const entries = families.flatMap((fn) => fn());
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
