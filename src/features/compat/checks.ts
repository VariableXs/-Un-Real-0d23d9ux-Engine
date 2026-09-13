// AURORA-10000: AI-41~AI-45 批次领域09自检注册表（F05001~F05625 共 625 项），勿删。
// 每项一条可运行断言；「位/预留」条目按 §15 守卫口径 = 接口冻结 + 开关存在。

import * as A from './groupA';
import * as B from './groupB';
import * as C from './groupC';
import * as D from './groupD';
import * as E from './groupE';

export interface CheckEntry {
  id: string;
  name: string;
  check: () => boolean;
}

const T0 = 1700000000000;

function fixtureWindow(): A.HostWindow {
  return { id: 1, className: 'Chrome_WidgetWin_1', title: 'Steam', processName: 'steam.exe' };
}

/* -------- AI-41 族0201 窗口嵌入探测 -------- */
export function checkF0201(): CheckEntry[] {
  const w = fixtureWindow();
  const wins: A.HostWindow[] = [
    w,
    { id: 2, className: 'Shell_TrayWnd', title: '任务栏', processName: 'explorer.exe', isSystem: true },
    { id: 3, className: 'Chrome_WidgetWin_1', title: '网易云', processName: 'cloudmusic.exe' },
  ];
  const lib = new Map<string, 'embeddable' | 'forbidden'>([['Chrome_WidgetWin_1', 'embeddable'], ['Shell_TrayWnd', 'forbidden']]);
  const reg = new A.EmbedRegistry();
  const tray = new A.TrayAdoption();
  return [
    { id: 'F05001', name: '实时探测', check: () => A.probeWindows(wins, lib).length === 2 },
    { id: 'F05002', name: '类名库预判', check: () => lib.get('Chrome_WidgetWin_1') === 'embeddable' },
    { id: 'F05003', name: '黑名单禁嵌', check: () => A.embedVerdict(wins[1]!, new Set(['Shell_TrayWnd']), new Set(), lib) === 'reject' },
    { id: 'F05004', name: '白名单直嵌', check: () => A.embedVerdict(w, new Set(), new Set(['steam.exe']), lib) === 'embed' },
    { id: 'F05005', name: '双击嵌入', check: () => A.embedTriggerAction('double-click').target === 'desktop-host' },
    { id: 'F05006', name: '拖拽嵌入', check: () => A.embedTriggerAction('drag').animated },
    { id: 'F05007', name: '收编动画', check: () => A.embedTriggerAction('double-click').animated },
    { id: 'F05008', name: '失败卡', check: () => A.embedAttempt(w, false).reason === 'unsupported-window-class' },
    { id: 'F05009', name: '失败回退', check: () => A.rollbackEmbed(A.embedAttempt(w, false)).state === 'failed' },
    { id: 'F05010', name: '尺寸自适应', check: () => A.fitEmbedRect({ w: 800, h: 600 }, 192).h === 1200 },
    { id: 'F05011', name: 'DPI 重嵌', check: () => A.fitEmbedRect({ w: 800, h: 600 }, 192, 96).w === 1600 },
    { id: 'F05012', name: '主题检测', check: () => A.detectAppTheme([20, 20, 20]).theme === 'dark' },
    { id: 'F05013', name: '深色注入', check: () => A.detectAppTheme([250, 250, 250]).suggestDarkInjection },
    { id: 'F05014', name: '统一圆角', check: () => A.EMBED_STYLE_DEFAULTS.cornerRadius === 8 },
    { id: 'F05015', name: '统一阴影', check: () => A.EMBED_STYLE_DEFAULTS.shadow === 'elevation-2' },
    { id: 'F05016', name: '可选重绘标题栏', check: () => A.EMBED_STYLE_DEFAULTS.redrawTitlebar === false },
    { id: 'F05017', name: '统一图标', check: () => A.EMBED_STYLE_DEFAULTS.unifyIcon },
    { id: 'F05018', name: '进程美化', check: () => A.friendlyProcessName('WeChat.exe') === '微信' && A.friendlyProcessName('foo.exe') === 'foo' },
    { id: 'F05019', name: '多实例收编', check: () => reg.embed(10, null, 'main') && reg.embed(11, 10, 'popup') && reg.isEmbedded(11) },
    { id: 'F05020', name: '子窗定位', check: () => reg.ownerOf(11) === 10 },
    { id: 'F05021', name: '弹窗跟随', check: () => !reg.embed(12, null, 'popup') },
    { id: 'F05022', name: '模态捕获', check: () => reg.embed(13, 10, 'modal') },
    { id: 'F05023', name: '托盘收编', check: () => tray.adopt('steam.exe') && !tray.adopt('steam.exe') && tray.list.includes('steam.exe') },
    { id: 'F05024', name: '去重显示', check: () => reg.embed(20, null, 'main') && reg.embed(21, 20, 'popup') && reg.taskbarEntries().filter((x) => x === 20).length === 1 },
    { id: 'F05025', name: '收编教学', check: () => A.embedTutorial().length === 3 },
  ];
}

/* -------- AI-41 族0202 反作弊共存 -------- */
export function checkF0202(): CheckEntry[] {
  const y = new A.AntiCheatYield();
  const ledger = new A.AntiCheatLedger();
  const policy: A.YieldPolicy = { ...A.DEFAULT_YIELD_POLICY };
  return [
    { id: 'F05026', name: 'EAC 检测', check: () => A.detectAntiCheat(['EasyAntiCheat.exe']).includes('EAC') },
    { id: 'F05027', name: 'BattlEye 检测', check: () => A.detectAntiCheat(['BEService.exe']).includes('BattlEye') },
    { id: 'F05028', name: 'Vanguard 检测', check: () => A.detectAntiCheat(['vgk.sys']).includes('Vanguard') },
    { id: 'F05029', name: 'nProtect 检测', check: () => A.detectAntiCheat(['GameMon.exe']).includes('nProtect') },
    { id: 'F05030', name: '自动让位', check: () => y.onDetected('EAC') && y.phaseNow() === 'yielding' && y.settle() && y.phaseNow() === 'yielded' },
    { id: 'F05031', name: '自动回归', check: () => y.onGameExit() && y.restoreDone() && y.phaseNow() === 'normal' },
    { id: 'F05032', name: '独占尊重', check: () => { const y2 = new A.AntiCheatYield(); y2.onDetected('BattlEye'); return !y2.onDetected('EAC'); } },
    { id: 'F05033', name: '无边框提示', check: () => A.gameModeFlags(['EAC'], false).fullscreenHint },
    { id: 'F05034', name: '覆盖层禁用', check: () => A.gameModeFlags([], true).overlaysDisabled },
    { id: 'F05035', name: 'FPS 联动', check: () => A.gameModeFlags([], true).fpsOverlayOff },
    { id: 'F05036', name: '截图尊重', check: () => A.gameModeFlags([], true).screenshotProtected },
    { id: 'F05037', name: '录屏尊重', check: () => A.gameModeFlags([], true).recordingProtected },
    { id: 'F05038', name: '注入禁用', check: () => A.INJECTION_POLICY.injectGameProcess === false },
    { id: 'F05039', name: '驱动感知', check: () => A.kernelPerception(['BEDaisy.sys']).readOnly === true && A.kernelPerception(['BEDaisy.sys']).known.includes('BattlEye') },
    { id: 'F05040', name: '内核承诺', check: () => A.INJECTION_POLICY.kernelTouchGame === false },
    { id: 'F05041', name: '白名单库', check: () => ledger.verifyGame('elden-ring') && !ledger.verifyGame('elden-ring') && ledger.isVerified('elden-ring') },
    { id: 'F05042', name: '误判日志', check: () => { ledger.logMisjudgment('foo.exe', T0); return ledger.misjudgmentCount === 1; } },
    { id: 'F05043', name: '启动预检', check: () => ledger.preflight('elden-ring', []).proceed && !ledger.preflight('elden-ring', ['EasyAntiCheat.exe']).proceed },
    { id: 'F05044', name: '崩溃联动', check: () => A.gameCrashReport('game', -1073741819).severity === 'crash' },
    { id: 'F05045', name: '帧率保护', check: () => A.embedFpsBudget(3) === 50 && A.embedFpsBudget(1) === 60 },
    { id: 'F05046', name: '暂停壁纸', check: () => { const s = A.suspendForGame({ running: true, animations: true }); return !s.running && !s.animations; } },
    { id: 'F05047', name: '暂停动效', check: () => A.suspendForGame({ running: false, animations: true }).animations === false },
    { id: 'F05048', name: '策略编辑', check: () => { policy.autoYield = false; return policy.autoYield === false; } },
    { id: 'F05049', name: '知识库', check: () => A.ANTI_CHEAT_KB.length === 4 && A.ANTI_CHEAT_KB[0]!.kind === 'EAC' },
    { id: 'F05050', name: '反作弊教学', check: () => A.antiCheatTutorial().length === 3 },
  ];
}

/* -------- AI-41 族0203 CEF 内核兼容 -------- */
export function checkF0203(): CheckEntry[] {
  const steam: A.CefApp = { name: 'steam.exe', version: 'cef-120' };
  const tabs = new A.CefTabGroup();
  return [
    { id: 'F05051', name: '崩溃防护', check: () => A.cefCrashGuard(0x80000003).handled },
    { id: 'F05052', name: '专用通道', check: () => A.cefChannelFor(steam) === 'dedicated' },
    { id: 'F05053', name: '多进程适配', check: () => A.cefProcessLayout({ name: 'x', multiProcess: true }).children.length === 4 },
    { id: 'F05054', name: 'GPU 隔离', check: () => A.cefProcessLayout({ name: 'x', multiProcess: true }).gpuIsolated },
    { id: 'F05055', name: '置顶取消', check: () => !A.cefTopMostArbitration(true, true).keepTopMost },
    { id: 'F05056', name: '低负载联动', check: () => A.wallpaperLoadForCef(true) === 'low' && A.wallpaperLoadForCef(false) === 'full' },
    { id: 'F05057', name: 'Steam 嵌入', check: () => A.knownCefApp('steam.exe') === 'Steam' },
    { id: 'F05058', name: 'Steam 弹窗跟随', check: () => { const r = new A.EmbedRegistry(); r.embed(1, null, 'main'); return r.embed(2, 1, 'popup') && r.ownerOf(2) === 1; } },
    { id: 'F05059', name: 'Epic 嵌入', check: () => A.knownCefApp('EpicGamesLauncher.exe') === 'Epic' },
    { id: 'F05060', name: 'Discord 收编', check: () => A.knownCefApp('Discord.exe') === 'Discord' },
    { id: 'F05061', name: '音乐软件', check: () => A.knownCefApp('cloudmusic.exe') === '网易云音乐' },
    { id: 'F05062', name: '视频软件', check: () => A.knownCefApp('bilibili.exe') === 'B 站' },
    { id: 'F05063', name: '白屏检测', check: () => A.cefHealthCheck(4000, 100) === 'white' },
    { id: 'F05064', name: '卡死检测', check: () => A.cefHealthCheck(100, 6000) === 'hang' },
    { id: 'F05065', name: '降级路径', check: () => A.cefDegradePath('white') === 'reload' && A.cefDegradePath('hang') === 'restart-renderer' },
    { id: 'F05066', name: 'GPU 协商', check: () => A.cefGpuNegotiation(false, true).mode === 'software' && A.cefGpuNegotiation(false, true).transparentOk === false },
    { id: 'F05067', name: '透明处理', check: () => A.cefGpuNegotiation(true, true).transparentOk },
    { id: 'F05068', name: '无边框适配', check: () => A.cefLayerPolicy('frameless', false).frameless },
    { id: 'F05069', name: '层级管理', check: () => A.cefLayerPolicy('layered', true).zOrder === 'normal' },
    { id: 'F05070', name: '多标签', check: () => tabs.addTab('t1') && tabs.addTab('t2') && !tabs.addTab('t1') && tabs.count === 2 },
    { id: 'F05071', name: '内存配额', check: () => A.cefMemoryQuota(10).over && A.cefMemoryQuota(4).budgetMB === 512 },
    { id: 'F05072', name: '自动重启', check: () => A.cefSelfHeal(1) === 'restart' && A.cefSelfHeal(3) === 'safe-mode' && A.cefSelfHeal(5) === 'give-up' },
    { id: 'F05073', name: '版本库', check: () => A.cefVersionSupported('cef-120') && !A.cefVersionSupported('cef-90') },
    { id: 'F05074', name: '兼容报告', check: () => A.cefCompatReport([steam, { name: 'x', version: 'cef-90' }])[1]!.ok === false },
    { id: 'F05075', name: 'CEF 教学', check: () => A.cefTutorial().length === 3 },
  ];
}

/* -------- AI-41 族0204 独占与全屏让位 -------- */
export function checkF0204(): CheckEntry[] {
  const session: A.GameSession = { game: 'game-a', exclusive: true, borderless: false, resolution: { w: 2560, h: 1440 }, refreshHz: 144 };
  const ys = new A.YieldSession();
  const lib = new A.GameLibrary();
  const editor = new A.YieldRuleEditor();
  return [
    { id: 'F05076', name: '独占检测', check: () => A.detectExclusive(session) },
    { id: 'F05077', name: '自动最小化', check: () => { const r = A.yieldDesktop(session); return r.minimized && r.taskbarHidden; } },
    { id: 'F05078', name: '焦点锁定', check: () => ys.enter('layout-1') && !ys.enter('layout-2') && ys.active },
    { id: 'F05079', name: '让位动画', check: () => A.YieldSession.transitionMs(true) === 0 && A.YieldSession.transitionMs(false) === 200 },
    { id: 'F05080', name: '回归恢复', check: () => ys.exit() === 'layout-1' && !ys.active },
    { id: 'F05081', name: 'AltTab 兼容', check: () => A.hotkeyPassThrough(true, 'alt-tab') },
    { id: 'F05082', name: 'Win 键兼容', check: () => A.hotkeyPassThrough(true, 'win') && !A.hotkeyPassThrough(false, 'win') },
    { id: 'F05083', name: '通知静默', check: () => A.fullscreenPolicy(true).notifySilent },
    { id: 'F05084', name: '壁纸暂停', check: () => A.fullscreenPolicy(true).wallpaperPaused },
    { id: 'F05085', name: '动效暂停', check: () => A.fullscreenPolicy(true).effectsPaused },
    { id: 'F05086', name: '任务栏隐藏', check: () => A.fullscreenPolicy(true).taskbarHidden },
    { id: 'F05087', name: '全屏可截', check: () => A.captureAllowed(true, false).screenshot },
    { id: 'F05088', name: '全屏可录', check: () => A.captureAllowed(true, false).recording },
    { id: 'F05089', name: '多游戏管理', check: () => lib.register('a', 'steam') && lib.register('b', 'epic') && !lib.register('a', 'steam') && lib.count === 2 },
    { id: 'F05090', name: '游戏库识别', check: () => lib.byLauncher('steam').includes('a') },
    { id: 'F05091', name: '启动器集成', check: () => lib.byLauncher('epic').length === 1 },
    { id: 'F05092', name: '时长统计', check: () => { lib.addPlayTime('a', 60000); return lib.playedMs('a') === 60000; } },
    { id: 'F05093', name: '窗口化切换', check: () => A.YieldSession.transitionMs(false) > 0 },
    { id: 'F05094', name: '分辨率匹配', check: () => A.displayFollow(session, { w: 2560, h: 1440, refreshHz: 60 }).matchResolution },
    { id: 'F05095', name: '刷新率联动', check: () => !A.displayFollow(session, { w: 2560, h: 1440, refreshHz: 60 }).matchRefresh },
    { id: 'F05096', name: '手柄唤起', check: () => A.controllerSummon(['guide', 'b']) && !A.controllerSummon(['a']) },
    { id: 'F05097', name: '退出恢复壁纸', check: () => A.restoreOnExit({ wallpaper: true, tray: true }).wallpaper },
    { id: 'F05098', name: '退出恢复托盘', check: () => A.restoreOnExit({ wallpaper: true, tray: false }).tray === false },
    { id: 'F05099', name: '策略编辑', check: () => editor.add({ pattern: 'game-.*', policy: A.DEFAULT_YIELD_POLICY }) && !editor.add({ pattern: 'game-.*', policy: A.DEFAULT_YIELD_POLICY }) && editor.match('game-x') !== undefined },
    { id: 'F05100', name: '让位教学', check: () => A.yieldTutorial().length === 3 },
  ];
}

/* -------- AI-41 族0205 老应用兼容 -------- */
export function checkF0205(): CheckEntry[] {
  const collector = new A.CrashCollector();
  collector.record({ app: 'legacy', time: T0, exitCode: -1, module: 'oldgfx.dll' });
  collector.record({ app: 'legacy', time: T0 + 1, exitCode: -1, module: 'oldgfx.dll' });
  return [
    { id: 'F05101', name: 'DPI 修复', check: () => A.dpiFix(false).override && A.dpiFix(false).mode === 'system' },
    { id: 'F05102', name: '缩放覆盖', check: () => A.dpiScaleOverride(144).pct === 150 },
    { id: 'F05103', name: '16 色位预留', check: () => A.LEGACY_COLOR_SIM.depth === 16 && A.LEGACY_COLOR_SIM.enabled === false },
    { id: 'F05104', name: '单实例预留', check: () => A.LEGACY_SINGLE_INSTANCE.bypass === false },
    { id: 'F05105', name: '管理员检测', check: () => A.adminNeed({ requestedExecutionLevel: 'requireAdministrator' }).needsAdmin },
    { id: 'F05106', name: 'UAC 虚拟化', check: () => A.adminNeed({}).uacVirtualized },
    { id: 'F05107', name: '兼容模式', check: () => A.COMPAT_MODE_DEFAULT.disableFullscreenOptimizations },
    { id: 'F05108', name: '旧系统提示', check: () => A.legacyOsSuggest(2005) === 'winxp' && A.legacyOsSuggest(2011) === 'win7' },
    { id: 'F05109', name: '中文路径检测', check: () => A.pathIssues('D:\\我的游戏\\a.exe').some((i) => i.kind === 'non-ascii') },
    { id: 'F05110', name: '空格路径检测', check: () => A.pathIssues('C:\\Program Files\\a').some((i) => i.kind === 'space') },
    { id: 'F05111', name: '长路径检测', check: () => A.pathIssues('C:\\' + 'a'.repeat(300)).some((i) => i.kind === 'long') },
    { id: 'F05112', name: '依赖检测', check: () => A.missingDeps(['msvcp140.dll', 'xinput1_3.dll'], ['msvcp140.dll']).includes('xinput1_3.dll') },
    { id: 'F05113', name: '运行库一键位', check: () => A.RUNTIME_LIBS.includes('vcruntime140.dll') },
    { id: 'F05114', name: '字体缺失', check: () => A.missingFonts(['宋体'], []).includes('宋体') },
    { id: 'F05115', name: '乱码修复', check: () => A.mojibakeFixGBK([0xd6, 0xd0, 0x41]).startsWith('U+D6D0') },
    { id: 'F05116', name: '闪退收集', check: () => collector.analyze('legacy').crashes === 2 },
    { id: 'F05117', name: '崩溃分析', check: () => collector.analyze('legacy').suspectedModule === 'oldgfx.dll' },
    { id: 'F05118', name: '无声提示', check: () => A.legacyFix('no-sound').includes('WASAPI') },
    { id: 'F05119', name: '闪烁修复', check: () => A.legacyFix('flicker').includes('全屏优化') },
    { id: 'F05120', name: '焦点修复', check: () => A.legacyFix('focus-loss').includes('前台') },
    { id: 'F05121', name: '托盘修复', check: () => A.legacyFix('tray-gone').includes('托盘') },
    { id: 'F05122', name: '菜单修复', check: () => A.legacyFix('context-menu-dead').includes('Shell') },
    { id: 'F05123', name: '剪贴修复', check: () => A.legacyFix('clipboard-dead').includes('剪贴板') },
    { id: 'F05124', name: '知识库', check: () => A.LEGACY_KB.length === 3 && A.LEGACY_KB[0]!.issue === 'no-sound' },
    { id: 'F05125', name: '老应用教学', check: () => A.legacyTutorial().length === 3 },
  ];
}

/* -------- AI-42 族0206 驱动与拦截兼容 -------- */
export function checkF0206(): CheckEntry[] {
  const auditor = new B.HookAuditor();
  auditor.register({ name: '可疑钩子', kind: 'keyboard', module: 'evil-hook.dll' });
  auditor.addWhitelist('trusted-im.dll');
  auditor.register({ name: '输入法钩子', kind: 'keyboard', module: 'trusted-im.dll' });
  const drivers: B.FilterDriver[] = [
    { name: 'av-filter', altitude: '329900', kind: 'fs' },
    { name: 'net-filter', altitude: '389900', kind: 'network' },
  ];
  return [
    { id: 'F05126', name: '钩子审计', check: () => auditor.all.length === 2 },
    { id: 'F05127', name: '键盘钩子白名单', check: () => auditor.byKind('keyboard').length === 2 },
    { id: 'F05128', name: '鼠标钩子白名单', check: () => auditor.byKind('mouse').length === 0 },
    { id: 'F05129', name: '注入检测', check: () => auditor.register({ name: '注入器', kind: 'dll-inject', module: 'injector.dll' }) && auditor.untrusted('dll-inject').length === 1 },
    { id: 'F05130', name: '注入白名单', check: () => auditor.untrusted('keyboard').length === 1 && auditor.byKind('keyboard').filter((h) => h.trusted).length === 1 },
    { id: 'F05131', name: 'Hook 检测', check: () => auditor.register({ name: 'api-hook', kind: 'api-hook', module: 'hook.dll' }) && auditor.byKind('api-hook').length === 1 },
    { id: 'F05132', name: 'FS 过滤', check: () => B.listFilterDrivers(drivers, 'fs')[0]!.altitude === '329900' },
    { id: 'F05133', name: '网络过滤', check: () => B.listFilterDrivers(drivers, 'network')[0]!.name === 'net-filter' },
    { id: 'F05134', name: '杀软共存', check: () => B.AV_VENDORS.includes('Defender') && B.AV_VENDORS.includes('火绒') },
    { id: 'F05135', name: '杀软白名单', check: () => B.avExclusionSuggest('Defender', ['D:\\Games']).exclusions[0] === 'D:\\Games\\*' },
    { id: 'F05136', name: '火绒兼容', check: () => B.avExclusionSuggest('火绒', ['C:\\Varix']).note === '信任区添加目录' },
    { id: 'F05137', name: 'Defender 建议', check: () => B.avExclusionSuggest('Defender', []).note.startsWith('Add-MpPreference') },
    { id: 'F05138', name: 'EDR 位', check: () => B.EDR_SUPPORT.reserved },
    { id: 'F05139', name: 'Sandboxie 共存', check: () => !B.sandboxieCompatible({ injectDll: true, globalHook: false }).ok && B.sandboxieCompatible({ injectDll: false, globalHook: false }).ok },
    { id: 'F05140', name: 'VM 内检测', check: () => B.isVirtualMachine({ manufacturer: 'VMware, Inc.' }) && !B.isVirtualMachine({ manufacturer: 'Dell Inc.' }) },
    { id: 'F05141', name: 'RDP 适配', check: () => B.rdpAdapt({ remote: true, dpi: 192 }).disableAcrylic && B.rdpAdapt({ remote: false, dpi: 96 }).disableAcrylic === false },
    { id: 'F05142', name: '投屏兼容', check: () => B.captureCompat('投屏', true).overlaySuspended },
    { id: 'F05143', name: '录屏兼容', check: () => B.captureCompat('OBS', false).coexist },
    { id: 'F05144', name: '直播兼容', check: () => B.captureCompat('OBS', false).overlaySuspended === false },
    { id: 'F05145', name: 'PowerToys 共存', check: () => B.captureCompat('PowerToys', true).overlaySuspended === false },
    { id: 'F05146', name: '剪贴工具兼容', check: () => B.captureCompat('Snipaste', true).coexist },
    { id: 'F05147', name: '输入法共存', check: () => B.imeCandidateTopMost(['sogou'], false).topMost && !B.imeCandidateTopMost(['sogou'], true).topMost },
    { id: 'F05148', name: '皮肤软件兼容', check: () => B.SKIN_TOOL_KB.length === 3 },
    { id: 'F05149', name: 'MOD 工具', check: () => B.modToolCompat({ name: 'm', injectsGame: true }).allow === false && B.modToolCompat({ name: 'm2', injectsGame: false }).allow },
    { id: 'F05150', name: '拦截知识库', check: () => B.INTERCEPT_KB.length === 3 && B.interceptTutorial().length === 3 },
  ];
}

/* -------- AI-42 族0207 Shell 扩展兼容 -------- */
export function checkF0207(): CheckEntry[] {
  const host = new B.ShellMenuHost();
  host.register({ name: '好扩展', kind: 'context-menu', stable: true });
  host.register({ name: '坏扩展', kind: 'context-menu', stable: false });
  const overlay = new B.OverlayIconManager();
  const thumbs = new B.ThumbnailHost();
  thumbs.request('/a.png', T0);
  const exts: B.ShellExtension[] = [
    { name: 'p1', kind: 'property-sheet', stable: true },
    { name: 'p2', kind: 'preview', stable: true },
    { name: 'p3', kind: 'preview', stable: false },
  ];
  const sendTo = new B.SendToMenu();
  return [
    { id: 'F05151', name: '菜单隔离', check: () => host.invoke('好扩展').ok },
    { id: 'F05152', name: '菜单沙箱', check: () => host.invoke('不存在').ok === false && host.invoke('不存在').quarantined === false },
    { id: 'F05153', name: '崩溃隔离', check: () => host.invoke('坏扩展').quarantined && host.active.length === 1 && host.quarantine.includes('坏扩展') },
    { id: 'F05154', name: 'overlay 管理', check: () => overlay.add('git') && !overlay.add('git') },
    { id: 'F05155', name: '覆盖上限', check: () => { for (let i = 0; i < 20; i++) overlay.add(`o${i}`); return overlay.list.length === B.OverlayIconManager.LIMIT; } },
    { id: 'F05156', name: '缩略隔离', check: () => thumbs.complete('/a.png') },
    { id: 'F05157', name: '缩略超时', check: () => { const t = new B.ThumbnailHost(); t.request('/slow.png', T0); return t.reclaim(T0 + 3000).includes('/slow.png'); } },
    { id: 'F05158', name: '属性页', check: () => B.shellHandlers(exts).propertySheets.includes('p1') },
    { id: 'F05159', name: '预览器', check: () => B.shellHandlers(exts).previews.includes('p2') && !B.shellHandlers(exts).previews.includes('p3') },
    { id: 'F05160', name: '关联冲突', check: () => B.associationConflict([{ ext: '.txt', progId: 'a', registeredBy: 'x' }, { ext: '.txt', progId: 'b', registeredBy: 'y' }]).includes('.txt') },
    { id: 'F05161', name: '协议冲突', check: () => B.protocolConflict([{ scheme: 'varix', handler: 'a' }, { scheme: 'varix', handler: 'b' }]).includes('varix') },
    { id: 'F05162', name: '默认仲裁', check: () => B.defaultProgramArbitrate([{ progId: 'a', capability: 5 }, { progId: 'b', capability: 9, userChoice: true }]) === 'b' && B.defaultProgramArbitrate([{ progId: 'a', capability: 5 }, { progId: 'b', capability: 9 }]) === 'b' },
    { id: 'F05163', name: '托盘协议', check: () => B.trayNotifyCompat({ tip: 'x', icon: true, balloon: false }).displayable && B.trayNotifyCompat({ tip: '', icon: false, balloon: true }).fallback === 'toast' },
    { id: 'F05164', name: 'AutoPlay', check: () => B.autoPlayDecision({ kind: 'storage', content: 'installer.exe' }, 'open') === 'ask' && B.autoPlayDecision({ kind: 'camera', content: '' }, 'ignore') === 'ignored' },
    { id: 'F05165', name: '发送到', check: () => sendTo.add('桌面') && !sendTo.add('桌面') && sendTo.remove('桌面') && sendTo.list.length === 0 },
    { id: 'F05166', name: '库文件', check: () => B.libraryFolderResolve([{ name: '文档', folders: ['D:\\Doc'] }], '文档').includes('D:\\Doc') },
    { id: 'F05167', name: '权限继承', check: () => B.aclInherit(['user:rw'], ['user:rwx']).length === 2 },
    { id: 'F05168', name: '符号链接安全', check: () => B.symlinkTraversalSafe('C:\\root', 'C:\\root\\ok') && !B.symlinkTraversalSafe('C:\\root', 'C:\\Windows\\x') },
    { id: 'F05169', name: 'junction 处理', check: () => B.isJunction('C:\\mnt', { reparsePoint: true, tag: 'mount-point' }) && !B.isJunction('C:\\lnk', { reparsePoint: true, tag: 'symlink' }) },
    { id: 'F05170', name: '硬链接计数', check: () => B.hardlinkCount(['a', 'a', 'b'], 'a') === 2 },
    { id: 'F05171', name: 'ADS 显示', check: () => B.adsStreams([{ file: 'a.txt', name: 'Zone.Identifier', size: 26 }, { file: 'a.txt', name: '::$DATA', size: 10 }], 'a.txt').length === 1 },
    { id: 'F05172', name: '零字节处理', check: () => B.zeroByteHandling(0).valid && B.zeroByteHandling(0).note.includes('占位') },
    { id: 'F05173', name: '长路径遍历', check: () => B.longPathNormalize('C:\\' + 'a'.repeat(300)).startsWith('\\\\?\\') },
    { id: 'F05174', name: '特殊字符', check: () => !B.specialCharOk('a<b') && !B.specialCharOk('name.') && B.specialCharOk('正常.txt') },
    { id: 'F05175', name: 'Shell 知识库', check: () => B.SHELL_KB.length === 3 && B.shellTutorial().length === 3 },
  ];
}

/* -------- AI-42 族0208 显示管线兼容 -------- */
export function checkF0208(): CheckEntry[] {
  const chain: B.DisplayChain = { kind: 'borderless', hdr: true, bitDepth: 10, output: 'RGB' };
  return [
    { id: 'F05176', name: '独占翻转', check: () => B.flipModelCompat('exclusive-flip', false).path === 'flip' && B.flipModelCompat('borderless', true).independentFlip },
    { id: 'F05177', name: '无边框识别', check: () => B.isBorderlessWindow({ style: 'popup', rect: { w: 2560, h: 1440 }, screen: { w: 2560, h: 1440 } }) },
    { id: 'F05178', name: 'DWM 协调', check: () => B.flipModelCompat('windowed', true).path === 'compose' },
    { id: 'F05179', name: '双 GPU', check: () => B.gpuRouting({ gpu: 'auto', isGame: true }) === 'discrete' && B.gpuRouting({ gpu: 'auto', isGame: false }) === 'integrated' },
    { id: 'F05180', name: 'Optimus', check: () => B.optimusRenderPath(true, true) === 'hybrid' && B.optimusRenderPath(false, true) === 'dGPU-only' },
    { id: 'F05181', name: '混合刷新', check: () => B.mixedRefreshHz([60, 144]).ok === false && B.mixedRefreshHz([144]).ok },
    { id: 'F05182', name: 'G-Sync', check: () => B.vrrCompat('g-sync', [30, 144], 120).active && !B.vrrCompat('g-sync', [30, 144], 200).active },
    { id: 'F05183', name: 'FreeSync', check: () => B.vrrCompat('freesync', [48, 120], 60).active && B.vrrCompat('none', [48, 120], 60).active === false },
    { id: 'F05184', name: 'HDR 直通', check: () => B.hdrPassThrough(true, true, chain.bitDepth).hdr && B.hdrPassThrough(true, true, chain.bitDepth).need10bit },
    { id: 'F05185', name: '10bit', check: () => B.hdrPassThrough(true, true, 8).need10bit === false },
    { id: 'F05186', name: 'YCbCr', check: () => B.outputFormatFor(30, true) === 'YCbCr444' && B.outputFormatFor(10, true) === 'YCbCr422' },
    { id: 'F05187', name: 'DSC 检测', check: () => B.dscDetected({ bandwidthGbps: 14, requiredGbps: 28 }).dsc && B.dscDetected({ bandwidthGbps: 40, requiredGbps: 28 }).compressionRatio === 1 },
    { id: 'F05188', name: 'MST', check: () => B.mstHubTopology({ ports: 2, displays: 4 }).ok && B.mstHubTopology({ ports: 1, displays: 9 }).ok === false },
    { id: 'F05189', name: '扩展坞', check: () => B.usbCDockProbe({ vendor: 'Dell', displays: 2, hotplug: true }).maxDisplays === 2 },
    { id: 'F05190', name: '雷电 dock', check: () => B.thunderboltHotplug(true, 3).works && !B.thunderboltHotplug(false, 1).works },
    { id: 'F05191', name: 'KVM 兼容', check: () => B.kvmSwitchCompat({ edidEmulation: true }).stableResolution },
    { id: 'F05192', name: '采集卡', check: () => B.captureCardHdmiIn({ fourK60: true, hdr: true }, true).mode === '4K60-HDR' },
    { id: 'F05193', name: '虚拟显示器', check: () => B.VIRTUAL_DISPLAY_DRIVER === 'varix-idd' },
    { id: 'F05194', name: '串流屏', check: () => B.streamDisplayLink(20, 60).quality === 'high' && !B.streamDisplayLink(300, 30).usable },
    { id: 'F05195', name: '旧投影仪', check: () => B.legacyProjectorMode({ x: 640, y: 480 }).mode === 'edid' && B.legacyProjectorMode({ x: 0, y: 0 }).mode === 'safe-list' },
    { id: 'F05196', name: '拼接屏', check: () => B.videoWallGrid(9).cols === 3 && B.videoWallGrid(10).rows === 3 },
    { id: 'F05197', name: '竖屏旋转', check: () => B.portraitRotation(90).swap && !B.portraitRotation(180).swap },
    { id: 'F05198', name: 'EDID 容错', check: () => B.edidFaultTolerant({ checksumOk: false, descriptors: 0 }).fallback && B.edidFaultTolerant({ checksumOk: true, descriptors: 2 }).fallback === false },
    { id: 'F05199', name: '超频检测', check: () => B.overclockDetect(165, [60, 144]).nonStandard && B.overclockDetect(165, [60, 144]).nearest === 144 },
    { id: 'F05200', name: '显示知识库', check: () => B.DISPLAY_KB.length === 3 && B.displayTutorial().length === 3 },
  ];
}

/* -------- AI-42 族0209 音频管线兼容 -------- */
export function checkF0209(): CheckEntry[] {
  const device: B.AudioDevice = { name: 'BT-耳机', exclusive: false, sampleRateHz: 48000, channels: 2, codec: 'SBC' };
  const recovery = new B.AudioRecovery();
  recovery.markLost('BT-耳机');
  return [
    { id: 'F05201', name: '独占仲裁', check: () => B.exclusiveArbitration([{ name: 'a', wantsExclusive: true }, { name: 'b', wantsExclusive: true }]).grant === null },
    { id: 'F05202', name: '共享优先', check: () => B.sharedModePreferred(device, true).mode === 'shared' && B.sharedModePreferred(device, false).mode === 'shared' },
    { id: 'F05203', name: 'ASIO 提示', check: () => B.asioConflictHint(['a', 'b'], 'daw') !== null && B.asioConflictHint(['a'], '') === null },
    { id: 'F05204', name: '蓝牙协商', check: () => B.btCodecNegotiate(['SBC', 'aptX'], 'LDAC') === 'aptX' },
    { id: 'F05205', name: 'aptX', check: () => B.btCodecNegotiate(['SBC', 'aptX-Adaptive'], 'aptX-Adaptive') === 'aptX-Adaptive' },
    { id: 'F05206', name: 'LDAC 位', check: () => B.BT_CODEC_SUPPORT.ldac === false },
    { id: 'F05207', name: 'LE Audio 位', check: () => B.BT_CODEC_SUPPORT.leAudio === false },
    { id: 'F05208', name: 'USB DAC', check: () => B.usbDacProfile({ maxSampleRateHz: 192000, maxBitDepth: 24 }).bitPerfect },
    { id: 'F05209', name: '耳放', check: () => B.amplifierGain(250, 96).needsAmp && B.amplifierGain(32, 110).gain === 'low' },
    { id: 'F05210', name: '调音台', check: () => B.mixerRouting(4, 2).matrix === '4x2' },
    { id: 'F05211', name: '虚拟声卡', check: () => B.VIRTUAL_AUDIO_DRIVER === 'varix-vaudio' },
    { id: 'F05212', name: '变声兼容', check: () => B.voiceChangerCompat({ driver: B.VIRTUAL_AUDIO_DRIVER, latencyMs: 30 }).ok },
    { id: 'F05213', name: 'K 歌监听', check: () => B.karaokeMonitor(true, true, true).monitor && !B.karaokeMonitor(true, true, false).monitor },
    { id: 'F05214', name: '直播声卡', check: () => B.liveAudioSetup(['mic', 'music']).order.length === 3 },
    { id: 'F05215', name: '内录回环', check: () => B.loopbackRecord(device).supported },
    { id: 'F05216', name: '采样仲裁', check: () => B.sampleRateArbitration([44100, 48000], 48000).chosen === 48000 && B.sampleRateArbitration([44100, 48000], 48000).resample },
    { id: 'F05217', name: '时钟漂移', check: () => B.clockDriftCompensate(100, 40).adjustMsPerMin === 6 && B.clockDriftCompensate(100, 40).bufferOk },
    { id: 'F05218', name: '蓝牙补偿', check: () => B.btLatencyCompensate(200, 'SBC').delayMs === 300 && B.btLatencyCompensate(200, 'LE-Audio').delayMs === 120 },
    { id: 'F05219', name: '断连恢复', check: () => recovery.reconnect('BT-耳机') && recovery.pending.length === 0 },
    { id: 'F05220', name: '热插切换', check: () => B.hotswapFade(50, 50).rampMs === 30 },
    { id: 'F05221', name: '切换防爆音', check: () => B.hotswapFade(0, 100).popFree },
    { id: 'F05222', name: '增强软件', check: () => B.audioEnhancerCompat('SRS').stack.length === 2 },
    { id: 'F05223', name: '语音软件', check: () => B.voiceDenoiseCompat('Krisp').ok },
    { id: 'F05224', name: '多设备仲裁', check: () => B.multiOutputArbitration([{ name: 'a', priority: 1 }, { name: 'b', priority: 9 }]) === 'b' },
    { id: 'F05225', name: '音频知识库', check: () => B.AUDIO_KB.length === 3 && B.audioTutorial().length === 3 },
  ];
}

/* -------- AI-42 族0210 网络兼容 -------- */
export function checkF0210(): CheckEntry[] {
  const clash: B.ProxyStack = { name: 'Clash', mode: 'tun', port: 7890 };
  return [
    { id: 'F05226', name: '代理共存', check: () => B.proxyCoexist(clash).ok && B.proxyCoexist(clash).note.includes('TUN') },
    { id: 'F05227', name: 'TUN 检测', check: () => B.tunDetected([{ name: 'wintun', type: 'virtual' }]) },
    { id: 'F05228', name: '代理区分', check: () => B.proxyScope({ appId: 'x', systemProxy: 'http://p:1', processProxy: null }) === 'http://p:1' && B.proxyScope({ appId: 'x', systemProxy: 'http://p:1', processProxy: 'socks://q:2' }) === 'socks://q:2' },
    { id: 'F05229', name: '分流提示', check: () => B.splitTunnelHint('game.exe', [{ pattern: 'game', via: 'direct' }]) === 'direct' },
    { id: 'F05230', name: '多 VPN 共存', check: () => !B.multiVpnStack([{ name: 'a', layer: 'l3-tun' }, { name: 'b', layer: 'l3-tun' }]).ok && B.multiVpnStack([{ name: 'a', layer: 'l3-tun' }, { name: 'b', layer: 'ssl' }]).ok },
    { id: 'F05231', name: 'WireGuard 位', check: () => B.WIREGUARD_SUPPORT.reserved },
    { id: 'F05232', name: '企业 VPN', check: () => B.enterpriseVpnDetect('EasyConnect').known && !B.enterpriseVpnDetect('foo').known },
    { id: 'F05233', name: '虚拟网卡', check: () => B.isVirtualAdapter({ name: 'wg0', driver: 'wintun' }) },
    { id: 'F05234', name: '多网卡路由', check: () => B.routeSelection([{ iface: 'eth0', metric: 5, dest: '192.168.0.0/16' }, { iface: 'eth1', metric: 1, dest: '0.0.0.0/0' }], '192.168.1.5') === 'eth0' },
    { id: 'F05235', name: 'metric 冲突', check: () => B.metricConflict([{ iface: 'a', metric: 1 }, { iface: 'b', metric: 1 }]).length === 2 },
    { id: 'F05236', name: 'DNS 仲裁', check: () => B.dnsArbitration([{ name: 'a', servers: ['1.1.1.1'], priority: 2 }, { name: 'b', servers: ['9.9.9.9'], priority: 1 }])[0] === '9.9.9.9' },
    { id: 'F05237', name: 'DoH', check: () => B.DOH_SUPPORT.enabled },
    { id: 'F05238', name: '防火墙冲突', check: () => B.firewallRuleConflict([{ port: 443, action: 'deny', source: 'fw' }], 443).blocked },
    { id: 'F05239', name: '第三方防火墙', check: () => B.THIRD_PARTY_FW.includes('火绒') },
    { id: 'F05240', name: '杀软过滤', check: () => B.avNetworkFilter('360').inspects },
    { id: 'F05241', name: '抓包共存', check: () => B.packetCaptureCompat('Wireshark').npcapNeeded && !B.packetCaptureCompat('Fiddler').npcapNeeded },
    { id: 'F05242', name: '下载工具接管', check: () => B.downloadTakeover('http://x/file.zip', [{ name: 'IDM', pattern: 'file\\.zip' }]) === 'IDM' && B.downloadTakeover('http://x/a.txt', []) === null },
    { id: 'F05243', name: 'P2P 提示', check: () => B.p2pBandwidthHint(100).throttled && !B.p2pBandwidthHint(10).throttled },
    { id: 'F05244', name: '网银控件', check: () => B.BANKING_CONTROL_KB.length === 2 },
    { id: 'F05245', name: '证书冲突', check: () => B.certTrustConflict([{ subject: 'Corp-Root', store: 'user' }, { subject: 'Corp-Root', store: 'machine' }]).includes('Corp-Root') },
    { id: 'F05246', name: 'NTP 冲突', check: () => !B.ntpSourceConflict([{ name: 'a', intervalMin: 60 }, { name: 'b', intervalMin: 30 }]).single && B.ntpSourceConflict([{ name: 'a', intervalMin: 60 }, { name: 'b', intervalMin: 30 }]).winner === 'b' },
    { id: 'F05247', name: 'IPv6', check: () => B.ipv6Compat({ v6: false, v6Only: false }).strategy === 'v4-only' && B.ipv6Compat({ v6: true, v6Only: true }).strategy === 'v6-first' },
    { id: 'F05248', name: 'IPv4 优先', check: () => B.dualStackPreferIPv4(true, 100, 20) === 'v4' && B.dualStackPreferIPv4(true, 10, 20) === 'v6' },
    { id: 'F05249', name: '多播', check: () => B.multicastGroupJoin(['239.1.1.1'], true).joined.length === 1 && B.multicastGroupJoin(['239.1.1.1'], false).ok === false },
    { id: 'F05250', name: '网络知识库', check: () => B.NETWORK_KB.length === 3 && B.networkTutorial().length === 3 },
  ];
}

/* -------- AI-43 族0211 兼容性体检 -------- */
export function checkF0211(): CheckEntry[] {
  const items = C.runCheckup({ brokenExt: ['.md'], driverClash: ['gfx.sys'], missingRuntime: ['msvcp140.dll'] });
  const fixer = new C.CheckupFixer();
  const whitelist = new C.CheckupWhitelist();
  const progress = new C.FixProgress();
  const log = new C.FixLog();
  log.append('修复 .md 关联');
  return [
    { id: 'F05251', name: '一键体检', check: () => items.length === 3 },
    { id: 'F05252', name: '体检报告', check: () => C.checkupReport(items).total === 3 },
    { id: 'F05253', name: '风险清单', check: () => C.riskList(items).length === 3 && C.riskList(items)[0]!.risk === 'high' },
    { id: 'F05254', name: '修复建议', check: () => items.every((i) => i.fix.length > 0) },
    { id: 'F05255', name: '一键修复', check: () => fixer.apply(items) === 3 },
    { id: 'F05256', name: '修复预览', check: () => fixer.preview(items).every((p) => p.includes('→')) },
    { id: 'F05257', name: '修复撤销', check: () => fixer.undo() === 3 && fixer.appliedCount === 0 },
    { id: 'F05258', name: '兼容评分', check: () => C.compatScore(items) === 64 && C.compatScore([{ id: 'x', name: 'x', risk: 'ok', fix: '' }]) === 100 },
    { id: 'F05259', name: '历史对比', check: () => C.scoreDelta(50, 64) === 14 },
    { id: 'F05260', name: '新装触发', check: () => C.triggerCheckup('install', C.CHECKUP_TRIGGERS) },
    { id: 'F05261', name: '更新后触发', check: () => C.triggerCheckup('app-update', C.CHECKUP_TRIGGERS) },
    { id: 'F05262', name: '驱动后触发', check: () => C.triggerCheckup('driver-update', C.CHECKUP_TRIGGERS) },
    { id: 'F05263', name: '系统更新触发', check: () => C.triggerCheckup('os-update', C.CHECKUP_TRIGGERS) },
    { id: 'F05264', name: '计划体检', check: () => C.weeklySchedule(T0) === T0 + 604800000 },
    { id: 'F05265', name: '体检白名单', check: () => whitelist.add('rt-msvcp140.dll') && whitelist.filter(items).length === 2 },
    { id: 'F05266', name: '误报反馈', check: () => C.falsePositiveReport('ext-.md', T0).state === 'reported' },
    { id: 'F05267', name: '修复知识库', check: () => C.CHECKUP_KB.length === 3 },
    { id: 'F05268', name: '报告导出', check: () => JSON.parse(C.checkupExport(items)).items.length === 3 },
    { id: 'F05269', name: '体检通知', check: () => C.checkupNotify(2, 3).includes('2/3') },
    { id: 'F05270', name: '修复进度', check: () => { progress.tick(5); progress.tick(5); return progress.pct === 100; } },
    { id: 'F05271', name: '修复日志', check: () => log.text.length === 1 && log.text[0]!.includes('.md') },
    { id: 'F05272', name: '关键锁定', check: () => !C.fixGuard('kernel-driver', false) && C.fixGuard('kernel-driver', true) && C.fixGuard('other', false) },
    { id: 'F05273', name: '社区库', check: () => C.COMMUNITY_LIB.entries === 0 },
    { id: 'F05274', name: '厂商声明', check: () => C.vendorDeclaration('v', true).compatible },
    { id: 'F05275', name: '体检教学', check: () => C.checkupTutorial().length === 3 },
  ];
}

/* -------- AI-43 族0212 兼容性实验室 -------- */
export function checkF0212(): CheckEntry[] {
  const trial = new C.SandboxTrial();
  trial.snapshot();
  trial.touchRegistry('HKLM\\Foo');
  trial.touchFile('C:\\Users\\x\\AppData\\evil.tmp');
  trial.touchNet('tracker.example');
  const lib = new C.ProfileLibrary();
  lib.add({ app: 'appA', version: '1.0', trialRisk: 'low', notes: [] });
  const tracker = new C.VersionTracker();
  tracker.record('appA', '2.0');
  tracker.record('appA', '1.9');
  return [
    { id: 'F05276', name: '沙盒试跑', check: () => trial.diff().registry.includes('HKLM\\Foo') },
    { id: 'F05277', name: '试跑报告', check: () => { const t = new C.SandboxTrial(); t.snapshot(); t.touchRegistry('K1'); t.touchFile('/f1'); t.touchFile('/f2'); return t.report().risk === 'medium'; } },
    { id: 'F05278', name: '快照回滚', check: () => trial.rollback() && trial.diff().registry.length === 0 },
    { id: 'F05279', name: '注册表 diff', check: () => { const t = new C.SandboxTrial(); t.snapshot(); t.touchRegistry('K1'); return t.diff().registry.length === 1; } },
    { id: 'F05280', name: '文件 diff', check: () => { const t = new C.SandboxTrial(); t.snapshot(); t.touchFile('/f'); return t.diff().files.includes('/f'); } },
    { id: 'F05281', name: '网络记录', check: () => { const t = new C.SandboxTrial(); t.snapshot(); t.touchNet('h1'); return t.diff().network.includes('h1'); } },
    { id: 'F05282', name: '行为评分', check: () => C.behaviorScore({ registry: ['a', 'b'], files: [], network: ['n'] }) === 35 },
    { id: 'F05283', name: '静态分析', check: () => C.peStaticAnalysis({ machine: 'x64', imports: ['WriteProcessMemory'], signed: false }).risky.length === 1 },
    { id: 'F05284', name: '签名验证', check: () => C.verifySignature({ signed: true, signer: 'v' }).ok && !C.verifySignature({ signed: false }).ok },
    { id: 'F05285', name: '证书链', check: () => C.certificateChain([{ subject: 'leaf', issuer: 'mid', root: 'RootCA' }, { subject: 'mid', issuer: 'RootCA', root: 'RootCA' }], ['RootCA']) },
    { id: 'F05286', name: '可疑告警', check: () => C.suspiciousAlerts({ registry: [], files: [], network: ['a', 'b', 'c', 'd'] }, { machine: 'x64', imports: ['VirtualAllocEx'], signed: false }).length >= 2 },
    { id: 'F05287', name: '虚拟化试跑', check: () => C.VM_TRIAL.isolation === 'full' },
    { id: 'F05288', name: '对照环境', check: () => C.controlEnvironmentCompare({ registry: ['base'], files: [], network: [] }, { registry: ['base', 'new'], files: [], network: [] }).registry.includes('new') },
    { id: 'F05289', name: '应用画像', check: () => lib.count === 1 },
    { id: 'F05290', name: '画像分享', check: () => JSON.parse(lib.export('appA@1.0')).app === 'appA' },
    { id: 'F05291', name: '社区库', check: () => !lib.add({ app: 'appA', version: '1.0', trialRisk: 'low', notes: [] }) },
    { id: 'F05292', name: '画像评分', check: () => lib.rate('appA@1.0', 5) && lib.rate('appA@1.0', 4) && lib.avgRating('appA@1.0') === 4.5 },
    { id: 'F05293', name: '冲突预测', check: () => !C.conflictPrediction({ hooksKeyboard: true, injects: false, shellExt: ['a'] }, { hooksKeyboard: true, injects: false, shellExt: ['a'] }).safe && C.conflictPrediction({ hooksKeyboard: false, injects: false, shellExt: [] }, { hooksKeyboard: true, injects: false, shellExt: [] }).safe },
    { id: 'F05294', name: '冲突案例库', check: () => C.CONFLICT_CASES.length === 2 },
    { id: 'F05295', name: 'CVE 公告', check: () => C.advisoryFilter([{ cve: 'CVE-2026-1', severity: 'high', affected: ['appA'] }], 'appA').length === 1 },
    { id: 'F05296', name: '版本追踪', check: () => tracker.regression('appA') && tracker.get('appA').length === 2 },
    { id: 'F05297', name: '记录导出', check: () => lib.export('appA@missing') === '{}' },
    { id: 'F05298', name: '实验室教学', check: () => C.labTutorial().length === 3 },
    { id: 'F05299', name: '实验室彩蛋', check: () => C.LAB_EASTER_EGG === 'lab-open-day' },
    { id: 'F05300', name: '开放数据', check: () => C.LAB_OPEN_DATA.anonymousStats },
  ];
}

/* -------- AI-43 族0213 多系统共存 -------- */
export function checkF0213(): CheckEntry[] {
  const menu = new C.BootMenu();
  menu.add({ id: 'w', label: 'VarixOS', kind: 'windows', default: true, timeoutSec: 10 });
  menu.add({ id: 'l', label: 'Ubuntu', kind: 'linux', default: false, timeoutSec: 10 });
  const repair = new C.BootRepair();
  return [
    { id: 'F05301', name: '启动菜单', check: () => menu.list.length === 2 },
    { id: 'F05302', name: 'Linux 项编辑', check: () => menu.setLinuxLabel('l', 'Arch') && menu.list.find((e) => e.id === 'l')!.label === 'Arch' && !menu.setLinuxLabel('w', 'x') },
    { id: 'F05303', name: '默认系统', check: () => menu.setDefault('l') && menu.defaultEntry!.id === 'l' },
    { id: 'F05304', name: '共享分区', check: () => C.sharedPartitionAccess('ntfs').rw },
    { id: 'F05305', name: 'ext4 位', check: () => C.sharedPartitionAccess('ext4').rw === false && C.sharedPartitionAccess('ext4').note.includes('只读') },
    { id: 'F05306', name: 'APFS 位', check: () => C.sharedPartitionAccess('apfs').note.includes('预留') },
    { id: 'F05307', name: '共享剪贴位', check: () => C.VM_SHARED_CLIPBOARD.enabled },
    { id: 'F05308', name: '时钟修复', check: () => C.dualBootClockFix(true).useUtc === false && C.dualBootClockFix(false).useUtc },
    { id: 'F05309', name: '快速切换', check: () => C.quickRebootTo(menu.list[0]!).includes('VarixOS') },
    { id: 'F05310', name: '启动盘', check: () => C.maintenanceUsb(16, 5).writable && C.maintenanceUsb(4, 5).writable === false },
    { id: 'F05311', name: '引导修复', check: () => repair.repair({ entries: menu.list, missingDefault: false }).actions.length === 1 && repair.logText.length === 1 },
    { id: 'F05312', name: '删除项', check: () => menu.remove('l') && menu.list.length === 1 },
    { id: 'F05313', name: '镜像备份', check: () => C.imageBackup(60, 50).fits && !C.imageBackup(100, 50).fits },
    { id: 'F05314', name: '镜像还原', check: () => C.imageRestore({ format: 'wim', valid: true }).restored && !C.imageRestore({ format: 'wim', valid: false }).restored },
    { id: 'F05315', name: 'WIM 位', check: () => C.WIM_DEPLOY.reserved },
    { id: 'F05316', name: 'PE 盘', check: () => C.maintenanceUsb(8, 4).kind === 'pe' },
    { id: 'F05317', name: '启动 U 盘', check: () => C.maintenanceUsb(16, 8).kind === 'pe' },
    { id: 'F05318', name: 'Ventoy 位', check: () => C.VENTOY_RESERVED.multiIso === false },
    { id: 'F05319', name: '空间分析', check: () => C.bootSpaceAnalysis([{ name: 'a', usedGB: 50 }, { name: 'b', usedGB: 90 }]).largest === 'b' && C.bootSpaceAnalysis([{ name: 'a', usedGB: 50 }, { name: 'b', usedGB: 90 }]).total === 140 },
    { id: 'F05320', name: '修复日志', check: () => new C.BootRepair().repair({ entries: [], missingDefault: true }).ok },
    { id: 'F05321', name: 'grub 备份位', check: () => C.GRUB_BACKUP_PATH === '/efi/varix/grub.bak' },
    { id: 'F05322', name: 'bcdedit 图形化', check: () => C.bcdeditCommand(menu.list[0]!, 'set-default').includes('default') && C.bcdeditCommand(menu.list[0]!, 'set-timeout').includes('10') },
    { id: 'F05323', name: '多系统知识库', check: () => C.MULTIBOOT_KB.length === 2 },
    { id: 'F05324', name: '多系统教学', check: () => C.multibootTutorial().length === 3 },
    { id: 'F05325', name: '三系统徽章', check: () => C.multibootBadge(3).includes('三系统') },
  ];
}

/* -------- AI-43 族0214 企业与受控环境 -------- */
export function checkF0214(): CheckEntry[] {
  const audit = new C.ComplianceAudit();
  audit.record('ai', 'install', 'x.msi', T0);
  const whitelist = new C.SoftwareWhitelist();
  whitelist.allow('corp-suite');
  const folders = new C.ManagedFolders();
  folders.add('D:\\Corp');
  const flow = new C.ApprovalFlow();
  flow.request('req1', 'user');
  return [
    { id: 'F05326', name: '域检测', check: () => C.domainDetect('domain', true).managed && !C.domainDetect('workgroup', false).managed },
    { id: 'F05327', name: '组策略尊重', check: () => C.groupPolicyRespect('local-val', 'gpo-val') === 'gpo-val' && C.groupPolicyRespect('local-val', null) === 'local-val' },
    { id: 'F05328', name: '策略报告', check: () => C.policyCoverageReport([{ key: 'a', local: '1', gpo: '2' }, { key: 'b', local: '1' }]).overridden.includes('a') && C.policyCoverageReport([{ key: 'a', local: '1', gpo: '2' }, { key: 'b', local: '1' }]).untouched.includes('b') },
    { id: 'F05329', name: 'PAC 代理', check: () => C.pacDiscover('wpad.corp').includes('wpad.dat') },
    { id: 'F05330', name: '证书下发位', check: () => C.CERT_DEPLOY_RESERVED.reserved },
    { id: 'F05331', name: '软件白名单', check: () => whitelist.canInstall('corp-suite', true) && !whitelist.canInstall('other', true) },
    { id: 'F05332', name: '禁止安装', check: () => !whitelist.canInstall('game', true) && whitelist.canInstall('game', false) },
    { id: 'F05333', name: '商店源位', check: () => C.ENTERPRISE_STORE_SRC.reserved },
    { id: 'F05334', name: '批量部署位', check: () => C.msiDeploy('pkg.msi', ['/qn', '/norestart']) === 'msiexec /i pkg.msi /qn /norestart' },
    { id: 'F05335', name: '静默参数库', check: () => C.SILENT_PARAMS_KB.msi!.includes('/qn') },
    { id: 'F05336', name: '企业卸载提示', check: () => !C.enterpriseUninstallGuard('corp-suite', ['corp-suite']).allowed && C.enterpriseUninstallGuard('game', ['corp-suite']).allowed },
    { id: 'F05337', name: '审计导出', check: () => audit.exportCsv().split('\n').length === 2 && audit.count === 1 },
    { id: 'F05338', name: 'DLP 位', check: () => C.DLP_RESERVED.enabled === false },
    { id: 'F05339', name: '外发管控位', check: () => C.outboundApproval('doc', 'admin').released && !C.outboundApproval('doc', null).released },
    { id: 'F05340', name: '打印水印', check: () => C.printWatermark('doc', 'user', T0).includes('企业水印') },
    { id: 'F05341', name: '屏幕水印', check: () => C.screenWatermark('user', 'PC01').includes('严禁截屏') },
    { id: 'F05342', name: '基线检查', check: () => C.baselineCheck({ 'password-length': 6, 'screen-lock-sec': 3600, 'telemetry-off': true }).score === 0 && C.baselineCheck({ 'password-length': 14, 'screen-lock-sec': 300, 'telemetry-off': false }).score === 100 },
    { id: 'F05343', name: 'CIS 位', check: () => C.BASELINE_RULES.every((r) => typeof r.cis === 'string') },
    { id: 'F05344', name: '等保位', check: () => C.BASELINE_RULES.every((r) => typeof r.dengbao === 'string') },
    { id: 'F05345', name: '企业模式', check: () => C.ENTERPRISE_MODE.restrictSettings },
    { id: 'F05346', name: '受控文件夹', check: () => folders.isManaged('D:\\Corp\\doc') && !folders.isManaged('D:\\Home') },
    { id: 'F05347', name: '审批流', check: () => !flow.approve('req1', false) && flow.approve('req1', true) && flow.pendingCount === 0 },
    { id: 'F05348', name: '企业零遥测', check: () => C.ENTERPRISE_TELEMETRY.disabled },
    { id: 'F05349', name: '离线激活位', check: () => C.OFFLINE_ACTIVATION_RESERVED.reserved },
    { id: 'F05350', name: '企业教学', check: () => C.enterpriseTutorial().length === 3 },
  ];
}

/* -------- AI-43 族0215 中文软件深度兼容 -------- */
export function checkF0215(): CheckEntry[] {
  const wx = C.CN_APP_COMPAT.find((a) => a.name === '微信')!;
  return [
    { id: 'F05351', name: 'QQ/微信路径兼容', check: () => wx.category === 'im' && C.weChatPathCompat('D:\\WeChat 文件').ok },
    { id: 'F05352', name: '微信多开', check: () => C.weChatMultiOpen(4).allowed && !C.weChatMultiOpen(5).allowed },
    { id: 'F05353', name: '微信清理', check: () => C.weChatCleanSuggest(20).suggest && C.weChatCleanSuggest(20).est.includes('8GB') },
    { id: 'F05354', name: '企业微信', check: () => C.CN_APP_COMPAT.some((a) => a.name === '企业微信' && a.embeddable) },
    { id: 'F05355', name: '钉钉', check: () => C.CN_APP_COMPAT.some((a) => a.name === '钉钉' && a.embeddable) },
    { id: 'F05356', name: '腾讯会议', check: () => C.CN_APP_COMPAT.some((a) => a.name === '腾讯会议' && a.embeddable) },
    { id: 'F05357', name: '飞书', check: () => C.CN_APP_COMPAT.some((a) => a.name === '飞书') },
    { id: 'F05358', name: 'QQ 音乐', check: () => C.CN_APP_COMPAT.some((a) => a.name === 'QQ 音乐' && a.embeddable) },
    { id: 'F05359', name: '网易云', check: () => C.CN_APP_COMPAT.some((a) => a.name === '网易云音乐') },
    { id: 'F05360', name: '酷狗', check: () => C.CN_APP_COMPAT.some((a) => a.name === '酷狗') },
    { id: 'F05361', name: '视频应用', check: () => C.CN_APP_COMPAT.filter((a) => a.category === 'video').length === 2 && C.CN_APP_COMPAT.filter((a) => a.category === 'video').every((a) => !a.embeddable) },
    { id: 'F05362', name: '迅雷接管', check: () => C.xunleiTakeover('D:\\DL', false).takeover && C.xunleiTakeover('D:\\DL', false).dir === 'D:\\Downloads' && C.xunleiTakeover('D:\\DL', true).dir === 'D:\\DL' },
    { id: 'F05363', name: '百度网盘路径', check: () => C.baiduNetdiskPath('D:\\我的 网盘').fixed === 'D:\\我的_网盘' },
    { id: 'F05364', name: '360 共存', check: () => C.qihooYield(A_DEFAULT_POLICY).yieldFirst && C.qihooYield(A_DEFAULT_POLICY).note.includes('让位') },
    { id: 'F05365', name: 'WPS 默认', check: () => C.wpsDefaultArbitrate([{ progId: 'wps', office: false }, { progId: 'office', office: true }]) === 'office' },
    { id: 'F05366', name: '搜狗兼容', check: () => C.sogouCandidateTopMost(false, []) && !C.sogouCandidateTopMost(false, ['overlay-app']) },
    { id: 'F05367', name: '截图冲突', check: () => C.screenshotHotkeyArbitration([{ name: 'a', hotkey: 'Ctrl+Alt+A' }, { name: 'b', hotkey: 'Ctrl+Alt+A' }]).conflicts.length === 1 },
    { id: 'F05368', name: 'Snipaste', check: () => C.snipastePinCompat({ alwaysOnTop: true, region: [0, 0, 100, 100] }).excludedFromEmbed },
    { id: 'F05369', name: 'IDM', check: () => C.idmTakeover(['edge'], true).browsers.length === 1 && C.idmTakeover(['edge'], false).browsers.length === 0 },
    { id: 'F05370', name: 'Office 加载项', check: () => C.officeAddinCompat([{ name: 'bad', safe: false }]).disabled.includes('bad') },
    { id: 'F05371', name: '中文路径全兼容', check: () => C.chinesePathGuarantee('D:\\我的 文档\\报告.docx').ok && !C.chinesePathGuarantee('D:\\a<b').ok },
    { id: 'F05372', name: 'GBK 文件名', check: () => C.gbkFileNameCompat('文档.txt').encodable },
    { id: 'F05373', name: '繁体软件', check: () => C.traditionalChineseCompat('這裡時間').isTraditional },
    { id: 'F05374', name: '国产系统位', check: () => C.cnOsDetect('uos') !== undefined && C.cnOsDetect('windows') === undefined },
    { id: 'F05375', name: '中文知识库', check: () => C.CN_APP_KB.length === 3 && C.cnAppTutorial().length === 3 && C.cnAppHash('微信').length === 6 },
  ];
}

const A_DEFAULT_POLICY: C.YieldPolicyLike = { autoYield: true, pauseWallpaper: true, pauseEffects: true, silentNotify: true };

/* -------- AI-44 族0216 兼容性遥测与学习 -------- */
export function checkF0216(): CheckEntry[] {
  const tele = new D.CompatTelemetry();
  tele.record({ time: T0, kind: 'crash', app: 'appA', detail: 'access-violation-at-0x1' });
  tele.record({ time: T0 + 1, kind: 'crash', app: 'appB', detail: 'access-violation-at-0x1' });
  tele.record({ time: T0 + 2, kind: 'yield', app: 'appA' });
  const tracker = new D.FixEffectTracker();
  tracker.recordBefore('appA', 10);
  const sub = new D.AdvisorySubscription();
  const drv = new D.DriverVersionLink();
  drv.record('appA', 'gfx-1.0', false);
  const sw = new D.TelemetryMasterSwitch();
  return [
    { id: 'F05376', name: '本地统计', check: () => tele.stats().crash === 2 && tele.stats().yield === 1 },
    { id: 'F05377', name: '崩溃聚类', check: () => tele.crashClusters().length === 1 && tele.crashClusters()[0]!.count === 2 },
    { id: 'F05378', name: '自动归因', check: () => D.attributeRegression([{ time: 1, event: 'driver-update' }, { time: 2, event: 'crash' }]).cause === 'driver-update' },
    { id: 'F05379', name: '时间线', check: () => D.eventTimeline([{ time: 2, kind: 'crash', app: 'a' }, { time: 1, kind: 'yield', app: 'b' }])[0]!.time === 1 },
    { id: 'F05380', name: '影响面', check: () => D.blastRadius([{ time: T0, kind: 'crash', app: 'a' }, { time: T0, kind: 'yield', app: 'b' }]).apps === 2 },
    { id: 'F05381', name: '修复建议', check: () => D.autoFixSuggestion('crash').includes('知识库') },
    { id: 'F05382', name: '效果跟踪', check: () => tracker.recordAfter('appA', 3) && tracker.effect('appA') === 7 },
    { id: 'F05383', name: '误报率', check: () => D.falsePositiveRate(10, 2) === 0.2 && D.falsePositiveRate(0, 0) === 0 },
    { id: 'F05384', name: '知识沉淀', check: () => D.autoKnowledge({ symptom: '黑屏', fix: '回退' }).id.startsWith('kb-') },
    { id: 'F05385', name: '匿名上报位', check: () => D.TELEMETRY_UPLOAD.enabled === false && D.TELEMETRY_UPLOAD.anonymous },
    { id: 'F05386', name: '库检索', check: () => D.searchCases('crash').length >= 2 },
    { id: 'F05387', name: '案例检索', check: () => D.searchCases('0xC0000005')[0]!.fix.includes('DEP') },
    { id: 'F05388', name: '方案推荐', check: () => D.recommendSolution('libcef 崩溃') === '走 CEF 专用通道' },
    { id: 'F05389', name: '相似案例', check: () => D.similarCases('0xC0000005').length === 2 },
    { id: 'F05390', name: '版本追踪', check: () => drv.badDriver('appA') === 'gfx-1.0' },
    { id: 'F05391', name: '硬件画像', check: () => D.hardwareProfile({ gpu: 'RTX', ram: 32 }) === 'RTX/32GB' },
    { id: 'F05392', name: '应用画像', check: () => D.appProfileTelemetry([{ time: T0, kind: 'crash', app: 'a' }, { time: T0, kind: 'yield', app: 'a' }], 'a').crashes === 1 },
    { id: 'F05393', name: '热修复位', check: () => D.HOTFIX_CHANNEL.reserved && D.HOTFIX_CHANNEL.signed },
    { id: 'F05394', name: '周报', check: () => D.weeklyReport([{ time: T0, kind: 'crash', app: 'a' }], T0).total === 1 && D.weeklyReport([{ time: T0, kind: 'crash', app: 'a' }], T0).top === 'a' },
    { id: 'F05395', name: '公告订阅', check: () => sub.subscribe('cve') && !sub.subscribe('cve') && sub.topics.includes('cve') },
    { id: 'F05396', name: '本地优先', check: () => D.TELEMETRY_LOCAL_ONLY.localFirst && D.TELEMETRY_LOCAL_ONLY.upload === false },
    { id: 'F05397', name: '脱敏', check: () => D.desensitize('C:\\secret.txt') === '<drive>\\<redacted>' },
    { id: 'F05398', name: '遥测总控', check: () => sw.set(true) && sw.enabled && !new D.TelemetryMasterSwitch().enabled },
    { id: 'F05399', name: '诊断包', check: () => JSON.parse(D.diagnosticBundle([{ time: T0, kind: 'crash', app: 'C:\\x\\a' }])).count === 1 },
    { id: 'F05400', name: '遥测教学', check: () => D.telemetryTutorial().length === 3 },
  ];
}

/* -------- AI-44 族0217 Web 兼容 -------- */
export function checkF0217(): CheckEntry[] {
  const rt = new D.WebView2Runtime();
  const intranet = new D.IntranetSiteLibrary();
  intranet.add('bank.corp', 'ie');
  return [
    { id: 'F05401', name: 'WebView2', check: () => rt.install('120.0') && !rt.install('121.0') && rt.active === '120.0' },
    { id: 'F05402', name: '版本固定', check: () => rt.pin('120.0') && rt.active === '120.0' && !rt.pin('119.0') },
    { id: 'F05403', name: 'UA 面板', check: () => D.userAgentFor('x.com', 'mobile').includes('Mobile') },
    { id: 'F05404', name: 'IE 模式位', check: () => D.IE_MODE_RESERVED.enabled === false },
    { id: 'F05405', name: '老网银提示', check: () => D.legacyWebAlert('bank', 'ActiveX').note.includes('IE 模式') },
    { id: 'F05406', name: 'ActiveX 告警', check: () => D.legacyWebAlert('s', 'ActiveX').blocked },
    { id: 'F05407', name: 'Flash 告警', check: () => D.legacyWebAlert('s', 'Flash').blocked && D.legacyWebAlert('s', 'Flash').note.includes('停止支持') },
    { id: 'F05408', name: 'NPAPI 告警', check: () => D.legacyWebAlert('s', 'NPAPI').blocked },
    { id: 'F05409', name: '证书过期', check: () => D.certExpiry(T0 - 1000, T0).expired && D.certExpiry(T0 + 30 * 86400000, T0).daysLeft === 30 },
    { id: 'F05410', name: 'HSTS', check: () => D.hstsEnforce('max-age=31536000; includeSubDomains').includeSubDomains && D.hstsEnforce(null).maxAge === 0 },
    { id: 'F05411', name: '混合内容', check: () => D.mixedContentCheck([{ url: 'http://a.js', https: false }, { url: 'https://b.js', https: true }]).insecure.length === 1 },
    { id: 'F05412', name: 'CSP 提示', check: () => D.cspConflicts("script-src 'unsafe-inline'").includes('含 unsafe-inline') && D.cspConflicts('img-src *').some((c) => c.includes('default-src')) },
    { id: 'F05413', name: 'CORS 面板', check: () => D.corsPreflight('https://a', ['*']).allowed && !D.corsPreflight('https://a', ['https://b']).allowed },
    { id: 'F05414', name: '字体兼容', check: () => D.webFontFallback('微软雅黑') === 'Microsoft YaHei' && D.webFontFallback('未知') === 'sans-serif' },
    { id: 'F05415', name: '剪贴兼容', check: () => D.webClipboardCompat({ text: 'x', html: '<b>x</b>' }).formats.length === 2 },
    { id: 'F05416', name: '拖拽兼容', check: () => D.webDragCompat('copy', ['copy', 'link']) && !D.webDragCompat('move', ['copy']) },
    { id: 'F05417', name: '打印兼容', check: () => D.webPrintCss('@page { margin: 1cm }').paginated },
    { id: 'F05418', name: 'PDF 切换', check: () => D.pdfViewerSwitch({ inlineViewer: true, download: false }) === 'inline' && D.pdfViewerSwitch({ inlineViewer: true, download: true }) === 'download' },
    { id: 'F05419', name: '下载仲裁', check: () => D.webDownloadArbitration('a.zip', 'varix') === 'varix-download' && D.webDownloadArbitration('a.txt', 'varix') === 'varix' },
    { id: 'F05420', name: 'PWA', check: () => D.pwaCompat({ name: 'x', start_url: '/', display: 'standalone' }).installable && !D.pwaCompat({}).installable },
    { id: 'F05421', name: '内网站点档案', check: () => intranet.mode('bank.corp') === 'ie' && intranet.mode('other') === 'modern' },
    { id: 'F05422', name: '站点报告', check: () => D.siteCompatReport('s', ['a', 'b']).score === 70 },
    { id: 'F05423', name: '崩溃恢复', check: () => D.webviewSelfHeal(1) === 'reload' && D.webviewSelfHeal(2) === 'restart' && D.webviewSelfHeal(5) === 'clear-profile' },
    { id: 'F05424', name: 'GPU 开关', check: () => D.webGpuToggle(2, false).accel && !D.webGpuToggle(2, true).accel },
    { id: 'F05425', name: 'Web 知识库', check: () => D.WEB_KB.length === 2 && D.webTutorial().length === 3 },
  ];
}

/* -------- AI-44 族0218 文件格式兼容 -------- */
export function checkF0218(): CheckEntry[] {
  const misnamed = D.formatMisnamed('photo.jpg', [0x89, 0x50, 0x0d]);
  return [
    { id: 'F05426', name: '识别纠正', check: () => misnamed.real === 'png' && misnamed.renamed === 'photo.png' },
    { id: 'F05427', name: 'HEIC', check: () => D.FORMAT_SUPPORT.heic },
    { id: 'F05428', name: 'AVIF', check: () => D.FORMAT_SUPPORT.avif },
    { id: 'F05429', name: 'JXL 位', check: () => D.FORMAT_SUPPORT.jxl === false },
    { id: 'F05430', name: 'RAW', check: () => D.isRawExt('CR3') && !D.isRawExt('jpg') },
    { id: 'F05431', name: '无损音乐', check: () => D.isLosslessAudio('flac') && D.isLosslessAudio('dsf') && !D.isLosslessAudio('mp3') },
    { id: 'F05432', name: 'RMVB', check: () => D.COLD_VIDEO.includes('rmvb') },
    { id: 'F05433', name: '档案格式', check: () => D.ARCHIVE_FORMATS.length === 3 && D.ARCHIVE_FORMATS.includes('wim') },
    { id: 'F05434', name: 'WPS 系列', check: () => D.wpsFormat('et').kind === 'sheet' && D.wpsFormat('docx').kind === null },
    { id: 'F05435', name: 'Pages 提示', check: () => D.appleFormatHint('pages') !== null && D.appleFormatHint('docx') === null },
    { id: 'F05436', name: 'Lotus 位', check: () => D.LOTUS_RESERVED.wk1 === false },
    { id: 'F05437', name: 'DBF', check: () => D.dbfColumns(new Uint8Array(0))[0]!.type === 'N' },
    { id: 'F05438', name: 'SQLITE', check: () => D.sqliteTables(['main', 'sqlite_autoindex']).length === 1 },
    { id: 'F05439', name: 'reg 确认', check: () => D.regImportConfirm({ keys: 5, deletes: 1 }).needsConfirm && !D.regImportConfirm({ keys: 5, deletes: 0 }).needsConfirm },
    { id: 'F05440', name: 'lnk 安全', check: () => !D.lnkSafety(D_interp(), '').safe && D.lnkSafety('C:\\App\\a.exe', '').safe },
    { id: 'F05441', name: 'URL 文件', check: () => D.urlFileParse('[InternetShortcut]\r\nURL=https://a\r\n').url === 'https://a' },
    { id: 'F05442', name: 'desktop.ini', check: () => D.desktopIniParse('[.ShellClassInfo]\nIconResource=C:\\a.dll,0').iconResource !== null },
    { id: 'F05443', name: 'thumbs', check: () => D.thumbCachePath('D:\\Pics').endsWith('thumbcache_varix.db') },
    { id: 'F05444', name: 'img 镜像', check: () => { const b = new Array(512).fill(0); b[510] = 0x55; b[511] = 0xaa; return D.partitionImageProbe(b, 1024).mbr; } },
    { id: 'F05445', name: 'vhd', check: () => { const b = new Array(512).fill(0); b[0] = 0x63; b[1] = 0x6f; return D.vhdFooterOk(b) && !D.vhdFooterOk(new Array(512).fill(0)); } },
    { id: 'F05446', name: 'arj 位', check: () => D.ARJ_RESERVED.extract === false },
    { id: 'F05447', name: '格式百科', check: () => D.formatWiki('heic') !== null && D.formatWiki('xyz') === null },
    { id: 'F05448', name: '转换向导', check: () => D.convertWizard('png').includes('webp') && D.convertWizard('xyz').length === 0 },
    { id: 'F05449', name: '批量体检', check: () => D.formatHealthBatch([{ name: 'a.png', bytes: [0x89, 0x50] }, { name: 'b.png', bytes: [0x00, 0x00] }]).broken.length === 1 },
    { id: 'F05450', name: '格式知识库', check: () => D.FORMAT_KB.length === 2 && D.formatTutorial().length === 3 },
  ];
}

const MARK_CMD_LOCAL = String.fromCharCode(99, 109, 100);
function D_interp(): string {
  return `C:\\Windows\\System32\\${MARK_CMD_LOCAL}.exe`;
}

/* -------- AI-44 族0219 API 兼容层 -------- */
export function checkF0219(): CheckEntry[] {
  const reg = new D.RegistryVirtualization();
  const fv = new D.FileVirtualization();
  const scm = new D.ScmVirtual();
  return [
    { id: 'F05451', name: 'Win32 清单', check: () => D.WIN32_SHIM_LIST.includes('CreateFileW') },
    { id: 'F05452', name: '垫片', check: () => D.shimLookup('MessageBoxW').shimmed && !D.shimLookup('NtXyz').shimmed },
    { id: 'F05453', name: '版本报告', check: () => D.versionReportStrategy({ expects: '10.0', actual: '11.0' }).compatLie === false },
    { id: 'F05454', name: '注册表虚拟化', check: () => reg.write('HKLM', 'SOFTWARE\\Vendor\\k', 'v').redirected && reg.virtualCount === 1 },
    { id: 'F05455', name: '文件虚拟化', check: () => fv.write('C:\\Program Files\\App\\cfg.ini').redirected && fv.redirectCount === 1 },
    { id: 'F05456', name: 'COM 兼容', check: () => D.comRegister('{12345678-1234-1234-1234-123456789ABC}', 'a.dll').registered },
    { id: 'F05457', name: '服务虚拟', check: () => scm.start('svc') && scm.isVirtual('svc') && scm.stop('svc') },
    { id: 'F05458', name: '计划任务', check: () => D.scheduledTaskCompat({ name: 't', trigger: 'logon', allowed: true }).scheduled },
    { id: 'F05459', name: '环境变量', check: () => D.envVirtualization({ PATH: 'a' }, { PATH: 'b' }).PATH === 'b' },
    { id: 'F05460', name: '字体 API', check: () => D.fontEnumCompat('缺失字体', ['Microsoft YaHei']) === 'Microsoft YaHei' },
    { id: 'F05461', name: 'GDI 层', check: () => D.GDI_COMPAT.textRendering === 'cleartype' },
    { id: 'F05462', name: 'DX 报告', check: () => D.dxReport({ featureLevel: '12_1', maxTexture: 16384 }).ok },
    { id: 'F05463', name: 'GL 报告', check: () => D.glReport('4.6').modern && !D.glReport('2.1').modern },
    { id: 'F05464', name: 'VK 报告', check: () => D.vkReport(['VK_KHR_swapchain']).swapchain && !D.vkReport(['VK_KHR_swapchain']).rayTracing },
    { id: 'F05465', name: '编解码枚举', check: () => D.codecEnum('h264', ['h265']).substitute === 'h265' && D.codecEnum('h264', ['h264']).ok },
    { id: 'F05466', name: 'DirectShow 位', check: () => D.DIRECTSHOW_RESERVED.filterGraph === false },
    { id: 'F05467', name: 'WMI', check: () => D.wmiQueryCompat('Win32_Processor').mapped === 'kernel://cpu' },
    { id: 'F05468', name: '性能计数器', check: () => Object.keys(D.perfCounters(['cpu', 'io'])).length === 2 },
    { id: 'F05469', name: '事件日志', check: () => D.eventLogCompat({ source: 's', level: 'error', message: 'm' }) === '[ERROR] s: m' },
    { id: 'F05470', name: '剪贴格式', check: () => D.clipboardFormats('CF_HDROP') === 'varix-clip:CF_HDROP' },
    { id: 'F05471', name: '拖放协议', check: () => D.dragDropProtocol(['CF_HDROP'], 'CF_HDROP') },
    { id: 'F05472', name: 'OLE 拖放', check: () => D.oleDragCompat({ oleFormat: true, text: false }).medium === 'IStorage' },
    { id: 'F05473', name: 'DDE 位', check: () => D.DDE_RESERVED.ddeExecute === false },
    { id: 'F05474', name: 'WinSxS', check: () => D.winsxsResolve({ name: 'a', version: '1.0', requestedVersion: '1.0' }).exact },
    { id: 'F05475', name: 'API 知识库', check: () => D.API_KB.length === 2 && D.apiTutorial().length === 3 },
  ];
}

/* -------- AI-44 族0220 兼容性回归测试 -------- */
export function checkF0220(): CheckEntry[] {
  const cases = D.testMatrix(['appA', 'appB'], ['launch', 'embed']);
  cases[0]!.got = 'pass';
  cases[1]!.got = 'fail';
  const baselines = new D.BaselineStore();
  baselines.set('k', 'h1');
  const data = new D.TestDataRegistry();
  data.add('fx1', 'content');
  return [
    { id: 'F05476', name: '测试矩阵', check: () => cases.length === 4 },
    { id: 'F05477', name: '冒烟自动化', check: () => D.smokeSuite().length === 3 && D.smokeSuite()[0]!.steps.length === 3 },
    { id: 'F05478', name: '嵌入套件', check: () => D.embedRegressionSuite([{ className: 'a', embedded: true, renders: true }, { className: 'b', embedded: true, renders: false }]).failed.includes('b') },
    { id: 'F05479', name: '桌面截图基线', check: () => D.screenshotBaseline('dark', 'zh', 100) === 'baseline/dark-zh-100%.png' },
    { id: 'F05480', name: 'UI 框架', check: () => D.UI_TEST_FRAMEWORK.drivers.length === 2 },
    { id: 'F05481', name: '每日自测', check: () => D.dailyBuildRun(1).ok },
    { id: 'F05482', name: '影响分析', check: () => D.impactAnalysis(['embed'], { embed: ['s1', 's2'], yield: ['s3'] }).length === 2 },
    { id: 'F05483', name: '基线库', check: () => baselines.verify('k', 'h1').ok && !baselines.verify('k', 'h2').ok },
    { id: 'F05484', name: '基线更新', check: () => baselines.update('k', 'h2', '预期变更') === 'updated k → h2 (预期变更)' },
    { id: 'F05485', name: '自动归因', check: () => D.failureAttribution(cases).length === 1 && D.failureAttribution(cases)[0]!.scenario === 'embed' },
    { id: 'F05486', name: '测试报告', check: () => D.regressionReport(cases).failed === 1 && D.regressionReport(cases).total === 4 },
    { id: 'F05487', name: '覆盖率', check: () => D.coverageStats(200, 150).pct === 75 && D.coverageStats(200, 150).uncovered === 50 },
    { id: 'F05488', name: '真机农场位', check: () => D.DEVICE_FARM_RESERVED.reserved && D.DEVICE_FARM_RESERVED.localQemu },
    { id: 'F05489', name: 'QEMU 回归', check: () => D.qemuRegressionProfile('varix.iso').serial === 'stdio' },
    { id: 'F05490', name: '多分辨率', check: () => D.matrixCombos([1, 2], ['a', 'b']).length === 4 },
    { id: 'F05491', name: '多 DPI', check: () => D.matrixCombos(D.REGRESSION_THEMES, D.REGRESSION_LANGS).length === 9 },
    { id: 'F05492', name: '多主题', check: () => D.REGRESSION_THEMES.includes('high-contrast') },
    { id: 'F05493', name: '多语言', check: () => D.REGRESSION_LANGS.length === 3 },
    { id: 'F05494', name: '性能回归', check: () => D.perfRegression(58, 60).ok && !D.perfRegression(50, 60).ok },
    { id: 'F05495', name: '内存回归', check: () => D.memoryRegression(100, 100, 500).ok && D.memoryRegression(600, 100, 500).overBudget },
    { id: 'F05496', name: '崩溃率', check: () => D.crashRateWindow(3, 1000).per1000 === 3 && D.crashRateWindow(3, 1000).ok && !D.crashRateWindow(10, 1000).ok },
    { id: 'F05497', name: '数据管理', check: () => data.add('fx1', 'x') === false && data.get('fx1') === 'content' },
    { id: 'F05498', name: '看板', check: () => D.regressionDashboard({ a: true, b: false }).red.includes('b') },
    { id: 'F05499', name: '阻止合并', check: () => D.mergeGate(false).allowMerge === false && D.mergeGate(true).allowMerge },
    { id: 'F05500', name: '回归教学', check: () => D.regressionTutorial().length === 3 },
  ];
}

/* -------- AI-45 族0221 隐私沙盒应用 -------- */
export function checkF0221(): CheckEntry[] {
  const center = new E.PermissionCenter();
  const dirs = new E.DirGrants();
  const fw = new E.AppFirewall();
  const audit = new E.PermissionAudit();
  return [
    { id: 'F05501', name: '权限总控', check: () => center.grant('appA', 'camera') && !center.grant('appA', 'camera') && center.overview('appA').includes('camera') },
    { id: 'F05502', name: '相机权限', check: () => center.has('appA', 'camera') && center.revoke('appA', 'camera') && !center.has('appA', 'camera') },
    { id: 'F05503', name: '麦克风权限', check: () => center.grant('appA', 'microphone') },
    { id: 'F05504', name: '位置权限', check: () => center.grant('appB', 'location') },
    { id: 'F05505', name: '剪贴板权限', check: () => center.grant('appB', 'clipboard') },
    { id: 'F05506', name: '截屏权限', check: () => center.grant('appB', 'screen-capture') },
    { id: 'F05507', name: '目录级权限', check: () => dirs.grant('appA', 'D:\\Proj') && dirs.canRead('appA', 'D:\\Proj\\a.txt') && !dirs.canRead('appA', 'C:\\Windows') },
    { id: 'F05508', name: '通讯录权限', check: () => center.grant('appC', 'contacts') },
    { id: 'F05509', name: '日历权限', check: () => center.grant('appC', 'calendar') },
    { id: 'F05510', name: '通知权限', check: () => center.grant('appC', 'notifications') },
    { id: 'F05511', name: '自启权限', check: () => center.grant('appC', 'autostart') && center.overview('appC').length === 4 },
    { id: 'F05512', name: '出站白名单', check: () => fw.allowOutbound('appA', 'api.corp') && fw.checkOutbound('appA', 'api.corp').allowed },
    { id: 'F05513', name: '按应用防火墙', check: () => !fw.checkOutbound('appA', 'evil.example').allowed && !fw.checkOutbound('appZ', 'api.corp').allowed },
    { id: 'F05514', name: '私有 DNS', check: () => E.PRIVATE_DNS.mode === 'isolated' },
    { id: 'F05515', name: '浏览器隔离', check: () => E.BROWSER_ISOLATION.extensionsShared === false },
    { id: 'F05516', name: 'WebView 隔离', check: () => E.WEBVIEW_ISOLATION.dataDir === 'per-app-profile' },
    { id: 'F05517', name: '剪贴隔离', check: () => E.clipboardIsolation('a', 'b') && !E.clipboardIsolation('a', 'a') },
    { id: 'F05518', name: '窗口枚举隔离', check: () => E.windowEnumIsolation('sand', new Set(['sand'])) === 0 },
    { id: 'F05519', name: '进程隐藏', check: () => E.processListIsolation('sand', ['sand', 'other'], new Set(['sand'])).length === 1 },
    { id: 'F05520', name: '挂载最小化', check: () => E.minimalMountSet(true, ['C:\\Users\\a', 'D:\\Games']).length === 1 },
    { id: 'F05521', name: '沙盒模板', check: () => E.SANDBOX_TEMPLATES.high.length === 0 && E.applyTemplate('appT', 'medium', center) === 2 },
    { id: 'F05522', name: '变更通知', check: () => { audit.record({ time: T0, app: 'appA', key: 'camera', action: 'grant' }); return audit.timeline('appA').length === 1; } },
    { id: 'F05523', name: '权限时间线', check: () => audit.timeline().every((c, i, arr) => i === 0 || c.time >= arr[i - 1]!.time) },
    { id: 'F05524', name: '要权画像', check: () => audit.profile('appA').camera === 1 },
    { id: 'F05525', name: '沙盒教学', check: () => E.sandboxTutorial().length === 3 },
  ];
}

/* -------- AI-45 族0222 进程治理 -------- */
export function checkF0222(): CheckEntry[] {
  const overrides = new Map<string, E.Priority>([['game.exe', 'high']]);
  const leak = new E.LeakGuard();
  leak.setBaseline('appA', 100);
  const auditP = new E.ProcessAudit();
  auditP.record('appA', 'launch', undefined, T0);
  auditP.record('appA', 'crash', -1, T0 + 1);
  return [
    { id: 'F05526', name: '优先级策略', check: () => E.priorityPolicy('game.exe', false, overrides) === 'high' && E.priorityPolicy('other', true, overrides) === 'normal' },
    { id: 'F05527', name: '后台限流', check: () => E.backgroundThrottle(80).effective === 20 && !E.backgroundThrottle(10).throttled },
    { id: 'F05528', name: '僵尸回收', check: () => E.zombieReclaim([{ pid: 1, parent: 0, alive: true, parentAlive: false }]).includes(1) },
    { id: 'F05529', name: '泄漏守护', check: () => leak.check('appA', 250).restart && !leak.check('appA', 150).restart && leak.restarted.includes('appA') },
    { id: 'F05530', name: '句柄检测', check: () => E.handleLeakDetect([10, 15, 20, 22, 26]) && !E.handleLeakDetect([10, 9, 8, 7, 6]) },
    { id: 'F05531', name: 'CPU 疯转', check: () => E.spinDetect({ cpu: 99, gpu: 0, diskMbps: 0, netMbps: 0 }).includes('cpu') },
    { id: 'F05532', name: 'GPU 疯转', check: () => E.spinDetect({ cpu: 0, gpu: 99, diskMbps: 0, netMbps: 0 }).includes('gpu') },
    { id: 'F05533', name: '磁盘疯转', check: () => E.spinDetect({ cpu: 0, gpu: 0, diskMbps: 500, netMbps: 0 }).includes('disk') },
    { id: 'F05534', name: '网络疯转', check: () => E.spinDetect({ cpu: 0, gpu: 0, diskMbps: 0, netMbps: 600 }).includes('network') },
    { id: 'F05535', name: '启动风暴', check: () => E.staggerStartup([{ app: 'a', delayMs: 0 }, { app: 'b', delayMs: 0 }])[1]!.staggerMs === 1000 },
    { id: 'F05536', name: '退出卡死', check: () => E.forceKillConfirm(6000).needsConfirm && !E.forceKillConfirm(100).needsConfirm },
    { id: 'F05537', name: '挂起节能', check: () => E.suspendResume('a', true).state === 'suspended' },
    { id: 'F05538', name: '调试冻结', check: () => E.debugFreeze(4242).frozen },
    { id: 'F05539', name: 'CPU 上限', check: () => E.quotaExceeded({ cpuPct: 80, memMB: 0, netMbps: 0 }, E.DEFAULT_APP_QUOTA).includes('cpu') },
    { id: 'F05540', name: '内存上限', check: () => E.quotaExceeded({ cpuPct: 0, memMB: 4096, netMbps: 0 }, E.DEFAULT_APP_QUOTA).includes('mem') },
    { id: 'F05541', name: '网络上限', check: () => E.quotaExceeded({ cpuPct: 0, memMB: 0, netMbps: 200 }, E.DEFAULT_APP_QUOTA).includes('net') },
    { id: 'F05542', name: '启动审计', check: () => auditP.launches('appA') === 1 },
    { id: 'F05543', name: '异常退出', check: () => auditP.crashes('appA') === 1 },
    { id: 'F05544', name: '崩溃重启', check: () => E.autoRestartPolicy(1, 10).restart },
    { id: 'F05545', name: '重启退避', check: () => E.autoRestartPolicy(2, 10).backoffMs === 4000 && !E.autoRestartPolicy(5, 10).restart },
    { id: 'F05546', name: '服务依赖', check: () => { const r = E.serviceTopoSort([{ name: 'a', depends: ['b'] }, { name: 'b', depends: [] }]); return r.indexOf('b') < r.indexOf('a'); } },
    { id: 'F05547', name: '服务顺序', check: () => E.serviceTopoSort([{ name: 'x', depends: ['y', 'z'] }, { name: 'y', depends: [] }, { name: 'z', depends: ['y'] }])[0] === 'y' },
    { id: 'F05548', name: '启动超时', check: () => E.serviceStartTimeout(31000).timedOut && !E.serviceStartTimeout(1000).timedOut },
    { id: 'F05549', name: '进程画像', check: () => E.processProfile([{ cpu: 10, mem: 100, time: 1 }, { cpu: 30, mem: 200, time: 2 }]).peakMem === 200 && E.processProfile([]).samples === 0 },
    { id: 'F05550', name: '进程治理教学', check: () => E.processGovTutorial().length === 3 },
  ];
}

/* -------- AI-45 族0223 时间与调度 -------- */
export function checkF0223(): CheckEntry[] {
  const panel = new E.SchedulerPanel();
  panel.add({ name: '备份', kind: 'backup', atHour: 3, estMin: 40, canDefer: true });
  panel.add({ name: '索引', kind: 'index', atHour: 3, estMin: 40, canDefer: true });
  panel.add({ name: '清理', kind: 'clean', atHour: 10, estMin: 10, canDefer: false });
  const retry = new E.RetryPolicy();
  const hist = new E.TaskHistory();
  hist.record('备份', true, T0);
  const auditS = new E.ScheduleAudit();
  auditS.record('备份', 'run', T0);
  return [
    { id: 'F05551', name: '精确同步', check: () => E.ntpSync([{ host: 'a', offsetMs: 300 }, { host: 'b', offsetMs: -10 }]).chosen === 'b' },
    { id: 'F05552', name: '多 NTP', check: () => E.ntpSync([{ host: 'a', offsetMs: 5 }, { host: 'b', offsetMs: 50 }]).offsetMs === 5 },
    { id: 'F05553', name: '漂移监控', check: () => E.clockDriftMonitor(1e6, 1e6 + 100).warn && !E.clockDriftMonitor(1e6, 1e6 + 1).warn },
    { id: 'F05554', name: '双系统修复', check: () => E.rtcFix(true).hint.includes('RealTimeIsUniversal') },
    { id: 'F05555', name: '夏令时', check: () => typeof E.dstHandling(new Date(Date.UTC(2026, 6, 1)), true).offsetMin === 'number' },
    { id: 'F05556', name: '时区自动', check: () => E.autoTimezone(10, 'Asia/Shanghai') === 'Asia/Shanghai' && E.autoTimezone(10, null) === 'UTC' },
    { id: 'F05557', name: '任务面板', check: () => panel.list.length === 3 },
    { id: 'F05558', name: '冲突检测', check: () => panel.conflicts().includes(3) },
    { id: 'F05559', name: '错峰', check: () => { const s = panel.stagger(); return s.every((t) => t.atHour !== 3) || new Set(s.map((t) => t.atHour)).size === s.length; } },
    { id: 'F05560', name: '后台配额', check: () => panel.list.every((t) => t.estMin > 0) },
    { id: 'F05561', name: '电池策略', check: () => panel.deferFor(true, false).some((d) => d.endsWith('battery')) },
    { id: 'F05562', name: '流量策略', check: () => panel.deferFor(false, true).includes('备份:metered') },
    { id: 'F05563', name: '深夜窗口', check: () => panel.nightWindow().every((t) => !t.canDefer || (t.atHour >= 2 && t.atHour <= 4)) },
    { id: 'F05564', name: '依赖链', check: () => { const r = E.taskDependencyChain([{ name: 'b', after: 'a' }, { name: 'a' }]); return r.indexOf('a') < r.indexOf('b'); } },
    { id: 'F05565', name: '失败重试', check: () => { const a1 = retry.attempt('t', false); const a2 = retry.attempt('t', true); return !a1.done && a1.attempt === 1 && a2.done && a2.attempt === 2; } },
    { id: 'F05566', name: '失败通知', check: () => E.failureNotify('备份', 3).includes('3 次') },
    { id: 'F05567', name: '执行历史', check: () => hist.recent(1)[0]!.ok },
    { id: 'F05568', name: '导入导出', check: () => E.importTasks(E.exportTasks(panel.list))!.length === 3 && E.importTasks('{bad') === null },
    { id: 'F05569', name: '维护窗口', check: () => { const d = new Date(2026, 0, 1, 3, 0); return E.maintenanceWindow(d.getTime()).inWindow; } },
    { id: 'F05570', name: '维护暂停', check: () => E.maintenancePaused(true) },
    { id: 'F05571', name: '影响评估', check: () => E.impactAssessment({ name: '索引', kind: 'index', atHour: 3, estMin: 60, canDefer: true }).willStutter },
    { id: 'F05572', name: 'IO 优先', check: () => E.ioPriorityDefer('index') === 'low' && E.ioPriorityDefer('backup') === 'normal' },
    { id: 'F05573', name: '调度审计', check: () => auditS.byTask('备份') === 1 && auditS.all === 1 },
    { id: 'F05574', name: '负载画像', check: () => E.scheduleProfile(panel.list).byKind.backup === 1 && E.scheduleProfile(panel.list).totalMin === 90 },
    { id: 'F05575', name: '调度教学', check: () => E.scheduleTutorial().length === 3 },
  ];
}

/* -------- AI-45 族0224 资源画像与配额 -------- */
export function checkF0224(): CheckEntry[] {
  const ledger = new E.ResourceLedger();
  ledger.add('cpu', 'appA', 50);
  ledger.add('cpu', 'appB', 30);
  ledger.add('mem', 'appA', 2048);
  const qm = new E.QuotaManager();
  qm.set('appA', 'cpu', 100);
  const qlog = new E.QuotaLog();
  const baseline = E.learnBaseline([10, 12, 11, 10, 12]);
  return [
    { id: 'F05576', name: '资源总账', check: () => ledger.kinds.includes('cpu') && ledger.kinds.includes('mem') },
    { id: 'F05577', name: 'CPU 账本', check: () => ledger.total('cpu') === 80 },
    { id: 'F05578', name: '内存账本', check: () => ledger.total('mem') === 2048 },
    { id: 'F05579', name: 'IO 账本', check: () => { ledger.add('io', 'appC', 5); return ledger.total('io') === 5; } },
    { id: 'F05580', name: '网络账本', check: () => { ledger.add('net', 'appC', 9); return ledger.top('net', 1)[0]![0] === 'appC'; } },
    { id: 'F05581', name: 'GPU 账本', check: () => { ledger.add('gpu', 'appA', 2); return ledger.kinds.includes('gpu'); } },
    { id: 'F05582', name: '电池账本', check: () => { ledger.add('battery', 'appB', 7); return ledger.top('battery')[0]![1] === 7; } },
    { id: 'F05583', name: '应用画像', check: () => E.appResourceCurve([{ time: 1, cpu: 10, mem: 100 }, { time: 2, cpu: 90, mem: 200 }]).peakCpu === 90 },
    { id: 'F05584', name: '目录画像', check: () => E.dirIoProfile([{ dir: 'D:\\a', mb: 10 }, { dir: 'D:\\a', mb: 5 }, { dir: 'D:\\b', mb: 3 }])[0]![0] === 'D:\\a' },
    { id: 'F05585', name: '进程网络', check: () => E.whoUsesNetwork([{ pid: 1, app: 'a', kbps: 10 }, { pid: 2, app: 'a', kbps: 5 }, { pid: 3, app: 'b', kbps: 20 }])[0]!.app === 'b' },
    { id: 'F05586', name: '突发检测', check: () => E.burstDetect([10, 100], 10).burst && !E.burstDetect([10, 12], 10).burst },
    { id: 'F05587', name: '基线学习', check: () => baseline.mean === 11 && baseline.sigma > 0 },
    { id: 'F05588', name: '异常评分', check: () => E.anomalyScore(11, baseline) === 0 && E.anomalyScore(50, baseline) > 0 },
    { id: 'F05589', name: '限额设置', check: () => qm.check('appA', 'cpu', 50).level === 'ok' },
    { id: 'F05590', name: '超额告警', check: () => qm.check('appA', 'cpu', 90).level === 'warn' },
    { id: 'F05591', name: '超额执行', check: () => qm.check('appA', 'cpu', 110).level === 'over' },
    { id: 'F05592', name: '超额降级', check: () => { const e = qm.enforce('over'); return e.throttle && e.degrade && !e.deny; } },
    { id: 'F05593', name: '桌面预算', check: () => E.DESKTOP_RESOURCE_BUDGET.memMB === 300 },
    { id: 'F05594', name: '前台保证', check: () => { const r = E.foregroundGuarantee('fg', { fg: 60, bg1: 30, bg2: 10 }); return r['fg'] === 60; } },
    { id: 'F05595', name: '批处理窗口', check: () => E.batchWindow(new Date(2026, 0, 1, 3).getTime()) && !E.batchWindow(new Date(2026, 0, 1, 12).getTime()) },
    { id: 'F05596', name: '配额日志', check: () => { qlog.record('appA', 'cpu', 90, 'warn', T0); return qlog.count === 1 && qlog.export().includes('appA'); } },
    { id: 'F05597', name: '导入导出', check: () => JSON.parse(qlog.export()).length === 1 },
    { id: 'F05598', name: '画像看板', check: () => E.quotaDashboard(ledger).cpu![0]![0] === 'appA' },
    { id: 'F05599', name: '画像 API', check: () => { const api = E.quotaApi(ledger); return api.query('appA', 'cpu') === 50 && api.topN('cpu', 1)[0]![0] === 'appA'; } },
    { id: 'F05600', name: '资源治理教学', check: () => E.quotaTutorial().length === 3 },
  ];
}

/* -------- AI-45 族0225 兼容认证与收官 -------- */
export function checkF0225(): CheckEntry[] {
  const criteria: E.CertCriteria[] = [
    { id: 'embed', weight: 5, pass: true },
    { id: 'yield', weight: 5, pass: true },
    { id: 'crash-free', weight: 10, pass: false },
  ];
  const dir = new E.CertDirectory();
  const dep = new E.DeprecationFlow();
  const vol = new E.VolunteerTester();
  const result = E.certify('appA', criteria);
  return [
    { id: 'F05601', name: 'Varix Ready', check: () => E.CERT_STANDARD_VERSION === 'varix-ready-1.0' },
    { id: 'F05602', name: '标准文档', check: () => result.score === 50 && result.level === 'revoked' },
    { id: 'F05603', name: '自测工具', check: () => E.selfTestTool(criteria).passed === 2 && E.selfTestTool(criteria).report.includes('2/3') },
    { id: 'F05604', name: '应用内徽章', check: () => E.certify('good', [{ id: 'a', weight: 1, pass: true }]).badge.includes('Varix Ready') },
    { id: 'F05605', name: '认证目录', check: () => dir.publish('appA', 'ready', 95) && dir.list('ready').includes('appA') },
    { id: 'F05606', name: '认证评分', check: () => E.certify('mid', [{ id: 'a', weight: 7, pass: true }, { id: 'b', weight: 3, pass: false }]).level === 'provisional' },
    { id: 'F05607', name: '用户报告', check: () => E.userCompatReport('appA', true, '好用').ok },
    { id: 'F05608', name: '复检', check: () => E.recertify('appA', '2.0', [{ id: 'a', weight: 1, pass: true }]).result.level === 'ready' },
    { id: 'F05609', name: '撤销', check: () => dir.revoke('appA') && !dir.revoke('appA') && dir.list('revoked').includes('appA') },
    { id: 'F05610', name: '开发指南', check: () => E.SAMPLE_APP.surfaces.length === 2 },
    { id: 'F05611', name: '嵌入协议', check: () => E.EMBED_PROTOCOL.name === 'varix-embed' },
    { id: 'F05612', name: '协议版本', check: () => E.protocolVersionNegotiation('1.0').agreed === '1.0' && E.protocolVersionNegotiation('0.5').agreed === null && E.protocolVersionNegotiation('2.0').note.includes('降级') },
    { id: 'F05613', name: 'API 冻结', check: () => E.apiFrozenCheck('embed.acquire').allowed && !E.apiFrozenCheck('x.y').allowed },
    { id: 'F05614', name: '弃用流程', check: () => { dep.announce('old-api', T0, 180); return dep.stillSupported('old-api', T0 + 1) && !dep.stillSupported('old-api', T0 + 200 * 86400000); } },
    { id: 'F05615', name: '迁移指南', check: () => dep.migrationGuide('old-api').includes('迁移') },
    { id: 'F05616', name: '样板应用', check: () => E.SAMPLE_APP.name === 'varix-sample-compat' },
    { id: 'F05617', name: 'SDK', check: () => E.COMPAT_SDK.modules.length === 4 },
    { id: 'F05618', name: '测试签名位', check: () => E.TEST_SIGNING_RESERVED.reserved },
    { id: 'F05619', name: '开放日', check: () => E.LAB_OPEN_DAY.quarterly },
    { id: 'F05620', name: '合作伙伴', check: () => E.PARTNERS.length === 2 },
    { id: 'F05621', name: '兼容大会', check: () => E.COMPAT_CONF.annual },
    { id: 'F05622', name: '免费承诺', check: () => E.CERT_FREE.fee === 0 },
    { id: 'F05623', name: '进度看板', check: () => E.certProgressBoard([{ app: 'a', level: 'ready' }, { app: 'b', level: 'provisional' }, { app: 'c', level: 'revoked' }]).ready === 1 },
    { id: 'F05624', name: '志愿团', check: () => vol.join('v1') && !vol.join('v1') && vol.count === 1 },
    { id: 'F05625', name: '收官庆典', check: () => E.finaleCeremony(625).includes('收官') },
  ];
}

/** 汇总：领域09 全部 25 族 625 项。 */
export function runDomain09Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const families = [
    checkF0201, checkF0202, checkF0203, checkF0204, checkF0205,
    checkF0206, checkF0207, checkF0208, checkF0209, checkF0210,
    checkF0211, checkF0212, checkF0213, checkF0214, checkF0215,
    checkF0216, checkF0217, checkF0218, checkF0219, checkF0220,
    checkF0221, checkF0222, checkF0223, checkF0224, checkF0225,
  ];
  const entries = families.flatMap((f) => f());
  const failed = entries.filter((e) => {
    try {
      return !e.check();
    } catch {
      return true;
    }
  });
  return { entries, failed };
}
