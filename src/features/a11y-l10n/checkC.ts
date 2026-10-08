// AURORA-10000: AI-68 批次（族0336~0340）自检条目，勿删。

import { CheckEntry, memoized } from './types';
import * as C from './groupC';

export function checkF0336(): CheckEntry[] {
  return [
    memoized({ id: 'F08376', name: '区域壁纸包', check: () => C.wallpapersFor('zh-CN').includes('长城') && C.wallpapersFor('xx').includes('长城') && C.REGION_WALLPAPERS.length === 2 }),
    memoized({ id: 'F08377', name: '区域音景', check: () => C.REGION_AMBIENCE['zh-CN']![0] === '江南雨巷' && C.REGION_AMBIENCE.en!.length === 2 }),
    memoized({ id: 'F08378', name: '节日主题注册', check: () => C.festivalTheme('zh-CN', '春节') === 'zh-CN:春节' }),
    memoized({ id: 'F08379', name: '假期日历', check: () => C.REGION_HOLIDAYS['zh-CN']!.length === 3 && C.REGION_HOLIDAYS.en!.includes('2026-07-04') }),
    memoized({ id: 'F08380', name: '天气数据源', check: () => C.weatherSource('en') === 'local-met-global' && C.weatherSource('xx') === 'local-met' }),
    memoized({ id: 'F08381', name: '新闻位预留', check: () => C.NEWS_SLOT.reserved && !C.NEWS_SLOT.enabled }),
    memoized({ id: 'F08382', name: '搜索建议过滤', check: () => C.searchSuggest('zh-CN', '天').join() === '天气' && C.searchSuggest('en', 'z').length === 0 }),
    memoized({ id: 'F08383', name: 'emoji 偏好表', check: () => C.EMOJI_PREFERENCE['zh-CN'] === 'color' && C.EMOJI_PREFERENCE['de-DE'] === 'regional' }),
    memoized({ id: 'F08384', name: '货币符号', check: () => C.currencySymbol('CNY') === '¥' && C.currencySymbol('EUR') === '€' && C.currencySymbol('XYZ') === 'XYZ' }),
    memoized({ id: 'F08385', name: '税号位预留', check: () => C.TAX_ID_SLOT.reserved && !C.TAX_ID_SLOT.enabled }),
    memoized({ id: 'F08386', name: '法律声明本地化', check: () => C.legalNotice('zh-CN').includes('本地') && C.legalNotice('en').includes('GDPR') && C.legalNotice('xx').includes('GDPR') }),
    memoized({ id: 'F08387', name: '年龄分级表', check: () => C.AGE_RATINGS['zh-CN']!.adult === '18+' && C.AGE_RATINGS.en!.all === 'E' }),
    memoized({ id: 'F08388', name: '内容过滤分级', check: () => C.contentFilter('zh-CN', 'teen') && !C.contentFilter('zh-CN', 'adult') && C.contentFilter('en', 'adult') }),
    memoized({ id: 'F08389', name: '代理建议位预留', check: () => C.PROXY_SLOT.reserved && !C.PROXY_SLOT.enabled }),
    memoized({ id: 'F08390', name: '时间格式偏好', check: () => C.TIME_FORMAT_PREF['zh-CN'] === '24h' && C.TIME_FORMAT_PREF.en === '12h' }),
    memoized({ id: 'F08391', name: '文化色语义', check: () => C.CULTURE_COLORS['zh-CN']!.joy === '红' && C.CULTURE_COLORS['zh-CN']!.mourn === '白' && C.CULTURE_COLORS.en!.mourn === 'black' }),
    memoized({ id: 'F08392', name: '手势文化差异', check: () => C.gestureMeaning('thumbsUp', 'en') === 'good' && C.gestureMeaning('thumbsUp', 'fa') === '冒犯' && C.gestureMeaning('unknown', 'xx') === '中性' }),
    memoized({ id: 'F08393', name: '区域教学五步', check: () => C.REGION_TUTORIAL.length === 5 }),
    memoized({ id: 'F08394', name: '区域彩蛋低频', check: () => C.regionEgg(false, 83) === null && C.regionEgg(true, 166) !== null && C.regionEgg(true, 1) === null }),
    memoized({ id: 'F08395', name: '区域测试用例', check: () => C.REGION_TESTS.length === 5 }),
    memoized({ id: 'F08396', name: '区域回归清单', check: () => C.REGION_REGRESSION.length === 3 }),
    memoized({ id: 'F08397', name: '区域文档页', check: () => C.REGION_DOCS.length === 3 }),
    memoized({ id: 'F08398', name: '区域 API 冻结', check: () => C.REGION_API_VERSION === '1.0' }),
    memoized({ id: 'F08399', name: '区域收官清单', check: () => C.REGION_FINALE.length === 5 }),
    memoized({ id: 'F08400', name: '区域致谢', check: () => C.REGION_THANKS.length === 3 }),
  ];
}

export function checkF0337(): CheckEntry[] {
  const wl = new C.WcagChecklist();
  wl.add('1.1.1', 'A', true); wl.add('1.4.3', 'AA', true); wl.add('2.4.8', 'AAA', false);
  const items = wl.items;
  return [
    memoized({ id: 'F08401', name: 'WCAG 2.2 清单引擎', check: () => wl.items.length === 3 && wl.score === 67 }),
    memoized({ id: 'F08402', name: 'A 级达标', check: () => C.wcagLevel(items, 'A') === true }),
    memoized({ id: 'F08403', name: 'AA 级达标', check: () => C.wcagLevel(items, 'AA') === true }),
    memoized({ id: 'F08404', name: 'AAA 目标未达', check: () => C.wcagLevel(items, 'AAA') === false }),
    memoized({ id: 'F08405', name: '自动审计对比度', check: () => C.contrastPasses([0, 0, 0], [255, 255, 255], 'AAA') && !C.contrastPasses([128, 128, 128], [136, 136, 136], 'AA') && C.wcagContrast([0, 0, 0], [255, 255, 255]) > 20 }),
    memoized({ id: 'F08406', name: '人工审计计划', check: () => { const p = C.manualAuditPlan(['a', 'b', 'c', 'd', 'e', 'f'], 5); return p.length === 2 && p[1]!.length === 1; } }),
    memoized({ id: 'F08407', name: '用户测试评分', check: () => C.userTestScore({ participant: 'p', disability: 'visual', tasksPassed: 8, tasksTotal: 10 }) === 80 && C.userTestScore({ participant: 'p', disability: 'motor', tasksPassed: 0, tasksTotal: 0 }) === 100 }),
    memoized({ id: 'F08408', name: '报告公开生成', check: () => { const r = C.publicReport([{ criterion: '1.1.1', passed: true }, { criterion: '1.4.3', passed: false }]); return r.total === 2 && r.passed === 1 && r.summary === '存在未达标项'; } }),
    memoized({ id: 'F08409', name: '修复 SLA', check: () => C.fixSla('P0') === 3 && C.fixSla('P1') === 14 && C.fixSla('P2') === 60 }),
    memoized({ id: 'F08410', name: 'a11y CI 门禁', check: () => C.a11yCiGate(96, 100) && !C.a11yCiGate(94, 100) && !C.a11yCiGate(100, 90) }),
    memoized({ id: 'F08411', name: '组件文档登记', check: () => C.A11Y_COMPONENT_DOCS.length === 3 && C.A11Y_COMPONENT_DOCS[0]!.roles.includes('switch') }),
    memoized({ id: 'F08412', name: '模式规范五类', check: () => C.PATTERN_SPECS.length === 5 }),
    memoized({ id: 'F08413', name: '颜色规范阈值', check: () => C.COLOR_SPECS.text === 4.5 && C.COLOR_SPECS.largeText === 3 }),
    memoized({ id: 'F08414', name: '动效规范', check: () => C.MOTION_SPECS.respectReducedMotion && C.MOTION_SPECS.maxFlashesPerSecond === 3 && C.MOTION_SPECS.durationTokensOnly }),
    memoized({ id: 'F08415', name: '声音规范', check: () => C.SOUND_SPECS.neverAudioOnly && C.SOUND_SPECS.visualFallback }),
    memoized({ id: 'F08416', name: '表单规范三要素', check: () => C.formA11ySpec(true, true, true) && !C.formA11ySpec(true, false, true) }),
    memoized({ id: 'F08417', name: '表格规范', check: () => C.tableA11ySpec(true, true) && !C.tableA11ySpec(true, false) }),
    memoized({ id: 'F08418', name: '模态规范三要素', check: () => C.modalA11ySpec(true, true, true) && !C.modalA11ySpec(true, true, false) }),
    memoized({ id: 'F08419', name: '焦点规范', check: () => C.FOCUS_SPEC.ringNeverHidden && C.FOCUS_SPEC.minContrast === 3 }),
    memoized({ id: 'F08420', name: '读屏矩阵', check: () => C.SCREEN_READER_MATRIX.length === 3 && C.screenReaderMatrixPass() }),
    memoized({ id: 'F08421', name: '认证徽章', check: () => C.a11yBadge('AA') === 'A11Y-AA-CERTIFIED' }),
    memoized({ id: 'F08422', name: '年报达标率', check: () => C.annualReport(2026, 118, 125).rate === 94.4 }),
    memoized({ id: 'F08423', name: '委员会', check: () => { const c = new C.A11yCouncil(); c.addMember('顾问甲'); return c.addMember('顾问甲') === false && c.resolve('扩大读屏覆盖') === 1 && c.members.length === 1; } }),
    memoized({ id: 'F08424', name: '培训课程表', check: () => C.A11Y_TRAINING.length === 5 }),
    memoized({ id: 'F08425', name: '认证收官清单', check: () => C.CERT_FINALE.length === 5 }),
  ];
}

export function checkF0338(): CheckEntry[] {
  const pack = C.testPack(1);
  return [
    memoized({ id: 'F08426', name: '三语截图矩阵', check: () => C.shotMatrix(['zh-CN', 'en'], ['设置', '关于']).length === 4 && C.shotMatrix(['zh-CN'], ['a'])[0] === 'zh-CN/a' }),
    memoized({ id: 'F08427', name: 'RTL 截图', check: () => C.rtlShots(['设置'])[0] === 'rtl/设置' }),
    memoized({ id: 'F08428', name: '伪本地截图', check: () => C.pseudoShots(['设置'])[0] === 'pseudo/设置' }),
    memoized({ id: 'F08429', name: '长文本溢出', check: () => C.longTextOverflow(20, C.DE_LONG_SAMPLE) && !C.longTextOverflow(200, C.DE_LONG_SAMPLE) }),
    memoized({ id: 'F08430', name: '短文本不截断', check: () => C.shortTextOk('OK') && !C.shortTextOk('', 1) }),
    memoized({ id: 'F08431', name: '无空格日文位', check: () => C.noSpaceText(C.JA_SAMPLE) && !C.noSpaceText('a b') }),
    memoized({ id: 'F08432', name: '大小写语言', check: () => C.turkishI('i') }),
    memoized({ id: 'F08433', name: '溢出矩阵', check: () => { const m = C.overflowMatrix([{ name: 'a', width: 5, text: 'very long string' }, { name: 'b', width: 50, text: 'short' }]); return m[0]!.overflow && !m[1]!.overflow; } }),
    memoized({ id: 'F08434', name: '日期矩阵', check: () => { const d = C.dateMatrix(new Date(2026, 0, 2), ['zh-CN', 'en-US']); return d.length === 2 && d[0]!.includes('2026') && d[1]!.includes('2026'); } }),
    memoized({ id: 'F08435', name: '数字矩阵', check: () => { const n = C.numberMatrix(1234.5, ['de-DE', 'en-US']); return n[0]!.includes('1.234') && n[1]!.includes('1,234'); } }),
    memoized({ id: 'F08436', name: '货币矩阵', check: () => { const c = C.currencyMatrix(5, ['de-DE', 'en-US'], 'EUR'); return c.every((s) => s.includes('€')); } }),
    memoized({ id: 'F08437', name: '排序矩阵', check: () => { const s = C.sortMatrix(['a', 'B', 'c'], ['en', 'de']); return s.en!.length === 3 && s.de!.length === 3; } }),
    memoized({ id: 'F08438', name: '复数矩阵', check: () => { const p = C.pluralMatrix(['en', 'zh-CN'], [1, 2]); return p.en!.join() === 'one,other' && p['zh-CN']!.every((x) => x === 'other'); } }),
    memoized({ id: 'F08439', name: '输入法矩阵', check: () => C.IME_MATRIX.length === 3 && C.IME_MATRIX[0]!.ime === 'pinyin' }),
    memoized({ id: 'F08440', name: '字体矩阵', check: () => { const m = C.fontMatrix(['Noto Sans SC'], ['中', 'a']); return m['Noto Sans SC']!.length === 2; } }),
    memoized({ id: 'F08441', name: 'TTS 矩阵', check: () => C.TTS_MATRIX.length === 5 && C.TTS_MATRIX.includes('ja-JP') }),
    memoized({ id: 'F08442', name: 'STT 矩阵', check: () => C.STT_MATRIX.length === 2 }),
    memoized({ id: 'F08443', name: 'DST 矩阵', check: () => { const d = C.dstMatrix('America/New_York'); return d.hasDst && d.winterOffset !== d.summerOffset && !C.dstMatrix('Asia/Shanghai').hasDst; } }),
    memoized({ id: 'F08444', name: '回归自动化', check: () => { const r = C.regressionAuto(['a', 'b', 'c'], ['a', 'x', 'c']); return !r.ok && r.diffs[0] === 'b' && C.regressionAuto(['a'], ['a']).ok; } }),
    memoized({ id: 'F08445', name: '测试数据包', check: () => pack.dates.length === 3 && pack.numbers.length === 5 && pack.strings.length === 4 }),
    memoized({ id: 'F08446', name: '本地化教学五步', check: () => C.L10N_TEST_TUTORIAL.length === 5 }),
    memoized({ id: 'F08447', name: '本地化彩蛋低频', check: () => C.l10nTestEgg(false, 79) === null && C.l10nTestEgg(true, 158) !== null && C.l10nTestEgg(true, 1) === null }),
    memoized({ id: 'F08448', name: '测试 API 冻结', check: () => C.L10N_TEST_API_VERSION === '1.0' }),
    memoized({ id: 'F08449', name: '测试看板', check: () => { const b = C.testBoard({ shots: { passed: 9, total: 10 }, dst: { passed: 2, total: 2 } }); return b[0]!.rate === 90 && b[1]!.rate === 100; } }),
    memoized({ id: 'F08450', name: '测试收官清单', check: () => C.L10N_TEST_FINALE.length === 5 }),
  ];
}

export function checkF0339(): CheckEntry[] {
  const cc = new C.CultureCouncil();
  return [
    memoized({ id: 'F08451', name: '敏感色板', check: () => C.SENSITIVE_COLORS['白+丧(东亚)']!.color === '#ffffff' && Object.keys(C.SENSITIVE_COLORS).length === 3 }),
    memoized({ id: 'F08452', name: '图标审查', check: () => C.iconReview('owl').length === 1 && C.iconReview('smile').length === 0 && C.iconReview('hand-ok').length === 1 }),
    memoized({ id: 'F08453', name: '本地姓名示例', check: () => C.SAMPLE_NAMES['zh-CN']![0] === '王小明' && C.SAMPLE_NAMES.en!.length === 2 }),
    memoized({ id: 'F08454', name: '日期示例', check: () => C.sampleDate('en').includes('2026') && C.sampleDate('zh-CN').includes('2026') }),
    memoized({ id: 'F08455', name: '支付显示位预留', check: () => C.PAYMENT_SLOT.reserved && !C.PAYMENT_SLOT.enabled }),
    memoized({ id: 'F08456', name: '邮件称呼礼仪', check: () => C.emailSalutation('en').includes('Dear') && C.emailSalutation('ja-JP').includes('様') && C.emailSalutation('zh-CN').includes('此致') }),
    memoized({ id: 'F08457', name: '颜色测试集', check: () => C.CULTURE_COLOR_TESTS.length === 3 }),
    memoized({ id: 'F08458', name: '图标测试集', check: () => C.CULTURE_ICON_TESTS.length === 3 }),
    memoized({ id: 'F08459', name: '审查委员会', check: () => cc.review('绿色帽子图标', 'reject', '文化敏感') === 1 && cc.review('红金喜庆色', 'pass') === 2 && cc.pending === 1 }),
    memoized({ id: 'F08460', name: '社区贡献通道', check: () => { const c = C.cultureContribution('新色板', 'zh-CN'); return c.id === 'culture:zh-CN:新色板' && c.status === 'received'; } }),
    memoized({ id: 'F08461', name: '设计幕后故事', check: () => C.CULTURE_STORIES.length === 3 }),
    memoized({ id: 'F08462', name: '文化展览页', check: () => C.CULTURE_EXHIBITS.length === 3 }),
    memoized({ id: 'F08463', name: '文化教学五步', check: () => C.CULTURE_TUTORIAL.length === 5 }),
    memoized({ id: 'F08464', name: '文化彩蛋低频', check: () => C.cultureEgg(false, 73) === null && C.cultureEgg(true, 146) !== null && C.cultureEgg(true, 1) === null }),
    memoized({ id: 'F08465', name: '文化回归清单', check: () => C.CULTURE_REGRESSION.length === 3 }),
    memoized({ id: 'F08466', name: '文化文档页', check: () => C.CULTURE_DOCS.length === 3 }),
    memoized({ id: 'F08467', name: '文化 API 冻结', check: () => C.CULTURE_API_VERSION === '1.0' }),
    memoized({ id: 'F08468', name: '文化收官清单', check: () => C.CULTURE_FINALE.length === 5 }),
    memoized({ id: 'F08469', name: '文化致谢', check: () => C.CULTURE_THANKS.length === 3 }),
    memoized({ id: 'F08470', name: '文化实验位预留', check: () => C.CULTURE_EXP_SLOT.reserved && !C.CULTURE_EXP_SLOT.enabled }),
    memoized({ id: 'F08471', name: '文化测试二批', check: () => C.CULTURE_TESTS_2.length === 3 }),
    memoized({ id: 'F08472', name: '文化审计项', check: () => C.CULTURE_AUDIT.length === 3 }),
    memoized({ id: 'F08473', name: '文化日历', check: () => C.CULTURE_CALENDAR['zh-CN']!.length === 2 && C.CULTURE_CALENDAR.global!.length === 2 }),
    memoized({ id: 'F08474', name: '文化壁纸集', check: () => C.CULTURE_WALLPAPERS.length === 4 && C.CULTURE_WALLPAPERS.includes('水墨山水') }),
    memoized({ id: 'F08475', name: '文化二期收官', check: () => C.CULTURE_FINALE_2.length === 4 }),
  ];
}

export function checkF0340(): CheckEntry[] {
  return [
    memoized({ id: 'F08476', name: 'NVDA 兼容', check: () => C.atList('screen-reader')[0]!.name === 'NVDA' && C.atList('screen-reader')[0]!.compatible }),
    memoized({ id: 'F08477', name: '放大兼容', check: () => C.atList('magnifier')[0]!.name === 'ZoomText' && C.atList('magnifier')[0]!.compatible }),
    memoized({ id: 'F08478', name: '语音兼容', check: () => C.atList('voice')[0]!.name === 'Dragon' && C.atList('voice')[0]!.compatible }),
    memoized({ id: 'F08479', name: '开关兼容', check: () => C.atList('switch')[0]!.name === 'Switch Access' && C.atList('switch')[0]!.compatible }),
    memoized({ id: 'F08480', name: '眼动兼容位', check: () => { const d = C.atList('eye')[0]!; return !d.compatible && d.note === '需眼动仪'; } }),
    memoized({ id: 'F08481', name: '点显器位', check: () => { const d = C.atList('braille')[0]!; return !d.compatible && d.note === '预留'; } }),
    memoized({ id: 'F08482', name: '助听兼容位', check: () => { const d = C.atList('hearing')[0]!; return !d.compatible && d.note === '预留'; } }),
    memoized({ id: 'F08483', name: '替代键盘', check: () => C.atList('keyboard')[0]!.name === 'One-Hand Keyboard' && C.atList('keyboard')[0]!.compatible }),
    memoized({ id: 'F08484', name: '摇杆兼容', check: () => C.atList('joystick')[0]!.name === 'QuadStick' && C.atList('joystick')[0]!.compatible }),
    memoized({ id: 'F08485', name: '兼容列表公开', check: () => C.AT_DEVICES.length === 9 && new Set(C.AT_DEVICES.map((d) => d.kind)).size === 9 }),
    memoized({ id: 'F08486', name: '修复流程五步', check: () => C.atFixFlow(0) === '接收兼容报告' && C.atFixFlow(4) === '随版本发布' && C.atFixFlow(9) === '随版本发布' }),
    memoized({ id: 'F08487', name: '伙伴计划', check: () => C.AT_PARTNERS.length === 3 }),
    memoized({ id: 'F08488', name: '厂商联系通道', check: () => C.vendorContact('nvda').slaDays === 14 && C.vendorContact('nvda').to.includes('nvda') }),
    memoized({ id: 'F08489', name: '兼容 API 冻结', check: () => C.AT_API_VERSION === '1.0' }),
    memoized({ id: 'F08490', name: '生态文档页', check: () => C.AT_DOCS.length === 3 }),
    memoized({ id: 'F08491', name: '生态教学五步', check: () => C.AT_TUTORIAL.length === 5 }),
    memoized({ id: 'F08492', name: '生态彩蛋低频', check: () => C.atEgg(false, 71) === null && C.atEgg(true, 142) !== null && C.atEgg(true, 1) === null }),
    memoized({ id: 'F08493', name: '生态测试矩阵', check: () => { const m = C.atMatrix(); return m['screen-reader']!.passed === 1 && m.braille!.passed === 0 && Object.keys(m).length === 9; } }),
    memoized({ id: 'F08494', name: '生态回归清单', check: () => C.AT_REGRESSION.length === 3 }),
    memoized({ id: 'F08495', name: '生态审计项', check: () => C.AT_AUDIT.length === 3 }),
    memoized({ id: 'F08496', name: '生态看板兼容率', check: () => C.atBoard() === 67 }),
    memoized({ id: 'F08497', name: '生态报告', check: () => { const r = C.atReport(); return r.total === 9 && r.compatible === 6 && r.rate === 67; } }),
    memoized({ id: 'F08498', name: '生态收官清单', check: () => C.AT_FINALE.length === 5 }),
    memoized({ id: 'F08499', name: '生态致谢', check: () => C.AT_THANKS.length === 3 }),
    memoized({ id: 'F08500', name: '生态社区入口', check: () => C.AT_COMMUNITY.forum === 'a11y.community' && C.AT_COMMUNITY.monthly === true }),
  ];
}
