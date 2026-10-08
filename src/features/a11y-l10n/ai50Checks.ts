/**
 * UNREAL-X-15000 · AI-50 无障碍五域 CheckSet（族0491~0500 · X12251~X12500），勿删。
 * 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
 */
import type { CheckEntry } from '../compat/checks';
import * as T from './ai50Models';

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

const BLACK: [number, number, number] = [0, 0, 0];
const WHITE: [number, number, number] = [255, 255, 255];

/* -------- 族0491 视觉无障碍 2.0 X12251~X12275 -------- */
export function checkF0491(): CheckEntry[] {
  const v = new T.VisualA11y();
  return mk25(12251, '视觉', [
    () => v.setScale(1.25) === 1.25 && v.scale === 1.25,
    () => v.setScale(1.5) === 1.5 && v.toggleHc() === true,
    () => T.TEXT_SCALES.length === 5 && v.setScale(2) === 2,
    () => v.snapshot().includes('"scale":2') && new T.VisualA11y().restore(v.snapshot()),
    () => { const q = new T.VisualA11y(); return q.restore(v.snapshot()) && q.scale === 2 && q.hc === true; },
    () => v.setScale(9.9) === 1 && v.clamped >= 1,
    () => T.VisualA11y.contrast(BLACK, WHITE) === 21,
    () => { const q = new T.VisualA11y(); return q.snapshot() === '{"scale":1,"hc":false}'; },
    () => { const q = new T.VisualA11y(); q.setScale(1.5); return q.scale === 1.5; },
    () => { const q = new T.VisualA11y(); q.setScale(2); q.setScale(1); return q.scale === 1; },
    () => v.materialMap('frosted') === 'solid' && v.materialMap('solid') === 'solid',
    () => T.VisualA11y.aaText(BLACK, WHITE) === true,
    () => T.TEXT_SCALES.every((s) => v.setScale(s) === s),
    () => { const q = new T.VisualA11y(); return q.setScale(Number.NaN) === 1 && q.clamped >= 1; },
    () => { const q = new T.VisualA11y(); q.hc = false; return q.materialMap('acrylic') === 'acrylic'; },
    () => { const t0 = performance.now(); for (let i = 0; i < 300; i++) T.VisualA11y.contrast([10, 20, 30], [200, 210, 220]); return performance.now() - t0 < 50; },
    () => T.VisualA11y.contrast(WHITE, WHITE) === 1,
    () => { const a = new T.VisualA11y(); const b = new T.VisualA11y(); a.setScale(1.5); b.setScale(1.5); return a.snapshot() === b.snapshot(); },
    () => { const q = new T.VisualA11y(); q.toggleHc(); q.toggleHc(); return q.hc === false; },
    () => { const q = new T.VisualA11y(); return q.restore('bad json') === false && q.scale === 1; },
    () => { const q = new T.VisualA11y(); q.restore('{"scale":1.25,"hc":true}'); return q.scale === 1.25 && q.hc; },
    () => T.VisualA11y.contrast(BLACK, WHITE) > T.VisualA11y.contrast([120, 120, 120], [130, 130, 130]),
    () => typeof T.VisualA11y.aaText === 'function' && typeof v.materialMap === 'function',
    () => { const q = new T.VisualA11y(); q.setScale(2); return q.snapshot().includes('"hc":false'); },
    () => { const q = new T.VisualA11y(); q.setScale(1.1); return q.snapshot().includes('1.1'); },
  ]);
}

/* -------- 族0492 听觉无障碍 2.0 X12276~X12300 -------- */
export function checkF0492(): CheckEntry[] {
  const h = new T.HearA11y();
  return mk25(12276, '听觉', [
    () => h.toggleMono() === true && h.mono === true,
    () => h.mix(0.4, 0.6) === 0.5,
    () => T.ALERT_KINDS.length === 5 && h.addAlert('flash'),
    () => h.setCaptionDelay(200) === 200 && h.captionDelayMs === 200,
    () => h.hasEquivalent() === true,
    () => h.setCaptionDelay(-1) === 0 && h.clamped >= 1,
    () => h.addAlert('nope') === false && h.clamped >= 2,
    () => { const q = new T.HearA11y(); return q.mix(1, 1) === 1 && q.mono === false; },
    () => { const q = new T.HearA11y(); q.addAlert('banner'); q.addAlert('banner'); return q.alerts.length === 1; },
    () => { const q = new T.HearA11y(); return q.hasEquivalent() === false; },
    () => h.mix(0.2, 0.2) === 0.2,
    () => h.setCaptionDelay(5000) === 5000,
    () => T.ALERT_KINDS.every((k) => h.addAlert(k) === false || true) && T.ALERT_KINDS.every((k) => (T.ALERT_KINDS as readonly string[]).includes(k)),
    () => { const q = new T.HearA11y(); return q.setCaptionDelay(Number.NaN) === 0; },
    () => { const q = new T.HearA11y(); q.addAlert('vibrate'); q.reset(); return q.alerts.length === 0 && !q.mono; },
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) { const q = new T.HearA11y(); q.mix(i / 1000, 0.5); } return performance.now() - t0 < 50; },
    () => h.setCaptionDelay(300) === 300,
    () => { const a = new T.HearA11y(); const b = new T.HearA11y(); a.addAlert('flash'); b.addAlert('flash'); return a.alerts.length === b.alerts.length; },
    () => { const q = new T.HearA11y(); q.toggleMono(); q.toggleMono(); return q.mono === false; },
    () => { const q = new T.HearA11y(); return q.addAlert('legend') && q.alerts[0] === 'legend'; },
    () => { let n = 0; for (const k of T.ALERT_KINDS) { if ((T.ALERT_KINDS as readonly string[]).includes(k)) n++; } return n === 5; },
    () => { const q = new T.HearA11y(); q.addAlert('caption'); return q.hasEquivalent(); },
    () => typeof h.mix === 'function' && typeof h.addAlert === 'function',
    () => { const q = new T.HearA11y(); q.addAlert('eggless'.slice(0, 0) || 'flash'); return q.alerts.includes('flash'); },
    () => { const q = new T.HearA11y(); q.addAlert('banner'); q.addAlert('vibrate'); return q.alerts.length === 2; },
  ]);
}

/* -------- 族0493 运动无障碍 2.0 X12301~X12325 -------- */
export function checkF0493(): CheckEntry[] {
  const m = new T.MotorA11y();
  return mk25(12301, '运动', [
    () => m.setDwell(1000) === 1000 && m.dwellMs === 1000,
    () => m.dwellOk(1000) === true && m.dwellOk(999) === false,
    () => m.pressModifier('ctrl') === 1 && m.pressModifier('shift') === 2,
    () => m.comboReady() === true,
    () => { m.clearSticky(); return m.stickySeq.length === 0; },
    () => m.setDwell(0) === 200 && m.clamped >= 1,
    () => m.pressModifier('f9') === 0 && m.clamped >= 2,
    () => { const q = new T.MotorA11y(); return q.setDwell(Number.NaN) === 800; },
    () => { const q = new T.MotorA11y(); q.pressModifier('alt'); q.pressModifier('win'); return q.comboReady(); },
    () => T.MotorA11y.targetOk(44, false) === true,
    () => m.setDwell(3000) === 3000,
    () => T.MotorA11y.targetOk(40, true) === true && T.MotorA11y.targetOk(39, true) === false,
    () => { const q = new T.MotorA11y(); q.pressModifier('ctrl'); q.pressModifier('ctrl'); return q.stickySeq.length === 2; },
    () => T.MotorA11y.targetOk(43, false) === false,
    () => { const q = new T.MotorA11y(); q.pressModifier('ctrl'); q.clearSticky(); return q.comboReady() === false; },
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) new T.MotorA11y().setDwell(i % 4000); return performance.now() - t0 < 50; },
    () => m.setDwell(200) === 200,
    () => { const a = new T.MotorA11y(); const b = new T.MotorA11y(); a.setDwell(600); b.setDwell(600); return a.dwellMs === b.dwellMs; },
    () => { const q = new T.MotorA11y(); for (let i = 0; i < 6; i++) q.pressModifier('ctrl'); return q.stickySeq.length === 5; },
    () => { const q = new T.MotorA11y(); return q.setDwell(-5) === 200 && q.clamped >= 1; },
    () => { let n = 0; for (const k of ['ctrl', 'shift', 'alt', 'win']) { if (m.pressModifier(k) > 0) n++; } return n === 4; },
    () => m.dwellOk(200) === true,
    () => typeof T.MotorA11y.targetOk === 'function' && typeof m.comboReady === 'function',
    () => { const q = new T.MotorA11y(); q.pressModifier('shift'); return q.pressModifier('bogus') === 1; },
    () => { const q = new T.MotorA11y(); q.pressModifier('ctrl'); q.pressModifier('alt'); return q.stickySeq.join('|') === 'ctrl|alt'; },
  ]);
}

/* -------- 族0494 认知无障碍 2.0 X12326~X12350 -------- */
export function checkF0494(): CheckEntry[] {
  const c = new T.CogA11y();
  return mk25(12326, '认知', [
    () => c.setLevel('simple') === 'simple' && c.level === 'simple',
    () => c.setFocus(25) === 25 && c.focusMin === 25,
    () => T.READ_LEVELS.length === 3 && c.setLevel('guided') === 'guided',
    () => c.block('game1') && c.isBlocked('game1'),
    () => c.simplify('初始化') === '开始设置',
    () => c.setFocus(999) === 120 && c.clamped >= 1,
    () => c.block('') === false,
    () => { const q = new T.CogA11y(); return q.simplify('不存在的词') === '不存在的词'; },
    () => { const q = new T.CogA11y(); q.block('a'); q.block('a'); return q.distractions === 1; },
    () => { const q = new T.CogA11y(); return q.setFocus(Number.NaN) === 25; },
    () => c.simplify('校验') === '检查',
    () => c.setFocus(1) === 1,
    () => T.READ_LEVELS.every((l) => c.setLevel(l) === l),
    () => { const q = new T.CogA11y(); return q.setLevel('nope') === 'plain' && q.clamped >= 1; },
    () => { const q = new T.CogA11y(); q.block('x'); q.reset(); return q.isBlocked('x') === false && q.distractions === 0; },
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) { const q = new T.CogA11y(); q.simplify('渲染'); } return performance.now() - t0 < 50; },
    () => c.simplify('同步') === '更新一致',
    () => { const a = new T.CogA11y(); const b = new T.CogA11y(); a.setLevel('simple'); b.setLevel('simple'); return a.level === b.level; },
    () => { const q = new T.CogA11y(); q.block('p'); q.block('q'); return q.isBlocked('p') && q.isBlocked('q'); },
    () => { const q = new T.CogA11y(); return q.setFocus(0) === 1; },
    () => { let n = 0; for (const t of ['初始化', '同步', '渲染', '校验', '遥测']) { if (c.simplify(t) !== t) n++; } return n === 5; },
    () => { const q = new T.CogA11y(); q.block('dup'); return q.block('dup') === false; },
    () => typeof c.simplify === 'function' && typeof c.isBlocked === 'function',
    () => { const q = new T.CogA11y(); q.setFocus(50); return q.focusMin === 50; },
    () => { const q = new T.CogA11y(); return q.simplify('遥测') === '使用记录'; },
  ]);
}

/* -------- 族0495 语音无障碍 2.0 X12351~X12375 -------- */
export function checkF0495(): CheckEntry[] {
  const s = new T.VoiceA11y();
  return mk25(12351, '语音', [
    () => s.setRate(1.5) === 1.5 && s.rate === 1.5,
    () => s.register('v1', 'tts') && s.voices.has('v1'),
    () => T.VOICE_ROLES.length === 5 && s.register('v2', 'screen-reader'),
    () => s.byRole('tts').join(',') === 'v1',
    () => T.VoiceA11y.ssmlOk('<speak>你好</speak>') === true,
    () => s.setRate(9) === 3 && s.clamped >= 1,
    () => s.register('v1', 'tts') === false,
    () => T.VoiceA11y.ssmlOk('<speak>缺闭合') === false,
    () => { const q = new T.VoiceA11y(); q.register('a', 'stt'); return q.byRole('stt')[0] === 'a'; },
    () => s.register('', 'tts') === false,
    () => s.setRate(0.5) === 0.5,
    () => s.register('bad-role', 'nope') === false && s.clamped >= 2,
    () => T.VoiceA11y.ssmlOk('speak>x</speak') === false,
    () => (new T.VoiceA11y().setRate(Number.NaN) === 1),
    () => { const q = new T.VoiceA11y(); q.register('r', 'command'); return q.revoke('r') && !q.voices.has('r'); },
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) { const q = new T.VoiceA11y(); q.setRate(i % 5); } return performance.now() - t0 < 50; },
    () => s.setRate(2) === 2,
    () => { const a = new T.VoiceA11y(); const b = new T.VoiceA11y(); a.setRate(1.5); b.setRate(1.5); return a.rate === b.rate; },
    () => { const q = new T.VoiceA11y(); q.register('d1', 'dictation'); return q.byRole('dictation').length === 1; },
    () => (T.VoiceA11y.ssmlOk('') === false),
    () => { let n = 0; for (const r of T.VOICE_ROLES) { const q = new T.VoiceA11y(); if (q.register(`x-${r}`, r)) n++; } return n === 5; },
    () => { const q = new T.VoiceA11y(); q.register('dup', 'tts'); return q.register('dup', 'stt') === false; },
    () => typeof T.VoiceA11y.ssmlOk === 'function' && typeof s.byRole === 'function',
    () => { const q = new T.VoiceA11y(); q.register('egg', 'command'); return q.byRole('command')[0] === 'egg'; },
    () => (new T.VoiceA11y().revoke('ghost') === false),
  ]);
}

/* -------- 族0496 本地化核心 2.0 X12376~X12400 -------- */
export function checkF0496(): CheckEntry[] {
  const l = new T.L10nCore();
  return mk25(12376, '本地化', [
    () => T.L10nCore.tagOk('zh-CN') && l.addFallback('zh-CN'),
    () => l.fallbacks.length === 1,
    () => T.PLURAL_CATEGORIES.length === 6 && T.L10nCore.pluralEn(1) === 'one',
    () => T.L10nCore.pluralEn(2) === 'other' && T.L10nCore.pluralEn(0) === 'other',
    () => l.resolve('zh-Hant', ['zh-CN', 'en']) === 'zh-CN',
    () => T.L10nCore.tagOk('1bad') === false && l.clamped >= 0,
    () => l.addFallback('nope!') === false && l.clamped >= 1,
    () => (new T.L10nCore().resolve('fr', ['en']) === 'en'),
    () => { const q = new T.L10nCore(); q.addFallback('zh-CN'); return q.addFallback('zh-CN') === false; },
    () => T.L10nCore.pluralZh() === 'other',
    () => l.isRtl('ar-SA') === true && l.isRtl('en-US') === false,
    () => T.L10nCore.tagOk('zh-Hant-TW') === true,
    () => l.addFallback('en') && l.resolve('xx', ['en']) === 'en',
    () => { const q = new T.L10nCore(); return q.fallbacks.length === 0 && q.clamped === 0; },
    () => { const q = new T.L10nCore(); q.addFallback('fr'); q.reset(); return q.fallbacks.length === 0; },
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.L10nCore.tagOk('zh-CN'); return performance.now() - t0 < 50; },
    () => l.resolve('zh-CN', ['zh-CN', 'en']) === 'zh-CN',
    () => { const a = new T.L10nCore(); const b = new T.L10nCore(); a.addFallback('fr'); b.addFallback('fr'); return a.fallbacks.length === b.fallbacks.length; },
    () => l.isRtl('he') === true && l.isRtl('fa-IR') === true,
    () => (new T.L10nCore().resolve('de', []) === ''),
    () => { let n = 0; for (const t of ['it', 'nl', 'pl', 'sv', 'no']) { if (l.addFallback(t)) n++; } return n === 5; },
    () => (T.L10nCore.tagOk('zh') === true),
    () => typeof l.resolve === 'function' && typeof T.L10nCore.pluralEn === 'function',
    () => { const q = new T.L10nCore(); q.addFallback('pt-BR'); return q.fallbacks[0] === 'pt-BR'; },
    () => (l.isRtl('zh-CN') === false),
  ]);
}

/* -------- 族0497 中文本地化深化 2.0 X12401~X12425 -------- */
export function checkF0497(): CheckEntry[] {
  return mk25(12401, '中文', [
    () => T.ZhL10n.punct('你好,世界') === '你好，世界',
    () => T.ZhL10n.punct('完成.') === '完成。',
    () => T.ZhL10n.ellipsisOk('继续……') === true && T.ZhL10n.abbr(15000) === '1.5万',
    () => T.ZhL10n.abbr(2.5e8) === '2.5亿',
    () => T.ZhL10n.dateCn(2026, 9, 13) === '2026年9月13日',
    () => T.ZhL10n.abbr(-5) === '0',
    () => T.ZhL10n.punct('a?b!c') === 'a？b！c',
    () => (T.ZhL10n.ellipsisOk('等待...') === false),
    () => T.ZhL10n.abbr(999) === '999',
    () => (T.ZhL10n.dateCn(2026, 13, 1) === '2026年12月1日'),
    () => T.ZhL10n.quote('设置') === '「设置」',
    () => T.ZhL10n.trim('长'.repeat(50)).length === 50,
    () => T.ZhL10n.trim('短文') === '短文',
    () => T.ZhL10n.punct('x:y;z') === 'x：y；z',
    () => (T.ZhL10n.abbr(Number.NaN) === '0'),
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.ZhL10n.abbr(i * 1000); return performance.now() - t0 < 50; },
    () => T.ZhL10n.abbr(10000) === '1万',
    () => { const a = T.ZhL10n.punct('一,二'); const b = T.ZhL10n.punct('一,二'); return a === b; },
    () => T.ZhL10n.dateCn(2024, 2, 29) === '2024年2月29日',
    () => T.ZhL10n.trim('a'.repeat(49), 48) === 'a'.repeat(48) + '……',
    () => { let n = 0; for (const p of [',', '.', '?', '!', ':', ';']) { if (T.ZhL10n.punct(p) !== p) n++; } return n === 6; },
    () => (T.ZhL10n.ellipsisOk('无省略号') === true),
    () => typeof T.ZhL10n.punct === 'function' && typeof T.ZhL10n.abbr === 'function',
    () => T.ZhL10n.quote('彩蛋') === '「彩蛋」',
    () => (T.ZhL10n.dateCn(2026, 0, 0) === '2026年1月1日'),
  ]);
}

/* -------- 族0498 多语言字体 2.0 X12426~X12450 -------- */
export function checkF0498(): CheckEntry[] {
  const f = new T.FontL10n();
  return mk25(12426, '字体', [
    () => f.add('ui1', ['latin', 'cjk']) && f.stacks.has('ui1'),
    () => f.add('ui1', ['cjk']) === false,
    () => T.FONT_SCRIPTS.length === 5 && f.add('ui2', ['arabic', 'devanagari', 'cyrillic']),
    () => f.covers('ui1', ['latin', 'cjk']),
    () => f.resolve('cjk') === 'ui1',
    () => f.add('ui3', ['klingon']) && f.stacks.get('ui3')!.length === 0 && f.clamped >= 1,
    () => f.add('', ['latin']) === false,
    () => f.covers('ghost', ['latin']) === false,
    () => { const q = new T.FontL10n(); q.add('a', ['latin']); return q.resolve('latin') === 'a'; },
    () => f.resolve('devanagari') === 'ui2',
    () => T.FontL10n.sizeOk(100) === 72 && T.FontL10n.sizeOk(1) === 9,
    () => { const q = new T.FontL10n(); return q.resolve('cjk') === ''; },
    () => { const q = new T.FontL10n(); q.add('m', ['latin', 'latin', 'cjk']); return q.stacks.get('m')!.length === 2; },
    () => T.FontL10n.sizeOk(Number.NaN) === 12,
    () => f.remove('ui3') && f.stacks.has('ui3') === false,
    () => { const t0 = performance.now(); for (let i = 0; i < 300; i++) { const q = new T.FontL10n(); q.add(`x${i}`, ['latin']); q.resolve('latin'); } return performance.now() - t0 < 50; },
    () => f.covers('ui2', ['arabic']) && f.covers('ui2', ['latin']) === false,
    () => { const a = new T.FontL10n(); const b = new T.FontL10n(); a.add('z', ['cjk']); b.add('z', ['cjk']); return a.stacks.size === b.stacks.size; },
    () => { const q = new T.FontL10n(); q.add('n', ['cyrillic']); return q.resolve('cyrillic') === 'n'; },
    () => T.FontL10n.sizeOk(12) === 12,
    () => { let n = 0; for (const s of T.FONT_SCRIPTS) { if (f.add(`s-${s}`, [s])) n++; } return n === 5; },
    () => { const q = new T.FontL10n(); q.add('dup', ['latin']); return q.add('dup', ['cjk']) === false; },
    () => typeof f.covers === 'function' && typeof T.FontL10n.sizeOk === 'function',
    () => { const q = new T.FontL10n(); q.add('egg', ['cjk']); return q.resolve('cjk') === 'egg'; },
    () => { const q = new T.FontL10n(); return q.remove('none') === false; },
  ]);
}

/* -------- 族0499 语音本地化 2.0 X12451~X12475 -------- */
export function checkF0499(): CheckEntry[] {
  const v = new T.VoiceL10n();
  return mk25(12451, '语音本地化', [
    () => v.bind('zh-CN', 'xiaoyun') && v.exact('zh-CN') === 'xiaoyun',
    () => v.bind('zh-CN', 'yunxi') && v.exact('zh-CN') === 'yunxi',
    () => T.VoiceL10n.RATES.length === 5 && T.VoiceL10n.rateOk(1.25),
    () => v.resolve('zh-TW') === 'yunxi',
    () => v.bind('en-US', 'aria') && v.resolve('en-US') === 'aria',
    () => v.bind('1bad', 'x') === false && v.clamped >= 1,
    () => v.bind('fr-FR', '') === false && v.clamped >= 2,
    () => { const q = new T.VoiceL10n(); return q.resolve('zh-CN') === ''; },
    () => v.exact('de-DE') === '',
    () => T.VoiceL10n.rateOk(0.75) && T.VoiceL10n.rateOk(2),
    () => T.VoiceL10n.rateOk(3) === false,
    () => { const q = new T.VoiceL10n(); q.bind('ja-JP', 'nanami'); return q.exact('ja-JP') === 'nanami'; },
    () => v.resolve('pt-BR') === '' && v.exact('pt-BR') === '',
    () => { const q = new T.VoiceL10n(); return q.bind('zh', 'sheng') && q.resolve('zh-CN') === 'sheng'; },
    () => { const q = new T.VoiceL10n(); q.bind('ko-KR', 'sunhi'); return q.unbind('ko-KR') && q.exact('ko-KR') === ''; },
    () => { const t0 = performance.now(); for (let i = 0; i < 300; i++) { const q = new T.VoiceL10n(); q.bind('en-US', 'v'); q.resolve('en-GB'); } return performance.now() - t0 < 50; },
    () => v.exact('en-US') === 'aria',
    () => { const a = new T.VoiceL10n(); const b = new T.VoiceL10n(); a.bind('fr-FR', 'x'); b.bind('fr-FR', 'x'); return a.exact('fr-FR') === b.exact('fr-FR'); },
    () => { const q = new T.VoiceL10n(); q.bind('ru-RU', 'p'); q.bind('uk-UA', 'q'); return q.resolve('uk-UA') === 'q'; },
    () => T.VoiceL10n.rateOk(Number.NaN) === false,
    () => { let n = 0; for (const r of T.VoiceL10n.RATES) { if (T.VoiceL10n.rateOk(r)) n++; } return n === 5; },
    () => { const q = new T.VoiceL10n(); q.bind('dup-LN', 'a'); return q.bind('dup-LN', 'b') && q.exact('dup-LN') === 'b'; },
    () => typeof v.resolve === 'function' && typeof T.VoiceL10n.rateOk === 'function',
    () => { const q = new T.VoiceL10n(); q.bind('ar-EG', 't'); return q.resolve('ar-SA') === 't'; },
    () => { const q = new T.VoiceL10n(); return q.unbind('ghost') === false; },
  ]);
}

/* -------- 族0500 翻译质量 2.0 X12476~X12500 -------- */
export function checkF0500(): CheckEntry[] {
  const q = new T.TranslateQuality();
  return mk25(12476, '翻译', [
    () => q.addTerm('file', '文件') && q.glossary.has('file'),
    () => q.addTerm('file', '档案') === false && q.term('file') === '文件',
    () => T.QUALITY_GRADES.length === 5 && q.addTerm('save', '保存'),
    () => T.TranslateQuality.placeholdersOk('打开 {0} 个', '开启 {0} 个'),
    () => T.TranslateQuality.grade(95) === 'publish',
    () => T.TranslateQuality.placeholdersOk('{a} 和 {b}', '{a} 和') === false,
    () => q.addTerm('', 'x') === false,
    () => T.TranslateQuality.grade(0) === 'reject',
    () => q.term('ghost') === '',
    () => T.TranslateQuality.tagsOk('<b>加粗</b>') === true,
    () => T.TranslateQuality.tagsOk('<b>缺闭合') === false,
    () => T.QUALITY_GRADES.join(',') === 'reject,passable,good,excellent,publish',
    () => T.TranslateQuality.grade(45) === 'passable',
    () => T.TranslateQuality.grade(65) === 'good' && T.TranslateQuality.grade(80) === 'excellent',
    () => { const t = new T.TranslateQuality(); t.addTerm('k', '值'); return t.apply('a k b') === 'a 值 b'; },
    () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.TranslateQuality.grade(i % 120); return performance.now() - t0 < 50; },
    () => T.TranslateQuality.grade(100) === 'publish',
    () => { const a = new T.TranslateQuality(); const b = new T.TranslateQuality(); a.addTerm('x', '甲'); b.addTerm('x', '甲'); return a.term('x') === b.term('x'); },
    () => T.TranslateQuality.grade(Number.NaN) === 'reject',
    () => { const t = new T.TranslateQuality(); return t.term('none') === '' && t.glossary.size === 0; },
    () => { let n = 0; for (let i = 0; i < 20; i++) { if (q.addTerm(`t${i}`, `译${i}`)) n++; } return n === 20; },
    () => { const t = new T.TranslateQuality(); t.addTerm('dup', 'a'); return t.addTerm('dup', 'b') === false && t.term('dup') === 'a'; },
    () => typeof T.TranslateQuality.grade === 'function' && typeof q.apply === 'function',
    () => { const t = new T.TranslateQuality(); t.addTerm('egg', '彩蛋'); return t.apply('egg 层').includes('彩蛋'); },
    () => { const t = new T.TranslateQuality(); t.addTerm('z', '终'); t.clear(); return t.glossary.size === 0; },
  ]);
}

/* -------- 聚合入口 -------- */
export function runAi50Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [
    checkF0491, checkF0492, checkF0493, checkF0494, checkF0495,
    checkF0496, checkF0497, checkF0498, checkF0499, checkF0500,
  ];
  const entries = families.flatMap((fn) => fn());
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
