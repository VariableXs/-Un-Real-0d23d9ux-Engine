import { beforeEach, describe, expect, it } from "vitest";
import {
  RULE_TEMPLATES,
  clearRuleLogs,
  decide,
  importRules,
  loadRules,
  resolveSnapRect,
  ruleLogs,
  saveRule,
  validateRule,
} from "../rules";
import type { Rule } from "../rules";

function rule(p: Partial<Rule>): Rule {
  return {
    id: "r1",
    name: "测试规则",
    trigger: { app: "write" },
    actions: [{ type: "topmost" }],
    priority: 10,
    enabled: true,
    ...p,
  };
}

beforeEach(() => {
  localStorage.clear();
  clearRuleLogs();
});

describe("N-03 规则校验与导入", () => {
  it("合法规则零错误", () => {
    expect(validateRule(rule({}))).toEqual([]);
  });

  it("语法错误行级报错不崩溃（验收 ④）", () => {
    const errs = validateRule({ id: "", trigger: { app: 1, titlePattern: "[" }, actions: [] });
    expect(errs.length).toBeGreaterThan(0);
    expect(errs.some((e) => e.includes("titlePattern"))).toBe(true);
  });

  it("导入他人规则 → 同 id 覆盖、其余追加；非法条目跳过并报行级错误（验收 ②）", () => {
    saveRule(rule({ id: "r1", name: "本地" }));
    const { merged, errors } = importRules([
      rule({ id: "r1", name: "外来" }),
      rule({ id: "r2", name: "新增" }),
      { broken: true },
    ]);
    expect(merged.find((r) => r.id === "r1")?.name).toBe("外来");
    expect(merged.some((r) => r.id === "r2")).toBe(true);
    expect(errors).toEqual([{ index: 2, errs: expect.any(Array) }]);
    expect(loadRules().length).toBe(2);
  });
});

describe("N-03 裁决", () => {
  it("触发延迟路径纯同步、留痕日志（验收 ① 的日志口径）", () => {
    const d = decide({ winId: "w1", app: "write", title: "文档", restricted: false }, [rule({})], 123);
    expect(d.rule?.name).toBe("测试规则");
    expect(ruleLogs()).toHaveLength(1);
    expect(ruleLogs()[0]!.ts).toBe(123);
  });

  it("冲突按 priority 优先、同分比 specificity（精确 > 通配）", () => {
    const wildcard = rule({ id: "w", trigger: { app: "*" }, priority: 10 });
    const precise = rule({ id: "p", trigger: { app: "write" }, priority: 10 });
    const highWildcard = rule({ id: "hw", trigger: { app: "*" }, priority: 99 });
    const d = decide({ winId: "w1", app: "write", title: "", restricted: false }, [wildcard, precise, highWildcard]);
    expect(d.rule?.id).toBe("hw"); // priority 先于 specificity
    const d2 = decide({ winId: "w1", app: "write", title: "", restricted: false }, [wildcard, precise]);
    expect(d2.rule?.id).toBe("p"); // 同分精确胜
  });

  it("L4/管理员窗口：任何规则下动作全部拒绝（安全分叉不可放宽，验收 ③）", () => {
    const d = decide({ winId: "w1", app: "game.exe", title: "AntiCheat", restricted: true }, [rule({ priority: 999 })]);
    expect(d.rule).toBeNull();
    expect(ruleLogs()[0]!.text).toContain("拒绝");
  });

  it("透明度越界动作被拒绝并留日志", () => {
    const d = decide(
      { winId: "w1", app: "write", title: "", restricted: false },
      [rule({ actions: [{ type: "opacity", value: 5 }] })],
    );
    expect(d.rejected).toHaveLength(1);
    expect(d.rejected[0]!.reason).toContain("30");
  });

  it("日志环形缓冲上限 500 条", () => {
    for (let i = 0; i < 505; i++) {
      decide({ winId: `w${i}`, app: "write", title: "", restricted: true }, []);
    }
    expect(ruleLogs().length).toBe(500);
    expect(ruleLogs()[0]!.text).toContain("w5");
  });

  it("无命中 → rule=null 维持现状", () => {
    expect(decide({ winId: "w1", app: "fate", title: "", restricted: false }, [rule({})]).rule).toBeNull();
  });
});

describe("N-03 模板与矩形换算", () => {
  it("内置模板 12 条且全部通过校验", () => {
    expect(RULE_TEMPLATES).toHaveLength(12);
    for (const t of RULE_TEMPLATES) expect(validateRule({ id: "x", ...t.rule })).toEqual([]);
  });

  it("snapRect 比例分量按工作区换算、像素分量原样保留", () => {
    const r = resolveSnapRect({ x: 0.5, y: 0, w: 0.5, h: 1 }, { x: 0, y: 54, w: 1920, h: 1000 });
    expect(r).toEqual({ x: 960, y: 54, w: 960, h: 1000 });
    const abs = resolveSnapRect({ x: 100, y: 200, w: 800, h: 600 }, { x: 0, y: 0, w: 1920, h: 1000 });
    expect(abs).toEqual({ x: 100, y: 200, w: 800, h: 600 });
  });
});

