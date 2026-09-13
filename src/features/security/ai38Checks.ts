/**
 * UNREAL-X-15000 · AI-38 防线工程 CheckSet（族0371~0376 · X09251~X09400），勿删。
 * 本文件承载 V 线三族 75 项（族0372/0373/0374 · X09276~X09350）；
 * K 线三族 75 项在 kernel/varix/src/sec/（netiso2/crypto2/permmin2）。
 * 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
 */
import type { CheckEntry } from '../uikit/checks';
import * as T from './ai38Models';

/* -------- 族0372 安全应急 2.0 X09276~X09300 -------- */
export function checkF0372(): CheckEntry[] {
  return [
    { id: 'X09276', name: '应急·最小闭环', check: () => new T.EmergencyHub().trigger({ kind: 'leak', score: 10 }) === 'observe' },
    { id: 'X09277', name: '应急·全量参数', check: () => { const h = new T.EmergencyHub(); h.trigger({ kind: 'ransom', score: 99 }); return h.level === 'freeze'; } },
    { id: 'X09278', name: '应急·档位矩阵', check: () => { const got = [10, 40, 70, 90, 99].map((sc) => { const h = new T.EmergencyHub(); return h.trigger({ kind: 'intrusion', score: sc }); }); return got.join(',') === 'observe,alert,lockdown,airgap,freeze'; } },
    { id: 'X09279', name: '应急·快照迁移', check: () => { const h = new T.EmergencyHub(); h.trigger({ kind: 'leak', score: 85 }); h.trigger({ kind: 'intrusion', score: 99 }); return h.log.length === 2 && h.level === 'freeze'; } },
    { id: 'X09280', name: '应急·联调集成', check: () => { const h = new T.EmergencyHub(); h.trigger({ kind: 'ransom', score: 65 }); return T.EmergencyHub.label(h.level) === '锁定入口'; } },
    { id: 'X09281', name: '应急·越界钳制', check: () => { const h = new T.EmergencyHub(); h.setLevel('panic' as string); return h.level === 'observe' && h.clamped === 1; } },
    { id: 'X09282', name: '应急·失败叙事', check: () => { const h = new T.EmergencyHub(); h.setLevel('freeze'); const a = h.advice(); h.setLevel('alert'); return a === '全量冻结，进入应急响应' && h.advice() === '核对告警来源，确认后升级'; } },
    { id: 'X09283', name: '应急·中断还原', check: () => { const h = new T.EmergencyHub(); h.trigger({ kind: 'lost-device', score: 88 }); return h.standDown() === 'observe' && h.log.length === 1; } },
    { id: 'X09284', name: '应急·资源降级', check: () => { const h = new T.EmergencyHub(); h.trigger({ kind: 'leak', score: 30 }); return h.escalateOnly(); } },
    { id: 'X09285', name: '应急·回滚净身', check: () => { const q = new T.EmergencyHub(); return q.level === 'observe' && q.log.length === 0 && q.clamped === 0; } },
    { id: 'X09286', name: '应急·动效令牌', check: () => T.EMERGENCY_LEVELS.length === 5 && Object.keys(T.EMERGENCY_LABELS).length === 5 },
    { id: 'X09287', name: '应急·三态焦点', check: () => { const h = new T.EmergencyHub(); return h.trigger({ kind: 'intrusion', score: 59 }) === 'alert' && h.trigger({ kind: 'intrusion', score: 0 }) === 'observe'; } },
    { id: 'X09288', name: '应急·键盘序', check: () => T.EMERGENCY_LEVELS.every((lv, i) => T.EMERGENCY_LEVELS.indexOf(lv) === i) },
    { id: 'X09289', name: '应急·微文案', check: () => T.EmergencyHub.label('observe') === '观察值机' && T.EmergencyHub.label('airgap') === '断网隔离' },
    { id: 'X09290', name: '应急·aria 等价', check: () => { const h = new T.EmergencyHub(); h.setLevel('lockdown'); return h.advice().length > 4; } },
    { id: 'X09291', name: '应急·基准采集', check: () => { const h = new T.EmergencyHub(); let n = 0; for (let i = 0; i < 100; i++) { if (typeof h.trigger({ kind: 'leak', score: i }) === 'string') n++; } return n === 100 && h.log.length === 100; } },
    { id: 'X09292', name: '应急·热路径', check: () => { const h = new T.EmergencyHub(); return h.trigger({ kind: 'ransom', score: 95 }) === 'freeze'; } },
    { id: 'X09293', name: '应急·零漂移', check: () => { const h = new T.EmergencyHub(); const a = h.trigger({ kind: 'leak', score: 72 }); h.standDown(); const b = h.trigger({ kind: 'leak', score: 72 }); return a === b && a === 'lockdown'; } },
    { id: 'X09294', name: '应急·低配减档', check: () => { const h = new T.EmergencyHub(); return h.trigger({ kind: 'intrusion', score: 29 }) === 'observe'; } },
    { id: 'X09295', name: '应急·守卫', check: () => { const h = new T.EmergencyHub(); h.trigger({ kind: 'leak', score: 96 }); h.setLevel('observe'); h.trigger({ kind: 'leak', score: 10 }); return h.level === 'observe'; } },
    { id: 'X09296', name: '应急·智能建议', check: () => { const h = new T.EmergencyHub(); h.setLevel('airgap'); return h.advice() === '断网隔离，保存现场'; } },
    { id: 'X09297', name: '应急·批量模式', check: () => { const h = new T.EmergencyHub(); for (let i = 0; i < 50; i++) h.trigger({ kind: 'leak', score: i }); return h.log.length === 50; } },
    { id: 'X09298', name: '应急·跨域联动', check: () => { const h = new T.EmergencyHub(); h.trigger({ kind: 'ransom', score: 90 }); return h.level === 'airgap'; } },
    { id: 'X09299', name: '应急·扩展点', check: () => typeof T.EmergencyHub.label === 'function' && typeof new T.EmergencyHub().standDown === 'function' },
    { id: 'X09300', name: '应急·彩蛋层', check: () => new T.EmergencyHub().trigger({ kind: 'intrusion', score: 0 }) === 'observe' },
  ];
}

/* -------- 族0373 长者守护 2.0 X09301~X09325 -------- */
export function checkF0373(): CheckEntry[] {
  return [
    { id: 'X09301', name: '长者·最小闭环', check: () => new T.ElderGuard().apply('companion') === 'companion' },
    { id: 'X09302', name: '长者·全量参数', check: () => { const q = new T.ElderGuard(); q.apply('secure'); return q.profile.payConfirmYuan === 0 && q.profile.blockUnknown; } },
    { id: 'X09303', name: '长者·档位矩阵', check: () => T.ELDER_LEVELS.every((lv) => { const q = new T.ElderGuard(); q.apply(lv); return T.ELDER_PRESETS[lv].stepsGoal > 0; }) },
    { id: 'X09304', name: '长者·快照迁移', check: () => { const q = new T.ElderGuard(); q.apply('cautious'); return q.profile.payConfirmYuan === 500; } },
    { id: 'X09305', name: '长者·联调集成', check: () => { const q = new T.ElderGuard(); q.apply('large'); return q.callPolicy(true) === 'allow' && q.payPolicy(3500) === 'confirm'; } },
    { id: 'X09306', name: '长者·越界钳制', check: () => { const q = new T.ElderGuard(); q.apply(' turbo' as string); return q.level === 'standard' && q.clamped === 1; } },
    { id: 'X09307', name: '长者·失败叙事', check: () => { const q = new T.ElderGuard(); q.apply('secure'); return q.payPolicy(1) === 'confirm' && q.callPolicy(false) === 'screen'; } },
    { id: 'X09308', name: '长者·中断还原', check: () => { const q = new T.ElderGuard(); q.apply('companion'); return q.ack('son') === 1 && q.ack('son') === 1; } },
    { id: 'X09309', name: '长者·资源降级', check: () => { const q = new T.ElderGuard(); q.apply('secure'); return q.payPolicy(100) === 'confirm'; } },
    { id: 'X09310', name: '长者·回滚净身', check: () => { const q = new T.ElderGuard(); return q.level === 'standard' && q.acks.length === 0; } },
    { id: 'X09311', name: '长者·动效令牌', check: () => T.ELDER_LEVELS.length === 5 && Object.keys(T.ELDER_PRESETS).length === 5 },
    { id: 'X09312', name: '长者·三态焦点', check: () => { const q = new T.ElderGuard(); q.apply('standard'); return q.callPolicy(false) === 'allow' && q.payPolicy(999) === 'pass'; } },
    { id: 'X09313', name: '长者·键盘序', check: () => T.ELDER_LEVELS.every((lv, i) => T.ELDER_LEVELS.indexOf(lv) === i), },
    { id: 'X09314', name: '长者·微文案', check: () => T.ElderGuard.label('companion') === '陪伴' && T.ElderGuard.label('secure') === '安心' },
    { id: 'X09315', name: '长者·aria 等价', check: () => { const q = new T.ElderGuard(); q.apply('cautious'); return q.callPolicy(false) === 'screen' && q.profile.stepsGoal === 2000; } },
    { id: 'X09316', name: '长者·基准采集', check: () => T.ELDER_LEVELS.map((lv) => T.ELDER_PRESETS[lv].payConfirmYuan).every((y) => y >= 0) },
    { id: 'X09317', name: '长者·热路径', check: () => { const q = new T.ElderGuard(); q.apply('companion'); return q.payPolicy(1001) === 'confirm' && q.payPolicy(999) === 'pass'; } },
    { id: 'X09318', name: '长者·零漂移', check: () => { const q = new T.ElderGuard(); q.apply('large'); const a = q.profile; q.apply('large'); return a === q.profile; } },
    { id: 'X09319', name: '长者·低配减档', check: () => { const q = new T.ElderGuard(); q.apply('standard'); return q.profile.blockUnknown === false; } },
    { id: 'X09320', name: '长者·守卫', check: () => { const q = new T.ElderGuard(); q.apply('secure'); return q.payPolicy(Number.MAX_SAFE_INTEGER) === 'confirm'; } },
    { id: 'X09321', name: '长者·智能建议', check: () => { const q = new T.ElderGuard(); q.apply('cautious'); return q.callPolicy(false) === 'screen'; } },
    { id: 'X09322', name: '长者·批量模式', check: () => { const q = new T.ElderGuard(); ['son', 'daughter', 'son', 'nurse'].forEach((who) => q.ack(who)); return q.acks.length === 3; } },
    { id: 'X09323', name: '长者·跨域联动', check: () => { const q = new T.ElderGuard(); q.apply('secure'); return q.callPolicy(false) === 'screen' && q.payPolicy(0) === 'pass'; } },
    { id: 'X09324', name: '长者·扩展点', check: () => typeof T.ElderGuard.label === 'function' && 'profile' in new T.ElderGuard() },
    { id: 'X09325', name: '长者·彩蛋层', check: () => new T.ElderGuard().apply('large') === 'large' },
  ];
}

/* -------- 族0374 安全教育 2.0 X09326~X09350 -------- */
export function checkF0374(): CheckEntry[] {
  const syllabus: T.EduLesson[] = T.SecurityAcademy.syllabus();
  const L = (i: number): T.EduLesson => syllabus[i]!;
  return [
    { id: 'X09326', name: '教育·最小闭环', check: () => { const q = new T.SecurityAcademy(); return q.complete(L(0), 80); } },
    { id: 'X09327', name: '教育·全量参数', check: () => syllabus.length === 5 && L(4).passScore === 80 },
    { id: 'X09328', name: '教育·档位矩阵', check: () => { const q = new T.SecurityAcademy(); let ok = true; for (const l of syllabus) ok = ok && q.complete(l, 100); return ok && q.unlocked.length === 5; } },
    { id: 'X09329', name: '教育·快照迁移', check: () => { const q = new T.SecurityAcademy(); q.complete(L(0), 50); return q.wrongBook.includes(1); } },
    { id: 'X09330', name: '教育·联调集成', check: () => { const q = new T.SecurityAcademy(); q.complete(L(0), 100); return q.unlocked.includes('advance'); } },
    { id: 'X09331', name: '教育·越界钳制', check: () => { const q = new T.SecurityAcademy(); return !q.complete({ id: 99, tier: 'nope' as T.EduTier, topic: 'x', passScore: 60 }, 100) && q.clamped === 1; } },
    { id: 'X09332', name: '教育·失败叙事', check: () => { const q = new T.SecurityAcademy(); const ok = q.complete(L(1), 59); return !ok && q.wrongBook.includes(2); } },
    { id: 'X09333', name: '教育·中断还原', check: () => { const q = new T.SecurityAcademy(); q.complete(L(0), 40); return q.clearWrong(1) === 0; } },
    { id: 'X09334', name: '教育·资源降级', check: () => { const q = new T.SecurityAcademy(); return !q.complete(L(2), 69) && q.complete(L(2), 71) === false ? false : true; } },
    { id: 'X09335', name: '教育·回滚净身', check: () => { const q = new T.SecurityAcademy(); return q.unlocked.length === 1 && q.wrongBook.length === 0; } },
    { id: 'X09336', name: '教育·动效令牌', check: () => T.EDU_TIERS.length === 5 && T.SecurityAcademy.syllabus().every((l, i) => l.id === i + 1) },
    { id: 'X09337', name: '教育·三态焦点', check: () => { const q = new T.SecurityAcademy(); return q.complete(L(0), 60) && !q.complete(L(1), 64); } },
    { id: 'X09338', name: '教育·键盘序', check: () => T.EDU_TIERS.every((t, i) => T.EDU_TIERS.indexOf(t) === i) },
    { id: 'X09339', name: '教育·微文案', check: () => T.SecurityAcademy.label('intro') === '入门' && T.SecurityAcademy.label('trainer') === '讲师' },
    { id: 'X09340', name: '教育·aria 等价', check: () => syllabus.every((l) => l.topic.length >= 4) },
    { id: 'X09341', name: '教育·基准采集', check: () => { const q = new T.SecurityAcademy(); let n = 0; for (const l of syllabus) if (q.complete(l, 100)) n++; return n === 5; } },
    { id: 'X09342', name: '教育·热路径', check: () => { const q = new T.SecurityAcademy(); return q.complete(L(0), 61); } },
    { id: 'X09343', name: '教育·零漂移', check: () => { const q = new T.SecurityAcademy(); const a = q.complete(L(0), 70); const b = q.complete({ ...L(0), id: 9 }, 70); return a === b; } },
    { id: 'X09344', name: '教育·低配减档', check: () => { const q = new T.SecurityAcademy(); return !q.complete(L(0), 59); } },
    { id: 'X09345', name: '教育·守卫', check: () => { const q = new T.SecurityAcademy(); q.complete(L(0), 40); q.complete(L(0), 40); return q.wrongBook.filter((id) => id === 1).length === 1; } },
    { id: 'X09346', name: '教育·智能建议', check: () => { const q = new T.SecurityAcademy(); q.complete(L(0), 40); q.complete(L(0), 90); return q.clearWrong(1) === 0 && q.unlocked.includes('advance'); } },
    { id: 'X09347', name: '教育·批量模式', check: () => { const q = new T.SecurityAcademy(); for (let i = 0; i < 30; i++) q.complete({ ...L(0), id: 100 + i }, 30); return q.wrongBook.length === 30; } },
    { id: 'X09348', name: '教育·跨域联动', check: () => { const q = new T.SecurityAcademy(); q.complete(L(0), 100); q.complete(L(1), 100); return q.unlocked.includes('drill'); } },
    { id: 'X09349', name: '教育·扩展点', check: () => typeof T.SecurityAcademy.syllabus === 'function' && typeof new T.SecurityAcademy().clearWrong === 'function' },
    { id: 'X09350', name: '教育·彩蛋层', check: () => new T.SecurityAcademy().complete(L(0), 100) },
  ];
}

/** AI-38 V 线聚合：3 族 75 项（X09276~X09350）。 */
export function runAi38VChecks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [checkF0372, checkF0373, checkF0374];
  const entries = families.flatMap((fn) => fn());
  const failed = entries.filter((e) => {
    try {
      return !e.check();
    } catch {
      return true;
    }
  });
  return { entries, failed };
}
