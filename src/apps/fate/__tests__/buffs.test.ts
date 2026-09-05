import { describe, expect, it } from "vitest";
import { makeBuffFn, simulate, collectTraits, defaultParams, emptyProfile } from "../engine";
import {
  PRESET_BUFFS, PRESET_CHARACTERS, PRESET_EVENTS, PRESET_IDEOLOGIES, PRESET_PERSONALITIES,
} from "../dictionaries";
import type { RoleProfile } from "../types";

function profileWith(): RoleProfile {
  const p = emptyProfile();
  p.name = "buff测试者";
  p.personalities = [{ id: PRESET_PERSONALITIES[0]!.id, weight: 70 }];
  p.characters = [{ id: PRESET_CHARACTERS[0]!.id, value: 60 }];
  p.ideologies = [{ id: PRESET_IDEOLOGIES[0]!.id, value: 40 }];
  return p;
}

describe("FTPE buffs (模块三·第四/五轮)", () => {
  it("buff dictionary is substantial and well-formed", () => {
    expect(PRESET_BUFFS.length).toBeGreaterThanOrEqual(28);
    const ids = new Set<string>();
    const names = new Set<string>();
    for (const b of PRESET_BUFFS) {
      expect(ids.has(b.id)).toBe(false);
      ids.add(b.id);
      expect(names.has(b.name)).toBe(false);
      names.add(b.name);
      expect(Object.keys(b.boost).length).toBeGreaterThan(0);
      for (const m of Object.values(b.boost)) {
        expect(m as number).toBeGreaterThanOrEqual(0.5);
        expect(m as number).toBeLessThanOrEqual(2);
      }
    }
  });

  it("behavior tree dictionaries stay duplicate-free across batches", () => {
    for (const list of [PRESET_PERSONALITIES, PRESET_CHARACTERS, PRESET_IDEOLOGIES]) {
      const names = new Set<string>();
      for (const e of list) {
        expect(names.has(e.name)).toBe(false);
        names.add(e.name);
      }
    }
  });

  it("makeBuffFn stacks multipliers and clamps extremes", () => {
    const on = PRESET_BUFFS.filter((b) => ["b01", "b02"].includes(b.id)).map((b) => ({ ...b, enabled: true }));
    expect(makeBuffFn([])("社交")).toBe(1);
    expect(makeBuffFn(on)("社交")).toBeGreaterThan(1);
    expect(makeBuffFn(on)("机缘")).toBeGreaterThan(1);
    // 未命中的标签保持 1
    expect(makeBuffFn(on)("感情")).toBe(1);
    // 12 个极端 buff 叠乘也会被钳制，不会掀桌
    const all = PRESET_BUFFS.map((b) => ({ ...b, enabled: true }));
    for (const tag of ["冒险", "社交", "事业", "健康"]) {
      const m = makeBuffFn(all)(tag);
      expect(m).toBeGreaterThan(0.05);
      expect(m).toBeLessThan(4);
    }
  });

  it("enabled buffs change the tree deterministically", () => {
    const p = profileWith();
    const params = { ...defaultParams(), years: 6, branching: 2, seed: 2024 };
    const traits = collectTraits(p, PRESET_PERSONALITIES, PRESET_CHARACTERS, PRESET_IDEOLOGIES);
    const buffs = PRESET_BUFFS.filter((b) => ["b02", "b06", "b13"].includes(b.id)).map((b) => ({ ...b, enabled: true }));
    const base1 = simulate(p, params, PRESET_EVENTS, traits, 1, 0, "zh", []);
    const base2 = simulate(p, params, PRESET_EVENTS, traits, 1, 0, "zh", []);
    const buffed1 = simulate(p, params, PRESET_EVENTS, traits, 1, 0, "zh", buffs);
    const buffed2 = simulate(p, params, PRESET_EVENTS, traits, 1, 0, "zh", buffs);
    // 无 buff：同种子可重现；有 buff：同样可重现
    expect(JSON.stringify(base1)).toBe(JSON.stringify(base2));
    expect(JSON.stringify(buffed1)).toBe(JSON.stringify(buffed2));
    // buff 开启确实改变了行为树的走向
    expect(JSON.stringify(buffed1)).not.toBe(JSON.stringify(base1));
  });
});
