// UNREAL-X-15000: AI-10 批次（领域03 壁纸与微件 · 族0091~0100）自检测试，勿删。
import { describe, expect, it } from 'vitest';
import { runAi10Checks } from '../checks';
import { WallpaperEngine, WallpaperPalette, WallpaperLibrary, WallpaperWorkshop } from '../groupA';
import { WallpaperPhysics } from '../groupB';
import { LockscreenIntegration, DeskTimeAmbience } from '../groupC';
import { WidgetFramework, WidgetCollection, WidgetInteractionLayer, WIDGET_CATALOG } from '../../../system/widgets/groupA';

describe('UNREAL-X-15000 AI-10 全量自检（X02251~X02500）', () => {
  it('250 项全部通过', () => {
    const { entries, failed } = runAi10Checks();
    expect(entries.length).toBe(250);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });
});

describe('AI-10 壁纸与微件核', () => {
  it('壁纸引擎五模式与轮播', () => {
    const e = new WallpaperEngine();
    e.setPlaylist(['a', 'b']);
    e.next();
    expect(e.current()).toBe('b');
    e.setBatterySaver(true);
    e.setMode('shader');
    expect(e.effectiveMode()).toBe('static');
  });
  it('取色：L/C 钳制与语义映射', () => {
    const p = new WallpaperPalette();
    const colors = p.extract([[255, 0, 0], [0, 0, 255]]);
    expect(colors).toHaveLength(5);
    expect(colors.every((c) => c.l >= 0.55 && c.l <= 0.72 && c.c <= 0.13)).toBe(true);
    expect(Object.keys(WallpaperPalette.semantic(colors))).toEqual(['accent', 'success', 'warn', 'danger']);
  });
  it('壁纸库去重登记', () => {
    const lib = new WallpaperLibrary();
    lib.add({ id: 'a', name: 'a', tags: [], favorite: false, bytes: 1 });
    expect(lib.add({ id: 'a', name: 'a', tags: [], favorite: false, bytes: 1 })).toBe('dup');
  });
  it('工坊参数护栏', () => {
    expect(WallpaperWorkshop.sanitize({ hue: -80 }).hue).toBe(280);
    expect(WallpaperWorkshop.generate({ kind: 'gradient', hue: 0, layers: 99, seed: 0 }).stops).toHaveLength(8);
  });
  it('动态物理低配降级', () => {
    const p = new WallpaperPhysics();
    p.setTier('storm');
    p.setLowPower(true);
    expect(p.params().particles).toBe(65);
    expect(WallpaperPhysics.degradeChain('storm')).toHaveLength(5);
  });
  it('微件框架配额与生命周期', () => {
    const fw = new WidgetFramework();
    fw.setMaxMounted(1);
    fw.register({ id: 'a', name: 'a', size: 'small', refreshMs: 500 });
    fw.register({ id: 'b', name: 'b', size: 'small', refreshMs: 500 });
    expect(fw.mount('a')).toBe(true);
    expect(fw.mount('b')).toBe(false);
  });
  it('微件集刷新节流', () => {
    const c = new WidgetCollection();
    c.update('clock', 'a', 0);
    expect(c.update('clock', 'b', 100)).toBe('throttled');
    expect(c.update('clock', 'b', 300)).toBe('fresh');
    expect(WIDGET_CATALOG).toHaveLength(8);
  });
  it('互动层命中与展开', () => {
    const il = new WidgetInteractionLayer();
    il.layout([{ x: 0, y: 0, w: 100, h: 100, widgetId: 'w' }]);
    expect(il.hitTest(50, 50)).toBe('w');
    il.interact('expand', 'w');
    expect(il.expanded()).toBe('w');
  });
  it('锁屏与时空感', () => {
    const lk = new LockscreenIntegration();
    lk.lock(1);
    expect(lk.isLocked()).toBe(true);
    expect(DeskTimeAmbience.phaseOf(23)).toBe('night');
    const ta = new DeskTimeAmbience();
    ta.setNightShift(true);
    expect(ta.effectiveKelvin(23)).toBe(1800);
  });
});
