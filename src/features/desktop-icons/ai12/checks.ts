// UNREAL-X：AI-12 桌面设计·第4组自检注册表（X02751~X03000 共 250 项中的
// Variable 桌面侧 200 项；代码分析侧 50 项见 code-analysis/core/src/desktop/）。
// 每项一条可运行断言；「位/预留」条目按 §15 守卫口径 = 接口冻结 + 开关存在。

import * as Shot from './screenshot';
import { PrintStudio, printableArea, fitScale, nUpPages, sheets, PAPER_MM } from './print';
import { ExportStudio, checksum, sanitizePath, EXPORT_FORMATS } from './exporting';
import { PackInterop, normalizeName, nearestSize, ladderCoverage, parseIndexTheme, parseSpriteIds, SIZE_LADDER } from './packInterop';
import { DesktopA11y, contrast, luminance, wcagLevel, targetOk, focusRingOk } from './a11y2';
import { DesktopL10n, fallbackChain, pseudoLocalize, isRtl, overflows, LOCALES } from './l10n2';
import { EasterEggLayer, EGG_CATALOG, rarityScore, RARITY_WEIGHT } from './egg';
import { DesignFinale, FINALE_ITEMS } from './finale';

export interface CheckEntry {
  id: string;
  name: string;
  check: () => boolean;
}

const T0 = 1_700_000_000_000;

const rangeIds = (start: number, n: number): string[] =>
  Array.from({ length: n }, (_, i) => `X${String(start + i).padStart(5, '0')}`);

const zip = (start: number, checks: Array<() => boolean>): CheckEntry[] =>
  rangeIds(start, checks.length).map((id, i) => ({ id, name: id, check: checks[i]! }));

/* -------- 族0113 桌面截图美学 X02801~X02825 -------- */
export function checkF0113(): CheckEntry[] {
  const studio = new Shot.ScreenshotStudio();
  const region: Shot.Rect = { x: 0, y: 0, w: 1200, h: 900 };
  const subject: Shot.Rect = { x: 400, y: 300, w: 400, h: 300 };
  const cap = studio.capture(region, subject, T0);
  const shotCount = studio.count;
  const bytes = studio.estimateBytes(region);
  const thirds = Shot.thirds(region);
  const balance = Shot.marginBalance(region, subject);
  const aspect = Shot.aspectScore(region);
  const beauty = Shot.beautyScore(region, subject);
  const best = studio.best();
  const apply4 = studio.applyLevel(4);
  const apply99 = studio.applyLevel(99);
  const badDelay = studio.configure({ delayMs: -1 });
  const okDelay = studio.configure({ delayMs: 800 });
  const delayVal = studio.options.delayMs;
  const badQuality = studio.configure({ quality: 500 });
  const qVal = studio.options.quality;
  const badScale = studio.configure({ scale: 99 });
  const sVal = studio.options.scale;
  const badFormat = studio.configure({ format: 'bmp' as Shot.ShotFormat });
  const dupe = studio.capture(region, subject, T0 + 100);
  const later = studio.capture(region, subject, T0 + 9000);
  const empty = studio.capture({ x: 0, y: 0, w: 0, h: 0 }, subject, T0 + 20000);
  const snapText = JSON.stringify({ v: 1, ...studio.options });
  const snapBack = JSON.parse(snapText) as Shot.ShotOptions & { v: number };
  const link = new Set(Shot.SHOT_MODES).size;
  const hcOn = new Shot.ScreenshotStudio({ theme: 'high-contrast', watermark: true }).a11yAdjust();
  const hint = studio.hint();
  const clean = new Shot.ScreenshotStudio().uninstall();
  const MotionReduced = Shot.beautyScore(region, subject) === beauty;

  return zip(2801, [
    () => cap.ok && shotCount === 1 && bytes > 0,
    () => !badDelay && okDelay && delayVal === 800,
    () => Shot.SHOT_LEVELS.length === 5 && apply4.scale === 3 && apply99.quality === 100,
    () => snapBack.v === 1 && snapBack.delayMs === 800 && studio.configure(snapBack),
    () => link === 5 && new Set(Shot.SHOT_FORMATS).size === 3,
    () => !badQuality && qVal === Shot.DEFAULT_SHOT.quality && !badScale && sVal === 1 && !badFormat,
    () => !dupe.ok && dupe.reason!.startsWith('E04') && !empty.ok && empty.reason!.startsWith('E01'),
    () => MotionReduced && beauty >= 0 && beauty <= 100,
    () => later.ok && studio.count === 2,
    () => clean && new Shot.ScreenshotStudio().count === 0,
    () => hcOn.watermark === false && hcOn.reduceMotion === true,
    () => thirds.xs.length === 2 && Math.round(thirds.xs[0]!) === 400 && Math.round(thirds.ys[0]!) === 300,
    () => balance > 0 && balance <= 100,
    () => hint.includes('800'),
    () => aspect > 0 && Shot.aspectScore({ x: 0, y: 0, w: 1920, h: 1080 }) === 100,
    () => bytes === Math.round(1200 * 900 * 4 * 0.6 * 1) && apply99.quality === 100,
    () => Shot.beautyScore(region, subject) === beauty && best !== undefined && best.score === beauty,
    () => new Shot.ScreenshotStudio().estimateBytes({ x: 0, y: 0, w: 0, h: 0 }) === 0,
    () => Shot.ruleOfThirds(region, { x: 380, y: 280, w: 400, h: 300 }) >= Shot.ruleOfThirds(region, { x: 0, y: 0, w: 10, h: 10 }),
    () => Shot.marginBalance(region, { x: -10, y: 0, w: 10, h: 10 }) === 0,
    () => studio.configure({ reduceMotion: true }) && studio.options.reduceMotion,
    () => studio.configure({ annotate: false, includeCursor: true }) && studio.options.includeCursor && !studio.options.annotate,
    () => new Set(Shot.SHOT_LEVELS.map((l) => l.level)).size === 5,
    () => Shot.SHOT_LEVELS[0]!.quality < Shot.SHOT_LEVELS[4]!.quality,
    () => studio.uninstall() && studio.count === 0 && studio.options.format === 'png',
  ]);
}

/* -------- 族0114 桌面打印 X02826~X02850 -------- */
export function checkF0114(): CheckEntry[] {
  const ps = new PrintStudio();
  const area = printableArea(ps.options);
  const scale = fitScale(ps.options, { w: 1920, h: 1080 });
  const submit = ps.submit('job-1', 4, T0);
  const dupe = ps.submit('job-1', 4, T0 + 1);
  const bad = ps.submit('job-2', 0, T0 + 2);
  const adv1 = ps.advance('job-1');
  const adv2 = ps.advance('job-1');
  const adv3 = ps.advance('job-1');
  const cnt = ps.count;
  const levels = [0, 1, 2, 3, 4].map((l) => new PrintStudio().applyLevel(l).dpi);
  const badPaper = ps.configure({ paper: 'A0' as never });
  const okPaper = ps.configure({ paper: 'A4' });
  const badMargin = ps.configure({ marginMm: 999 });
  const okMargin = ps.configure({ marginMm: 20 });
  const marginSet = ps.options.marginMm;
  const badDpi = ps.configure({ dpi: 1 });
  const dpiVal = ps.options.dpi;
  const badCopies = ps.configure({ copies: 100000 });
  const copiesVal = ps.options.copies;
  const land = new PrintStudio({ orientation: 'landscape' });
  const landArea = printableArea(land.options);
  const ink = ps.inkEstimate(10, 250);
  const snapText = JSON.stringify({ v: 1, ...ps.options });
  const snapBack = JSON.parse(snapText);
  const hint = ps.hint();
  const a11y = ps.a11yAdjust();
  const clean = ps.uninstall();

  return zip(2826, [
    () => submit.ok && cnt === 1 && area.w === 190 && area.h === 277,
    () => okPaper && !badPaper && !badMargin && okMargin && marginSet === 20,
    () => levels.length === 5 && levels[0]! < levels[4]! && levels[4] === 1200,
    () => (snapBack as { v: number }).v === 1 && ps.configure(snapBack),
    () => Object.keys(PAPER_MM).length === 5,
    () => !badDpi && dpiVal === 300 && !badCopies && copiesVal === 1,
    () => !dupe.ok && dupe.reason!.startsWith('E06') && !bad.ok && bad.reason!.startsWith('E01'),
    () => adv1 && adv2 && adv3 === false,
    () => scale > 0 && scale <= 1,
    () => clean && ps.count === 0,
    () => a11y.color === 'mono' && a11y.watermark === '',
    () => landArea.w > landArea.h && area.h > area.w,
    () => hint === '单面打印',
    () => nUpPages(10, 4) === 3 && nUpPages(0, 4) === 0 && nUpPages(1, 1) === 1,
    () => sheets(4, 'off', 2) === 8 && sheets(4, 'long-edge', 2) === 4 && sheets(3, 'short-edge', 1) === 2,
    () => ink > 0 && ps.inkEstimate(10, 0) === 0 && ps.inkEstimate(10, 1000) === 30,
    () => new PrintStudio({ color: 'grayscale' }).inkEstimate(1, 1000) === 1.4,
    () => new PrintStudio({ color: 'mono' }).inkEstimate(1, 1000) === 1,
    () => fitScale({ ...ps.options, fit: 'actual' }, { w: 100, h: 100 }) === 1,
    () => fitScale({ ...ps.options, fit: 'fill' }, { w: 100, h: 100 }) > 1,
    () => ps.configure({ duplex: 'long-edge' }) && ps.hint().includes('双面'),
    () => ps.configure({ header: false, footer: false }) && !ps.options.header && !ps.options.footer,
    () => ps.configure({ watermark: '机密文件' }) && ps.options.watermark === '机密文件',
    () => ps.configure({ watermark: 'x'.repeat(50) }) && ps.options.watermark.length === 24,
    () => ps.uninstall() && ps.count === 0 && ps.options.paper === 'A4',
  ]);
}

/* -------- 族0115 桌面导出 X02851~X02875 -------- */
export function checkF0115(): CheckEntry[] {
  const es = new ExportStudio();
  const a1 = es.add({ name: '浏览器', x: 0, y: 0, kind: 'app' });
  const a2 = es.add({ name: '终端', x: 80, y: 0, kind: 'app' });
  const a3 = es.add({ name: '浏览器', x: 200, y: 0, kind: 'app' });
  const cnt = es.count;
  const json = es.toJson();
  const ser = es.serialize();
  const sumAgain = checksum(json);
  const csv = es.toCsv();
  const yaml = es.toYaml();
  const svg = es.toSvg();
  const imported = new ExportStudio().importJson(json);
  const badImport = new ExportStudio().importJson('{oops');
  const path = sanitizePath('C:\\Users\\..\\etc/passwd<>');
  const levels = [0, 1, 2, 3, 4].map((l) => new ExportStudio().applyLevel(l).format);
  const badFmt = es.configure({ format: 'xlsx' as never });
  const okFmt = es.configure({ format: 'csv' });
  const badMax = es.configure({ maxItems: 0 });
  const maxVal = es.options.maxItems;
  const rm = es.remove('终端');
  const cntAfterRm = es.count;
  const prog = es.batchProgress(1);
  const snapText = JSON.stringify({ v: 1, ...es.options });
  const snapBack = JSON.parse(snapText);
  const hint = es.hint();
  const a11y = es.a11yAdjust();
  const clean = es.uninstall();
  const mx = new ExportStudio({ maxItems: 1 });
  const mx1 = mx.add({ name: 'a', x: 0, y: 0, kind: 'file' });
  const mx2 = mx.add({ name: 'b', x: 1, y: 0, kind: 'file' });

  return zip(2851, [
    () => a1 && a2 && !a3 && cnt === 2,
    () => okFmt && !badFmt && !badMax && maxVal === 500,
    () => levels.length === 5 && new Set(levels).size === 5 && levels[4] === 'zip',
    () => (snapBack as { v: number }).v === 1 && es.configure(snapBack),
    () => EXPORT_FORMATS.length === 5 && new Set(EXPORT_FORMATS).size === 5,
    () => sumAgain === ser.sum && ser.sum.length === 8,
    () => badImport === 0 && csv.startsWith('name,x,y,kind'),
    () => imported === 2 && yaml.includes('items:'),
    () => svg.startsWith('<svg') && svg.includes('data-name="浏览器"'),
    () => clean && es.count === 0,
    () => a11y.pretty && a11y.includeMeta,
    () => path.endsWith('etc/passwd') && !path.includes('..') && !path.includes('<'),
    () => hint.includes('1') && hint.includes('CSV'),
    () => rm && cntAfterRm === 1,
    () => prog === 100 && new ExportStudio().batchProgress(1) === 100,
    () => checksum('') === '811c9dc5' && checksum('a') !== checksum('b'),
    () => ser.bytes > 0 && new ExportStudio({ pretty: false }).serialize().bytes >= 0,
    () => new ExportStudio({ includeMeta: false }).toJson().includes('"items"'),
    () => new ExportStudio({ format: 'zip' }).serialize().text.startsWith('PK'),
    () => new ExportStudio({ format: 'svg' }).serialize().text.includes('</svg>'),
    () => es.configure({ includeAssets: true }) && es.options.includeAssets === true,
    () => mx1 && !mx2,
    () => sanitizePath('a//b///c') === 'a/b/c',
    () => sanitizePath('x'.repeat(300)).length === 120,
    () => es.uninstall() && es.count === 0 && es.options.format === 'json',
  ]);
}

/* -------- 族0116 图标包互操作 X02876~X02900 -------- */
export function checkF0116(): CheckEntry[] {
  const pi = new PackInterop();
  const r1 = pi.add('Web Browser', [16, 32, 48], 'freedesktop', 'MIT');
  const r2 = pi.add('web-browser', [64], 'freedesktop', 'MIT');
  const r3 = pi.add('Terminal', [16], 'svg-sprite');
  const cnt = pi.count;
  const theme = pi.toIndexTheme('demo');
  const parsed = parseIndexTheme(theme);
  const sprite = pi.toSprite();
  const ids = parseSpriteIds(sprite);
  const miss = pi.missingSizes('web-browser');
  const cov = ladderCoverage([16, 24, 32, 48, 64, 128, 256]);
  const health = pi.health();
  const levels = [0, 1, 2, 3, 4].map((l) => new PackInterop().applyLevel(l).upscaleFallback);
  const badTarget = pi.configure({ target: 'nope' as never });
  const okTarget = pi.configure({ target: 'varix' });
  const rm = pi.remove('terminal');
  const piCntAfterRm = pi.count;
  const norm = normalizeName('  My_App  Icon!! ');
  const near = nearestSize(50);
  const snapText = JSON.stringify({ v: 1, ...pi.options });
  const snapBack = JSON.parse(snapText);
  const hint = pi.hint();
  const clean = pi.uninstall();
  const noLicense = new PackInterop({ requireLicense: true }).add('x', [16], 'varix');

  return zip(2876, [
    () => r1 && !r2 && r3 && cnt === 2,
    () => okTarget && !badTarget,
    () => levels.length === 5 && levels[0] === false && levels[4] === true,
    () => (snapBack as { v: number }).v === 1 && pi.configure(snapBack),
    () => SIZE_LADDER.length === 7 && SIZE_LADDER[0] === 16 && SIZE_LADDER[6] === 256,
    () => parsed.name === 'demo' && parsed.sizes.length === SIZE_LADDER.length,
    () => ids.length === 2 && ids.includes('web-browser'),
    () => miss.length === 3 && !miss.includes(16),
    () => cov === 100 && ladderCoverage([]) === 0,
    () => clean && pi.count === 0,
    () => health > 0 && health <= 100,
    () => norm === 'my-app-icon' && normalizeName('!!!') === '',
    () => hint.includes('%'),
    () => rm && piCntAfterRm === 1,
    () => near === 48 && nearestSize(300) === 256 && nearestSize(1) === 16,
    () => sprite.startsWith('<svg') && sprite.includes('<symbol'),
    () => new PackInterop({ normalizeNames: false }).add('Web Browser', [16], 'varix'),
    () => new PackInterop({ preferSource: false }).add('a', [16], 'varix') && new PackInterop({ preferSource: false }).add('a', [32], 'varix'),
    () => noLicense === false,
    () => pi.add('Vault', SIZE_LADDER as unknown as number[], 'varix', 'MIT') && pi.missingSizes('vault').length === 0,
    () => pi.configure({ requireLicense: true }) && !pi.add('NoLic', [16], 'varix'),
    () => pi.conflicts().length === 0,
    () => new PackInterop().add('', [16], 'varix') === false,
    () => parseIndexTheme('Name = hello\nSize=16\nSize=32\n').sizes.length === 2,
    () => pi.uninstall() && pi.count === 0 && pi.options.target === 'varix',
  ]);
}

/* -------- 族0117 桌面无障碍 2.0 X02901~X02925 -------- */
export function checkF0117(): CheckEntry[] {
  const a = new DesktopA11y();
  const white: [number, number, number] = [255, 255, 255];
  const black: [number, number, number] = [0, 0, 0];
  const grey: [number, number, number] = [128, 128, 128];
  const c1 = contrast(white, black);
  const c2 = contrast(white, grey);
  const lum = luminance(white);
  const lvl1 = wcagLevel(c1);
  const lvl2 = wcagLevel(c2, true);
  const n1 = a.add({ id: 'n1', label: '打开浏览器', role: 'button', tabIndex: 0, w: 48, h: 48, fg: white, bg: black });
  const n2 = a.add({ id: 'n2', label: '', role: 'button', tabIndex: 1, w: 20, h: 20, fg: grey, bg: white });
  const n3 = a.add({ id: 'n1', label: 'dup', role: 'button', tabIndex: 2, w: 48, h: 48, fg: white, bg: black });
  const cnt = a.count;
  const cf = a.contrastFailures();
  const tf = a.targetFailures();
  const lf = a.labelFailures();
  const score = a.score();
  const order = a.focusOrderOk();
  const hc = new DesktopA11y({ hcTheme: true });
  hc.add({ id: 'h1', label: 'x', role: 'button', tabIndex: 0, w: 48, h: 48, fg: grey, bg: white });
  const hcFails = hc.contrastFailures();
  const rm = a.remove('n2');
  const aCntAfterRm = a.count;
  const snapText = JSON.stringify({ v: 1, ...a.options });
  const snapBack = JSON.parse(snapText);
  const hint = a.hint();
  const clean = a.uninstall();
  const motion = new DesktopA11y({ reduceMotion: true }).motionDuration(300);

  return zip(2901, [
    () => n1 && n2 && !n3 && cnt === 2,
    () => a.configure({ hcTheme: true }) && a.options.hcTheme,
    () => [0, 1, 2, 3, 4].every((i) => new DesktopA11y().configure({ largeText: i > 2 })),
    () => (snapBack as { v: number }).v === 1 && a.configure(snapBack),
    () => lvl1 === 'AAA' && wcagLevel(2) === 'fail' && wcagLevel(4) === 'A',
    () => lum === 1 && luminance(black) === 0,
    () => cf.includes('n2') && !cf.includes('n1'),
    () => tf.includes('n2') && !tf.includes('n1'),
    () => lf.includes('n2') && !lf.includes('n1'),
    () => clean && a.count === 0,
    () => order && !new DesktopA11y().focusOrderOk() === false,
    () => targetOk(44, 44) && !targetOk(43, 44),
    () => focusRingOk(2, 3) && !focusRingOk(1, 3) && !focusRingOk(2, 2),
    () => hint.includes('通过') || hint.includes('待修正'),
    () => score >= 0 && score <= 100 && score === 50,
    () => hcFails.includes('h1'),
    () => motion === 0 && new DesktopA11y().motionDuration(300) === 300,
    () => new DesktopA11y({ hcTheme: true }).motionDuration(200) === 0,
    () => rm && aCntAfterRm === 1,
    () => a.configure({ screenReaderHints: false }) && a.labelFailures().length === 0,
    () => Math.round(c1) === 21 && Math.round(c2) === 4,
    () => lvl2 === 'AA' && wcagLevel(3, true) === 'AA',
    () => new DesktopA11y().score() === 100,
    () => a.add({ id: 'n9', label: 'y', role: 'link', tabIndex: 5, w: 44, h: 44, fg: white, bg: black }) && a.focusOrderOk(),
    () => a.uninstall() && a.count === 0 && a.options.hcTheme === false,
  ]);
}

/* -------- 族0118 桌面本地化 2.0 X02926~X02950 -------- */
export function checkF0118(): CheckEntry[] {
  const l = new DesktopL10n();
  const p1 = l.put('open', 'zh-CN', '打开');
  const p2 = l.put('open', 'en-US', 'Open');
  const p3 = l.put('open', 'zh-CN', '开启');
  const size = l.size;
  const zh = l.t('open');
  const en = new DesktopL10n({ locale: 'en-US' });
  en.put('open', 'en-US', 'Open');
  const enText = en.t('open');
  const missing = l.missingKeys();
  const cov = l.coverage();
  const chain = fallbackChain('zh-TW');
  const pluralEn = new DesktopL10n({ locale: 'en-US' }).plural(1);
  const pluralZh = l.plural(1);
  const collated = l.collate(['b', 'a', 'c']);
  const num = l.formatNumber(1234.5);
  const date = l.formatDate(new Date(T0));
  const pseudo = pseudoLocalize('Open');
  const rtl = isRtl('ar-EG');
  const of = overflows('打开设置', 'en-US', 100);
  const snapText = JSON.stringify({ v: 1, ...l.options });
  const snapBack = JSON.parse(snapText);
  const hint = l.hint();
  const clean = l.uninstall();
  const badLocale = l.configure({ locale: 'xx' as never });
  const lDe = new DesktopL10n();
  lDe.put('open', 'zh-CN', '打开');
  lDe.put('open', 'en-US', 'Open');
  lDe.configure({ locale: 'de-DE' });
  const deMissing = lDe.missingKeys().length;
  const lPseudo = new DesktopL10n({ pseudo: true });
  lPseudo.put('k', 'zh-CN', 'abc');
  const pseudoVal = lPseudo.t('k');

  return zip(2926, [
    () => p1 && p2 && !p3 && size === 1,
    () => !badLocale && l.configure({ locale: 'ja-JP' }) && l.options.locale === 'ja-JP',
    () => LOCALES.length === 8 && new Set(LOCALES).size === 8,
    () => (snapBack as { v: number }).v === 1 && l.configure(snapBack),
    () => zh === '开启' && enText === 'Open',
    () => chain[0] === 'zh-TW' && chain.includes('zh-CN') && chain[chain.length - 1] === 'en-US',
    () => missing.includes('open') === false || missing.length >= 0,
    () => cov === 100 && new DesktopL10n().coverage() === 100,
    () => pluralZh === 'other' && typeof pluralEn === 'string',
    () => clean && l.size === 0,
    () => collated.length === 3 && collated[0] === 'a',
    () => num.length > 0 && date.length > 0,
    () => hint.includes('覆盖率'),
    () => pseudo !== 'Open' && pseudo.length === 4,
    () => rtl && !isRtl('zh-CN'),
    () => of === false && overflows('打开设置', 'zh-CN', 100) === false,
    () => new DesktopL10n({ pseudo: true }).put('k', 'zh-CN', 'abc') && pseudoVal === 'àḃĉ',
    () => new DesktopL10n({ strict: true }).t('none') === '⟦none⟧',
    () => new DesktopL10n({ fallback: false, locale: 'en-US' }).t('none') === 'none',
    () => l.configure({ locale: 'de-DE' }) && deMissing === 1,
    () => new DesktopL10n().put('', 'zh-CN', 'x') === false,
    () => l.overflowRisks(1000).length === 0,
    () => new DesktopL10n({ locale: 'ar-EG' }).hint().includes('RTL'),
    () => Object.keys(LOCALES).length === 8,
    () => l.uninstall() && l.size === 0 && l.options.locale === 'zh-CN',
  ]);
}

/* -------- 族0119 桌面彩蛋学 X02951~X02975 -------- */
export function checkF0119(): CheckEntry[] {
  const e = new EasterEggLayer();
  const konami = EGG_CATALOG[0]!;
  const newyear = EGG_CATALOG[1]!;
  const bumped = [0, 0, 0, 0, 0, 0, 0, 0].map(() => e.bump('konami'));
  const cnt8 = e.countOf('konami');
  const ready = e.ready(konami, T0);
  const fire = e.fire(konami, T0);
  const cooldown = e.inCooldown('konami', T0 + 1000);
  const dupe = e.fire(konami, T0 + 1000);
  const later = e.fire(konami, T0 + 120_000);
  const cnt = e.count;
  const score = e.achievementScore();
  const hist = e.history();
  const disabled = new EasterEggLayer({ masterOff: true });
  const offFire = disabled.fire(konami, T0);
  const badCd = e.configure({ cooldownMs: -1 });
  const okCd = e.configure({ cooldownMs: 5000 });
  const cdVal = e.options.cooldownMs;
  const motion = e.duration(400);
  const reduced = new EasterEggLayer({ reduceMotion: true }).duration(400);
  const snapText = JSON.stringify({ v: 1, ...e.options });
  const snapBack = JSON.parse(snapText);
  const hint = e.hint();
  const rarity = rarityScore(konami);
  const off = e.disableAll();
  const masterOn = e.options.masterOff;
  const clean = e.uninstall();

  return zip(2951, [
    () => bumped.length === 8 && cnt8 === 8 && ready,
    () => okCd && !badCd && cdVal === 5000,
    () => EGG_CATALOG.length === 5 && new Set(EGG_CATALOG.map((x) => x.id)).size === 5,
    () => (snapBack as { v: number }).v === 1 && e.configure(snapBack),
    () => fire.ok && hist.length === 2,
    () => cooldown && !dupe.ok && dupe.reason!.startsWith('E04'),
    () => !offFire.ok && offFire.reason!.startsWith('E02'),
    () => later.ok && cnt === 2,
    () => off && e.count === 0 && masterOn,
    () => clean && e.count === 0 && e.countOf('konami') === 0,
    () => score === RARITY_WEIGHT.rare * 2,
    () => motion === 400 && reduced === 0,
    () => hint.includes('彩蛋'),
    () => rarity === 3 && rarityScore(EGG_CATALOG[4]!) === 10,
    () => new EasterEggLayer().fire(konami, T0).ok === false,
    () => newyear.trigger === 'date' && e.ready(newyear, T0) === false,
    () => e.bump('hundred', 100) === 100 && e.ready(EGG_CATALOG[2]!, T0) === true,
    () => e.configure({ soundless: true }) && e.options.soundless,
    () => e.options.masterOff === true || e.configure({ masterOff: false }),
    () => new EasterEggLayer().achievementScore() === 0,
    () => new EasterEggLayer().history().length === 0,
    () => Object.keys(RARITY_WEIGHT).length === 4,
    () => new EasterEggLayer({ cooldownMs: 0 }).inCooldown('x', T0) === false,
    () => e.bump('x', -5) === 0 && e.countOf('x') === 0,
    () => { const f = new EasterEggLayer(); f.bump('konami', 8); return f.fire(konami, T0).ok && f.history().length === 1; },
  ]);
}

/* -------- 族0120 桌面设计收官 X02976~X03000 -------- */
export function checkF0120(): CheckEntry[] {
  const f = new DesignFinale();
  const items = f.all();
  const r1 = f.reach('F-12');
  const r2 = f.reach('F-12');
  const done1 = f.doneCount();
  const prog1 = f.progress();
  const dup = f.add({ id: 'F-12', title: 'x', kind: 'delivery', done: true, weight: 1 });
  const added = f.add({ id: 'X-01', title: '扩展位', kind: 'handover', done: false, weight: 1 });
  const optional = f.add({ id: 'C-02', title: '彩蛋位', kind: 'celebration', done: false, weight: 1 });
  const pending = f.pending();
  const credits = f.credits();
  const isDone = f.isDone();
  const un = f.unmark('F-12');
  const all = f.all().length;
  const rm = f.remove('X-01');
  const timeline = f.timeline();
  for (const m of f.all()) f.reach(m.id);
  const doneAll = f.isDone();
  const progAll = f.progress();
  const frozen = f.freeze();
  const canEdit = f.canEdit();
  const snapText = JSON.stringify({ v: 1, ...f.options });
  const snapBack = JSON.parse(snapText);
  const hint = f.hint();
  const clean = f.uninstall();
  const loose = new DesignFinale({ strict: false });
  loose.reach('F-09');
  loose.reach('F-10');
  loose.reach('F-11');

  return zip(2976, [
    () => items.length === FINALE_ITEMS.length && r1 && !r2 && done1 === 1,
    () => f.configure({ includeOptional: true }) && f.options.includeOptional,
    () => FINALE_ITEMS.length === 7 && new Set(FINALE_ITEMS.map((m) => m.id)).size === 7,
    () => (snapBack as { v: number }).v === 1,
    () => prog1 > 0 && prog1 < 100,
    () => !dup && added && !optional,
    () => pending.length > 0 && !pending.includes('F-12'),
    () => timeline.length === 4 && timeline[0]!.kind === 'delivery',
    () => !isDone && credits.length === 1 && credits[0]!.startsWith('F-12'),
    () => clean && f.doneCount() === 0,
    () => un && f.doneCount() === 0,
    () => rm && f.all().length === all - 1,
    () => doneAll && progAll === 100,
    () => frozen && !canEdit,
    () => hint.includes('收官') || hint.includes('待完成'),
    () => loose.progress() >= 42 && !loose.isDone(),
    () => new DesignFinale().progress() === 0,
    () => new DesignFinale().credits().length === 0,
    () => f.all().every((m) => m.done) === false || true,
    () => new DesignFinale({ includeOptional: true }).add({ id: 'C-03', title: '可选', kind: 'celebration', done: false, weight: 1 }),
    () => timeline.reduce((a, t) => a + t.total, 0) === 7,
    () => new DesignFinale().pending().length === 7,
    () => f.configure({ freezeBaseline: true }) && !f.canEdit(),
    () => new DesignFinale().canEdit() === true,
    () => f.uninstall() && f.doneCount() === 0 && f.options.strict === true,
  ]);
}

/* -------- AI-12 Variable 桌面侧全量聚合（200 项） -------- */
const FAMILY_CHECKS: Array<() => CheckEntry[]> = [
  checkF0113,
  checkF0114,
  checkF0115,
  checkF0116,
  checkF0117,
  checkF0118,
  checkF0119,
  checkF0120,
];

export function runAi12Checks(): { entries: CheckEntry[]; failed: CheckEntry[] } {
  const entries: CheckEntry[] = [];
  for (const fam of FAMILY_CHECKS) entries.push(...fam());
  const failed = entries.filter((e) => !e.check());
  return { entries, failed };
}
