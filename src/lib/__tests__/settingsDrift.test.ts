/**
 * AI-20 质量门禁与收官组 — M-81 设置漂移测试（Settings Drift Tests）。
 *
 * fixtures/settings/v{n}.json = 每个历史版本的匿名化快照；
 * 逐级迁移到当前（coerceSettings）→ 断言结果。
 * 纪律：新版本必须新增 fixtures/settings/v{n+1}.json 并在 VERSIONS 数组登记，
 * 缺失即本测试红（矩阵锁死每个历史用户的升级路径）。
 * 后端 DB 迁移矩阵（cargo test settings_drift）与本测试双侧覆盖。
 */
import { describe, it, expect } from "vitest";
import { coerceSettings, DEFAULT_SETTINGS } from "../settings";

/**
 * fixtures/settings/v{n}.json（vite JSON import + import.meta.glob 枚举）。
 * 矩阵登记表：fixtures 目录里的每个 v*.json 都必须在此登记（双向锁）。
 */
const VERSIONS = ["v1.0", "v1.1", "v2.0"];
const FIXTURES = import.meta.glob("../../../fixtures/settings/*.json", { eager: true }) as Record<
  string,
  Record<string, unknown>
>;

function loadFixture(v: string): Record<string, string> {
  const mod = FIXTURES[`../../../fixtures/settings/${v}.json`];
  if (!mod) throw new Error(`fixture missing: ${v}`);
  const raw = (mod as { default?: Record<string, unknown> }).default ?? mod;
  const out: Record<string, string> = {};
  for (const [k, val] of Object.entries(raw)) {
    if (k.startsWith("//")) continue; // 注释键不计
    out[k] = typeof val === "string" ? val : JSON.stringify(val);
  }
  return out;
}

describe("AI-20 M-81：设置漂移矩阵（每版快照 → 迁移到当前）", () => {
  it("fixtures 目录与登记表双向一致（新版本漏登记即红）", () => {
    const files = Object.keys(FIXTURES)
      .map((p) => p.split("/").pop()?.replace(/\.json$/, "") ?? "")
      .filter((f) => /^v[\d.]+$/.test(f))
      .sort();
    expect(files).toEqual([...VERSIONS].sort());
  });

  for (const v of VERSIONS) {
    it(`${v} → 当前版：迁移成功且不变量成立`, () => {
      const s = coerceSettings(loadFixture(v));
      // 不变量：数值全部落在安全区间（坏值被清洗而非穿透）
      expect(s.fontSize).toBeGreaterThanOrEqual(12);
      expect(s.fontSize).toBeLessThanOrEqual(26);
      expect(s.editorWidthPct).toBeGreaterThanOrEqual(58);
      expect(s.editorWidthPct).toBeLessThanOrEqual(72);
      expect(s.autosaveDelayMs).toBeGreaterThanOrEqual(300);
      expect(s.uiZoom).toBeGreaterThanOrEqual(0.8);
      expect(s.uiZoom).toBeLessThanOrEqual(1.5);
      expect(s.nightLight).toBeLessThanOrEqual(70);
      // 不变量：枚举合法
      expect(["zh", "zh-TW", "en"]).toContain(s.language);
      expect(s.altTabFilter === "off" || s.altTabFilter === "app" || s.altTabFilter === "monitor").toBe(true);
      // 不变量：未知/废弃键被忽略（deprecatedRemovedKey 不产生任何字段）
      expect((s as unknown as Record<string, unknown>).deprecatedRemovedKey).toBeUndefined();
    });
  }

  it("v1.0（最老快照）：全部现代键回落默认（升级路径零事故）", () => {
    const s = coerceSettings(loadFixture("v1.0"));
    expect(s.wizardDone).toBe(true); // 快照已有键保留
    expect(s.oobeDone).toBe(DEFAULT_SETTINGS.oobeDone); // 新键 = 默认
    expect(s.inputFeel).toEqual(DEFAULT_SETTINGS.inputFeel);
    expect(s.notifyRetentionDays).toBe(DEFAULT_SETTINGS.notifyRetentionDays);
  });

  it("v1.1（坏值快照）：越界值全部被 clamp/回落（人为破坏迁移被测试拦截）", () => {
    const s = coerceSettings(loadFixture("v1.1"));
    expect(s.fontSize).toBeLessThanOrEqual(26); // 999 → clamp
    expect(s.editorWidthPct).toBeLessThanOrEqual(72); // 999 → clamp
    expect(s.lineHeight).toBeLessThanOrEqual(2.4); // 9.9 → clamp
    expect(s.autosaveDelayMs).toBeGreaterThanOrEqual(300); // -5 → clamp
    expect(s.uiZoom).toBeLessThanOrEqual(1.5); // 9 → clamp
    expect(s.nightLight).toBeLessThanOrEqual(70); // 200 → clamp
    expect(s.mindDefaults.wasdSpeed).toBeLessThanOrEqual(1200); // 99999 → clamp
  });

  it("JSON 坏值（customBg 损坏）→ 保留默认而非抛错", () => {
    const s = coerceSettings({ ...loadFixture("v1.0"), customBg: "{not-json" });
    expect(s.customBg.type).toBe(DEFAULT_SETTINGS.customBg.type);
  });

  it("空对象 / 全垃圾输入 → 完整默认值", () => {
    const s = coerceSettings({});
    expect(s).toEqual(DEFAULT_SETTINGS);
    const garbage = coerceSettings({ language: "fr", theme: "nope", fontSize: "abc" });
    expect(garbage.language).toBe("zh"); // 非法语言回落
    expect(garbage.fontSize).toBe(DEFAULT_SETTINGS.fontSize);
  });
});
