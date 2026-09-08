import { describe, expect, it } from "vitest";
import {
  contrastRatio, compositeOver, defaultVariant, detectVariant, formatOklch,
  hexToOklch, oklchToHex, relativeLuminance, validateVariantContrast,
} from "../tokens";

describe("contrastRatio（WCAG 相对亮度）", () => {
  it("边界值", () => {
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 1);
    expect(contrastRatio("#ffffff", "#000000")).toBeCloseTo(21, 1);
    expect(contrastRatio("#ffffff", "#ffffff")).toBeCloseTo(1, 5);
    expect(contrastRatio("#000000", "#000000")).toBeCloseTo(1, 5);
  });

  it("已知中灰（#767676 vs 白 ≈ 4.54，AA 临界）", () => {
    expect(contrastRatio("#767676", "#ffffff")).toBeGreaterThan(4.5);
    expect(contrastRatio("#757575", "#ffffff")).toBeLessThan(4.5);
  });

  it("无效输入返回 0（必然不达标）", () => {
    expect(contrastRatio("nope", "#ffffff")).toBe(0);
    expect(contrastRatio("#12345", "#ffffff")).toBe(0);
    expect(relativeLuminance("zzz")).toBe(-1);
  });

  it("亮度排序：白 > 灰 > 黑", () => {
    const w = relativeLuminance("#ffffff");
    const g = relativeLuminance("#808080");
    const b = relativeLuminance("#000000");
    expect(w).toBeGreaterThan(g);
    expect(g).toBeGreaterThan(b);
  });
});

describe("compositeOver 半透明合成", () => {
  it("全不透明 = 原色", () => {
    expect(compositeOver("#ff0000", "#00ff00")).toBe("#ff0000");
  });

  it("半透明红叠白底 = 粉", () => {
    // #ff000080 → alpha 0.502 → r=255, g=b≈127
    const out = compositeOver("#ff000080", "#ffffff");
    expect(out).not.toBeNull();
    expect(out?.slice(0, 3)).toBe("#ff");
    const g = parseInt(out?.slice(3, 5) ?? "00", 16);
    expect(g).toBeGreaterThan(100);
    expect(g).toBeLessThan(160);
  });
});

describe("OKLCH 换算（标准 OKLab 矩阵）", () => {
  it("黑白灰的 L 值", () => {
    const black = hexToOklch("#000000");
    const white = hexToOklch("#ffffff");
    expect(black?.l ?? 9).toBeCloseTo(0, 2);
    expect(white?.l ?? 0).toBeCloseTo(1, 2);
    expect((white?.c ?? 1)).toBeLessThan(0.001);
  });

  it("往返一致（gamut 内）", () => {
    for (const hex of ["#3355aa", "#aa5533", "#22aa88", "#8844cc", "#dedede"]) {
      const o = hexToOklch(hex);
      expect(o).not.toBeNull();
      expect(oklchToHex(o?.l ?? 0, o?.c ?? 0, o?.h ?? 0).toLowerCase()).toBe(hex.toLowerCase());
    }
  });

  it("gamut 外收敛：输出仍是合法 hex 且亮度接近", () => {
    const hex = oklchToHex(0.5, 0.4, 30);
    expect(hex).toMatch(/^#[0-9a-fA-F]{6}$/);
    const back = hexToOklch(hex);
    expect(back?.l ?? 0).toBeCloseTo(0.5, 1);
  });

  it("formatOklch 输出 CSS oklch() 形态", () => {
    expect(formatOklch(hexToOklch("#ffffff"))).toMatch(/^oklch\(/);
    expect(formatOklch(null)).toBe("—");
  });
});

describe("detectVariant 跟随 data-theme 信号", () => {
  it("映射（settings.ThemeId → 工坊变体）", () => {
    expect(detectVariant("high-contrast")).toBe("hc");
    expect(detectVariant("paper")).toBe("light");
    expect(detectVariant("deep-space")).toBe("dark");
    expect(detectVariant("minimal-black")).toBe("dark");
    expect(detectVariant("custom")).toBe("dark");
    expect(detectVariant(undefined)).toBe("dark");
  });
});

describe("默认三变体（取自 tokens.css 三亮度层）", () => {
  it("默认即 AA 达标（防线：出厂主题不触发红牌）", () => {
    for (const kind of ["light", "dark", "hc"] as const) {
      const { colors } = defaultVariant(kind);
      const bad = validateVariantContrast(colors).filter((p) => !p.ok);
      expect(bad, `${kind} 应全对达标: ${JSON.stringify(bad)}`).toHaveLength(0);
    }
  });

  it("hc 文字对比拉满", () => {
    const { colors } = defaultVariant("hc");
    expect(contrastRatio(colors["--text-primary"] ?? "#000", colors["--bg-canvas"] ?? "#fff")).toBeGreaterThan(15);
  });
});

describe("validateVariantContrast 红牌行为", () => {
  it("低对比组合被点名", () => {
    const colors = defaultVariant("dark").colors;
    colors["--text-primary"] = "#555555";
    const pairs = validateVariantContrast(colors);
    const hit = pairs.find((p) => p.fgKey === "--text-primary" && p.bgKey === "--bg-canvas");
    expect(hit?.ok).toBe(false);
  });
});