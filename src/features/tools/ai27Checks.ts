/**
 * UNREAL-X-15000 · AI-27 工具智能与联动 V 线 CheckSet（族0261/0262/0264/0265/0266 · X06501~X06550 / X06576~X06650），勿删。
 * 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
 * 族0263（X06551~X06575）与族0267~0270（X06651~X06750）为 C/K 线，本文件不占用。
 */
import type { CheckEntry } from '../uikit/checks';
import * as T from './ai27Models';

/* -------- 族0261 学习工具 2.0 X06501~X06525 -------- */
export function checkF0261(): CheckEntry[] {
  return [
    { id: 'X06501', name: '学习·最小闭环', check: () => { const q = new T.LearnTool(); q.addCard('w1'); const r = q.review('w1', 5); return !!r && r.reps === 1 && r.interval === 1 && Math.abs(r.ease - 2.6) < 1e-9 && q.stats.streak === 1; } },
    { id: 'X06502', name: '学习·全量参数', check: () => { const p = new T.LearnTool('hard').profile; return p.eFactorBase === 2.4 && p.maxNewPerDay === 30 && p.focusMinutes === 40 && p.reviewCap === 100; } },
    { id: 'X06503', name: '学习·档位矩阵', check: () => T.LEARN_DIFFICULTY_LEVELS.length === 5 && T.DEFAULT_LEARN_DIFFICULTY === 'normal' && Object.keys(T.LEARN_DIFFICULTY_MATRIX).length === 5 },
    { id: 'X06504', name: '学习·快照迁移', check: () => { const q = new T.LearnTool('easy'); q.addCard('a'); q.review('a', 1); const r = T.LearnTool.deserialize(q.serialize()); return r.difficulty === 'easy' && r.wrongBook.includes('a') && r.stats.reviewed === 1; } },
    { id: 'X06505', name: '学习·联调集成', check: () => { const q = new T.LearnTool(); q.addCard('w'); q.review('w', 5); const r = q.review('w', 5); return !!r && r.reps === 2 && r.interval === 6 && Math.abs(r.ease - 2.7) < 1e-9; } },
    { id: 'X06506', name: '学习·越界钳制', check: () => { const q = new T.LearnTool('zzz'); return q.difficulty === T.DEFAULT_LEARN_DIFFICULTY && q.clamped === 1 && T.clampEase(99) === T.SM2_MAX_EASE && T.clampEase(-1) === T.SM2_MIN_EASE; } },
    { id: 'X06507', name: '学习·失败叙事', check: () => T.LearnTool.deserialize('{bad').difficulty === 'normal' && new T.LearnTool().review('ghost', 5) === null },
    { id: 'X06508', name: '学习·中断还原', check: () => { const q = T.LearnTool.deserialize(JSON.stringify({ difficulty: 'master' })); return q.difficulty === 'master' && q.cards.size === 0 && q.stats.reviewed === 0; } },
    { id: 'X06509', name: '学习·资源降级', check: () => { const q = new T.LearnTool(); let done = 0; for (let i = 0; i < 20; i++) { q.focusStart(); if (q.focusTick(30) === 30) done += 1; } return done === 20 && q.history.length === T.FOCUS_HISTORY_CAP && q.focusMinutes === 0; } },
    { id: 'X06510', name: '学习·净身', check: () => { const q = new T.LearnTool(); q.addCard('a'); q.review('a', 2); q.focusStart(); q.focusTick(30); q.reset(); return q.cards.size === 0 && q.wrongBook.length === 0 && q.stats.reviewed === 0 && q.focusMinutes === 0 && !q.focusRunning && q.history.length === 0; } },
    { id: 'X06511', name: '学习·动效令牌', check: () => T.LEARN_DIFFICULTY_MATRIX.kids.focusMinutes === 15 && T.LEARN_DIFFICULTY_MATRIX.master.focusMinutes === 50 },
    { id: 'X06512', name: '学习·三态焦点', check: () => { const q = new T.LearnTool(); const a = q.focusTick(5); q.focusStart(); const b = q.focusTick(10); const c = q.focusTick(20); return a === 0 && b === 10 && c === 30 && q.history[0] === 30 && !q.focusRunning; } },
    { id: 'X06513', name: '学习·键盘序', check: () => T.LEARN_DIFFICULTY_LEVELS.every((p) => new T.LearnTool(p).difficulty === p) },
    { id: 'X06514', name: '学习·微文案', check: () => { const q = new T.LearnTool(); q.addCard('a'); q.addCard('b'); q.review('a', 5); q.review('b', 2); return q.accuracy === 0.5 && q.stats.streak === 0; } },
    { id: 'X06515', name: '学习·aria 等价', check: () => { const q = new T.LearnTool(); q.addCard('a'); for (let i = 0; i < 3; i++) q.review('a', 5); return q.stats.bestStreak === 3 && q.stats.correct === 3; } },
    { id: 'X06516', name: '学习·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.sm2Review({ id: 'w', ease: 2.5, interval: 1, reps: 1, lapses: 0 }, 4); return performance.now() - t0 < 50; } },
    { id: 'X06517', name: '学习·热路径', check: () => new T.LearnTool().review('missing', 5) === null },
    { id: 'X06518', name: '学习·零漂移', check: () => { const q = new T.LearnTool('hard'); q.addCard('a'); q.review('a', 4); const a = T.LearnTool.deserialize(q.serialize()); const b = T.LearnTool.deserialize(a.serialize()); return b.serialize() === a.serialize() && b.serialize() === q.serialize(); } },
    { id: 'X06519', name: '学习·低配减档', check: () => { const p = new T.LearnTool('kids').profile; return p.maxNewPerDay === 5 && p.reviewCap === 20 && p.focusMinutes === 15; } },
    { id: 'X06520', name: '学习·守卫', check: () => { const q = new T.LearnTool('kids'); return q.canAddNew(4) && !q.canAddNew(5) && q.canReview(19) && !q.canReview(20); } },
    { id: 'X06521', name: '学习·智能建议', check: () => { const q = new T.LearnTool(); q.addCard('a'); const s0 = q.suggest('a'); q.review('a', 5); return s0 === '新学' && q.suggest('a') === '1天后复习'; } },
    { id: 'X06522', name: '学习·批量模式', check: () => { const q = new T.LearnTool(); let n = 0; for (let i = 0; i < 10; i++) { q.addCard(`w${i}`); if (q.review(`w${i}`, 5)) n += 1; } return n === 10 && q.cards.size === 10; } },
    { id: 'X06523', name: '学习·跨域联动', check: () => { const q = new T.LearnTool('master'); q.focusStart(); const a = q.focusTick(49); const b = q.focusTick(1); return a === 49 && b === 50 && q.history.length === 1 && T.LEARN_DIFFICULTY_MATRIX.master.focusMinutes === 50; } },
    { id: 'X06524', name: '学习·扩展点', check: () => { const q = new T.LearnTool(); q.addCard('a'); q.review('a', 1); const ok = q.clearWrong('a'); return ok === true && q.wrongBook.length === 0 && q.clearWrong('a') === false && typeof T.sm2Review === 'function' && typeof T.LearnTool.deserialize === 'function'; } },
    { id: 'X06525', name: '学习·彩蛋层', check: () => { const q = new T.LearnTool(); q.addCard('a'); for (let i = 0; i < 10; i++) q.review('a', 5); return q.eggs[0] === 'egg-streak-10' && q.eggs.length === 1 && T.LEARN_EGG_MILESTONES.length === 3; } },
  ];
}

/* -------- 族0262 家庭模式 2.0 X06526~X06550 -------- */
export function checkF0262(): CheckEntry[] {
  return [
    { id: 'X06526', name: '家庭·最小闭环', check: () => { const q = new T.FamilyMode(); return q.level === 'child' && q.profile.dailyLimitMin === 60 && q.profile.filterOn === true; } },
    { id: 'X06527', name: '家庭·全量参数', check: () => { const p = new T.FamilyMode('toddler').profile; return p.dailyLimitMin === 30 && p.sessionLimitMin === 10 && p.filterOn === true && p.allowedHours.length === 9 && p.allowedHours[0] === 9; } },
    { id: 'X06528', name: '家庭·档位矩阵', check: () => T.FAMILY_MODE_LEVELS.length === 5 && T.DEFAULT_FAMILY_LEVEL === 'child' && Object.keys(T.FAMILY_MODE_MATRIX).length === 5 },
    { id: 'X06529', name: '家庭·快照迁移', check: () => { const q = new T.FamilyMode('teen'); q.setPin('1234'); q.addKeyword('刀'); const r = T.FamilyMode.deserialize(q.serialize()); return r.level === 'teen' && r.pinHash === q.pinHash && r.keywords.includes('刀'); } },
    { id: 'X06530', name: '家庭·联调集成', check: () => { const q = new T.FamilyMode(); q.setPin('2024'); q.addKeyword('赌博'); const a = q.verifyPin('2024'); const s = q.screen('拒绝赌博信息'); return a === true && s.ok === false && s.hits[0] === '赌博'; } },
    { id: 'X06531', name: '家庭·越界钳制', check: () => { const q = new T.FamilyMode('nope'); return q.level === T.DEFAULT_FAMILY_LEVEL && q.clamped === 1 && q.isHourAllowed(24) === false && q.isHourAllowed(-1) === false; } },
    { id: 'X06532', name: '家庭·失败叙事', check: () => { const q = new T.FamilyMode(); return q.setPin('12ab') === false && q.setPin('123') === false && q.setPin('1234567') === false && q.verifyPin('0000') === false; } },
    { id: 'X06533', name: '家庭·中断还原', check: () => { const r = T.FamilyMode.deserialize('{}'); return r.level === T.DEFAULT_FAMILY_LEVEL && r.pinHash === null && r.usedTodayMin === 0 && T.FamilyMode.deserialize('bad').level === 'child'; } },
    { id: 'X06534', name: '家庭·资源降级', check: () => { const q = new T.FamilyMode(); let added = 0; for (let i = 0; i < 70; i++) { if (q.addKeyword(`词${i}`)) added += 1; } return added === T.FAMILY_KEYWORD_CAP && q.keywords.length === T.FAMILY_KEYWORD_CAP; } },
    { id: 'X06535', name: '家庭·净身', check: () => { const q = new T.FamilyMode(); q.addKeyword('x'); q.recordUsage(9, 30); q.clearUsage(); q.removeKeyword('x'); return q.usedTodayMin === 0 && q.keywords.length === 0 && q.blockedHits === 0; } },
    { id: 'X06536', name: '家庭·动效令牌', check: () => T.FAMILY_MODE_MATRIX.toddler.dailyLimitMin < T.FAMILY_MODE_MATRIX.child.dailyLimitMin && T.FAMILY_MODE_MATRIX.child.dailyLimitMin < T.FAMILY_MODE_MATRIX.teen.dailyLimitMin && T.FAMILY_MODE_MATRIX.teen.dailyLimitMin < T.FAMILY_MODE_MATRIX.family.dailyLimitMin && T.FAMILY_MODE_MATRIX.family.dailyLimitMin < T.FAMILY_MODE_MATRIX.off.dailyLimitMin },
    { id: 'X06537', name: '家庭·三态焦点', check: () => { const q = new T.FamilyMode(); const unset = q.verifyPin('1111'); q.setPin('1111'); q.verifyPin('2222'); q.verifyPin('2222'); q.verifyPin('2222'); const locked = q.locked; const ok = q.verifyPin('1111'); return unset === false && locked === true && ok === true && q.locked === false && q.failedAttempts === 0; } },
    { id: 'X06538', name: '家庭·键盘序', check: () => T.FAMILY_MODE_LEVELS.every((p) => new T.FamilyMode(p).level === p) },
    { id: 'X06539', name: '家庭·微文案', check: () => { const q = new T.FamilyMode(); q.recordUsage(9, 20); q.recordUsage(15, 30); const r = q.report(); return r.totalMin === 50 && r.topHour === 15 && r.withinLimit === true; } },
    { id: 'X06540', name: '家庭·aria 等价', check: () => { const q = new T.FamilyMode(); q.addKeyword('a'); q.screen('xa'); q.screen('aa'); const r = q.report(); return r.blockedHits === 2 && q.blockedHits === 2; } },
    { id: 'X06541', name: '家庭·基准采集', check: () => { const q = new T.FamilyMode(); const t0 = performance.now(); for (let i = 0; i < 500; i++) q.screen('普通内容文本'); return performance.now() - t0 < 50; } },
    { id: 'X06542', name: '家庭·热路径', check: () => { const q = new T.FamilyMode('off'); return q.screen('任何内容').ok === true && q.screen('任何内容').hits.length === 0; } },
    { id: 'X06543', name: '家庭·零漂移', check: () => { const q = new T.FamilyMode('teen'); q.setPin('4321'); q.addKeyword('k1'); q.recordUsage(8, 15); const a = T.FamilyMode.deserialize(q.serialize()); const b = T.FamilyMode.deserialize(a.serialize()); return b.serialize() === a.serialize() && b.serialize() === q.serialize(); } },
    { id: 'X06544', name: '家庭·低配减档', check: () => { const q = new T.FamilyMode('off'); return q.profile.filterOn === false && q.profile.dailyLimitMin === 480 && q.isHourAllowed(23) === true; } },
    { id: 'X06545', name: '家庭·守卫', check: () => { const q = new T.FamilyMode('toddler'); q.recordUsage(9, 25); q.recordUsage(10, 10); return q.withinDailyLimit() === false && q.report().withinLimit === false; } },
    { id: 'X06546', name: '家庭·智能建议', check: () => { const q = new T.FamilyMode('child'); q.recordUsage(9, 40); return q.remainingMin() === 20 && q.remainingMin() === Math.max(0, 60 - 40); } },
    { id: 'X06547', name: '家庭·批量模式', check: () => { const q = new T.FamilyMode(); let n = 0; for (let h = 0; h < 24; h++) { if (q.recordUsage(h, 1) === 1) n += 1; } return n === 24 && q.usedTodayMin === 24 && q.report().topHour === 0; } },
    { id: 'X06548', name: '家庭·跨域联动', check: () => { const q = new T.FamilyMode('teen'); q.setPin('9999'); const allowed = q.isHourAllowed(15); q.recordUsage(15, 30); const r = T.FamilyMode.deserialize(q.serialize()); return allowed === true && r.verifyPin('9999') === true && r.usedTodayMin === 30; } },
    { id: 'X06549', name: '家庭·扩展点', check: () => { const q = new T.FamilyMode(); return typeof T.simplePinHash === 'function' && typeof T.FamilyMode.deserialize === 'function' && typeof q.removeKeyword === 'function' && T.FAMILY_PREFS_VERSION === 1; } },
    { id: 'X06550', name: '家庭·彩蛋层', check: () => { const q = new T.FamilyMode(); q.addKeyword('normal'); const before = q.eggs.length; q.addKeyword(T.FAMILY_EGG_KEYWORD); return before === 0 && q.eggs[0] === T.FAMILY_EGG_NAME && q.eggs.length === 1; } },
  ];
}

/* -------- 族0264 工作流编排器 2.0 X06576~X06600 -------- */
export function checkF0264(): CheckEntry[] {
  return [
    { id: 'X06576', name: '工作流·最小闭环', check: () => { const q = new T.WorkflowEngine(); q.addNode('a', 'trigger'); q.addNode('b', 'action'); q.addEdge('a', 'b'); const r = q.run('a'); return r.ran[0] === 'a' && r.ran[1] === 'b' && r.failed === null && r.steps === 2; } },
    { id: 'X06577', name: '工作流·全量参数', check: () => { const q = new T.WorkflowEngine(); q.addNode('n', 'action', 2); const n = q.nodes.get('n'); return !!n && n.kind === 'action' && n.retry === 2 && n.next.length === 0; } },
    { id: 'X06578', name: '工作流·档位矩阵', check: () => T.WORKFLOW_NODE_KINDS.length === 3 && T.WORKFLOW_MAX_RETRY === 3 && T.WORKFLOW_MAX_NODES === 64 && T.WORKFLOW_MAX_STEPS === 128 },
    { id: 'X06579', name: '工作流·快照迁移', check: () => { const q = new T.WorkflowEngine(); q.addNode('t', 'trigger'); q.addNode('a', 'action', 1); q.addEdge('t', 'a'); q.addTrigger('manual'); const r = T.WorkflowEngine.deserialize(q.serialize()); return r.nodes.size === 2 && r.nodes.get('a')?.retry === 1 && r.triggers[0] === 'manual' && r.serialize() === q.serialize(); } },
    { id: 'X06580', name: '工作流·联调集成', check: () => { const q = new T.WorkflowEngine(); q.addNode('t', 'trigger'); q.addTrigger('manual'); q.enqueue('t'); const ran = q.step(true); return ran === 't' && q.logSize === 1 && q.queueSize() === 0 && q.log[0] === 't:ok'; } },
    { id: 'X06581', name: '工作流·越界钳制', check: () => { const q = new T.WorkflowEngine(); const bad = q.addNode('x', 'quantum'); q.addNode('r', 'action', 99); return bad === false && q.clamped === 1 && q.nodes.get('r')?.retry === 3 && q.nodes.size === 1; } },
    { id: 'X06582', name: '工作流·失败叙事', check: () => { const q = new T.WorkflowEngine(); const r0 = q.run('ghost'); q.addNode('a', 'action', 1); const r = q.run('a', () => true); return r0.steps === 0 && r.failed === 'a' && r.ran.length === 0 && r.steps === 2 && q.log.includes('a:fail'); } },
    { id: 'X06583', name: '工作流·中断还原', check: () => { const r = T.WorkflowEngine.deserialize('{bad'); return r.nodes.size === 0 && r.triggers.length === 0 && T.WorkflowEngine.deserialize('not-json').queue.length === 0; } },
    { id: 'X06584', name: '工作流·资源降级', check: () => { const q = new T.WorkflowEngine(); let n = 0; for (let i = 0; i < 70; i++) { if (q.addNode(`n${i}`, 'action')) n += 1; } return n === T.WORKFLOW_MAX_NODES && q.nodes.size === T.WORKFLOW_MAX_NODES; } },
    { id: 'X06585', name: '工作流·净身', check: () => { const q = new T.WorkflowEngine(); q.addNode('a', 'trigger'); q.enqueue('a'); q.step(true); q.clear(); return q.nodes.size === 0 && q.triggers.length === 0 && q.queue.length === 0 && q.log.length === 0 && q.clamped === 0; } },
    { id: 'X06586', name: '工作流·动效令牌', check: () => { const q = new T.WorkflowEngine(); q.addNode('c', 'action'); q.addNode('a', 'trigger'); q.addNode('b', 'action'); q.addEdge('a', 'b'); q.addEdge('b', 'c'); const topo = q.topoOrder(); return topo[0] === 'a' && topo[2] === 'c' && topo.length === 3; } },
    { id: 'X06587', name: '工作流·三态焦点', check: () => { const q = new T.WorkflowEngine(); q.addNode('c', 'condition'); q.addNode('yes', 'action'); q.addNode('no', 'action'); q.addEdge('c', 'yes'); q.addEdge('c', 'no'); return q.branch('c', true) === 'yes' && q.branch('c', false) === 'no' && q.branch('c', true) !== q.branch('c', false); } },
    { id: 'X06588', name: '工作流·键盘序', check: () => T.WORKFLOW_NODE_KINDS.every((k) => { const q = new T.WorkflowEngine(); return q.addNode(`n-${k}`, k); }) },
    { id: 'X06589', name: '工作流·微文案', check: () => { const q = new T.WorkflowEngine(); q.addNode('a', 'action'); q.run('a'); return q.log[0] === 'a:ok'; } },
    { id: 'X06590', name: '工作流·aria 等价', check: () => { const q = new T.WorkflowEngine(); q.addNode('c', 'condition'); q.addNode('y', 'action'); q.addEdge('c', 'y'); return q.branch('c', true) === 'y' && q.branch('c', false) === null; } },
    { id: 'X06591', name: '工作流·基准采集', check: () => { const q = new T.WorkflowEngine(); for (let i = 0; i < 60; i++) q.addNode(`n${i}`, 'action'); const t0 = performance.now(); for (let i = 0; i < 50; i++) q.run(`n${i}`); return performance.now() - t0 < 50; } },
    { id: 'X06592', name: '工作流·热路径', check: () => new T.WorkflowEngine().run('x').steps === 0 && new T.WorkflowEngine().step(true) === null },
    { id: 'X06593', name: '工作流·零漂移', check: () => { const q = new T.WorkflowEngine(); q.addNode('t', 'trigger'); q.addNode('a', 'action', 3); q.addEdge('t', 'a'); q.addTrigger('event:deploy'); const a = T.WorkflowEngine.deserialize(q.serialize()); const b = T.WorkflowEngine.deserialize(a.serialize()); return b.serialize() === a.serialize() && b.serialize() === q.serialize(); } },
    { id: 'X06594', name: '工作流·低配减档', check: () => { const q = new T.WorkflowEngine(); q.addNode('a', 'action', 0); const r = q.run('a', () => true); return r.failed === 'a' && r.steps === 1; } },
    { id: 'X06595', name: '工作流·守卫', check: () => { const q = new T.WorkflowEngine(); q.addNode('a', 'action'); q.addNode('b', 'action'); const e1 = q.addEdge('a', 'b'); const e2 = q.addEdge('b', 'a'); const self = q.addEdge('a', 'a'); return e1 === true && e2 === false && self === false && q.hasCycle() === false; } },
    { id: 'X06596', name: '工作流·智能建议', check: () => { const q = new T.WorkflowEngine(); return q.suggestRetry('action') === 2 && q.suggestRetry('trigger') === 0 && q.suggestRetry('condition') === 0; } },
    { id: 'X06597', name: '工作流·批量模式', check: () => { const q = new T.WorkflowEngine(); for (let i = 0; i < 20; i++) q.addNode(`n${i}`, 'action'); let n = 0; for (let i = 0; i < 20; i++) { if (q.enqueue(`n${i}`)) n += 1; } let ran = 0; while (q.queue.length > 0) { if (q.step(true) !== null) ran += 1; } return n === 20 && ran === 20 && q.logSize === 20; } },
    { id: 'X06598', name: '工作流·跨域联动', check: () => { const q = new T.WorkflowEngine(); q.addNode('t', 'trigger'); q.addNode('c', 'condition'); q.addNode('y', 'action'); q.addEdge('t', 'c'); q.addEdge('c', 'y'); const r = q.run('t'); return r.ran.length === 3 && q.topoOrder()[0] === 't' && q.hasCycle() === false; } },
    { id: 'X06599', name: '工作流·扩展点', check: () => { const q = new T.WorkflowEngine(); return typeof T.WorkflowEngine.deserialize === 'function' && typeof q.topoOrder === 'function' && typeof q.branch === 'function' && T.WORKFLOW_PREFS_VERSION === 1; } },
    { id: 'X06600', name: '工作流·彩蛋层', check: () => { const q = new T.WorkflowEngine(); q.addTrigger('manual'); const before = q.eggs.length; q.addTrigger(T.WORKFLOW_EGG_TRIGGER); return before === 0 && q.eggs[0] === T.WORKFLOW_EGG_NAME && q.triggers.length === 2; } },
  ];
}

/* -------- 族0265 系统自动化 2.0 X06601~X06625 -------- */
export function checkF0265(): CheckEntry[] {
  return [
    { id: 'X06601', name: '自动化·最小闭环', check: () => { const q = new T.AutomationEngine(); q.addRule('r1', 'boot', 'notify'); const fired = q.dispatch('boot'); return fired.length === 1 && fired[0] === 'r1' && q.rules.get('r1')?.runs === 1; } },
    { id: 'X06602', name: '自动化·全量参数', check: () => { const q = new T.AutomationEngine(); q.addRule('r', 't', 'a', 'critical', false); const r = q.rules.get('r'); return !!r && r.priority === 'critical' && r.enabled === false && r.runs === 0; } },
    { id: 'X06603', name: '自动化·档位矩阵', check: () => T.AUTOMATION_PRIORITY_LEVELS.length === 5 && T.DEFAULT_AUTOMATION_PRIORITY === 'normal' && T.AUTOMATION_PRIORITY_MATRIX.critical.weight === 5 && T.AUTOMATION_PRIORITY_MATRIX.idle.weight === 1 },
    { id: 'X06604', name: '自动化·快照迁移', check: () => { const q = new T.AutomationEngine(); q.addRule('r1', 'boot', 'notify', 'high'); q.addRule('r2', 'boot', 'log', 'low'); q.dispatch('boot'); const d = T.AutomationEngine.deserialize(q.serialize()); return d.rules.size === 2 && (d.rules.get('r1')?.runs ?? 0) === 1 && d.serialize() === q.serialize(); } },
    { id: 'X06605', name: '自动化·联调集成', check: () => { const q = new T.AutomationEngine(); q.addRule('low', 'evt', 'x', 'low'); q.addRule('crit', 'evt', 'y', 'critical'); const fired = q.dispatch('evt'); return fired[0] === 'crit' && fired[1] === 'low'; } },
    { id: 'X06606', name: '自动化·越界钳制', check: () => { const q = new T.AutomationEngine(); q.addRule('r', 't', 'a', 'ultra'); return q.clamped === 1 && q.rules.get('r')?.priority === 'normal'; } },
    { id: 'X06607', name: '自动化·失败叙事', check: () => { const q = new T.AutomationEngine(); q.addRule('r', 't', 'a'); q.setEnabled('r', false); const fired = q.dispatch('t'); return fired.length === 0 && q.logSize === 0 && q.dispatch('nothing').length === 0; } },
    { id: 'X06608', name: '自动化·中断还原', check: () => { const d = T.AutomationEngine.deserialize('not-json'); return d.rules.size === 0 && d.globalEnabled === true && T.AutomationEngine.deserialize('{}').logSize === 0; } },
    { id: 'X06609', name: '自动化·资源降级', check: () => { const q = new T.AutomationEngine(); let n = 0; for (let i = 0; i < 70; i++) { if (q.addRule(`r${i}`, 'evt', 'act')) n += 1; } return n === T.AUTOMATION_MAX_RULES && q.rules.size === T.AUTOMATION_MAX_RULES; } },
    { id: 'X06610', name: '自动化·净身', check: () => { const q = new T.AutomationEngine(); q.addRule('r', 't', 'a'); q.dispatch('t'); q.setGlobal(false); q.reset(); return q.rules.size === 0 && q.logSize === 0 && q.globalEnabled === true && q.loopGuardHits === 0 && q.clamped === 0; } },
    { id: 'X06611', name: '自动化·动效令牌', check: () => { const w = T.AUTOMATION_PRIORITY_LEVELS.map((p) => T.AUTOMATION_PRIORITY_MATRIX[p].weight); return w[0] === 5 && w[4] === 1 && w.every((x, i) => i === 0 || (w[i - 1] ?? 0) > x); } },
    { id: 'X06612', name: '自动化·三态焦点', check: () => { const q = new T.AutomationEngine(); q.addRule('r', 't', 'a'); q.setGlobal(false); const off = q.dispatch('t').length; q.setGlobal(true); const on = q.dispatch('t').length; return off === 0 && on === 1 && q.globalEnabled === true; } },
    { id: 'X06613', name: '自动化·键盘序', check: () => T.AUTOMATION_PRIORITY_LEVELS.every((p) => { const q = new T.AutomationEngine(); q.addRule('r', 't', 'a', p); return q.rules.get('r')?.priority === p; }) },
    { id: 'X06614', name: '自动化·微文案', check: () => { const q = new T.AutomationEngine(); q.addRule('r1', 'boot', 'toast'); q.dispatch('boot'); return q.log[0] === 'r1→toast'; } },
    { id: 'X06615', name: '自动化·aria 等价', check: () => { const q = new T.AutomationEngine(); q.addRule('on', 'e1', 'a'); q.addRule('off', 'e2', 'b', 'normal', false); q.dispatch('e1'); const s = q.stats(); return s.total === 2 && s.enabled === 1 && s.runs === 1; } },
    { id: 'X06616', name: '自动化·基准采集', check: () => { const q = new T.AutomationEngine(); for (let i = 0; i < 60; i++) q.addRule(`r${i}`, 'evt', 'act'); const t0 = performance.now(); for (let i = 0; i < 100; i++) q.dispatch('evt'); return performance.now() - t0 < 50; } },
    { id: 'X06617', name: '自动化·热路径', check: () => new T.AutomationEngine().dispatch('evt').length === 0 && new T.AutomationEngine().stats().total === 0 },
    { id: 'X06618', name: '自动化·零漂移', check: () => { const q = new T.AutomationEngine(); q.addRule('r1', 'e', 'act', 'high'); q.dispatch('e'); const a = T.AutomationEngine.deserialize(q.serialize()); const b = T.AutomationEngine.deserialize(a.serialize()); return b.serialize() === a.serialize() && b.serialize() === q.serialize(); } },
    { id: 'X06619', name: '自动化·低配减档', check: () => { const q = new T.AutomationEngine(); q.addRule('r', 'e', 'a', 'idle'); return T.AUTOMATION_PRIORITY_MATRIX.idle.maxRunsPerMin === 5 && T.AUTOMATION_PRIORITY_MATRIX.idle.weight === 1 && q.rules.get('r')?.priority === 'idle'; } },
    { id: 'X06620', name: '自动化·守卫', check: () => { const q = new T.AutomationEngine(); q.addRule('self', 'e', 'e'); const f1 = q.dispatch('e'); q.clearLog(); const guard1 = q.loopGuardHits; const q2 = new T.AutomationEngine(); q2.addRule('a', 'e1', 'e2'); q2.addRule('b', 'e2', 'e1'); q2.dispatch('e1'); return f1.length === 1 && guard1 === 1 && q2.loopGuardHits === 1 && q2.logSize === 2; } },
    { id: 'X06621', name: '自动化·智能建议', check: () => { const q = new T.AutomationEngine(); return q.suggestPriority('security:login') === 'critical' && q.suggestPriority('notify:done') === 'low' && q.suggestPriority('other') === 'normal'; } },
    { id: 'X06622', name: '自动化·批量模式', check: () => { const q = new T.AutomationEngine(); let n = 0; for (let i = 0; i < 10; i++) { if (q.addRule(`r${i}`, 'evt', `act${i}`)) n += 1; } const fired = q.dispatch('evt'); return n === 10 && fired.length === 10 && q.stats().runs === 10; } },
    { id: 'X06623', name: '自动化·跨域联动', check: () => { const q = new T.AutomationEngine(); q.addRule('a', 'boot', 'ready'); q.addRule('b', 'ready', 'done'); const fired = q.dispatch('boot'); return fired.length === 2 && fired[0] === 'a' && fired[1] === 'b' && q.loopGuardHits === 0; } },
    { id: 'X06624', name: '自动化·扩展点', check: () => { const q = new T.AutomationEngine(); return typeof T.AutomationEngine.deserialize === 'function' && typeof q.dispatch === 'function' && typeof q.suggestPriority === 'function' && T.AUTOMATION_PREFS_VERSION === 1; } },
    { id: 'X06625', name: '自动化·彩蛋层', check: () => { const q = new T.AutomationEngine(); q.addRule('plain', 'e1', 'log'); const before = q.eggs.length; q.addRule('party', 'e2', T.AUTOMATION_EGG_ACTION); return before === 0 && q.eggs[0] === T.AUTOMATION_EGG_NAME && q.eggs.length === 1; } },
  ];
}

/* -------- 族0266 快捷指令库 2.0 X06626~X06650 -------- */
export function checkF0266(): CheckEntry[] {
  return [
    { id: 'X06626', name: '指令·最小闭环', check: () => { const q = new T.ShortcutLib(); q.create('c1', '问候', ['你好，{{name}}']); const out = q.run('c1', { name: '世界' }); return out?.length === 1 && out[0] === '你好，世界'; } },
    { id: 'X06627', name: '指令·全量参数', check: () => { const q = new T.ShortcutLib(); q.create('c1', '全量', ['{{a}}-{{b}}-{{a}}'], 'dev', true); const sc = q.items.get('c1'); return sc?.category === 'dev' && sc.favorite === true && q.run('c1', { a: 'x', b: 'y' })?.[0] === 'x-y-x'; } },
    { id: 'X06628', name: '指令·档位矩阵', check: () => T.SHORTCUT_CATEGORIES.length === 5 && T.DEFAULT_SHORTCUT_CATEGORY === 'general' && T.SHORTCUT_MAX_STEPS === 32 && T.SHORTCUT_HISTORY_CAP === 32 },
    { id: 'X06629', name: '指令·快照迁移', check: () => { const q = new T.ShortcutLib(); q.create('c1', '迁移', ['s1', 's2'], 'fun', true); const data = q.exportById('c1') ?? ''; const r = T.importShortcut(data); return r.id === 'c1' && r.category === 'fun' && r.steps.length === 2 && T.exportShortcut(r) === data; } },
    { id: 'X06630', name: '指令·联调集成', check: () => { const q = new T.ShortcutLib(); q.create('c1', '集成', ['打开 {{app}}']); q.toggleFavorite('c1'); const out = q.run('c1', { app: '日历' }); return out?.[0] === '打开 日历' && q.favorites()[0] === 'c1' && q.lastRun()?.id === 'c1'; } },
    { id: 'X06631', name: '指令·越界钳制', check: () => { const q = new T.ShortcutLib(); q.create('c1', 'x', [], 'nonsense'); return q.clamped === 1 && q.items.get('c1')?.category === 'general'; } },
    { id: 'X06632', name: '指令·失败叙事', check: () => { const q = new T.ShortcutLib(); return q.run('ghost') === null && q.toggleFavorite('ghost') === null && T.importShortcut('{bad').id === 'imported'; } },
    { id: 'X06633', name: '指令·中断还原', check: () => { const q = new T.ShortcutLib(); q.create('c1', '中断', ['a']); q.run('c1'); const q2 = new T.ShortcutLib(); q2.import(q.exportById('c1') ?? ''); return q2.items.get('c1')?.name === '中断' && q2.items.get('c1')?.steps[0] === 'a'; } },
    { id: 'X06634', name: '指令·资源降级', check: () => { const q = new T.ShortcutLib(); const many = Array.from({ length: 40 }, (_, i) => `s${i}`); q.create('c1', '降级', many); return q.items.get('c1')?.steps.length === T.SHORTCUT_MAX_STEPS && q.items.get('c1')?.steps[0] === 's0'; } },
    { id: 'X06635', name: '指令·净身', check: () => { const q = new T.ShortcutLib(); q.create('c1', 'x', ['a']); q.run('c1'); q.reset(); return q.items.size === 0 && q.historySize === 0 && q.seq === 0 && q.clamped === 0; } },
    { id: 'X06636', name: '指令·动效令牌', check: () => T.SHORTCUT_CATEGORIES[0] === 'general' && T.SHORTCUT_CATEGORIES[4] === 'fun' && new Set(T.SHORTCUT_CATEGORIES).size === 5 },
    { id: 'X06637', name: '指令·三态焦点', check: () => { const q = new T.ShortcutLib(); q.create('c1', 'x', []); const a = q.toggleFavorite('c1'); const b = q.toggleFavorite('c1'); return a === true && b === false && q.favorites().length === 0; } },
    { id: 'X06638', name: '指令·键盘序', check: () => T.SHORTCUT_CATEGORIES.every((c) => { const q = new T.ShortcutLib(); q.create('c1', 'x', [], c); return q.items.get('c1')?.category === c; }) },
    { id: 'X06639', name: '指令·微文案', check: () => { const q = new T.ShortcutLib(); q.create('c1', '问候', ['{{hour}} 点提醒：喝水']); return q.run('c1', { hour: '9' })?.[0] === '9 点提醒：喝水'; } },
    { id: 'X06640', name: '指令·aria 等价', check: () => T.renderStep('{{a}} {{b}}', { a: 'x' }) === 'x {{b}}' && T.renderStep('无参数', {}) === '无参数' },
    { id: 'X06641', name: '指令·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.renderStep('渲染 {{p}} 次', { p: String(i) }); return performance.now() - t0 < 50; } },
    { id: 'X06642', name: '指令·热路径', check: () => new T.ShortcutLib().run('x') === null && T.renderStep('a', {}) === 'a' },
    { id: 'X06643', name: '指令·零漂移', check: () => { const q = new T.ShortcutLib(); q.create('c1', '稳定', ['s1'], 'dev', true); const data = q.exportById('c1') ?? ''; const a = T.importShortcut(data); const b = T.importShortcut(T.exportShortcut(a)); return T.exportShortcut(b) === T.exportShortcut(a) && T.exportShortcut(a) === data; } },
    { id: 'X06644', name: '指令·低配减档', check: () => { const q = new T.ShortcutLib(); q.create('c1', '空', []); const out = q.run('c1'); return out?.length === 0 && q.historySize === 1; } },
    { id: 'X06645', name: '指令·守卫', check: () => { const q = new T.ShortcutLib(); const first = q.create('c1', 'a', []); const dup = q.create('c1', 'b', []); return first === true && dup === false && q.items.size === 1 && q.items.get('c1')?.name === 'a'; } },
    { id: 'X06646', name: '指令·智能建议', check: () => T.suggestCategory(['打开 设置']) === 'system' && T.suggestCategory(['git 提交代码']) === 'dev' && T.suggestCategory(['喝茶']) === 'general' },
    { id: 'X06647', name: '指令·批量模式', check: () => { const q = new T.ShortcutLib(); let n = 0; for (let i = 0; i < 10; i++) { if (q.create(`c${i}`, `n${i}`, ['run'])) n += 1; } let m = 0; for (let i = 0; i < 10; i++) { if (q.run(`c${i}`)) m += 1; } return n === 10 && m === 10 && q.historySize === 10 && q.lastRun()?.seq === 10; } },
    { id: 'X06648', name: '指令·跨域联动', check: () => { const q = new T.ShortcutLib(); q.create('a', 'x', [], 'dev'); q.create('b', 'y', [], 'dev'); q.create('c', 'z', [], 'fun'); q.toggleFavorite('a'); return q.listByCategory('dev').length === 2 && q.listByCategory('fun')[0] === 'c' && q.favorites()[0] === 'a'; } },
    { id: 'X06649', name: '指令·扩展点', check: () => typeof T.renderStep === 'function' && typeof T.importShortcut === 'function' && typeof T.exportShortcut === 'function' && typeof T.suggestCategory === 'function' && T.SHORTCUT_PREFS_VERSION === 1 },
    { id: 'X06650', name: '指令·彩蛋层', check: () => { const q = new T.ShortcutLib(); q.create('normal', 'x', ['a']); q.run('normal'); const before = q.eggs.length; q.create(T.SHORTCUT_EGG_ID, 'y', ['b']); q.run(T.SHORTCUT_EGG_ID); return before === 0 && q.eggs[0] === T.SHORTCUT_EGG_NAME && q.eggs.length === 1; } },
  ];
}

// UNREAL-X AI-27（族0261~0270 · X06501~X06750）V 线聚合：五族 × 25 项 = 125 项，只增不删。
export function runAi27VChecks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [checkF0261, checkF0262, checkF0264, checkF0265, checkF0266];
  const entries = families.flatMap((f) => f());
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
