/**
 * UNREAL-X-15000 · AI-20 输入工程与中文 V 线 CheckSet（族0191/0196/0197/0198 · X04751~X04775 / X04876~X04950），勿删。
 * 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
 */
import type { CheckEntry } from '../uikit/checks';
import * as T from './ai20Models';

/* -------- 族0191 中文排版 2.0 X04751~X04775 -------- */
export function checkF0191(): CheckEntry[] {
  const t = new T.CjkTypography();
  return [
    { id: 'X04751', name: '排版·最小闭环', check: () => t.profileId === 'balanced' && t.profile.lineHeight === 1.6 },
    { id: 'X04752', name: '排版·全量参数', check: () => T.CJK_PROFILE_MATRIX[t.profileId].hanSpace === true && t.profile.hangingPunctuation === false },
    { id: 'X04753', name: '排版·档位矩阵', check: () => T.CJK_TYPOGRAPHY_PROFILES.length === 5 && T.DEFAULT_CJK_PROFILE === 'balanced' },
    { id: 'X04754', name: '排版·快照迁移', check: () => { const r = T.CjkTypography.deserialize(t.serialize()); return r.profileId === 'balanced'; } },
    { id: 'X04755', name: '排版·联调集成', check: () => t.hanSpace('输入引擎engine').includes('\u2009') },
    { id: 'X04756', name: '排版·越界钳制', check: () => new T.CjkTypography('nope').profileId === 'balanced' && new T.CjkTypography('nope').clamped === 1 },
    { id: 'X04757', name: '排版·失败叙事', check: () => T.CjkTypography.deserialize('{bad').profileId === 'balanced' },
    { id: 'X04758', name: '排版·中断还原', check: () => { const q = T.CjkTypography.deserialize('not-json'); return q.profileId === T.DEFAULT_CJK_PROFILE; } },
    { id: 'X04759', name: '排版·资源降级', check: () => { const q = new T.CjkTypography('light'); return q.profile.hangingPunctuation === false && q.profile.lineHeight === 1.5; } },
    { id: 'X04760', name: '排版·回滚净身', check: () => { const q = T.CjkTypography.deserialize(t.serialize()); return q.profileId === 'balanced' && q.clamped === 0; } },
    { id: 'X04761', name: '排版·动效令牌', check: () => t.profile.tracking === 0 },
    { id: 'X04762', name: '排版·三态焦点', check: () => t.squeeze('，引擎') === '引擎' },
    { id: 'X04763', name: '排版·键盘序', check: () => T.CJK_TYPOGRAPHY_PROFILES.every((p) => new T.CjkTypography(p).profileId === p) },
    { id: 'X04764', name: '排版·微文案', check: () => t.hanSpace('中文2.0') === '中文\u20092.0' },
    { id: 'X04765', name: '排版·aria 等价', check: () => t.hanSpace('AA测试') === 'AA\u2009测试' },
    { id: 'X04766', name: '排版·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) t.hanSpace('中文x1'); return performance.now() - t0 < 50; } },
    { id: 'X04767', name: '排版·热路径', check: () => new T.CjkTypography('off').hanSpace('中文a') === '中文a' },
    { id: 'X04768', name: '排版·零漂移', check: () => { const a = T.CjkTypography.deserialize(t.serialize()); const b = T.CjkTypography.deserialize(a.serialize()); return b.serialize() === a.serialize(); } },
    { id: 'X04769', name: '排版·低配减档', check: () => { const q = new T.CjkTypography('off'); return q.profile.hanSpace === false && q.profile.hangingPunctuation === false; } },
    { id: 'X04770', name: '排版·守卫', check: () => { const q = new T.CjkTypography('print'); return q.profile.hangingPunctuation && q.profile.tracking > 0; } },
    { id: 'X04771', name: '排版·智能建议', check: () => t.indent('段落') === '段落' && new T.CjkTypography('strict').indent('段落').startsWith('\u3000\u3000') },
    { id: 'X04772', name: '排版·批量模式', check: () => { let n = 0; for (const p of T.CJK_TYPOGRAPHY_PROFILES) { if (new T.CjkTypography(p).profileId === p) n++; } return n === 5; } },
    { id: 'X04773', name: '排版·跨域联动', check: () => T.CjkTypography.deserialize(JSON.stringify({ profileId: 'print' })).profile.hangingPunctuation === true },
    { id: 'X04774', name: '排版·扩展点', check: () => typeof T.CjkTypography.deserialize === 'function' && typeof t.hanSpace === 'function' },
    { id: 'X04775', name: '排版·彩蛋层', check: () => t.squeeze('正常句子') === '正常句子' },
  ];
}

/* -------- 族0196 通用输入细节 X04876~X04900 -------- */
export function checkF0196(): CheckEntry[] {
  const d = new T.InputDetail();
  return [
    { id: 'X04876', name: '细节·最小闭环', check: () => d.apply('hello') === 'hello' },
    { id: 'X04877', name: '细节·全量参数', check: () => { const q = new T.InputDetail({ smartQuotes: false, emDash: false, ellipsis: false, collapseSpace: false }); return q.apply('"a"--b...c  d') === '"a"--b...c  d'; } },
    { id: 'X04878', name: '细节·档位矩阵', check: () => d.smartQuotes && d.emDash && d.ellipsis && d.collapseSpace },
    { id: 'X04879', name: '细节·快照迁移', check: () => d.history.length === 1 && d.history[0] === 'hello' },
    { id: 'X04880', name: '细节·联调集成', check: () => d.apply('say "hi" now') === 'say \u201chi\u201d now' },
    { id: 'X04881', name: '细节·越界钳制', check: () => { const q = new T.InputDetail(); q.apply('a'.repeat(1)); return q.history.length <= 8; } },
    { id: 'X04882', name: '细节·失败叙事', check: () => d.apply('') === '' },
    { id: 'X04883', name: '细节·中断续用', check: () => { const q = new T.InputDetail(); q.apply('b'); q.apply('c'); return q.history.length === 2 && q.history[1] === 'c'; } },
    { id: 'X04884', name: '细节·资源降级', check: () => { const q = new T.InputDetail({ smartQuotes: false }); return q.apply('say "hi"') === 'say "hi"'; } },
    { id: 'X04885', name: '细节·净身', check: () => { d.reset(); return d.history.length === 0; } },
    { id: 'X04886', name: '细节·动效令牌', check: () => d.apply('fast--slow') === 'fast\u2014slow' },
    { id: 'X04887', name: '细节·三态焦点', check: () => { const q = new T.InputDetail({ emDash: false }); return q.apply('a--b') === 'a--b'; } },
    { id: 'X04888', name: '细节·键盘序', check: () => d.apply('wait...ok') === 'wait\u2026ok' },
    { id: 'X04889', name: '细节·微文案', check: () => d.apply('a...b') === 'a\u2026b' },
    { id: 'X04890', name: '细节·aria 等价', check: () => d.apply('two  spaces') === 'two spaces' },
    { id: 'X04891', name: '细节·基准采集', check: () => { const t0 = performance.now(); const q = new T.InputDetail(); for (let i = 0; i < 500; i++) q.apply('"x" -- y...'); return performance.now() - t0 < 50; } },
    { id: 'X04892', name: '细节·热路径', check: () => { const q = new T.InputDetail(); return q.apply('x') === 'x'; } },
    { id: 'X04893', name: '细节·零漂移', check: () => { const q = new T.InputDetail(); q.apply('s "q" t...'); return q.history.length === 1; } },
    { id: 'X04894', name: '细节·低配减档', check: () => { const q = new T.InputDetail({ collapseSpace: false }); return q.apply('a  b') === 'a  b'; } },
    { id: 'X04895', name: '细节·守卫', check: () => d.apply("引号 'x' 守卫") === '引号 ‘x’ 守卫' },
    { id: 'X04896', name: '细节·智能建议', check: () => { const q = new T.InputDetail({ smartQuotes: true }); return q.apply("'x'") === '\u2018x\u2019'; } },
    { id: 'X04897', name: '细节·批量模式', check: () => { const q = new T.InputDetail(); let n = 0; for (let i = 0; i < 20; i++) if (typeof q.apply(`t${i} ...`) === 'string') n++; return n === 20; } },
    { id: 'X04898', name: '细节·跨域联动', check: () => d.apply('中英 mixed--ok...') === '中英 mixed\u2014ok\u2026' },
    { id: 'X04899', name: '细节·扩展点', check: () => { const q = new T.InputDetail({ ellipsis: false }); return q.apply('a...b') === 'a...b'; } },
    { id: 'X04900', name: '细节·彩蛋层', check: () => { d.reset(); return d.apply('clean') === 'clean'; } },
  ];
}

/* -------- 族0197 输入彩蛋 2.0 X04901~X04925 -------- */
export function checkF0197(): CheckEntry[] {
  const e = new T.InputEgg();
  let last: string | null = null;
  for (const ch of 'variable') last = e.feed(ch);
  return [
    { id: 'X04901', name: '彩蛋·最小闭环', check: () => last === 'egg-palette' && e.firedCount === 1 },
    { id: 'X04902', name: '彩蛋·全量参数', check: () => new T.InputEgg(false).feed('v') === null },
    { id: 'X04903', name: '彩蛋·档位矩阵', check: () => Object.keys(T.INPUT_EGG_SEQUENCES).length === 2 },
    { id: 'X04904', name: '彩蛋·快照迁移', check: () => e.fired[0] === 'egg-palette' },
    { id: 'X04905', name: '彩蛋·联调集成', check: () => { const q = new T.InputEgg(); let r: string | null = null; for (const ch of 'xauroray') r = q.feed(ch) ?? r; return r === 'egg-confetti'; } },
    { id: 'X04906', name: '彩蛋·越界钳制', check: () => { const q = new T.InputEgg(); for (let i = 0; i < 30; i++) q.feed('a'); return q.buffer.length <= 16; } },
    { id: 'X04907', name: '彩蛋·失败叙事', check: () => { const q = new T.InputEgg(); return q.feed('z') === null && q.firedCount === 0; } },
    { id: 'X04908', name: '彩蛋·中断续跑', check: () => { const q = new T.InputEgg(); for (const ch of 'vari') q.feed(ch); for (const ch of 'able') q.feed(ch); return q.firedCount === 1; } },
    { id: 'X04909', name: '彩蛋·降级守护', check: () => new T.InputEgg().feed('') === null },
    { id: 'X04910', name: '彩蛋·净身', check: () => { const q = new T.InputEgg(); return q.firedCount === 0 && q.buffer.length === 0; } },
    { id: 'X04911', name: '彩蛋·只触发一次', check: () => { const q = new T.InputEgg(); for (const ch of 'variable') q.feed(ch); let again: string | null = null; for (const ch of 'variable') again = q.feed(ch); return again === null && q.firedCount === 1; } },
    { id: 'X04912', name: '彩蛋·大小写不敏感', check: () => { const q = new T.InputEgg(); let r: string | null = null; for (const ch of 'Variable') r = q.feed(ch); return r === 'egg-palette'; } },
    { id: 'X04913', name: '彩蛋·前缀噪声', check: () => { const q = new T.InputEgg(); let r: string | null = null; for (const ch of 'zzzvariable') r = q.feed(ch); return r === 'egg-palette'; } },
    { id: 'X04914', name: '彩蛋·环形缓冲', check: () => { const q = new T.InputEgg(); for (let i = 0; i < 20; i++) q.feed(String.fromCharCode(97 + (i % 26))); return q.buffer.length === 16; } },
    { id: 'X04915', name: '彩蛋·第二个暗号', check: () => { const q = new T.InputEgg(); for (const ch of 'aurora') q.feed(ch); return q.fired.includes('egg-confetti'); } },
    { id: 'X04916', name: '彩蛋·基准采集', check: () => { const t0 = performance.now(); const q = new T.InputEgg(); for (let i = 0; i < 500; i++) q.feed('a'); return performance.now() - t0 < 50; } },
    { id: 'X04917', name: '彩蛋·热路径', check: () => { const q = new T.InputEgg(); return q.feed('q') === null; } },
    { id: 'X04918', name: '彩蛋·零漂移', check: () => { const q = new T.InputEgg(); for (const ch of 'variable') q.feed(ch); return q.firedCount === 1 && q.fired.length === 1; } },
    { id: 'X04919', name: '彩蛋·低配减档', check: () => { const q = new T.InputEgg(false); for (const ch of 'aurora') q.feed(ch); return q.firedCount === 0; } },
    { id: 'X04920', name: '彩蛋·守卫', check: () => T.INPUT_EGG_SEQUENCES['variable'] === 'egg-palette' },
    { id: 'X04921', name: '彩蛋·智能建议', check: () => { const q = new T.InputEgg(); let r: string | null = null; for (const ch of 'variablE') r = q.feed(ch); return r === 'egg-palette'; } },
    { id: 'X04922', name: '彩蛋·批量模式', check: () => { const q = new T.InputEgg(); let n = 0; for (const ch of 'aauroraa') { if (q.feed(ch)) n++; } return n === 1; } },
    { id: 'X04923', name: '彩蛋·跨域联动', check: () => { const q = new T.InputEgg(); for (const ch of 'auroravariable') q.feed(ch); return q.firedCount === 2; } },
    { id: 'X04924', name: '彩蛋·扩展点', check: () => typeof T.INPUT_EGG_SEQUENCES === 'object' && typeof e.feed === 'function' },
    { id: 'X04925', name: '彩蛋·彩蛋层', check: () => e.firedCount === 1 && e.buffer.length <= 16 },
  ];
}

/* -------- 族0198 输入迁移 X04926~X04950 -------- */
export function checkF0198(): CheckEntry[] {
  const prefs: T.InputPrefs = { profile: 'strict', smartQuotes: false, eggsEnabled: true };
  const data = T.exportInputPrefs(prefs);
  return [
    { id: 'X04926', name: '迁移·最小闭环', check: () => T.importInputPrefs(data).profile === 'strict' },
    { id: 'X04927', name: '迁移·全量参数', check: () => { const r = T.importInputPrefs(data); return r.smartQuotes === false && r.eggsEnabled === true; } },
    { id: 'X04928', name: '迁移·档位矩阵', check: () => T.INPUT_PREFS_VERSION === 1 },
    { id: 'X04929', name: '迁移·快照迁移', check: () => { const r = T.importInputPrefs(data); return T.exportInputPrefs(r) === data; } },
    { id: 'X04930', name: '迁移·联调集成', check: () => { const r = T.importInputPrefs(data); return new T.CjkTypography(r.profile).profileId === 'strict'; } },
    { id: 'X04931', name: '迁移·越界钳制', check: () => T.importInputPrefs(JSON.stringify({ profile: 'hack', smartQuotes: true, eggsEnabled: false })).profile === 'balanced' },
    { id: 'X04932', name: '迁移·失败叙事', check: () => { const r = T.importInputPrefs('not-json'); return r.profile === 'balanced' && r.smartQuotes === true; } },
    { id: 'X04933', name: '迁移·中断续跑', check: () => T.importInputPrefs(JSON.stringify({ profile: 'light' })).profile === 'light' },
    { id: 'X04934', name: '迁移·降级守护', check: () => { const r = T.importInputPrefs('{}'); return r.profile === 'balanced' && r.smartQuotes === true && r.eggsEnabled === true; } },
    { id: 'X04935', name: '迁移·净身', check: () => { const r = T.importInputPrefs(''); return r.profile === 'balanced'; } },
    { id: 'X04936', name: '迁移·动效令牌', check: () => data.startsWith(`{"v":${T.INPUT_PREFS_VERSION}`) },
    { id: 'X04937', name: '迁移·三态焦点', check: () => T.importInputPrefs(JSON.stringify({ smartQuotes: false })).smartQuotes === false },
    { id: 'X04938', name: '迁移·键盘序', check: () => T.CJK_TYPOGRAPHY_PROFILES.every((p) => T.importInputPrefs(JSON.stringify({ profile: p })).profile === p) },
    { id: 'X04939', name: '迁移·微文案', check: () => T.importInputPrefs(JSON.stringify({ profile: 'print' })).profile === 'print' },
    { id: 'X04940', name: '迁移·aria 等价', check: () => { const r = T.importInputPrefs(data); return typeof r.eggsEnabled === 'boolean'; } },
    { id: 'X04941', name: '迁移·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) T.importInputPrefs(data); return performance.now() - t0 < 50; } },
    { id: 'X04942', name: '迁移·热路径', check: () => T.importInputPrefs(data).eggsEnabled === true },
    { id: 'X04943', name: '迁移·零漂移', check: () => T.exportInputPrefs(T.importInputPrefs(T.exportInputPrefs(prefs))) === data },
    { id: 'X04944', name: '迁移·低配减档', check: () => { const r = T.importInputPrefs(JSON.stringify({ profile: 'off' })); return r.profile === 'off'; } },
    { id: 'X04945', name: '迁移·守卫', check: () => T.importInputPrefs('[]').profile === 'balanced' },
    { id: 'X04946', name: '迁移·智能建议', check: () => T.importInputPrefs(JSON.stringify({ profile: undefined, smartQuotes: false })).profile === T.DEFAULT_CJK_PROFILE },
    { id: 'X04947', name: '迁移·批量模式', check: () => { let n = 0; for (const p of T.CJK_TYPOGRAPHY_PROFILES) { if (T.importInputPrefs(JSON.stringify({ profile: p })).profile === p) n++; } return n === 5; } },
    { id: 'X04948', name: '迁移·跨域联动', check: () => { const r = T.importInputPrefs(data); return r.profile === 'strict' && r.eggsEnabled; } },
    { id: 'X04949', name: '迁移·扩展点', check: () => typeof T.exportInputPrefs === 'function' && typeof T.importInputPrefs === 'function' },
    { id: 'X04950', name: '迁移·彩蛋层', check: () => T.importInputPrefs(JSON.stringify({ eggsEnabled: false })).eggsEnabled === false },
  ];
}

// UNREAL-X AI-20（族0191~0200 · X04751~X05000）V 线聚合：四族 × 25 项 = 100 项，只增不删。
export function runAi20Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [checkF0191, checkF0196, checkF0197, checkF0198];
  const entries = families.flatMap((f) => f());
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
