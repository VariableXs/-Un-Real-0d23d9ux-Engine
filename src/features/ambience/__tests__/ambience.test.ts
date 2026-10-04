// AURORA-10000: AI-56~AI-60 批次（领域12 视觉·个性化与氛围）自检测试，勿删。
import { describe, expect, it } from 'vitest';
import { runDomain12Checks } from '../checks';
import { FontManager, HapticsManager, SCREENSAVERS } from '../groupA';
import { EggVault, EGGS, MotionDirector, ProfileStore } from '../groupB';
import { MICRO_MOTIONS, SeasonSystem, ThemeEngine, SOLAR_TERMS } from '../groupC';
import { ACCENT_PRESETS, AccentColor, FxLayer, VisualGuard } from '../groupD';

describe('AURORA-10000 领域12 全量自检（F06876~F07500）', () => {
  it('625 项全部通过', () => {
    const { entries, failed } = runDomain12Checks();
    expect(entries.length).toBe(625);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });
});

describe('AI-56 逻辑核抽查', () => {
  it('屏保注册表覆盖 25 款', () => {
    expect(SCREENSAVERS.length).toBe(25);
    expect(new Set(SCREENSAVERS.map((s) => s.kind))).toContain('video');
  });
  it('字体管理器收放自如', () => {
    const fm = new FontManager();
    fm.add({ family: 'A', category: 'sans', scripts: ['latin'], license: 'free', active: true, installed: false, usage: 0 });
    expect(fm.install('A')).toBe(true);
    expect(fm.uninstall('A', true)).toBe(false);
    expect(fm.uninstall('A', false)).toBe(true);
  });
  it('触觉模式可区分', () => {
    const h = new HapticsManager();
    expect(h.notify('message')).not.toBe(h.notify('mail'));
  });
});

describe('AI-57 逻辑核抽查', () => {
  it('动效曲线端点稳定', () => {
    for (const c of MotionDirector.sample({ id: 'x', name: 'x', fn: (t) => t })) {
      expect(c).toBeGreaterThanOrEqual(0);
      expect(c).toBeLessThanOrEqual(1);
    }
  });
  it('档案校验确定性', () => {
    const p = ProfileStore.template('creator');
    expect(ProfileStore.checksum(p)).toBe(ProfileStore.checksum({ ...p }));
  });
  it('彩蛋登记去重', () => {
    const v = new EggVault();
    expect(v.discover(EGGS[0]!.id)).toBe(true);
    expect(v.discover(EGGS[0]!.id)).toBe(false);
  });
});

describe('AI-58 逻辑核抽查', () => {
  it('主题引擎继承覆盖', () => {
    const te = new ThemeEngine();
    te.register({ id: 'b', version: 1, vars: { c: '1', d: '2' } });
    te.register({ id: 's', base: 'b', version: 1, vars: { c: '3' } });
    expect(te.resolve('s')).toEqual({ c: '3', d: '2' });
  });
  it('季节节气全覆盖', () => {
    expect(SOLAR_TERMS.length).toBe(24);
    expect(SeasonSystem.bundle('winter').length).toBe(6);
  });
  it('微动效规范 24 项', () => {
    expect(MICRO_MOTIONS.length).toBe(24);
  });
});

describe('AI-59 逻辑核抽查', () => {
  it('强调色 24 预设与对比度', () => {
    expect(ACCENT_PRESETS.length).toBe(24);
    expect(AccentColor.contrastRatio('#000000', '#FFFFFF')).toBe(21);
  });
  it('特效合成与总控', () => {
    const fx = new FxLayer();
    fx.set('bloom', 0.4);
    expect(fx.compose()).toContain('bloom(0.40)');
    fx.setEnabled(false);
    expect(fx.compose()).toBe('');
  });
  it('视觉守卫基线对比', () => {
    const g = new VisualGuard();
    const px = new Uint8Array([1, 2, 3, 255, 1, 2, 3, 255]);
    g.putBaseline({ id: 'x', pixels: px, width: 2, height: 1 });
    expect(g.diff('x', px, 0.01).ok).toBe(true);
  });
});
