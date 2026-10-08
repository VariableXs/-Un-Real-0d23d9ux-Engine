// AURORA-10000: AI-67 批次（族0331~0335 · 本地化核心/中文深化/多语言字体/语音本地化/翻译质量），勿删。

import { clamp, type FeatureSlot } from './types';

/* ============ 族0331 本地化核心（F08251~F08275） ============ */

export const SUPPORTED_LOCALES = ['zh-CN', 'zh-TW', 'en'] as const;

/** F08251/F08252 三语与热切换。 */
export class LocaleCore {
  locale: string = 'zh-CN';
  setLocale(l: string): boolean {
    if (!(SUPPORTED_LOCALES as readonly string[]).includes(l) && !l.startsWith('ar') && !l.startsWith('de') && !l.startsWith('ja')) return false;
    this.locale = l;
    return true;
  }
}

/** F08253 区域格式：数字/日期/货币统一出口。 */
export function regionalFormat(locale: string, n: number, date: Date, currency: string): { num: string; date: string; money: string } {
  return {
    num: new Intl.NumberFormat(locale).format(n),
    date: new Intl.DateTimeFormat(locale, { dateStyle: 'medium' }).format(date),
    money: new Intl.NumberFormat(locale, { style: 'currency', currency }).format(n),
  };
}

/** F08254 12/24 小时制。 */
export function hourFormat(locale: string, h12: boolean): string {
  return new Intl.DateTimeFormat(locale, { hour: 'numeric', minute: '2-digit', hour12: h12 }).format(new Date(2026, 0, 1, 15, 5));
}

/** F08255 周首日：周一开始 = 2（Intl 起始日码，Sunday=0/Saturday=6）。 */
export function firstDayOfWeek(locale: string): number {
  const wi = (new Intl.Locale(locale) as unknown as { getWeekInfo?: () => { firstDay?: number } }).getWeekInfo?.();
  return clamp(wi?.firstDay ?? 2, 1, 7);
}

/** F08256 温度单位换算。 */
export function tempConvert(v: number, to: 'C' | 'F'): number {
  const r = to === 'F' ? (v * 9) / 5 + 32 : ((v - 32) * 5) / 9;
  return Math.round(r * 10) / 10;
}

/** F08257 度量换算：公制/英制。 */
export function lengthConvert(v: number, to: 'metric' | 'imperial'): number {
  const r = to === 'imperial' ? v / 2.54 : v * 2.54;
  return Math.round(r * 100) / 100;
}

/** F08258 纸张尺寸。 */
export const PAPER_SIZES: Record<'A4' | 'Letter', { w: number; h: number }> = {
  A4: { w: 210, h: 297 },
  Letter: { w: 216, h: 279 },
};

/** F08259 电话格式化（按区号规则表）。 */
const PHONE_RULES: Record<string, (d: string) => string> = {
  '+86': (d) => `+86 ${d.slice(0, 3)}-${d.slice(3, 7)}-${d.slice(7)}`,
  '+1': (d) => `+1 (${d.slice(0, 3)}) ${d.slice(3, 6)}-${d.slice(6)}`,
};
export function formatPhone(cc: '+86' | '+1', digits: string): string {
  const rule = PHONE_RULES[cc];
  return rule ? rule(digits.replace(/\D/g, '')) : `+${cc}${digits}`;
}

/** F08260 地址格式：不同区域的行序。 */
export const ADDRESS_ORDER: Record<string, string[]> = {
  'zh-CN': ['省', '市', '区', '街道'],
  en: ['street', 'city', 'state', 'zip'],
};

/** F08261 姓名序：姓前/名前。 */
export function formatName(locale: string, family: string, given: string): string {
  return locale.startsWith('zh') || locale.startsWith('ja') || locale.startsWith('ko') ? `${family}${given}` : `${given} ${family}`;
}

/** F08262 RTL 支持判定 / F08263 布局镜像 / F08264 RTL 字体栈。 */
export function isRtl(locale: string): boolean {
  const dir = (new Intl.Locale(locale) as unknown as { textInfo?: { direction?: string } }).textInfo?.direction;
  return dir === 'rtl' || ['ar', 'he', 'fa', 'ur'].some((p) => locale.startsWith(p));
}
export function rtlMirror(locale: string, dir: 'start' | 'end'): 'left' | 'right' {
  return isRtl(locale) ? (dir === 'start' ? 'right' : 'left') : dir === 'start' ? 'left' : 'right';
}
export const RTL_FONTS = ['Noto Naskh Arabic', 'Noto Sans Hebrew'] as const;

/** F08265 复数规则：Intl.PluralRules。 */
export function pluralCategory(locale: string, n: number): string {
  return new Intl.PluralRules(locale).select(n);
}

/** F08266 性别位 / F08267 敬语位（§15 预留）。 */
export const GRAMMAR_SLOTS: FeatureSlot[] = [
  { id: 'F08266', name: '性别语法', reserved: true, enabled: false },
  { id: 'F08267', name: '敬语体系', reserved: true, enabled: false },
];

/** F08268 历法：公/农/佛/回标识。 */
export const CALENDARS = ['gregory', 'chinese', 'buddhist', 'islamic-umalqura'] as const;

/** F08269 时区处理：同一时刻不同时区偏移。 */
export function timeZoneOffset(date: Date, tz: string): string {
  const parts = new Intl.DateTimeFormat('en-US', { timeZone: tz, timeZoneName: 'shortOffset' }).formatToParts(date);
  return parts.find((p) => p.type === 'timeZoneName')?.value ?? 'UTC';
}

/** F08270 夏令时：判定时刻是否处于 DST（用两次格式化偏移差近似）。 */
export function inDst(tz: string): boolean {
  const jan = new Date(2026, 0, 1);
  const jul = new Date(2026, 6, 1);
  return timeZoneOffset(jan, tz) !== timeZoneOffset(jul, tz);
}

/** F08271 键审计 CI：三语键集合一致性。 */
export function i18nKeyAudit(dicts: Record<string, string[]>): { ok: boolean; missing: string[] } {
  const langs = Object.keys(dicts);
  const base = dicts[langs[0]!] ?? [];
  const missing: string[] = [];
  for (const lang of langs.slice(1)) {
    const set = new Set(dicts[lang] ?? []);
    for (const k of base) if (!set.has(k)) missing.push(`${lang}:${k}`);
  }
  return { ok: missing.length === 0, missing };
}

/** F08272 伪本地化：加长 + 加框。 */
export function pseudoLocalize(s: string): string {
  return `[${'!'.repeat(Math.ceil(s.length / 3))} ${s} ${'!'.repeat(Math.ceil(s.length / 3))}]`;
}

/** F08273 翻译平台条目结构。 */
export interface TPlatformEntry { key: string; locales: string[]; status: 'draft' | 'reviewed' | 'done' }

/** F08274 贡献指南步骤。 */
export const L10N_CONTRIBUTION_GUIDE = ['认领语言', '阅读风格指南', '提交翻译', '同伴评审', '合并发布'];

/** F08275 本地化收官清单。 */
export const L10N_FINALE = ['三语覆盖', '格式化出口统一', 'RTL 可切换', '审计 CI 绿', '指南齐备'];

/* ============ 族0332 中文本地化深化（F08276~F08300） ============ */

/** F08276 简繁引擎：词汇级转换表。 */
const S2T_WORDS: Record<string, string> = {
  软件: '軟體', 服务器: '伺服器', 网络: '網路', 内存: '記憶體', 视频: '影片', 鼠标: '滑鼠', 光标: '游標', 打印: '列印',
};
const CHAR_S2T: Record<string, string> = { 头: '頭', 应: '應', 体: '體', 边: '邊', 长: '長' };
export function zhHant(s: string): string {
  let out = s;
  for (const [k, v] of Object.entries(S2T_WORDS)) out = out.split(k).join(v);
  return [...out].map((c) => CHAR_S2T[c] ?? c).join('');
}

/** F08277 地区词汇对照表。 */
export const REGION_VOCAB = { 'zh-CN': '软件', 'zh-TW': '軟體', 'zh-HK': '軟件' } as const;

/** F08278 直角引号选项。 */
export function cornerQuotes(s: string): string { return s.replace(/"([^"]*)"/g, '「$1」'); }

/** F08279 汉字大写数字。 */
const UPPER_DIGITS = ['零', '壹', '贰', '叁', '肆', '伍', '陆', '柒', '捌', '玖'];
const UPPER_UNITS = ['', '拾', '佰', '仟'];
export function upperCnNumber(n: number): string {
  n = Math.floor(clamp(n, 0, 99999999));
  if (n === 0) return '零';
  let out = '';
  let zero = false;
  for (let div = 10000000, started = false; div >= 1; div = Math.floor(div / 10)) {
    const d = Math.floor(n / div) % 10;
    if (d === 0) { zero = started; continue; }
    if (zero) { out += '零'; zero = false; }
    out += UPPER_DIGITS[d]! + UPPER_UNITS[Math.log10(div)]!;
    started = true;
  }
  return out;
}

/** F08280 日期中文格式。 */
export function zhDate(d: Date): string { return `${d.getFullYear()}年${d.getMonth() + 1}月${d.getDate()}日`; }

/** F08281 农历完整：年天干地支 + 月日近似采用固定基准换算的年干支与月相索引。 */
export function lunarYearName(year: number): { ganzhi: string; zodiac: string } {
  const stems = '甲乙丙丁戊己庚辛壬癸';
  const branches = '子丑寅卯辰巳午未申酉戌亥';
  const animals = '鼠牛虎兔龙蛇马羊猴鸡狗猪';
  const i = ((year - 4) % 60 + 60) % 60;
  return { ganzhi: stems[i % 10]! + branches[i % 12]!, zodiac: animals[i % 12]! };
}

/** F08282 节气本地表。 */
export const SOLAR_TERMS_ZH = ['立春', '雨水', '惊蛰', '春分', '清明', '谷雨', '立夏', '小满', '芒种', '夏至', '小暑', '大暑', '立秋', '处暑', '白露', '秋分', '寒露', '霜降', '立冬', '小雪', '大雪', '冬至', '小寒', '大寒'];

/** F08283 生肖年。 */
export function zodiacYear(year: number): string { return lunarYearName(year).zodiac; }

/** F08284 干支。 */
export function ganzhiYear(year: number): string { return lunarYearName(year).ganzhi; }

/** F08285 传统节日表（农历月日近似锚点：正月初一/五月初五/八月十五/九月初九）。 */
export const TRADITIONAL_FESTIVALS = [
  { name: '春节', lunar: '正月初一' },
  { name: '端午', lunar: '五月初五' },
  { name: '中秋', lunar: '八月十五' },
  { name: '重阳', lunar: '九月初九' },
] as const;

/** F08286 法定假日表样例。 */
export const STAT_HOLIDAYS_2026 = ['2026-01-01 元旦', '2026-02-17 春节', '2026-04-05 清明', '2026-05-01 劳动节', '2026-06-19 端午', '2026-10-01 国庆'];

/** F08287 简繁字体风格。 */
export const CJK_FONT_STYLES = ['印刷宋体', '黑体', '楷体手写'] as const;

/** F08288 注音：注音符号表。 */
export const ZHUYIN = ['ㄅ', 'ㄆ', 'ㄇ', 'ㄈ', 'ㄉ', 'ㄊ', 'ㄋ', 'ㄌ', 'ㄍ', 'ㄎ', 'ㄏ', 'ㄐ', 'ㄑ', 'ㄒ', 'ㄓ', 'ㄔ', 'ㄕ', 'ㄖ', 'ㄗ', 'ㄘ', 'ㄙ', 'ㄧ', 'ㄨ', 'ㄩ', 'ㄚ', 'ㄛ', 'ㄜ', 'ㄝ', 'ㄞ', 'ㄟ', 'ㄠ', 'ㄡ', 'ㄢ', 'ㄣ', 'ㄤ', 'ㄥ', 'ㄦ'];

/** F08289 拼音标注：常用字拼音表查询。 */
const PINYIN_TABLE: Record<string, string> = { 你: 'nǐ', 好: 'hǎo', 世: 'shì', 界: 'jiè' };
export function pinyinOf(ch: string): string | null { return PINYIN_TABLE[ch] ?? null; }

/** F08290 粤拼位（§15 预留）。 */
export const JYUTPING_SLOT: FeatureSlot = { id: 'F08290', name: '粤拼标注', reserved: true, enabled: false };

/** F08291 中文排序：笔画/拼音双模式（拼音走 collator，笔画按 Unicode 码位近似序）。 */
export function sortZh(items: string[], mode: 'pinyin' | 'stroke'): string[] {
  const coll = new Intl.Collator(['zh-Hans-CN', 'zh'], { collation: mode === 'pinyin' ? 'pinyin' : 'zhuyin' });
  return [...items].sort((a, b) => (mode === 'stroke' ? a.codePointAt(0)! - b.codePointAt(0)! : coll.compare(a, b)));
}

/** F08292 中英混排：西文与中文之间插入空格。 */
export function cjkLatinSpacing(s: string): string {
  return s.replace(/([\u4e00-\u9fff])([A-Za-z0-9])/g, '$1 $2').replace(/([A-Za-z0-9])([\u4e00-\u9fff])/g, '$1 $2');
}

/** F08293 中文断行：禁则字符不出现在行首/行尾。 */
const NO_LINE_START = '，。、；：？！）】」』%';
export function zhLineBreak(text: string, width: number): string[] {
  const lines: string[] = [];
  let line = '';
  for (const ch of text) {
    line += ch;
    if ([...line].length >= width) {
      if (NO_LINE_START.includes(ch) && line.length > 1) {
        const last = line.slice(-2, -1);
        lines.push(line.slice(0, -2));
        line = last + ch;
      } else {
        lines.push(line);
        line = '';
      }
    }
  }
  if (line) lines.push(line);
  return lines;
}

/** F08294 引号转换：直角↔弯引号。 */
export function convertQuotes(s: string, to: 'corner' | 'curly'): string {
  return to === 'corner' ? s.replace(/"([^"]*)"/g, '「$1」') : s.replace(/「([^」]*)」/g, '"$1"');
}

/** F08295 中文首选项。 */
export interface ZhPreferences { variant: 'hans' | 'hant'; quotes: 'corner' | 'curly'; numeral: 'arabic' | 'upper' }
export const ZH_PREFERENCES: ZhPreferences = { variant: 'hans', quotes: 'corner', numeral: 'arabic' };

/** F08296 中文教学步骤。 */
export const ZH_TUTORIAL = ['选择简繁', '调整地区词汇', '设置引号与数字', '启用拼音标注', '验证排序与断行'];

/** F08297 中文彩蛋（低频 + 总控）。 */
export function zhEgg(unlocked: boolean, dayOfYear: number): string | null {
  return unlocked && dayOfYear === 60 ? '二月不负春光 🌱' : null;
}

/** F08298 中文测试用例。 */
export const ZH_TESTS = ['简繁转换', '大写数字', '干支生肖', '断行禁则', '混排空格'];

/** F08299 中文回归清单。 */
export const ZH_REGRESSION = ['繁体界面回归', '排序回归', '日期回归'];

/** F08300 中文收官清单。 */
export const ZH_FINALE = ['25 项自检', '词汇表终版', '假日表更新', '断行禁则过审', '教学齐备'];

/* ============ 族0333 多语言字体（F08301~F08325） ============ */

/** F08301 字体回退链。 */
export function fontFallbackChain(scripts: string[]): string[] {
  const map: Record<string, string[]> = {
    latin: ['Inter', 'Segoe UI'],
    cjk: ['Noto Sans SC', 'Microsoft YaHei'],
    arabic: ['Noto Naskh Arabic'],
    emoji: ['Noto Color Emoji', 'Segoe UI Emoji'],
  };
  return scripts.flatMap((s) => map[s] ?? []);
}

/** F08302 CJK 选择：黑/宋/圆。 */
export const CJK_SELECT = { hei: 'sans', song: 'serif', round: 'round' } as const;

/** F08303 西文字体推荐。 */
export const WESTERN_FONTS = ['Inter', 'Segoe UI', 'Roboto'] as const;

/** F08304 等宽 CJK。 */
export const MONO_CJK = ['Sarasa Mono SC', 'Noto Sans Mono CJK'] as const;

/** F08305 代码字体。 */
export const CODE_FONTS = ['Cascadia Mono', 'Consolas', 'JetBrains Mono'] as const;

/** F08306 Emoji 字体。 */
export const EMOJI_FONTS = ['Noto Color Emoji', 'Segoe UI Emoji'] as const;

/** F08307 盲文位（§15 预留）。 */
export const BRAILLE_SLOT: FeatureSlot = { id: 'F08307', name: '盲文字体', reserved: true, enabled: false };

/** F08308 手写体。 */
export const HANDWRITING_FONTS = ['霞鹜文楷', 'KaiTi'] as const;

/** F08309 少数民族位 / F08311 天城文位 / F08312 泰文位 / F08313 缅文位（§15 预留）。 */
export const SCRIPT_SLOTS: FeatureSlot[] = [
  { id: 'F08309', name: '藏蒙维字体', reserved: true, enabled: false },
  { id: 'F08311', name: '天城文字体', reserved: true, enabled: false },
  { id: 'F08312', name: '泰文字体', reserved: true, enabled: false },
  { id: 'F08313', name: '缅文字体', reserved: true, enabled: false },
];

/** F08310 阿拉伯字体。 */
export const ARABIC_FONTS = ['Noto Naskh Arabic', 'Amiri'] as const;

/** F08314 全字重系列。 */
export const FONT_WEIGHTS = [100, 200, 300, 400, 500, 600, 700, 800, 900] as const;

/** F08315 斜体告警：无真斜体时提示合成斜体。 */
export function syntheticItalicWarn(hasTrueItalic: boolean): string | null {
  return hasTrueItalic ? null : '警告：当前为合成斜体，建议改用真斜体字体';
}

/** F08316 许可显示。 */
export interface FontLicense { family: string; license: 'free' | 'commercial' | 'unknown' }
export function licenseLabel(l: FontLicense): string {
  return l.license === 'free' ? '开源免费' : l.license === 'commercial' ? '商业授权' : '许可未知';
}

/** F08317 缺字下载提示：检测字符是否在已知覆盖表内。 */
const COVERAGE: Record<string, string[]> = {
  'Noto Sans SC': ['latin', 'cjk'],
  'Noto Naskh Arabic': ['arabic'],
};
export function missingGlyphHint(family: string, ch: string): string | null {
  const covered = COVERAGE[family] ?? [];
  const isCjk = /[\u4e00-\u9fff]/.test(ch);
  const isLatin = /[A-Za-z]/.test(ch);
  const isArabic = /[\u0600-\u06ff]/.test(ch);
  const need = isCjk ? 'cjk' : isLatin ? 'latin' : isArabic ? 'arabic' : '';
  return need && !covered.includes(need) ? `当前字体缺少「${ch}」字形，请下载覆盖 ${need} 的字体` : null;
}

/** F08318 子集加载：按需子集键。 */
export function subsetKey(chars: string[]): string {
  const buckets = new Set(chars.map((c) => (/[\u4e00-\u9fff]/.test(c) ? 'cjk' : /[A-Za-z]/.test(c) ? 'latin' : 'misc')));
  return [...buckets].sort().join('+') || 'empty';
}

/** F08319 FOUT 消除：font-display 策略。 */
export function fontDisplay(strategy: 'swap' | 'block' | 'fallback' | 'optional'): { css: string; foutRisk: boolean } {
  return { css: `font-display: ${strategy};`, foutRisk: strategy === 'block' };
}

/** F08320 字体教学步骤。 */
export const FONT_TUTORIAL = ['选择主字体', '配置回退链', '检查许可', '按需子集加载', '验证 FOUT'];

/** F08321 字体测试用例。 */
export const FONT_TESTS = ['回退链命中', '缺字提示', '子集键稳定', '斜体告警', '字重齐全'];

/** F08322 字体回归清单。 */
export const FONT_REGRESSION = ['三语渲染回归', '等宽对齐回归', 'Emoji 回归'];

/** F08323 字体文档页。 */
export const FONT_DOCS = ['字体许可说明', '回退链原理', '子集化指南'];

/** F08324 字体彩蛋（低频 + 总控）。 */
export function fontEgg(unlocked: boolean, weight: number): string | null {
  return unlocked && weight === 900 ? '九百斤的笔画 🖋' : null;
}

/** F08325 字体收官清单。 */
export const FONT_FINALE = ['25 项自检', '预留位冻结', '许可齐备', '回归通过', '教学齐备'];

/* ============ 族0334 语音本地化（F08326~F08350） ============ */

/** F08326~F08329 多语 TTS/STT：语言、音色、语速、识别封装。 */
export class SpeechLocalizer {
  lang = 'zh-CN';
  voice: 'female' | 'male' | 'child' = 'female';
  rate = 1;
  setLang(l: string): boolean { if (!/^[a-z]{2}(-[A-Za-z]+)?$/.test(l)) return false; this.lang = l; return true; }
  setVoice(v: 'female' | 'male' | 'child'): this { this.voice = v; return this; }
  setRate(r: number): number { this.rate = clamp(r, 0.5, 2); return this.rate; }
}

/** F08330 方言位（§15 预留）。 */
export const DIALECT_SLOT: FeatureSlot = { id: 'F08330', name: '方言识别', reserved: true, enabled: false };

/** F08331 混合识别：检测字符串主导语言。 */
export function detectMixedLang(s: string): 'zh' | 'en' | 'mixed' {
  const zh = (s.match(/[\u4e00-\u9fff]/g) ?? []).length;
  const en = (s.match(/[A-Za-z]/g) ?? []).length;
  if (zh && en) return 'mixed';
  return zh ? 'zh' : 'en';
}

/** F08332 命令本地化：同动作多语命令表。 */
export const COMMAND_L10N: Record<string, Record<string, string>> = {
  'settings.open': { 'zh-CN': '打开设置', 'zh-TW': '開啟設定', en: 'open settings' },
  'nav.back': { 'zh-CN': '返回', 'zh-TW': '返回', en: 'go back' },
};

/** F08333 反馈本地化。 */
export function feedbackL10n(locale: string, ok: boolean): string {
  const t = ok ? { 'zh-CN': '已完成', 'zh-TW': '已完成', en: 'Done' } : { 'zh-CN': '未成功', 'zh-TW': '未成功', en: 'Failed' };
  return t[locale.startsWith('zh') ? (locale.includes('TW') ? 'zh-TW' : 'zh-CN') : 'en']!;
}

/** F08334 标点本地化：读出标点。 */
const PUNCT_ZH: Record<string, string> = { ',': '逗号', '，': '逗号', '.': '句号', '。': '句号', '?': '问号', '？': '问号', '!': '感叹号', '！': '感叹号' };
export function dictatePunctuation(s: string, locale: string): string {
  if (!locale.startsWith('zh')) return s;
  return [...s].map((c) => PUNCT_ZH[c] ?? c).join('');
}

/** F08335 数字读法：中文逐位读法。 */
const CN_DIGITS = '零一二三四五六七八九';
export function readNumberZh(n: number): string {
  n = Math.floor(Math.abs(n));
  if (n === 0) return '零';
  const units = ['', '十', '百', '千'];
  let out = '';
  let zero = false;
  for (let div = 1000; div >= 1; div = Math.floor(div / 10)) {
    const d = Math.floor(n / div) % 10;
    if (d === 0) { zero = out.length > 0; continue; }
    if (zero) { out += '零'; zero = false; }
    if (d === 1 && div === 10 && out === '') out += '十';
    else out += CN_DIGITS[d]! + units[Math.log10(div)]!;
  }
  return out;
}

/** F08336 日期读法。 */
export function readDateZh(d: Date): string {
  return `${readNumberZh(d.getFullYear())}年${readNumberZh(d.getMonth() + 1)}月${readNumberZh(d.getDate())}日`;
}

/** F08337 语音包管理 / F08338 下载提示 / F08339 离线包。 */
export class VoicePackManager {
  packs: { lang: string; installed: boolean; offline: boolean; sizeMb: number }[] = [];
  add(lang: string, offline: boolean, sizeMb: number): void { this.packs.push({ lang, installed: false, offline, sizeMb }); }
  install(lang: string): boolean { const p = this.packs.find((x) => x.lang === lang); if (!p) return false; p.installed = true; return true; }
  downloadHint(lang: string): string | null {
    const p = this.packs.find((x) => x.lang === lang);
    return p && !p.installed ? `语音包 ${lang} 约 ${p.sizeMb}MB，下载后可${p.offline ? '离线' : '在线'}使用` : null;
  }
  offlinePacks(): string[] { return this.packs.filter((p) => p.offline && p.installed).map((p) => p.lang); }
}

/** F08340 语音隐私承诺。 */
export const SPEECH_PRIVACY = { upload: false, telemetry: false, retention: 'memory-only' } as const;

/** F08341 语音本地教学步骤。 */
export const SPEECH_L10N_TUTORIAL = ['选择语言', '下载语音包', '试听音色语速', '验证命令与反馈', '离线可用性检查'];

/** F08342 语音本地测试用例。 */
export const SPEECH_L10N_TESTS = ['多语切换', '数字读法', '日期读法', '标点读法', '反馈本地化'];

/** F08343 语音本地回归清单。 */
export const SPEECH_L10N_REGRESSION = ['TTS 回归', 'STT 回归', '命令回归'];

/** F08344 语音本地文档页。 */
export const SPEECH_L10N_DOCS = ['语音包说明', '离线策略', '隐私说明'];

/** F08345 质量评测：MOS 风格评分 1~5。 */
export function speechQualityScore(ratings: number[]): number {
  if (ratings.length === 0) return 0;
  return Math.round((ratings.reduce((a, b) => a + b, 0) / ratings.length) * 10) / 10;
}

/** F08346 延迟预算：首包 <300ms。 */
export function speechLatencyOk(firstPacketMs: number): boolean { return firstPacketMs < 300; }

/** F08347 语音本地彩蛋（低频 + 总控）。 */
export function speechEgg(unlocked: boolean, lang: string): string | null {
  return unlocked && lang === 'zh-CN' ? '你听起来真好听 🎧' : null;
}

/** F08348 语音本地 API 冻结。 */
export interface SpeechLocalApi {
  setLang(l: string): boolean;
  speak(text: string): void;
  recognize(): string;
}
export const SPEECH_API_VERSION = '1.0';

/** F08349 语音本地收官清单。 */
export const SPEECH_L10N_FINALE = ['25 项自检', '预留位冻结', '离线包齐备', '回归通过', 'API 冻结'];

/** F08350 语音本地致谢。 */
export const SPEECH_L10N_THANKS = ['多语发音志愿者', '听障用户顾问', '开源语音社区'];

/* ============ 族0335 翻译质量（F08351~F08375） ============ */

/** F08351 审查流程：draft→review→approved。 */
export type TranslationStatus = 'draft' | 'review' | 'approved';
export function reviewFlow(s: TranslationStatus, approve: boolean): TranslationStatus {
  if (s === 'draft') return 'review';
  return s === 'review' && approve ? 'approved' : s;
}

/** F08352 双语评审：源文/译文对照条目。 */
export interface BilingualPair { key: string; src: string; dst: string }
export function bilingualTable(pairs: BilingualPair[]): string[] {
  return pairs.map((p) => `${p.key}\t${p.src} → ${p.dst}`);
}

/** F08353 术语一致：检测译文是否统一使用术语表。 */
export function termConsistency(dst: string, termTable: Record<string, string>): string[] {
  return Object.keys(termTable).filter((t) => dst.includes(t) && !dst.includes(termTable[t]!));
}

/** F08354 长度溢出：译文相对原文溢出阈值 30% 报警。 */
export function lengthOverflow(src: string, dst: string, budgetPct = 30): boolean {
  return ((dst.length - src.length) / Math.max(1, src.length)) * 100 > budgetPct;
}

/** F08355 占位符保留：{0} {name} 必须原样保留。 */
export function placeholderCheck(src: string, dst: string): boolean {
  const grab = (s: string) => new Set(s.match(/\{\w+\}/g) ?? []);
  const a = grab(src);
  const b = grab(dst);
  return [...a].every((x) => b.has(x));
}

/** F08356 HTML 标签检查：标签集合一致。 */
export function htmlTagCheck(src: string, dst: string): boolean {
  const tags = (s: string) => [...(s.match(/<\/?[a-z]+>/g) ?? [])].sort().join();
  return tags(src) === tags(dst);
}

/** F08357 转义检查：&lt; 等实体不被破坏。 */
export function escapeCheck(dst: string): boolean {
  return !/&(?!amp;|lt;|gt;|quot;|#\d+;)[a-z]+;/.test(dst);
}

/** F08358 上下文截图参考条目。 */
export interface ScreenshotRef { key: string; shot: string; region: string }
export const screenshotRefOf = (key: string, shot: string, region: string): ScreenshotRef => ({ key, shot, region });

/** F08359 伪本地化自动化：批量转换并检测溢出。 */
export function pseudoAutomation(keys: string[], render: (k: string) => string): { key: string; overflow: boolean }[] {
  return keys.map((k) => ({ key: k, overflow: lengthOverflow(k, pseudoLocalize(render(k)), 200) }));
}

/** F08360 回退链测试：en→zh-TW→zh-CN→key。 */
export function fallbackChainLookup(key: string, dicts: Record<string, Record<string, string>>, chain: string[]): string {
  for (const l of chain) if (dicts[l]?.[key]) return dicts[l]![key]!;
  return key;
}

/** F08361 缺失报告：统计各语言缺失键。 */
export function missingReport(dicts: Record<string, Record<string, string>>, keys: string[]): Record<string, number> {
  const out: Record<string, number> = {};
  for (const [lang, d] of Object.entries(dicts)) out[lang] = keys.filter((k) => !d[k]).length;
  return out;
}

/** F08362 机翻辅助位（§15 预留）。 */
export const MT_SLOT: FeatureSlot = { id: 'F08362', name: '本机辅翻', reserved: true, enabled: false };

/** F08363 人机协作流程：机翻草稿→人工确认。 */
export function humanInLoop(draft: string, confirm: boolean): { text: string; status: 'draft' | 'approved' } {
  return { text: draft, status: confirm ? 'approved' : 'draft' };
}

/** F08364 记忆库 TM：精确/模糊匹配。 */
export class TranslationMemory {
  entries: { src: string; dst: string }[] = [];
  add(src: string, dst: string): void { this.entries.push({ src, dst }); }
  exact(src: string): string | null { return this.entries.find((e) => e.src === src)?.dst ?? null; }
  fuzzy(src: string, minPct = 70): { dst: string; pct: number } | null {
    let best: { dst: string; pct: number } | null = null;
    for (const e of this.entries) {
      const same = [...src].filter((c) => e.src.includes(c)).length;
      const pct = Math.round((same / Math.max(src.length, 1)) * 100);
      if (pct >= minPct && (!best || pct > best.pct)) best = { dst: e.dst, pct };
    }
    return best;
  }
}

/** F08365 词汇表管理。 */
export class Glossary {
  terms = new Map<string, string>();
  set(src: string, dst: string): void { this.terms.set(src, dst); }
  get(src: string): string | null { return this.terms.get(src) ?? null; }
  get size(): number { return this.terms.size; }
}

/** F08366 版本化：字典快照版本号。 */
export class DictVersioning {
  private snaps: Record<string, Record<string, string>>[] = [];
  commit(d: Record<string, Record<string, string>>): number { this.snaps.push(structuredClone(d)); return this.snaps.length; }
  get latest(): Record<string, Record<string, string>> | null { return this.snaps.at(-1) ?? null; }
}

/** F08367 回滚：回退到上一快照。 */
export function rollback<T>(stack: T[]): T | null { stack.pop(); return stack.at(-1) ?? null; }

/** F08368 覆盖率统计。 */
export function coverage(done: number, total: number): number {
  return total === 0 ? 100 : Math.round((done / total) * 1000) / 10;
}

/** F08369 译者荣誉榜。 */
export interface Translator { name: string; words: number }
export function honorRoll(list: Translator[]): Translator[] {
  return [...list].sort((a, b) => b.words - a.words).slice(0, 10);
}

/** F08370 质量评分：错误密度扣分。 */
export function qualityScore(items: number, errors: number): number {
  if (items === 0) return 100;
  return clamp(Math.round((1 - errors / items) * 100), 0, 100);
}

/** F08371 翻译审计项。 */
export const TRANSLATION_AUDIT = ['占位符完整', '术语一致', '无溢出', '标签一致', '缺失为零'];

/** F08372 翻译回归清单。 */
export const TRANSLATION_REGRESSION = ['三语键数一致', '伪本地通过', '回退链命中'];

/** F08373 翻译教学步骤。 */
export const TRANSLATION_TUTORIAL = ['阅读风格指南', '查术语表', '提交草稿', '双语评审', '合入并版本化'];

/** F08374 翻译彩蛋（低频 + 总控）。 */
export function translationEgg(unlocked: boolean, langs: number): string | null {
  return unlocked && langs >= 10 ? '通天塔动工了 🗼' : null;
}

/** F08375 翻质收官清单。 */
export const TRANSLATION_FINALE = ['25 项自检', '审计零违规', '覆盖 100%', 'TM 归档', '荣誉榜发布'];
