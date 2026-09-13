// UNREAL-X：AI-12 桌面设计·第4组（族0113~0120）Variable 桌面侧自检测试，勿删。
import { describe, expect, it } from 'vitest';
import { runAi12Checks } from '../checks';
import {
  ScreenshotStudio,
  beautyScore,
  ruleOfThirds,
  marginBalance,
  aspectScore,
  thirds,
  SHOT_LEVELS,
} from '../screenshot';
import { PrintStudio, printableArea, fitScale, nUpPages, sheets } from '../print';
import { ExportStudio, checksum, sanitizePath } from '../exporting';
import { PackInterop, normalizeName, nearestSize, ladderCoverage, parseIndexTheme } from '../packInterop';
import { DesktopA11y, contrast, luminance, wcagLevel, targetOk } from '../a11y2';
import { DesktopL10n, fallbackChain, pseudoLocalize, isRtl } from '../l10n2';
import { EasterEggLayer, EGG_CATALOG, rarityScore } from '../egg';
import { DesignFinale, FINALE_ITEMS } from '../finale';

describe('UNREAL-X AI-12 桌面设计第4组全量自检（X02801~X03000）', () => {
  it('200 项全部通过', () => {
    const { entries, failed } = runAi12Checks();
    expect(entries.length).toBe(200);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(entries[0]!.id).toBe('X02801');
    expect(entries[199]!.id).toBe('X03000');
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });
});

describe('AI-12 族0113 截图美学', () => {
  it('构图评分与档位矩阵', () => {
    const region = { x: 0, y: 0, w: 1200, h: 900 };
    expect(thirds(region).xs).toEqual([400, 800]);
    expect(ruleOfThirds(region, { x: 370, y: 270, w: 60, h: 60 })).toBeGreaterThan(80);
    expect(marginBalance(region, { x: 500, y: 400, w: 200, h: 100 })).toBeGreaterThanOrEqual(0);
    expect(aspectScore({ x: 0, y: 0, w: 1600, h: 900 })).toBeGreaterThan(90);
    expect(beautyScore(region, { x: 400, y: 300, w: 400, h: 300 })).toBeGreaterThan(0);
    expect(SHOT_LEVELS.length).toBe(5);
    const st = new ScreenshotStudio();
    expect(st.applyLevel(2).scale).toBe(1);
    expect(st.capture({ x: 0, y: 0, w: 400, h: 300 }, { x: 100, y: 100, w: 200, h: 150 }, 0).ok).toBe(true);
  });
});

describe('AI-12 族0114 桌面打印', () => {
  it('页面编排与耗材', () => {
    const ps = new PrintStudio();
    expect(printableArea(ps.options)).toEqual({ w: 190, h: 277 });
    expect(fitScale(ps.options, { w: 1920, h: 1080 })).toBeGreaterThan(0);
    expect(nUpPages(9, 4)).toBe(3);
    expect(sheets(5, 'long-edge', 1)).toBe(3);
    expect(ps.submit('a', 2, 0).ok).toBe(true);
    expect(ps.submit('a', 2, 1).ok).toBe(false);
  });
});

describe('AI-12 族0115 桌面导出', () => {
  it('序列化与校验', () => {
    const es = new ExportStudio();
    es.add({ name: 'a', x: 0, y: 0, kind: 'app' });
    expect(es.serialize().sum).toBe(checksum(es.toJson()));
    expect(sanitizePath('a/../../b')).toBe('a/b');
    expect(new ExportStudio().importJson(es.toJson())).toBe(1);
  });
});

describe('AI-12 族0116 图标包互操作', () => {
  it('命名归一与阶梯', () => {
    expect(normalizeName('My App!')).toBe('my-app');
    expect(nearestSize(40)).toBe(32);
    expect(ladderCoverage([16, 32])).toBe(29);
    const pi = new PackInterop();
    pi.add('Web Browser', [16, 32], 'freedesktop', 'MIT');
    expect(pi.count).toBe(1);
    expect(parseIndexTheme(pi.toIndexTheme('t')).name).toBe('t');
  });
});

describe('AI-12 族0117 桌面无障碍 2.0', () => {
  it('真实对比度与等级', () => {
    const white: [number, number, number] = [255, 255, 255];
    const black: [number, number, number] = [0, 0, 0];
    expect(luminance(white)).toBe(1);
    expect(Math.round(contrast(white, black))).toBe(21);
    expect(wcagLevel(21)).toBe('AAA');
    expect(targetOk(44, 44)).toBe(true);
    const a = new DesktopA11y();
    a.add({ id: 'x', label: '打开', role: 'button', tabIndex: 0, w: 44, h: 44, fg: white, bg: black });
    expect(a.contrastFailures()).toEqual([]);
    expect(a.score()).toBe(100);
  });
});

describe('AI-12 族0118 桌面本地化 2.0', () => {
  it('回退链与伪本地化', () => {
    expect(fallbackChain('zh-TW')).toEqual(['zh-TW', 'zh-CN', 'en-US']);
    expect(pseudoLocalize('Open')).not.toBe('Open');
    expect(isRtl('ar-EG')).toBe(true);
    const l = new DesktopL10n();
    l.put('k', 'zh-CN', '值');
    expect(l.t('k')).toBe('值');
    expect(l.coverage()).toBe(100);
  });
});

describe('AI-12 族0119 桌面彩蛋学', () => {
  it('触发、冷却与总开关', () => {
    const e = new EasterEggLayer();
    const def = EGG_CATALOG[0]!;
    e.bump(def.id, 8);
    expect(e.fire(def, 0).ok).toBe(true);
    expect(e.fire(def, 100).ok).toBe(false);
    expect(rarityScore(def)).toBe(3);
    expect(e.disableAll()).toBe(true);
  });
});

describe('AI-12 族0120 桌面设计收官', () => {
  it('里程碑与加权完成度', () => {
    const f = new DesignFinale();
    expect(f.all().length).toBe(FINALE_ITEMS.length);
    expect(f.reach('F-12')).toBe(true);
    expect(f.reach('F-12')).toBe(false);
    for (const m of f.all()) f.reach(m.id);
    expect(f.isDone()).toBe(true);
    expect(f.progress()).toBe(100);
  });
});
