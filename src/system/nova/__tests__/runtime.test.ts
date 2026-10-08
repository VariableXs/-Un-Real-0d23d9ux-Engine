/**
 * NOVA-200 S0 地基 · NovaRuntime / activate 纯逻辑单测。
 * 运行时在 node 环境下行为层 no-op（真实模块激活需 DOM），此处验证：
 * 降级映射、域激活判据、同步差分、装配表覆盖缺口与 activate 幂等。
 */

import { beforeEach, describe, expect, it } from "vitest";
import { NOVA_DOMAINS, NOVA_FEATURES, novaDomainActive, resetNovaAll, setNovaOn } from "../registry";
import {
  activeDomainsOf,
  degradeFromSettings,
  diffModuleSync,
  missingDomainModules,
} from "../NovaRuntime";
import { activateNovaFoundation } from "../activate";

beforeEach(() => resetNovaAll());

describe("degradeFromSettings（统一降级链映射）", () => {
  it("safeMode=true → safe 数据集开", () => {
    expect(degradeFromSettings({ safeMode: true, perfMode: "high" })).toEqual({ safe: true, static: false });
  });

  it("perfMode=static → static 数据集开（App 的『static』性能档即静态模式）", () => {
    expect(degradeFromSettings({ safeMode: false, perfMode: "static" })).toEqual({ safe: false, static: true });
  });

  it("reduce-motion 不在本运行时管辖（归 App 既有链路）", () => {
    const d = degradeFromSettings({ safeMode: false, perfMode: "high" });
    expect(Object.keys(d)).toEqual(["safe", "static"]);
  });
});

describe("activeDomainsOf（按域激活判据 = novaDomainActive）", () => {
  it("默认态：五个已交付域全部激活（默认开关多为 true）", () => {
    const table = [
      { domain: "boot" as const },
      { domain: "windows" as const },
      { domain: "desktop" as const },
      { domain: "dock" as const },
      { domain: "input" as const },
    ];
    expect(activeDomainsOf(table)).toEqual(["boot", "windows", "desktop", "dock", "input"]);
  });

  it("域内全关 → 域退出激活集合", () => {
    for (const f of NOVA_FEATURES.filter((x) => x.domain === "boot")) setNovaOn(f.id, false);
    expect(novaDomainActive("boot")).toBe(false);
    expect(activeDomainsOf([{ domain: "boot" }])).toEqual([]);
  });

  it("域内任一开启 → 域激活", () => {
    for (const f of NOVA_FEATURES.filter((x) => x.domain === "input")) setNovaOn(f.id, false);
    setNovaOn("W-051", true); // 仅恢复一项
    expect(activeDomainsOf([{ domain: "input" }])).toEqual(["input"]);
  });
});

describe("diffModuleSync（激活幂等差分）", () => {
  it("未激活且应激活 → activate；已激活且不应激活 → deactivate", () => {
    const live = new Map([["boot" as const, { domain: "boot" as const }]]);
    const { toActivate, toDeactivate } = diffModuleSync(new Set(["windows", "dock"]), live);
    expect(toActivate).toEqual(["windows", "dock"]);
    expect(toDeactivate).toEqual(["boot"]);
  });

  it("已激活且应激活 → 零调用（幂等）", () => {
    const live = new Map([["boot" as const, { domain: "boot" as const }]]);
    const { toActivate, toDeactivate } = diffModuleSync(new Set(["boot"]), live);
    expect(toActivate).toEqual([]);
    expect(toDeactivate).toEqual([]);
  });

  it("全关 → 全部 deactivate", () => {
    const live = new Map([
      ["boot" as const, { domain: "boot" as const }],
      ["input" as const, { domain: "input" as const }],
    ]);
    const { toActivate, toDeactivate } = diffModuleSync(new Set(), live);
    expect(toActivate).toEqual([]);
    expect(toDeactivate.sort()).toEqual(["boot", "input"]);
  });
});

describe("missingDomainModules（S18 门禁：装配表覆盖缺口）", () => {
  it("当前装配表缺 AI-06…AI-16 的 11 个域（如实暴露，不虚报完整）", () => {
    const table = [
      { domain: "boot" as const },
      { domain: "windows" as const },
      { domain: "desktop" as const },
      { domain: "dock" as const },
      { domain: "input" as const },
    ];
    const missing = missingDomainModules(table);
    expect(missing).toHaveLength(11);
    expect(missing).toContain("files");
    expect(missing).toContain("quality");
  });

  it("十六域全配 → 零缺口", () => {
    expect(missingDomainModules(NOVA_DOMAINS.map((d) => ({ domain: d.id })))).toEqual([]);
  });
});

describe("activateNovaFoundation（副作用激活幂等）", () => {
  it("node 环境安全 no-op 且可重复调用（不抛错）", () => {
    expect(() => activateNovaFoundation()).not.toThrow();
    expect(() => activateNovaFoundation()).not.toThrow();
  });
});
