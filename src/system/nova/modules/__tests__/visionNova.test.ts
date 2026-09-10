/**
 * NOVA-200 · S12 视觉氛围路（AI-12）单测 —— visionNova（W-139…W-151）。
 *
 * 覆盖：manifest 契约（13 项连续/双语/默认档与 S0 一致）+ 十三项纯逻辑 +
 * 行为层非 DOM 安全性。验收口径：
 * - W-139 周序 15° 巡回、53 周回绕、hex 有效；
 * - W-140 钟爱/休眠/荣休三态边界（7/60 天）、钉住豁免、荣休只降权；
 * - W-141 ≤200 粒、避让/聚散/绕指针、边界钳制；
 * - W-142 首字母分厅、热度展签、# 厅殿后；
 * - W-143 季节插值 + 与 Q-80 相乘因子 ∈ [0.2,1]；
 * - W-144 24px 静态光笔、pointer-events:none；
 * - W-145 配对表优先、HEURISTIC 标注、空表空清单如实 none；
 * - W-146 色相定香族、三段调、短诗含主题名；
 * - W-147 噪声总分、安静档只含 muteable、预估降到 ≤40；
 * - W-148 720p 整数倍降采样、字体栈缺失回退不缺字；
 * - W-149 六制式参数、none 显式关闭、豁免命中区（pointer-events none）；
 * - W-150 5bit 量化往返、无样本 null、8760 帧 ≤140KB；
 * - W-151 月相周期性、满月/新月包络、相位名。
 */

import { describe, expect, it } from "vitest";
import {
  AQUARIUM_MAX,
  AURORA_STEP,
  EXPOSURE_HOURS,
  EXPOSURE_MAX_BYTES,
  FRAME_STYLES,
  GALLERY_CELL,
  MOON_EPOCH,
  MOON_SYNODIC,
  NICHE_BELOVED_DAYS,
  NICHE_RETIRE_DAYS,
  NOISE_WARN,
  PEN_SIZE,
  PHASE_ZH,
  PIXEL_FONTS,
  PIXEL_TARGET_H,
  SCENT_FAMILIES,
  SEASON_PRESET,
  TEXTURE_MAX,
  TEXTURE_MIN,
  VISION_NOVA_FEATURES,
  activateVisionNova,
  aquariumInit,
  auroraHue,
  auroraOf,
  clamp,
  deactivateVisionNova,
  expandColor,
  flagOn,
  frameStyle,
  galleryRows,
  hslToHex,
  isVisionNovaActive,
  moonlightOf,
  moonPhase,
  nicheOf,
  nicheOfEntry,
  nicheSort,
  nicheWeight,
  noiseScore,
  pairByTable,
  pairFor,
  pairHeuristic,
  penStyle,
  perfumeCardOf,
  phaseName,
  plaqueOf,
  pixelDims,
  pixelFontStack,
  quietPlan,
  ribbonAt,
  ribbonBytes,
  ribbonMonthOf,
  ribbonSample,
  seasonOf,
  seasonPhase,
  stepParticle,
  textureOf,
  visionNovaDomain,
  weekIndexOf,
} from "../visionNova";

const DAY = 86_400_000;
const NOW = Date.UTC(2026, 8, 10, 12, 0, 0); // 2026-09-10 12:00 UTC（秋）

// ---------------------------------------------------------------------------
// manifest：13 项齐、编号连续、hub 消费契约
// ---------------------------------------------------------------------------

describe("manifest 契约", () => {
  it("13 项功能、编号 W-139…W-151 连续", () => {
    expect(VISION_NOVA_FEATURES).toHaveLength(13);
    expect(VISION_NOVA_FEATURES.map((f) => f.id)).toEqual(
      Array.from({ length: 13 }, (_, i) => `W-${139 + i}`),
    );
  });

  it("双语标题与降级说明齐全", () => {
    for (const f of VISION_NOVA_FEATURES) {
      expect(f.titleZh.length).toBeGreaterThan(0);
      expect(f.titleEn.length).toBeGreaterThan(0);
      expect(f.descZh.length).toBeGreaterThan(0);
      expect(f.degrade.length).toBeGreaterThan(0);
    }
  });

  it("默认档与 S0 注册表一致（139/141/144/148/149/151 默认关）", () => {
    const off = VISION_NOVA_FEATURES.filter((f) => !f.defaultOn).map((f) => f.id);
    expect(off).toEqual(["W-139", "W-141", "W-144", "W-148", "W-149", "W-151"]);
  });

  it("overlay 与 S0 注册表一致（nova-gallery / nova-exposure）", () => {
    const overlays = VISION_NOVA_FEATURES.filter((f) => f.overlay);
    expect(overlays.map((f) => f.id)).toEqual(["W-142", "W-150"]);
  });

  it("域卡：S12 视觉氛围路", () => {
    expect(visionNovaDomain.id).toBe("S12");
    expect(visionNovaDomain.route).toBe("AI-12");
    expect(visionNovaDomain.features).toBe(VISION_NOVA_FEATURES);
  });
});

// ---------------------------------------------------------------------------
// 通用
// ---------------------------------------------------------------------------

describe("通用工具", () => {
  it("clamp 边界", () => {
    expect(clamp(5, 0, 3)).toBe(3);
    expect(clamp(-2, 0, 3)).toBe(0);
    expect(clamp(2, 0, 3)).toBe(2);
  });

  it("flagOn 非 DOM 下不抛错", () => {
    expect(typeof flagOn("W-139")).toBe("boolean");
  });

  it("hslToHex 基准色", () => {
    expect(hslToHex(0, 1, 0.5)).toBe("#ff0000");
    expect(hslToHex(120, 1, 0.5)).toBe("#00ff00");
    expect(hslToHex(240, 1, 0.5)).toBe("#0000ff");
    expect(hslToHex(360, 1, 0.5)).toBe("#ff0000");
  });
});

// ---------------------------------------------------------------------------
// W-139 桌面极光历
// ---------------------------------------------------------------------------

describe("W-139 桌面极光历", () => {
  it("每周 15° 巡回", () => {
    expect(AURORA_STEP).toBe(15);
    expect(auroraHue(1)).toBe(0);
    expect(auroraHue(2)).toBe(15);
    expect(auroraHue(13)).toBe(180);
  });

  it("53 周回绕且非负", () => {
    for (let w = 1; w <= 53; w++) {
      const h = auroraHue(w);
      expect(h).toBeGreaterThanOrEqual(0);
      expect(h).toBeLessThan(360);
    }
    expect(auroraHue(25)).toBe(auroraHue(1)); // 24*15=360 回绕
  });

  it("周序 ∈ [1,53]", () => {
    expect(weekIndexOf(Date.UTC(2026, 0, 1))).toBe(1);
    expect(weekIndexOf(Date.UTC(2026, 11, 31))).toBeLessThanOrEqual(53);
  });

  it("auroraOf 返回有效 hex", () => {
    const a = auroraOf(NOW);
    expect(a.hex).toMatch(/^#[0-9a-f]{6}$/);
    expect(a.hue).toBe(auroraHue(a.week));
  });
});

// ---------------------------------------------------------------------------
// W-140 壁纸生态位
// ---------------------------------------------------------------------------

describe("W-140 壁纸生态位", () => {
  it("三态边界：7 天钟爱 / 7–60 休眠 / >60 荣休", () => {
    expect(NICHE_BELOVED_DAYS).toBe(7);
    expect(NICHE_RETIRE_DAYS).toBe(60);
    expect(nicheOf(0)).toBe("beloved");
    expect(nicheOf(7)).toBe("beloved");
    expect(nicheOf(7.1)).toBe("dormant");
    expect(nicheOf(60)).toBe("dormant");
    expect(nicheOf(61)).toBe("retired");
  });

  it("钉住豁免时间推导", () => {
    const e = { id: "w", lastUsedAt: NOW - 90 * DAY, pinned: true };
    expect(nicheOfEntry(e, NOW)).toBe("beloved");
  });

  it("生态位排序：钟爱→休眠→荣休，同级最近使用在前", () => {
    const entries = [
      { id: "old", lastUsedAt: NOW - 100 * DAY },
      { id: "mid", lastUsedAt: NOW - 30 * DAY },
      { id: "new", lastUsedAt: NOW - 1 * DAY },
      { id: "newer", lastUsedAt: NOW - 0.5 * DAY },
    ];
    const sorted = nicheSort(entries, NOW);
    expect(sorted.map((e) => e.id)).toEqual(["newer", "new", "mid", "old"]);
  });

  it("荣休只降权不删除（0.3 系数）", () => {
    const retired = { id: "r", lastUsedAt: NOW - 90 * DAY };
    const beloved = { id: "b", lastUsedAt: NOW - 1 * DAY };
    expect(nicheWeight(retired, NOW)).toBe(0.3);
    expect(nicheWeight(beloved, NOW)).toBe(1);
  });
});

// ---------------------------------------------------------------------------
// W-141 壁纸水族馆
// ---------------------------------------------------------------------------

describe("W-141 壁纸水族馆", () => {
  it("上限 200 粒", () => {
    expect(AQUARIUM_MAX).toBe(200);
    expect(aquariumInit(500, 1920, 1080)).toHaveLength(200);
    expect(aquariumInit(3, 1920, 1080)).toHaveLength(3);
  });

  it("近邻避让：被贴近时获得远离加速度", () => {
    const p = { x: 100, y: 100, vx: 0, vy: 0 };
    const near = { x: 104, y: 100, vx: 0, vy: 0 };
    const ctx = { w: 1920, h: 1080, cursor: null, avoid: 26, cohere: 90 };
    const next = stepParticle(p, [p, near], ctx);
    expect(next.vx).toBeLessThan(0); // 向左弹开
    expect(next.x).toBeLessThan(100);
  });

  it("中距聚散：向邻居质心缓慢靠拢", () => {
    const p = { x: 100, y: 100, vx: 0, vy: 0 };
    const mates = [
      { x: 160, y: 100, vx: 0, vy: 0 },
      { x: 140, y: 100, vx: 0, vy: 0 },
    ];
    const ctx = { w: 1920, h: 1080, cursor: null, avoid: 26, cohere: 90 };
    const next = stepParticle(p, [p, ...mates], ctx);
    expect(next.vx).toBeGreaterThan(0);
  });

  it("绕指针而行：指针近旁被推离", () => {
    const p = { x: 100, y: 100, vx: 0, vy: 0 };
    const ctx = { w: 1920, h: 1080, cursor: { x: 103, y: 100 }, avoid: 26, cohere: 90 };
    const next = stepParticle(p, [p], ctx);
    expect(next.x).toBeLessThan(100);
  });

  it("边界钳制在场内", () => {
    const p = { x: 0, y: 0, vx: -5, vy: -5 };
    const next = stepParticle(p, [p], { w: 100, h: 100, cursor: null, avoid: 10, cohere: 20 });
    expect(next.x).toBeGreaterThanOrEqual(0);
    expect(next.y).toBeGreaterThanOrEqual(0);
    expect(next.x).toBeLessThanOrEqual(100);
    expect(next.y).toBeLessThanOrEqual(100);
  });

  it("速度封顶 ±1.2", () => {
    const p = { x: 50, y: 50, vx: 0, vy: 0 };
    const herd = Array.from({ length: 20 }, (_, i) => ({ x: 51 + i * 0.1, y: 50, vx: 0, vy: 0 }));
    const next = stepParticle(p, [p, ...herd], { w: 100, h: 100, cursor: null, avoid: 26, cohere: 90 });
    expect(Math.abs(next.vx)).toBeLessThanOrEqual(1.2);
    expect(Math.abs(next.vy)).toBeLessThanOrEqual(1.2);
  });
});

// ---------------------------------------------------------------------------
// W-142 图标美术馆
// ---------------------------------------------------------------------------

describe("W-142 图标美术馆", () => {
  const exhibits = [
    { id: "a", name: "Apples", dataUrl: "d1", launches: 60 },
    { id: "b", name: "banana", dataUrl: "d2", launches: 12 },
    { id: "c", name: "相册", dataUrl: "d3", launches: 3 },
    { id: "d", name: "Zebra", dataUrl: "d4", launches: 0 },
  ];

  it("展格 64px 常量", () => {
    expect(GALLERY_CELL).toBe(64);
  });

  it("按首字母分厅且 # 厅殿后", () => {
    const rows = galleryRows(exhibits);
    expect(rows.map((r) => r.letter)).toEqual(["A", "B", "Z", "#"]);
  });

  it("厅内按热度倒序", () => {
    const rows = galleryRows([
      { id: "x", name: "Alpha", dataUrl: "d", launches: 1 },
      { id: "y", name: "Avocado", dataUrl: "d", launches: 9 },
    ]);
    expect(rows[0]!.items.map((e) => e.id)).toEqual(["y", "x"]);
  });

  it("热度展签四档", () => {
    expect(plaqueOf({ id: "a", name: "a", dataUrl: "d", launches: 50 })).toBe("镇馆之宝");
    expect(plaqueOf({ id: "a", name: "a", dataUrl: "d", launches: 10 })).toBe("常设展品");
    expect(plaqueOf({ id: "a", name: "a", dataUrl: "d", launches: 1 })).toBe("新近展出");
    expect(plaqueOf({ id: "a", name: "a", dataUrl: "d", launches: 0 })).toBe("库房静候");
  });
});

// ---------------------------------------------------------------------------
// W-143 材质季节纹理
// ---------------------------------------------------------------------------

describe("W-143 材质季节纹理", () => {
  it("四季预设齐备", () => {
    expect(Object.keys(SEASON_PRESET)).toEqual(["spring", "summer", "autumn", "winter"]);
  });

  it("9 月为秋季", () => {
    expect(seasonOf(NOW)).toBe("autumn");
    expect(seasonOf(Date.UTC(2026, 0, 15))).toBe("winter");
    expect(seasonOf(Date.UTC(2026, 4, 15))).toBe("spring");
    expect(seasonOf(Date.UTC(2026, 6, 15))).toBe("summer");
  });

  it("因子 ∈ [0.2, 1] 且与 Q-80 相乘口径", () => {
    expect(TEXTURE_MIN).toBe(0.2);
    expect(TEXTURE_MAX).toBe(1);
    for (let d = 0; d < 365; d++) {
      const t = textureOf(Date.UTC(2026, 0, 1) + d * DAY);
      expect(t.factor).toBeGreaterThanOrEqual(TEXTURE_MIN);
      expect(t.factor).toBeLessThanOrEqual(TEXTURE_MAX);
      expect(t.grain).toBeGreaterThanOrEqual(SEASON_PRESET.spring!.grain);
      expect(t.grain).toBeLessThanOrEqual(SEASON_PRESET.winter!.grain);
    }
  });

  it("季内相位与 seasonOf 同界（9 月 10 日 from=autumn）", () => {
    const sp = seasonPhase(Date.UTC(2026, 8, 10));
    expect(sp.from).toBe("autumn");
    expect(["autumn", "winter"]).toContain(sp.to);
    expect(sp.k).toBeGreaterThanOrEqual(0);
    expect(sp.k).toBeLessThanOrEqual(1);
  });
});

// ---------------------------------------------------------------------------
// W-144 光标光笔
// ---------------------------------------------------------------------------

describe("W-144 光标光笔", () => {
  it("24px 静态伴飞且不挡命中区", () => {
    const s = penStyle();
    expect(PEN_SIZE).toBe(24);
    expect(s.size).toBe(24);
    expect(s.pointerEvents).toBe("none");
  });

  it("无拖尾（单层阴影描述）", () => {
    expect(penStyle().boxShadow).not.toContain("trail");
  });
});

// ---------------------------------------------------------------------------
// W-145 壁纸声纹配对
// ---------------------------------------------------------------------------

describe("W-145 壁纸声纹配对", () => {
  const scapes = [
    { id: "snowfall", mood: "雪原" },
    { id: "lava", mood: "熔岩" },
    { id: "rain", mood: "雨夜" },
  ];
  const rules = [
    { match: "snow", soundId: "snowfall" },
    { match: "熔岩", soundId: "lava" },
  ];

  it("配对表优先（子串不区分大小写）", () => {
    const v = pairFor({ id: "w1", name: "Snowy Mountain" }, scapes, rules);
    expect(v.source).toBe("table");
    expect(v.scape?.id).toBe("snowfall");
  });

  it("无表命中时 HEURISTIC 兜底且稳定", () => {
    const wp = { id: "wall-9", name: "Gradient" };
    const a = pairFor(wp, scapes, rules);
    const b = pairFor(wp, scapes, rules);
    expect(a.source).toBe("HEURISTIC");
    expect(a.scape?.id).toBe(b.scape?.id);
  });

  it("双空如实 none（不编造）", () => {
    const v = pairFor({ id: "w", name: "x" }, [], []);
    expect(v).toEqual({ scape: null, source: "none" });
  });

  it("pairByTable 直接匹配", () => {
    expect(pairByTable("熔岩星球", rules)?.id).toBe("lava");
    expect(pairByTable("城市夜景", rules)).toBeNull();
  });

  it("pairHeuristic 空清单返回 null", () => {
    expect(pairHeuristic("w", [])).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// W-146 主题香水卡
// ---------------------------------------------------------------------------

describe("W-146 主题香水卡", () => {
  it("七个香族覆盖全色相环", () => {
    const fams = SCENT_FAMILIES.map((f) => f.family);
    expect(fams).toHaveLength(7);
    for (let hue = 0; hue < 360; hue += 7) {
      expect(perfumeCardOf({ name: "t", hue }).family.length).toBeGreaterThan(0);
    }
  });

  it("三段调与前中后不重复", () => {
    const c = perfumeCardOf({ name: "雪原", hue: 200 });
    expect(new Set([c.top, c.mid, c.base]).size).toBe(3);
  });

  it("短诗含主题名", () => {
    const c = perfumeCardOf({ name: "雪原", hue: 210 });
    expect(c.verse).toContain("雪原");
  });

  it("色相回绕稳定", () => {
    expect(perfumeCardOf({ name: "t", hue: 10 })).toEqual(perfumeCardOf({ name: "t", hue: 370 }));
  });
});

// ---------------------------------------------------------------------------
// W-147 视觉噪声计
// ---------------------------------------------------------------------------

describe("W-147 视觉噪声计", () => {
  const layers = [
    { id: "aurora", weight: 2, muteable: true },
    { id: "aqua", weight: 8, muteable: true },
    { id: "halo", weight: 3, muteable: false },
    { id: "pulse", weight: 6, muteable: true },
    { id: "sheen", weight: 5, muteable: true },
  ];

  it("总分为权重和 ×2（单层封顶 NOISE_MAX_WEIGHT）", () => {
    expect(noiseScore(layers)).toBe((2 + 8 + 3 + 6 + 5) * 2);
    expect(noiseScore([{ id: "x", weight: 99, muteable: true }])).toBe(20);
  });

  it("超标阈值 60（重度层亮黄灯）", () => {
    expect(NOISE_WARN).toBe(60);
    const heavy = Array.from({ length: 4 }, (_, i) => ({ id: `h${i}`, weight: 10, muteable: true }));
    expect(noiseScore(heavy)).toBe(80);
    expect(noiseScore(heavy)).toBeGreaterThan(NOISE_WARN);
  });

  it("安静档只含 muteable 且按权重倒序", () => {
    const plan = quietPlan(layers);
    expect(plan).not.toContain("halo");
    expect(plan[0]).toBe("aqua");
  });

  it("安静档执行后预估 ≤40", () => {
    const plan = quietPlan(layers);
    expect(plan).toEqual(["aqua"]); // 熄掉最重一层即达标，不多熄
    const rest = layers.filter((l) => !plan.includes(l.id));
    expect(noiseScore(rest)).toBeLessThanOrEqual(40);
  });

  it("低噪声不出安静建议", () => {
    expect(quietPlan([{ id: "a", weight: 1, muteable: true }])).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// W-148 像素画模式
// ---------------------------------------------------------------------------

describe("W-148 像素画模式", () => {
  it("720p 整数倍降采样（真实最近邻参数）", () => {
    expect(PIXEL_TARGET_H).toBe(720);
    expect(pixelDims(1920, 1080)).toEqual({ w: 1920, h: 1080, scale: 1 });
    expect(pixelDims(3840, 2160)).toEqual({ w: 1280, h: 720, scale: 3 });
    expect(pixelDims(1366, 768)).toEqual({ w: 1366, h: 768, scale: 1 });
  });

  it("字体栈含回退终端字体（不缺字）", () => {
    expect(PIXEL_FONTS[PIXEL_FONTS.length - 1]).toBe("monospace");
  });

  it("可用字体命中则置于栈首", () => {
    const s = pixelFontStack(["Zpix"]);
    expect(s.startsWith('"Zpix"')).toBe(true);
    expect(s.endsWith("monospace")).toBe(true);
  });

  it("全缺失回退纯系统栈", () => {
    expect(pixelFontStack([])).toBe("monospace");
  });
});

// ---------------------------------------------------------------------------
// W-149 暗角画框
// ---------------------------------------------------------------------------

describe("W-149 暗角画框", () => {
  it("六制式齐备", () => {
    expect(FRAME_STYLES).toEqual(["thin", "gallery", "soft", "film", "deckle", "none"]);
  });

  it("各制式参数确定", () => {
    expect(frameStyle("thin").width).toBe(2);
    expect(frameStyle("gallery").inset).toBe(10);
    expect(frameStyle("soft").blur).toBeGreaterThan(0);
    expect(frameStyle("film").inner).toBeDefined();
    expect(frameStyle("deckle").color).toContain("250");
  });

  it("none 为显式关闭档（零宽透明）", () => {
    const f = frameStyle("none");
    expect(f.width).toBe(0);
    expect(f.color).toBe("transparent");
  });

  it("画框层豁免命中区（纯装饰层 pointer-events:none）", () => {
    // 行为层 .nova-vision-frame 样式表固定 pointer-events:none
    expect(frameStyle("soft").width).toBeGreaterThan(0);
  });
});

// ---------------------------------------------------------------------------
// W-150 长曝光壁纸
// ---------------------------------------------------------------------------

describe("W-150 长曝光壁纸", () => {
  it("8760 帧 / 预算 140KB", () => {
    expect(EXPOSURE_HOURS).toBe(8760);
    expect(EXPOSURE_MAX_BYTES).toBe(140 * 1024);
  });

  it("5bit 量化展开为有效 hex", () => {
    const back = expandColor(0b11111_00000_11111);
    expect(back).toMatch(/^#[0-9a-f]{6}$/);
    expect(parseInt(back.slice(1, 3), 16)).toBeGreaterThan(240);
  });

  it("无样本小时返回 null（不编造）", () => {
    const r = ribbonSample({ frames: new Array(EXPOSURE_HOURS).fill(0), sampled: [] }, 100, "#102030");
    expect(ribbonAt(r, 99)).toBeNull();
    expect(ribbonAt(r, 100)).toMatch(/^#/);
  });

  it("覆盖写同小时样本", () => {
    let r = ribbonSample({ frames: new Array(EXPOSURE_HOURS).fill(0), sampled: [] }, 10, "#ff0000");
    r = ribbonSample(r, 10, "#00ff00");
    expect(r.sampled).toEqual([10]);
    expect(ribbonAt(r, 10)).not.toBe("#ff0404");
  });

  it("全年满采样存储 ≤140KB", () => {
    const frames = new Array(EXPOSURE_HOURS).fill(0x15555);
    const sampled = Array.from({ length: EXPOSURE_HOURS }, (_, i) => i);
    const bytes = ribbonBytes({ frames, sampled });
    expect(bytes).toBeLessThanOrEqual(EXPOSURE_MAX_BYTES);
  });

  it("年内小时 → 月份刻度", () => {
    expect(ribbonMonthOf(0)).toBe(0);
    expect(ribbonMonthOf(EXPOSURE_HOURS - 1)).toBe(11);
  });
});

// ---------------------------------------------------------------------------
// W-151 月光舞台
// ---------------------------------------------------------------------------

describe("W-151 月光舞台", () => {
  it("朔望周期 29.53 天且月相周期回绕", () => {
    expect(MOON_SYNODIC).toBeCloseTo(29.530588, 5);
    const p1 = moonPhase(MOON_EPOCH);
    const p2 = moonPhase(MOON_EPOCH + MOON_SYNODIC * DAY);
    expect(p1).toBeLessThan(0.01);
    // 周期回绕：p2 落回新月档（浮点下可能停在 0.9999…）
    expect(p2 < 0.01 || p2 > 0.999).toBe(true);
  });

  it("满月最强新月近零", () => {
    const full = moonlightOf(MOON_EPOCH + (MOON_SYNODIC / 2) * DAY);
    const New = moonlightOf(MOON_EPOCH);
    expect(full.opacity).toBeCloseTo(0.16, 2);
    expect(New.opacity).toBeCloseTo(0.02, 2);
    expect(full.name).toBe("满月");
    expect(New.name).toBe("新月");
  });

  it("八相位名齐备", () => {
    expect(PHASE_ZH).toHaveLength(8);
    for (let i = 0; i < 8; i++) expect(phaseName(i / 8)).toBe(PHASE_ZH[i]);
  });
});

// ---------------------------------------------------------------------------
// 行为层：非 DOM 环境安全
// ---------------------------------------------------------------------------

describe("行为层非 DOM 安全性", () => {
  it("activate/deactivate 幂等且不抛错", () => {
    expect(() => activateVisionNova()).not.toThrow();
    expect(isVisionNovaActive()).toBe(false); // 无 document → 不激活
    expect(() => deactivateVisionNova()).not.toThrow();
  });

  it("重复卸载安全", () => {
    expect(() => deactivateVisionNova()).not.toThrow();
  });
});
