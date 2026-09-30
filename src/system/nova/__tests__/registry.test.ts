/**
 * NOVA-200 S0 · 注册表单测（AI-01 路自证）。
 * 口径（实施总步骤 §0.1）：200 项唯一、十六域分布 8×12+8×13、持久化往返、
 * 与 singularity 键空间零冲突、词条与 registry 一一对应。
 */

import { beforeEach, describe, expect, it } from "vitest";
import {
  NOVA_DOMAINS,
  NOVA_FEATURES,
  novaOn,
  novaParam,
  novaSnapshot,
  novaStats,
  resetNovaAll,
  setNovaOn,
  setNovaParam,
  subscribeNova,
} from "../registry";
import { NOVA_LABELS, novaT } from "../labels";

const STORAGE_KEY = "nova.registry.v1";

beforeEach(() => {
  resetNovaAll();
});

describe("nova/registry 200 项注册表", () => {
  it("恰好 200 项且 id 唯一", () => {
    expect(NOVA_FEATURES).toHaveLength(200);
    expect(new Set(NOVA_FEATURES.map((f) => f.id)).size).toBe(200);
  });

  it("编号连续：W-001 … W-200", () => {
    NOVA_FEATURES.forEach((f, i) => {
      expect(f.id).toBe(`W-${String(i + 1).padStart(3, "0")}`);
    });
  });

  it("十六域定义完整且分布为 8×12 + 8×13", () => {
    expect(NOVA_DOMAINS).toHaveLength(16);
    const byDomain = new Map<string, number>();
    for (const f of NOVA_FEATURES) byDomain.set(f.domain, (byDomain.get(f.domain) ?? 0) + 1);
    expect(byDomain.size).toBe(16);
    const counts = [...byDomain.values()].sort((a, b) => a - b);
    expect(counts).toEqual([...Array(8).fill(12), ...Array(8).fill(13)].sort((a, b) => a - b));
    expect(counts.reduce((a, b) => a + b, 0)).toBe(200);
  });

  it("每项 changelog 为一行非空说明；域 id 合法", () => {
    const domainIds = new Set(NOVA_DOMAINS.map((d) => d.id));
    for (const f of NOVA_FEATURES) {
      expect(f.changelog.length).toBeGreaterThan(4);
      expect(f.changelog).not.toMatch(/\n/);
      expect(domainIds.has(f.domain)).toBe(true);
    }
  });

  it("默认态克制：重动效/opt-in 类默认关（抽测）", () => {
    for (const id of ["W-013", "W-039", "W-086", "W-090", "W-121", "W-141", "W-164"]) {
      expect(NOVA_FEATURES.find((f) => f.id === id)?.on).toBe(false);
    }
    for (const id of ["W-001", "W-002", "W-006", "W-008", "W-012"]) {
      expect(NOVA_FEATURES.find((f) => f.id === id)?.on).toBe(true);
    }
  });

  it("setNovaOn / setNovaParam 状态生效并持久化往返", () => {
    setNovaOn("W-001", false);
    expect(novaOn("W-001")).toBe(false);
    setNovaParam("W-002", "diskFactor", 80);
    expect(novaParam("W-002", "diskFactor")).toBe(80);
    const raw = localStorage.getItem(STORAGE_KEY);
    expect(raw).toBeTruthy();
    const parsed = JSON.parse(raw!) as Record<string, { on: boolean; params: Record<string, unknown> }>;
    expect(parsed["W-001"]!.on).toBe(false);
    expect(parsed["W-002"]!.params.diskFactor).toBe(80);
    expect(novaSnapshot()["W-002"]!.params.diskFactor).toBe(80);
  });

  it("未知 id / 未知参数键安全回退", () => {
    expect(novaOn("W-999")).toBe(false);
    expect(novaParam("W-002", "nope")).toBe(false);
  });

  it("键空间与 singularity 零冲突：注册表只写 nova.* 键", () => {
    localStorage.setItem("variable.singu.probe", "1");
    setNovaOn("W-001", false);
    setNovaParam("W-002", "diskFactor", 65);
    const keys: string[] = [];
    for (let i = 0; i < localStorage.length; i++) keys.push(localStorage.key(i) ?? "");
    expect(keys).toContain("nova.registry.v1");
    for (const k of keys) {
      if (k !== "variable.singu.probe") expect(k.startsWith("nova.")).toBe(true);
    }
  });

  it("subscribeNova 通知与退订", () => {
    let hits = 0;
    const un = subscribeNova(() => hits++);
    setNovaOn("W-003", false);
    expect(hits).toBe(1);
    un();
    setNovaOn("W-003", true);
    expect(hits).toBe(1); // 退订后不再通知
  });

  it("novaStats 统计口径与恢复默认", () => {
    const before = novaStats();
    expect(before.total).toBe(200);
    expect(before.on).toBe(NOVA_FEATURES.filter((f) => f.on).length);
    setNovaOn("W-001", false);
    setNovaOn("W-002", false);
    expect(novaStats().on).toBe(before.on - 2);
    resetNovaAll();
    expect(novaStats().on).toBe(before.on);
  });
});

describe("nova/labels 词条与注册表一一对应", () => {
  it("每个 W-XXX 与 W-XXXd 均有词条，novaT 双语可译", () => {
    for (const f of NOVA_FEATURES) {
      expect(NOVA_LABELS[f.id]).toBeTruthy();
      expect(NOVA_LABELS[`${f.id}d`]).toBeTruthy();
      expect(novaT(f.id, "zh")).not.toBe(f.id);
      expect(novaT(f.id, "en")).not.toBe(f.id);
    }
  });

  it("每个参数与选项的 labelKey 均有词条", () => {
    for (const f of NOVA_FEATURES) {
      for (const p of f.params ?? []) {
        expect(NOVA_LABELS[p.labelKey], `${f.id} 参数 ${p.labelKey} 缺词条`).toBeTruthy();
        for (const opt of p.options ?? []) {
          expect(NOVA_LABELS[opt.labelKey], `${f.id} 选项 ${opt.labelKey} 缺词条`).toBeTruthy();
        }
      }
    }
  });

  it("缺键回退键名本身", () => {
    expect(novaT("nope", "zh")).toBe("nope");
  });
});
