import { describe, it, expect } from "vitest";
import { ICON_SEMANTICS, ALL_ICON_SEMANTICS, resolveIcon, resolveSegoe, SEGOE_FLUENT_MAP, ICON_SIZE_MAP } from "../iconRegistry";

describe("AI-17 iconRegistry（U-10/Z-01）", () => {
  it("每个语义都有唯一 lucide 组件映射", () => {
    for (const s of ALL_ICON_SEMANTICS) {
      expect(ICON_SEMANTICS[s], `semantic ${s} missing`).toBeTruthy();
    }
    // 语义键数与映射数一致（无未登记映射）
    expect(Object.keys(ICON_SEMANTICS).length).toBe(ALL_ICON_SEMANTICS.length);
  });

  it("lucide 组件无重复登记（同一组件不得承担两个语义）", () => {
    const seen = new Map<unknown, string>();
    for (const [semantic, icon] of Object.entries(ICON_SEMANTICS)) {
      const prev = seen.get(icon);
      expect(prev, `semantic conflict: "${prev}" and "${semantic}" share one icon`).toBeUndefined();
      seen.set(icon, semantic);
    }
  });

  it("resolveIcon 未知语义抛错（防漂移）", () => {
    expect(() => resolveIcon("nope" as never)).toThrow(/unknown semantic/);
  });

  it("resolveIcon 返回登记的组件", () => {
    expect(resolveIcon("delete")).toBe(ICON_SEMANTICS.delete);
  });

  it("Segoe Fluent 码位映射格式正确（单字符 PUA 码位）", () => {
    for (const [semantic, cp] of Object.entries(SEGOE_FLUENT_MAP)) {
      expect(cp, semantic).toMatch(/^[\uE000-\uF8FF]$/);
    }
  });

  it("高频语义都有 Segoe 对应物（Z-01 系统图标首选）", () => {
    for (const key of ["delete", "close", "search", "settings", "folder", "copy", "refresh"] as const) {
      expect(resolveSegoe(key), key).toBeTruthy();
    }
  });

  it("尺寸档位符合 Z-01 规格（16/18/24/32）", () => {
    expect(Object.values(ICON_SIZE_MAP).sort()).toEqual([16, 18, 24, 32]);
  });
});
