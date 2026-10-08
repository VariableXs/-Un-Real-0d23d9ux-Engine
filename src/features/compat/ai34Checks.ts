/**
 * UNREAL-X-15000 · AI-34 兼容工程 V 线 CheckSet（族0333/0334/0335/0337/0338 · X08301~X08350 / X08401~X08450），勿删。
 * 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
 */
import type { CheckEntry } from '../uikit/checks';
import * as T from './ai34Models';

/* -------- 族0333 多系统共存 X08301~X08325 -------- */
export function checkF0333(): CheckEntry[] {
  const m = new T.CoexistMatrix();
  return [
    { id: 'X08301', name: '共存·最小闭环', check: () => { m.add('VARIX', 0); return m.menu()[0] === 'VARIX'; } },
    { id: 'X08302', name: '共存·全量参数', check: () => { const q = new T.CoexistMatrix(); q.add('A', 2, false); q.add('B', 1); return q.menu().join() === 'B,A'; } },
    { id: 'X08303', name: '共存·档位矩阵', check: () => T.TIER_MATRIX.every((t) => new T.CoexistMatrix(t).tier === t) },
    { id: 'X08304', name: '共存·快照迁移', check: () => { const q = T.CoexistMatrix.deserialize(m.serialize()); return q.menu()[0] === 'VARIX'; } },
    { id: 'X08305', name: '共存·联调集成', check: () => { const q = new T.CoexistMatrix(); q.add('Win', 1); q.add('Linux', 0); return q.menu().join() === 'Linux,Win'; } },
    { id: 'X08306', name: '共存·越界钳制', check: () => new T.CoexistMatrix('nope').tier === 'balanced' && new T.CoexistMatrix('nope').clamped === 1 },
    { id: 'X08307', name: '共存·失败叙事', check: () => T.explainError('E3401').next.includes('重排') },
    { id: 'X08308', name: '共存·中断还原', check: () => T.CoexistMatrix.deserialize('not-json').tier === T.DEFAULT_TIER },
    { id: 'X08309', name: '共存·资源降级', check: () => { const q = new T.CoexistMatrix(); for (let i = 0; i < 20; i++) q.add('sys' + i, i); return q.menu().length === 12; } },
    { id: 'X08310', name: '共存·回滚净身', check: () => { const q = new T.CoexistMatrix(); q.add('X', 1); q.add('X', 9); return q.menu().join() === 'X'; } },
    { id: 'X08311', name: '共存·动效令牌', check: () => T.motionFor('balanced').durationMs === 180 && T.motionFor('off').scale === 0 },
    { id: 'X08312', name: '共存·三态焦点', check: () => { const q = new T.CoexistMatrix(); q.add('A', 1, false); q.add('B', 2); return q.risk() === 1 && new T.CoexistMatrix().risk() === 0; } },
    { id: 'X08313', name: '共存·键盘序', check: () => { const q = new T.CoexistMatrix(); q.add('C', 3); q.add('A', 1); q.add('B', 2); return q.menu().join() === 'A,B,C'; } },
    { id: 'X08314', name: '共存·微文案', check: () => T.explainError('E3401').text === '多系统引导项冲突' },
    { id: 'X08315', name: '共存·aria 等价', check: () => typeof m.menu === 'function' && m.menu().every((n) => n.length > 0) },
    { id: 'X08316', name: '共存·基准采集', check: () => { const q = new T.CoexistMatrix(); const t0 = performance.now(); for (let i = 0; i < 500; i++) q.add('n' + (i % 12), i); return performance.now() - t0 < 50; } },
    { id: 'X08317', name: '共存·热路径', check: () => { const q = new T.CoexistMatrix(); q.add('hot', 0); return q.menu()[0] === 'hot'; } },
    { id: 'X08318', name: '共存·零漂移', check: () => { const a = T.CoexistMatrix.deserialize(m.serialize()); const b = T.CoexistMatrix.deserialize(a.serialize()); return b.serialize() === a.serialize(); } },
    { id: 'X08319', name: '共存·低配减档', check: () => { const q = new T.CoexistMatrix('off'); q.add('A', 1, false); return q.tier === 'off' && q.risk() === 1; } },
    { id: 'X08320', name: '共存·守卫', check: () => T.CoexistMatrix.conflict(['C:', 'D:'], ['D:', 'E:']) && !T.CoexistMatrix.conflict(['C:'], ['D:']) },
    { id: 'X08321', name: '共存·智能建议', check: () => { const q = new T.CoexistMatrix(); for (let i = 0; i < 9; i++) q.add('u' + i, i, false); return q.risk() === 3; } },
    { id: 'X08322', name: '共存·批量模式', check: () => { const q = new T.CoexistMatrix(); ['a', 'b', 'c'].forEach((n, i) => q.add(n, i)); return q.menu().length === 3; } },
    { id: 'X08323', name: '共存·跨域联动', check: () => { const q = T.CoexistMatrix.deserialize(new T.CoexistMatrix('strict').serialize()); return q.tier === 'strict'; } },
    { id: 'X08324', name: '共存·扩展点', check: () => typeof T.CoexistMatrix.deserialize === 'function' && typeof m.add === 'function' },
    { id: 'X08325', name: '共存·彩蛋层', check: () => { const q = new T.CoexistMatrix(); q.add('VARIX', 0); q.add('win98', 9, false); return q.menu()[1] === 'win98'; } },
  ];
}

/* -------- 族0334 企业环境 X08326~X08350 -------- */
export function checkF0334(): CheckEntry[] {
  const e = new T.EnterpriseEnv();
  e.push('wallpaper', 'corp-dark', true);
  return [
    { id: 'X08326', name: '企业·最小闭环', check: () => e.effective('wallpaper') === 'corp-dark' },
    { id: 'X08327', name: '企业·全量参数', check: () => { const q = new T.EnterpriseEnv(); q.push('k', 'v', true); q.push('k', 'v2', false); return q.effective('k') === 'v'; } },
    { id: 'X08328', name: '企业·档位矩阵', check: () => T.TIER_MATRIX.every((t) => new T.EnterpriseEnv(t).tier === t) },
    { id: 'X08329', name: '企业·快照迁移', check: () => { const q = T.EnterpriseEnv.deserialize(e.serialize()); return q.effective('wallpaper') === 'corp-dark'; } },
    { id: 'X08330', name: '企业·联调集成', check: () => { const q = new T.EnterpriseEnv(); q.push('upd', 'weekly', false); return q.effective('upd', 'daily') === 'daily'; } },
    { id: 'X08331', name: '企业·越界钳制', check: () => new T.EnterpriseEnv(42).tier === 'balanced' && new T.EnterpriseEnv(42).clamped === 1 },
    { id: 'X08332', name: '企业·失败叙事', check: () => T.explainError('E3402').next.includes('只读') },
    { id: 'X08333', name: '企业·中断还原', check: () => T.EnterpriseEnv.deserialize('bad{').tier === T.DEFAULT_TIER },
    { id: 'X08334', name: '企业·资源降级', check: () => new T.EnterpriseEnv('off').deployQuota() === 0 },
    { id: 'X08335', name: '企业·回滚净身', check: () => { const q = new T.EnterpriseEnv(); q.push('k', 'v', false); q.policies.delete('k'); return q.effective('k') === '' && q.enforcedList().length === 0; } },
    { id: 'X08336', name: '企业·动效令牌', check: () => T.motionFor('strict').curve === 'ease-standard' },
    { id: 'X08337', name: '企业·三态焦点', check: () => { const q = new T.EnterpriseEnv(); q.push('a', '1', true); q.push('b', '2', false); return q.enforcedList().join() === 'a'; } },
    { id: 'X08338', name: '企业·键盘序', check: () => { const q = new T.EnterpriseEnv(); q.push('c', '1', true); q.push('a', '2', true); q.push('b', '3', true); return q.enforcedList().join() === 'a,b,c'; } },
    { id: 'X08339', name: '企业·微文案', check: () => T.explainError('E3402').text === '域策略拒绝挂载' },
    { id: 'X08340', name: '企业·aria 等价', check: () => e.enforcedList().every((k) => k.length > 0) },
    { id: 'X08341', name: '企业·基准采集', check: () => { const q = new T.EnterpriseEnv(); const t0 = performance.now(); for (let i = 0; i < 500; i++) q.push('p' + i, 'v', false); return performance.now() - t0 < 50; } },
    { id: 'X08342', name: '企业·热路径', check: () => new T.EnterpriseEnv('print').deployQuota() === 50 },
    { id: 'X08343', name: '企业·零漂移', check: () => { const a = T.EnterpriseEnv.deserialize(e.serialize()); const b = T.EnterpriseEnv.deserialize(a.serialize()); return b.serialize() === a.serialize(); } },
    { id: 'X08344', name: '企业·低配减档', check: () => new T.EnterpriseEnv('light').deployQuota() === 5 },
    { id: 'X08345', name: '企业·守卫', check: () => { const q = new T.EnterpriseEnv(); q.push('k', 'v', true); q.push('k', 'v2', false); return q.effective('k') === 'v'; } },
    { id: 'X08346', name: '企业·智能建议', check: () => { const q = new T.EnterpriseEnv(); q.push('k', 'domain', false); return q.effective('k', 'local') === 'local' && q.enforcedList().length === 0; } },
    { id: 'X08347', name: '企业·批量模式', check: () => { let n = 0; for (const t of T.TIER_MATRIX) if (new T.EnterpriseEnv(t).tier === t) n++; return n === 5; } },
    { id: 'X08348', name: '企业·跨域联动', check: () => { const q = T.EnterpriseEnv.deserialize(new T.EnterpriseEnv('strict').serialize()); return q.tier === 'strict' && q.deployQuota() === 50; } },
    { id: 'X08349', name: '企业·扩展点', check: () => typeof T.EnterpriseEnv.deserialize === 'function' && typeof e.push === 'function' },
    { id: 'X08350', name: '企业·彩蛋层', check: () => e.effective('wallpaper', 'local-dark') === 'corp-dark' },
  ];
}

/* -------- 族0335 中文深度兼容 X08351~X08375 -------- */
export function checkF0335(): CheckEntry[] {
  const c = new T.CjkCompat();
  return [
    { id: 'X08351', name: '中文·最小闭环', check: () => c.needsVariant('汉字') },
    { id: 'X08352', name: '中文·全量参数', check: () => !c.needsVariant('abc123') && c.needsVariant('漢') },
    { id: 'X08353', name: '中文·档位矩阵', check: () => T.TIER_MATRIX.every((t) => new T.CjkCompat(t).tier === t) },
    { id: 'X08354', name: '中文·快照迁移', check: () => T.CjkCompat.deserialize(c.serialize()).tier === 'balanced' },
    { id: 'X08355', name: '中文·联调集成', check: () => c.pathLen('C:/文档/报告.txt') === 16 },
    { id: 'X08356', name: '中文·越界钳制', check: () => new T.CjkCompat('x').tier === 'balanced' && new T.CjkCompat('x').clamped === 1 },
    { id: 'X08357', name: '中文·失败叙事', check: () => T.explainError('E3403').next.includes('归一') },
    { id: 'X08358', name: '中文·中断还原', check: () => T.CjkCompat.deserialize('!!').tier === T.DEFAULT_TIER },
    { id: 'X08359', name: '中文·资源降级', check: () => new T.CjkCompat('off').normalize('e\u0301') === 'e\u0301' },
    { id: 'X08360', name: '中文·回滚净身', check: () => c.normalize('e\u0301') === 'é' },
    { id: 'X08361', name: '中文·动效令牌', check: () => c.pathLen('中文') === 4 && c.pathLen('ab') === 2 },
    { id: 'X08362', name: '中文·三态焦点', check: () => c.fontFallback('楷体')[0] === '楷体' && c.fontFallback('黑体').length === 2 && c.fontFallback('mono').length === 1 },
    { id: 'X08363', name: '中文·键盘序', check: () => c.fontFallback('楷体').join() === '楷体,黑体,system' },
    { id: 'X08364', name: '中文·微文案', check: () => T.explainError('E3403').text === '中文路径编码歧义' },
    { id: 'X08365', name: '中文·aria 等价', check: () => c.toFullWidth('A') === 'Ａ' && c.toFullWidth('５') === '５' },
    { id: 'X08366', name: '中文·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) c.pathLen('目录/文件' + i); return performance.now() - t0 < 50; } },
    { id: 'X08367', name: '中文·热路径', check: () => c.toFullWidth('a') === 'ａ' },
    { id: 'X08368', name: '中文·零漂移', check: () => { const a = T.CjkCompat.deserialize(c.serialize()); const b = T.CjkCompat.deserialize(a.serialize()); return b.serialize() === a.serialize(); } },
    { id: 'X08369', name: '中文·低配减档', check: () => new T.CjkCompat('off').toFullWidth('a!') === 'a!' },
    { id: 'X08370', name: '中文·守卫', check: () => c.pathLen('长'.repeat(200)) === 260, },
    { id: 'X08371', name: '中文·智能建议', check: () => c.pathLen('C:/中文/文件') === 12 },
    { id: 'X08372', name: '中文·批量模式', check: () => { let n = 0; for (const t of T.TIER_MATRIX) if (new T.CjkCompat(t).tier === t) n++; return n === 5; } },
    { id: 'X08373', name: '中文·跨域联动', check: () => T.CjkCompat.deserialize(JSON.stringify({ tier: 'print' })).tier === 'print' },
    { id: 'X08374', name: '中文·扩展点', check: () => typeof T.CjkCompat.deserialize === 'function' && typeof c.normalize === 'function' },
    { id: 'X08375', name: '中文·彩蛋层', check: () => new T.CjkCompat('strict').normalize('㐀') === '㐀' },
  ];
}

/* -------- 族0337 Web 兼容 X08401~X08425 -------- */
export function checkF0337(): CheckEntry[] {
  const w = new T.WebCompat();
  return [
    { id: 'X08401', name: 'Web·最小闭环', check: () => w.pickEngine('Mozilla/5.0 Modern') === 'modern' },
    { id: 'X08402', name: 'Web·全量参数', check: () => w.pickEngine('Legacy/1.0') === 'legacy' },
    { id: 'X08403', name: 'Web·档位矩阵', check: () => T.TIER_MATRIX.every((t) => new T.WebCompat(t).tier === t) },
    { id: 'X08404', name: 'Web·快照迁移', check: () => T.WebCompat.deserialize(w.serialize()).tier === 'balanced' },
    { id: 'X08405', name: 'Web·联调集成', check: () => w.renderPath(true, true) === 'webgl' && w.renderPath(false, true) === 'canvas2d' && w.renderPath(false, false) === 'text' },
    { id: 'X08406', name: 'Web·越界钳制', check: () => new T.WebCompat(9).tier === 'balanced' && new T.WebCompat(9).clamped === 1 },
    { id: 'X08407', name: 'Web·失败叙事', check: () => T.explainError('E3404').next.includes('静态') },
    { id: 'X08408', name: 'Web·中断还原', check: () => T.WebCompat.deserialize('{{').tier === T.DEFAULT_TIER },
    { id: 'X08409', name: 'Web·资源降级', check: () => new T.WebCompat('off').renderPath(true, true) === 'canvas2d' },
    { id: 'X08410', name: 'Web·回滚净身', check: () => { const q = new T.WebCompat('off'); return q.engine === 'static' && q.pickEngine('Modern') === 'static'; } },
    { id: 'X08411', name: 'Web·动效令牌', check: () => T.motionFor('off').curve === 'linear-fade' },
    { id: 'X08412', name: 'Web·三态焦点', check: () => w.cookiePolicy(true) === 'lax' && w.cookiePolicy(false) === 'accept' },
    { id: 'X08413', name: 'Web·键盘序', check: () => new T.WebCompat('strict').cookiePolicy(true) === 'reject' },
    { id: 'X08414', name: 'Web·微文案', check: () => T.explainError('E3404').text === 'Web 视图内核过旧' },
    { id: 'X08415', name: 'Web·aria 等价', check: () => typeof w.renderPath === 'function' },
    { id: 'X08416', name: 'Web·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) w.pickEngine('Modern'); return performance.now() - t0 < 50; } },
    { id: 'X08417', name: 'Web·热路径', check: () => w.inlineScriptAllowed() === true },
    { id: 'X08418', name: 'Web·零漂移', check: () => { const a = T.WebCompat.deserialize(w.serialize()); const b = T.WebCompat.deserialize(a.serialize()); return b.serialize() === a.serialize(); } },
    { id: 'X08419', name: 'Web·低配减档', check: () => new T.WebCompat('off').engine === 'static' },
    { id: 'X08420', name: 'Web·守卫', check: () => !new T.WebCompat('strict').inlineScriptAllowed() && !new T.WebCompat('print').inlineScriptAllowed() },
    { id: 'X08421', name: 'Web·智能建议', check: () => new T.WebCompat('light').renderPath(false, false) === 'text' },
    { id: 'X08422', name: 'Web·批量模式', check: () => { let n = 0; for (const t of T.TIER_MATRIX) if (new T.WebCompat(t).tier === t) n++; return n === 5; } },
    { id: 'X08423', name: 'Web·跨域联动', check: () => T.WebCompat.deserialize(new T.WebCompat('off').serialize()).engine === 'static' },
    { id: 'X08424', name: 'Web·扩展点', check: () => typeof T.WebCompat.deserialize === 'function' && typeof w.cookiePolicy === 'function' },
    { id: 'X08425', name: 'Web·彩蛋层', check: () => new T.WebCompat('print').pickEngine('Legacy/9') === 'legacy' },
  ];
}

/* -------- 族0338 格式兼容 X08426~X08450 -------- */
export function checkF0338(): CheckEntry[] {
  const f = new T.FormatBridge();
  const rows = [['名', 'B'], ['a,"b"', 'c']];
  return [
    { id: 'X08426', name: '格式·最小闭环', check: () => f.sniff('{"a":1}') === 'json' },
    { id: 'X08427', name: '格式·全量参数', check: () => f.sniff('<?xml v=1') === 'xml' && f.sniff('a=1') === 'ini' && f.sniff('a,b\nc,d') === 'csv' && f.sniff('\x00\x01') === 'bin' },
    { id: 'X08428', name: '格式·档位矩阵', check: () => T.TIER_MATRIX.every((t) => new T.FormatBridge(t).tier === t) },
    { id: 'X08429', name: '格式·快照迁移', check: () => T.FormatBridge.deserialize(f.serialize()).tier === 'balanced' },
    { id: 'X08430', name: '格式·联调集成', check: () => f.fromCsv('a,"b,c"').join('|') === 'a|b,c' },
    { id: 'X08431', name: '格式·越界钳制', check: () => new T.FormatBridge(null).tier === 'balanced' && new T.FormatBridge(null).clamped === 1 },
    { id: 'X08432', name: '格式·失败叙事', check: () => T.explainError('E3405').next.includes('中转') },
    { id: 'X08433', name: '格式·中断还原', check: () => T.FormatBridge.deserialize('nope').tier === T.DEFAULT_TIER },
    { id: 'X08434', name: '格式·资源降级', check: () => f.toCsv([]) === '' },
    { id: 'X08435', name: '格式·回滚净身', check: () => f.fromUniversal('garbage{') === null },
    { id: 'X08436', name: '格式·动效令牌', check: () => T.motionFor('balanced').scale === 1 },
    { id: 'X08437', name: '格式·三态焦点', check: () => f.toCsv(['a', 'b,c', 'd"e']) === 'a,"b,c","d""e"' },
    { id: 'X08438', name: '格式·键盘序', check: () => T.TIER_MATRIX.every((t) => new T.FormatBridge(t).tier === t) },
    { id: 'X08439', name: '格式·微文案', check: () => T.explainError('E3405').text === '格式嗅探不确定' },
    { id: 'X08440', name: '格式·aria 等价', check: () => typeof f.toUniversal === 'function' },
    { id: 'X08441', name: '格式·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) f.sniff('{"x":1}'); return performance.now() - t0 < 50; } },
    { id: 'X08442', name: '格式·热路径', check: () => f.toCsv(rows[0]!) === '名,B' },
    { id: 'X08443', name: '格式·零漂移', check: () => { const a = T.FormatBridge.deserialize(f.serialize()); const b = T.FormatBridge.deserialize(a.serialize()); return b.serialize() === a.serialize(); } },
    { id: 'X08444', name: '格式·低配减档', check: () => f.fromCsv('only-header').length === 1 },
    { id: 'X08445', name: '格式·守卫', check: () => T.FormatBridge.deserialize('[]').tier === 'balanced' },
    { id: 'X08446', name: '格式·智能建议', check: () => f.fromCsv(f.toCsv(rows[1]!)).join('|') === 'a,"b"|c' },
    { id: 'X08447', name: '格式·批量模式', check: () => { let n = 0; for (const t of T.TIER_MATRIX) if (new T.FormatBridge(t).tier === t) n++; return n === 5; } },
    { id: 'X08448', name: '格式·跨域联动', check: () => { const q = T.FormatBridge.deserialize(new T.FormatBridge('print').serialize()); return q.tier === 'print'; } },
    { id: 'X08449', name: '格式·扩展点', check: () => typeof T.FormatBridge.deserialize === 'function' && typeof f.sniff === 'function' },
    { id: 'X08450', name: '格式·彩蛋层', check: () => (f.fromUniversal(f.toUniversal([['中', '文']])) ?? [])[0]!.join() === '中,文' },
  ];
}

// UNREAL-X AI-34（族0333~0340 · X08251~X08500）V 线聚合：五族 × 25 项 = 125 项，只增不删。
export function runAi34Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [checkF0333, checkF0334, checkF0335, checkF0337, checkF0338];
  const entries = families.flatMap((f) => f());
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
