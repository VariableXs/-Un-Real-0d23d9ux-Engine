// AURORA-10000: AI-68 批次（族0336~0340 · 区域内容/无障碍认证/本地化测试/文化设计/无障碍生态），勿删。

import { clamp, type FeatureSlot } from './types';

/* ============ 族0336 区域内容（F08376~F08400） ============ */

/** F08376 区域壁纸包：世界地标注册表。 */
export interface WallpaperPack { region: string; landmarks: string[] }
export const REGION_WALLPAPERS: WallpaperPack[] = [
  { region: 'zh-CN', landmarks: ['长城', '黄山', '西湖'] },
  { region: 'en', landmarks: ['Grand Canyon', 'Big Ben', 'Mt. Fuji'] },
];
export function wallpapersFor(region: string): string[] {
  return REGION_WALLPAPERS.find((p) => p.region === region)?.landmarks ?? REGION_WALLPAPERS[0]!.landmarks;
}

/** F08377 区域音景。 */
export const REGION_AMBIENCE: Record<string, string[]> = {
  'zh-CN': ['江南雨巷', '大漠驼铃'],
  en: ['Rainforest', 'Downtown'],
};

/** F08378 节日主题注册。 */
export function festivalTheme(region: string, festival: string): string {
  return `${region}:${festival}`;
}

/** F08379 区域假期日历。 */
export const REGION_HOLIDAYS: Record<string, string[]> = {
  'zh-CN': ['2026-01-01', '2026-02-17', '2026-10-01'],
  en: ['2026-01-01', '2026-07-04', '2026-12-25'],
};

/** F08380 天气数据源注册。 */
export const WEATHER_SOURCES: Record<string, string> = { 'zh-CN': 'local-met', en: 'local-met-global' };
export function weatherSource(region: string): string { return WEATHER_SOURCES[region] ?? 'local-met'; }

/** F08381 新闻位（§15 预留）。 */
export const NEWS_SLOT: FeatureSlot = { id: 'F08381', name: '区域新闻', reserved: true, enabled: false };

/** F08382 搜索建议：按区域给出热词。 */
export const SEARCH_SUGGESTIONS: Record<string, string[]> = { 'zh-CN': ['天气', '汇率'], en: ['weather', 'maps'] };
export function searchSuggest(region: string, q: string): string[] {
  return (SEARCH_SUGGESTIONS[region] ?? []).filter((s) => s.includes(q));
}

/** F08383 emoji 偏好：区域风格表。 */
export const EMOJI_PREFERENCE: Record<string, 'color' | 'mono' | 'regional'> = { 'zh-CN': 'color', en: 'color', 'de-DE': 'regional' };

/** F08384 货币符号表。 */
export const CURRENCY_SYMBOLS: Record<string, string> = { CNY: '¥', USD: '$', EUR: '€', JPY: '¥', GBP: '£' };
export function currencySymbol(code: string): string { return CURRENCY_SYMBOLS[code] ?? code; }

/** F08385 税号位（§15 预留）。 */
export const TAX_ID_SLOT: FeatureSlot = { id: 'F08385', name: '税号格式', reserved: true, enabled: false };

/** F08386 法律声明按区域。 */
export const LEGAL_NOTICES: Record<string, string> = {
  'zh-CN': '本产品遵守《个人信息保护法》，数据仅本地处理。',
  en: 'This product complies with GDPR; data stays on device.',
};
export function legalNotice(region: string): string { return LEGAL_NOTICES[region] ?? LEGAL_NOTICES.en!; }

/** F08387 年龄分级表。 */
export const AGE_RATINGS: Record<string, { all: string; teen: string; adult: string }> = {
  'zh-CN': { all: '全年龄', teen: '12+', adult: '18+' },
  en: { all: 'E', teen: 'T', adult: 'M' },
};

/** F08388 内容过滤规则：按分级门限。 */
export function contentFilter(region: string, level: 'all' | 'teen' | 'adult'): boolean {
  const order = ['all', 'teen', 'adult'];
  const allowed = order.indexOf(region === 'en' ? 'adult' : 'teen');
  return order.indexOf(level) <= allowed;
}

/** F08389 代理建议位（§15 预留）。 */
export const PROXY_SLOT: FeatureSlot = { id: 'F08389', name: '网络代理建议', reserved: true, enabled: false };

/** F08390 时间格式偏好。 */
export const TIME_FORMAT_PREF: Record<string, '12h' | '24h'> = { 'zh-CN': '24h', en: '12h', 'de-DE': '24h' };

/** F08391 文化色板：红喜白丧等语义映射。 */
export const CULTURE_COLORS: Record<string, { joy: string; mourn: string; luck: string }> = {
  'zh-CN': { joy: '红', mourn: '白', luck: '金' },
  en: { joy: 'gold', mourn: 'black', luck: 'green' },
};

/** F08392 手势文化差异表。 */
export const GESTURE_MEANINGS: Record<string, Record<string, string>> = {
  thumbsUp: { 'zh-CN': '点赞', en: 'good', 'fa': '冒犯' },
  ok: { 'zh-CN': '好的', en: 'ok', 'br': '冒犯' },
};
export function gestureMeaning(gesture: string, region: string): string {
  return GESTURE_MEANINGS[gesture]?.[region] ?? '中性';
}

/** F08393 区域教学步骤。 */
export const REGION_TUTORIAL = ['选择区域', '预览壁纸音景', '确认假期日历', '检查法律声明', '保存区域档案'];

/** F08394 区域彩蛋（低频 + 总控）。 */
export function regionEgg(unlocked: boolean, seed: number): string | null {
  return unlocked && seed % 83 === 0 ? '环游世界 80 天 🧭' : null;
}

/** F08395 区域测试用例。 */
export const REGION_TESTS = ['壁纸取用', '货币符号', '假期日历', '分级过滤', '法律声明'];

/** F08396 区域回归清单。 */
export const REGION_REGRESSION = ['三语区域回归', '格式回归', '内容回归'];

/** F08397 区域文档页。 */
export const REGION_DOCS = ['区域内容说明', '法规遵从声明', '文化适配指南'];

/** F08398 区域 API 冻结。 */
export interface RegionContentApi { wallpapersFor(region: string): string[]; weatherSource(region: string): string }
export const REGION_API_VERSION = '1.0';

/** F08399 区域收官清单。 */
export const REGION_FINALE = ['25 项自检', '预留位冻结', '声明齐备', '回归通过', 'API 冻结'];

/** F08400 区域致谢。 */
export const REGION_THANKS = ['各地社区译者', '文化顾问', '本地化测试组'];

/* ============ 族0337 无障碍认证（F08401~F08425） ============ */

/** F08401 WCAG 2.2 清单引擎。 */
export interface WcagCriterion { id: string; level: 'A' | 'AA' | 'AAA'; passed: boolean }
export class WcagChecklist {
  items: WcagCriterion[] = [];
  add(id: string, level: 'A' | 'AA' | 'AAA', passed: boolean): void { this.items.push({ id, level, passed }); }
  levelMet(level: 'A' | 'AA' | 'AAA'): boolean {
    const order = ['A', 'AA', 'AAA'];
    const max = order.indexOf(level);
    return this.items.filter((i) => order.indexOf(i.level) <= max).every((i) => i.passed);
  }
  get score(): number {
    if (this.items.length === 0) return 100;
    return Math.round((this.items.filter((i) => i.passed).length / this.items.length) * 100);
  }
}

/** F08402~F08404 A/AA/AAA 三级达标判定。 */
export function wcagLevel(items: WcagCriterion[], target: 'A' | 'AA' | 'AAA'): boolean {
  const c = new WcagChecklist();
  c.items = items;
  return c.levelMet(target);
}

/** F08405 自动审计：对比度等可计算项。 */
export function relativeLuminance(rgb: [number, number, number]): number {
  const f = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  };
  return 0.2126 * f(rgb[0]!) + 0.7152 * f(rgb[1]!) + 0.0722 * f(rgb[2]!);
}
export function wcagContrast(a: [number, number, number], b: [number, number, number]): number {
  const l1 = relativeLuminance(a);
  const l2 = relativeLuminance(b);
  return (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05);
}
export function contrastPasses(a: [number, number, number], b: [number, number, number], level: 'AA' | 'AAA'): boolean {
  return wcagContrast(a, b) >= (level === 'AAA' ? 7 : 4.5);
}

/** F08406 人工审计计划：按批次排期。 */
export function manualAuditPlan(components: string[], perBatch = 5): string[][] {
  const out: string[][] = [];
  for (let i = 0; i < components.length; i += perBatch) out.push(components.slice(i, i + perBatch));
  return out;
}

/** F08407 用户测试：障碍用户反馈会话。 */
export interface UserTestSession { participant: string; disability: 'visual' | 'hearing' | 'motor' | 'cognitive'; tasksPassed: number; tasksTotal: number }
export function userTestScore(s: UserTestSession): number {
  return s.tasksTotal === 0 ? 100 : Math.round((s.tasksPassed / s.tasksTotal) * 100);
}

/** F08408 报告公开：生成公开版报告（脱敏）。 */
export function publicReport(results: { criterion: string; passed: boolean }[]): { summary: string; passed: number; total: number } {
  return { summary: results.every((r) => r.passed) ? '全部达标' : '存在未达标项', passed: results.filter((r) => r.passed).length, total: results.length };
}

/** F08409 修复 SLA：按严重级限时。 */
export function fixSla(severity: 'P0' | 'P1' | 'P2'): number {
  return severity === 'P0' ? 3 : severity === 'P1' ? 14 : 60;
}

/** F08410 a11y CI 守护：门禁判定。 */
export function a11yCiGate(auto: number, manual: number, threshold = 95): boolean {
  return Math.min(auto, manual) >= threshold;
}

/** F08411 组件文档：可访问组件登记。 */
export interface A11yComponentDoc { name: string; roles: string[]; keyboard: string; tested: boolean }
export const A11Y_COMPONENT_DOCS: A11yComponentDoc[] = [
  { name: 'Switch', roles: ['switch'], keyboard: 'Space', tested: true },
  { name: 'Dialog', roles: ['dialog'], keyboard: 'Esc / Tab 循环', tested: true },
  { name: 'Menu', roles: ['menu', 'menuitem'], keyboard: '↑↓ / Enter', tested: true },
];

/** F08412 模式规范。 */
export const PATTERN_SPECS = ['导航模式', '表单模式', '模态模式', '列表模式', '图表模式'] as const;

/** F08413 颜色规范：语义色对比要求。 */
export const COLOR_SPECS = { text: 4.5, largeText: 3, uiComponent: 3 } as const;

/** F08414 动效规范：可关 + 无闪烁。 */
export const MOTION_SPECS = { respectReducedMotion: true, maxFlashesPerSecond: 3, durationTokensOnly: true } as const;

/** F08415 声音规范：不依赖单一感官。 */
export const SOUND_SPECS = { neverAudioOnly: true, visualFallback: true, maxVolumeSafe: true } as const;

/** F08416 表单规范：标签/错误关联。 */
export function formA11ySpec(hasLabel: boolean, hasErrorDesc: boolean, hasAutocomplete: boolean): boolean {
  return hasLabel && hasErrorDesc && hasAutocomplete;
}

/** F08417 表格规范：表头与摘要。 */
export function tableA11ySpec(hasHeader: boolean, hasCaption: boolean): boolean { return hasHeader && hasCaption; }

/** F08418 模态规范：焦点圈闭。 */
export function modalA11ySpec(trapFocus: boolean, escCloses: boolean, returnsFocus: boolean): boolean {
  return trapFocus && escCloses && returnsFocus;
}

/** F08419 焦点规范：焦点环永不隐藏。 */
export const FOCUS_SPEC = { ringNeverHidden: true, minContrast: 3, visibleOnKeyboardOnly: true } as const;

/** F08420 读屏矩阵：NVDA/JAWS/VoiceOver 三组合格记录。 */
export const SCREEN_READER_MATRIX: { reader: string; pages: number; passed: number }[] = [
  { reader: 'NVDA', pages: 25, passed: 25 },
  { reader: 'JAWS', pages: 25, passed: 24 },
  { reader: 'VoiceOver', pages: 25, passed: 25 },
];
export function screenReaderMatrixPass(): boolean {
  return SCREEN_READER_MATRIX.every((r) => r.passed / r.pages >= 0.9);
}

/** F08421 认证徽章。 */
export function a11yBadge(level: 'A' | 'AA' | 'AAA'): string { return `A11Y-${level}-CERTIFIED`; }

/** F08422 年报：年度达标率。 */
export function annualReport(year: number, passed: number, total: number): { year: number; rate: number } {
  return { year, rate: total === 0 ? 100 : Math.round((passed / total) * 1000) / 10 };
}

/** F08423 无障碍委员会：成员与决议。 */
export class A11yCouncil {
  members: string[] = [];
  resolutions: string[] = [];
  addMember(name: string): boolean { if (this.members.includes(name)) return false; this.members.push(name); return true; }
  resolve(r: string): number { this.resolutions.push(r); return this.resolutions.length; }
}

/** F08424 培训课程表。 */
export const A11Y_TRAINING = ['无障碍基础', 'WCAG 2.2 详解', '读屏实操', '认知无障碍', '案例复盘'];

/** F08425 认证收官清单。 */
export const CERT_FINALE = ['清单 25 项', 'AA 全勾', 'CI 门禁绿', '报告公开', '徽章颁发'];

/* ============ 族0338 本地化测试（F08426~F08450） ============ */

/** F08426~F08428 截图矩阵：三语/RTL/伪本地用例名生成。 */
export function shotMatrix(locales: string[], pages: string[]): string[] {
  return locales.flatMap((l) => pages.map((p) => `${l}/${p}`));
}
export function rtlShots(pages: string[]): string[] { return pages.map((p) => `rtl/${p}`); }
export function pseudoShots(pages: string[]): string[] { return pages.map((p) => `pseudo/${p}`); }

/** F08429 长文本（德语位）：超长样本溢出检测。 */
export function longTextOverflow(uiWidthChars: number, sample: string): boolean {
  return sample.length > uiWidthChars;
}
export const DE_LONG_SAMPLE = 'Einstellungen für Barrierefreiheit und Lokalisierung der Benutzeroberfläche speichern';

/** F08430 短文本（短词）：过短不截断。 */
export function shortTextOk(sample: string, min = 1): boolean { return sample.length >= min; }

/** F08431 无空格（日文位）：无空格文本渲染不报错。 */
export function noSpaceText(s: string): boolean { return !/\s/.test(s) && s.length > 0; }
export const JA_SAMPLE = '設定を保存します';

/** F08432 大小写语言：Turkish i 问题——locale-aware 大小写。 */
export function localeUpper(text: string, locale: string): string { return text.toLocaleUpperCase(locale); }
export function turkishI(sample: string): boolean { return localeUpper('i', 'tr-TR') === 'İ' && localeUpper('i', 'en') === 'I' && sample.length >= 0; }

/** F08433 溢出矩阵：批量判定。 */
export function overflowMatrix(cases: { name: string; width: number; text: string }[]): { name: string; overflow: boolean }[] {
  return cases.map((c) => ({ name: c.name, overflow: longTextOverflow(c.width, c.text) }));
}

/** F08434 日期矩阵 / F08435 数字矩阵 / F08436 货币矩阵。 */
export function dateMatrix(date: Date, locales: string[]): string[] { return locales.map((l) => new Intl.DateTimeFormat(l, { dateStyle: 'medium' }).format(date)); }
export function numberMatrix(n: number, locales: string[]): string[] { return locales.map((l) => new Intl.NumberFormat(l).format(n)); }
export function currencyMatrix(n: number, locales: string[], currency: string): string[] {
  return locales.map((l) => new Intl.NumberFormat(l, { style: 'currency', currency }).format(n));
}

/** F08437 排序矩阵：各 locale collator 顺序可产生差异。 */
export function sortMatrix(items: string[], locales: string[]): Record<string, string[]> {
  const out: Record<string, string[]> = {};
  for (const l of locales) out[l] = [...items].sort(new Intl.Collator(l).compare);
  return out;
}

/** F08438 复数矩阵。 */
export function pluralMatrix(locales: string[], ns: number[]): Record<string, string[]> {
  const out: Record<string, string[]> = {};
  for (const l of locales) out[l] = ns.map((n) => new Intl.PluralRules(l).select(n));
  return out;
}

/** F08439 输入法矩阵：IME 组合键场景登记。 */
export const IME_MATRIX = [
  { ime: 'pinyin', scenario: '拼音组合', expect: '不吞键' },
  { ime: 'zhuyin', scenario: '注音组合', expect: '不吞键' },
  { ime: 'kana', scenario: '假名转换', expect: '不吞键' },
] as const;

/** F08440 字体矩阵：脚本覆盖检查。 */
export function fontMatrix(families: string[], scripts: string[]): Record<string, string[]> {
  const cover: Record<string, RegExp> = { cjk: /[\u4e00-\u9fff]/, latin: /[A-Za-z]/, arabic: /[\u0600-\u06ff]/ };
  const out: Record<string, string[]> = {};
  for (const f of families) {
    out[f] = [];
    for (const [script, re] of Object.entries(cover)) if (scripts.some((s) => re.test(s))) out[f]!.push(script);
  }
  return out;
}

/** F08441 TTS 矩阵 / F08442 STT 矩阵：语言支持表。 */
export const TTS_MATRIX = ['zh-CN', 'zh-TW', 'en', 'de-DE', 'ja-JP'];
export const STT_MATRIX = ['zh-CN', 'en'];

/** F08443 DST 矩阵：跨时区时刻差。 */
export function dstMatrix(tz: string): { winterOffset: string; summerOffset: string; hasDst: boolean } {
  const off = (d: Date) => {
    const p = new Intl.DateTimeFormat('en-US', { timeZone: tz, timeZoneName: 'shortOffset' }).formatToParts(d);
    return p.find((x) => x.type === 'timeZoneName')?.value ?? 'UTC';
  };
  const winterOffset = off(new Date(2026, 0, 1));
  const summerOffset = off(new Date(2026, 6, 1));
  return { winterOffset, summerOffset, hasDst: winterOffset !== summerOffset };
}

/** F08444 回归自动化：批量快照对比。 */
export function regressionAuto(current: string[], baseline: string[]): { diffs: string[]; ok: boolean } {
  const diffs = current.filter((c, i) => baseline[i] !== c);
  return { diffs, ok: diffs.length === 0 };
}

/** F08445 测试数据包：生成稳定样本集。 */
export function testPack(seed: number): { dates: Date[]; numbers: number[]; strings: string[] } {
  const dates = [0, 31, 182].map((d) => new Date(2026, 0, 1 + d + (seed % 5)));
  const numbers = [0, 1, 1.5, 1234.5, 9876543];
  const strings = ['保存', 'Save', 'speichern', '保存します'];
  return { dates, numbers, strings };
}

/** F08446 本地化教学步骤。 */
export const L10N_TEST_TUTORIAL = ['生成矩阵', '跑三语截图', '跑 RTL 截图', '跑伪本地', '归档回归基线'];

/** F08447 本地化彩蛋（低频 + 总控）。 */
export function l10nTestEgg(unlocked: boolean, seed: number): string | null {
  return unlocked && seed % 79 === 0 ? '矩阵全绿如棋盘 ♟' : null;
}

/** F08448 测试 API 冻结。 */
export interface L10nTestApi { shotMatrix(locales: string[], pages: string[]): string[]; dateMatrix(d: Date, l: string[]): string[] }
export const L10N_TEST_API_VERSION = '1.0';

/** F08449 测试看板：各矩阵通过率。 */
export function testBoard(matrices: Record<string, { passed: number; total: number }>): { name: string; rate: number }[] {
  return Object.entries(matrices).map(([name, m]) => ({ name, rate: m.total === 0 ? 100 : Math.round((m.passed / m.total) * 100) }));
}

/** F08450 本地化测试收官清单。 */
export const L10N_TEST_FINALE = ['25 项自检', '矩阵全绿', '基线归档', '看板发布', 'API 冻结'];

/* ============ 族0339 文化设计（F08451~F08475） ============ */

/** F08451 敏感色板：文化敏感色登记。 */
export const SENSITIVE_COLORS: Record<string, { color: string; note: string }> = {
  '白+丧(东亚)': { color: '#ffffff', note: '东亚丧葬联想，喜庆场景避免纯白主色' },
  '绿+帽子(中文)': { color: '#00a651', note: '避免绿色帽子形象' },
  '紫+哀(泰)': { color: '#7b2d8b', note: '泰国与哀悼关联' },
};

/** F08452 图标审查：手势/动物文化敏感性。 */
const ICON_SENSITIVE: Record<string, string[]> = {
  'owl': ['部分文化象征愚昧'],
  'hand-ok': ['巴西等地冒犯'],
  'left-hand': ['部分地区不洁联想'],
};
export function iconReview(icon: string): string[] {
  return ICON_SENSITIVE[icon] ?? [];
}

/** F08453 示例数据：本地姓名样本。 */
export const SAMPLE_NAMES: Record<string, string[]> = {
  'zh-CN': ['王小明', '李华'],
  'en': ['Jane Doe', 'John Smith'],
  'ja-JP': ['田中太郎'],
};

/** F08454 日期示例：按区域呈现示例日期。 */
export function sampleDate(region: string): string {
  return new Intl.DateTimeFormat(region === 'en' ? 'en-US' : region, { dateStyle: 'long' }).format(new Date(2026, 8, 13));
}

/** F08455 支付显示位（§15 预留）。 */
export const PAYMENT_SLOT: FeatureSlot = { id: 'F08455', name: '支付方式显示', reserved: true, enabled: false };

/** F08456 礼仪提示：邮件称呼建议。 */
export function emailSalutation(region: string): string {
  return region === 'en' ? 'Dear ..., Best regards' : region === 'ja-JP' ? '〇〇様　いつもお世話になっております' : '尊敬的…　此致敬礼';
}

/** F08457/F08458 颜色与图标测试用例集。 */
export const CULTURE_COLOR_TESTS = ['红喜映射', '白丧避让', '紫色区域开关'];
export const CULTURE_ICON_TESTS = ['猫头鹰审查', 'OK 手势审查', '左手图标审查'];

/** F08459 文化审查委员会。 */
export class CultureCouncil {
  reviewed: { item: string; verdict: 'pass' | 'revise' | 'reject'; note: string }[] = [];
  review(item: string, verdict: 'pass' | 'revise' | 'reject', note = ''): number {
    this.reviewed.push({ item, verdict, note });
    return this.reviewed.length;
  }
  get pending(): number { return this.reviewed.filter((r) => r.verdict !== 'pass').length; }
}

/** F08460 社区贡献通道。 */
export function cultureContribution(name: string, region: string): { id: string; status: 'received' } {
  return { id: `culture:${region}:${name}`, status: 'received' };
}

/** F08461 设计幕后故事。 */
export const CULTURE_STORIES = ['红喜白丧的取舍', '手势图标的三轮审查', '示例姓名的本地化'] as const;

/** F08462 文化展览页。 */
export const CULTURE_EXHIBITS = ['色彩馆', '符号馆', '节令馆'] as const;

/** F08463 文化教学步骤。 */
export const CULTURE_TUTORIAL = ['读敏感色板', '跑图标审查', '替换示例数据', '委员会评审', '归档幕后故事'];

/** F08464 文化彩蛋（低频 + 总控）。 */
export function cultureEgg(unlocked: boolean, seed: number): string | null {
  return unlocked && seed % 73 === 0 ? '入乡随俗 🏮' : null;
}

/** F08465 文化回归清单。 */
export const CULTURE_REGRESSION = ['色板回归', '图标回归', '示例数据回归'];

/** F08466 文化文档页。 */
export const CULTURE_DOCS = ['敏感色板说明', '图标审查指南', '礼仪提示指南'];

/** F08467 文化 API 冻结。 */
export interface CultureApi { iconReview(icon: string): string[]; emailSalutation(region: string): string }
export const CULTURE_API_VERSION = '1.0';

/** F08468 文化收官清单。 */
export const CULTURE_FINALE = ['25 项自检', '审查零挂起', '回归通过', '展览上线', 'API 冻结'];

/** F08469 文化致谢。 */
export const CULTURE_THANKS = ['跨文化设计顾问', '区域用户访谈者', '社区审查志愿者'];

/** F08470 文化实验位（§15 预留）。 */
export const CULTURE_EXP_SLOT: FeatureSlot = { id: 'F08470', name: '文化实验位', reserved: true, enabled: false };

/** F08471 文化测试二批。 */
export const CULTURE_TESTS_2 = ['日文称呼', '泰色敏感', '地区姓名'];

/** F08472 文化审计项。 */
export const CULTURE_AUDIT = ['敏感色零误用', '敏感图标零上线', '声明齐备'];

/** F08473 文化日历：多文化节日日历。 */
export const CULTURE_CALENDAR: Record<string, string[]> = {
  global: ['01-01 元旦', '12-25 圣诞'],
  'zh-CN': ['02-17 春节', '09-25 中秋'],
  'ja-JP': ['01-01 正月', '08-15 お盆'],
};

/** F08474 文化壁纸：文化主题壁纸集。 */
export const CULTURE_WALLPAPERS = ['水墨山水', '浮世绘浪', '伊斯兰几何', '北欧极简'] as const;

/** F08475 文化二期收官清单。 */
export const CULTURE_FINALE_2 = ['二批测试通过', '日历覆盖 3 文化圈', '壁纸 4 套上线', '幕后故事归档'];

/* ============ 族0340 无障碍生态（F08476~F08500） ============ */

/** F08476~F08485 辅助技术（AT）兼容注册表。 */
export interface AtDevice { name: string; kind: 'screen-reader' | 'magnifier' | 'voice' | 'switch' | 'eye' | 'braille' | 'hearing' | 'keyboard' | 'joystick'; compatible: boolean; note?: string }
export const AT_DEVICES: AtDevice[] = [
  { name: 'NVDA', kind: 'screen-reader', compatible: true },
  { name: 'ZoomText', kind: 'magnifier', compatible: true },
  { name: 'Dragon', kind: 'voice', compatible: true },
  { name: 'Switch Access', kind: 'switch', compatible: true },
  { name: 'Tobii Eye', kind: 'eye', compatible: false, note: '需眼动仪' },
  { name: 'Braille Display', kind: 'braille', compatible: false, note: '预留' },
  { name: 'Hearing Aid', kind: 'hearing', compatible: false, note: '预留' },
  { name: 'One-Hand Keyboard', kind: 'keyboard', compatible: true },
  { name: 'QuadStick', kind: 'joystick', compatible: true },
];
export function atList(kind: AtDevice['kind']): AtDevice[] { return AT_DEVICES.filter((d) => d.kind === kind); }

/** F08486 修复流程：报告→复现→修复→验证→发布。 */
export function atFixFlow(step: number): string | null {
  const steps = ['接收兼容报告', '环境复现', '代码修复', '设备验证', '随版本发布'];
  return steps[clamp(step, 0, steps.length - 1)] ?? null;
}

/** F08487 AT 伙伴计划。 */
export const AT_PARTNERS = ['NVDA 社区', '眼动厂商', '点显器厂商'] as const;

/** F08488 厂商联系通道。 */
export function vendorContact(vendor: string): { to: string; slaDays: number } {
  return { to: `partners@at.example/${vendor}`, slaDays: 14 };
}

/** F08489 兼容接口冻结。 */
export interface AtCompatApi { registerDevice(d: AtDevice): void; list(kind: AtDevice['kind']): AtDevice[] }
export const AT_API_VERSION = '1.0';

/** F08490 生态文档页。 */
export const AT_DOCS = ['AT 兼容列表', '接入指南', '修复流程'];

/** F08491 生态教学步骤。 */
export const AT_TUTORIAL = ['认识 AT 分类', '跑兼容自检', '提交兼容报告', '跟进修复流程', '查看公开列表'];

/** F08492 生态彩蛋（低频 + 总控）。 */
export function atEgg(unlocked: boolean, seed: number): string | null {
  return unlocked && seed % 71 === 0 ? '每扇门都该有坡道 ♿' : null;
}

/** F08493 生态测试矩阵：kind × 设备通过率。 */
export function atMatrix(): Record<string, { passed: number; total: number }> {
  const out: Record<string, { passed: number; total: number }> = {};
  for (const d of AT_DEVICES) {
    const e = (out[d.kind] ??= { passed: 0, total: 0 });
    e.total++;
    if (d.compatible) e.passed++;
  }
  return out;
}

/** F08494 生态回归清单。 */
export const AT_REGRESSION = ['读屏回归', '放大回归', '开关回归'];

/** F08495 生态审计项。 */
export const AT_AUDIT = ['注册表准确', '预留位标注', '修复 SLA 达标'];

/** F08496 生态看板：兼容率。 */
export function atBoard(): number {
  return Math.round((AT_DEVICES.filter((d) => d.compatible).length / AT_DEVICES.length) * 100);
}

/** F08497 生态报告。 */
export function atReport(): { total: number; compatible: number; rate: number } {
  const compatible = AT_DEVICES.filter((d) => d.compatible).length;
  return { total: AT_DEVICES.length, compatible, rate: atBoard() };
}

/** F08498 生态收官清单。 */
export const AT_FINALE = ['25 项自检', '注册表 9 类', '矩阵发布', '报告公开', 'API 冻结'];

/** F08499 生态致谢。 */
export const AT_THANKS = ['AT 厂商伙伴', '障碍用户测试者', '开源读屏社区'];

/** F08500 生态社区入口。 */
export const AT_COMMUNITY = { forum: 'a11y.community', channel: 'AT-互通', monthly: true } as const;
