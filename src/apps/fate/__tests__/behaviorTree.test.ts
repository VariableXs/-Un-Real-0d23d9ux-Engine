import { describe, expect, it } from "vitest";
import { makeRng } from "../engine";
import {
  BEHAVIOR_TREE_NODES,
} from "../dictionaries";
import {
  blackboardFromTraits, defaultFatePolicyTree, makeBtBiasFn, selectBehavior,
  tickBT, type BtNode,
} from "../behaviorTree";

/** 词典本身的行为树品控：id/name 唯一、三类齐备、bias 数值有界。 */
describe("BEHAVIOR_TREE_NODES (行为树词典)", () => {
  it("ids and names are globally unique", () => {
    expect(BEHAVIOR_TREE_NODES.length).toBeGreaterThanOrEqual(25);
    const ids = new Set<string>();
    const names = new Set<string>();
    for (const b of BEHAVIOR_TREE_NODES) {
      expect(ids.has(b.id)).toBe(false);
      ids.add(b.id);
      expect(names.has(b.name)).toBe(false);
      names.add(b.name);
    }
  });

  it("covers all three node kinds with bilingual descriptions", () => {
    for (const kind of ["composite", "decorator", "leaf"] as const) {
      expect(BEHAVIOR_TREE_NODES.some((b) => b.kind === kind)).toBe(true);
    }
    for (const b of BEHAVIOR_TREE_NODES) {
      expect(b.nameEn.length).toBeGreaterThan(0);
      expect(b.desc.length).toBeGreaterThan(0);
      expect(b.descEn.length).toBeGreaterThan(0);
    }
  });

  it("bias values stay within the engine multiplier envelope", () => {
    for (const b of BEHAVIOR_TREE_NODES) {
      for (const k of Object.values(b.bias)) {
        // 负值 = 压制该标签的行为（如并行推进压制健康），幅值仍须有界
        expect(Math.abs(k)).toBeGreaterThanOrEqual(0.05);
        expect(Math.abs(k)).toBeLessThanOrEqual(4);
      }
    }
  });
});

/** 基本语义：每个节点类型的成败规则都要真正可执行。 */
describe("tickBT (行为树执行器)", () => {
  const rng = (): () => number => makeRng(7);

  it("sequence aborts on first failure", () => {
    const tree: BtNode = {
      id: "s", kind: "sequence", children: [
        { id: "a1", kind: "action", action: "稳守本业" },
        { id: "c1", kind: "condition", tag: "冒险", threshold: 999 },
        { id: "a2", kind: "action", action: "放手一搏" },
      ],
    };
    const bb = blackboardFromTraits({ 冒险: 80 });
    const r = tickBT(tree, bb, rng());
    expect(r.status).toBe("failure");
    expect(r.trace).toEqual(["稳守本业"]);
  });

  it("selector falls through to the first succeeding branch", () => {
    const tree: BtNode = {
      id: "sel", kind: "selector", children: [
        { id: "c1", kind: "condition", tag: "事业", threshold: 999 },
        { id: "c2", kind: "condition", tag: "事业", threshold: 25 },
        { id: "a", kind: "action", action: "审时度势" },
      ],
    };
    const r = tickBT(tree, blackboardFromTraits({ 事业: 20 }), rng());
    expect(r.status).toBe("success");
    expect(r.trace).toEqual(["审时度势"]);
  });

  it("parallel requires every child to succeed", () => {
    const ok: BtNode = { id: "p-ok", kind: "parallel", children: [{ id: "a", kind: "action" }, { id: "b", kind: "action" }] };
    const bad: BtNode = { id: "p-bad", kind: "parallel", children: [{ id: "a", kind: "action" }, { id: "c", kind: "condition", tag: "健康", threshold: 999 }] };
    expect(tickBT(ok, blackboardFromTraits(), rng()).status).toBe("success");
    expect(tickBT(bad, blackboardFromTraits(), rng()).status).toBe("failure");
  });

  it("inverter flips the child result", () => {
    const t: BtNode = { id: "inv", kind: "invert", children: [{ id: "c", kind: "condition", tag: "冒险", threshold: 50 }] };
    expect(tickBT(t, blackboardFromTraits({ 冒险: 80 }), rng()).status).toBe("failure");
    expect(tickBT(t, blackboardFromTraits({ 冒险: 10 }), rng()).status).toBe("success");
  });

  it("retry stops after its budget", () => {
    const t: BtNode = { id: "r", kind: "retry", retries: 3, children: [{ id: "c", kind: "condition", tag: "事业", threshold: 999 }] };
    expect(tickBT(t, blackboardFromTraits(), rng()).status).toBe("failure");
  });

  it("cooldown blocks re-execution within its window", () => {
    const t: BtNode = { id: "cd", kind: "cooldown", cooldown: 2, children: [{ id: "a", kind: "action", action: "调养身心" }] };
    const bb = blackboardFromTraits();
    const r1 = tickBT(t, bb, rng());
    const r2 = tickBT(t, bb, rng());
    const r3 = tickBT(t, bb, rng());
    expect(r1.status).toBe("success");
    expect(r2.status).toBe("failure");
    expect(r3.status).toBe("success");
  });

  it("counter limit caps successful executions", () => {
    const t: BtNode = { id: "lim", kind: "limit", limit: 2, children: [{ id: "a", kind: "action", action: "积累财富" }] };
    const bb = blackboardFromTraits();
    expect(tickBT(t, bb, rng()).status).toBe("success");
    expect(tickBT(t, bb, rng()).status).toBe("success");
    expect(tickBT(t, bb, rng()).status).toBe("failure");
  });

  it("probability gate consumes the rng deterministically", () => {
    const t: BtNode = { id: "pg", kind: "probability", probability: 0.5, children: [{ id: "a", kind: "action", action: "放手一搏" }] };
    const a = tickBT(t, blackboardFromTraits(), makeRng(1));
    const b = tickBT(t, blackboardFromTraits(), makeRng(1));
    expect(a).toEqual(b);
    const open = tickBT(t, blackboardFromTraits(), (): number => 0.1);
    const shut = tickBT(t, blackboardFromTraits(), (): number => 0.9);
    expect(open.trace).toEqual(["放手一搏"]);
    expect(shut.trace).toEqual([]);
    expect(shut.status).toBe("failure");
  });
});

/** 与命运引擎的联动：策略树根据性格选行为，bias 进入事件概率乘子。 */
describe("defaultFatePolicyTree (命运策略树)", () => {
  it("chooses Bold Push for the adventurous and Hold Ground for the steady", () => {
    const bold = selectBehavior({ 冒险: 80 }, makeRng(3));
    const steady = selectBehavior({ 守成: 80 }, makeRng(3));
    expect(bold.trace).toContain("放手一搏");
    expect(steady.trace).toContain("稳守本业");
  });

  it("nurses health first when the body is failing", () => {
    const r = selectBehavior({ 健康: -80, 冒险: 90 }, makeRng(3));
    expect(r.trace[0]).toBe("调养身心");
  });

  it("falls back to Assess when nothing crosses the threshold", () => {
    const r = selectBehavior({}, makeRng(3));
    expect(r.trace).toEqual(["突破茧壳"]); // 觉醒尚浅 → 逼自己破壳
  });

  it("is fully deterministic for the same traits and seed", () => {
    const a = selectBehavior({ 冒险: 60, 社交: 55 }, makeRng(42));
    const b = selectBehavior({ 冒险: 60, 社交: 55 }, makeRng(42));
    expect(a).toEqual(b);
  });

  it("selected behavior biases event tags exactly like the dictionary says", () => {
    const fn = makeBtBiasFn(["放手一搏"]);
    expect(fn("冒险")).toBeCloseTo(1.2);
    expect(fn("危险")).toBeCloseTo(1.15);
    expect(fn("健康")).toBe(1);
    const clamped = makeBtBiasFn(["调养身心", "守护家人", "放手一搏"]);
    // 叠乘仍被钳制在引擎的 0.05..4 包络内
    expect(clamped("健康")).toBeLessThanOrEqual(4);
    expect(clamped("健康")).toBeGreaterThanOrEqual(0.05);
  });

  it("every leaf referenced by the policy tree exists in the dictionary", () => {
    const dict = new Set(BEHAVIOR_TREE_NODES.map((b) => b.name));
    const tree = defaultFatePolicyTree();
    const leaves: string[] = [];
    const walk = (n: { kind: string; action?: string; children?: { kind: string; action?: string; children?: unknown }[] }): void => {
      if (n.kind === "action" && n.action) leaves.push(n.action);
      for (const c of n.children ?? []) walk(c as { kind: string; action?: string; children?: { kind: string; action?: string; children?: unknown }[] });
    };
    walk(tree as unknown as { kind: string; children?: { kind: string; action?: string; children?: unknown }[] });
    expect(leaves.length).toBeGreaterThan(5);
    for (const l of leaves) expect(dict.has(l)).toBe(true);
  });
});
