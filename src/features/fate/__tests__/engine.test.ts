import { describe, expect, it } from "vitest";
import {
  collectTraits, defaultParams, drillDown, emptyProfile, layoutTree, makeRng,
  simulate, traitMultiplier, treeStats,
} from "../engine";
import {
  PRESET_CHARACTERS, PRESET_EVENTS, PRESET_IDEOLOGIES, PRESET_PERSONALITIES, PRESET_RANDOMS,
} from "../dictionaries";
import type { RoleProfile } from "../types";

function profileWith(): RoleProfile {
  const p = emptyProfile();
  p.name = "测试者";
  p.personalities = [{ id: PRESET_PERSONALITIES[0]!.id, weight: 70 }];
  p.characters = [
    { id: PRESET_CHARACTERS[0]!.id, value: 80 },
    { id: PRESET_CHARACTERS[1]!.id, value: -60 },
  ];
  p.ideologies = [{ id: PRESET_IDEOLOGIES[0]!.id, value: 55 }];
  return p;
}

describe("FTPE engine (chapter 9)", () => {
  it("dictionary seeds are substantial and tag-linked", () => {
    expect(PRESET_PERSONALITIES.length).toBeGreaterThanOrEqual(90);
    expect(PRESET_CHARACTERS.length).toBeGreaterThanOrEqual(95);
    expect(PRESET_IDEOLOGIES.length).toBeGreaterThanOrEqual(90);
    expect(PRESET_EVENTS.length).toBeGreaterThanOrEqual(300);
    expect(PRESET_RANDOMS.length).toBeGreaterThanOrEqual(15);
    // 模块三：id 全局唯一（扩容批次不得与基础库冲突）
    const ids = new Set<string>();
    for (const e of PRESET_EVENTS) {
      expect(ids.has(e.id)).toBe(false);
      ids.add(e.id);
    }
    expect(PRESET_RANDOMS.length).toBeGreaterThanOrEqual(8);
    for (const e of PRESET_EVENTS) {
      expect(e.tags.length).toBeGreaterThan(0);
      expect(e.base).toBeGreaterThan(0);
    }
  });

  it("slider values change event probabilities precisely (spec 1.3)", () => {
    const brave = PRESET_CHARACTERS.find((c) => c.name === "勇敢") ?? PRESET_CHARACTERS[0]!;
    const tag = brave.effects[0]!.tag;
    const low = traitMultiplier([{ effects: brave.effects, value: -100 }], tag);
    const high = traitMultiplier([{ effects: brave.effects, value: 100 }], tag);
    expect(high).toBeGreaterThan(low);
  });

  it("simulate produces a deterministic multi-branch fate tree", () => {
    const p = profileWith();
    const params = { ...defaultParams(), years: 6, branching: 2, seed: 42 };
    const traits = collectTraits(p, PRESET_PERSONALITIES, PRESET_CHARACTERS, PRESET_IDEOLOGIES);
    const a = simulate(p, params, PRESET_EVENTS, traits, 1, 0);
    const b = simulate(p, params, PRESET_EVENTS, traits, 1, 0);
    expect(a.children.length).toBeGreaterThan(0);
    // 同种子完全可重现（13.1）
    expect(JSON.stringify(a)).toBe(JSON.stringify(b));
    const stats = treeStats(a);
    expect(stats.count).toBeGreaterThan(5);
    expect(stats.endings.length).toBeGreaterThan(0);
  });

  it("layout assigns ordered leaves and parents sit between children", () => {
    const p = profileWith();
    const params = { ...defaultParams(), years: 4, branching: 2, seed: 7 };
    const traits = collectTraits(p, PRESET_PERSONALITIES, PRESET_CHARACTERS, PRESET_IDEOLOGIES);
    const root = simulate(p, params, PRESET_EVENTS, traits, 1, 0);
    const pos = layoutTree(root);
    expect(pos.size).toBe(treeStats(root).count);
    for (const c of root.children) {
      expect(pos.get(c.id)!.x).toBeGreaterThan(pos.get(root.id)!.x);
    }
  });

  it("drillDown regenerates a subtree as reverse feedback onto the trunk (ch.10)", () => {
    const p = profileWith();
    const params = { ...defaultParams(), years: 4, branching: 2, seed: 11 };
    const traits = collectTraits(p, PRESET_PERSONALITIES, PRESET_CHARACTERS, PRESET_IDEOLOGIES);
    const root = simulate(p, params, PRESET_EVENTS, traits, 1, 0);
    const target = root.children[0]!;
    const before = JSON.stringify(target.children);
    const sub = drillDown(target, params, PRESET_EVENTS, traits, 1, 0, 3, 3);
    expect(sub.children.length).toBeGreaterThan(0);
    expect(sub.drilled).toBe(true);
    target.children = sub.children;
    expect(JSON.stringify(target.children)).not.toBe(before);
  });

  it("makeRng is reproducible", () => {
    const a = makeRng(9);
    const b = makeRng(9);
    expect(a()).toBe(b());
    expect(a()).toBe(b());
  });
});
