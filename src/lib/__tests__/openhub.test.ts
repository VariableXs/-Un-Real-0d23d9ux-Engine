/**
 * AI-14 开放接口组前端库单测：
 * shareFormat（Z-54）/ themeToken（Z-50）/ connectors（Z-55）/
 * ecoMatrix（Z-56）/ pluginManifest（Z-52）。
 */
import { describe, expect, it } from "vitest";

import {
  openEnvelope,
  sealEnvelope,
} from "../shareFormat";
import { contrastRatio, validateTheme } from "../themeToken";
import { coerceConnector, normalizeRefresh } from "../connectors";
import { checkEcoCompat, entryOf, matrixSelfCheck } from "../ecoMatrix";
import { validatePluginManifest } from "../pluginManifest";

// ---- Z-54 分享信封 ----
describe("shareFormat", () => {
  it("seal → open 往返一致", () => {
    const env = sealEnvelope("keymap", { a: 1 });
    const r = openEnvelope<typeof env.payload>(JSON.stringify(env), (p) => Object.keys(p).length);
    expect(r.ok).toBe(true);
    if (r.ok) expect(r.summary).toContain("1 项");
  });

  it("篡改 payload 会被 checksum 拒绝", () => {
    const env = sealEnvelope("layout", { x: 1 });
    const bad = { ...env, payload: { x: 2 } };
    const r = openEnvelope(JSON.stringify(bad));
    expect(r.ok).toBe(false);
    if (!r.ok) expect(r.error).toContain("校验和不匹配");
  });

  it("坏 JSON / 未知 kind / 缺字段全部拦截", () => {
    expect(openEnvelope("not json").ok).toBe(false);
    expect(openEnvelope(JSON.stringify({ kind: "evil", version: 1, payload: {}, checksum: "x" })).ok).toBe(false);
    expect(openEnvelope(JSON.stringify({ kind: "theme", payload: {} })).ok).toBe(false);
  });
});

// ---- Z-50 主题令牌 ----
describe("themeToken", () => {
  it("合法主题通过", () => {
    const r = validateTheme({
      tokens: { "--v-color-bg": "#0b1122", "--v-color-fg": "#dfe7f5", "--v-color-accent": "#6f8fd8" },
    });
    expect(r.ok).toBe(true);
  });

  it("缺必填 / 非法色 / 低对比全部拦截", () => {
    expect(validateTheme({ tokens: {} }).ok).toBe(false);
    expect(validateTheme({ tokens: { "--v-color-bg": "#000", "--v-color-fg": "zzz", "--v-color-accent": "#fff" } }).ok).toBe(false);
    const low = validateTheme({ tokens: { "--v-color-bg": "#777777", "--v-color-fg": "#888888", "--v-color-accent": "#ffffff" } });
    expect(low.ok).toBe(false);
    expect(low.items.some((i) => i.message.includes("对比度"))).toBe(true);
  });

  it("非法变量名报 error，未知变量报 warning", () => {
    const r = validateTheme({ tokens: { "--v-color-bg": "#000000", "--v-color-fg": "#ffffff", "--v-color-accent": "#ffffff", "color-x": "#123456", "--v-unknown-x": "1" } });
    expect(r.ok).toBe(false);
    expect(r.items.some((i) => i.message.includes("color-x"))).toBe(true);
    expect(r.items.some((i) => i.level === "warning" && i.message.includes("--v-unknown-x"))).toBe(true);
  });

  it("WCAG 对比度基准：黑白 = 21:1", () => {
    expect(contrastRatio("#ffffff", "#000000")).toBeCloseTo(21, 0);
  });
});

// ---- Z-55 连接器 ----
describe("connectors", () => {
  it("coerce 合法条目", () => {
    const c = coerceConnector({ id: "a", path: "C:/x.json", kind: "json", refreshSec: 30 });
    expect(c).not.toBeNull();
    expect(c?.refreshSec).toBe(30);
  });

  it("坏 kind / 坏 refresh 被拒或归零", () => {
    expect(coerceConnector({ id: "a", path: "x", kind: "exe", refreshSec: 10 })).toBeNull();
    expect(coerceConnector({ id: "a", path: "x", kind: "json", refreshSec: 3 })?.refreshSec).toBe(0);
  });

  it("normalizeRefresh：0（手动）或 ≥10 且 ≤3600", () => {
    expect(normalizeRefresh(5)).toBe(0);
    expect(normalizeRefresh(10)).toBe(10);
    expect(normalizeRefresh(99999)).toBe(3600);
  });
});

// ---- Z-56 兼容矩阵 ----
describe("ecoMatrix", () => {
  it("矩阵自检通过（条目齐全、区间合法）", () => {
    expect(matrixSelfCheck()).toEqual([]);
  });

  it("当前格式版本 → supported", () => {
    for (const kind of ["theme", "script", "config"] as const) {
      expect(checkEcoCompat(kind, entryOf(kind).formatVersion).verdict).toBe("supported");
    }
  });

  it("过旧 / 过新 → unsupported", () => {
    expect(checkEcoCompat("theme", "0.1").verdict).toBe("unsupported");
    expect(checkEcoCompat("theme", "99.0").verdict).toBe("unsupported");
  });
});

// ---- Z-52 插件清单 ----
describe("pluginManifest", () => {
  const good = {
    id: "demo-clock",
    name: "Demo Clock",
    version: "1.0.0",
    engine: ">=1.5 <2",
    permissions: ["ui.notify"],
    entry: "main.js",
  };

  it("合法清单通过", () => {
    expect(validatePluginManifest(good).ok).toBe(true);
  });

  it("未知权限 / 坏 id / 坏 version / 绝对路径 entry 全部拦截", () => {
    expect(validatePluginManifest({ ...good, permissions: ["fs.raw"] }).ok).toBe(false);
    expect(validatePluginManifest({ ...good, id: "Bad_ID" }).ok).toBe(false);
    expect(validatePluginManifest({ ...good, version: "1.0" }).ok).toBe(false);
    expect(validatePluginManifest({ ...good, entry: "C:/evil/main.js" }).ok).toBe(false);
    expect(validatePluginManifest({ ...good, entry: "../escape.js" }).ok).toBe(false);
  });

  it("零权限给 warning 不拦截；缺字段整体拒绝", () => {
    const r = validatePluginManifest({ ...good, permissions: [] });
    expect(r.ok).toBe(true);
    expect(r.items.some((i) => i.level === "warning")).toBe(true);
    expect(validatePluginManifest({ id: "x" }).ok).toBe(false);
  });
});
