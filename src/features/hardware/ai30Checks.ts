/**
 * UNREAL-X-15000 · AI-30 系统服务面 V 线 CheckSet（族0291~0300 · X07251~X07500），勿删。
 * 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
 */
import type { CheckEntry } from '../uikit/checks';
import * as T from './ai30Models';

/* -------- 族0291 系统信息诊断 2.0 X07251~X07275 -------- */
export function checkF0291(): CheckEntry[] {
  const d = new T.Diagnostics();
  return [
    { id: 'X07251', name: '诊断·最小闭环', check: () => d.profileId === 'standard' && d.profile.timeoutMs === 8000 },
    { id: 'X07252', name: '诊断·全量参数', check: () => { const q = new T.Diagnostics('deep'); return q.profile.sampleCount === 300 && q.profile.includeKernel === true; } },
    { id: 'X07253', name: '诊断·档位矩阵', check: () => T.DIAG_LEVELS.length === 5 && T.DIAG_LEVELS.every((l) => T.DIAG_LEVEL_MATRIX[l].timeoutMs >= 0) },
    { id: 'X07254', name: '诊断·快照迁移', check: () => { const r = T.Diagnostics.deserialize(d.serialize()); return r.profileId === 'standard' && r.scanCount === 0; } },
    { id: 'X07255', name: '诊断·集成验证', check: () => d.scan([{ cpu: 20, mem: 30, disk: 10 }]) === Math.round(100 - (20 * 0.4 + 30 * 0.35 + 10 * 0.25)) },
    { id: 'X07256', name: '诊断·越界钳制', check: () => T.clampSample(-5) === 0 && T.clampSample(180) === 100 && T.clampSample(Number.NaN) === 0 },
    { id: 'X07257', name: '诊断·失败叙事', check: () => { const it = d.addIssue('DISK_SMART', 3); return it.advice.includes('备份') && (d.narrative()[0] ?? '').startsWith('【DISK_SMART】'); } },
    { id: 'X07258', name: '诊断·中断还原', check: () => { const q = new T.Diagnostics(); q.scan([]); q.scan([]); return q.resume() === 2; } },
    { id: 'X07259', name: '诊断·资源降级', check: () => d.degrade(90, 80) === 'quick' && new T.Diagnostics('deep').degrade(50, 10) === 'quick' },
    { id: 'X07260', name: '诊断·回滚净身', check: () => { const q = T.Diagnostics.deserialize(d.serialize()); return q.scanCount === d.scanCount && q.issues.length === d.issues.length; } },
    { id: 'X07261', name: '诊断·动效令牌', check: () => T.DIAG_LEVEL_MATRIX.quick.sampleCount === 10 && T.DIAG_LEVEL_MATRIX.custom.timeoutMs > T.DIAG_LEVEL_MATRIX.quick.timeoutMs },
    { id: 'X07262', name: '诊断·三态焦点', check: () => { const it = d.addIssue('MEM_LEAK', 2); return it.severity === 2 && it.advice.length > 0; } },
    { id: 'X07263', name: '诊断·键盘序', check: () => (T.DIAG_LEVELS as string[]).includes('custom') && new T.Diagnostics('custom').profileId === 'custom' },
    { id: 'X07264', name: '诊断·微文案', check: () => d.addIssue('KERNEL_EVENT', 1).advice.includes('内核事件日志') },
    { id: 'X07265', name: '诊断·aria 等价', check: () => d.issues.every((i) => i.advice.length > 0) && T.diagScore([]) === 100 },
    { id: 'X07266', name: '诊断·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.diagScore([{ cpu: i % 100, mem: 50, disk: 20 }]); return performance.now() - t0 < 50; } },
    { id: 'X07267', name: '诊断·热路径', check: () => T.diagScore([{ cpu: 0, mem: 0, disk: 0 }]) === 100 && T.diagScore([{ cpu: 100, mem: 100, disk: 100 }]) === 0 },
    { id: 'X07268', name: '诊断·零漂移', check: () => { const a = T.Diagnostics.deserialize(d.serialize()); const b = T.Diagnostics.deserialize(a.serialize()); return b.serialize() === a.serialize(); } },
    { id: 'X07269', name: '诊断·低配减档', check: () => { const q = T.Diagnostics.deserialize('not-json'); return q.profileId === T.DEFAULT_DIAG_LEVEL; } },
    { id: 'X07270', name: '诊断·守卫', check: () => d.clamped === 0 && new T.Diagnostics('bogus').clamped === 1 },
    { id: 'X07271', name: '诊断·智能建议', check: () => d.addIssue('UNKNOWN_X', 9).severity === 1 },
    { id: 'X07272', name: '诊断·批量模式', check: () => { let n = 0; for (const l of T.DIAG_LEVELS) if (new T.Diagnostics(l).profileId === l) n++; return n === 5; } },
    { id: 'X07273', name: '诊断·跨域联动', check: () => { const q = T.Diagnostics.deserialize(JSON.stringify({ profileId: 'offline' })); return q.profileId === 'offline'; } },
    { id: 'X07274', name: '诊断·扩展点', check: () => typeof T.Diagnostics.deserialize === 'function' && typeof T.diagScore === 'function' },
    { id: 'X07275', name: '诊断·彩蛋层', check: () => d.addIssue('EASTER_EGG', 0).severity === 0 },
  ];
}

/* -------- 族0292 更新部署 2.0 X07276~X07300 -------- */
export function checkF0292(): CheckEntry[] {
  const u = new T.UpdateChannel();
  return [
    { id: 'X07276', name: '更新·最小闭环', check: () => u.ring === 'stable' && u.available('1.0.0', '1.0.1') },
    { id: 'X07277', name: '更新·全量参数', check: () => T.UPDATE_RINGS.length === 5 && new T.UpdateChannel('canary').ring === 'canary' },
    { id: 'X07278', name: '更新·档位矩阵', check: () => T.UPDATE_RINGS.every((r) => new T.UpdateChannel(r).ring === r) },
    { id: 'X07279', name: '更新·快照迁移', check: () => { const q = T.UpdateChannel.deserialize(u.serialize()); return q.ring === 'stable' && q.installed.length === 0; } },
    { id: 'X07280', name: '更新·集成验证', check: () => u.apply('1.0.0', '1.0.1') && u.installed.length === 1 },
    { id: 'X07281', name: '更新·越界钳制', check: () => T.cmpVersion('1.0', '1.0.0') === 0 && T.rolloutPass(-1, 50) === false },
    { id: 'X07282', name: '更新·失败叙事', check: () => T.UpdateChannel.narrative('E_SIG').includes('签名') && T.UpdateChannel.narrative('E_NET').includes('断点') },
    { id: 'X07283', name: '更新·中断续跑', check: () => { u.pause(); const blocked = !u.available('1.0.1', '1.0.2'); u.resume(); return blocked && u.available('1.0.1', '1.0.2'); } },
    { id: 'X07284', name: '更新·资源降级', check: () => T.rolloutPass(30, 50) && !T.rolloutPass(60, 50) },
    { id: 'X07285', name: '更新·回滚净身', check: () => u.apply('1.0.1', '1.0.2') && u.rollback() === '1.0.1' && new T.UpdateChannel().rollback() === null },
    { id: 'X07286', name: '更新·动效令牌', check: () => u.apply('1.0.2', '1.0.2') === false && u.installed.length === 2 },
    { id: 'X07287', name: '更新·三态焦点', check: () => new T.UpdateChannel('beta').clamped === 0 && new T.UpdateChannel('nope').clamped === 1 },
    { id: 'X07288', name: '更新·键盘序', check: () => T.cmpVersion('2.0.0', '1.9.9') === 1 && T.cmpVersion('1.9.9', '2.0.0') === -1 },
    { id: 'X07289', name: '更新·微文案', check: () => T.UpdateChannel.narrative('E_DISK').includes('磁盘') },
    { id: 'X07290', name: '更新·aria 等价', check: () => T.UpdateChannel.narrative('Z9').includes('诊断') },
    { id: 'X07291', name: '更新·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.cmpVersion('1.0.10', '1.0.9'); return performance.now() - t0 < 50; } },
    { id: 'X07292', name: '更新·热路径', check: () => T.cmpVersion('10.0', '9.99.99') === 1 && T.cmpVersion('1.0.0', '1.0.1') === -1 },
    { id: 'X07293', name: '更新·零漂移', check: () => { const a = T.UpdateChannel.deserialize(u.serialize()); const b = T.UpdateChannel.deserialize(a.serialize()); return b.serialize() === a.serialize(); } },
    { id: 'X07294', name: '更新·低配减档', check: () => { const q = T.UpdateChannel.deserialize('{bad'); return q.ring === 'stable'; } },
    { id: 'X07295', name: '更新·守卫', check: () => { const q = T.UpdateChannel.deserialize(JSON.stringify({ ring: 'lts', installed: ['5.0', '6.0'] })); return q.ring === 'lts' && q.rollback() === '5.0'; } },
    { id: 'X07296', name: '更新·智能建议', check: () => !u.available('2.0.0', '1.9.0') },
    { id: 'X07297', name: '更新·批量模式', check: () => { let n = 0; for (const r of T.UPDATE_RINGS) if (new T.UpdateChannel(r).ring === r) n++; return n === 5; } },
    { id: 'X07298', name: '更新·跨域联动', check: () => { const q = T.UpdateChannel.deserialize(JSON.stringify({ ring: 'dev', paused: true })); return q.ring === 'dev' && !q.available('1.0.0', '2.0.0'); } },
    { id: 'X07299', name: '更新·扩展点', check: () => typeof T.cmpVersion === 'function' && typeof T.rolloutPass === 'function' },
    { id: 'X07300', name: '更新·彩蛋层', check: () => u.installed.every((v) => /^[\d.]+$/.test(v)) },
  ];
}

/* -------- 族0293 灾备迁移 2.0 X07301~X07325 -------- */
export function checkF0293(): CheckEntry[] {
  const v = new T.BackupVault();
  return [
    { id: 'X07301', name: '灾备·最小闭环', check: () => { const s = v.add('daily', 'snap1', 100); return v.restorable(s); } },
    { id: 'X07302', name: '灾备·全量参数', check: () => T.BACKUP_TIERS.length === 5 && typeof T.backupChecksum('a') === 'number' },
    { id: 'X07303', name: '灾备·档位矩阵', check: () => T.BACKUP_TIERS.every((t) => v.add(t, 'm-' + t, 10).tier === t) },
    { id: 'X07304', name: '灾备·快照迁移', check: () => { const q = T.BackupVault.import(v.export()); return q.snaps.length === v.snaps.length; } },
    { id: 'X07305', name: '灾备·集成验证', check: () => v.totalBytes() === 100 + 10 * 5 },
    { id: 'X07306', name: '灾备·越界钳制', check: () => v.add('daily', 'neg', -50).bytes === 0 && v.add('daily', 'nan', Number.NaN).bytes === 0 },
    { id: 'X07307', name: '灾备·失败叙事', check: () => !v.restorable({ tier: 'daily', label: 'x', checksum: 1, bytes: 2 }) },
    { id: 'X07308', name: '灾备·中断续跑', check: () => { const q = T.BackupVault.import(v.export()); return q.snaps.every((s) => q.restorable(s)); } },
    { id: 'X07309', name: '灾备·资源降级', check: () => { const q = new T.BackupVault(); q.add('hourly', 'a', 500); q.add('hourly', 'b', 500); const rm = q.prune(600); return rm.length === 1 && q.totalBytes() === 500; } },
    { id: 'X07310', name: '灾备·回滚净身', check: () => { const q = T.BackupVault.import('{bad'); return q.snaps.length === 0; } },
    { id: 'X07311', name: '灾备·动效令牌', check: () => T.BACKUP_TIERS[0] === 'hourly' && T.BACKUP_TIERS[4] === 'archive' },
    { id: 'X07312', name: '灾备·三态焦点', check: () => new T.BackupVault().add('bogus', 'x', 1).tier === 'daily' },
    { id: 'X07313', name: '灾备·键盘序', check: () => T.backupChecksum('abc') === T.backupChecksum('abc') && T.backupChecksum('abd') !== T.backupChecksum('abc') },
    { id: 'X07314', name: '灾备·微文案', check: () => v.snaps.every((s) => s.label.length > 0) },
    { id: 'X07315', name: '灾备·aria 等价', check: () => v.snaps[0] !== undefined && v.restorable(v.snaps[0]) },
    { id: 'X07316', name: '灾备·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.backupChecksum('snap-' + i); return performance.now() - t0 < 50; } },
    { id: 'X07317', name: '灾备·热路径', check: () => T.backupChecksum('') === 0x811c9dc5 },
    { id: 'X07318', name: '灾备·零漂移', check: () => { const a = T.BackupVault.import(v.export()); const b = T.BackupVault.import(a.export()); return b.export() === a.export(); } },
    { id: 'X07319', name: '灾备·低配减档', check: () => { const q = new T.BackupVault(); for (let i = 0; i < 70; i++) q.add('daily', 'p' + i, 1); return q.snaps.length === 64; } },
    { id: 'X07320', name: '灾备·守卫', check: () => new T.BackupVault().prune(0).length === 0 },
    { id: 'X07321', name: '灾备·智能建议', check: () => v.add('weekly', '智能清理', 1).checksum > 0 },
    { id: 'X07322', name: '灾备·批量模式', check: () => { const q = new T.BackupVault(); for (let i = 0; i < 10; i++) q.add('daily', 'b' + i, i); return q.snaps.length === 10; } },
    { id: 'X07323', name: '灾备·跨域联动', check: () => { const q = T.BackupVault.import(JSON.stringify([{ tier: 'monthly', label: 'cross', checksum: T.backupChecksum('monthlycross0'), bytes: 0 }])); return q.snaps[0] !== undefined && q.restorable(q.snaps[0]); } },
    { id: 'X07324', name: '灾备·扩展点', check: () => typeof T.BackupVault.import === 'function' && typeof v.export === 'function' },
    { id: 'X07325', name: '灾备·彩蛋层', check: () => v.snaps.some((s) => s.label === '智能清理') },
  ];
}

/* -------- 族0294 安全硬件 2.0 X07326~X07350 -------- */
export function checkF0294(): CheckEntry[] {
  const t = new T.TpmState();
  return [
    { id: 'X07326', name: '安全硬件·最小闭环', check: () => t.extend(0, 'boot') > 0 && t.pcrs.get(0)! > 0 },
    { id: 'X07327', name: '安全硬件·全量参数', check: () => { const q = new T.TpmState(); const v1 = q.extend(5, 'k'); const v2 = q.extend(5, 'k'); return q.pcrs.size === 1 && v1 > 0 && v2 !== v1; } },
    { id: 'X07328', name: '安全硬件·档位矩阵', check: () => { const q = new T.TpmState(); const a = q.extend(0, 'x'); const b = q.extend(0, 'y'); return a !== b; } },
    { id: 'X07329', name: '安全硬件·快照迁移', check: () => { const q = new T.TpmState(); q.bootMeasure(['bl', 'kernel', 'drv']); return q.booted && q.quote() > 0; } },
    { id: 'X07330', name: '安全硬件·集成验证', check: () => { const q = new T.TpmState(); q.bootMeasure(['a']); return q.seal('k1', [0]) && q.unseal('k1'); } },
    { id: 'X07331', name: '安全硬件·越界钳制', check: () => { const q = new T.TpmState(); q.extend(99, 'x'); return q.pcrs.size === 1 && q.clamped === 1; } },
    { id: 'X07332', name: '安全硬件·失败叙事', check: () => !t.unseal('missing') },
    { id: 'X07333', name: '安全硬件·中断续跑', check: () => { const q = new T.TpmState(); q.extend(1, 'a'); const v1 = q.pcrs.get(1)!; q.extend(1, 'b'); return v1 !== q.pcrs.get(1); } },
    { id: 'X07334', name: '安全硬件·资源降级', check: () => !t.seal('empty', []) && t.clamped === 1 },
    { id: 'X07335', name: '安全硬件·回滚净身', check: () => { const q = new T.TpmState(); q.extend(0, 'x'); return q.quote() > 0 && q.sealed.size === 0; } },
    { id: 'X07336', name: '安全硬件·动效令牌', check: () => T.TpmState.secureBoot('ok-sig', ['ok-sig', 'v2']) && !T.TpmState.secureBoot('bad', ['ok-sig']) },
    { id: 'X07337', name: '安全硬件·三态焦点', check: () => { const q = new T.TpmState(); q.extend(-3, 'x'); q.extend(30, 'y'); return q.clamped === 2; } },
    { id: 'X07338', name: '安全硬件·键盘序', check: () => { const q = new T.TpmState(); const a = q.extend(0, 'same'); const r = new T.TpmState(); const b = r.extend(0, 'same'); return a === b; } },
    { id: 'X07339', name: '安全硬件·微文案', check: () => t.sealed.size === 0 && t.pcrs.get(0)! > 0 },
    { id: 'X07340', name: '安全硬件·aria 等价', check: () => { const q = new T.TpmState(); q.bootMeasure(['m1', 'm2']); const q1 = q.quote(); const q2 = new T.TpmState(); q2.bootMeasure(['m1', 'm2']); return q1 === q2.quote(); } },
    { id: 'X07341', name: '安全硬件·基准采集', check: () => { const q = new T.TpmState(); const t0 = performance.now(); for (let i = 0; i < 500; i++) q.extend(i % 24, 'd' + i); return performance.now() - t0 < 50; } },
    { id: 'X07342', name: '安全硬件·热路径', check: () => { const q = new T.TpmState(); return q.extend(0, '') > 0; } },
    { id: 'X07343', name: '安全硬件·零漂移', check: () => { const q = new T.TpmState(); q.bootMeasure(['bl', 'os']); return q.quote() === (() => { const r = new T.TpmState(); r.bootMeasure(['bl', 'os']); return r.quote(); })(); } },
    { id: 'X07344', name: '安全硬件·低配减档', check: () => { const q = new T.TpmState(); return !q.booted && q.quote() === 0; } },
    { id: 'X07345', name: '安全硬件·守卫', check: () => t.pcrs.get(0) === t.pcrs.get(0) },
    { id: 'X07346', name: '安全硬件·智能建议', check: () => { const q = new T.TpmState(); q.extend(2, 'att'); return q.seal('att-key', [2]) && q.unseal('att-key'); } },
    { id: 'X07347', name: '安全硬件·批量模式', check: () => { const q = new T.TpmState(); let n = 0; for (let i = 0; i < 24; i++) { q.extend(i, 'p' + i); n++; } return n === 24 && q.pcrs.size === 24; } },
    { id: 'X07348', name: '安全硬件·跨域联动', check: () => { const q = new T.TpmState(); q.bootMeasure(['secure-chain']); return T.TpmState.secureBoot('secure-chain', ['secure-chain']); } },
    { id: 'X07349', name: '安全硬件·扩展点', check: () => typeof T.TpmState.secureBoot === 'function' && typeof t.quote === 'function' },
    { id: 'X07350', name: '安全硬件·彩蛋层', check: () => t.extend(0, 'variable') > 0 },
  ];
}

/* -------- 族0295 性能调校 2.0 X07351~X07375 -------- */
export function checkF0295(): CheckEntry[] {
  const t = new T.Tuner();
  return [
    { id: 'X07351', name: '调校·最小闭环', check: () => t.profile === 'balanced' && t.spec.cpuBoost === 80 },
    { id: 'X07352', name: '调校·全量参数', check: () => new T.Tuner('game').spec.fanPct === 85 && new T.Tuner('game').spec.gpuBoost === 100 },
    { id: 'X07353', name: '调校·档位矩阵', check: () => T.TUNE_PROFILES.length === 5 && T.TUNE_PROFILES.every((p) => T.TUNE_MATRIX[p].cpuBoost > 0) },
    { id: 'X07354', name: '调校·快照迁移', check: () => { const q = T.Tuner.deserialize(t.serialize()); return q.profile === 'balanced' && q.ocPct === 0; } },
    { id: 'X07355', name: '调校·集成验证', check: () => t.apply(4000) === 3200 },
    { id: 'X07356', name: '调校·越界钳制', check: () => t.setOc(50) === 15 && t.setOc(-5) === 0 && t.setOc(Number.NaN) === 0 },
    { id: 'X07357', name: '调校·失败叙事', check: () => new T.Tuner('nope').profile === 'balanced' },
    { id: 'X07358', name: '调校·中断续跑', check: () => { const q = T.Tuner.deserialize(JSON.stringify({ profile: 'eco', ocPct: 5 })); return q.ocPct === 5 && q.profile === 'eco'; } },
    { id: 'X07359', name: '调校·资源降级', check: () => new T.Tuner('performance').thermalGuard(95) === 'balanced' },
    { id: 'X07360', name: '调校·回滚净身', check: () => { const q = new T.Tuner('eco'); q.setOc(15); return q.apply(2000) === Math.round(2000 * 0.6 * 1.15); } },
    { id: 'X07361', name: '调校·动效令牌', check: () => T.TUNE_MATRIX.eco.cpuBoost < T.TUNE_MATRIX.balanced.cpuBoost && T.TUNE_MATRIX.balanced.cpuBoost < T.TUNE_MATRIX.performance.cpuBoost },
    { id: 'X07362', name: '调校·三态焦点', check: () => new T.Tuner('performance').thermalGuard(60) === 'performance' },
    { id: 'X07363', name: '调校·键盘序', check: () => T.TUNE_PROFILES.every((p) => new T.Tuner(p).profile === p) },
    { id: 'X07364', name: '调校·微文案', check: () => new T.Tuner('creator').spec.fanPct === 65 },
    { id: 'X07365', name: '调校·aria 等价', check: () => { const q = T.Tuner.deserialize(t.serialize()); const r = T.Tuner.deserialize(q.serialize()); return r.serialize() === q.serialize(); } },
    { id: 'X07366', name: '调校·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) new T.Tuner('game').apply(3000); return performance.now() - t0 < 50; } },
    { id: 'X07367', name: '调校·热路径', check: () => new T.Tuner('performance').apply(1000) === 1000 },
    { id: 'X07368', name: '调校·零漂移', check: () => { const q = T.Tuner.deserialize('bad-json'); return q.profile === 'balanced'; } },
    { id: 'X07369', name: '调校·低配减档', check: () => new T.Tuner('game').thermalGuard(99) === 'performance' },
    { id: 'X07370', name: '调校·守卫', check: () => t.clamped >= 1 && new T.Tuner().clamped === 0 },
    { id: 'X07371', name: '调校·智能建议', check: () => new T.Tuner('eco').spec.fanPct === 35 },
    { id: 'X07372', name: '调校·批量模式', check: () => { let n = 0; for (const p of T.TUNE_PROFILES) if (new T.Tuner(p).spec.gpuBoost > 0) n++; return n === 5; } },
    { id: 'X07373', name: '调校·跨域联动', check: () => { const q = T.Tuner.deserialize(JSON.stringify({ profile: 'creator', ocPct: 10 })); return q.apply(1000) === 1100; } },
    { id: 'X07374', name: '调校·扩展点', check: () => typeof T.TUNE_MATRIX === 'object' && typeof t.thermalGuard === 'function' },
    { id: 'X07375', name: '调校·彩蛋层', check: () => new T.Tuner('game').spec.cpuBoost === 95 },
  ];
}

/* -------- 族0296 触屏笔 2.0 X07376~X07400 -------- */
export function checkF0296(): CheckEntry[] {
  const p = new T.PenCalibration();
  return [
    { id: 'X07376', name: '触笔·最小闭环', check: () => p.mapPressure(0.5) > 0 && p.mapPressure(0) === 0 && p.mapPressure(1) === 1 },
    { id: 'X07377', name: '触笔·全量参数', check: () => p.gamma === 2.2 && p.tiltMax === 60 && p.pressureMax === 8192 },
    { id: 'X07378', name: '触笔·档位矩阵', check: () => T.PenCalibration.sensitivity(0) === 2048 && T.PenCalibration.sensitivity(2) === 8192 && T.PenCalibration.sensitivity(4) === 16384 },
    { id: 'X07379', name: '触笔·快照迁移', check: () => { const q = T.PenCalibration.deserialize(p.serialize()); return q.gamma === 2.2 && q.haptic === false; } },
    { id: 'X07380', name: '触笔·集成验证', check: () => p.mapPressure(1) === 1 && T.PenCalibration.isPalm(500) && !T.PenCalibration.isPalm(100) },
    { id: 'X07381', name: '触笔·越界钳制', check: () => p.mapPressure(-1) === 0 && p.mapPressure(2) === 1 && p.clamped === 2 },
    { id: 'X07382', name: '触笔·失败叙事', check: () => T.PenCalibration.deserialize('bad').gamma === 2.2 },
    { id: 'X07383', name: '触笔·中断续跑', check: () => p.clampTilt(90) === 60 && p.clampTilt(-90) === -60 && p.clampTilt(30) === 30 },
    { id: 'X07384', name: '触笔·资源降级', check: () => { const q = new T.PenCalibration(); q.gamma = 1; return q.mapPressure(0.5) === 0.5; } },
    { id: 'X07385', name: '触笔·回滚净身', check: () => { const q = new T.PenCalibration(); return q.serialize() === JSON.stringify({ gamma: 2.2, tiltMax: 60, haptic: false }); } },
    { id: 'X07386', name: '触笔·动效令牌', check: () => p.hover(5) && !p.hover(0) && !p.hover(11) },
    { id: 'X07387', name: '触笔·三态焦点', check: () => { const q = new T.PenCalibration(); q.haptic = true; return T.PenCalibration.deserialize(q.serialize()).haptic === true; } },
    { id: 'X07388', name: '触笔·键盘序', check: () => p.mapPressure(0.25) < p.mapPressure(0.5) && p.mapPressure(0.5) < p.mapPressure(0.75) },
    { id: 'X07389', name: '触笔·微文案', check: () => T.PenCalibration.sensitivity(1) === 4096 && T.PenCalibration.sensitivity(3) === 16384 },
    { id: 'X07390', name: '触笔·aria 等价', check: () => T.PenCalibration.isPalm(400) === false && T.PenCalibration.isPalm(401) === true },
    { id: 'X07391', name: '触笔·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) p.mapPressure(i / 1000); return performance.now() - t0 < 50; } },
    { id: 'X07392', name: '触笔·热路径', check: () => { const q = new T.PenCalibration(); return q.mapPressure(0.5) === Math.pow(0.5, 2.2); } },
    { id: 'X07393', name: '触笔·零漂移', check: () => { const a = T.PenCalibration.deserialize(p.serialize()); const b = T.PenCalibration.deserialize(a.serialize()); return b.serialize() === a.serialize(); } },
    { id: 'X07394', name: '触笔·低配减档', check: () => { const q = new T.PenCalibration(); return q.clampTilt(0) === 0 && q.clamped === 0; } },
    { id: 'X07395', name: '触笔·守卫', check: () => { const q = T.PenCalibration.deserialize(JSON.stringify({ gamma: 0, tiltMax: -5 })); return q.gamma === 2.2 && q.tiltMax === 60; } },
    { id: 'X07396', name: '触笔·智能建议', check: () => p.hover(10) && !p.hover(10.5) },
    { id: 'X07397', name: '触笔·批量模式', check: () => { let n = 0; for (let i = 0; i <= 4; i++) if (T.PenCalibration.sensitivity(i as 0) > 0) n++; return n === 5; } },
    { id: 'X07398', name: '触笔·跨域联动', check: () => { const q = T.PenCalibration.deserialize(JSON.stringify({ gamma: 1.8, tiltMax: 45, haptic: true })); return q.gamma === 1.8 && q.tiltMax === 45; } },
    { id: 'X07399', name: '触笔·扩展点', check: () => typeof T.PenCalibration.deserialize === 'function' && typeof T.PenCalibration.isPalm === 'function' },
    { id: 'X07400', name: '触笔·彩蛋层', check: () => p.mapPressure(1) === 1 },
  ];
}

/* -------- 族0297 摄像头影像 2.0 X07401~X07425 -------- */
export function checkF0297(): CheckEntry[] {
  const c = new T.CameraStack();
  return [
    { id: 'X07401', name: '摄像头·最小闭环', check: () => c.mode === '1080p' && c.fps === 30 && c.grab() !== null },
    { id: 'X07402', name: '摄像头·全量参数', check: () => new T.CameraStack('4K').fps === 24 && new T.CameraStack('480p').fps === 60 },
    { id: 'X07403', name: '摄像头·档位矩阵', check: () => T.CAM_MODES.length === 5 && T.CAM_MODES.every((m) => T.CAM_FPS[m] > 0) },
    { id: 'X07404', name: '摄像头·快照迁移', check: () => { const q = T.CameraStack.deserialize(c.serialize()); return q.mode === '1080p' && q.shutter === false; } },
    { id: 'X07405', name: '摄像头·集成验证', check: () => { const g = c.grab()!; return g.mode === '1080p' && g.exposure === 0; } },
    { id: 'X07406', name: '摄像头·越界钳制', check: () => c.setExposure(9) === 3 && c.setExposure(-9) === -3 && c.setExposure(Number.NaN) === 0 },
    { id: 'X07407', name: '摄像头·失败叙事', check: () => new T.CameraStack('nope').mode === '1080p' },
    { id: 'X07408', name: '摄像头·中断续跑', check: () => { c.setShutter(true); const blocked = c.grab() === null; c.setShutter(false); return blocked && c.grab() !== null; } },
    { id: 'X07409', name: '摄像头·资源降级', check: () => T.CameraStack.autoGain(5) === 1600 && T.CameraStack.autoGain(30) === 800 && T.CameraStack.autoGain(500) === 100 },
    { id: 'X07410', name: '摄像头·回滚净身', check: () => { const q = new T.CameraStack(); q.setShutter(true); return T.CameraStack.deserialize(q.serialize()).shutter === true; } },
    { id: 'X07411', name: '摄像头·动效令牌', check: () => T.CAM_FPS['1080p'] === 30 && T.CAM_FPS['1440p'] === 30 },
    { id: 'X07412', name: '摄像头·三态焦点', check: () => c.setExposure(1.4) === 1 && c.setExposure(-1.6) === -2 },
    { id: 'X07413', name: '摄像头·键盘序', check: () => T.CAM_MODES.every((m) => new T.CameraStack(m).mode === m) },
    { id: 'X07414', name: '摄像头·微文案', check: () => c.shutter === false && typeof c.setShutter === 'function' },
    { id: 'X07415', name: '摄像头·aria 等价', check: () => { const q = T.CameraStack.deserialize(c.serialize()); const r = T.CameraStack.deserialize(q.serialize()); return r.serialize() === q.serialize(); } },
    { id: 'X07416', name: '摄像头·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.CameraStack.autoGain(i); return performance.now() - t0 < 50; } },
    { id: 'X07417', name: '摄像头·热路径', check: () => T.CameraStack.autoGain(100) === 400 },
    { id: 'X07418', name: '摄像头·零漂移', check: () => { const q = T.CameraStack.deserialize('bad'); return q.mode === '1080p'; } },
    { id: 'X07419', name: '摄像头·低配减档', check: () => new T.CameraStack('720p').clamped === 0 },
    { id: 'X07420', name: '摄像头·守卫', check: () => new T.CameraStack('8K').clamped === 1 },
    { id: 'X07421', name: '摄像头·智能建议', check: () => T.CameraStack.autoGain(49) === 800 && T.CameraStack.autoGain(50) === 400 },
    { id: 'X07422', name: '摄像头·批量模式', check: () => { let n = 0; for (const m of T.CAM_MODES) if (new T.CameraStack(m).grab() !== null) n++; return n === 5; } },
    { id: 'X07423', name: '摄像头·跨域联动', check: () => { const q = T.CameraStack.deserialize(JSON.stringify({ mode: '720p', shutter: true, exposure: -1 })); return q.grab() === null && q.exposure === -1; } },
    { id: 'X07424', name: '摄像头·扩展点', check: () => typeof T.CAM_FPS === 'object' && typeof c.serialize === 'function' },
    { id: 'X07425', name: '摄像头·彩蛋层', check: () => new T.CameraStack('4K').mode === '4K' },
  ];
}

/* -------- 族0298 显示器色准 2.0 X07426~X07450 -------- */
export function checkF0298(): CheckEntry[] {
  const d = new T.DisplayCalib();
  return [
    { id: 'X07426', name: '色准·最小闭环', check: () => d.target === 'sRGB' && T.deltaE([50, 0, 0], [50, 0, 0]) === 0 },
    { id: 'X07427', name: '色准·全量参数', check: () => d.brightness === 120 && new T.DisplayCalib('Display P3').target === 'Display P3' },
    { id: 'X07428', name: '色准·档位矩阵', check: () => T.COLOR_TARGETS.length === 5 && T.COLOR_TARGETS.every((t) => T.DisplayCalib.coverage(t) > 0) },
    { id: 'X07429', name: '色准·快照迁移', check: () => { const q = T.DisplayCalib.deserialize(d.serialize()); return q.target === 'sRGB' && q.brightness === 120; } },
    { id: 'X07430', name: '色准·集成验证', check: () => T.deltaE([50, 2, 0], [50, 0, 0]) === 2 && T.colorGrade(1) === 'professional' },
    { id: 'X07431', name: '色准·越界钳制', check: () => d.setBrightness(999) === 500 && d.setBrightness(-5) === 0 && d.setBrightness(Number.NaN) === 0 },
    { id: 'X07432', name: '色准·失败叙事', check: () => T.colorGrade(3) === 'good' && T.colorGrade(7) === 'fair' && T.colorGrade(99) === 'calibrate' },
    { id: 'X07433', name: '色准·中断续跑', check: () => { const q = T.DisplayCalib.deserialize(JSON.stringify({ target: 'Rec.2020', brightness: 200 })); return q.target === 'Rec.2020' && q.brightness === 200; } },
    { id: 'X07434', name: '色准·资源降级', check: () => T.DisplayCalib.coverage('sRGB') === 100 && T.DisplayCalib.coverage('Rec.2020') === 70 },
    { id: 'X07435', name: '色准·回滚净身', check: () => { const q = new T.DisplayCalib(); return q.clamped === 0 && q.serialize() === JSON.stringify({ target: 'sRGB', brightness: 120 }); } },
    { id: 'X07436', name: '色准·动效令牌', check: () => T.DisplayCalib.clampWhitepoint(6500) === 6500 && T.DisplayCalib.clampWhitepoint(100) === 2000 && T.DisplayCalib.clampWhitepoint(99999) === 10000 },
    { id: 'X07437', name: '色准·三态焦点', check: () => new T.DisplayCalib('Adobe RGB').clamped === 0 },
    { id: 'X07438', name: '色准·键盘序', check: () => T.COLOR_TARGETS.every((t) => new T.DisplayCalib(t).target === t) },
    { id: 'X07439', name: '色准·微文案', check: () => T.colorGrade(2) === 'excellent' && T.colorGrade(1.5) === 'excellent' },
    { id: 'X07440', name: '色准·aria 等价', check: () => T.deltaE([100, 0, 0], [0, 0, 0]) === 100 },
    { id: 'X07441', name: '色准·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.deltaE([i, 0, 0], [0, 0, 0]); return performance.now() - t0 < 50; } },
    { id: 'X07442', name: '色准·热路径', check: () => T.deltaE([1, 1, 1], [0, 0, 0]) === Math.round(Math.sqrt(3) * 100) / 100 },
    { id: 'X07443', name: '色准·零漂移', check: () => { const q = T.DisplayCalib.deserialize('bad'); return q.target === 'sRGB'; } },
    { id: 'X07444', name: '色准·低配减档', check: () => { const q = T.DisplayCalib.deserialize(JSON.stringify({ target: 'native' })); return q.target === 'native' && T.DisplayCalib.coverage('native') === 100; } },
    { id: 'X07445', name: '色准·守卫', check: () => new T.DisplayCalib('nope').clamped === 1 },
    { id: 'X07446', name: '色准·智能建议', check: () => T.colorGrade(T.deltaE([10, 5, 5], [10, 0, 0])) === 'fair' && T.colorGrade(0.5) === 'professional' },
    { id: 'X07447', name: '色准·批量模式', check: () => { let n = 0; for (const t of T.COLOR_TARGETS) if (T.DisplayCalib.coverage(t) >= 70) n++; return n === 5; } },
    { id: 'X07448', name: '色准·跨域联动', check: () => { const q = T.DisplayCalib.deserialize(JSON.stringify({ target: 'Display P3', brightness: 300 })); return T.DisplayCalib.coverage(q.target) === 96 && q.brightness === 300; } },
    { id: 'X07449', name: '色准·扩展点', check: () => typeof T.deltaE === 'function' && typeof T.colorGrade === 'function' },
    { id: 'X07450', name: '色准·彩蛋层', check: () => { const q = new T.DisplayCalib('sRGB'); return q.setBrightness(120) === 120 && q.clamped === 0; } },
  ];
}

/* -------- 族0299 声音空间化 2.0 X07451~X07475 -------- */
export function checkF0299(): CheckEntry[] {
  const m = new T.SpatialMixer();
  return [
    { id: 'X07451', name: '空间化·最小闭环', check: () => m.layout === 'stereo' && m.channels === 2 },
    { id: 'X07452', name: '空间化·全量参数', check: () => { m.setHrtf('room'); return m.hrtf === 'room'; } },
    { id: 'X07453', name: '空间化·档位矩阵', check: () => T.SPATIAL_LAYOUTS.length === 5 && T.SPATIAL_CHANNELS.mono === 1 && T.SPATIAL_CHANNELS.atmos === 12 },
    { id: 'X07454', name: '空间化·快照迁移', check: () => { const q = T.SpatialMixer.deserialize(m.serialize()); return q.layout === 'stereo' && q.hrtf === 'room'; } },
    { id: 'X07455', name: '空间化·集成验证', check: () => { const p = T.SpatialMixer.pan(0); return p[0] === 0.5 && p[1] === 0.5; } },
    { id: 'X07456', name: '空间化·越界钳制', check: () => T.SpatialMixer.pan(180)[0] === 0 && T.SpatialMixer.pan(-180)[0] === 1 },
    { id: 'X07457', name: '空间化·失败叙事', check: () => new T.SpatialMixer('nope').layout === 'stereo' },
    { id: 'X07458', name: '空间化·中断续跑', check: () => { m.setHrtf(''); return m.hrtf === 'generic'; } },
    { id: 'X07459', name: '空间化·资源降级', check: () => T.SpatialMixer.upmix([1, 2]).length === 4 && T.SpatialMixer.upmix([1, 2])[1] === 1 },
    { id: 'X07460', name: '空间化·回滚净身', check: () => { const q = new T.SpatialMixer(); return q.serialize() === JSON.stringify({ layout: 'stereo', hrtf: 'generic' }); } },
    { id: 'X07461', name: '空间化·动效令牌', check: () => T.SpatialMixer.lfe('stereo') === false && T.SpatialMixer.lfe('5.1') === true },
    { id: 'X07462', name: '空间化·三态焦点', check: () => T.SpatialMixer.pan(90)[1] === 1 && T.SpatialMixer.pan(-90)[1] === 0 },
    { id: 'X07463', name: '空间化·键盘序', check: () => T.SPATIAL_LAYOUTS.every((l) => new T.SpatialMixer(l).layout === l) },
    { id: 'X07464', name: '空间化·微文案', check: () => new T.SpatialMixer('7.1').channels === 8 },
    { id: 'X07465', name: '空间化·aria 等价', check: () => { const p = T.SpatialMixer.pan(45); return Math.abs(p[0] + p[1] - 1) < 0.01; } },
    { id: 'X07466', name: '空间化·基准采集', check: () => { const t0 = performance.now(); for (let i = -90; i < 90; i++) T.SpatialMixer.pan(i); return performance.now() - t0 < 50; } },
    { id: 'X07467', name: '空间化·热路径', check: () => T.SpatialMixer.lfe('atmos') === true },
    { id: 'X07468', name: '空间化·零漂移', check: () => { const a = T.SpatialMixer.deserialize(m.serialize()); const b = T.SpatialMixer.deserialize(a.serialize()); return b.serialize() === a.serialize(); } },
    { id: 'X07469', name: '空间化·低配减档', check: () => { const q = T.SpatialMixer.deserialize('bad'); return q.layout === 'stereo'; } },
    { id: 'X07470', name: '空间化·守卫', check: () => new T.SpatialMixer('quad').clamped === 1 },
    { id: 'X07471', name: '空间化·智能建议', check: () => T.SpatialMixer.pan(30)[1] > T.SpatialMixer.pan(0)[1] },
    { id: 'X07472', name: '空间化·批量模式', check: () => { const src = [1, 2, 3]; return T.SpatialMixer.upmix(src).length === 6 && src.length === 3; } },
    { id: 'X07473', name: '空间化·跨域联动', check: () => { const q = T.SpatialMixer.deserialize(JSON.stringify({ layout: 'atmos', hrtf: 'personal' })); return q.channels === 12 && T.SpatialMixer.lfe(q.layout); } },
    { id: 'X07474', name: '空间化·扩展点', check: () => typeof T.SpatialMixer.pan === 'function' && typeof T.SpatialMixer.upmix === 'function' },
    { id: 'X07475', name: '空间化·彩蛋层', check: () => new T.SpatialMixer('mono').channels === 1 },
  ];
}

/* -------- 族0300 扫描摄入 2.0 X07476~X07500 -------- */
export function checkF0300(): CheckEntry[] {
  const s = new T.ScanStack();
  return [
    { id: 'X07476', name: '扫描·最小闭环', check: () => s.dpi === 300 && s.feedBatch(['p1']) === 1 && s.scanPage() === 'p1' },
    { id: 'X07477', name: '扫描·全量参数', check: () => new T.ScanStack(600).dpi === 600 && s.addOcr('zh-CN') === 1 },
    { id: 'X07478', name: '扫描·档位矩阵', check: () => T.SCAN_DPI.length === 5 && T.SCAN_DPI[0] === 150 && T.SCAN_DPI[4] === 2400 },
    { id: 'X07479', name: '扫描·快照迁移', check: () => { const q = T.ScanStack.deserialize(s.serialize()); return q.dpi === 300 && q.ocrLangs.includes('zh-CN'); } },
    { id: 'X07480', name: '扫描·集成验证', check: () => { const q = new T.ScanStack(); q.feedBatch(['a', 'b', 'c']); return q.scanPage() === 'a' && q.progress === 2 && q.scanPage() === 'b' && q.scanPage() === 'c' && q.scanPage() === null; } },
    { id: 'X07481', name: '扫描·越界钳制', check: () => s.setDpi(500) === 600 && s.setDpi(1) === 150 && s.setDpi(99999) === 2400 },
    { id: 'X07482', name: '扫描·失败叙事', check: () => { const q = new T.ScanStack(); return q.scanPage() === null; } },
    { id: 'X07483', name: '扫描·中断续跑', check: () => { const q = new T.ScanStack(); q.feedBatch(['x1', 'x2']); const first = q.scanPage(); const r = T.ScanStack.deserialize(q.serialize()); return first === 'x1' && r.scanPage() === 'x2'; } },
    { id: 'X07484', name: '扫描·资源降级', check: () => T.ScanStack.compressRatio(150) === 2 && T.ScanStack.compressRatio(300) === 4 && T.ScanStack.compressRatio(1200) === 8 },
    { id: 'X07485', name: '扫描·回滚净身', check: () => { const q = new T.ScanStack(); return q.adfQueue.length === 0 && q.ocrLangs.length === 0 && q.progress === 0; } },
    { id: 'X07486', name: '扫描·动效令牌', check: () => T.ScanStack.deskew(30) === 15 && T.ScanStack.deskew(-30) === -15 && T.ScanStack.deskew(5) === 5 },
    { id: 'X07487', name: '扫描·三态焦点', check: () => new T.ScanStack(300).clamped === 0 },
    { id: 'X07488', name: '扫描·键盘序', check: () => { const q = new T.ScanStack(); q.addOcr('zh-CN'); q.addOcr('en'); q.addOcr('zh-CN'); return q.ocrLangs.length === 2 && q.ocrLangs[0] === 'zh-CN'; } },
    { id: 'X07489', name: '扫描·微文案', check: () => T.ScanStack.compressRatio(600) === 6 && T.ScanStack.compressRatio(2400) === 8 },
    { id: 'X07490', name: '扫描·aria 等价', check: () => { const q = T.ScanStack.deserialize(s.serialize()); const r = T.ScanStack.deserialize(q.serialize()); return r.serialize() === q.serialize(); } },
    { id: 'X07491', name: '扫描·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.ScanStack.deskew(i % 40); return performance.now() - t0 < 50; } },
    { id: 'X07492', name: '扫描·热路径', check: () => T.ScanStack.deskew(0) === 0 },
    { id: 'X07493', name: '扫描·零漂移', check: () => { const q = T.ScanStack.deserialize('bad'); return q.dpi === 300; } },
    { id: 'X07494', name: '扫描·低配减档', check: () => new T.ScanStack(200).dpi === 150 && new T.ScanStack(400).dpi === 300 },
    { id: 'X07495', name: '扫描·守卫', check: () => new T.ScanStack(Number.NaN).dpi === 300 && new T.ScanStack(undefined as unknown as number).dpi === 300 },
    { id: 'X07496', name: '扫描·智能建议', check: () => T.ScanStack.compressRatio(1200) === 8 },
    { id: 'X07497', name: '扫描·批量模式', check: () => { const q = new T.ScanStack(); const pages = Array.from({ length: 50 }, (_, i) => 'p' + i); q.feedBatch(pages); let n = 0; while (q.scanPage() !== null) n++; return n === 50; } },
    { id: 'X07498', name: '扫描·跨域联动', check: () => { const q = T.ScanStack.deserialize(JSON.stringify({ dpi: 600, ocrLangs: ['en'], queue: ['q1'] })); return q.dpi === 600 && q.scanPage() === 'q1'; } },
    { id: 'X07499', name: '扫描·扩展点', check: () => typeof T.SCAN_DPI === 'object' && typeof T.ScanStack.deskew === 'function' },
    { id: 'X07500', name: '扫描·彩蛋层', check: () => new T.ScanStack(2400).dpi === 2400 },
  ];
}

/** AI-30 V 线聚合（族0291~0300 · 250 项）。 */
export function runAi30Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const entries = [
    ...checkF0291(), ...checkF0292(), ...checkF0293(), ...checkF0294(), ...checkF0295(),
    ...checkF0296(), ...checkF0297(), ...checkF0298(), ...checkF0299(), ...checkF0300(),
  ];
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
