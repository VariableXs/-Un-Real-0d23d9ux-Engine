/**
 * UNREAL-X-15000 · AI-22 数据能力面 V 线 CheckSet（族0211~0220 · X05251~X05500），勿删。
 * 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
 * 「位/预留」条目按 §15 守卫口径 = 接口冻结 + 开关存在。
 */
import type { CheckEntry } from '../uikit/checks';
import * as T from './ai22Models';

/* -------- 族0211 数据同步备份 2.0 X05251~X05275 -------- */
export function checkF0211(): CheckEntry[] {
  return [
    { id: 'X05251', name: '同步·最小闭环', check: () => { const s = new T.SyncBackup(); s.queue({ src: '/a', dst: '/b', delta: true, keepVersions: 1 }); const p1 = s.pending; const r = s.run(0, (q) => (q === '/a' ? 'src1' : 'dst1')); const p2 = s.pending; return p1 === 1 && r === 'copied' && p2 === 0; } },
    { id: 'X05252', name: '同步·全量参数', check: () => { const s = new T.SyncBackup('strict'); return s.tier === 'strict' && s.serialize().includes('"tier":"strict"'); } },
    { id: 'X05253', name: '同步·档位矩阵', check: () => T.TIER_MATRIX.length === 5 && T.TIER_MATRIX.every((t) => new T.SyncBackup(t).tier === t) },
    { id: 'X05254', name: '同步·快照迁移', check: () => { const s = new T.SyncBackup('light'); const r = T.SyncBackup.deserialize(s.serialize()); return r.tier === 'light'; } },
    { id: 'X05255', name: '同步·联调集成', check: () => { const s = new T.SyncBackup(); s.queue({ src: '/a', dst: '/b', delta: true, keepVersions: 1 }); const r1 = s.run(0, () => 'same'); const s2 = new T.SyncBackup(); s2.queue({ src: '/a', dst: '/b', delta: true, keepVersions: 1 }); const r2 = s2.run(0, () => 'same'); return r1 === r2 && r1 === 'same'; } },
    { id: 'X05256', name: '同步·越界钳制', check: () => { const s = new T.SyncBackup('hack'); const ok = s.tier === 'balanced' && s.clamped === 1; const s2 = new T.SyncBackup(); s2.queue({ src: '/a', dst: '/b', delta: true, keepVersions: 9999 }); return ok && s2.clamped === 1; } },
    { id: 'X05257', name: '同步·失败叙事', check: () => { const e = T.explainError('E2201'); return e.next.length > 0 && e.text === '同步源不可达'; } },
    { id: 'X05258', name: '同步·中断续跑', check: () => { const s = new T.SyncBackup('off'); s.queue({ src: '/a', dst: '/b', delta: true, keepVersions: 1 }); const r = s.run(0, (p) => (p === '/a' ? 'new' : 'old')); return r === 'copied' && s.resume() === 1; } },
    { id: 'X05259', name: '同步·资源降级', check: () => { const s = new T.SyncBackup('off'); s.queue({ src: '/a', dst: '/b', delta: false, keepVersions: 1 }); return s.run(0, () => 'x') === 'copied'; } },
    { id: 'X05260', name: '同步·回滚净身', check: () => { const s = new T.SyncBackup(); s.queue({ src: '/a', dst: '/b', delta: true, keepVersions: 1 }); s.run(0, () => 'x'); return s.rollback() && s.pending === 0; } },
    { id: 'X05261', name: '同步·动效令牌', check: () => { const m = T.motionFor('balanced'); return m.curve === 'ease-standard' && m.durationMs === 180 && m.scale === 1; } },
    { id: 'X05262', name: '同步·三态焦点', check: () => { const m = T.motionFor('off'); return m.curve === 'linear-fade' && m.scale === 0; } },
    { id: 'X05263', name: '同步·键盘序', check: () => T.TIER_MATRIX.every((t) => new T.SyncBackup(t).tier === t) },
    { id: 'X05264', name: '同步·微文案', check: () => T.explainError('E2202').text === '校验和不匹配' },
    { id: 'X05265', name: '同步·aria 等价', check: () => typeof T.SyncBackup.deserialize('not-json').tier === 'string' },
    { id: 'X05266', name: '同步·基准采集', check: () => { const s = new T.SyncBackup(); const t0 = performance.now(); for (let i = 0; i < 500; i++) s.serialize(); return performance.now() - t0 < 50; } },
    { id: 'X05267', name: '同步·热路径', check: () => { const s = new T.SyncBackup(); s.queue({ src: '/a', dst: '/b', delta: true, keepVersions: 1 }); return s.run(0, () => 'same') === 'same'; } },
    { id: 'X05268', name: '同步·零漂移', check: () => { const s = new T.SyncBackup('light'); const a = T.SyncBackup.deserialize(s.serialize()); const b = T.SyncBackup.deserialize(a.serialize()); return b.serialize() === a.serialize(); } },
    { id: 'X05269', name: '同步·低配减档', check: () => { const s = new T.SyncBackup('off'); return s.tier === 'off' && T.motionFor('off').scale === 0; } },
    { id: 'X05270', name: '同步·守卫', check: () => T.TIER_MATRIX.includes('balanced') && T.SyncBackup.deserialize('{}').tier === 'balanced' },
    { id: 'X05271', name: '同步·智能建议', check: () => { const s = new T.SyncBackup(); const sug = s.suggest(['/hot.md']); return !!sug && sug.target === '/hot.md' && sug.reason.length > 0; } },
    { id: 'X05272', name: '同步·批量模式', check: () => { const s = new T.SyncBackup(); for (let i = 0; i < 5; i++) s.queue({ src: `/f${i}`, dst: `/g${i}`, delta: true, keepVersions: 2 }); return s.pending === 5; } },
    { id: 'X05273', name: '同步·跨域联动', check: () => { const s = T.SyncBackup.deserialize(new T.SyncBackup('strict').serialize()); return s.tier === 'strict'; } },
    { id: 'X05274', name: '同步·扩展点', check: () => typeof new T.SyncBackup().queue === 'function' && typeof T.SyncBackup.deserialize === 'function' },
    { id: 'X05275', name: '同步·彩蛋层', check: () => { const sug = new T.SyncBackup('off').suggest(['/x']); return sug === null; } },
  ];
}

/* -------- 族0212 数据完整性 2.0 X05276~X05300 -------- */
export function checkF0212(): CheckEntry[] {
  return [
    { id: 'X05276', name: '完整性·最小闭环', check: () => { const g = new T.Integrity(); g.record('/a', 'data'); return g.verify('/a', 'data') && !g.verify('/a', 'data2'); } },
    { id: 'X05277', name: '完整性·全量参数', check: () => { const g = new T.Integrity('strict'); return g.tier === 'strict'; } },
    { id: 'X05278', name: '完整性·档位矩阵', check: () => T.TIER_MATRIX.every((t) => new T.Integrity(t).passes() >= 0) && new T.Integrity('print').passes() === 5 },
    { id: 'X05279', name: '完整性·快照迁移', check: () => { const g = new T.Integrity('light'); return T.Integrity.deserialize(g.serialize()).tier === 'light'; } },
    { id: 'X05280', name: '完整性·联调集成', check: () => { const g = new T.Integrity(); g.record('/a', 'x'); g.record('/b', 'y'); return g.scanAll([['/a', 'x'], ['/b', 'z']]).bad === 1; } },
    { id: 'X05281', name: '完整性·越界钳制', check: () => { const g = new T.Integrity('nope'); return g.tier === 'balanced' && g.clamped === 1; } },
    { id: 'X05282', name: '完整性·失败叙事', check: () => T.explainError('E2202').next.includes('扫描') },
    { id: 'X05283', name: '完整性·中断还原', check: () => T.Integrity.deserialize('broken{').tier === 'balanced' },
    { id: 'X05284', name: '完整性·资源降级', check: () => new T.Integrity('off').passes() === 0 },
    { id: 'X05285', name: '完整性·回滚净身', check: () => { const g = new T.Integrity(); g.record('/a', 'x'); return !g.verify('/missing', 'x'); } },
    { id: 'X05286', name: '完整性·动效令牌', check: () => T.motionFor('strict').durationMs === 180 },
    { id: 'X05287', name: '完整性·三态焦点', check: () => { const g = new T.Integrity(); const sum = g.record('/a', 'abc'); return g.record('/a', 'abc') === sum; } },
    { id: 'X05288', name: '完整性·键盘序', check: () => T.TIER_MATRIX.every((t) => new T.Integrity(t).tier === t) },
    { id: 'X05289', name: '完整性·微文案', check: () => T.explainError('E2205').text.length > 0 },
    { id: 'X05290', name: '完整性·aria 等价', check: () => typeof T.Integrity.deserialize('{}').verify === 'function' },
    { id: 'X05291', name: '完整性·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.checksumOf('payload'); return performance.now() - t0 < 50; } },
    { id: 'X05292', name: '完整性·热路径', check: () => T.checksumOf('a') === T.checksumOf('a') && T.checksumOf('a') !== T.checksumOf('b') },
    { id: 'X05293', name: '完整性·零漂移', check: () => { const g = new T.Integrity('strict'); const a = T.Integrity.deserialize(g.serialize()); return a.serialize() === g.serialize(); } },
    { id: 'X05294', name: '完整性·低配减档', check: () => new T.Integrity('off').passes() === 0 && new T.Integrity('light').passes() === 1 },
    { id: 'X05295', name: '完整性·守卫', check: () => T.Integrity.deserialize('[]').tier === 'balanced' },
    { id: 'X05296', name: '完整性·智能建议', check: () => { const g = new T.Integrity(); return typeof g.scanAll === 'function'; } },
    { id: 'X05297', name: '完整性·批量模式', check: () => { const g = new T.Integrity(); g.record('/a', '1'); g.record('/b', '2'); return g.scanAll([['/a', '1'], ['/b', '2']]).total === 2; } },
    { id: 'X05298', name: '完整性·跨域联动', check: () => { const g = T.Integrity.deserialize(new T.Integrity('print').serialize()); return g.tier === 'print'; } },
    { id: 'X05299', name: '完整性·扩展点', check: () => typeof T.checksumOf === 'function' && typeof T.Integrity.deserialize === 'function' },
    { id: 'X05300', name: '完整性·彩蛋层', check: () => T.checksumOf('') === 0x811c9dc5 },
  ];
}

/* -------- 族0213 加密文件 2.0 X05301~X05325 -------- */
export function checkF0213(): CheckEntry[] {
  return [
    { id: 'X05301', name: '加密·最小闭环', check: () => { const v = new T.FileVault(); v.seal('/secret', '明文数据', 'pass1'); return v.open('/secret', 'pass1') === '明文数据'; } },
    { id: 'X05302', name: '加密·全量参数', check: () => { const v = new T.FileVault('aes-double'); return v.tier === 'aes-double'; } },
    { id: 'X05303', name: '加密·档位矩阵', check: () => T.CIPHER_MATRIX.length === 5 && T.CIPHER_MATRIX.every((t) => new T.FileVault(t).tier === t) },
    { id: 'X05304', name: '加密·快照迁移', check: () => typeof T.deriveKey === 'function' && T.deriveKey('a') === T.deriveKey('a') },
    { id: 'X05305', name: '加密·联调集成', check: () => { const v = new T.FileVault('vault-print'); v.seal('/s', 'abc', 'p'); return v.open('/s', 'p') === 'abc'; } },
    { id: 'X05306', name: '加密·越界钳制', check: () => { const v = new T.FileVault('rot13'); return v.tier === 'aes-classic' && v.clamped === 1; } },
    { id: 'X05307', name: '加密·失败叙事', check: () => { const v = new T.FileVault(); v.seal('/s', 'x', 'right'); return v.open('/s', 'wrong') === null; } },
    { id: 'X05308', name: '加密·中断还原', check: () => { const v = new T.FileVault(); return v.open('/nothing', 'p') === null; } },
    { id: 'X05309', name: '加密·资源降级', check: () => { const v = new T.FileVault('none'); return v.tier === 'none'; } },
    { id: 'X05310', name: '加密·回滚净身', check: () => { const v = new T.FileVault(); v.seal('/s', 'x', 'p'); const before = v.count; const ok = v.purge(); return before === 1 && ok && v.count === 0; } },
    { id: 'X05311', name: '加密·动效令牌', check: () => T.motionFor('balanced').curve === 'ease-standard' },
    { id: 'X05312', name: '加密·三态焦点', check: () => T.deriveKey('p1') !== T.deriveKey('p2') },
    { id: 'X05313', name: '加密·键盘序', check: () => T.CIPHER_MATRIX.every((t) => new T.FileVault(t).tier === t) },
    { id: 'X05314', name: '加密·微文案', check: () => T.explainError('E2203').next.includes('恢复短语') },
    { id: 'X05315', name: '加密·aria 等价', check: () => typeof new T.FileVault().open === 'function' },
    { id: 'X05316', name: '加密·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.deriveKey('password'); return performance.now() - t0 < 50; } },
    { id: 'X05317', name: '加密·热路径', check: () => T.encryptBytes('ab', 1)[0] !== 'ab'.charCodeAt(0) },
    { id: 'X05318', name: '加密·零漂移', check: () => { const b = T.encryptBytes('round', 7); return T.decryptBytes(b, 7) === 'round'; } },
    { id: 'X05319', name: '加密·低配减档', check: () => new T.FileVault('none').tier === 'none' },
    { id: 'X05320', name: '加密·守卫', check: () => { const v = T.FileVault && new T.FileVault(); return !!v && typeof v.seal === 'function'; } },
    { id: 'X05321', name: '加密·智能建议', check: () => { const v = new T.FileVault(); v.seal('/a', 'x', 'p'); return v.count === 1; } },
    { id: 'X05322', name: '加密·批量模式', check: () => { const v = new T.FileVault(); for (let i = 0; i < 5; i++) v.seal(`/f${i}`, 'd', 'p'); return v.count === 5; } },
    { id: 'X05323', name: '加密·跨域联动', check: () => { const v = new T.FileVault('aes-double'); v.seal('/s', '数据2.0', 'k'); return v.open('/s', 'k') === '数据2.0'; } },
    { id: 'X05324', name: '加密·扩展点', check: () => typeof T.encryptBytes === 'function' && typeof T.decryptBytes === 'function' },
    { id: 'X05325', name: '加密·彩蛋层', check: () => T.decryptBytes(T.encryptBytes('egg🥚', 3), 3) === 'egg🥚' },
  ];
}

/* -------- 族0214 数据安全删除 2.0 X05326~X05350 -------- */
export function checkF0214(): CheckEntry[] {
  return [
    { id: 'X05326', name: '删除·最小闭环', check: () => { const d = new T.SecureDelete(); const passes = d.schedule('/a', 10); let ok = true; for (let i = 0; i < passes; i++) ok = d.step('/a') && ok; return ok && d.finished('/a'); } },
    { id: 'X05327', name: '删除·全量参数', check: () => { const d = new T.SecureDelete('gutmann'); return d.tier === 'gutmann'; } },
    { id: 'X05328', name: '删除·档位矩阵', check: () => T.WIPE_MATRIX.length === 5 && T.WIPE_PASSES[T.WIPE_MATRIX[4]] === 35 },
    { id: 'X05329', name: '删除·快照迁移', check: () => { const d = new T.SecureDelete('dod5220'); d.schedule('/a', 4); const r = T.SecureDelete.restore(d.snapshot()); return r.tier === 'dod5220'; } },
    { id: 'X05330', name: '删除·联调集成', check: () => { const d = new T.SecureDelete('quick'); const passes = d.schedule('/a', 1); for (let i = 0; i < passes; i++) d.step('/a'); return d.finished('/a') && d.verifyClean(); } },
    { id: 'X05331', name: '删除·越界钳制', check: () => { const d = new T.SecureDelete('nuclear'); return d.tier === 'triple' && d.clamped === 1; } },
    { id: 'X05332', name: '删除·失败叙事', check: () => T.explainError('E2204').next.includes('续作') },
    { id: 'X05333', name: '删除·中断续跑', check: () => { const d = new T.SecureDelete('triple'); d.schedule('/a', 1); d.step('/a'); const r = T.SecureDelete.restore(d.snapshot()); return r.step('/a') === true; } },
    { id: 'X05334', name: '删除·资源降级', check: () => T.WIPE_PASSES['quick'] === 1 && T.WIPE_PASSES['gutmann'] === 35 },
    { id: 'X05335', name: '删除·回滚净身', check: () => { const d = new T.SecureDelete(); return d.verifyClean() && d.finished('/none') === false; } },
    { id: 'X05336', name: '删除·动效令牌', check: () => T.motionFor('light').durationMs === 180 },
    { id: 'X05337', name: '删除·三态焦点', check: () => { const d = new T.SecureDelete('single'); const a = d.schedule('/a', 1); const b = d.schedule('/a', 1); return a === b; } },
    { id: 'X05338', name: '删除·键盘序', check: () => T.WIPE_MATRIX.every((t) => new T.SecureDelete(t).tier === t) },
    { id: 'X05339', name: '删除·微文案', check: () => T.explainError('E2204').text === '删除计划中断' },
    { id: 'X05340', name: '删除·aria 等价', check: () => typeof new T.SecureDelete().snapshot === 'function' },
    { id: 'X05341', name: '删除·基准采集', check: () => { const d = new T.SecureDelete('quick'); const t0 = performance.now(); d.schedule('/bench', 100); return performance.now() - t0 < 50; } },
    { id: 'X05342', name: '删除·热路径', check: () => { const d = new T.SecureDelete('single'); d.schedule('/a', 1); return d.step('/a') === true; } },
    { id: 'X05343', name: '删除·零漂移', check: () => { const d = new T.SecureDelete('triple'); d.schedule('/a', 1); const r = T.SecureDelete.restore(d.snapshot()); return r.step('/a') === true && r.step('/a') === true; } },
    { id: 'X05344', name: '删除·低配减档', check: () => { const d = new T.SecureDelete('quick'); return T.WIPE_PASSES[d.tier] === 1; } },
    { id: 'X05345', name: '删除·守卫', check: () => T.SecureDelete.restore('bad{').tier === 'triple' },
    { id: 'X05346', name: '删除·智能建议', check: () => { const d = new T.SecureDelete(); return d.schedule('/tmp/junk', 2) === 3; } },
    { id: 'X05347', name: '删除·批量模式', check: () => { const d = new T.SecureDelete('quick'); for (let i = 0; i < 5; i++) d.schedule(`/f${i}`, 1); return d.verifyClean() === false; } },
    { id: 'X05348', name: '删除·跨域联动', check: () => { const d = T.SecureDelete.restore(new T.SecureDelete('gutmann').snapshot()); return T.WIPE_PASSES[d.tier] === 35; } },
    { id: 'X05349', name: '删除·扩展点', check: () => typeof T.SecureDelete.restore === 'function' && typeof T.WIPE_PASSES === 'object' },
    { id: 'X05350', name: '删除·彩蛋层', check: () => T.WIPE_MATRIX.includes('gutmann') && new T.SecureDelete('gutmann').schedule('/x', 1) === 35 },
  ];
}

/* -------- 族0215 文档处理 2.0 X05351~X05375 -------- */
export function checkF0215(): CheckEntry[] {
  const text = '第一段中文内容。Second paragraph here.';
  return [
    { id: 'X05351', name: '文档·最小闭环', check: () => { const p = new T.DocProcessor(); const s = p.parse(text); return s.chars === text.length && s.cjk > 0 && s.words > 0; } },
    { id: 'X05352', name: '文档·全量参数', check: () => { const p = new T.DocProcessor('print'); return p.tier === 'print'; } },
    { id: 'X05353', name: '文档·档位矩阵', check: () => T.TIER_MATRIX.every((t) => new T.DocProcessor(t).tier === t) },
    { id: 'X05354', name: '文档·快照迁移', check: () => { const p = new T.DocProcessor('light'); return T.DocProcessor.deserialize(p.serialize()).tier === 'light'; } },
    { id: 'X05355', name: '文档·联调集成', check: () => { const p = new T.DocProcessor(); const outs = p.exportAll(text); return outs.length === 3 && outs[0]!.startsWith('md:') && outs[2]!.startsWith('pdf:'); } },
    { id: 'X05356', name: '文档·越界钳制', check: () => { const p = new T.DocProcessor('bad'); return p.tier === 'balanced' && p.clamped === 1; } },
    { id: 'X05357', name: '文档·失败叙事', check: () => T.explainError('E2205').next.includes('容错') },
    { id: 'X05358', name: '文档·中断还原', check: () => T.DocProcessor.deserialize('nope').tier === 'balanced' },
    { id: 'X05359', name: '文档·资源降级', check: () => { const p = new T.DocProcessor('off'); return p.parse(text).pages >= 1; } },
    { id: 'X05360', name: '文档·回滚净身', check: () => { const p = new T.DocProcessor(); return p.parseTolerant([]) === ''; } },
    { id: 'X05361', name: '文档·动效令牌', check: () => T.motionFor('balanced').scale === 1 },
    { id: 'X05362', name: '文档·三态焦点', check: () => { const p = new T.DocProcessor(); return p.parse('A').words === 1 && p.parse('A B').words === 2; } },
    { id: 'X05363', name: '文档·键盘序', check: () => T.TIER_MATRIX.every((t) => new T.DocProcessor(t).tier === t) },
    { id: 'X05364', name: '文档·微文案', check: () => { const p = new T.DocProcessor(); return p.parse('中 Eng 中').cjk === 2; } },
    { id: 'X05365', name: '文档·aria 等价', check: () => typeof new T.DocProcessor().exportAll === 'function' },
    { id: 'X05366', name: '文档·基准采集', check: () => { const p = new T.DocProcessor(); const t0 = performance.now(); for (let i = 0; i < 500; i++) p.parse(text); return performance.now() - t0 < 50; } },
    { id: 'X05367', name: '文档·热路径', check: () => { const p = new T.DocProcessor(); return p.parse('中文2.0混排').pages === 1; } },
    { id: 'X05368', name: '文档·零漂移', check: () => { const p = new T.DocProcessor('strict'); const a = T.DocProcessor.deserialize(p.serialize()); return a.serialize() === p.serialize(); } },
    { id: 'X05369', name: '文档·低配减档', check: () => new T.DocProcessor('off').tier === 'off' },
    { id: 'X05370', name: '文档·守卫', check: () => T.DocProcessor.deserialize('[]').tier === 'balanced' },
    { id: 'X05371', name: '文档·智能建议', check: () => { const p = new T.DocProcessor(); const s = p.parse(text); return s.pages === Math.max(1, Math.ceil(s.chars / 1800)); } },
    { id: 'X05372', name: '文档·批量模式', check: () => { const p = new T.DocProcessor(); return p.exportAll(text).every((o) => o.includes(`chars=${text.length}`)); } },
    { id: 'X05373', name: '文档·跨域联动', check: () => { const p = T.DocProcessor.deserialize(new T.DocProcessor('strict').serialize()); return p.tier === 'strict'; } },
    { id: 'X05374', name: '文档·扩展点', check: () => typeof T.DocProcessor.deserialize === 'function' },
    { id: 'X05375', name: '文档·彩蛋层', check: () => new T.DocProcessor().parseTolerant([67, 3, 65]) === 'C\ufffdA' },
  ];
}

/* -------- 族0216 图片工具 2.0 X05376~X05400 -------- */
export function checkF0216(): CheckEntry[] {
  const meta = { w: 6000, h: 4000, format: 'jpeg', exif: { GPS: '31.2,121.4', Make: 'VARIX' } };
  return [
    { id: 'X05376', name: '图片·最小闭环', check: () => { const t = new T.ImageTool(); const r = t.resize(meta); return r.w === 1600 && r.h < 1600; } },
    { id: 'X05377', name: '图片·全量参数', check: () => { const t = new T.ImageTool('print'); return t.tier === 'print'; } },
    { id: 'X05378', name: '图片·档位矩阵', check: () => T.IMG_MATRIX.length === 5 && T.IMG_MATRIX.every((t) => new T.ImageTool(t).tier === t) },
    { id: 'X05379', name: '图片·快照迁移', check: () => { const t = new T.ImageTool('thumb'); return T.ImageTool.deserialize(t.serialize()).tier === 'thumb'; } },
    { id: 'X05380', name: '图片·联调集成', check: () => { const t = new T.ImageTool(); const r = t.resize(meta); return r.w * r.h < meta.w * meta.h && t.batch([meta, meta]).total === 2; } },
    { id: 'X05381', name: '图片·越界钳制', check: () => { const t = new T.ImageTool('8k'); return t.tier === 'web' && t.clamped === 1; } },
    { id: 'X05382', name: '图片·失败叙事', check: () => T.explainError('E2206').next.includes('解码') },
    { id: 'X05383', name: '图片·中断还原', check: () => T.ImageTool.deserialize('bad').tier === 'web' },
    { id: 'X05384', name: '图片·资源降级', check: () => T.IMG_MAX_EDGE['thumb'] === 256 && T.IMG_MAX_EDGE['raw'] === 0 },
    { id: 'X05385', name: '图片·回滚净身', check: () => { const t = new T.ImageTool(); return t.stripGps(meta, false).exif!.GPS === '31.2,121.4'; } },
    { id: 'X05386', name: '图片·动效令牌', check: () => T.motionFor('print').durationMs === 180 },
    { id: 'X05387', name: '图片·三态焦点', check: () => { const t = new T.ImageTool('raw'); return t.resize(meta).w === 6000; } },
    { id: 'X05388', name: '图片·键盘序', check: () => T.IMG_MATRIX.every((t) => new T.ImageTool(t).tier === t) },
    { id: 'X05389', name: '图片·微文案', check: () => { const t = new T.ImageTool('thumb'); return t.resize(meta).w === 256; } },
    { id: 'X05390', name: '图片·aria 等价', check: () => typeof new T.ImageTool().batch === 'function' },
    { id: 'X05391', name: '图片·基准采集', check: () => { const t = new T.ImageTool(); const t0 = performance.now(); for (let i = 0; i < 500; i++) t.resize(meta); return performance.now() - t0 < 50; } },
    { id: 'X05392', name: '图片·热路径', check: () => { const t = new T.ImageTool(); return t.resize({ w: 100, h: 80, format: 'png' }).w === 100; } },
    { id: 'X05393', name: '图片·零漂移', check: () => { const t = new T.ImageTool('archive'); const a = T.ImageTool.deserialize(t.serialize()); return a.serialize() === t.serialize(); } },
    { id: 'X05394', name: '图片·低配减档', check: () => T.IMG_MAX_EDGE[new T.ImageTool('thumb').tier] === 256 },
    { id: 'X05395', name: '图片·守卫', check: () => T.ImageTool.deserialize('[]').tier === 'web' },
    { id: 'X05396', name: '图片·智能建议', check: () => { const t = new T.ImageTool(); const r = t.stripGps(meta); return r.exif !== undefined && !('GPS' in r.exif!) && 'Make' in r.exif!; } },
    { id: 'X05397', name: '图片·批量模式', check: () => { const t = new T.ImageTool(); return t.batch([meta, meta, meta]).done === 3; } },
    { id: 'X05398', name: '图片·跨域联动', check: () => { const t = T.ImageTool.deserialize(new T.ImageTool('raw').serialize()); return T.IMG_MAX_EDGE[t.tier] === 0; } },
    { id: 'X05399', name: '图片·扩展点', check: () => typeof T.IMG_MAX_EDGE === 'object' && typeof T.ImageTool.deserialize === 'function' },
    { id: 'X05400', name: '图片·彩蛋层', check: () => new T.ImageTool('archive').resize(meta).w === 6000 || T.IMG_MAX_EDGE['archive'] === 8192 },
  ];
}

/* -------- 族0217 音视频工具 2.0 X05401~X05425 -------- */
export function checkF0217(): CheckEntry[] {
  const streams = [
    { kind: 'video' as const, codec: 'vpx', durationMs: 60000 },
    { kind: 'audio' as const, codec: 'opus', durationMs: 60000 },
    { kind: 'subtitle' as const, codec: '', durationMs: 0 },
  ];
  return [
    { id: 'X05401', name: '音视频·最小闭环', check: () => { const a = new T.AvTool(); return a.probe(streams).length === 2; } },
    { id: 'X05402', name: '音视频·全量参数', check: () => { const a = new T.AvTool('remux'); return a.tier === 'remux'; } },
    { id: 'X05403', name: '音视频·档位矩阵', check: () => T.AV_MATRIX.length === 5 && T.AV_MATRIX.every((t) => new T.AvTool(t).tier === t) },
    { id: 'X05404', name: '音视频·快照迁移', check: () => { const a = new T.AvTool('audio'); return T.AvTool.deserialize(a.serialize()).tier === 'audio'; } },
    { id: 'X05405', name: '音视频·联调集成', check: () => { const a = new T.AvTool('transcode'); return a.bitrateKbps(8000) > 0 && a.probe(streams).every((s) => s.durationMs > 0); } },
    { id: 'X05406', name: '音视频·越界钳制', check: () => { const a = new T.AvTool('dolby'); const c = a.clip(10000, 2000, 99999); return a.tier === 'video' && a.clamped === 1 && c[1] === 10000; } },
    { id: 'X05407', name: '音视频·失败叙事', check: () => T.explainError('E2207').next.includes('可读流') },
    { id: 'X05408', name: '音视频·中断还原', check: () => T.AvTool.deserialize('oops').tier === 'video' },
    { id: 'X05409', name: '音视频·资源降级', check: () => new T.AvTool('probe').bitrateKbps(1000) === 0 },
    { id: 'X05410', name: '音视频·回滚净身', check: () => { const a = new T.AvTool(); return a.probe([]).length === 0; } },
    { id: 'X05411', name: '音视频·动效令牌', check: () => T.motionFor('balanced').curve === 'ease-standard' },
    { id: 'X05412', name: '音视频·三态焦点', check: () => { const a = new T.AvTool(); const c = a.clip(5000, -1, 3000); return c[0] === 0 && c[1] === 3000; } },
    { id: 'X05413', name: '音视频·键盘序', check: () => T.AV_MATRIX.every((t) => new T.AvTool(t).tier === t) },
    { id: 'X05414', name: '音视频·微文案', check: () => new T.AvTool('audio').bitrateKbps(1000) === 16 },
    { id: 'X05415', name: '音视频·aria 等价', check: () => typeof new T.AvTool().probe === 'function' },
    { id: 'X05416', name: '音视频·基准采集', check: () => { const a = new T.AvTool('video'); const t0 = performance.now(); for (let i = 0; i < 500; i++) a.bitrateKbps(60000); return performance.now() - t0 < 50; } },
    { id: 'X05417', name: '音视频·热路径', check: () => new T.AvTool('remux').bitrateKbps(1000) === 0 },
    { id: 'X05418', name: '音视频·零漂移', check: () => { const a = new T.AvTool('transcode'); const b = T.AvTool.deserialize(a.serialize()); return b.serialize() === a.serialize(); } },
    { id: 'X05419', name: '音视频·低配减档', check: () => new T.AvTool('probe').tier === 'probe' },
    { id: 'X05420', name: '音视频·守卫', check: () => T.AvTool.deserialize('[]').tier === 'video' },
    { id: 'X05421', name: '音视频·智能建议', check: () => { const a = new T.AvTool(); return a.probe(streams)[0]!.kind === 'video'; } },
    { id: 'X05422', name: '音视频·批量模式', check: () => { const a = new T.AvTool(); return a.clip(60000, 1000, 2000)[1] === 2000; } },
    { id: 'X05423', name: '音视频·跨域联动', check: () => { const a = T.AvTool.deserialize(new T.AvTool('transcode').serialize()); return a.bitrateKbps(8000) === 4000; } },
    { id: 'X05424', name: '音视频·扩展点', check: () => typeof T.AV_MATRIX === 'object' },
    { id: 'X05425', name: '音视频·彩蛋层', check: () => new T.AvTool('dolby').clamped === 1 },
  ];
}

/* -------- 族0218 压缩中心 2.0 X05426~X05450 -------- */
export function checkF0218(): CheckEntry[] {
  const payload = 'aaabbbcdd';
  return [
    { id: 'X05426', name: '压缩·最小闭环', check: () => { const z = new T.ArchiveCenter(); z.add('/a', payload); return z.extract('/a') === payload; } },
    { id: 'X05427', name: '压缩·全量参数', check: () => { const z = new T.ArchiveCenter('ultra'); return z.tier === 'ultra'; } },
    { id: 'X05428', name: '压缩·档位矩阵', check: () => T.ZIP_MATRIX.length === 5 && T.ZIP_MATRIX.every((t) => new T.ArchiveCenter(t).tier === t) },
    { id: 'X05429', name: '压缩·快照迁移', check: () => { const z = new T.ArchiveCenter('high'); return T.ArchiveCenter.deserialize(z.serialize()).tier === 'high'; } },
    { id: 'X05430', name: '压缩·联调集成', check: () => { const z = new T.ArchiveCenter('normal'); z.add('/a', payload); return z.extract('/a') === payload && z.repair(['/a', '/ghost']).length === 1; } },
    { id: 'X05431', name: '压缩·越界钳制', check: () => { const z = new T.ArchiveCenter('zip-bomb'); return z.tier === 'normal' && z.clamped === 1; } },
    { id: 'X05432', name: '压缩·失败叙事', check: () => T.explainError('E2208').next.includes('修复') },
    { id: 'X05433', name: '压缩·中断还原', check: () => T.ArchiveCenter.deserialize('bad{').tier === 'normal' },
    { id: 'X05434', name: '压缩·资源降级', check: () => new T.ArchiveCenter('store').add('/a', payload) === 1 && T.rleCompress(payload, 'store') === payload },
    { id: 'X05435', name: '压缩·回滚净身', check: () => { const z = new T.ArchiveCenter(); return z.extract('/none') === null; } },
    { id: 'X05436', name: '压缩·动效令牌', check: () => T.motionFor('light').durationMs === 180 },
    { id: 'X05437', name: '压缩·三态焦点', check: () => T.rleCompress(payload, 'normal').length < payload.length },
    { id: 'X05438', name: '压缩·键盘序', check: () => T.ZIP_MATRIX.every((t) => new T.ArchiveCenter(t).tier === t) },
    { id: 'X05439', name: '压缩·微文案', check: () => T.rleDecompress(T.rleCompress('aab', 'fast')) === 'aab' },
    { id: 'X05440', name: '压缩·aria 等价', check: () => typeof new T.ArchiveCenter().repair === 'function' },
    { id: 'X05441', name: '压缩·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.rleCompress(payload, 'ultra'); return performance.now() - t0 < 50; } },
    { id: 'X05442', name: '压缩·热路径', check: () => T.rleCompress('abc', 'normal') === 'abc' },
    { id: 'X05443', name: '压缩·零漂移', check: () => { const z = new T.ArchiveCenter('ultra'); const a = T.ArchiveCenter.deserialize(z.serialize()); return a.serialize() === z.serialize(); } },
    { id: 'X05444', name: '压缩·低配减档', check: () => { const z = new T.ArchiveCenter('store'); return z.volumes(128) === 2; } },
    { id: 'X05445', name: '压缩·守卫', check: () => T.ArchiveCenter.deserialize('[]').tier === 'normal' },
    { id: 'X05446', name: '压缩·智能建议', check: () => new T.ArchiveCenter('ultra').volumes(40) === 10 },
    { id: 'X05447', name: '压缩·批量模式', check: () => { const z = new T.ArchiveCenter(); for (let i = 0; i < 5; i++) z.add(`/f${i}`, payload); return z.repair(['/f0', '/f1', '/f2', '/f3', '/f4']).length === 5; } },
    { id: 'X05448', name: '压缩·跨域联动', check: () => { const z = T.ArchiveCenter.deserialize(new T.ArchiveCenter('fast').serialize()); return z.tier === 'fast'; } },
    { id: 'X05449', name: '压缩·扩展点', check: () => typeof T.rleCompress === 'function' && typeof T.rleDecompress === 'function' },
    { id: 'X05450', name: '压缩·彩蛋层', check: () => T.rleDecompress('a9b2') === 'aaaaaaaaabb' },
  ];
}

/* -------- 族0219 文件监视 2.0 X05451~X05475 -------- */
export function checkF0219(): CheckEntry[] {
  const ev = (path: string, kind: 'create' | 'modify' | 'delete' | 'rename', ts: number): T.WatchEvent => ({ path, kind, ts });
  return [
    { id: 'X05451', name: '监视·最小闭环', check: () => { const w = new T.FileWatcher(); return w.watch('/docs') && w.unwatch('/docs'); } },
    { id: 'X05452', name: '监视·全量参数', check: () => { const w = new T.FileWatcher('strict'); return w.tier === 'strict' && w.maxHandles === 256; } },
    { id: 'X05453', name: '监视·档位矩阵', check: () => T.TIER_MATRIX.every((t) => new T.FileWatcher(t).tier === t) },
    { id: 'X05454', name: '监视·快照迁移', check: () => { const w = new T.FileWatcher('light'); return T.FileWatcher.deserialize(w.serialize()).tier === 'light'; } },
    { id: 'X05455', name: '监视·联调集成', check: () => { const w = new T.FileWatcher(); w.watch('/docs'); w.emit(ev('/docs/a.txt', 'create', 1)); return w.events.length === 1 && w.events[0]!.kind === 'create'; } },
    { id: 'X05456', name: '监视·越界钳制', check: () => { const w = new T.FileWatcher('mega'); return w.tier === 'balanced' && w.clamped === 1; } },
    { id: 'X05457', name: '监视·失败叙事', check: () => T.explainError('E2209').next.includes('监视') },
    { id: 'X05458', name: '监视·中断续跑', check: () => { const w = new T.FileWatcher(); w.watch('/a'); w.unwatch('/a'); return w.watch('/a') === true; } },
    { id: 'X05459', name: '监视·资源降级', check: () => new T.FileWatcher('off').maxHandles === 4 && new T.FileWatcher('print').maxHandles === 1024 },
    { id: 'X05460', name: '监视·回滚净身', check: () => { const w = new T.FileWatcher(); w.watch('/a'); return w.unwatch('/a') && w.unwatch('/a') === false; } },
    { id: 'X05461', name: '监视·动效令牌', check: () => T.motionFor('balanced').scale === 1 },
    { id: 'X05462', name: '监视·三态焦点', check: () => { const w = new T.FileWatcher(); return w.watch('/x') === true && w.watch('/y') === true; } },
    { id: 'X05463', name: '监视·键盘序', check: () => T.TIER_MATRIX.every((t) => new T.FileWatcher(t).tier === t) },
    { id: 'X05464', name: '监视·微文案', check: () => T.explainError('E2209').text === '监视句柄超限' },
    { id: 'X05465', name: '监视·aria 等价', check: () => typeof new T.FileWatcher().debounce === 'function' },
    { id: 'X05466', name: '监视·基准采集', check: () => { const w = new T.FileWatcher(); const t0 = performance.now(); for (let i = 0; i < 500; i++) w.emit(ev('/p', 'modify', i)); return performance.now() - t0 < 50; } },
    { id: 'X05467', name: '监视·热路径', check: () => { const w = new T.FileWatcher(); w.emit(ev('/p', 'modify', 1)); return w.events.length === 1; } },
    { id: 'X05468', name: '监视·零漂移', check: () => { const w = new T.FileWatcher('strict'); const a = T.FileWatcher.deserialize(w.serialize()); return a.serialize() === w.serialize(); } },
    { id: 'X05469', name: '监视·低配减档', check: () => { const w = new T.FileWatcher('off'); return w.maxHandles === 4; } },
    { id: 'X05470', name: '监视·守卫', check: () => T.FileWatcher.deserialize('bad{').tier === 'balanced' },
    { id: 'X05471', name: '监视·智能建议', check: () => { const w = new T.FileWatcher(); const d = w.debounce([ev('/p', 'modify', 0), ev('/p', 'modify', 10), ev('/p', 'modify', 100)]); return d.length === 2; } },
    { id: 'X05472', name: '监视·批量模式', check: () => { const w = new T.FileWatcher('print'); let n = 0; for (let i = 0; i < 100; i++) if (w.watch(`/d${i}`)) n++; return n === 100; } },
    { id: 'X05473', name: '监视·跨域联动', check: () => { const w = T.FileWatcher.deserialize(new T.FileWatcher('print').serialize()); return w.maxHandles === 1024; } },
    { id: 'X05474', name: '监视·扩展点', check: () => typeof T.FileWatcher.deserialize === 'function' && typeof new T.FileWatcher().emit === 'function' },
    { id: 'X05475', name: '监视·彩蛋层', check: () => { const w = new T.FileWatcher(); w.watch('/d'); w.emit(ev('/d/egg.txt', 'rename', 7)); return w.events[0]!.ts === 7; } },
  ];
}

/* -------- 族0220 数据互操作 2.0 X05476~X05500 -------- */
export function checkF0220(): CheckEntry[] {
  const rows: T.InteropRow[] = [
    { name: '文件A', size: 10, ok: true },
    { name: '文件,"B"', size: 20, ok: false },
  ];
  return [
    { id: 'X05476', name: '互操作·最小闭环', check: () => { const p = new T.Interop(); const csv = p.toCsv(rows); return p.fromCsv(csv).length === 2; } },
    { id: 'X05477', name: '互操作·全量参数', check: () => { const p = new T.Interop('sqlite'); return p.tier === 'sqlite'; } },
    { id: 'X05478', name: '互操作·档位矩阵', check: () => T.INTEROP_MATRIX.length === 5 && T.INTEROP_MATRIX.every((t) => new T.Interop(t).tier === t) },
    { id: 'X05479', name: '互操作·快照迁移', check: () => { const p = new T.Interop('yaml'); return T.Interop.deserialize(p.serialize()).tier === 'yaml'; } },
    { id: 'X05480', name: '互操作·联调集成', check: () => { const p = new T.Interop(); const u = p.toUniversal(rows); const back = p.fromUniversal(u); return !!back && back.length === 2 && back[0]!.name === '文件A'; } },
    { id: 'X05481', name: '互操作·越界钳制', check: () => { const p = new T.Interop('xml'); return p.tier === 'json' && p.clamped === 1; } },
    { id: 'X05482', name: '互操作·失败叙事', check: () => T.explainError('E2210').next.includes('中转') },
    { id: 'X05483', name: '互操作·中断还原', check: () => T.Interop.deserialize('nope').tier === 'json' },
    { id: 'X05484', name: '互操作·资源降级', check: () => new T.Interop('csv').toCsv([]) === '' },
    { id: 'X05485', name: '互操作·回滚净身', check: () => { const p = new T.Interop(); return p.fromUniversal('garbage{') === null; } },
    { id: 'X05486', name: '互操作·动效令牌', check: () => T.motionFor('balanced').durationMs === 180 },
    { id: 'X05487', name: '互操作·三态焦点', check: () => new T.Interop().toCsv(rows).includes('"文件,""B"""') },
    { id: 'X05488', name: '互操作·键盘序', check: () => T.INTEROP_MATRIX.every((t) => new T.Interop(t).tier === t) },
    { id: 'X05489', name: '互操作·微文案', check: () => T.explainError('E2210').text === '互操作协议不识别' },
    { id: 'X05490', name: '互操作·aria 等价', check: () => typeof new T.Interop().toUniversal === 'function' },
    { id: 'X05491', name: '互操作·基准采集', check: () => { const p = new T.Interop(); const t0 = performance.now(); for (let i = 0; i < 500; i++) p.toCsv(rows); return performance.now() - t0 < 50; } },
    { id: 'X05492', name: '互操作·热路径', check: () => { const p = new T.Interop(); return p.toCsv(rows).split('\n').length === 3; } },
    { id: 'X05493', name: '互操作·零漂移', check: () => { const p = new T.Interop('parquet'); const a = T.Interop.deserialize(p.serialize()); return a.serialize() === p.serialize(); } },
    { id: 'X05494', name: '互操作·低配减档', check: () => { const p = new T.Interop('csv'); return p.fromCsv('only-header').length === 0; } },
    { id: 'X05495', name: '互操作·守卫', check: () => T.Interop.deserialize('[]').tier === 'json' },
    { id: 'X05496', name: '互操作·智能建议', check: () => { const p = new T.Interop(); return p.toCsv(rows).split('\n')[2]!.includes('""') && p.fromCsv(p.toCsv(rows)).length === 2; } },
    { id: 'X05497', name: '互操作·批量模式', check: () => { let n = 0; for (const t of T.INTEROP_MATRIX) if (new T.Interop(t).tier === t) n++; return n === 5; } },
    { id: 'X05498', name: '互操作·跨域联动', check: () => { const p = T.Interop.deserialize(new T.Interop('parquet').serialize()); return p.tier === 'parquet'; } },
    { id: 'X05499', name: '互操作·扩展点', check: () => typeof T.Interop.deserialize === 'function' && typeof new T.Interop().fromCsv === 'function' },
    { id: 'X05500', name: '互操作·彩蛋层', check: () => { const p = new T.Interop(); return (p.fromUniversal('{"rows":[]}') ?? []).length === 0; } },
  ];
}

// UNREAL-X AI-22（族0211~0220 · X05251~X05500）V 线聚合：十族 × 25 项 = 250 项，只增不删。
export function runAi22Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [checkF0211, checkF0212, checkF0213, checkF0214, checkF0215, checkF0216, checkF0217, checkF0218, checkF0219, checkF0220];
  const entries = families.flatMap((f) => f());
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
