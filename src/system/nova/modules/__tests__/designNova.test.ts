/**
 * NOVA-200 · S15 UI 度量路（AI-15）单测 —— designNova（W-176…W-187）。
 *
 * 覆盖：manifest 契约（12 项连续/双语/默认档与 S0 一致）+ 十二项纯逻辑 +
 * 行为层非 DOM 安全性。验收口径：
 * - W-176 三比例即时切换；参考线随制式变化；
 * - W-177 跑调判定=偏离基准（中位数）30%；
 * - W-178 同类时长差 ≤ 20% 过关、曲线不统一记问题、位移 3 倍离群；
 * - W-179 三代年代循环 + 属性值映射；
 * - W-180 轨道切分与 CSS Grid 参数 1:1；
 * - W-181 三旋钮钳制、压盘重名覆盖与 ≤12 封顶；
 * - W-182 重心偏移阈值（≤0.15 均衡 / >0.3 失衡）；
 * - W-183 五场景连续性（角度 >15° / 大小 >2px 跳变）+ 缺场景如实标注；
 * - W-184 匿名坐标 30 天滚动 + 封顶 + 五档热度；
 * - W-185 >5s 半速 / >10s 保释暂停；
 * - W-186 三维湿度打分与潮湿线；
 * - W-187 WCAG 黑白 21:1、正文 4.5 / 大字 3.0、白名单降误报、不可解析如实跳过。
 */

import { describe, expect, it } from "vitest";
import {
  BALANCE_OFF,
  BALANCE_SLIGHT,
  BAIL_HALF_MS,
  BAIL_PAUSE_MS,
  CURSOR_DEG_TOL,
  CURSOR_PX_TOL,
  CURSOR_SCENES,
  DENSITY_DAMP,
  DENSITY_PAGE_CAP,
  DESIGN_NOVA_FEATURES,
  HEAT_BUCKET_PX,
  HEAT_CAP,
  HEAT_KEEP_MS,
  MUSEUM_ERAS,
  MUSEUM_RETURN_MS,
  MOTION_MS_TOLERANCE,
  MOTION_PX_OUTLIER,
  OFFKEY_PCT,
  PHI_RATIOS,
  SPECIMEN_SCENES,
  TYPE_KNOB_DEF,
  TYPE_PRESET_MAX,
  activateDesignNova,
  aaNeed,
  bailClass,
  bailOf,
  balanceReport,
  clamp,
  contrastRatio,
  cursorAudit,
  deactivateDesignNova,
  densityAudit,
  densityOf,
  designNovaDomain,
  eraAttr,
  evictHeat,
  exportSymphony,
  flagOn,
  heatCells,
  iconBalance,
  isDesignNovaActive,
  isLargeText,
  motionScore,
  nextEra,
  nextPhiMode,
  normalizeKnobs,
  parseHex,
  phiLines,
  recordHeat,
  relLum,
  savePreset,
  sentinelReport,
  spanLabel,
  symphonyOf,
  typographyCss,
  trackCells,
  xrayCells,
} from "../designNova";
import type { ContrastNode, CursorSceneSample, DensityPage, IconSample, MotionEntry, TypePreset } from "../designNova";

const NOW = Date.UTC(2026, 8, 10, 12, 0, 0); // 2026-09-10 12:00 UTC

// ---------------------------------------------------------------------------
// manifest：12 项齐、编号连续、hub 消费契约
// ---------------------------------------------------------------------------

describe("designNova manifest", () => {
  it("W-176…W-187 共 12 项，编号连续无缺", () => {
    expect(DESIGN_NOVA_FEATURES).toHaveLength(12);
    const ids = DESIGN_NOVA_FEATURES.map((f) => Number(f.id.slice(2)));
    for (let i = 1; i < ids.length; i++) expect(ids[i]).toBe(ids[i - 1]! + 1);
    expect(ids[0]).toBe(176);
    expect(ids[ids.length - 1]).toBe(187);
  });

  it("每项必含中英标题/描述/降级说明", () => {
    for (const f of DESIGN_NOVA_FEATURES) {
      expect(f.titleZh.length).toBeGreaterThan(0);
      expect(f.titleEn.length).toBeGreaterThan(0);
      expect(f.descZh.length).toBeGreaterThan(0);
      expect(f.degrade.length).toBeGreaterThan(0);
    }
    expect(designNovaDomain.id).toBe("S15");
    expect(designNovaDomain.route).toBe("AI-15");
    expect(designNovaDomain.nameZh).toBe("UI 设计与优化");
  });

  it("默认档与 S0 注册表口径一致（W-176/177/179/180 默认关，其余默认开）", () => {
    const off = ["W-176", "W-177", "W-179", "W-180"];
    for (const f of DESIGN_NOVA_FEATURES) {
      expect(f.defaultOn).toBe(off.includes(f.id) ? false : true);
    }
  });
});

// ---------------------------------------------------------------------------
// W-176 黄金比实验室
// ---------------------------------------------------------------------------

describe("W-176 黄金比实验室", () => {
  it("三制式齐备：黄金分割/三分线/根号矩形", () => {
    expect(PHI_RATIOS.map((r) => r.key)).toEqual(["phi", "thirds", "root2"]);
    expect(PHI_RATIOS[0]!.ratio).toBeCloseTo(1.618, 3);
    expect(PHI_RATIOS[2]!.ratio).toBeCloseTo(Math.SQRT2, 6);
    expect(PHI_RATIOS[1]!.ratio).toBeNull();
  });

  it("三分线：1/3 与 2/3 双轴", () => {
    const g = phiLines("thirds", 900, 600);
    expect(g.vLines).toEqual([300, 600]);
    expect(g.hLines).toEqual([200, 400]);
  });

  it("黄金分割：主分割位 w/φ 对称成对", () => {
    const PHI = (1 + Math.sqrt(5)) / 2;
    const g = phiLines("phi", 1000, 1000);
    expect(g.vLines.some((x) => Math.abs(x - 1000 / PHI) < 1e-6)).toBe(true);
    expect(g.vLines.some((x) => Math.abs(x - (1000 - 1000 / PHI)) < 1e-6)).toBe(true);
    expect(g.vLines.length).toBeGreaterThanOrEqual(2);
  });

  it("根号矩形：√2 分割线", () => {
    const g = phiLines("root2", 1000, 500);
    expect(g.vLines.some((x) => Math.abs(x - 1000 / Math.SQRT2) < 1e-6)).toBe(true);
    expect(g.hLines.some((y) => Math.abs(y - 500 / Math.SQRT2) < 1e-6)).toBe(true);
  });

  it("三比例即时循环切换（验收）", () => {
    expect(nextPhiMode("phi")).toBe("thirds");
    expect(nextPhiMode("thirds")).toBe("root2");
    expect(nextPhiMode("root2")).toBe("phi"); // 闭环
  });

  it("非法尺寸返回空网格（诚实空态）", () => {
    expect(phiLines("phi", 0, 100).vLines).toEqual([]);
    expect(phiLines("phi", -5, 0).hLines).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// W-177 白空交响
// ---------------------------------------------------------------------------

describe("W-177 白空交响", () => {
  it("基准=中位数；30% 内不跑调", () => {
    const r = symphonyOf([16, 16, 17, 18, 16]);
    expect(r.base).toBe(16);
    expect(r.offCount).toBe(0);
    expect(r.verdict).toBe("harmony");
  });

  it("偏离基准 >30% 标红跑调", () => {
    const r = symphonyOf([16, 16, 16, 32]); // 32 偏离 100%
    expect(r.offCount).toBe(1);
    expect(r.lines[3]!.offKey).toBe(true);
    expect(r.verdict).toBe("offkey");
  });

  it("恰好 30% 不算跑调（边界含）", () => {
    const base = 10;
    const edge = base * (1 + OFFKEY_PCT); // +30% 整
    expect(symphonyOf([base, edge]).offCount).toBe(0);
  });

  it("空样本如实空态；音高为 log2 音阶", () => {
    expect(symphonyOf([]).verdict).toBe("empty");
    const r = symphonyOf([8, 16]);
    expect(r.lines[0]!.pitch).toBeLessThan(r.lines[1]!.pitch);
  });

  it("报告可导出为 JSON（验收）", () => {
    const txt = exportSymphony(symphonyOf([8, 16, 32]));
    const parsed = JSON.parse(txt) as { kind: string; v: number };
    expect(parsed.kind).toBe("nova-design-symphony");
    expect(parsed.v).toBe(1);
  });
});

// ---------------------------------------------------------------------------
// W-178 动画成绩单
// ---------------------------------------------------------------------------

describe("W-178 动画成绩单", () => {
  const mk = (id: string, group: string, ms: number, curve = "ease-out", px = 24): MotionEntry => ({ id, group, ms, curve, px });

  it("同类时长差 ≤ 20% 过关", () => {
    const r = motionScore([mk("a", "drawer", 200), mk("b", "drawer", 240)]); // 20% 整
    expect(r.issues.filter((i) => i.field === "ms")).toHaveLength(0);
    expect(MOTION_MS_TOLERANCE).toBe(0.2);
  });

  it("同类时长差 > 20% 记问题项并定位", () => {
    const r = motionScore([mk("a", "drawer", 200), mk("b", "drawer", 210), mk("c", "drawer", 300)]); // 中位 210，c 偏离最大
    const ms = r.issues.find((i) => i.field === "ms");
    expect(ms).toBeDefined();
    expect(ms!.id).toBe("c"); // 定位到偏离最大者
    expect(ms!.detail).toContain("%");
  });

  it("同类曲线不统一记问题", () => {
    const r = motionScore([mk("a", "toast", 200, "ease-in"), mk("b", "toast", 210, "ease-out")]);
    const curve = r.issues.find((i) => i.field === "curve");
    expect(curve).toBeDefined();
    expect(curve!.detail).toContain("ease-in / ease-out");
  });

  it("位移 > 组中位 3 倍记离群", () => {
    const r = motionScore([mk("a", "fly", 200, "ease", 20), mk("b", "fly", 210, "ease", 20), mk("c", "fly", 215, "ease", 80)]); // 80 > 20*3
    expect(r.issues.some((i) => i.field === "px" && i.id === "c")).toBe(true);
    expect(MOTION_PX_OUTLIER).toBe(3);
  });

  it("成绩分与档位：满分为 A，问题多为 D", () => {
    expect(motionScore([mk("a", "g", 200), mk("b", "g", 205)]).grade).toBe("A");
    const bad = motionScore([
      mk("a1", "g1", 100), mk("b1", "g1", 400), mk("c1", "g1", 100, "linear"),
      mk("a2", "g2", 100), mk("b2", "g2", 400), mk("c2", "g2", 100, "linear"),
    ]);
    expect(bad.grade).toBe("D");
    expect(bad.score).toBeLessThan(70);
  });

  it("空清单如实空态（checked=0 满分语义）", () => {
    const r = motionScore([]);
    expect(r.checked).toBe(0);
    expect(r.groups).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// W-179 控件考古馆
// ---------------------------------------------------------------------------

describe("W-179 控件考古馆", () => {
  it("三代年代齐备：Win3.1 / Win95 / Win7", () => {
    expect(MUSEUM_ERAS.map((e) => e.key)).toEqual(["win31", "win95", "win7"]);
    expect(MUSEUM_ERAS[0]!.year).toBe("1992");
    expect(MUSEUM_ERAS[2]!.year).toBe("2009");
  });

  it("年代循环切换；null 起点回 Win3.1", () => {
    expect(nextEra(null)).toBe("win31");
    expect(nextEra("win31")).toBe("win95");
    expect(nextEra("win95")).toBe("win7");
    expect(nextEra("win7")).toBe("win31");
  });

  it("换装属性值映射；null = 卸妆归来", () => {
    expect(eraAttr("win95")).toBe("nova-win95");
    expect(eraAttr(null)).toBeNull();
  });

  it("10 秒自动归来常量", () => {
    expect(MUSEUM_RETURN_MS).toBe(10_000);
  });
});

// ---------------------------------------------------------------------------
// W-180 网格透视
// ---------------------------------------------------------------------------

describe("W-180 网格透视", () => {
  it("轨道切分与 CSS Grid 参数 1:1（验收）", () => {
    const cells = trackCells(300, 3, 30); // 3 列 30px 槽
    expect(cells).toHaveLength(3);
    const w = (300 - 30 * 2) / 3; // 80
    expect(cells[0]).toEqual({ x: 0, w });
    expect(cells[1]).toEqual({ x: 110, w });
    expect(cells[2]).toEqual({ x: 220, w });
  });

  it("X 光线框：列×行全覆盖 + span/gap 标注", () => {
    const rects = xrayCells({ x: 10, y: 20, w: 300, h: 120 }, 3, 2, 10);
    expect(rects).toHaveLength(6);
    expect(rects[0]!.label).toContain("span 1/3");
    expect(rects[0]!.label).toContain("gap 10px");
    expect(rects[0]!.x).toBe(10);
  });

  it("spanLabel：跨列与槽位标注", () => {
    expect(spanLabel(1, 3, 8)).toBe("span 3 · col 1–3 · gap 8px");
    expect(spanLabel(2, 2, 0)).toBe("span 1 · col 2–2 · gap 0px");
  });

  it("非法参数诚实空态", () => {
    expect(trackCells(0, 3, 10)).toEqual([]);
    expect(trackCells(100, 0, 10)).toEqual([]);
    expect(trackCells(10, 5, 100)).toEqual([]); // 槽太宽 → 列宽 ≤ 0
  });
});

// ---------------------------------------------------------------------------
// W-181 字体微唱机
// ---------------------------------------------------------------------------

describe("W-181 字体微唱机", () => {
  it("三旋钮定义：字重/字距/行高", () => {
    expect(TYPE_KNOB_DEF.weight).toMatchObject({ min: 100, max: 900, def: 400 });
    expect(TYPE_KNOB_DEF.tracking).toMatchObject({ min: -50, max: 200, def: 0 });
    expect(TYPE_KNOB_DEF.lineHeight).toMatchObject({ min: 1.0, max: 2.0, def: 1.5 });
    expect(SPECIMEN_SCENES.map((s) => s.key)).toEqual(["news", "list", "settings"]);
  });

  it("旋钮钳制：超界回界内、非法回默认", () => {
    expect(normalizeKnobs({ weight: 9999, tracking: -999, lineHeight: 9 })).toEqual({ weight: 900, tracking: -50, lineHeight: 2.0 });
    expect(normalizeKnobs({})).toEqual({ weight: 400, tracking: 0, lineHeight: 1.5 });
    expect(normalizeKnobs({ weight: Number.NaN })).toEqual({ weight: 400, tracking: 0, lineHeight: 1.5 });
  });

  it("旋钮 → CSS 片段（字距千分 em）", () => {
    const css = typographyCss({ weight: 600, tracking: 25, lineHeight: 1.4 });
    expect(css).toEqual({ fontWeight: "600", letterSpacing: "0.025em", lineHeight: "1.40" });
  });

  it("压盘：重名覆盖、≤12 封顶、名字截断", () => {
    const base: TypePreset[] = [];
    let presets = savePreset(base, { name: "紧凑", knobs: { weight: 400, tracking: -20, lineHeight: 1.3 }, savedAt: NOW });
    expect(presets).toHaveLength(1);
    presets = savePreset(presets, { name: "紧凑", knobs: { weight: 700, tracking: 10, lineHeight: 1.6 }, savedAt: NOW });
    expect(presets).toHaveLength(1); // 覆盖不重复
    expect(presets[0]!.knobs.weight).toBe(700);
    for (let i = 0; i < 20; i++) {
      presets = savePreset(presets, { name: `预设${i}`, knobs: { weight: 400, tracking: 0, lineHeight: 1.5 }, savedAt: NOW });
    }
    expect(presets.length).toBeLessThanOrEqual(TYPE_PRESET_MAX); // 封顶 12
    expect(TYPE_PRESET_MAX).toBe(12);
    const long = savePreset([], { name: "x".repeat(50), knobs: { weight: 400, tracking: 0, lineHeight: 1.5 }, savedAt: NOW });
    expect(long[0]!.name.length).toBeLessThanOrEqual(24);
  });

  it("无名预设回「未命名」不丢", () => {
    const p = savePreset([], { name: "", knobs: { weight: 400, tracking: 0, lineHeight: 1.5 }, savedAt: NOW });
    expect(p[0]!.name).toBe("未命名");
  });
});

// ---------------------------------------------------------------------------
// W-182 图标配重天平
// ---------------------------------------------------------------------------

describe("W-182 图标配重天平", () => {
  const symmetric: IconSample = {
    id: "ok",
    w: 4,
    h: 4,
    mask: [
      [0, 1, 1, 0],
      [1, 1, 1, 1],
      [1, 1, 1, 1],
      [0, 1, 1, 0],
    ],
  };
  const leftHeavy: IconSample = {
    id: "tilt",
    w: 4,
    h: 4,
    mask: [
      [1, 0, 0, 0],
      [1, 0, 0, 0],
      [1, 0, 0, 0],
      [1, 0, 0, 0],
    ],
  };

  it("对称图标 → 均衡（重心居中）", () => {
    const r = iconBalance(symmetric);
    expect(r.cx).toBeCloseTo(0.5, 5);
    expect(r.cy).toBeCloseTo(0.5, 5);
    expect(r.verdict).toBe("balanced");
    expect(r.fgRatio).toBeGreaterThan(0);
  });

  it("左重右轻 → 失衡标红（重心偏移 > 阈值）", () => {
    const r = iconBalance(leftHeavy);
    expect(r.cx).toBeCloseTo(0, 5);
    expect(r.offset).toBeGreaterThan(BALANCE_OFF);
    expect(r.verdict).toBe("off");
    expect(BALANCE_OFF).toBe(0.3);
    expect(BALANCE_SLIGHT).toBe(0.15);
  });

  it("轻偏阈值分档", () => {
    // 重心 x = 0.3 → dx = 0.4 → offset 0.4 > 0.3 失衡；x=0.45 → 0.1 轻偏以下
    const mild: IconSample = {
      id: "mild",
      w: 10,
      h: 2,
      mask: [
        [0, 0, 0, 0, 1, 1, 0, 0, 0, 0],
        [0, 0, 0, 0, 1, 1, 0, 0, 0, 0],
      ],
    };
    const r = iconBalance(mild);
    expect(r.verdict).toBe("balanced");
  });

  it("空遮罩/零尺寸 → 如实均衡空态", () => {
    expect(iconBalance({ id: "e", w: 0, h: 0, mask: [] }).fgRatio).toBe(0);
    expect(iconBalance({ id: "z", w: 4, h: 4, mask: [[0, 0], [0, 0]] }).verdict).toBe("balanced"); // 零前景
  });

  it("图标包报告：只体检计数 + 最差定位", () => {
    const r = balanceReport([symmetric, leftHeavy]);
    expect(r.checked).toBe(2);
    expect(r.flagged).toHaveLength(1);
    expect(r.worst!.id).toBe("tilt");
  });
});

// ---------------------------------------------------------------------------
// W-183 光标形影一致
// ---------------------------------------------------------------------------

describe("W-183 光标形影一致", () => {
  const scene = (s: string, shadowDeg: number, shadowPx: number, cursor = "default"): CursorSceneSample => ({ scene: s as CursorSceneSample["scene"], cursor, shadowDeg, shadowPx });
  const five = (deg = 45, px = 2): CursorSceneSample[] => CURSOR_SCENES.map((s) => scene(s, deg, px));

  it("五场景齐备常量", () => {
    expect([...CURSOR_SCENES]).toEqual(["desktop", "panel", "input", "drag", "wait"]);
    expect(CURSOR_DEG_TOL).toBe(15);
    expect(CURSOR_PX_TOL).toBe(2);
  });

  it("五场景一致 → consistent、无缺场景", () => {
    const r = cursorAudit(five());
    expect(r.verdict).toBe("consistent");
    expect(r.jumps).toHaveLength(0);
    expect(r.missing).toHaveLength(0);
    expect(r.inspected).toHaveLength(5);
  });

  it("阴影角度跳变 >15° 出具体检单 + 修复建议", () => {
    const r = cursorAudit([scene("desktop", 45, 2), scene("panel", 70, 2), scene("input", 45, 2), scene("drag", 45, 2), scene("wait", 45, 2)]);
    const jump = r.jumps.find((j) => j.field === "shadowDeg");
    expect(jump).toBeDefined();
    expect(jump!.from).toBe("desktop");
    expect(jump!.to).toBe("panel");
    expect(jump!.delta).toBe(25);
    expect(jump!.suggest).toContain("统一");
    expect(r.verdict).toBe("jumpy");
  });

  it("阴影大小跳变 >2px 记跳变；边界 2px 不记", () => {
    const r = cursorAudit([scene("desktop", 45, 2), scene("panel", 45, 4.1), scene("input", 45, 2), scene("drag", 45, 2), scene("wait", 45, 2)]);
    expect(r.jumps.some((j) => j.field === "shadowPx")).toBe(true);
    const edge = cursorAudit([scene("desktop", 45, 2), scene("panel", 45, 4)]);
    expect(edge.jumps).toHaveLength(0); // 恰 2px 不跳
  });

  it("缺场景如实标注未巡检", () => {
    const r = cursorAudit([scene("desktop", 45, 2), scene("panel", 45, 2)]);
    expect(r.missing).toEqual(["input", "drag", "wait"]);
    expect(r.verdict).toBe("consistent"); // 已检部分一致
  });

  it("不足两场景 → insufficient（不编造结论）", () => {
    expect(cursorAudit([scene("desktop", 45, 2)]).verdict).toBe("insufficient");
    expect(cursorAudit([]).verdict).toBe("insufficient");
  });
});

// ---------------------------------------------------------------------------
// W-184 UI 物理热图
// ---------------------------------------------------------------------------

describe("W-184 UI 物理热图", () => {
  it("30 天滚动剔除", () => {
    const log = [
      { x: 1, y: 1, ts: NOW - HEAT_KEEP_MS - 1 }, // 过期
      { x: 2, y: 2, ts: NOW - HEAT_KEEP_MS + 1000 }, // 保留
      { x: 3, y: 3, ts: NOW },
    ];
    const kept = evictHeat(log, NOW);
    expect(kept).toHaveLength(2);
    expect(kept.map((p) => p.x)).toEqual([2, 3]);
  });

  it("匿名坐标记录：非法坐标拒收、合法入册", () => {
    let log = recordHeat([], 100, 200, NOW);
    expect(log).toEqual([{ x: 100, y: 200, ts: NOW }]);
    log = recordHeat(log, -5, 0, NOW);
    log = recordHeat(log, Number.NaN, 10, NOW);
    expect(log).toHaveLength(1); // 负值/NaN 拒收
  });

  it("封顶滚动（20000 条）", () => {
    let log: Array<{ x: number; y: number; ts: number }> = [];
    for (let i = 0; i < HEAT_CAP + 50; i++) log = recordHeat(log, i % 800, i % 600, NOW);
    expect(log.length).toBe(HEAT_CAP);
    expect(log[0]!.x).toBe(50); // 最早的被滚出
  });

  it("桶聚合 + 五档热度（最热 L4）", () => {
    expect(HEAT_BUCKET_PX).toBe(48);
    let log: Array<{ x: number; y: number; ts: number }> = [];
    for (let i = 0; i < 10; i++) log = recordHeat(log, 10, 10, NOW); // 同桶 10 次
    log = recordHeat(log, 500, 500, NOW); // 另一桶 1 次
    const cells = heatCells(log);
    const hot = cells.find((c) => c.x === 0 && c.y === 0)!;
    const cold = cells.find((c) => c.x === Math.floor(500 / HEAT_BUCKET_PX))!;
    expect(hot.count).toBe(10);
    expect(hot.level).toBe(4);
    expect(cold.count).toBe(1);
    expect(cold.level).toBe(1); // >0 但 ≤25% → L1
  });

  it("空日志空态", () => {
    expect(heatCells([])).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// W-185 动画保释官
// ---------------------------------------------------------------------------

describe("W-185 动画保释官", () => {
  it("阈值：>5s 半速、>10s 保释暂停", () => {
    expect(BAIL_HALF_MS).toBe(5_000);
    expect(BAIL_PAUSE_MS).toBe(10_000);
    expect(bailOf(0)).toBe("full");
    expect(bailOf(4_999)).toBe("full");
    expect(bailOf(5_000)).toBe("half"); // 5s 整进半速
    expect(bailOf(9_999)).toBe("half");
    expect(bailOf(10_000)).toBe("paused"); // 10s 整进暂停
    expect(bailOf(60_000)).toBe("paused");
  });

  it("状态 → CSS 类映射", () => {
    expect(bailClass("full")).toBe("");
    expect(bailClass("half")).toBe("nova-design-bail-half");
    expect(bailClass("paused")).toBe("nova-design-bail-paused");
  });
});

// ---------------------------------------------------------------------------
// W-186 信息密度湿度计
// ---------------------------------------------------------------------------

describe("W-186 信息密度湿度计", () => {
  const page = (id: string, elements: number, viewportPx: number, textChars: number, blankPx: number): DensityPage => ({ id, elements, viewportPx, textChars, blankPx });
  const VP = 1_000_000; // 1000×1000

  it("三轴打分：元素/留白/字数 → 湿度加权", () => {
    const r = densityOf(page("p", 300, VP, 6000, 100_000)); // 每万px²: 3 元素 / 60 字；留白 10%
    expect(r.eScore).toBe(50); // 3/6*100
    expect(r.cScore).toBe(50); // 60/120*100
    expect(r.airScore).toBe(90); // 1-0.1
    expect(r.humidity).toBe(Math.round(50 * 0.4 + 90 * 0.2 + 50 * 0.4)); // 58
    expect(r.verdict).toBe("mild");
  });

  it("超密页标「潮湿」；空旷页「干爽」", () => {
    expect(DENSITY_DAMP).toBe(66);
    const damp = densityOf(page("d", 900, VP, 18_000, 10_000));
    expect(damp.humidity).toBeGreaterThan(DENSITY_DAMP);
    expect(damp.verdict).toBe("damp");
    const dry = densityOf(page("dry", 60, VP, 600, 700_000));
    expect(dry.verdict).toBe("dry");
  });

  it("走查封顶 20 页 + 最差定位", () => {
    const pages: DensityPage[] = [];
    for (let i = 0; i < 30; i++) pages.push(page(`p${i}`, 100 + i * 10, VP, 1000 + i * 400, 300_000));
    const a = densityAudit(pages);
    expect(a.pages.length).toBe(DENSITY_PAGE_CAP);
    expect(DENSITY_PAGE_CAP).toBe(20);
    expect(a.worst!.id).toBe("p19"); // 前 20 页中 i=19 最密（未饱和单调递增）
  });

  it("零视口防御（不除零）", () => {
    expect(() => densityOf(page("z", 10, 0, 10, 0))).not.toThrow();
    expect(densityOf(page("z", 10, 0, 10, 0)).humidity).toBeGreaterThanOrEqual(0);
  });
});

// ---------------------------------------------------------------------------
// W-187 对比度哨兵
// ---------------------------------------------------------------------------

describe("W-187 对比度哨兵", () => {
  it("相对亮度与对比度：黑白 21:1（WCAG 基准）", () => {
    expect(relLum([0, 0, 0])).toBe(0);
    expect(relLum([255, 255, 255])).toBeCloseTo(1, 6);
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 1);
    expect(contrastRatio("#ffffff", "#000000")).toBeCloseTo(21, 1); // 无序无关
    expect(contrastRatio("#777777", "#777777")).toBeCloseTo(1, 5); // 同色 1:1
  });

  it("parseHex：3/6 位十六进制；非法返回 null", () => {
    expect(parseHex("#fff")).toEqual([255, 255, 255]);
    expect(parseHex("#03a9f4")).toEqual([3, 169, 244]);
    expect(parseHex("red")).toBeNull();
    expect(parseHex("#12345")).toBeNull();
  });

  it("大字判定与 AA 阈值：正文 4.5 / 大字 3.0", () => {
    expect(isLargeText(24, false)).toBe(true);
    expect(isLargeText(18.66, true)).toBe(true);
    expect(isLargeText(18.66, false)).toBe(false);
    expect(isLargeText(16, true)).toBe(false);
    expect(aaNeed(16, false)).toBe(4.5);
    expect(aaNeed(24, false)).toBe(3);
    expect(aaNeed(18.66, true)).toBe(3);
  });

  it("全文巡检：不达标红框标记、定位最差", () => {
    const nodes: ContrastNode[] = [
      { id: "ok", fg: "#111111", bg: "#ffffff", fontSize: 16 }, // ~18:1 达标
      { id: "bad", fg: "#999999", bg: "#ffffff", fontSize: 16 }, // ~2.8:1 违规
      { id: "badLarge", fg: "#999999", bg: "#ffffff", fontSize: 24 }, // 大字 3:1 边缘（2.85 < 3 违规）
    ];
    const r = sentinelReport(nodes);
    expect(r.checked).toBe(3);
    expect(r.fails.map((f) => f.id).sort()).toEqual(["bad", "badLarge"]);
    expect(r.verdict).toBe("fail");
    expect(r.worst!.ratio).toBeLessThan(3);
  });

  it("装饰性文本白名单降误报（验收：误报 ≤ 2%）", () => {
    const nodes: ContrastNode[] = [
      { id: "deco", fg: "#cccccc", bg: "#ffffff", fontSize: 16, decorative: true },
      { id: "real", fg: "#111111", bg: "#ffffff", fontSize: 16 },
    ];
    const r = sentinelReport(nodes);
    expect(r.whitelisted).toBe(1);
    expect(r.fails).toHaveLength(0);
    expect(r.verdict).toBe("pass");
  });

  it("不可解析颜色如实跳过（不编造）", () => {
    const r = sentinelReport([
      { id: "weird", fg: "rgb(1,2,3)", bg: "#ffffff", fontSize: 16 },
      { id: "ok", fg: "#111111", bg: "#ffffff", fontSize: 16 },
    ]);
    expect(r.checked).toBe(1); // weird 被跳过
    expect(r.verdict).toBe("pass");
  });

  it("空清单 → empty（如实空态）", () => {
    expect(sentinelReport([]).verdict).toBe("empty");
  });
});

// ---------------------------------------------------------------------------
// 通用与行为层安全（node 环境 no-op 语义）
// ---------------------------------------------------------------------------

describe("通用与行为层", () => {
  it("clamp 边界", () => {
    expect(clamp(5, 0, 10)).toBe(5);
    expect(clamp(-1, 0, 10)).toBe(0);
    expect(clamp(11, 0, 10)).toBe(10);
  });

  it("未知功能 id 一律 false（安全默认）", () => {
    expect(flagOn("W-999")).toBe(false);
  });

  it("激活/卸载在非 DOM 环境安全 no-op（幂等）", () => {
    expect(() => activateDesignNova()).not.toThrow();
    expect(isDesignNovaActive()).toBe(false);
    expect(() => deactivateDesignNova()).not.toThrow();
    expect(() => activateDesignNova()).not.toThrow();
    expect(() => deactivateDesignNova()).not.toThrow();
  });
});
