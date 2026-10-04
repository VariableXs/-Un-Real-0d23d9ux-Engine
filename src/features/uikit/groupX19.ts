// UNREAL-X AI-19：族0181~0190「内核输入栈」断言组（X04501~X04750 代表性断言），勿删。
// 每族 ≥5 条可运行断言：覆盖功能逻辑 / 边界钳制 / 集成口径三类。
// 全部走 src/features/inputFeel/x2ink.ts 的真实实现（与 kernel/varix/src/inkstack.rs 同口径）。

import * as K from '../inputFeel/x2ink';

export interface CheckEntry {
  id: string;
  name: string;
  check: () => boolean;
}

const base = [
  { name: 'ps2', caps: K.DRV_KBD, probeOrder: 1, present: true },
  { name: 'hid', caps: K.DRV_KBD | K.DRV_PTR, probeOrder: 0, present: true },
  { name: 'touch', caps: K.DRV_TOUCH | K.DRV_PTR, probeOrder: 2, present: true },
];

/* -------- 族0181 驱动抽象 -------- */
export function checkX0181(): CheckEntry[] {
  return [
    { id: 'X04501', name: '驱动探测按序', check: () => K.probe(base).join() === 'hid,ps2,touch' },
    { id: 'X04502', name: '能力位复合判定', check: () => K.hasCaps(base[2]!, K.DRV_TOUCH | K.DRV_PTR) && !K.hasCaps(base[0]!, K.DRV_PTR) },
    { id: 'X04503', name: '降级按探测序', check: () => K.fallbackPick(base, K.DRV_PTR) === 'hid' && K.fallbackPick(base, K.DRV_CKEY) === null },
    { id: 'X04504', name: '空表安全', check: () => K.probe([]).length === 0 && K.fallbackPick([], K.DRV_KBD) === null },
    { id: 'X04505', name: '不在位不探测', check: () => K.probe([{ name: 'a', caps: 1, probeOrder: 0, present: false }]).length === 0 },
  ];
}

/* -------- 族0182 事件管线 -------- */
export function checkX0182(): CheckEntry[] {
  const r = new K.EvRing();
  for (let k = 0; k < 16; k++) r.push({ kind: 1, code: k, tMs: k });
  const fullRejected = !r.push({ kind: 1, code: 99, tMs: 99 });
  const merged = new K.EvRing();
  merged.push({ kind: 3, code: 0, tMs: 1 });
  merged.push({ kind: 3, code: 0, tMs: 2 });
  return [
    { id: 'X04526', name: '入队即出队', check: () => { const q = new K.EvRing(); q.push({ kind: 1, code: 30, tMs: 10 }); return q.pop()?.code === 30 && q.pop() === null; } },
    { id: 'X04527', name: '容量 16 满队拒收', check: () => fullRejected && r.dropped === 1 },
    { id: 'X04528', name: 'move 事件合并', check: () => merged.length === 1 },
    { id: 'X04529', name: 'down 不合并', check: () => { const q = new K.EvRing(); q.push({ kind: 1, code: 5, tMs: 1 }); q.push({ kind: 1, code: 5, tMs: 2 }); return q.length === 2; } },
    { id: 'X04530', name: '回绕后顺序不乱', check: () => { const q = new K.EvRing(); for (let k = 0; k < 16; k++) q.push({ kind: 1, code: k, tMs: k }); q.pop(); q.push({ kind: 1, code: 16, tMs: 16 }); return q.pop()?.code === 1 && q.pop()?.code === 2; } },
  ];
}

/* -------- 族0183 重复率去抖 -------- */
export function checkX0183(): CheckEntry[] {
  const c = K.REPEAT_TIERS[K.DEFAULT_REPEAT] ?? { delayMs: 300, periodMs: 40 };
  return [
    { id: 'X04551', name: '五档重复率目录', check: () => K.REPEAT_TIERS.length === 5 && K.DEFAULT_REPEAT === 2 },
    { id: 'X04552', name: '延迟前不重复', check: () => K.repeatCount(c, 100) === 0 && K.repeatCount(c, c.delayMs) === 1 },
    { id: 'X04553', name: '重复次数公式', check: () => K.repeatCount({ delayMs: 100, periodMs: 50 }, 310) === 5 },
    { id: 'X04554', name: '消抖窗口', check: () => K.debounced(null, 10, 50) === false && K.debounced(100, 120, 50) === true && K.debounced(100, 200, 50) === false },
    { id: 'X04555', name: '非法配置钳制', check: () => K.clampRepeatCfg(0, 0).delayMs === 50 && K.clampRepeatCfg(9999, 9999).periodMs === 250 },
  ];
}

/* -------- 族0184 组合键引擎 -------- */
export function checkX0184(): CheckEntry[] {
  const a: K.Chord = { mods: K.MOD_CTRL | K.MOD_SHIFT, key: 80 };
  return [
    { id: 'X04576', name: '组合键全等命中', check: () => K.chordMatch(a, { mods: 3, key: 80 }) && !K.chordMatch(a, { mods: 1, key: 80 }) },
    { id: 'X04577', name: '标签固定序', check: () => K.chordLabel(a) === 'Ctrl+Shift+80' && K.chordLabel({ mods: 0, key: 42 }) === '42' },
    { id: 'X04578', name: '冲突检测', check: () => K.chordConflict([a], a) && !K.chordConflict([a], { mods: 0, key: 1 }) && !K.chordConflict([], a) },
    { id: 'X04579', name: '序列进度机', check: () => K.seqProgress(K.seqProgress(K.seqProgress(0, true), true), true) === 3 && K.seqProgress(2, false) === 0 },
    { id: 'X04580', name: '16 修饰组合全可命中', check: () => Array.from({ length: 16 }, (_, m) => K.chordMatch({ mods: m, key: 1 }, { mods: m, key: 1 })).every(Boolean) },
  ];
}

/* -------- 族0185 输入法框架 -------- */
export function checkX0185(): CheckEntry[] {
  const pool = ['ni', 'nihao', 'niubi', 'wo', 'women', 'zai', 'zaijian'];
  const im = new K.Ime();
  im.typeKey('n'); im.typeKey('i'); im.candidates(pool);
  const candOk = im.state === K.IME_CAND && im.cands.length === 3;
  im.nav(100); im.nav(-100);
  const word = im.commit();
  const empty = new K.Ime();
  return [
    { id: 'X04601', name: '组合→候选→上屏', check: () => candOk && word === 'ni' },
    { id: 'X04602', name: '候选窗口 9 上限', check: () => { const q = new K.Ime(); q.typeKey('a'); q.candidates(['a1', 'a2', 'a3', 'a4', 'a5', 'a6', 'a7', 'a8', 'a9', 'a10']); return q.cands.length === 9; } },
    { id: 'X04603', name: '导航钳制', check: () => { const q = new K.Ime(); q.typeKey('w'); q.candidates(pool); q.nav(100); return q.sel === 1; } },
    { id: 'X04604', name: '无候选回组合态', check: () => { const q = new K.Ime(); q.typeKey('x'); q.candidates(pool); return q.cands.length === 0 && q.state === K.IME_COMPOSING; } },
    { id: 'X04605', name: 'Esc 回空闲', check: () => { const q = new K.Ime(); q.typeKey('x'); q.esc(); return q.state === K.IME_IDLE && q.buf === ''; } },
    { id: 'X04606', name: '非候选不上屏', check: () => empty.commit() === null },
  ];
}

/* -------- 族0186 无线延迟 -------- */
export function checkX0186(): CheckEntry[] {
  const b: K.LinkBudget = { pollMs: 8, retryMs: 1, jitterMs: 4 };
  return [
    { id: 'X04626', name: '预算三段加和', check: () => K.linkTotal(b) === 13 && K.feelsWired(b) },
    { id: 'X04627', name: '超预算判否', check: () => !K.feelsWired({ pollMs: 20, retryMs: 2, jitterMs: 5 }) && K.feelsWired({ pollMs: 8, retryMs: 3, jitterMs: 5 }) },
    { id: 'X04628', name: '抖动三档', check: () => K.jitterGrade(2) === 'good' && K.jitterGrade(5) === 'fair' && K.jitterGrade(6) === 'poor' },
    { id: 'X04629', name: 'P95 采样', check: () => K.p95([7]) === 7 && K.p95([]) === 0 && K.p95([9, 1, 5]) === 9 },
    { id: 'X04630', name: 'P95 单调不降', check: () => K.p95([1, 2, 3, 4, 5, 6, 7, 8, 9, 10]) <= K.p95([1, 2, 3, 4, 5, 6, 7, 8, 9, 11]) },
  ];
}

/* -------- 族0187 缓冲回放 -------- */
export function checkX0187(): CheckEntry[] {
  const l = new K.ReplayLog(32);
  l.record(2, 20); l.record(1, 10);
  const full = new K.ReplayLog(1);
  full.record(1, 1);
  return [
    { id: 'X04651', name: '时序回放', check: () => l.replay().join() === '1,2' },
    { id: 'X04652', name: '容量钳制', check: () => new K.ReplayLog(0).cap === 1 && new K.ReplayLog(64).cap === 32 },
    { id: 'X04653', name: '满录拒绝', check: () => !full.record(2, 2) && full.length === 1 },
    { id: 'X04654', name: '校验链确定性', check: () => l.chainHash() === l.chainHash() && l.chainHash() !== 0 },
    { id: 'X04655', name: '截断净身', check: () => { const q = new K.ReplayLog(8); for (let k = 0; k < 5; k++) q.record(k, k); q.truncate(2); return q.length === 2; } },
  ];
}

/* -------- 族0188 输入功耗 -------- */
export function checkX0188(): CheckEntry[] {
  return [
    { id: 'X04676', name: '五档预算递降', check: () => K.INPUT_POWER_TIERS.length === 5 && K.INPUT_POWER_TIERS.every((t, i) => i === 0 || t < K.INPUT_POWER_TIERS[i - 1]!) },
    { id: 'X04677', name: '空闲降档边界', check: () => K.idleTier(999) === 0 && K.idleTier(5000) === 2 && K.idleTier(60000) === 4 },
    { id: 'X04678', name: '唤醒惩罚线性', check: () => K.wakePenalty(0) === 0 && K.wakePenalty(2) === 10 && K.wakePenalty(99) === 20 },
    { id: 'X04679', name: '低电强省', check: () => K.powerGuard(10, 0) === 4 && K.powerGuard(20, 1) === 1 && K.powerGuard(80, 99) === 4 },
    { id: 'X04680', name: '按键能耗公式', check: () => K.keyEnergy(0, 100) === 80000 && K.keyEnergy(4, 100) < K.keyEnergy(0, 100) },
  ];
}

/* -------- 族0189 输入基准 -------- */
export function checkX0189(): CheckEntry[] {
  return [
    { id: 'X04701', name: '五场景基准目录', check: () => K.BENCH_SUITE.length === 5 && K.benchTotal(K.BENCH_SUITE[0]!) === 14 },
    { id: 'X04702', name: '基准评分公式', check: () => K.benchScore(K.BENCH_SUITE[0]!, 14) === 1000 && K.benchScore(K.BENCH_SUITE[3]!, 16) === 960 },
    { id: 'X04703', name: '严重超时钳零', check: () => K.benchScore({ name: 'x', pollMs: 100, translateMs: 0, renderMs: 0 }, 10) === 0 },
    { id: 'X04704', name: '劣化判定', check: () => !K.regressed(1000, 980) && K.regressed(1000, 900) && !K.regressed(1000, 950) },
    { id: 'X04705', name: '场景名互异', check: () => new Set(K.BENCH_SUITE.map((b) => b.name)).size === 5 },
  ];
}

/* -------- 族0190 输入遥测 -------- */
export function checkX0190(): CheckEntry[] {
  const t = new K.TelemRing(16);
  t.emit(1, 10);
  const ring = new K.TelemRing(4);
  for (let k = 0; k < 6; k++) ring.emit(k, k);
  return [
    { id: 'X04726', name: '发一条收一条', check: () => t.count(1) === 1 && t.count(2) === 0 },
    { id: 'X04727', name: '环形覆盖最旧', check: () => ring.count(0) === 0 && ring.count(5) === 1 },
    { id: 'X04728', name: '指纹确定性', check: () => t.fingerprint() === t.fingerprint() && t.fingerprint() !== 0 },
    { id: 'X04729', name: '采样率钳制', check: () => K.clampRate(0) === 0 && K.clampRate(100) === 100 && K.clampRate(150) === 100 },
    { id: 'X04730', name: '长稳容量守恒', check: () => { const q = new K.TelemRing(16); for (let k = 0; k < 64; k++) q.emit(k % 4, k); return q.count(1) === 4; } },
  ];
}
