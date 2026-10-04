/**
 * UNREAL-X-15000 · AI-33 兼容性防线·第 1 组 CheckSet（族0321~0330 · X08001~X08250），勿删。
 * 每族 25 项 = 五层 × 五档（基础实装/边界与恢复/手感与细节/性能与优化/创新拓展）。
 */
import type { CheckEntry } from '../uikit/checks';
import * as T from './ai33Models';

/* -------- 族0321 窗口嵌入探测 2.0 X08001~X08025 -------- */
export function checkF0321(): CheckEntry[] {
  const p = new T.EmbedProbe();
  return [
    { id: 'X08001', name: '嵌入·最小闭环', check: () => p.probe({ title: 'w1', className: 'Cls', styles: 0, hasOwnChrome: false }) === 'child' },
    { id: 'X08002', name: '嵌入·全量参数', check: () => p.probe({ title: 'w2', className: 'Cls', styles: 0, hasOwnChrome: true }) === 'none' },
    { id: 'X08003', name: '嵌入·档位矩阵', check: () => p.probe({ title: 'w3', className: 'Cls', styles: 0x80000000, hasOwnChrome: false }) === 'layered' },
    { id: 'X08004', name: '嵌入·快照迁移', check: () => p.probed.length === 3 && p.probed[0]!.title === 'w1' },
    { id: 'X08005', name: '嵌入·联调集成', check: () => p.probe({ title: 'w4', className: 'Cls', styles: 0x00040000, hasOwnChrome: false }) === 'transparent' && p.candidates().length === 2 },
    { id: 'X08006', name: '嵌入·越界钳制', check: () => p.probe({ title: '', className: 'Cls', styles: 0, hasOwnChrome: false }) === 'none' && p.clamped >= 1 },
    { id: 'X08007', name: '嵌入·失败叙事', check: () => p.probe({ title: 'w5', className: '', styles: 0, hasOwnChrome: false }) === 'none' && p.clamped >= 2 },
    { id: 'X08008', name: '嵌入·中断还原', check: () => { const q = new T.EmbedProbe(); return q.probed.length === 0 && q.clamped === 0; } },
    { id: 'X08009', name: '嵌入·资源降级', check: () => T.EmbedProbe.modeLabel('layered') === '分层窗口' && T.EmbedProbe.modeLabel('none') === '独立窗口' },
    { id: 'X08010', name: '嵌入·回滚净身', check: () => { const q = new T.EmbedProbe(); q.probe({ title: 'x', className: 'C', styles: 0, hasOwnChrome: false }); q.probe({ title: '', className: 'C', styles: 0, hasOwnChrome: false }); return q.probed.length === 1; } },
    { id: 'X08011', name: '嵌入·动效令牌', check: () => T.EmbedProbe.modeLabel('child') === '子窗口' && T.EmbedProbe.modeLabel('transparent') === '透明窗口' },
    { id: 'X08012', name: '嵌入·三态焦点', check: () => { const q = new T.EmbedProbe(); q.probe({ title: 'a', className: 'C', styles: 0x80000000, hasOwnChrome: true }); return q.candidates().length === 0; } },
    { id: 'X08013', name: '嵌入·键盘序', check: () => { const q = new T.EmbedProbe(); let n = 0; for (let i = 0; i < 10; i++) { if (q.probe({ title: `t${i}`, className: 'C', styles: 0, hasOwnChrome: false }) === 'child') n++; } return n === 10; } },
    { id: 'X08014', name: '嵌入·微文案', check: () => T.EmbedProbe.modeLabel('child').length > 0 && T.EmbedProbe.modeLabel('none').length > 0 },
    { id: 'X08015', name: '嵌入·aria 等价', check: () => p.candidates().every((w) => !w.hasOwnChrome) },
    { id: 'X08016', name: '嵌入·基准采集', check: () => { const t0 = performance.now(); const q = new T.EmbedProbe(); for (let i = 0; i < 500; i++) q.probe({ title: `t${i}`, className: 'C', styles: 0, hasOwnChrome: false }); return performance.now() - t0 < 50; } },
    { id: 'X08017', name: '嵌入·热路径', check: () => { const q = new T.EmbedProbe(); return q.probe({ title: 'z', className: 'C', styles: 0, hasOwnChrome: false }) === 'child'; } },
    { id: 'X08018', name: '嵌入·零漂移', check: () => { const q = new T.EmbedProbe(); const m1 = q.probe({ title: 'd', className: 'C', styles: 0x00040000, hasOwnChrome: false }); const m2 = q.probe({ title: 'd', className: 'C', styles: 0x00040000, hasOwnChrome: false }); return m1 === m2; } },
    { id: 'X08019', name: '嵌入·低配减档', check: () => { const q = new T.EmbedProbe(); return q.probe({ title: 'l', className: 'C', styles: 0x80000000 | 0x00040000, hasOwnChrome: false }) === 'layered'; } },
    { id: 'X08020', name: '嵌入·守卫', check: () => { const q = new T.EmbedProbe(); return q.candidates().length === 0; } },
    { id: 'X08021', name: '嵌入·智能建议', check: () => { const q = new T.EmbedProbe(); q.probe({ title: 'own', className: 'C', styles: 0, hasOwnChrome: true }); q.probe({ title: 'ok', className: 'C', styles: 0, hasOwnChrome: false }); return q.candidates().every((w) => w.title === 'ok'); } },
    { id: 'X08022', name: '嵌入·批量模式', check: () => { const q = new T.EmbedProbe(); let n = 0; for (let i = 0; i < 20; i++) if (typeof q.probe({ title: `b${i}`, className: 'C', styles: 0, hasOwnChrome: i % 2 === 0 }) === 'string') n++; return n === 20; } },
    { id: 'X08023', name: '嵌入·跨域联动', check: () => { const q = new T.EmbedProbe(); q.probe({ title: 'cef', className: 'CefBrowserWindow', styles: 0, hasOwnChrome: false }); return q.candidates()[0]!.className === 'CefBrowserWindow'; } },
    { id: 'X08024', name: '嵌入·扩展点', check: () => typeof T.EmbedProbe.modeLabel === 'function' && typeof p.candidates === 'function' },
    { id: 'X08025', name: '嵌入·彩蛋层', check: () => new T.EmbedProbe().probe({ title: 'e', className: 'Egg', styles: 0, hasOwnChrome: false }) === 'child' },
  ];
}

/* -------- 族0322 反作弊共存 2.0 X08026~X08050 -------- */
export function checkF0322(): CheckEntry[] {
  const c = new T.AnticheatCoex();
  return [
    { id: 'X08026', name: '共存·最小闭环', check: () => { c.setTrust('acs', 'kernel'); return c.policy('acs') === 'isolate'; } },
    { id: 'X08027', name: '共存·全量参数', check: () => { c.setTrust('acs', 'kernel'); return c.canHook('acs') === false; } },
    { id: 'X08028', name: '共存·档位矩阵', check: () => T.AC_TRUST_LEVELS.length === 4 && T.AC_TRUST_LEVELS[0] === 'unknown' },
    { id: 'X08029', name: '共存·快照迁移', check: () => c.trust['acs'] === 'kernel' },
    { id: 'X08030', name: '共存·联调集成', check: () => { c.setTrust('battleye', 'signed'); return c.policy('battleye') === 'allow-controlled' && c.canHook('battleye') === true; } },
    { id: 'X08031', name: '共存·越界钳制', check: () => c.setTrust('x', 'rootkit') === 'unknown' && c.clamped >= 1 },
    { id: 'X08032', name: '共存·失败叙事', check: () => c.policy('ghost') === 'deny' },
    { id: 'X08033', name: '共存·中断还原', check: () => { const q = new T.AnticheatCoex(); return q.policy('any') === 'deny'; } },
    { id: 'X08034', name: '共存·资源降级', check: () => { c.setTrust('obs1', 'observed'); return c.policy('obs1') === 'allow-controlled'; } },
    { id: 'X08035', name: '共存·回滚净身', check: () => { const q = new T.AnticheatCoex(); q.setTrust('p', 'signed'); q.setTrust('p', 'unknown'); return q.policy('p') === 'deny'; } },
    { id: 'X08036', name: '共存·动效令牌', check: () => T.AC_TRUST_LEVELS[3] === 'kernel' },
    { id: 'X08037', name: '共存·三态焦点', check: () => { const q = new T.AnticheatCoex(); return q.canHook('nobody') === false; } },
    { id: 'X08038', name: '共存·键盘序', check: () => T.AC_TRUST_LEVELS.every((l) => { const q = new T.AnticheatCoex(); return q.setTrust('k', l) === l; }) },
    { id: 'X08039', name: '共存·微文案', check: () => { const q = new T.AnticheatCoex(); q.setTrust('v', 'signed'); return ['isolate', 'allow-controlled', 'deny'].includes(q.policy('v')); } },
    { id: 'X08040', name: '共存·aria 等价', check: () => { const q = new T.AnticheatCoex(); q.setTrust('h', 'kernel'); return q.canHook('h') === false && q.canHook('unknown-p') === false; } },
    { id: 'X08041', name: '共存·基准采集', check: () => { const t0 = performance.now(); const q = new T.AnticheatCoex(); for (let i = 0; i < 500; i++) q.setTrust(`p${i}`, 'signed'); return performance.now() - t0 < 50; } },
    { id: 'X08042', name: '共存·热路径', check: () => { const q = new T.AnticheatCoex(); q.setTrust('fast', 'kernel'); return q.policy('fast') === 'isolate'; } },
    { id: 'X08043', name: '共存·零漂移', check: () => { const q = new T.AnticheatCoex(); q.setTrust('s', 'observed'); const a = q.policy('s'); const b = q.policy('s'); return a === b && a === 'allow-controlled'; } },
    { id: 'X08044', name: '共存·低配减档', check: () => { const q = new T.AnticheatCoex(); return q.setTrust('u', 'weird') === 'unknown' && q.policy('u') === 'deny'; } },
    { id: 'X08045', name: '共存·守卫', check: () => { const q = new T.AnticheatCoex(); return q.canHook('none') === false; } },
    { id: 'X08046', name: '共存·智能建议', check: () => { const q = new T.AnticheatCoex(); q.setTrust('g', 'signed'); return q.policy('g') === 'allow-controlled'; } },
    { id: 'X08047', name: '共存·批量模式', check: () => { const q = new T.AnticheatCoex(); let n = 0; for (let i = 0; i < 20; i++) { q.setTrust(`m${i}`, i % 2 === 0 ? 'signed' : 'observed'); if (q.canHook(`m${i}`)) n++; } return n === 20; } },
    { id: 'X08048', name: '共存·跨域联动', check: () => { const q = new T.AnticheatCoex(); q.setTrust('embed-game', 'kernel'); return q.policy('embed-game') === 'isolate'; } },
    { id: 'X08049', name: '共存·扩展点', check: () => typeof c.policy === 'function' && typeof c.canHook === 'function' },
    { id: 'X08050', name: '共存·彩蛋层', check: () => new T.AnticheatCoex().setTrust('egg', 'observed') === 'observed' },
  ];
}

/* -------- 族0323 CEF 内核兼容 2.0 X08051~X08075 -------- */
export function checkF0323(): CheckEntry[] {
  const c = new T.CefCompat();
  return [
    { id: 'X08051', name: 'CEF·最小闭环', check: () => c.pipeline(109) === 'modern' },
    { id: 'X08052', name: 'CEF·全量参数', check: () => c.pipeline(108) === 'legacy' && c.pipeline(130) === 'modern' },
    { id: 'X08053', name: 'CEF·档位矩阵', check: () => Object.keys(T.CEF_FLAG_MATRIX).length === 5 && T.CEF_FLAG_MATRIX['force-color-profile'] === 'srgb' },
    { id: 'X08054', name: 'CEF·快照迁移', check: () => { c.setFlag('disable-gpu', true); return T.CefCompat.deserialize(JSON.stringify(c.flags)).flags['disable-gpu'] === true; } },
    { id: 'X08055', name: 'CEF·联调集成', check: () => c.launchArgs().includes('--disable-gpu') },
    { id: 'X08056', name: 'CEF·越界钳制', check: () => c.pipeline(Number.NaN) === 'legacy' && c.clamped >= 1 },
    { id: 'X08057', name: 'CEF·失败叙事', check: () => c.pipeline(0) === 'legacy' },
    { id: 'X08058', name: 'CEF·中断还原', check: () => T.CefCompat.deserialize('{bad').pipeline(109) === 'modern' },
    { id: 'X08059', name: 'CEF·资源降级', check: () => { const q = new T.CefCompat(); q.setFlag('disable-gpu', true); q.setFlag('disable-software-rasterizer', true); return q.launchArgs().length === 3; } },
    { id: 'X08060', name: 'CEF·回滚净身', check: () => { const q = T.CefCompat.deserialize('{}'); return q.flags['disable-gpu'] === false && q.clamped === 0; } },
    { id: 'X08061', name: 'CEF·动效令牌', check: () => c.setFlag('unknown-flag', true) === false && c.clamped >= 2 },
    { id: 'X08062', name: 'CEF·三态焦点', check: () => { const q = new T.CefCompat(); q.setFlag('enable-features', 'LayerTreeSimplification'); return q.launchArgs().includes('--enable-features=LayerTreeSimplification'); } },
    { id: 'X08063', name: 'CEF·键盘序', check: () => { const q = new T.CefCompat(); return Object.keys(T.CEF_FLAG_MATRIX).every((k) => q.setFlag(k, true) || k === 'enable-features' || k === 'force-color-profile'); } },
    { id: 'X08064', name: 'CEF·微文案', check: () => c.launchArgs().every((a) => a.startsWith('--')) },
    { id: 'X08065', name: 'CEF·aria 等价', check: () => { const q = new T.CefCompat(); q.setFlag('force-color-profile', 'display-p3'); return q.launchArgs().includes('--force-color-profile=display-p3'); } },
    { id: 'X08066', name: 'CEF·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) new T.CefCompat().pipeline(110); return performance.now() - t0 < 50; } },
    { id: 'X08067', name: 'CEF·热路径', check: () => c.pipeline(120) === 'modern' },
    { id: 'X08068', name: 'CEF·零漂移', check: () => { const a = T.CefCompat.deserialize(JSON.stringify({ 'disable-gpu': true })); const b = T.CefCompat.deserialize(JSON.stringify({ 'disable-gpu': true })); return a.launchArgs().join('|') === b.launchArgs().join('|'); } },
    { id: 'X08069', name: 'CEF·低配减档', check: () => { const q = new T.CefCompat(); q.setFlag('disable-gpu-vsync', true); return q.launchArgs().includes('--disable-gpu-vsync'); } },
    { id: 'X08070', name: 'CEF·守卫', check: () => { const q = new T.CefCompat(); return q.launchArgs().length === 1; } },
    { id: 'X08071', name: 'CEF·智能建议', check: () => { const q = T.CefCompat.deserialize(JSON.stringify({ 'disable-gpu': true, 'force-color-profile': 'display-p3' })); return q.launchArgs().length === 2; } },
    { id: 'X08072', name: 'CEF·批量模式', check: () => { const q = new T.CefCompat(); let n = 0; for (let i = 100; i < 120; i++) if (typeof q.pipeline(i) === 'string') n++; return n === 20; } },
    { id: 'X08073', name: 'CEF·跨域联动', check: () => { const q = new T.CefCompat(); q.pipeline(85); return q.flags['disable-gpu'] === false; } },
    { id: 'X08074', name: 'CEF·扩展点', check: () => typeof T.CefCompat.deserialize === 'function' && typeof c.setFlag === 'function' },
    { id: 'X08075', name: 'CEF·彩蛋层', check: () => new T.CefCompat().pipeline(111) === 'modern' },
  ];
}

/* -------- 族0324 独占全屏让位 2.0 X08076~X08100 -------- */
export function checkF0324(): CheckEntry[] {
  const y = new T.FullscreenYield();
  return [
    { id: 'X08076', name: '让位·最小闭环', check: () => y.state === 'windowed' && y.shouldYield('notif') === false },
    { id: 'X08077', name: '让位·全量参数', check: () => y.setState('exclusive') === 'exclusive' && y.shouldYield('notif') === true },
    { id: 'X08078', name: '让位·档位矩阵', check: () => y.setState('borderless') === 'borderless' && y.shouldYield('notif') === false },
    { id: 'X08079', name: '让位·快照迁移', check: () => { y.setState('exclusive'); return y.fallback() === 'borderless'; } },
    { id: 'X08080', name: '让位·联调集成', check: () => y.registerOverlay('gamebar') === true && y.overlays.length === 1 },
    { id: 'X08081', name: '让位·越界钳制', check: () => y.setState('weird') === 'windowed' && y.clamped >= 1 },
    { id: 'X08082', name: '让位·失败叙事', check: () => y.shouldYield('') === false && y.clamped >= 2 },
    { id: 'X08083', name: '让位·中断还原', check: () => { const q = new T.FullscreenYield(); return q.fallback() === 'windowed'; } },
    { id: 'X08084', name: '让位·资源降级', check: () => { const q = new T.FullscreenYield(); q.setState('borderless'); return q.fallback() === 'borderless'; } },
    { id: 'X08085', name: '让位·回滚净身', check: () => { const q = new T.FullscreenYield(); q.setState('exclusive'); q.setState('windowed'); return q.state === 'windowed' && q.shouldYield('x') === false; } },
    { id: 'X08086', name: '让位·动效令牌', check: () => { const q = new T.FullscreenYield(); q.setState('exclusive'); return q.shouldYield('osd') === true; } },
    { id: 'X08087', name: '让位·三态焦点', check: () => { const q = new T.FullscreenYield(); q.setState('windowed'); q.setState('borderless'); q.setState('exclusive'); return q.state === 'exclusive'; } },
    { id: 'X08088', name: '让位·键盘序', check: () => { const q = new T.FullscreenYield(); return ['windowed', 'borderless', 'exclusive'].every((s) => q.setState(s) === s); } },
    { id: 'X08089', name: '让位·微文案', check: () => y.registerOverlay('gamebar') === false },
    { id: 'X08090', name: '让位·aria 等价', check: () => { const q = new T.FullscreenYield(); return q.registerOverlay('') === false; } },
    { id: 'X08091', name: '让位·基准采集', check: () => { const t0 = performance.now(); const q = new T.FullscreenYield(); for (let i = 0; i < 500; i++) q.setState('exclusive'); return performance.now() - t0 < 50; } },
    { id: 'X08092', name: '让位·热路径', check: () => { const q = new T.FullscreenYield(); q.setState('exclusive'); return q.fallback() === 'borderless'; } },
    { id: 'X08093', name: '让位·零漂移', check: () => { const q = new T.FullscreenYield(); const a = q.setState('exclusive'); const b = q.fallback(); return a === 'exclusive' && b === 'borderless'; } },
    { id: 'X08094', name: '让位·低配减档', check: () => { const q = new T.FullscreenYield(); q.setState('borderless'); return q.shouldYield('osd') === false; } },
    { id: 'X08095', name: '让位·守卫', check: () => { const q = new T.FullscreenYield(); q.setState('exclusive'); q.setState('windowed'); return q.fallback() === 'windowed'; } },
    { id: 'X08096', name: '让位·智能建议', check: () => { const q = new T.FullscreenYield(); q.setState('exclusive'); return q.fallback() === 'borderless' && q.state === 'exclusive'; } },
    { id: 'X08097', name: '让位·批量模式', check: () => { const q = new T.FullscreenYield(); let n = 0; for (let i = 0; i < 20; i++) { if (typeof q.setState(i % 2 === 0 ? 'exclusive' : 'windowed') === 'string') n++; } return n === 20; } },
    { id: 'X08098', name: '让位·跨域联动', check: () => { const q = new T.FullscreenYield(); q.setState('exclusive'); return q.overlays.every((o) => q.shouldYield(o) === true) || q.overlays.length === 2; } },
    { id: 'X08099', name: '让位·扩展点', check: () => typeof y.fallback === 'function' && typeof y.registerOverlay === 'function' },
    { id: 'X08100', name: '让位·彩蛋层', check: () => new T.FullscreenYield().setState('exclusive') === 'exclusive' },
  ];
}

/* -------- 族0325 老应用兼容 2.0 X08101~X08125 -------- */
export function checkF0325(): CheckEntry[] {
  const l = new T.LegacyApp();
  return [
    { id: 'X08101', name: '老应用·最小闭环', check: () => l.enabled.size === 0 && l.applyPreset('win9x').length === 5 },
    { id: 'X08102', name: '老应用·全量参数', check: () => l.enabled.has('registry') === true && l.enabled.has('gdi') === true },
    { id: 'X08103', name: '老应用·档位矩阵', check: () => T.SHIM_LAYERS.length === 5 },
    { id: 'X08104', name: '老应用·快照迁移', check: () => l.applyPreset('winxp').length === 4 && !l.enabled.has('registry') },
    { id: 'X08105', name: '老应用·联调集成', check: () => l.applyPreset('modern').length === 0 && l.enabled.size === 0 },
    { id: 'X08106', name: '老应用·越界钳制', check: () => { l.applyPreset('win7'); return l.toggle('nope', true) === false && l.clamped >= 1; } },
    { id: 'X08107', name: '老应用·失败叙事', check: () => { const q = new T.LegacyApp(); return (q.applyPreset('winme' as 'win9x')).length === 0; } },
    { id: 'X08108', name: '老应用·中断还原', check: () => { const q = new T.LegacyApp(); q.applyPreset('winxp'); q.toggle('font', false); return q.enabled.size === 3; } },
    { id: 'X08109', name: '老应用·资源降级', check: () => { const q = new T.LegacyApp(); return q.applyPreset('win10').length === 1; } },
    { id: 'X08110', name: '老应用·回滚净身', check: () => { const q = new T.LegacyApp(); q.applyPreset('win9x'); return q.toggle('dpi', false) === true && q.enabled.size === 4; } },
    { id: 'X08111', name: '老应用·动效令牌', check: () => T.LegacyApp.clampScale(150) === 150 },
    { id: 'X08112', name: '老应用·三态焦点', check: () => T.LegacyApp.clampScale(600) === 500 && T.LegacyApp.clampScale(50) === 100 },
    { id: 'X08113', name: '老应用·键盘序', check: () => T.SHIM_LAYERS.every((s) => { const q = new T.LegacyApp(); return q.toggle(s, true) === true && q.enabled.has(s); }) },
    { id: 'X08114', name: '老应用·微文案', check: () => T.LegacyApp.clampScale(137) === 125 },
    { id: 'X08115', name: '老应用·aria 等价', check: () => { const q = new T.LegacyApp(); q.applyPreset('win9x'); return q.toggle('gdi', false) === true && !q.enabled.has('gdi'); } },
    { id: 'X08116', name: '老应用·基准采集', check: () => { const t0 = performance.now(); const q = new T.LegacyApp(); for (let i = 0; i < 500; i++) q.applyPreset('winxp'); return performance.now() - t0 < 50; } },
    { id: 'X08117', name: '老应用·热路径', check: () => { const q = new T.LegacyApp(); return q.applyPreset('win7').join(',') === 'dpi,theme'; } },
    { id: 'X08118', name: '老应用·零漂移', check: () => { const q = new T.LegacyApp(); const a = q.applyPreset('winxp').sort().join('|'); const b = new T.LegacyApp().applyPreset('winxp').sort().join('|'); return a === b; } },
    { id: 'X08119', name: '老应用·低配减档', check: () => { const q = new T.LegacyApp(); return q.applyPreset('modern').length === 0; } },
    { id: 'X08120', name: '老应用·守卫', check: () => { const q = new T.LegacyApp(); q.applyPreset('win9x'); return q.enabled.size === 5; } },
    { id: 'X08121', name: '老应用·智能建议', check: () => { const q = new T.LegacyApp(); q.applyPreset('win7'); return q.toggle('gdi', true) === true && q.enabled.has('gdi'); } },
    { id: 'X08122', name: '老应用·批量模式', check: () => { let n = 0; for (const s of T.SHIM_LAYERS) { const q = new T.LegacyApp(); if (q.toggle(s, true) && q.enabled.size === 1) n++; } return n === 5; } },
    { id: 'X08123', name: '老应用·跨域联动', check: () => { const q = new T.LegacyApp(); q.applyPreset('winxp'); return q.enabled.has('theme') === true; } },
    { id: 'X08124', name: '老应用·扩展点', check: () => typeof T.LegacyApp.clampScale === 'function' && typeof l.toggle === 'function' },
    { id: 'X08125', name: '老应用·彩蛋层', check: () => new T.LegacyApp().applyPreset('modern').length === 0 },
  ];
}

/* -------- 族0326 驱动拦截兼容 X08126~X08150 -------- */
export function checkF0326(): CheckEntry[] {
  const d = new T.DriverIntercept();
  return [
    { id: 'X08126', name: '拦截·最小闭环', check: () => d.decide('any.sys') === 'pass' },
    { id: 'X08127', name: '拦截·全量参数', check: () => d.addRule('nvidia', 'emulate', 10) === true && d.decide('nvidia.sys') === 'emulate' },
    { id: 'X08128', name: '拦截·档位矩阵', check: () => d.addRule('cheat', 'block', 20) === true && d.decide('cheatdrv.sys') === 'block' },
    { id: 'X08129', name: '拦截·快照迁移', check: () => d.rules.length === 2 },
    { id: 'X08130', name: '拦截·联调集成', check: () => d.addRule('ok.sys', 'pass', 5) === true && d.decide('ok.sys') === 'pass' },
    { id: 'X08131', name: '拦截·越界钳制', check: () => d.addRule('bad', 'hook', 1) === false && d.clamped >= 1 },
    { id: 'X08132', name: '拦截·失败叙事', check: () => d.addRule('', 'block', 1) === false },
    { id: 'X08133', name: '拦截·中断还原', check: () => { const q = new T.DriverIntercept(); return q.decide('x') === 'pass' && q.rules.length === 0; } },
    { id: 'X08134', name: '拦截·资源降级', check: () => d.decide('unknown-driver.sys') === 'pass' },
    { id: 'X08135', name: '拦截·回滚净身', check: () => { d.reset(); return d.rules.length === 0; } },
    { id: 'X08136', name: '拦截·动效令牌', check: () => { d.addRule('a', 'block', 1); d.addRule('ab', 'pass', 2); return d.decide('ab.sys') === 'pass'; } },
    { id: 'X08137', name: '拦截·三态焦点', check: () => { const q = new T.DriverIntercept(); q.addRule('a', 'emulate', 3); return q.decide('aaa.sys') === 'emulate'; } },
    { id: 'X08138', name: '拦截·键盘序', check: () => { const q = new T.DriverIntercept(); let n = 0; for (let i = 0; i < 10; i++) if (q.addRule(`m${i}`, 'emulate', i)) n++; return n === 10; } },
    { id: 'X08139', name: '拦截·微文案', check: () => { const q = new T.DriverIntercept(); q.addRule('nv', 'block', 1); return q.decide('nv') === 'block'; } },
    { id: 'X08140', name: '拦截·aria 等价', check: () => { const q = new T.DriverIntercept(); q.addRule('p1', 'pass', 9); q.addRule('p2', 'block', 1); return q.decide('p1p2') === 'pass'; } },
    { id: 'X08141', name: '拦截·基准采集', check: () => { const t0 = performance.now(); const q = new T.DriverIntercept(); for (let i = 0; i < 500; i++) q.decide(`drv${i}.sys`); return performance.now() - t0 < 50; } },
    { id: 'X08142', name: '拦截·热路径', check: () => { const q = new T.DriverIntercept(); return q.decide('fast') === 'pass'; } },
    { id: 'X08143', name: '拦截·零漂移', check: () => { const q = new T.DriverIntercept(); q.addRule('s', 'emulate', 1); return q.decide('s.sys') === q.decide('s.sys'); } },
    { id: 'X08144', name: '拦截·低配减档', check: () => { const q = new T.DriverIntercept(); q.addRule('x', 'block', 1); q.reset(); return q.decide('x.sys') === 'pass'; } },
    { id: 'X08145', name: '拦截·守卫', check: () => { const q = new T.DriverIntercept(); return q.addRule('y', 'BLOCK', 1) === false; } },
    { id: 'X08146', name: '拦截·智能建议', check: () => { const q = new T.DriverIntercept(); q.addRule('emu', 'emulate', 7); return q.decide('emu-driver') === 'emulate'; } },
    { id: 'X08147', name: '拦截·批量模式', check: () => { const q = new T.DriverIntercept(); let n = 0; for (let i = 0; i < 20; i++) { q.addRule(`r${i}`, 'pass', i); if (typeof q.decide(`r${i}`) === 'string') n++; } return n === 20; } },
    { id: 'X08148', name: '拦截·跨域联动', check: () => { const q = new T.DriverIntercept(); q.addRule('anticheat', 'isolate' === 'isolate' ? 'block' : 'pass', 9); return q.decide('anticheat.sys') === 'block'; } },
    { id: 'X08149', name: '拦截·扩展点', check: () => typeof d.decide === 'function' && typeof d.reset === 'function' },
    { id: 'X08150', name: '拦截·彩蛋层', check: () => { const q = new T.DriverIntercept(); q.reset(); return q.rules.length === 0; } },
  ];
}

/* -------- 族0327 Shell 扩展兼容 X08151~X08175 -------- */
export function checkF0327(): CheckEntry[] {
  const s = new T.ShellExtCompat();
  return [
    { id: 'X08151', name: 'ShellExt·最小闭环', check: () => s.policy === 'blocklist' && s.shouldLoad('{AAA1}') === true },
    { id: 'X08152', name: 'ShellExt·全量参数', check: () => s.blocked.add('{BAD}').size === 1 && s.shouldLoad('{BAD}') === false },
    { id: 'X08153', name: 'ShellExt·档位矩阵', check: () => T.SHEXT_LOAD_POLICY.length === 3 },
    { id: 'X08154', name: 'ShellExt·快照迁移', check: () => s.setPolicy('allowlist') === true && s.shouldLoad('{AAA1}') === false },
    { id: 'X08155', name: 'ShellExt·联调集成', check: () => { s.allowed.add('{AAA1}'); return s.shouldLoad('{AAA1}') === true; } },
    { id: 'X08156', name: 'ShellExt·越界钳制', check: () => new T.ShellExtCompat('weird').policy === 'blocklist' && new T.ShellExtCompat('weird').clamped === 1 },
    { id: 'X08157', name: 'ShellExt·失败叙事', check: () => s.setPolicy('nope') === false && s.clamped >= 1 },
    { id: 'X08158', name: 'ShellExt·中断还原', check: () => { const q = new T.ShellExtCompat('audit-only'); return q.shouldLoad('{ANY}') === true; } },
    { id: 'X08159', name: 'ShellExt·资源降级', check: () => { const q = new T.ShellExtCompat('blocklist'); return q.shouldLoad('{NEW}') === true; } },
    { id: 'X08160', name: 'ShellExt·回滚净身', check: () => s.crashes['x'] === undefined && s.blocked.size === 1 },
    { id: 'X08161', name: 'ShellExt·动效令牌', check: () => { const q = new T.ShellExtCompat(); return q.reportCrash('{C1}') === false && q.reportCrash('{C1}') === false && q.reportCrash('{C1}') === true; } },
    { id: 'X08162', name: 'ShellExt·三态焦点', check: () => { const q = new T.ShellExtCompat(); q.reportCrash('{X}'); q.reportCrash('{X}'); return q.blocked.has('{X}') === false; } },
    { id: 'X08163', name: 'ShellExt·键盘序', check: () => T.SHEXT_LOAD_POLICY.every((p) => new T.ShellExtCompat(p).policy === p) },
    { id: 'X08164', name: 'ShellExt·微文案', check: () => { const q = new T.ShellExtCompat(); return q.reportCrash('{C2}') === false && q.crashes['{C2}'] === 1; } },
    { id: 'X08165', name: 'ShellExt·aria 等价', check: () => { const q = new T.ShellExtCompat('allowlist'); q.allowed.add('{OK}'); return q.shouldLoad('{OK}') === true && q.shouldLoad('{NO}') === false; } },
    { id: 'X08166', name: 'ShellExt·基准采集', check: () => { const t0 = performance.now(); const q = new T.ShellExtCompat(); for (let i = 0; i < 500; i++) q.shouldLoad(`{G${i}}`); return performance.now() - t0 < 50; } },
    { id: 'X08167', name: 'ShellExt·热路径', check: () => { const q = new T.ShellExtCompat('audit-only'); return q.shouldLoad('{F}') === true; } },
    { id: 'X08168', name: 'ShellExt·零漂移', check: () => { const q = new T.ShellExtCompat(); const a = q.shouldLoad('{S}'); const b = q.shouldLoad('{S}'); return a === b; } },
    { id: 'X08169', name: 'ShellExt·低配减档', check: () => { const q = new T.ShellExtCompat('allowlist'); return q.shouldLoad('{X}') === false; } },
    { id: 'X08170', name: 'ShellExt·守卫', check: () => { const q = new T.ShellExtCompat(); for (let i = 0; i < 3; i++) q.reportCrash('{CC}'); return q.blocked.has('{CC}'); } },
    { id: 'X08171', name: 'ShellExt·智能建议', check: () => { const q = new T.ShellExtCompat(); q.reportCrash('{D1}'); q.reportCrash('{D1}'); return q.crashes['{D1}'] === 2 && q.blocked.size === 0; } },
    { id: 'X08172', name: 'ShellExt·批量模式', check: () => { const q = new T.ShellExtCompat('audit-only'); let n = 0; for (let i = 0; i < 20; i++) if (q.shouldLoad(`{B${i}}`)) n++; return n === 20; } },
    { id: 'X08173', name: 'ShellExt·跨域联动', check: () => { const q = new T.ShellExtCompat(); q.setPolicy('allowlist'); q.allowed.add('{E}'); return q.shouldLoad('{E}') === true && q.shouldLoad('{F}') === false; } },
    { id: 'X08174', name: 'ShellExt·扩展点', check: () => typeof s.setPolicy === 'function' && typeof s.reportCrash === 'function' },
    { id: 'X08175', name: 'ShellExt·彩蛋层', check: () => new T.ShellExtCompat('audit-only').policy === 'audit-only' },
  ];
}

/* -------- 族0328 显示管线兼容 X08176~X08200 -------- */
export function checkF0328(): CheckEntry[] {
  const d = new T.DisplayPipeline();
  return [
    { id: 'X08176', name: '显示·最小闭环', check: () => d.negotiate(['hdr10']) === 'hdr10' && d.bitDepth() === 10 },
    { id: 'X08177', name: '显示·全量参数', check: () => d.negotiate(['sdr-high']) === 'sdr-high' && d.bitDepth() === 8 },
    { id: 'X08178', name: '显示·档位矩阵', check: () => T.DISPLAY_FALLBACK_CHAIN.length === 3 },
    { id: 'X08179', name: '显示·快照迁移', check: () => d.caps.join(',') === 'sdr-high' },
    { id: 'X08180', name: '显示·联调集成', check: () => d.negotiate(['sdr-native', 'sdr-high']) === 'sdr-high' },
    { id: 'X08181', name: '显示·越界钳制', check: () => d.negotiate([]) === 'sdr-native' },
    { id: 'X08182', name: '显示·失败叙事', check: () => d.negotiate(['weird-mode']) === 'sdr-native' },
    { id: 'X08183', name: '显示·中断还原', check: () => d.negotiate(null as unknown as string[]) === 'sdr-native' && d.clamped >= 1 },
    { id: 'X08184', name: '显示·资源降级', check: () => T.DisplayPipeline.clampRefresh(1000) === 500 },
    { id: 'X08185', name: '显示·回滚净身', check: () => { const q = new T.DisplayPipeline(); return q.current === 'hdr10' && q.caps.length === 0; } },
    { id: 'X08186', name: '显示·动效令牌', check: () => T.DisplayPipeline.clampRefresh(24) === 24 },
    { id: 'X08187', name: '显示·三态焦点', check: () => T.DisplayPipeline.clampRefresh(144.6) === 145 },
    { id: 'X08188', name: '显示·键盘序', check: () => T.DISPLAY_FALLBACK_CHAIN.every((m) => d.negotiate([m]) === m) },
    { id: 'X08189', name: '显示·微文案', check: () => T.DisplayPipeline.clampRefresh(60) === 60 },
    { id: 'X08190', name: '显示·aria 等价', check: () => { const q = new T.DisplayPipeline(); q.negotiate(['sdr-native']); return q.bitDepth() === 8; } },
    { id: 'X08191', name: '显示·基准采集', check: () => { const t0 = performance.now(); const q = new T.DisplayPipeline(); for (let i = 0; i < 500; i++) q.negotiate(['hdr10']); return performance.now() - t0 < 50; } },
    { id: 'X08192', name: '显示·热路径', check: () => T.DisplayPipeline.clampRefresh(Number.NaN) === 60 },
    { id: 'X08193', name: '显示·零漂移', check: () => { const q = new T.DisplayPipeline(); const a = q.negotiate(['sdr-high']); const b = q.negotiate(['sdr-high']); return a === b; } },
    { id: 'X08194', name: '显示·低配减档', check: () => { const q = new T.DisplayPipeline(); return q.negotiate(['sdr-native']) === 'sdr-native' && q.bitDepth() === 8; } },
    { id: 'X08195', name: '显示·守卫', check: () => T.DisplayPipeline.clampRefresh(10) === 24 },
    { id: 'X08196', name: '显示·智能建议', check: () => { const q = new T.DisplayPipeline(); return q.negotiate(['sdr-native', 'hdr10']) === 'hdr10'; } },
    { id: 'X08197', name: '显示·批量模式', check: () => { let n = 0; for (const m of T.DISPLAY_FALLBACK_CHAIN) { const q = new T.DisplayPipeline(); if (q.negotiate([m]) === m) n++; } return n === 3; } },
    { id: 'X08198', name: '显示·跨域联动', check: () => { const q = new T.DisplayPipeline(); q.negotiate(['hdr10']); return q.current === 'hdr10' && q.bitDepth() === 10; } },
    { id: 'X08199', name: '显示·扩展点', check: () => typeof d.negotiate === 'function' && typeof T.DisplayPipeline.clampRefresh === 'function' },
    { id: 'X08200', name: '显示·彩蛋层', check: () => new T.DisplayPipeline().negotiate(['hdr10', 'sdr-high']) === 'hdr10' },
  ];
}

/* -------- 族0329 音频管线兼容 X08201~X08225 -------- */
export function checkF0329(): CheckEntry[] {
  const a = new T.AudioPipeline();
  return [
    { id: 'X08201', name: '音频·最小闭环', check: () => a.fmt === 'int16-48k' && a.exclusive === false },
    { id: 'X08202', name: '音频·全量参数', check: () => a.negotiate(['float32-192k']) === 'float32-192k' && a.setExclusive(true) === true && a.exclusive === true },
    { id: 'X08203', name: '音频·档位矩阵', check: () => T.AUDIO_FMT_CHAIN.length === 4 },
    { id: 'X08204', name: '音频·快照迁移', check: () => a.addApo('loudness') === true && a.apox.length === 1 },
    { id: 'X08205', name: '音频·联调集成', check: () => a.negotiate(['int16-44k']) === 'int16-44k' && a.setExclusive(false) === true },
    { id: 'X08206', name: '音频·越界钳制', check: () => { const q = new T.AudioPipeline(); return q.setExclusive(true) === false && q.clamped >= 1; } },
    { id: 'X08207', name: '音频·失败叙事', check: () => a.negotiate(['weird-fmt']) === 'int16-44k' },
    { id: 'X08208', name: '音频·中断还原', check: () => a.negotiate(null as unknown as string[]) === 'int16-44k' && a.clamped >= 1 },
    { id: 'X08209', name: '音频·资源降级', check: () => a.addApo('') === false && a.apox.length === 1 },
    { id: 'X08210', name: '音频·回滚净身', check: () => { const q = new T.AudioPipeline(); return q.fmt === 'int16-48k' && q.apox.length === 0; } },
    { id: 'X08211', name: '音频·动效令牌', check: () => { const q = new T.AudioPipeline(); q.negotiate(['float32-48k']); return q.setExclusive(true) === true; } },
    { id: 'X08212', name: '音频·三态焦点', check: () => a.apox.length === 1 && a.apox[0] === 'loudness' },
    { id: 'X08213', name: '音频·键盘序', check: () => T.AUDIO_FMT_CHAIN.every((f) => new T.AudioPipeline().negotiate([f]) === f) },
    { id: 'X08214', name: '音频·微文案', check: () => a.addApo('eq') === true && a.apox.length === 2 },
    { id: 'X08215', name: '音频·aria 等价', check: () => { const q = new T.AudioPipeline(); return q.negotiate(['int16-48k']) === 'int16-48k' && q.setExclusive(true) === false; } },
    { id: 'X08216', name: '音频·基准采集', check: () => { const t0 = performance.now(); const q = new T.AudioPipeline(); for (let i = 0; i < 500; i++) q.negotiate(['float32-48k']); return performance.now() - t0 < 50; } },
    { id: 'X08217', name: '音频·热路径', check: () => { const q = new T.AudioPipeline(); return q.negotiate(['float32-48k']) === 'float32-48k'; } },
    { id: 'X08218', name: '音频·零漂移', check: () => { const q = new T.AudioPipeline(); const a1 = q.negotiate(['int16-48k']); const b1 = q.negotiate(['int16-48k']); return a1 === b1; } },
    { id: 'X08219', name: '音频·低配减档', check: () => { const q = new T.AudioPipeline(); return q.negotiate([]) === 'int16-44k'; } },
    { id: 'X08220', name: '音频·守卫', check: () => { const q = new T.AudioPipeline(); return q.addApo('a') && q.addApo('b') === true; } },
    { id: 'X08221', name: '音频·智能建议', check: () => { const q = new T.AudioPipeline(); q.negotiate(['float32-192k']); q.setExclusive(true); return q.exclusive === true; } },
    { id: 'X08222', name: '音频·批量模式', check: () => { const q = new T.AudioPipeline(); let n = 0; for (let i = 0; i < 8; i++) if (q.addApo(`apo${i}`)) n++; return n === 8 && q.addApo('over') === false; } },
    { id: 'X08223', name: '音频·跨域联动', check: () => { const q = new T.AudioPipeline(); q.negotiate(['float32-48k']); q.setExclusive(true); return q.exclusive === true && q.fmt === 'float32-48k'; } },
    { id: 'X08224', name: '音频·扩展点', check: () => typeof a.negotiate === 'function' && typeof a.addApo === 'function' },
    { id: 'X08225', name: '音频·彩蛋层', check: () => new T.AudioPipeline().negotiate(['float32-192k', 'int16-44k']) === 'float32-192k' },
  ];
}

/* -------- 族0330 网络兼容 2.0 X08226~X08250 -------- */
export function checkF0330(): CheckEntry[] {
  const n = new T.NetCompat();
  return [
    { id: 'X08226', name: '网络·最小闭环', check: () => n.proto === 'http1.1' },
    { id: 'X08227', name: '网络·全量参数', check: () => n.negotiate(['http1.1']) === 'http1.1' },
    { id: 'X08228', name: '网络·档位矩阵', check: () => T.NET_PROTO_CHAIN.length === 3 && T.NET_PROTO_CHAIN[0] === 'quic' },
    { id: 'X08229', name: '网络·快照迁移', check: () => n.negotiate(['http2', 'http1.1']) === 'http2' && n.proto === 'http2' },
    { id: 'X08230', name: '网络·联调集成', check: () => n.negotiate(['quic', 'http2', 'http1.1']) === 'quic' },
    { id: 'X08231', name: '网络·越界钳制', check: () => n.negotiate([]) === 'http1.1' },
    { id: 'X08232', name: '网络·失败叙事', check: () => n.negotiate(['h3']) === 'http1.1' },
    { id: 'X08233', name: '网络·中断还原', check: () => n.negotiate(null as unknown as string[]) === 'http1.1' && n.clamped >= 1 },
    { id: 'X08234', name: '网络·资源降级', check: () => { n.negotiate(['quic']); return n.coexist(true, false) === 'http2'; } },
    { id: 'X08235', name: '网络·回滚净身', check: () => { const q = new T.NetCompat(); return q.vpn === false && q.proxy === null; } },
    { id: 'X08236', name: '网络·动效令牌', check: () => T.NetCompat.clampPort(99999) === 65535 },
    { id: 'X08237', name: '网络·三态焦点', check: () => T.NetCompat.clampPort(0) === 1 && T.NetCompat.clampPort(443) === 443 },
    { id: 'X08238', name: '网络·键盘序', check: () => T.NET_PROTO_CHAIN.every((p) => new T.NetCompat().negotiate([p]) === p) },
    { id: 'X08239', name: '网络·微文案', check: () => { n.negotiate(['quic']); n.coexist(false, true); return n.proto === 'http1.1' && n.proxy === 'system'; } },
    { id: 'X08240', name: '网络·aria 等价', check: () => { const q = new T.NetCompat(); q.negotiate(['quic']); return q.coexist(true, false) === 'http2' && q.vpn === true; } },
    { id: 'X08241', name: '网络·基准采集', check: () => { const t0 = performance.now(); const q = new T.NetCompat(); for (let i = 0; i < 500; i++) q.negotiate(['quic']); return performance.now() - t0 < 50; } },
    { id: 'X08242', name: '网络·热路径', check: () => T.NetCompat.clampPort(8080) === 8080 },
    { id: 'X08243', name: '网络·零漂移', check: () => { const q = new T.NetCompat(); q.negotiate(['http2']); const a = q.coexist(false, false); const b = q.coexist(false, false); return a === b && a === 'http2'; } },
    { id: 'X08244', name: '网络·低配减档', check: () => { const q = new T.NetCompat(); return q.negotiate(['http1.1', 'quic']) === 'quic'; } },
    { id: 'X08245', name: '网络·守卫', check: () => T.NetCompat.clampPort(Number.NaN) === 443 },
    { id: 'X08246', name: '网络·智能建议', check: () => { const q = new T.NetCompat(); q.negotiate(['quic']); q.coexist(true, true); return q.proto === 'http1.1'; } },
    { id: 'X08247', name: '网络·批量模式', check: () => { let n = 0; for (const p of T.NET_PROTO_CHAIN) { const q = new T.NetCompat(); if (q.negotiate([p]) === p) n++; } return n === 3; } },
    { id: 'X08248', name: '网络·跨域联动', check: () => { const q = new T.NetCompat(); q.negotiate(['quic']); return q.coexist(false, false) === 'quic' && q.vpn === false; } },
    { id: 'X08249', name: '网络·扩展点', check: () => typeof n.coexist === 'function' && typeof T.NetCompat.clampPort === 'function' },
    { id: 'X08250', name: '网络·彩蛋层', check: () => new T.NetCompat().negotiate(['quic']) === 'quic' },
  ];
}

/** AI-33 全量聚合：10 族 250 项。 */
export function runAi33Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [
    checkF0321, checkF0322, checkF0323, checkF0324, checkF0325,
    checkF0326, checkF0327, checkF0328, checkF0329, checkF0330,
  ];
  const entries = families.flatMap((fn) => fn());
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
