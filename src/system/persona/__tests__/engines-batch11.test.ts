import { describe, expect, it } from "vitest";
import { packAssets, unpackAssets, validatePath, spriteEntryPair, USE_PREFIX } from "../asset-package";
import { encodePng, rasterPointer } from "../png-encode";
import { DPI_SCALES, toPhysical, adaptForDpi, snapZoneFor, snapRect, clampToMinimum, pullBackToScreen, rescaleForDpi } from "../window-metrics";
import { PERF_BUDGETS, verdictFor, budgetAlert, regressionCheck } from "../perf-budget";

const pngOf = (size: number): Uint8Array => encodePng(rasterPointer("arrow", { size, color: "#6e7fd4", outline: "#ffffff", aaPx: 1 }));

// ---------- asset-package ----------

describe("asset-package · vxtheme 资产包（F127/F156 深化）", () => {
  const entries = spriteEntryPair("arrow", 24, pngOf);

  it("打包：清单逐条 CRC、PNG 签名门禁、字节预算", () => {
    const packed = packAssets(entries, 1000, 1024 * 1024);
    expect(packed.pkg.manifest).toHaveLength(2);
    expect(packed.totalBytes).toBeGreaterThan(0);
    expect(packed.withinBudget).toBe(true);
    expect(packAssets(entries, 1000, 10).withinBudget).toBe(false);
    const fake = [{ ...entries[0]!, png: new Uint8Array([1, 2, 3]) }];
    expect(() => packAssets(fake, 0)).toThrow(/不是 PNG/);
  });

  it("解包三重校验：合法通过、CRC 篡改拒、清单/数据不一致拒", () => {
    const packed = packAssets(entries, 1000);
    const ok = unpackAssets(structuredClone(packed.pkg));
    expect(ok.ok).toBe(true);
    if (ok.ok) expect(ok.entries.map((e) => e.path)).toEqual(entries.map((e) => e.path));
    const tampered = structuredClone(packed.pkg);
    tampered.manifest[0]!.checksum = 123;
    const bad = unpackAssets(tampered);
    expect(bad.ok).toBe(false);
    if (!bad.ok) expect(bad.badPaths).toContain("pointer/arrow@1x.png");
    const broken = { format: "vxtheme-assets", version: 1, manifest: [packed.pkg.manifest[0]], blobs: [] };
    expect(unpackAssets(broken).ok).toBe(false);
    expect(unpackAssets({ format: "other", version: 1 }).ok).toBe(false);
  });

  it("路径契约：use 前缀 + .png 后缀（整理不乱）", () => {
    expect(validatePath("pointer-sprite", "pointer/arrow@2x.png")).toBe(true);
    expect(validatePath("pointer-sprite", "icons/arrow.png")).toBe(false);
    expect(validatePath("boot-frame", "boot/f0.png")).toBe(true);
    expect(Object.keys(USE_PREFIX)).toHaveLength(5);
  });

  it("双倍率条目对：1x/2x 尺寸与命名契约", () => {
    expect(entries.map((e) => e.path)).toEqual(["pointer/arrow@1x.png", "pointer/arrow@2x.png"]);
    expect(entries[1]!.width).toBe(48);
  });
});

// ---------- window-metrics ----------

describe("window-metrics · 多屏与窗口几何（F168/F237 深化）", () => {
  const screen = { x: 0, y: 0, w: 1920, h: 1080, workTop: 0, workBottom: 1048 };

  it("DPI 四档：逻辑→物理取整到网格、sprite 选档", () => {
    expect(toPhysical(48, "100%")).toBe(48);
    expect(toPhysical(48, "150%")).toBe(72);
    expect(toPhysical(33, "125%")).toBe(41); // 四舍五入。
    expect(adaptForDpi(48, "200%").spriteTier).toBe(2);
    expect(adaptForDpi(48, "100%").spriteTier).toBe(1);
    expect(DPI_SCALES["200%"]).toBe(2);
  });

  it("贴边落点：八落点全判定（8px 触发带）", () => {
    expect(snapZoneFor(4, 500, screen)).toBe("left");
    expect(snapZoneFor(1916, 500, screen)).toBe("right");
    expect(snapZoneFor(960, 4, screen)).toBe("top-max");
    expect(snapZoneFor(4, 4, screen)).toBe("top-left");
    expect(snapZoneFor(1916, 1044, screen)).toBe("bottom-right");
    expect(snapZoneFor(960, 500, screen)).toBe("none");
  });

  it("贴边矩形：半屏/四分走工作区（任务栏 32px 永不遮）", () => {
    const left = snapRect("left", screen)!;
    expect(left.w).toBe(960);
    expect(left.y + left.h).toBe(1048); // 工作区底（不是 1080）。
    const bl = snapRect("bottom-left", screen)!;
    expect(bl.y).toBe(524);
    expect(bl.h).toBe(524);
    expect(snapRect("none", screen)).toBeNull();
  });

  it("最小尺寸钳制：三档降级（compact/scroll 判据有界）", () => {
    expect(clampToMinimum({ w: 800, h: 600 }, { w: 500, h: 300 }).degraded).toBe("none");
    const slight = clampToMinimum({ w: 520, h: 290 }, { w: 500, h: 300 });
    expect(slight.degraded).toBe("compact");
    const heavy = clampToMinimum({ w: 300, h: 150 }, { w: 500, h: 300 });
    expect(heavy.degraded).toBe("scroll");
    expect(heavy.w).toBe(500);
    expect(heavy.h).toBe(300);
  });

  it("显示器热切换拉回：超界夹回、未界原样（<1px 恢复契约）", () => {
    const mem = { id: "w", rect: { x: 1500, y: 200, w: 800, h: 600, workTop: 0, workBottom: 1048 }, dpiTier: "100%" as const };
    const small = { x: 0, y: 0, w: 1366, h: 768, workTop: 0, workBottom: 736 };
    const pulled = pullBackToScreen(mem, small);
    expect(pulled.adjusted).toBe(true);
    expect(pulled.rect.x).toBeLessThanOrEqual(1366 - 200);
    expect(pulled.rect.y + pulled.rect.h).toBeLessThanOrEqual(736 + 80); // 底部留 80 可见。
    const same = pullBackToScreen(mem, { ...screen });
    expect(same.adjusted).toBe(false);
  });

  it("DPI 变更重排：逻辑不变物理随档；档位相同零操作", () => {
    const mem = { id: "w", rect: { x: 100, y: 100, w: 800, h: 600, workTop: 0, workBottom: 1048 }, dpiTier: "100%" as const };
    const r = rescaleForDpi(mem, "200%");
    expect(r.rect.w).toBe(1600);
    expect(r.note).toContain("2.00");
    expect(rescaleForDpi(mem, "100%").note).toContain("未变");
  });
});

// ---------- perf-budget ----------

describe("perf-budget · 性能预算台账（八章/F061 深化）", () => {
  const samples = (mult: number): { page: string; metric: "interaction-p95-ms"; value: number; at: number }[] =>
    Array.from({ length: 20 }, (_, i) => ({ page: "ime", metric: "interaction-p95-ms" as const, value: (12 + (i % 5)) * mult, at: i }));

  it("预算表：主册硬线登记（ime 16ms / lock 2s / icons 2s 一处一事实）", () => {
    const ime = PERF_BUDGETS.find((b) => b.page === "ime")!;
    expect(ime.limit).toBe(16);
    expect(PERF_BUDGETS.find((b) => b.page === "lock")!.limit).toBe(2000);
  });

  it("P95 判定：预算内绿、超线给 overrun 分度（≤10% 黄 / >10% 红）", () => {
    const ok = verdictFor(samples(1)).verdicts[0]!;
    expect(ok.within).toBe(true);
    expect(ok.p95).toBeLessThanOrEqual(16);
    const bad = verdictFor(samples(2)).verdicts[0]!;
    expect(bad.within).toBe(false);
    expect(budgetAlert(bad).tone).toBe("danger");
    expect(budgetAlert(bad).next).toContain("回归");
    const mild = verdictFor([...samples(1), { page: "ime", metric: "interaction-p95-ms" as const, value: 17.3, at: 999 }]).verdicts[0]!;
    if (!mild.within) expect(budgetAlert(mild).tone).toBe("warn");
  });

  it("未登记采样显性拒绝（预算先于采样——不许先跑后定线）", () => {
    const r = verdictFor([{ page: "unheard", metric: "frame-ms", value: 1, at: 0 }]);
    expect(r.verdicts).toHaveLength(0);
    expect(r.unregistered).toEqual(["unheard:frame-ms"]);
  });

  it("回归对比（F061 口径）：劣化 >10% 且超线 = 红检出", () => {
    const prev = samples(1);
    const same = regressionCheck(prev, samples(1));
    expect(same[0]!.regressed).toBe(false);
    const worse = regressionCheck(prev, samples(2));
    expect(worse[0]!.regressed).toBe(true); // 2× 劣化且超 16ms 线。
  });
});
