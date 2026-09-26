import { beforeEach, describe, expect, it } from "vitest";
import { compatibilityVerdict, migrateTokenTableV0, V0_BRIDGE } from "../compat-matrix";
import {
  setSunProvider, hasSunProvider, injectSunTimes, sunAccuracyVerdict,
  feedUsageFromRecentEngine, reconcileCounts, restorePointPathReverify,
  vxappPackDescriptor, clockDriftOk, SUN_ACCURACY_TOLERANCE_MIN,
} from "../integrations";
import { estimateArchiveSize } from "../archive-engine";
import { weatherFreshness, perfGuardAction, WEATHER_STALE_MS } from "../widgets-engine";
import { imeSkinJsonSchema, validateImeSkinPackage } from "../imeskin";
import { scanBudgetCheck, SCAN_BUDGET_MS, SCAN_BUDGET_CHARS } from "../font-engine";
import { defaultTokenTable } from "../tokens";
import { personaStore } from "../store";

beforeEach(() => {
  personaStore.reset();
  setSunProvider(null); // provider 注册不跨用例泄漏
});

describe("compat-matrix · 令牌版本兼容矩阵（F151/F161 深化）", () => {
  it("三态判定：当前版本 native / 旧版本 migratable / 缺版本戳拒绝", () => {
    expect(compatibilityVerdict(1).level).toBe("native");
    const v0 = compatibilityVerdict(0);
    expect(v0.level).toBe("migratable");
    expect(v0.degradations.length).toBeGreaterThan(0); // 降级清单必须非空——迁移不静默
    const noVer = compatibilityVerdict(undefined);
    expect(noVer.level).toBe("unsupported");
    expect(noVer.reason).toContain("版本戳");
    expect(compatibilityVerdict(99).level).toBe("unsupported");
  });

  it("v0 → v1 迁移：11 桥全接 round-trip；迁移前原始输入随行（随时可退）", () => {
    const v0Colors = Object.fromEntries([
      ["--bg-canvas", "#101018"], ["--bg-surface", "#1a1a24"], ["--bg-raised", "#22222e"],
      ["--text-primary", "#e8e8f0"], ["--text-secondary", "#a0a0b4"], ["--accent", "#ff8800"],
      ["--accent-soft", "#ff880022"], ["--success", "#5fbf8a"], ["--warn", "#d4b45f"],
      ["--danger", "#d4685f"], ["--stroke", "#8c8ca0"],
    ]);
    const r = migrateTokenTableV0({ version: 0, colors: v0Colors });
    expect(r.ok).toBe(true);
    expect(r.mapped).toHaveLength(Object.keys(V0_BRIDGE).length);
    expect(r.dropped).toHaveLength(0);
    expect(r.table!.colors["--p-accent"]).toBe("#ff8800");
    // 默认兜底：v0 没有的语义色按出厂值补齐（覆盖度报告可查）。
    expect(r.table!.colors["--p-border-subtle"]).toBe(defaultTokenTable().colors["--p-border-subtle"]);
    expect(r.source).toEqual({ version: 0, colors: v0Colors }); // 原始输入保真
  });

  it("非法色值逐项进 dropped 且不阻断其余桥接；全不可桥接诚实拒绝", () => {
    const r = migrateTokenTableV0({ version: 0, colors: { "--accent": "#ff8800", "--warn": "red", "--bogus": "#123456" } });
    expect(r.ok).toBe(true);
    expect(r.mapped).toHaveLength(1);
    expect(r.dropped).toEqual(["--warn", "--bogus"]);
    const dead = migrateTokenTableV0({ version: 0, colors: { "--bogus": "#123456" } });
    expect(dead.ok).toBe(false);
    expect(dead.reason).toContain("诚实拒绝");
  });
});

describe("integrations · F116 日落注入点（F153 集成）", () => {
  it("provider 缺席走兜底；注册后注入；今日不可用诚实降级", () => {
    const noProvider = injectSunTimes("2026-09-26");
    expect(noProvider.injected).toBe(false);
    expect(noProvider.detail).toContain("兜底");
    setSunProvider((date) => (date === "2026-09-26" ? { sunset: 1080, sunrise: 360, source: "F116城市库" } : null));
    expect(hasSunProvider()).toBe(true);
    const ok = injectSunTimes("2026-09-26");
    expect(ok.injected).toBe(true);
    expect(ok.times!.source).toBe("F116城市库");
    const missing = injectSunTimes("2026-09-27");
    expect(missing.injected).toBe(false);
    expect(missing.detail).toContain("不可用");
  });

  it("±5 分钟判据：注入值与实际值的机械对拍", () => {
    expect(SUN_ACCURACY_TOLERANCE_MIN).toBe(5);
    const a = { sunset: 1080, sunrise: 360, source: "x" };
    expect(sunAccuracyVerdict(a, { ...a, sunset: 1083, sunrise: 362 }).ok).toBe(true);
    const v = sunAccuracyVerdict(a, { ...a, sunset: 1090, sunrise: 360 });
    expect(v.ok).toBe(false);
    expect(v.sunsetDeltaMin).toBe(10);
  });
});

describe("integrations · F072 计数灌入与两源对账（F167 集成）", () => {
  it("批量灌入：权威源覆盖同日计数；非法计数全清洗并逐条给因", () => {
    const ring: Record<string, { day: string; count: number }[]> = { open: [{ day: "2026-09-26", count: 3 }] };
    const r = feedUsageFromRecentEngine(
      ring,
      [
        { itemId: "open", count: 7, day: "2026-09-26" },   // 覆盖
        { itemId: "copy", count: 2 },                       // 新增（默认今天）
        { itemId: "bad", count: -1 },                       // 拒绝
        { itemId: "flood", count: 99999 },                  // 拒绝（护栏）
      ],
      "2026-09-26",
    );
    expect(r.accepted).toBe(2);
    expect(r.rejected).toBe(2);
    expect(r.rejectedReasons).toHaveLength(2);
    expect(ring.open).toEqual([{ day: "2026-09-26", count: 7 }]);
    expect(ring.copy).toEqual([{ day: "2026-09-26", count: 2 }]);
  });

  it("两源对账：差异逐条定位；一致则零 diff", () => {
    const ring: Record<string, { day: string; count: number }[]> = {
      a: [{ day: "2026-09-25", count: 2 }, { day: "2026-09-26", count: 3 }],
      b: [{ day: "2026-09-26", count: 1 }],
    };
    expect(reconcileCounts(ring, { a: 5, b: 1 }).consistent).toBe(true);
    const rec = reconcileCounts(ring, { a: 5, b: 4, c: 9 });
    expect(rec.consistent).toBe(false);
    expect(rec.diffs).toEqual([
      { itemId: "b", local: 1, remote: 4 },
      { itemId: "c", local: 0, remote: 9 },
    ]);
  });
});

describe("integrations · F121 双路保险 + F127 直通 + F187 时钟（F170/F161/F163 集成）", () => {
  it("双路复测：两路绿且结论一致；单路分叉被检出", () => {
    const before = { tier: "full", list: [1, 2, 3] };
    const good = restorePointPathReverify(before, before, before);
    expect(good.bothPathsAgree).toBe(true);
    expect(good.detail).toContain("双路一致");
    const forked = restorePointPathReverify(before, { tier: "full", list: [1, 2] }, before);
    expect(forked.directPathOk).toBe(false);
    expect(forked.restorePointPathOk).toBe(true);
    expect(forked.bothPathsAgree).toBe(false);
    expect(forked.detail).toContain("分叉");
  });

  it("vxapp 打包声明：配置+资产引用两件套（不含可执行——开放格式宪法）", () => {
    const d = vxappPackDescriptor("我的桌面人格");
    expect(d.packId).toBe("persona.我的桌面人格");
    expect(d.contents).toBe("vxtheme-profile");
    expect(d.layout.some((l) => l.kind === "config")).toBe(true);
    expect(d.layout.some((l) => l.kind === "asset-ref")).toBe(true);
  });

  it("时钟强一致：1s 容差边界判定", () => {
    expect(clockDriftOk(1000, 1500)).toBe(true);
    expect(clockDriftOk(1000, 2500)).toBe(false);
    expect(clockDriftOk(1000, 3000, 2000)).toBe(true);
  });
});

describe("archive-engine · 包体积预估（F161 深化）", () => {
  it("字节数与人话刻度 + 资产引用计数", () => {
    const pkg = {
      format: "vxtheme-profile", version: 1,
      meta: { name: "t", createdAt: "2026-09-26T00:00:00Z", version: 1 },
      sections: {
        theme: { a: "assets/wall.png", b: "asset-ref:pointer.cur", c: "普通值" },
        pointer: { scheme: "assets/p.cur" },
      },
    } as unknown as Parameters<typeof estimateArchiveSize>[0];
    const est = estimateArchiveSize(pkg);
    expect(est.bytes).toBeGreaterThan(0);
    expect(est.label).toMatch(/B|KB/);
    expect(est.assetRefs).toBe(3);
  });
});

describe("widgets-engine · 断网降级与性能执法（F163 深化）", () => {
  it("天气保鲜：实时/超龄「数据截至」/无数据三态", () => {
    const now = Date.now();
    expect(weatherFreshness(now - 1000, now).stale).toBe(false);
    const stale = weatherFreshness(now - WEATHER_STALE_MS - 60_000, now);
    expect(stale.stale).toBe(true);
    expect(stale.label).toContain("数据截至");
    const none = weatherFreshness(null, now);
    expect(none.stale).toBe(true);
    expect(none.label).toContain("暂无数据");
  });

  it("性能保护：预算内全速 / 超预算降透明 ×0.6 / 关闭不干预", () => {
    expect(perfGuardAction(2.0, 3.3, true).triggered).toBe(false);
    const over = perfGuardAction(4.1, 3.3, true);
    expect(over.triggered).toBe(true);
    expect(over.factor).toBe(0.6);
    expect(over.detail).toContain("降透明");
    const off = perfGuardAction(9.9, 3.3, false);
    expect(off.triggered).toBe(false);
    expect(off.detail).toContain("不干预");
  });
});

describe("imeskin · F126 增节 schema 与皮肤包校验（F166 深化）", () => {
  it("schema 生成：四色 hex + 档位枚举全在位", () => {
    const s = imeSkinJsonSchema();
    expect(s.$id).toBe("varix:persona:ime-skin:v1");
    const props = s.properties as Record<string, { enum?: unknown[]; minimum?: number; maximum?: number }>;
    expect(props.candidates!.enum).toEqual([5, 9]);
    expect(props.fontSize!.minimum).toBe(12);
    expect(props.opacity!.minimum).toBe(0.6);
    expect((props.colors as { properties: Record<string, unknown> }).properties.highlight).toBeTruthy();
  });

  it("皮肤包快检：合法通过；四类非法逐条指出", () => {
    expect(validateImeSkinPackage({
      followTheme: false, fontFamily: null, fontSize: 14, opacity: 0.9, candidates: 9,
      colors: { background: "#222230", text: "#e8e8f0", highlight: "#4a5fc1", highlightText: "#ffffff" },
    }).ok).toBe(true);
    const bad = validateImeSkinPackage({
      followTheme: "yes", fontSize: 20, opacity: 0.3, candidates: 7,
      colors: { background: "red", text: "#e8e8f0", highlight: "#4a5fc1", highlightText: "#ffffff" },
    });
    expect(bad.ok).toBe(false);
    expect(bad.issues.length).toBeGreaterThanOrEqual(4);
  });
});

describe("font-engine · 扫描预算判定（F159 深化）", () => {
  it("3500 字 <100ms 线：归一化对拍与超预算建议", () => {
    expect(SCAN_BUDGET_CHARS).toBe(3500);
    expect(SCAN_BUDGET_MS).toBe(100);
    const ok = scanBudgetCheck(50, 3500);
    expect(ok.ok).toBe(true);
    expect(ok.normalizedMs).toBeCloseTo(50);
    const over = scanBudgetCheck(40, 700); // 折算 200ms
    expect(over.ok).toBe(false);
    expect(over.detail).toContain("分批后台扫");
    expect(scanBudgetCheck(10, 0).detail).toContain("空字符集");
  });
});
