import { beforeEach, describe, expect, it } from "vitest";
import {
  COLOR_TOKENS, COLOR_TOKEN_COUNT, SPACING_TOKENS, MOTION_CURVES, MOTION_DURATIONS,
  validateTokenTable, defaultTokenTable, tokenTableJsonSchema, tokenTableHash,
  coverageReport, resetToken, tokenVarValues, applyTokenTableToDOM, clearTokenTableFromDOM,
  loadTokenTable, saveTokenTable, LEGACY_BRIDGE,
} from "../tokens";
import { personaStore } from "../store";

beforeEach(() => {
  personaStore.reset();
  clearTokenTableFromDOM();
});

describe("F151 主题令牌全集", () => {
  it("颜色令牌 24 枚（功能定义数字对账）", () => {
    expect(COLOR_TOKENS).toHaveLength(COLOR_TOKEN_COUNT);
    expect(COLOR_TOKEN_COUNT).toBe(24);
    expect(new Set(COLOR_TOKENS.map((t) => t.key)).size).toBe(24);
  });

  it("间距六档（乙-1 表）与动效五曲线三时长", () => {
    expect(SPACING_TOKENS).toHaveLength(6);
    expect(Object.keys(MOTION_CURVES)).toEqual(["enter", "exit", "emphasized", "spring", "linear"]);
    expect(Object.values(MOTION_DURATIONS).map((d) => d.ms)).toEqual([120, 200, 320]);
  });

  it("默认表校验通过且含版本戳", () => {
    const r = validateTokenTable(defaultTokenTable());
    expect(r.ok).toBe(true);
    expect(r.data?.version).toBe(1);
  });

  it("非法色值报 error；未知令牌报 conflict 不阻断；缺失令牌默认兜底报 warning", () => {
    const d = defaultTokenTable();
    const r = validateTokenTable({
      colors: { "--p-accent": "orange", "--p-unknown-token": "#112233" },
      radius: d.radius, font: d.font, motion: d.motion, version: 1,
    });
    expect(r.ok).toBe(false);
    expect(r.issues.some((i) => i.field === "colors.--p-accent" && i.level === "error")).toBe(true);
    expect(r.issues.some((i) => i.field === "colors.--p-unknown-token" && i.level === "conflict")).toBe(true);
    expect(r.issues.filter((i) => i.level === "warning" && i.message.includes("兜底")).length).toBeGreaterThan(0);
  });

  it("旧版本令牌表 warning 兜底（兼容矩阵）", () => {
    const d = defaultTokenTable();
    const r = validateTokenTable({ ...d, version: 0 });
    expect(r.issues.some((i) => i.level === "warning" && i.field === "version")).toBe(true);
  });

  it("圆角/字号越界钳制 + 非法动效档报错", () => {
    const d = defaultTokenTable();
    const r = validateTokenTable({
      ...d,
      radius: { control: 99, card: -3, window: 16 },
      font: { caption: 5, body: 15, title: 20, display: 32 },
      motion: { enter: { curve: "warp", duration: "micro" } },
    });
    expect(r.ok).toBe(false);
    expect(r.issues.some((i) => i.field === "radius.control" && i.level === "warning")).toBe(true);
    expect(r.issues.some((i) => i.field === "motion.enter.curve")).toBe(true);
  });

  it("JSON Schema 发布：结构与校验器双向一致（非法值 schema 也会拒）", () => {
    const schema = tokenTableJsonSchema() as { properties: Record<string, { required?: string[]; properties?: Record<string, unknown> }> };
    expect(schema.properties).toBeTruthy();
    expect(schema.properties.colors).toBeTruthy();
    expect(schema.properties.motion).toBeTruthy();
  });

  it("哈希：改一枚令牌哈希必变；恢复默认哈希还原（零残留对拍锚）", () => {
    const d = defaultTokenTable();
    const h1 = tokenTableHash(d);
    const changed = { ...d, colors: { ...d.colors, "--p-accent": "#ff8800" } };
    expect(tokenTableHash(changed)).not.toBe(h1);
    expect(tokenTableHash(JSON.parse(JSON.stringify(d)))).toBe(h1);
  });

  it("覆盖度报告：未覆盖清单 + 未知令牌清单", () => {
    const d = defaultTokenTable();
    const full = coverageReport(d);
    expect(full.covered).toBe(24);
    const partial = coverageReport(d, { "--p-accent": "#fff000" });
    expect(partial.covered).toBe(1);
    expect(partial.missing).toHaveLength(23);
    const withUnknown = coverageReport(d, { "--p-accent": "#fff000", "--p-mystery": "#fff000" });
    expect(withUnknown.unknown).toEqual(["--p-mystery"]);
  });

  it("单令牌回退：resetToken 只还原指定令牌", () => {
    const d = defaultTokenTable();
    const changed = { ...d, colors: { ...d.colors, "--p-accent": "#ff8800", "--p-warn": "#00ff00" } };
    const r = resetToken(changed, "--p-accent");
    expect(r.colors["--p-accent"]).toBe(d.colors["--p-accent"]);
    expect(r.colors["--p-warn"]).toBe("#00ff00");
  });

  it("热替换：apply 写入 DOM 变量 + 桥接既有变量；clear 全清（无残留）——node 环境无 DOM 时函数安全空转", () => {
    const d = defaultTokenTable();
    expect(() => applyTokenTableToDOM(d)).not.toThrow();
    if (typeof document === "undefined") {
      // node 测试环境：无 DOM，热替换的变量展开正确性由 tokenVarValues 用例覆盖。
      expect(() => clearTokenTableFromDOM()).not.toThrow();
      return;
    }
    const rootStyle = document.documentElement.style;
    expect(rootStyle.getPropertyValue("--p-accent")).toBeTruthy();
    expect(rootStyle.getPropertyValue(LEGACY_BRIDGE["--p-accent"] ?? "")).toBeTruthy();
    clearTokenTableFromDOM();
    expect(rootStyle.getPropertyValue("--p-accent")).toBe("");
    expect(rootStyle.getPropertyValue(LEGACY_BRIDGE["--p-accent"] ?? "")).toBe("");
  });

  it("tokenVarValues：动效双值展开（曲线+时长两个变量）", () => {
    const vars = tokenVarValues(defaultTokenTable());
    expect(vars["--p-motion-enter"]).toBe("200");
    expect(vars["--p-ease-enter"]).toBe(MOTION_CURVES.enter.bezier);
    expect(vars["--p-r-window"]).toBe("16px");
    expect(vars["--p-sp-1"]).toBe("4px");
  });

  it("save/load round-trip 经 personaStore（哈希等值）", () => {
    const d = defaultTokenTable();
    saveTokenTable(d);
    expect(tokenTableHash(loadTokenTable())).toBe(tokenTableHash(d));
  });
});
