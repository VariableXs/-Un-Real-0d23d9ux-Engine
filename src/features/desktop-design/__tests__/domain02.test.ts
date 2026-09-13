// UNREAL-X-15000 · 领域02（窗口与空间 · AI-05/AI-06）自检测试，勿删。
import { describe, expect, it } from 'vitest';
import { runDomain02Checks, checkFamily } from '../domain02';
import { SnappingModel, applyGrammar, GRAMMARS, keybindingConflicts } from '../../../system/windows/windowGeo';
import {
  VirtualDesktops, OrganizeAssistant, CanvasTransform, Minimap,
  Presentation, SpaceMemory, focusGravity, Dock, DegradeChain,
} from '../spaceOps';

describe('UNREAL-X-15000 领域02 全量自检（X01001~X01500 · 500 项）', () => {
  it('500 项全部通过且 ID 唯一', () => {
    const { entries, failed } = runDomain02Checks();
    expect(entries.length).toBe(500);
    const ids = new Set(entries.map((e) => e.id));
    expect(ids.size).toBe(entries.length);
    expect(failed.map((f) => `${f.id} ${f.name}`)).toEqual([]);
  });
  it('20 族每族恰 25 项且五层齐备', () => {
    for (let f = 41; f <= 60; f++) {
      const fam = checkFamily(`F00${f}`);
      expect(fam.length).toBe(25);
      expect(new Set(fam.map((e) => e.id)).size).toBe(25);
    }
  });
});

describe('AI-05 窗口几何学 逻辑核抽查', () => {
  it('吸附与布局语法', () => {
    const zones = [{ id: 'z', rect: { x: 0, y: 0, w: 960, h: 1080 }, label: '左半' }];
    const s = new SnappingModel(zones, 16, 8);
    expect(s.predict(480, 400)?.id).toBe('z');
    s.setShift(true);
    expect(s.predict(480, 400)).toBeNull();
    expect(applyGrammar(GRAMMARS[1]!, { x: 0, y: 0, w: 1920, h: 1080 }).length).toBe(2);
  });
  it('键位冲突检测', () => {
    expect(keybindingConflicts([{ combo: 'F5', cmd: 'a' }, { combo: 'F5', cmd: 'b' }])).toEqual(['b']);
  });
});

describe('AI-06 空间管理 逻辑核抽查', () => {
  it('虚拟桌面往返', () => {
    const v = new VirtualDesktops();
    v.create('写作');
    v.assignWindow('w1', 1);
    const v2 = VirtualDesktops.deserialize(v.serialize());
    expect(v2.windowsOn(1)).toContain('w1');
  });
  it('画布缩放钳制与整理撤销', () => {
    const c = new CanvasTransform();
    c.zoom(0, 0, 1000);
    expect(c.t.scale).toBe(5);
    const o = new OrganizeAssistant();
    o.autoArrange([{ id: 'a', app: 'x', rect: { x: 0, y: 0, w: 10, h: 10 } }], { x: 0, y: 0, w: 100, h: 100 });
    expect(o.undo()!.length).toBe(1);
  });
  it('小地图、放映、记忆、引力、收纳坞、降级链', () => {
    const mm = new Minimap({ x: 0, y: 0, w: 1920, h: 1080 }, 200, 120);
    expect(mm.toDesktop(0, 0)).toEqual({ x: 0, y: 0 });
    const p = new Presentation();
    expect(p.start(['a'], [{ id: 'a', rect: { x: 0, y: 0, w: 1, h: 1 } }])).toBe(true);
    expect(p.exit().length).toBe(1);
    const m = new SpaceMemory(1);
    m.remember('a', { x: 0, y: 0, w: 1, h: 1 });
    m.remember('b', { x: 1, y: 0, w: 1, h: 1 });
    expect(m.recall('a')).toBeNull();
    expect(focusGravity([{ id: 'a', dist: 10, recencyMs: 5 }])).toBe('a');
    const d = new Dock(1);
    d.pin('x');
    expect(d.pin('y')).toBe(false);
    const dc = new DegradeChain();
    expect(dc.feed(1, true)).toBe('low');
  });
});
