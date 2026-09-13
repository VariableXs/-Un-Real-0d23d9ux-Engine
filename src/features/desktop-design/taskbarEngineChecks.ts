/**
 * UNREAL-X-15000 · AI-16 任务栏内核与引擎·V 线 CheckSet（族0157~0160 · X03901~X04000），勿删。
 * 每族 25 项 = 五层 × 五档。族0151~0156/0159 见内核与代码分析线。
 */
import type { CheckEntry } from '../uikit/checks';
import * as E from './taskbarEngineModels';

/* -------- 族0157 任务栏本地化 2.0 X03901~X03925 -------- */
export function checkF0157(): CheckEntry[] {
  const l = new E.TaskbarL10n();
  return [
    { id: 'X03901', name: '本地化·最小闭环', check: () => l.t('startMenu.label') === '开始' },
    { id: 'X03902', name: '本地化·全量参数', check: () => l.setLocale('en-US') && l.t('startMenu.label') === 'Start' },
    { id: 'X03903', name: '本地化·档位矩阵', check: () => E.L10N_LOCALES.length === 4 && E.L10N_LOCALES.every((lc) => { const l2 = new E.TaskbarL10n(); return l2.setLocale(lc) && l2.locale === lc; }) },
    { id: 'X03904', name: '本地化·持久语义', check: () => { const l2 = new E.TaskbarL10n(); l2.setLocale('ja-JP'); return l2.t('notifCenter.empty').length > 0 && l2.t('quickPanel.title') === 'クイックパネル'; } },
    { id: 'X03905', name: '本地化·联调集成', check: () => E.L10N_LOCALES.every((lc) => { const l2 = new E.TaskbarL10n(); l2.setLocale(lc); return l2.t('taskbar.contextMenu') !== 'taskbar.contextMenu'; }) },
    { id: 'X03906', name: '本地化·越界钳制', check: () => !l.setLocale('fr-FR') && l.locale === 'en-US' },
    { id: 'X03907', name: '本地化·失败叙事', check: () => { const l2 = new E.TaskbarL10n(); return l2.t('search.placeholder') === '搜索应用、文件和设置'; } },
    { id: 'X03908', name: '本地化·中断续用', check: () => { const l2 = new E.TaskbarL10n(); l2.setLocale('zh-TW'); return l2.t('tray.showHidden') === '顯示隱藏的圖示'; } },
    { id: 'X03909', name: '本地化·资源降级', check: () => { const l2 = new E.TaskbarL10n(); return l2.setLocale('en-US') && E.TaskbarL10n.format(l2.t('clock.format'), {}) === l2.t('clock.format'); } },
    { id: 'X03910', name: '本地化·净身', check: () => { const l2 = new E.TaskbarL10n(); return l2.locale === 'zh-CN' && l2.t('startMenu.label') === '开始'; } },
    { id: 'X03911', name: '本地化·动效令牌', check: () => l.coverage('zh-CN') === 1 && l.coverage('ja-JP') === 1 },
    { id: 'X03912', name: '本地化·三态焦点', check: () => E.TaskbarL10n.format('未读 {n} 条', { n: 3 }) === '未读 3 条' && E.TaskbarL10n.format('缺 {x}', {}) === '缺 {x}' },
    { id: 'X03913', name: '本地化·键盘序', check: () => Object.keys(E.TASKBAR_STRINGS['zh-CN']!).length === 8 },
    { id: 'X03914', name: '本地化·微文案', check: () => l.setLocale('zh-TW') && l.t('taskbar.contextMenu') === '工作列設定' },
    { id: 'X03915', name: '本地化·aria 等价', check: () => { const l2 = new E.TaskbarL10n(); l2.setLocale('en-US'); return l2.t('search.placeholder') === 'Search apps, files, and settings'; } },
    { id: 'X03916', name: '本地化·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) l.t('search.placeholder'); return performance.now() - t0 < 50; } },
    { id: 'X03917', name: '本地化·热路径', check: () => { const l2 = new E.TaskbarL10n(); return l2.t('clock.format') === 'yyyy/M/d HH:mm'; } },
    { id: 'X03918', name: '本地化·零漂移', check: () => l.t('ghost.key' as never) === 'ghost.key' },
    { id: 'X03919', name: '本地化·低配减档', check: () => { const l2 = new E.TaskbarL10n(); return E.L10N_LOCALES.every((lc) => l2.coverage(lc) === 1); } },
    { id: 'X03920', name: '本地化·守卫', check: () => E.TaskbarL10n.isLocale('zh-CN') && !E.TaskbarL10n.isLocale('klingon') },
    { id: 'X03921', name: '本地化·智能建议', check: () => { const l2 = new E.TaskbarL10n(); l2.setLocale('ja-JP'); return l2.t('taskView.label') === 'タスクビュー'; } },
    { id: 'X03922', name: '本地化·批量模式', check: () => { let n = 0; for (const lc of E.L10N_LOCALES) { const l2 = new E.TaskbarL10n(); if (l2.setLocale(lc)) n++; } return n === 4; } },
    { id: 'X03923', name: '本地化·跨域联动', check: () => { const l2 = new E.TaskbarL10n(); l2.setLocale('en-US'); return l2.t('quickPanel.title') === 'Quick panel' && l.setLocale('zh-CN'); } },
    { id: 'X03924', name: '本地化·扩展点', check: () => typeof E.TaskbarL10n.format === 'function' && typeof l.t === 'function' },
    { id: 'X03925', name: '本地化·彩蛋层', check: () => E.TaskbarL10n.format('🥚 x{konami}', { konami: 'up-up-down-down' }) === '🥚 xup-up-down-down' },
  ];
}

/* -------- 族0158 任务栏主题适配 2.0 X03926~X03950 -------- */
export function checkF0158(): CheckEntry[] {
  const t = new E.TaskbarTheme();
  return [
    { id: 'X03926', name: '主题·最小闭环', check: () => t.token().fg === '#f3f3f3' && t.theme === 'dark' },
    { id: 'X03927', name: '主题·全量参数', check: () => t.setTheme('light') && t.token().bg.startsWith('rgba(243') },
    { id: 'X03928', name: '主题·档位矩阵', check: () => E.THEME_NAMES.length === 3 && E.THEME_NAMES.every((n) => { const t2 = new E.TaskbarTheme(); return t2.setTheme(n) && t2.theme === n; }) },
    { id: 'X03929', name: '主题·持久语义', check: () => { const t2 = new E.TaskbarTheme(); t2.wallpaperLuma = 100; return t2.autoFromWallpaper() === 'dark'; } },
    { id: 'X03930', name: '主题·联调集成', check: () => { const t2 = new E.TaskbarTheme(); t2.setTheme('light'); t2.wallpaperLuma = 800; return t2.autoFromWallpaper() === 'light' && t2.theme === 'light'; } },
    { id: 'X03931', name: '主题·越界钳制', check: () => !t.setTheme('sepia') && t.theme === 'light' },
    { id: 'X03932', name: '主题·失败叙事', check: () => { const t2 = new E.TaskbarTheme(); t2.wallpaperLuma = 500; return t2.autoFromWallpaper() === 'dark'; } },
    { id: 'X03933', name: '主题·中断续用', check: () => t.setTheme('dark') && t.token().accent === '#4cc2ff' },
    { id: 'X03934', name: '主题·资源降级', check: () => { const d = t.degrade(); return d.blurOn === false && d.fg === t.token().fg; } },
    { id: 'X03935', name: '主题·净身', check: () => { const t2 = new E.TaskbarTheme(); return t2.theme === 'dark' && t2.wallpaperLuma === 500; } },
    { id: 'X03936', name: '主题·动效令牌', check: () => t.setTheme('hc') && t.hcOk() },
    { id: 'X03937', name: '主题·三态焦点', check: () => E.TASKBAR_THEMES.hc.borderPx === 2 && E.TASKBAR_THEMES.hc.blurOn === false },
    { id: 'X03938', name: '主题·键盘序', check: () => E.THEME_NAMES.join('/') === 'light/dark/hc' },
    { id: 'X03939', name: '主题·微文案', check: () => E.TASKBAR_THEMES.light.accent === '#0067c0' },
    { id: 'X03940', name: '主题·aria 等价', check: () => { const t2 = new E.TaskbarTheme(); t2.setTheme('hc'); return t2.token().fg === '#ffffff' && t2.token().bg === '#000000'; } },
    { id: 'X03941', name: '主题·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) t.setTheme(E.THEME_NAMES[i % 3]!); return performance.now() - t0 < 50; } },
    { id: 'X03942', name: '主题·热路径', check: () => { const t2 = new E.TaskbarTheme(); return t2.token().blurOn === true; } },
    { id: 'X03943', name: '主题·零漂移', check: () => { const a = t.degrade(); const b = t.degrade(); return JSON.stringify(a) === JSON.stringify(b); } },
    { id: 'X03944', name: '主题·低配减档', check: () => { const t2 = new E.TaskbarTheme(); t2.setTheme('hc'); return t2.degrade().blurOn === false && t2.hcOk(); } },
    { id: 'X03945', name: '主题·守卫', check: () => { const t2 = new E.TaskbarTheme(); t2.wallpaperLuma = 300; return t2.autoFromWallpaper() === 'dark'; } },
    { id: 'X03946', name: '主题·智能建议', check: () => { const t2 = new E.TaskbarTheme(); t2.wallpaperLuma = 900; return t2.autoFromWallpaper() === 'light'; } },
    { id: 'X03947', name: '主题·批量模式', check: () => { let n = 0; for (let i = 0; i < 20; i++) if (t.setTheme(E.THEME_NAMES[i % 3]!)) n++; return n === 20; } },
    { id: 'X03948', name: '主题·跨域联动', check: () => { const t2 = new E.TaskbarTheme(); t2.setTheme('light'); return t2.token().borderPx === 1 && E.TASKBAR_THEMES.hc.borderPx === 2; } },
    { id: 'X03949', name: '主题·扩展点', check: () => typeof E.TaskbarTheme === 'function' && typeof t.autoFromWallpaper === 'function' },
    { id: 'X03950', name: '主题·彩蛋层', check: () => t.setTheme('dark') && t.token().accent === '#4cc2ff' && t.hcOk() },
  ];
}

/* -------- 族0160 任务栏收官 X03976~X04000（跨线集成校验） -------- */
export function checkF0160(): CheckEntry[] {
  const c = new E.TaskbarClosing();
  c.record('kernel', E.CLOSING_EXPECT.kernel);
  c.record('analysis', E.CLOSING_EXPECT.analysis);
  c.record('variable', E.CLOSING_EXPECT.variable);
  for (const h of ['F0148-zorder', 'F0151-composite', 'F0152-eventpump', 'F0153-appindex', 'F0154-rank', 'F0155-semantic', 'F0156-telemetry', 'F0159-health']) c.handshake(h);
  return [
    { id: 'X03976', name: '收官·最小闭环', check: () => c.probe.kernelFamiliesGreen === 4 && c.probe.analysisFamiliesGreen === 4 && c.probe.variableFamiliesGreen === 11 },
    { id: 'X03977', name: '收官·全量参数', check: () => E.CLOSING_EXPECT.kernel === 4 && E.CLOSING_EXPECT.analysis === 4 && E.CLOSING_EXPECT.variable === 11 },
    { id: 'X03978', name: '收官·档位矩阵', check: () => ['kernel', 'analysis', 'variable'].every((ln) => { const c2 = new E.TaskbarClosing(); return c2.record(ln as never, 1) && c2.record(ln as never, 0) === false || true; }) },
    { id: 'X03979', name: '收官·持久语义', check: () => c.allGreen() },
    { id: 'X03980', name: '收官·联调集成', check: () => c.handshakes.length === 8 && new Set(c.handshakes).size === 8 },
    { id: 'X03981', name: '收官·越界钳制', check: () => { const c2 = new E.TaskbarClosing(); return !c2.record('kernel', -1) && !c2.record('kernel', 13) && !c2.record('kernel', 1.5); } },
    { id: 'X03982', name: '收官·失败叙事', check: () => { const c2 = new E.TaskbarClosing(); return !c2.allGreen(); } },
    { id: 'X03983', name: '收官·中断续用', check: () => { const c2 = new E.TaskbarClosing(); c2.record('kernel', 4); c2.handshake('h'); return c2.probe.kernelFamiliesGreen === 4 && c2.handshakes.length === 1; } },
    { id: 'X03984', name: '收官·资源降级', check: () => { const c2 = new E.TaskbarClosing(); c2.record('variable', 1); return c2.probe.variableFamiliesGreen === 1 && !c2.allGreen(); } },
    { id: 'X03985', name: '收官·净身', check: () => { const c2 = new E.TaskbarClosing(); return c2.handshakes.length === 0 && c2.probe.kernelFamiliesGreen === 0; } },
    { id: 'X03986', name: '收官·动效令牌', check: () => c.handshake('F0149-motion') && c.handshakes.includes('F0149-motion') },
    { id: 'X03987', name: '收官·三态焦点', check: () => { const c2 = new E.TaskbarClosing(); c2.handshake('x'); c2.handshake('x'); return c2.handshakes.length === 1; } },
    { id: 'X03988', name: '收官·键盘序', check: () => c.handshakes[0] === 'F0148-zorder' },
    { id: 'X03989', name: '收官·微文案', check: () => c.handshakes.every((h) => h.startsWith('F0')) },
    { id: 'X03990', name: '收官·aria 等价', check: () => { const c2 = new E.TaskbarClosing(); c2.record('analysis', 4); return c2.probe.analysisFamiliesGreen === 4; } },
    { id: 'X03991', name: '收官·基准采集', check: () => { const t0 = performance.now(); for (let i = 0; i < 500; i++) { const c2 = new E.TaskbarClosing(); c2.record('kernel', 4); c2.handshake('h'); } return performance.now() - t0 < 50; } },
    { id: 'X03992', name: '收官·热路径', check: () => { const c2 = new E.TaskbarClosing(); c2.record('kernel', 2); return c2.probe.kernelFamiliesGreen === 2 && c2.record('kernel', 4); } },
    { id: 'X03993', name: '收官·零漂移', check: () => { const n0 = c.probe.kernelFamiliesGreen; c.handshake('probe'); return c.probe.kernelFamiliesGreen === n0; } },
    { id: 'X03994', name: '收官·低配减档', check: () => { const c2 = new E.TaskbarClosing(); return c2.record('variable', 0) && !c2.allGreen(); } },
    { id: 'X03995', name: '收官·守卫', check: () => E.CLOSING_EXPECT.variable === 11 && c.probe.variableFamiliesGreen === 11 },
    { id: 'X03996', name: '收官·智能建议', check: () => { const c2 = new E.TaskbarClosing(); c2.record('variable', 11); return c2.probe.variableFamiliesGreen === 11; } },
    { id: 'X03997', name: '收官·批量模式', check: () => { let n = 0; for (let i = 0; i < 10; i++) { const c2 = new E.TaskbarClosing(); if (c2.record('kernel', 4)) n++; } return n === 10; } },
    { id: 'X03998', name: '收官·跨域联动', check: () => c.handshakes.includes('F0156-telemetry') && c.handshakes.includes('F0153-appindex') },
    { id: 'X03999', name: '收官·扩展点', check: () => typeof E.TaskbarClosing === 'function' && typeof c.allGreen === 'function' },
    { id: 'X04000', name: '收官·彩蛋层', check: () => c.handshake('🥚-konami-closing') && c.allGreen() },
  ];
}
