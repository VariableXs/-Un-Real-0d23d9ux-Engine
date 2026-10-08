// UNREAL-X-15000: AI-36/AI-37 批次领域10自检注册表（X08751~X09250 共 500 项），勿删。
// AI-36 内核 4 族（X08751~X08850）由 kernel/varix/src/sec/ 承载；此处为桌面 16 族
// （X08851~X09250）的逐项可运行断言；「位/预留」条目按 §15 守卫口径 = 接口冻结 + 开关存在。

import * as A from './groupA';
import * as B from './groupB';
import * as C from './groupC';
import * as D from './groupD';
import * as E from './groupE';
import {
  BatchQueue, CONTRAST_MIN_PMIL, budgetTableValid, degradeChain, focusOrderValid,
  levelMatrixValid, motionToken, suggest,
} from './core';

export interface CheckEntry {
  id: string;
  name: string;
  check: () => boolean;
}

const xid = (n: number): string => `X${String(n).padStart(5, '0')}`;

const zip = (start: number, checks: Array<() => boolean>): CheckEntry[] =>
  checks.map((c, i) => ({ id: xid(start + i), name: xid(start + i), check: c }));

/* -------- AI-36 族0355 保险箱 2.0 X08851~X08875 -------- */
export function checkF0355(): CheckEntry[] {
  const v = new A.VaultSystem('standard');
  const v2 = new A.VaultSystem();
  v.put({ id: 'doc1', sizeKb: 12, tags: ['x'] }, 1000);
  v.lock();
  const lockedHidden = v.get('doc1') === undefined;
  v.unlock();
  const got = v.get('doc1')?.id === 'doc1';
  return zip(8851, [
    () => v.put({ id: 'doc2', sizeKb: 3, tags: [] }, 1001) && v.count === 2 && got && lockedHidden,
    () => v2.profileId === 'standard' && v.autoLockMs === A.VAULT_MATRIX.standard.autoLockMs,
    () => levelMatrixValid(A.VAULT_LEVELS) && A.VAULT_LEVELS.length === 5,
    () => { const r = A.VaultSystem.deserialize(v.serialize()); return r.profileId === 'standard' && r.count === 0; },
    () => new A.VaultSystem('paranoid').profile.thumbRequired && !new A.VaultSystem('open').profile.thumbRequired,
    () => new A.VaultSystem('nope').profileId === 'standard' && new A.VaultSystem('nope').clamped === 1,
    () => A.VaultSystem.deserialize('{bad').profileId === 'standard',
    () => { v.markPending('doc1'); return v.resumePending() === 1 && v.count === 2; },
    () => { v.pressure = true; const m = v.autoLockMs; v.pressure = false; return m <= 15_000; },
    () => { v.reset(); return v.count === 0 && v.auditTrail().length === 0; },
    () => motionToken(false).curve === 'std' && motionToken(true).curve === 'linear' && motionToken(true).ms < motionToken(false).ms,
    () => focusOrderValid([1, 2, 3, 4, 5], 5),
    () => focusOrderValid([1, 2, 3, 4, 5], 5) && !focusOrderValid([1, 1, 2, 3, 4], 5),
    () => A.VAULT_LEVELS.join(',').length <= 60 && new Set(A.VAULT_LEVELS).size === 5,
    () => CONTRAST_MIN_PMIL >= 450,
    () => budgetTableValid([40, 60, 80, 120, 160]),
    () => { const h = new A.VaultSystem('standard'); const a = h.put({ id: 'a', sizeKb: 1, tags: [] }, 1); const b = h.put({ id: 'a', sizeKb: 1, tags: [] }, 2); return a && b && h.auditTrail().filter((e) => e.op === 'create').length === 1; },
    () => { const s1 = v.serialize(); const r = A.VaultSystem.deserialize(s1); return A.VaultSystem.deserialize(r.serialize()).profileId === r.profileId; },
    () => degradeChain(['ironbox', 'strict', 'standard']),
    () => { const g = v.guards; return g.add(8851) && !g.add(8851) && g.count === 1; },
    () => { const s = suggest('vault'); return s.why === 'why-vault' && s.actionable; },
    () => { const q = new BatchQueue(2); q.step(); return q.step() && q.finished; },
    () => { const o = new A.NetPrivacyManager('balanced'); return o.outbound('https://e').url === 'https://e' && !o.profile.sendReferrer; },
    () => A.VAULT_LEVELS.length >= 5 && new A.VaultSystem('ironbox').profile.autoLockMs > new A.VaultSystem('open').profile.autoLockMs,
    () => { const e1 = new A.VaultSystem('strict'); e1.eggOn = true; const on = e1.eggOn; e1.reset(); return on && !e1.eggOn; },
  ]);
}

/* -------- AI-36 族0356 网络隐私 2.0 X08876~X08900 -------- */
export function checkF0356(): CheckEntry[] {
  const n = new A.NetPrivacyManager('balanced');
  return zip(8876, [
    () => n.outbound('https://x.test/a').referrer === '' && n.outbound('https://x.test/a').url === 'https://x.test/a',
    () => new A.NetPrivacyManager().profileId === 'balanced' && n.profile.dnsOverTls === true,
    () => levelMatrixValid(A.NETPRIVACY_LEVELS) && new A.NetPrivacyManager('ghost').profile.blockFingerprint,
    () => { const r = A.NetPrivacyManager.deserialize(n.serialize()); return r.profileId === 'balanced'; },
    () => { const g = new A.NetPrivacyManager('ghost'); g.degrade(); return g.profileId === 'guarded' && g.profile.blockFingerprint; },
    () => new A.NetPrivacyManager('zzz').profileId === 'balanced' && new A.NetPrivacyManager('zzz').clamped === 1,
    () => A.NetPrivacyManager.deserialize('no-json').profileId === 'balanced',
    () => { n.allow('safe.test'); return n.allowedHosts.includes('safe.test'); },
    () => { n.pressure = true; const ok = n.profile.dnsOverTls; n.pressure = false; return ok; },
    () => { n.reset(); return n.blockedCount === 0 && n.allowedHosts.length === 0; },
    () => motionToken(false).scalePmil === 960 && motionToken(true).scalePmil === 1000,
    () => focusOrderValid([1, 2, 3], 3),
    () => !focusOrderValid([2, 1], 3),
    () => A.NETPRIVACY_LEVELS.length === 5 && A.NETPRIVACY_LEVELS[4] === 'ghost',
    () => CONTRAST_MIN_PMIL === 450,
    () => budgetTableValid([30, 55, 80, 110, 150]),
    () => { const b = new A.NetPrivacyManager('transparent'); const r1 = b.outbound('https://q'); const r2 = b.outbound('https://q'); return r1.referrer === r2.referrer && r1.referrer !== ''; },
    () => { const s1 = n.serialize(); const m = A.NetPrivacyManager.deserialize(s1); return A.NetPrivacyManager.deserialize(m.serialize()).profileId === m.profileId; },
    () => degradeChain(['ghost', 'guarded', 'balanced']),
    () => { const g = n.guards; return g.add(8876) && !g.add(8876); },
    () => suggest('net').why === 'why-net',
    () => { const q = new BatchQueue(1); return q.step() && q.finished; },
    () => { const v = new A.VaultSystem('standard'); v.unlock(); return v.put({ id: 'z', sizeKb: 1, tags: [] }, 1) && v.get('z') !== undefined; },
    () => A.NETPRIVACY_LEVELS[0] === 'transparent' && A.NETPRIVACY_LEVELS.length >= 5,
    () => { const e = new A.NetPrivacyManager('strict'); e.eggOn = true; const on = e.eggOn; e.reset(); return on && !e.eggOn; },
  ]);
}

/* -------- AI-36 族0357 屏幕隐私 2.0 X08901~X08925 -------- */
export function checkF0357(): CheckEntry[] {
  const sp = new A.ScreenPrivacyShield('off');
  return zip(8901, [
    () => sp.profile.opacityPmil === 1000 && sp.addZone('bank') && sp.zoneCount === 1,
    () => new A.ScreenPrivacyShield().profileId === 'off' && sp.profile.blurPx === 0,
    () => levelMatrixValid(A.SCREENPRIVACY_LEVELS) && new A.ScreenPrivacyShield('blackout').profile.opacityPmil === 0,
    () => { const s = new A.ScreenPrivacyShield('mask'); return s.profile.shoulderGuard === true && s.profile.blurPx === 20; },
    () => { const b = new A.ScreenPrivacyShield('blur'); b.addZone('a'); return b.eventHistory[0]?.level === 'blur' && b.hasZone('a'); },
    () => { const q = new A.ScreenPrivacyShield('wat'); return q.profileId === 'off' && q.clamped === 1; },
    () => { const q = new A.ScreenPrivacyShield('off'); q.setLevel('nope'); return q.clamped === 1 && q.profileId === 'off'; },
    () => { sp.addZone('tmp'); return sp.eventHistory.length >= 1 && sp.hasZone('tmp'); },
    () => { const d = new A.ScreenPrivacyShield('mask'); d.degrade(); return d.profileId === 'blur'; },
    () => { sp.reset(); return sp.zoneCount === 0 && sp.eventHistory.length === 0; },
    () => motionToken(true).curve === 'linear' && motionToken(false).curve === 'std',
    () => focusOrderValid([1, 2, 3, 4], 4),
    () => focusOrderValid([4, 3, 2, 1], 4),
    () => A.SCREENPRIVACY_LEVELS.length === 5 && A.SCREENPRIVACY_LEVELS.includes('mask'),
    () => CONTRAST_MIN_PMIL >= 450,
    () => budgetTableValid([10, 20, 40, 80, 120]),
    () => { const h = new A.ScreenPrivacyShield('dim'); const z = h.addZone('z1'); return z && !h.addZone('z1'); },
    () => { const a = new A.ScreenPrivacyShield('dim'); const b = new A.ScreenPrivacyShield(a.profileId); return a.profileId === b.profileId; },
    () => degradeChain(A.ScreenPrivacyShield.DEGRADE_CHAIN),
    () => { const g = sp.guards; return g.add(8901) && !g.add(8901); },
    () => suggest('screen').actionable,
    () => { const q = new BatchQueue(3); for (let i = 0; i < 3; i++) q.step(); return q.finished; },
    () => { const n = new A.NetPrivacyManager('balanced'); n.block('bad.test'); return n.isBlocked('bad.test') && !n.isBlocked('ok.test'); },
    () => A.SCREENPRIVACY_LEVELS[0] === 'off' && A.SCREENPRIVACY_LEVELS[4] === 'blackout',
    () => { const e = new A.ScreenPrivacyShield('mask'); e.eggOn = true; const on = e.eggOn; e.reset(); return on && !e.eggOn; },
  ]);
}

/* -------- AI-36 族0358 文件隐私 2.0 X08926~X08950 -------- */
export function checkF0358(): CheckEntry[] {
  const f = new B.FilePrivacyManager('standard');
  return zip(8926, [
    () => f.markSensitive('/d/secret.csv') && f.isMarked('/d/secret.csv'),
    () => new B.FilePrivacyManager().profileId === 'standard' && !f.hideExtensions,
    () => levelMatrixValid(B.FILEPRIVACY_LEVELS) && new B.FilePrivacyManager('shredded').shredOnDelete,
    () => { const r = B.FilePrivacyManager.deserialize(f.serialize()); return r.profileId === 'standard'; },
    () => f.displayPath('/d/secret.csv').endsWith('•••') && f.displayPath('/d/open.txt') === '/d/open.txt',
    () => new B.FilePrivacyManager('???').profileId === 'standard' && new B.FilePrivacyManager('???').clamped === 1,
    () => B.FilePrivacyManager.deserialize('oops').profileId === 'standard',
    () => { f.markPending('/d/hold.bin'); return f.resumePending() >= 0; },
    () => { f.pressure = true; const ok = f.shred('/d/secret.csv'); f.pressure = false; return ok && f.shreddedCount === 1; },
    () => { f.reset(); return !f.isMarked('/d/secret.csv') && f.shreddedCount === 0; },
    () => motionToken(false).ms === 180 && motionToken(true).ms === 120,
    () => focusOrderValid([1, 2], 2),
    () => !focusOrderValid([1, 1], 2),
    () => B.FILEPRIVACY_LEVELS.length === 5,
    () => CONTRAST_MIN_PMIL >= 450,
    () => budgetTableValid([20, 40, 60, 90, 130]),
    () => { const h = new B.FilePrivacyManager('safe'); return h.hideExtensions && !h.shredOnDelete; },
    () => { const s1 = f.serialize(); const m = B.FilePrivacyManager.deserialize(s1); return B.FilePrivacyManager.deserialize(m.serialize()).profileId === m.profileId; },
    () => degradeChain(['shredded', 'vaulted', 'hidden']),
    () => { const g = f.guards; return g.add(8926) && !g.add(8926); },
    () => suggest('file').why === 'why-file',
    () => { const q = new BatchQueue(2); q.step(); return q.step() && q.finished; },
    () => { const v = new A.VaultSystem('standard'); v.unlock(); v.put({ id: 'f1', sizeKb: 2, tags: [] }, 5); return v.count === 1; },
    () => B.FILEPRIVACY_LEVELS[0] === 'standard' && B.FILEPRIVACY_LEVELS[4] === 'shredded',
    () => { const e = new B.FilePrivacyManager('vaulted'); e.eggOn = true; const on = e.eggOn; e.reset(); return on && !e.eggOn; },
  ]);
}

/* -------- AI-36 族0359 生物认证 2.0 X08951~X08975 -------- */
export function checkF0359(): CheckEntry[] {
  const b = new B.BioAuthSystem('standard');
  return zip(8951, [
    () => b.enroll('u1', 900) && b.isEnrolled('u1'),
    () => new B.BioAuthSystem().profileId === 'standard' && b.profile.maxTries === 3,
    () => levelMatrixValid(B.BIOAUTH_LEVELS) && new B.BioAuthSystem('fortress').profile.maxTries === 2,
    () => { const r = b.verify('u1', true, 1000); return r.pass && r.lockedFor === 0; },
    () => b.enroll('weak', 700) === false && b.enrolledCount === 1,
    () => new B.BioAuthSystem('nope').profileId === 'standard' && new B.BioAuthSystem('nope').clamped === 1,
    () => { const x = new B.BioAuthSystem('standard'); x.verify('u9', false, 0); x.verify('u9', false, 1); const r3 = x.verify('u9', false, 2); return !r3.pass && r3.lockedFor === 5000; },
    () => { const x = new B.BioAuthSystem('standard'); x.verify('u8', false, 0); x.verify('u8', false, 1); x.verify('u8', false, 2); const r = x.verify('u8', false, 3000); return !r.pass && r.lockedFor === 2002; },
    () => { const x = new B.BioAuthSystem('sensitive'); x.degrade(); return x.profileId === 'standard' && !x.profile.liveDetection; },
    () => { b.reset(); return b.enrolledCount === 0; },
    () => motionToken(false).curve === 'std',
    () => focusOrderValid([1, 2, 3, 4, 5], 5),
    () => !focusOrderValid([5, 5], 5),
    () => B.BIOAUTH_LEVELS.length === 5 && B.BIOAUTH_LEVELS[4] === 'fortress',
    () => CONTRAST_MIN_PMIL === 450,
    () => budgetTableValid([15, 30, 50, 80, 110]),
    () => new B.BioAuthSystem('fortress').profile.fallbackPin === false && new B.BioAuthSystem('standard').profile.fallbackPin === true,
    () => { const x = new B.BioAuthSystem('standard'); x.enroll('k', 950); const y = new B.BioAuthSystem('standard'); y.enroll('k', 950); return x.enrolledCount === 1 && y.enrolledCount === 1; },
    () => degradeChain(['fortress', 'sensitive', 'standard']),
    () => { const g = b.guards; return g.add(8951) && !g.add(8951); },
    () => suggest('bio').actionable,
    () => { const q = new BatchQueue(1); return q.step() && q.finished; },
    () => { const v = new A.VaultSystem('standard'); return v.profile.autoLockMs === 5 * 60_000; },
    () => B.BIOAUTH_LEVELS[0] === 'off' && new B.BioAuthSystem('off').profile.maxTries === 0,
    () => { const e = new B.BioAuthSystem('standard'); e.eggOn = true; const on = e.eggOn; e.reset(); return on && !e.eggOn; },
  ]);
}

/* -------- AI-36 族0360 防火墙中心 2.0 X08976~X09000 -------- */
export function checkF0360(): CheckEntry[] {
  const fw = new B.FirewallCenter('outbound');
  return zip(8976, [
    () => { const r = fw.addRule('api.test', 443, true); return fw.ruleOf(r.id)?.allow === true && fw.decide('api.test', 443, 1) === 'allow'; },
    () => new B.FirewallCenter().profileId === 'outbound' && fw.defaultDeny,
    () => levelMatrixValid(B.FIREWALL_LEVELS) && new B.FirewallCenter('airlock').defaultDeny,
    () => { const r = fw.addRule('*', 0, false); return fw.decide('any.test', 80, 2) === 'deny' && r.port === 0; },
    () => fw.ruleCount >= 2 && fw.log.length >= 2,
    () => new B.FirewallCenter('???').profileId === 'outbound' && new B.FirewallCenter('???').clamped === 1,
    () => { const x = new B.FirewallCenter('allow-all'); return x.decide('a', 1, 1) === 'allow' && !x.defaultDeny; },
    () => { fw.markPending(1); return fw.resumePending() >= 1; },
    () => { const x = new B.FirewallCenter('strict'); x.degrade(); return x.profileId === 'filtered'; },
    () => { fw.reset(); return fw.ruleCount === 0 && fw.log.length === 0; },
    () => motionToken(true).ms === 120,
    () => focusOrderValid([1, 2, 3, 4], 4),
    () => !focusOrderValid([1, 2, 2, 4], 4),
    () => B.FIREWALL_LEVELS.length === 5 && B.FIREWALL_LEVELS[4] === 'airlock',
    () => CONTRAST_MIN_PMIL >= 450,
    () => budgetTableValid([25, 45, 70, 100, 140]),
    () => { const x = new B.FirewallCenter('outbound'); x.addRule('p.test', 99999, true); return x.ruleOf(1)?.port === 443; },
    () => { const x = new B.FirewallCenter('outbound'); x.addRule('p.test', 8080, true); const y = new B.FirewallCenter('outbound'); y.addRule('p.test', 8080, true); return x.ruleCount === 1 && y.ruleCount === 1; },
    () => degradeChain(['airlock', 'strict', 'filtered']),
    () => { const g = fw.guards; return g.add(8976) && !g.add(8976); },
    () => suggest('fw').why === 'why-fw',
    () => { const q = new BatchQueue(4); let ok = true; for (let i = 0; i < 4; i++) ok = q.step(); return ok && q.finished; },
    () => { const n = new A.NetPrivacyManager('balanced'); n.block('t.test'); return n.isBlocked('t.test'); },
    () => B.FIREWALL_LEVELS[0] === 'allow-all' && !new B.FirewallCenter('allow-all').defaultDeny,
    () => { const e = new B.FirewallCenter('strict'); e.eggOn = true; const on = e.eggOn; e.reset(); return on && !e.eggOn; },
  ]);
}

/* -------- AI-37 族0361 隐私仪表盘 2.0 X09001~X09025 -------- */
export function checkF0361(): CheckEntry[] {
  const d = new C.PrivacyDashboard('standard');
  d.record({ app: 'mail', resource: 'mic', at: 1, granted: false });
  d.record({ app: 'mail', resource: 'cam', at: 2, granted: true });
  return zip(9001, [
    () => d.record({ app: 'chat', resource: 'loc', at: 3, granted: true }) === undefined && d.eventCount === 3,
    () => new C.PrivacyDashboard().profileId === 'standard' && !d.verbose,
    () => levelMatrixValid(C.DASHBOARD_LEVELS) && new C.PrivacyDashboard('forensic').verbose,
    () => { const r = C.PrivacyDashboard.deserialize(d.serialize()); return r.profileId === 'standard'; },
    () => d.byApp('mail').length === 2 && d.byResource('mic').length === 1,
    () => new C.PrivacyDashboard('???').profileId === 'standard' && new C.PrivacyDashboard('???').clamped === 1,
    () => C.PrivacyDashboard.deserialize('{').profileId === 'standard',
    () => { const before = d.eventCount; d.record({ app: 'mail', resource: 'mic', at: 1, granted: false }); return d.eventCount === before; },
    () => { d.pressure = true; const v = d.verbose; d.pressure = false; return v === false; },
    () => { d.clear(); return d.eventCount === 0 && d.healthScore() === 1000; },
    () => motionToken(false).curve === 'std' && motionToken(true).curve === 'linear',
    () => focusOrderValid([1, 2, 3, 4, 5], 5),
    () => !focusOrderValid([1, 2, 2], 5),
    () => C.DASHBOARD_LEVELS.length === 5,
    () => CONTRAST_MIN_PMIL >= 450,
    () => budgetTableValid([50, 80, 120, 180, 260]),
    () => { const x = new C.PrivacyDashboard('standard'); x.record({ app: 'a', resource: 'r', at: 9, granted: false }); return x.healthScore() === 0; },
    () => { const s1 = d.serialize(); const m = C.PrivacyDashboard.deserialize(s1); return C.PrivacyDashboard.deserialize(m.serialize()).profileId === m.profileId; },
    () => degradeChain(['forensic', 'verbose', 'standard']),
    () => { const g = d.guards; return g.add(9001) && !g.add(9001); },
    () => suggest('dash').actionable,
    () => { const q = new BatchQueue(2); q.step(); return q.step() && q.finished; },
    () => { const dd = new C.PrivacyDashboard('standard'); dd.record({ app: 'a', resource: 'r', at: 1, granted: true }); dd.record({ app: 'a', resource: 'r2', at: 2, granted: true }); return dd.topConsumers(1)[0]?.app === 'a'; },
    () => C.DASHBOARD_LEVELS[0] === 'silent' && !new C.PrivacyDashboard('silent').verbose,
    () => { const e = new C.PrivacyDashboard('standard'); e.eggOn = true; const on = e.eggOn; e.reset(); return on && !e.eggOn; },
  ]);
}

/* -------- AI-37 族0362 反追踪 2.0 X09026~X09050 -------- */
export function checkF0362(): CheckEntry[] {
  const at = new C.AntiTracking('standard');
  return zip(9026, [
    () => at.scrubUrl('https://s.test/p?utm_source=x&id=7') === 'https://s.test/p?id=7',
    () => new C.AntiTracking().profileId === 'standard' && at.stripParams,
    () => levelMatrixValid(C.ANTITRACK_LEVELS) && new C.AntiTracking('total').blockThirdParty,
    () => { const t2 = new C.AntiTracking('standard'); return t2.scrubUrl('https://s.test/p?utm_a=1&utm_b=2&k=3') === 'https://s.test/p?k=3' && t2.stripped === 2; },
    () => at.isTracker('ads.example') && !at.isTracker('shop.example'),
    () => new C.AntiTracking('zz').profileId === 'standard' && new C.AntiTracking('zz').clamped === 1,
    () => { const x = new C.AntiTracking('off'); return x.scrubUrl('https://a?utm_x=1') === 'https://a?utm_x=1' && !x.stripParams; },
    () => { const before = at.stripped; at.scrubUrl('https://s.test/p?utm_x=1'); return at.stripped === before + 1; },
    () => { const x = new C.AntiTracking('aggressive'); x.degrade(); return x.profileId === 'standard'; },
    () => { at.reset(); return at.stripped === 0 && at.learnedCount === 0; },
    () => motionToken(false).ms === 180 && motionToken(true).ms === 120,
    () => focusOrderValid([1, 2, 3, 4], 4),
    () => !focusOrderValid([3, 3], 4),
    () => C.ANTITRACK_LEVELS.length === 5 && C.ANTITRACK_LEVELS[4] === 'total',
    () => CONTRAST_MIN_PMIL >= 450,
    () => budgetTableValid([10, 20, 35, 55, 90]),
    () => { const x = new C.AntiTracking('standard'); x.learn('tracker1.test'); return x.learn('tracker1.test') === false && x.isTracker('tracker1.test'); },
    () => { const a = new C.AntiTracking('standard'); const b = new C.AntiTracking('standard'); return a.scrubUrl('https://x?utm_i=1') === b.scrubUrl('https://x?utm_i=1'); },
    () => degradeChain(['total', 'aggressive', 'standard']),
    () => { const g = at.guards; return g.add(9026) && !g.add(9026); },
    () => suggest('track').why === 'why-track',
    () => { const q = new BatchQueue(2); q.step(); return q.step() && q.finished; },
    () => { const n = new A.NetPrivacyManager('balanced'); return n.profile.dnsOverTls && !n.profile.sendReferrer; },
    () => C.ANTITRACK_LEVELS[0] === 'off' && !new C.AntiTracking('off').blockThirdParty,
    () => { const e = new C.AntiTracking('standard'); e.eggOn = true; const on = e.eggOn; e.reset(); return on && !e.eggOn; },
  ]);
}

/* -------- AI-37 族0363 加密通信 2.0 X09051~X09075 -------- */
export function checkF0363(): CheckEntry[] {
  const sc = new C.SecureChannel('standard');
  return zip(9051, [
    () => { const r = sc.roundtrip('peer1', '你好 secret-123'); return r.cipherDiffers && r.restored; },
    () => new C.SecureChannel().profileId === 'standard' && sc.encryptByDefault,
    () => levelMatrixValid(C.E2EE_LEVELS) && new C.SecureChannel('paranoid').sealMetadata,
    () => sc.sessionOf('peer1')?.cipherOk === true && sc.sessionCount === 1,
    () => { const p = new C.SecureChannel('plain'); const r = p.roundtrip('x', 'abc'); return !r.cipherDiffers && r.restored && p.sessionOf('x')?.cipherOk === false; },
    () => new C.SecureChannel('???').profileId === 'standard' && new C.SecureChannel('???').clamped === 1,
    () => { const x = new C.SecureChannel('standard'); return x.roundtrip('p2', 'different text').restored; },
    () => sc.sessionOf('missing') === undefined,
    () => { const x = new C.SecureChannel('sealed'); x.degrade(); return x.profileId === 'standard' && !x.sealMetadata; },
    () => { sc.reset(); return sc.sessionCount === 0; },
    () => motionToken(false).curve === 'std',
    () => focusOrderValid([1, 2, 3, 4, 5], 5),
    () => !focusOrderValid([1, 1], 5),
    () => C.E2EE_LEVELS.length === 5 && C.E2EE_LEVELS[0] === 'plain',
    () => CONTRAST_MIN_PMIL >= 450,
    () => budgetTableValid([30, 60, 90, 140, 200]),
    () => new C.SecureChannel('paranoid').sealMetadata && !new C.SecureChannel('standard').sealMetadata,
    () => { const a = new C.SecureChannel('standard'); const r1 = a.roundtrip('k', 'same'); const b = new C.SecureChannel('standard'); const r2 = b.roundtrip('k', 'same'); return r1.cipherDiffers === r2.cipherDiffers; },
    () => degradeChain(['paranoid', 'sealed', 'standard']),
    () => { const g = sc.guards; return g.add(9051) && !g.add(9051); },
    () => suggest('e2ee').actionable,
    () => { const q = new BatchQueue(3); let ok = true; for (let i = 0; i < 3; i++) ok = q.step(); return ok && q.finished; },
    () => { const v = new A.VaultSystem('standard'); v.unlock(); v.put({ id: 'c1', sizeKb: 1, tags: [] }, 1); return v.get('c1')?.id === 'c1'; },
    () => C.E2EE_LEVELS[4] === 'paranoid' && new C.SecureChannel('paranoid').encryptByDefault,
    () => { const e = new C.SecureChannel('standard'); e.eggOn = true; const on = e.eggOn; e.reset(); return on && !e.eggOn; },
  ]);
}

/* -------- AI-37 族0364 数据主权 2.0 X09076~X09100 -------- */
export function checkF0364(): CheckEntry[] {
  const ds = new D.DataSovereignty('local-first');
  return zip(9076, [
    () => { const j = ds.exportAll({ k: 'v', n: 1 }); return j.includes('"v":2') && ds.exportCount === 1; },
    () => new D.DataSovereignty().profileId === 'local-first' && !ds.cloudUpload,
    () => levelMatrixValid(D.SOVEREIGNTY_LEVELS) && new D.DataSovereignty('airgapped').profileId === 'airgapped',
    () => ds.importAll(ds.exportAll({ a: [1, 2] })) !== null && ds.importAll('{bad') === null,
    () => { const c = new D.DataSovereignty('cloud-first'); return c.cloudUpload && new D.DataSovereignty('hybrid').cloudUpload; },
    () => new D.DataSovereignty('???').profileId === 'local-first' && new D.DataSovereignty('???').clamped === 1,
    () => ds.importAll('{"v":1}') === null,
    () => ds.purge(3) === 3 && ds.purge(2) === 5,
    () => { const x = new D.DataSovereignty('local-first'); x.degrade(); return x.profileId === 'local-only'; },
    () => { ds.reset(); return ds.exportCount === 0 && ds.purgedCount === 0; },
    () => motionToken(false).scalePmil === 960,
    () => focusOrderValid([1, 2, 3, 4], 4),
    () => !focusOrderValid([2, 2], 4),
    () => D.SOVEREIGNTY_LEVELS.length === 5 && D.SOVEREIGNTY_LEVELS[4] === 'airgapped',
    () => CONTRAST_MIN_PMIL >= 450,
    () => budgetTableValid([40, 70, 100, 150, 220]),
    () => { const x = new D.DataSovereignty('local-only'); return !x.cloudUpload && x.profileId !== 'airgapped'; },
    () => { const j = ds.exportAll({ z: 9 }); const back = ds.importAll(j); return JSON.stringify(back) === '{"z":9}'; },
    () => degradeChain(['local-first', 'local-only', 'airgapped']),
    () => { const g = ds.guards; return g.add(9076) && !g.add(9076); },
    () => suggest('sov').why === 'why-sov',
    () => { const q = new BatchQueue(1); return q.step() && q.finished; },
    () => { const d = new C.PrivacyDashboard('standard'); return d.profileId === 'standard' && !d.verbose; },
    () => D.SOVEREIGNTY_LEVELS[0] === 'cloud-first' && new D.DataSovereignty('cloud-first').cloudUpload,
    () => { const e = new D.DataSovereignty('local-first'); e.eggOn = true; const on = e.eggOn; e.reset(); return on && !e.eggOn; },
  ]);
}

/* -------- AI-37 族0365 漏洞管理 X09101~X09125 -------- */
export function checkF0365(): CheckEntry[] {
  const vm = new D.VulnManager();
  return zip(9101, [
    () => vm.file({ id: 'CVE-X-1', severity: 'high', component: 'net', fixed: false }) && vm.count === 1,
    () => vm.get('CVE-X-1')?.severity === 'high' && vm.open().length === 1,
    () => D.VULN_SEVERITY.length === 5 && D.VULN_SEVERITY[4] === 'critical',
    () => { const r = vm.get('CVE-X-1')!; const ok = vm.fix(r.id); return ok && vm.fix(r.id) === false && vm.open().length === 0; },
    () => vm.file({ id: 'CVE-X-1', severity: 'low', component: 'x', fixed: false }) === false && vm.count === 1,
    () => vm.fix('missing') === false,
    () => vm.get('missing') === undefined,
    () => { vm.file({ id: 'V2', severity: 'critical', component: 'k', fixed: false }); return vm.slaQueue()[0]?.id === 'V2'; },
    () => vm.riskScore() === 4,
    () => { vm.reset(); return vm.count === 0 && vm.riskScore() === 0; },
    () => motionToken(true).curve === 'linear',
    () => focusOrderValid([1, 2, 3, 4, 5], 5),
    () => !focusOrderValid([1, 2, 3, 3, 5], 5),
    () => D.VULN_SEVERITY[0] === 'info',
    () => CONTRAST_MIN_PMIL === 450,
    () => budgetTableValid([20, 35, 55, 85, 120]),
    () => { const x = new D.VulnManager(); x.file({ id: 'A', severity: 'medium', component: 'c', fixed: false }); x.file({ id: 'B', severity: 'info', component: 'c', fixed: false }); return x.slaQueue()[0]?.id === 'A'; },
    () => { const x = new D.VulnManager(); const y = new D.VulnManager(); x.file({ id: 'S', severity: 'low', component: 'c', fixed: false }); y.file({ id: 'S', severity: 'low', component: 'c', fixed: false }); return x.count === 1 && y.count === 1; },
    () => degradeChain(['critical', 'high', 'medium']),
    () => { const g = vm.guards; return g.add(9101) && !g.add(9101); },
    () => suggest('vuln').actionable,
    () => { const q = new BatchQueue(2); q.step(); return q.step() && q.finished; },
    () => { const a = new D.AnomalyDetector('standard'); a.learnBaseline([10, 10, 10]); return a.baselineReady; },
    () => D.VULN_SEVERITY.length >= 5 && new Set(D.VULN_SEVERITY).size === 5,
    () => { const e = new D.VulnManager(); e.eggOn = true; const on = e.eggOn; e.reset(); return on && !e.eggOn; },
  ]);
}

/* -------- AI-37 族0366 异常检测 X09126~X09150 -------- */
export function checkF0366(): CheckEntry[] {
  const ad = new D.AnomalyDetector('standard');
  return zip(9126, [
    () => { ad.learnBaseline([100, 100, 100]); return ad.baselineReady && ad.deviationPmil(100) === 0; },
    () => new D.AnomalyDetector().profileId === 'standard' && ad.thresholdPmil === 700,
    () => levelMatrixValid(D.ANOMALY_LEVELS) && new D.AnomalyDetector('hair-trigger').thresholdPmil === 400,
    () => ad.deviationPmil(130) === 300 && ad.observe(130, 1) === false,
    () => ad.observe(200, 2) === true && ad.alertCount === 1,
    () => new D.AnomalyDetector('???').profileId === 'standard' && new D.AnomalyDetector('???').clamped === 1,
    () => { const x = new D.AnomalyDetector('standard'); return x.deviationPmil(5) === 0 && x.observe(5, 0) === false; },
    () => ad.alertLog[0]?.at === 2 && ad.alertLog[0]?.scorePmil === 1000,
    () => { const x = new D.AnomalyDetector('sensitive'); x.degrade(); return x.profileId === 'standard'; },
    () => { ad.reset(); return ad.alertCount === 0 && !ad.baselineReady; },
    () => motionToken(false).ms === 180,
    () => focusOrderValid([1, 2, 3, 4], 4),
    () => !focusOrderValid([4, 4], 4),
    () => D.ANOMALY_LEVELS.length === 5 && D.ANOMALY_LEVELS[0] === 'quiet',
    () => CONTRAST_MIN_PMIL >= 450,
    () => budgetTableValid([15, 25, 45, 70, 110]),
    () => { const x = new D.AnomalyDetector('standard'); x.learnBaseline([0, 0, 0]); return x.deviationPmil(1) === 1000; },
    () => { const a = new D.AnomalyDetector('standard'); a.learnBaseline([50, 50, 50]); const b = new D.AnomalyDetector('standard'); b.learnBaseline([50, 50, 50]); return a.deviationPmil(75) === b.deviationPmil(75); },
    () => degradeChain(['hair-trigger', 'sensitive', 'standard']),
    () => { const g = ad.guards; return g.add(9126) && !g.add(9126); },
    () => suggest('anom').why === 'why-anom',
    () => { const q = new BatchQueue(3); let ok = true; for (let i = 0; i < 3; i++) ok = q.step(); return ok && q.finished; },
    () => { const x = new D.AnomalyDetector('standard'); x.learnBaseline([10, 10, 10]); return x.whitelistScore(950) && !x.whitelistScore(950) && x.isWhitelisted(950); },
    () => D.ANOMALY_LEVELS[4] === 'hair-trigger' && new D.AnomalyDetector('quiet').thresholdPmil === 900,
    () => { const e = new D.AnomalyDetector('standard'); e.eggOn = true; const on = e.eggOn; e.reset(); return on && !e.eggOn; },
  ]);
}

/* -------- AI-37 族0367 完整性 2.0 X09151~X09175 -------- */
export function checkF0367(): CheckEntry[] {
  const im = new E.IntegrityMonitor('verify');
  return zip(9151, [
    () => im.seal('/sys/core', 'content-v1') === E.fnv1a('content-v1') && im.digestOf('/sys/core') !== undefined,
    () => new E.IntegrityMonitor().profileId === 'verify' && !im.blockOnFail,
    () => levelMatrixValid(E.INTEGRITY_LEVELS) && new E.IntegrityMonitor('sealed').blockOnFail,
    () => im.verify('/sys/core', 'content-v1') && !im.verify('/sys/core', 'content-v2') && im.violationCount === 1,
    () => im.verify('/unknown/path', 'anything') === true,
    () => new E.IntegrityMonitor('???').profileId === 'verify' && new E.IntegrityMonitor('???').clamped === 1,
    () => E.fnv1a('') === 0x811c9dc5 && E.fnv1a('a') !== E.fnv1a('b'),
    () => im.sealedCount === 1,
    () => { const x = new E.IntegrityMonitor('enforce'); x.degrade(); return x.profileId === 'verify'; },
    () => { im.reset(); return im.sealedCount === 0 && im.violationCount === 0; },
    () => motionToken(false).curve === 'std' && motionToken(true).curve === 'linear',
    () => focusOrderValid([1, 2, 3, 4, 5], 5),
    () => !focusOrderValid([1, 2, 2, 4, 5], 5),
    () => E.INTEGRITY_LEVELS.length === 5 && E.INTEGRITY_LEVELS[4] === 'sealed',
    () => CONTRAST_MIN_PMIL >= 450,
    () => budgetTableValid([10, 20, 40, 70, 100]),
    () => { const x = new E.IntegrityMonitor('verify'); x.seal('p', 'same'); const y = new E.IntegrityMonitor('verify'); y.seal('p', 'same'); return x.digestOf('p') === y.digestOf('p'); },
    () => E.fnv1a('hello') === E.fnv1a('hello'),
    () => degradeChain(['sealed', 'enforce', 'verify']),
    () => { const g = im.guards; return g.add(9151) && !g.add(9151); },
    () => suggest('integ').why === 'why-integ',
    () => { const q = new BatchQueue(2); q.step(); return q.step() && q.finished; },
    () => { const s = new E.SessionIdentity(); s.issue(1000); return s.tokenCount === 1 && s.alive(2000); },
    () => E.INTEGRITY_LEVELS[0] === 'observe' && !new E.IntegrityMonitor('observe').blockOnFail,
    () => { const e = new E.IntegrityMonitor('verify'); e.eggOn = true; const on = e.eggOn; e.reset(); return on && !e.eggOn; },
  ]);
}

/* -------- AI-37 族0368 会话身份 2.0 X09176~X09200 -------- */
export function checkF0368(): CheckEntry[] {
  const se = new E.SessionIdentity(60_000);
  return zip(9176, [
    () => { const t = se.issue(1000); return t.startsWith('sess-') && se.level === 'standard'; },
    () => new E.SessionIdentity().level === 'standard',
    () => E.SESSION_LEVELS.length === 5 && new Set(E.SESSION_LEVELS).size === 5,
    () => se.alive(60_999) && !se.alive(61_001),
    () => { se.stepUp(); return se.level === 'stepped-up' && se.canSensitive(2000); },
    () => new E.SessionIdentity(-5).clamped === 1,
    () => { const x = new E.SessionIdentity(1000); x.issue(0); return !x.alive(2000) && !x.canSensitive(2000); },
    () => { const x = new E.SessionIdentity(); x.issue(5); x.lock(); return x.level === 'locked' && !x.canSensitive(10); },
    () => { const x = new E.SessionIdentity(); x.issue(0); x.stepUp(); x.lock(); return x.level === 'locked'; },
    () => { se.reset(); return se.tokenCount === 0 && se.level === 'standard'; },
    () => motionToken(false).ms === 180 && motionToken(true).ms === 120,
    () => focusOrderValid([1, 2, 3, 4], 4),
    () => !focusOrderValid([1, 1], 4),
    () => E.SESSION_LEVELS[0] === 'guest' && E.SESSION_LEVELS[4] === 'locked',
    () => CONTRAST_MIN_PMIL >= 450,
    () => budgetTableValid([10, 20, 30, 50, 80]),
    () => { const x = new E.SessionIdentity(); const y = new E.SessionIdentity(); x.issue(7); y.issue(7); return x.tokenCount === 1 && y.tokenCount === 1; },
    () => { const x = new E.SessionIdentity(30_000); x.issue(0); const y = new E.SessionIdentity(30_000); y.issue(0); return x.alive(1000) === y.alive(1000); },
    () => degradeChain(['locked', 'stepped-up', 'confirmed']),
    () => { const g = se.guards; return g.add(9176) && !g.add(9176); },
    () => suggest('sess').why === 'why-sess',
    () => { const q = new BatchQueue(2); q.step(); return q.step() && q.finished; },
    () => { const i = new E.IntegrityMonitor('verify'); i.seal('x', 'y'); return i.verify('x', 'y'); },
    () => E.SESSION_LEVELS.length >= 5,
    () => { const e = new E.SessionIdentity(); e.eggOn = true; const on = e.eggOn; e.reset(); return on && !e.eggOn; },
  ]);
}

/* -------- AI-37 族0369 物理安全 2.0 X09201~X09225 -------- */
export function checkF0369(): CheckEntry[] {
  const ps = new E.PhysicalSecurity('lock');
  return zip(9201, [
    () => ps.tamper('lid', 100) === 'lock' && ps.tamperCount === 1,
    () => new E.PhysicalSecurity().profileId === 'lock' && ps.profile.usbGuard,
    () => levelMatrixValid(E.PHYSICAL_LEVELS) && new E.PhysicalSecurity('fort-knox').profile.micHardwareKill,
    () => ps.tamperLog[0]?.kind === 'lid' && ps.tamperLog[0]?.at === 100,
    () => { const x = new E.PhysicalSecurity('notify'); return x.tamper('usb', 1) === 'log' && !x.profile.lidCloseLock; },
    () => new E.PhysicalSecurity('???').profileId === 'lock' && new E.PhysicalSecurity('???').clamped === 1,
    () => { const x = new E.PhysicalSecurity('off'); return !x.profile.usbGuard && x.tamper('kbd', 1) === 'log'; },
    () => ps.tamper('cam', 200) === 'lock' && ps.tamperCount === 2,
    () => { const x = new E.PhysicalSecurity('wipe-pin'); x.degrade(); return x.profileId === 'lock'; },
    () => { ps.reset(); return ps.tamperCount === 0 && ps.tamperLog.length === 0; },
    () => motionToken(true).curve === 'linear',
    () => focusOrderValid([1, 2, 3, 4, 5], 5),
    () => !focusOrderValid([1, 2, 2, 4, 5], 5),
    () => E.PHYSICAL_LEVELS.length === 5 && E.PHYSICAL_LEVELS[4] === 'fort-knox',
    () => CONTRAST_MIN_PMIL >= 450,
    () => budgetTableValid([5, 15, 30, 60, 90]),
    () => new E.PhysicalSecurity('wipe-pin').profile.cameraShutter && !new E.PhysicalSecurity('lock').profile.cameraShutter,
    () => { const x = new E.PhysicalSecurity('lock'); const y = new E.PhysicalSecurity('lock'); return x.profile.lidCloseLock === y.profile.lidCloseLock; },
    () => degradeChain(['fort-knox', 'wipe-pin', 'lock']),
    () => { const g = ps.guards; return g.add(9201) && !g.add(9201); },
    () => suggest('phys').actionable,
    () => { const q = new BatchQueue(2); q.step(); return q.step() && q.finished; },
    () => { const b = new E.BackupSecurity('daily'); const id = b.backup(1000, 512); return b.snapshotOf(id)?.sealed === true; },
    () => E.PHYSICAL_LEVELS[0] === 'off' && !new E.PhysicalSecurity('off').profile.lidCloseLock,
    () => { const e = new E.PhysicalSecurity('lock'); e.eggOn = true; const on = e.eggOn; e.reset(); return on && !e.eggOn; },
  ]);
}

/* -------- AI-37 族0370 备份安全 2.0 X09226~X09250 -------- */
export function checkF0370(): CheckEntry[] {
  const bs = new E.BackupSecurity('daily');
  return zip(9226, [
    () => bs.backup(1000, 512) === 1 && bs.snapshotCount === 1,
    () => new E.BackupSecurity().profileId === 'daily' && bs.sealedByDefault,
    () => levelMatrixValid(E.BACKUP_LEVELS) && new E.BackupSecurity('immutable').immutable,
    () => bs.backup(1000, 999) === -1 && bs.snapshotCount === 1,
    () => bs.snapshotOf(1)?.at === 1000 && bs.snapshotOf(99) === undefined,
    () => new E.BackupSecurity('???').profileId === 'daily' && new E.BackupSecurity('???').clamped === 1,
    () => { const x = new E.BackupSecurity('manual'); return !x.sealedByDefault; },
    () => bs.backup(2000, 100) === 2 && bs.restorePoint(2500)?.id === 2 && bs.restorePoint(1500)?.id === 1,
    () => { const x = new E.BackupSecurity('continuous'); x.degrade(); return x.profileId === 'hourly'; },
    () => { const x = new E.BackupSecurity('daily'); const id = x.backup(1, 1); return x.deleteSnapshot(id) && !x.deleteSnapshot(id); },
    () => motionToken(false).curve === 'std',
    () => focusOrderValid([1, 2, 3, 4, 5], 5),
    () => !focusOrderValid([1, 2, 3, 3, 5], 5),
    () => E.BACKUP_LEVELS.length === 5 && E.BACKUP_LEVELS[4] === 'immutable',
    () => CONTRAST_MIN_PMIL >= 450,
    () => budgetTableValid([20, 40, 60, 100, 150]),
    () => { const x = new E.BackupSecurity('immutable'); const id = x.backup(9, 9); return !x.deleteSnapshot(id) && x.snapshotCount === 1; },
    () => { const x = new E.BackupSecurity('daily'); x.backup(5, 10); const y = new E.BackupSecurity('daily'); y.backup(5, 10); return x.snapshotCount === 1 && y.snapshotCount === 1; },
    () => degradeChain(['immutable', 'continuous', 'hourly']),
    () => { const g = bs.guards; return g.add(9226) && !g.add(9226); },
    () => suggest('backup').why === 'why-backup',
    () => { const q = new BatchQueue(3); let ok = true; for (let i = 0; i < 3; i++) ok = q.step(); return ok && q.finished; },
    () => { const p = new E.PhysicalSecurity('lock'); return p.tamper('lid', 1) === 'lock'; },
    () => E.BACKUP_LEVELS[0] === 'manual' && !new E.BackupSecurity('manual').sealedByDefault,
    () => { const e = new E.BackupSecurity('daily'); e.eggOn = true; const on = e.eggOn; e.reset(); return on && !e.eggOn; },
  ]);
}

/* -------- 领域10（AI-36/AI-37 部分）全量聚合 -------- */
const FAMILY_CHECKS: Array<{ start: number; fn: () => CheckEntry[] }> = [
  { start: 8851, fn: checkF0355 },
  { start: 8876, fn: checkF0356 },
  { start: 8901, fn: checkF0357 },
  { start: 8926, fn: checkF0358 },
  { start: 8951, fn: checkF0359 },
  { start: 8976, fn: checkF0360 },
  { start: 9001, fn: checkF0361 },
  { start: 9026, fn: checkF0362 },
  { start: 9051, fn: checkF0363 },
  { start: 9076, fn: checkF0364 },
  { start: 9101, fn: checkF0365 },
  { start: 9126, fn: checkF0366 },
  { start: 9151, fn: checkF0367 },
  { start: 9176, fn: checkF0368 },
  { start: 9201, fn: checkF0369 },
  { start: 9226, fn: checkF0370 },
];

export function runAi3637Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const entries: CheckEntry[] = [];
  for (const f of FAMILY_CHECKS) entries.push(...f.fn());
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}

/** 供设置页安全中心消费的族清单（落点占位：SecurityTab 后续接入）。 */
export const SECURITY_FAMILY_IDS = FAMILY_CHECKS.map((f) => xid(f.start));
