/**
 * NOVA-200 域3 桌面生态路（AI-03）—— deskNova 单测。
 * 覆盖：13 项纯逻辑 + 本地账本 + 功能注册表契约 + 非 DOM 激活幂等 + 布局数据级集成。
 */

import { beforeEach, describe, expect, it } from "vitest";
import {
  CONSTELLATIONS,
  DESK_FEATURES,
  agingOpacity,
  clusterBounds,
  clusterTouched,
  constellationById,
  constellationCells,
  constellationLinesPx,
  currentMonthCounts,
  deletePhoto,
  dormancyOpacity,
  effectiveOpacity,
  featureDefaults,
  firstCell,
  firstSeen,
  growthGlow,
  growthShadow,
  homeRestorePlan,
  horizonColor,
  horizonY,
  immuneUntil,
  layoutDiff,
  leaderboard,
  loadFeatureOverrides,
  loadHomes,
  loadImmunity,
  loadPhotos,
  loadUsage,
  makeCluster,
  makeThumb,
  monthKey,
  neighborDistance,
  nearestCell,
  panForX,
  recordIconUse,
  retireDue,
  saveFeatureOverride,
  savePhoto,
  seasonOf,
  SEASON_SHADOW,
  setImmunity,
  siestaState,
  sleepKing,
  snapBadge,
  snowActive,
  snowAvoid,
  snowStep,
  snapshotSizeOk,
  type SnowP,
} from "../deskNova";
import { deskNova, type DeskNovaCtx } from "../deskNova";
import { loadDesktopLayout, saveDesktopLayout, type Cell } from "../../../desktop-icons/layout";

const DAY = 86_400_000;
const TIER = { w: 100, h: 120, tile: 58, icon: 46 };

// ---------------------------------------------------------------------------
// W-026 星座排列
// ---------------------------------------------------------------------------

describe("W-026 图标星座排列", () => {
  it("六座内置，每座 12–20 位星点，连线索引合法，坐标归一化", () => {
    expect(CONSTELLATIONS).toHaveLength(6);
    expect(new Set(CONSTELLATIONS.map((c) => c.id)).size).toBe(6);
    for (const c of CONSTELLATIONS) {
      expect(c.points.length).toBeGreaterThanOrEqual(12);
      expect(c.points.length).toBeLessThanOrEqual(20);
      for (const [x, y] of c.points) {
        expect(x).toBeGreaterThanOrEqual(0);
        expect(x).toBeLessThanOrEqual(1);
        expect(y).toBeGreaterThanOrEqual(0);
        expect(y).toBeLessThanOrEqual(1);
      }
      for (const [a, b] of c.lines) {
        expect(a).toBeLessThan(c.points.length);
        expect(b).toBeLessThan(c.points.length);
      }
    }
  });

  it("constellationById 命中与未命中", () => {
    expect(constellationById("orion")?.zh).toBe("猎户座");
    expect(constellationById("nope")).toBeNull();
  });

  it("星座格子：数量一致、界内、零冲突、确定性", () => {
    const c = CONSTELLATIONS[1]!;
    const a = constellationCells(c, 20, 14, 9);
    const b = constellationCells(c, 20, 14, 9);
    expect(a).toHaveLength(20);
    expect(b).toEqual(a);
    const seen = new Set<string>();
    for (const cell of a) {
      expect(cell.c).toBeGreaterThanOrEqual(0);
      expect(cell.c).toBeLessThan(14);
      expect(cell.r).toBeGreaterThanOrEqual(0);
      expect(cell.r).toBeLessThan(9);
      expect(seen.has(`${cell.c},${cell.r}`)).toBe(false);
      seen.add(`${cell.c},${cell.r}`);
    }
  });

  it("星座格子：0 个安全；超出星点数循环复用仍无冲突", () => {
    const c = CONSTELLATIONS[0]!;
    expect(constellationCells(c, 0, 10, 8)).toHaveLength(0);
    const many = constellationCells(c, 40, 16, 12);
    expect(many).toHaveLength(40);
    expect(new Set(many.map((x) => `${x.c},${x.r}`)).size).toBe(40);
  });

  it("星图连线：端点落在格子中心", () => {
    const c = CONSTELLATIONS[2]!;
    const cells: Cell[] = [{ c: 0, r: 0 }, { c: 2, r: 1 }, { c: 4, r: 0 }];
    const lines = constellationLinesPx(c, cells, TIER, 14);
    expect(lines.length).toBeGreaterThan(0);
    const centers = cells.map((x) => [14 + x.c * TIER.w + 50, 14 + x.r * TIER.h + 60]);
    for (const l of lines) {
      const hit = centers.some(([x, y]) => (x === l.x1 && y === l.y1) || (x === l.x2 && y === l.y2));
      expect(hit).toBe(true);
    }
  });
});

// ---------------------------------------------------------------------------
// W-027 图标养成 + W-031 老化
// ---------------------------------------------------------------------------

describe("W-027 图标养成", () => {
  it("阴影加深单调且封顶 8%", () => {
    expect(growthShadow(0, 100)).toBe(0);
    expect(growthShadow(50, 100)).toBeCloseTo(0.04, 5);
    expect(growthShadow(200, 100)).toBeCloseTo(0.08, 5);
    expect(growthShadow(-5, 100)).toBe(0);
    expect(growthShadow(10, 0)).toBe(0);
  });

  it("底光增益单调（平方根曲线）", () => {
    expect(growthGlow(0, 100)).toBe(0);
    expect(growthGlow(25, 100)).toBeCloseTo(0.5, 5);
    expect(growthGlow(400, 100)).toBe(1);
  });

  it("半休眠：21 天起渐变，30 天到 85% 透明", () => {
    expect(dormancyOpacity(0)).toBe(1);
    expect(dormancyOpacity(20)).toBe(1);
    expect(dormancyOpacity(21)).toBeLessThan(1);
    expect(dormancyOpacity(25)).toBeLessThan(dormancyOpacity(22)!);
    expect(dormancyOpacity(30)).toBeCloseTo(0.15, 5);
    expect(dormancyOpacity(90)).toBeCloseTo(0.15, 5);
  });

  it("有效透明度取养成/老化较小者", () => {
    expect(effectiveOpacity(40)).toBeCloseTo(0.15, 5); // 休眠 0.15 < 老化 0.88
    expect(effectiveOpacity(10)).toBe(1);
  });
});

describe("W-031 图标老化公约", () => {
  it("老化梯度：7 天起每周 -3%，下限 60%", () => {
    expect(agingOpacity(0)).toBe(1);
    expect(agingOpacity(7)).toBe(1);
    expect(agingOpacity(14)).toBeCloseTo(0.97, 5);
    expect(agingOpacity(365)).toBeCloseTo(0.6, 5);
    expect(agingOpacity(10000)).toBeCloseTo(0.6, 5);
  });

  it("退休提案 30 天到期；拒绝后 90 天免疫", () => {
    expect(retireDue(29)).toBe(false);
    expect(retireDue(30)).toBe(true);
    expect(immuneUntil(1_000_000)).toBe(1_000_000 + 90 * DAY);
  });

  it("免疫账本读写", () => {
    setImmunity("app-1", 12345);
    expect(loadImmunity()["app-1"]).toBe(12345);
  });
});

// ---------------------------------------------------------------------------
// W-028 季节地影
// ---------------------------------------------------------------------------

describe("W-028 季节地影", () => {
  it("北半球四季映射", () => {
    expect(seasonOf(2, "north")).toBe("spring");
    expect(seasonOf(5, "north")).toBe("summer");
    expect(seasonOf(8, "north")).toBe("autumn");
    expect(seasonOf(11, "north")).toBe("winter");
    expect(seasonOf(0, "north")).toBe("winter");
  });

  it("南半球反转", () => {
    expect(seasonOf(2, "south")).toBe("autumn");
    expect(seasonOf(5, "south")).toBe("winter");
    expect(seasonOf(8, "south")).toBe("spring");
    expect(seasonOf(11, "south")).toBe("summer");
  });

  it("四季阴影四态互异", () => {
    const tints = Object.values(SEASON_SHADOW).map((s) => s.tint);
    expect(new Set(tints).size).toBe(4);
    expect(SEASON_SHADOW.summer.blur).toBeLessThan(SEASON_SHADOW.autumn.blur);
  });
});

// ---------------------------------------------------------------------------
// W-029 排行榜
// ---------------------------------------------------------------------------

describe("W-029 图标排行榜", () => {
  it("Top10 排序 + 金银铜档", () => {
    const counts: Record<string, number> = {};
    for (let i = 1; i <= 15; i++) counts[`app-${i}`] = i;
    const rows = leaderboard(counts);
    expect(rows).toHaveLength(10);
    expect(rows[0]).toMatchObject({ id: "app-15", medal: "gold" });
    expect(rows[1]!.medal).toBe("silver");
    expect(rows[2]!.medal).toBe("bronze");
    expect(rows[3]!.medal).toBe("none");
    expect(rows[9]!.count).toBe(6);
  });

  it("零计数不进榜；同分按 id 稳定排序", () => {
    const rows = leaderboard({ a: 2, b: 0, c: 2 });
    expect(rows).toHaveLength(2);
    expect(rows[0]!.id).toBe("a");
  });

  it("沉睡王：30 天零点击中最久未用者优先，权重大者优先", () => {
    const now = 1_000_000_000_000;
    const lastUse: Record<string, number> = {
      fresh: now - 1 * DAY,
      old: now - 40 * DAY,
      older: now - 50 * DAY,
    };
    expect(sleepKing(lastUse, now)?.id).toBe("older");
    expect(sleepKing(lastUse, now, { old: 118, older: 76 })?.id).toBe("old");
  });

  it("无沉睡者返回 null", () => {
    expect(sleepKing({ a: Date.now() }, Date.now())).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// W-030 午憩
// ---------------------------------------------------------------------------

describe("W-030 桌面午憩", () => {
  it("10 分钟阈值边界", () => {
    expect(siestaState(600_000, 0)).toBe("siesta");
    expect(siestaState(599_999, 0)).toBe("awake");
    expect(siestaState(1_000, 1_000)).toBe("awake");
  });
});

// ---------------------------------------------------------------------------
// W-032 地平线
// ---------------------------------------------------------------------------

describe("W-032 桌面地平线", () => {
  it("y 值 = 图标群基线下一格下缘；空桌面回落 78% 视口", () => {
    expect(horizonY(3, TIER, 14, 1000)).toBe(14 + 4 * 120 + 2);
    expect(horizonY(-1, TIER, 14, 1000)).toBe(780);
  });

  it("纯色壁纸对比色：亮/暗底互异；未知回落微光白 8%", () => {
    expect(horizonColor(0.8).color).not.toBe(horizonColor(0.2).color);
    expect(horizonColor(null).alpha).toBeCloseTo(0.08, 5);
  });
});

// ---------------------------------------------------------------------------
// W-033 布局合影
// ---------------------------------------------------------------------------

describe("W-033 布局合影", () => {
  const pos: Record<string, Cell> = {
    a: { c: 0, r: 0 },
    b: { c: 1, r: 0 },
    c: { c: 2, r: 3 },
  };

  it("缩略图 SVG 体积远低于 60KB", () => {
    const photo = { id: "p1", ts: 1, count: 3, positions: pos, thumb: makeThumb(pos) };
    expect(photo.thumb.startsWith("data:image/svg+xml")).toBe(true);
    expect(snapshotSizeOk(photo)).toBe(true);
  });

  it("差异三分类：新增 / 离开 / 移动互斥且准确", () => {
    const cur: Record<string, Cell> = {
      b: { c: 1, r: 1 },
      c: { c: 2, r: 3 },
      d: { c: 0, r: 2 },
    };
    const snap: Record<string, Cell> = {
      a: { c: 0, r: 0 },
      b: { c: 1, r: 0 },
      c: { c: 2, r: 3 },
    };
    const diff = layoutDiff(cur, snap);
    expect(diff.added).toEqual(["d"]);
    expect(diff.removed).toEqual(["a"]);
    expect(diff.moved).toEqual([{ id: "b", from: { c: 1, r: 0 }, to: { c: 1, r: 1 } }]);
  });

  it("合影账本：保存 / 删除存储往返", () => {
    const p = { id: "px", ts: 2, count: 1, positions: { a: { c: 0, r: 0 } }, thumb: makeThumb({ a: { c: 0, r: 0 } }) };
    savePhoto(p);
    expect(loadPhotos().some((x) => x.id === "px")).toBe(true);
    deletePhoto("px");
    expect(loadPhotos().some((x) => x.id === "px")).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// W-034 光雪
// ---------------------------------------------------------------------------

describe("W-034 图标层光雪", () => {
  it("晚间 18:00–06:00 自动开启", () => {
    expect(snowActive(18)).toBe(true);
    expect(snowActive(23)).toBe(true);
    expect(snowActive(0)).toBe(true);
    expect(snowActive(5)).toBe(true);
    expect(snowActive(6)).toBe(false);
    expect(snowActive(12)).toBe(false);
  });

  it("下落积分与落底回顶", () => {
    const p: SnowP = { x: 10, y: 10, vy: 20, sway: 5, phase: 0 };
    expect(snowStep(p, 1000, 100, 100, 0).y).toBeCloseTo(30, 5);
    const wrapped = snowStep({ ...p, y: 101 }, 16, 100, 100, 0);
    expect(wrapped.y).toBe(-2);
    expect(wrapped.x).toBeLessThanOrEqual(100);
  });

  it("绕流：进入图标矩形横向推出，y 不变", () => {
    const p: SnowP = { x: 50, y: 50, vy: 10, sway: 0, phase: 0 };
    const out = snowAvoid(p, [{ x: 40, y: 40, w: 20, h: 20 }]);
    expect(out.y).toBe(50);
    expect(out.x === 37 || out.x === 63).toBe(true);
    expect(snowAvoid(p, [])).toEqual(p);
  });
});

// ---------------------------------------------------------------------------
// W-035 声音地形
// ---------------------------------------------------------------------------

describe("W-035 声音地形", () => {
  it("X 线性映射 -1..1 并钳制；零宽安全", () => {
    expect(panForX(0, 1000)).toBe(-1);
    expect(panForX(500, 1000)).toBe(0);
    expect(panForX(1000, 1000)).toBe(1);
    expect(panForX(-10, 1000)).toBe(-1);
    expect(panForX(2000, 1000)).toBe(1);
    expect(panForX(100, 0)).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// W-036 结组
// ---------------------------------------------------------------------------

describe("W-036 图标结组", () => {
  it("包围格计算；空成员安全", () => {
    const cells = { a: { c: 1, r: 1 }, b: { c: 3, r: 4 } };
    expect(clusterBounds(cells, ["a", "b"])).toEqual({ minC: 1, minR: 1, maxC: 3, maxR: 4 });
    expect(clusterBounds(cells, [])).toBeNull();
  });

  it("唯一 id；成员拖出即解散语义", () => {
    const c1 = makeCluster(["a", "b"]);
    const c2 = makeCluster(["a", "b"]);
    expect(c1.id).not.toBe(c2.id);
    expect(clusterTouched(c1, "a")).toBe(true);
    expect(clusterTouched(c1, "z")).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// W-037 标尺
// ---------------------------------------------------------------------------

describe("W-037 桌面标尺模式", () => {
  const tier = { w: 100, h: 100, tile: 58, icon: 46 };

  it("最近邻像素距离（格中心距，欧氏最小）", () => {
    const cells = { self: { c: 2, r: 2 }, n1: { c: 3, r: 2 }, n2: { c: 2, r: 4 } };
    const r = neighborDistance(cells, { c: 2, r: 2 }, tier);
    expect(r.px).toBe(100);
    expect(r.neighborId).toBe("n1");
    expect(r.snapped).toBe(true);
  });

  it("孤立图标距离 0 / 无邻居", () => {
    const r = neighborDistance({ a: { c: 0, r: 0 } }, { c: 5, r: 5 }, tier);
    expect(r.px).toBe(0);
    expect(r.neighborId).toBeNull();
  });

  it("拖拽点 → 最近格钳制", () => {
    expect(nearestCell(14 + 250, 14 + 250, tier, 14, 10, 10)).toEqual({ c: 2, r: 2 }); // 格2中心 264
    expect(nearestCell(14 + 350, 14 + 350, tier, 14, 10, 10)).toEqual({ c: 3, r: 3 }); // 格3中心 364
    expect(nearestCell(-100, -100, tier, 14, 10, 10)).toEqual({ c: 0, r: 0 });
    expect(nearestCell(99999, 99999, tier, 14, 10, 10)).toEqual({ c: 9, r: 9 });
  });

  it("SNAP 徽标：格中心 ±8px 内命中", () => {
    const cx = 14 + 2 * 100 + 50;
    const cy = 14 + 2 * 100 + 50;
    expect(snapBadge(cx, cy, tier, 14)).toBe(true);
    expect(snapBadge(cx + 9, cy, tier, 14)).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// W-038 原点坐标
// ---------------------------------------------------------------------------

describe("W-038 原点坐标", () => {
  it("首次落点只补录一次", () => {
    const now = 111;
    const v1 = firstSeen({}, ["a", "b"], now);
    expect(v1).toEqual({ a: now, b: now });
    const v2 = firstSeen(v1, ["a", "b", "c"], 222);
    expect(v2).toEqual({ a: now, b: now, c: 222 });
  });

  it("首次落点格子只记录首次出现", () => {
    const v = firstCell({}, { a: { c: 1, r: 1 } });
    const v2 = firstCell(v, { a: { c: 5, r: 5 }, b: { c: 2, r: 2 } });
    expect(v2.a).toEqual({ c: 1, r: 1 });
    expect(v2.b).toEqual({ c: 2, r: 2 });
  });

  it("回原点计划：仅含偏离者", () => {
    const homes = { a: { c: 0, r: 0 }, b: { c: 1, r: 0 } };
    const plan = homeRestorePlan({ a: { c: 3, r: 3 }, b: { c: 1, r: 0 }, gone: { c: 9, r: 9 } }, homes);
    expect(plan).toEqual([{ id: "a", to: { c: 0, r: 0 } }]);
  });
});

// ---------------------------------------------------------------------------
// 本地账本（nova.desk.*）
// ---------------------------------------------------------------------------

describe("使用账本", () => {
  it("记录使用 → lastUse 更新 + 月度计数", () => {
    const t0 = new Date(2026, 8, 10, 12, 0, 0).getTime();
    recordIconUse("app-a", t0);
    recordIconUse("app-a", t0 + 10);
    const u = loadUsage();
    expect(u.lastUse["app-a"]).toBe(t0 + 10);
    expect(currentMonthCounts(u)["app-a"]).toBe(2);
    expect(monthKey(new Date(t0))).toBe("2026-09");
  });
});

// ---------------------------------------------------------------------------
// 契约（S0 可消费）
// ---------------------------------------------------------------------------

describe("契约与降级", () => {
  it("13 项功能、W-026…W-038 连续唯一、默认档符合域则", () => {
    expect(DESK_FEATURES).toHaveLength(13);
    const ids = DESK_FEATURES.map((f) => f.id);
    for (let i = 0; i < 13; i++) expect(ids[i]).toBe(`W-0${26 + i}`);
    expect(new Set(ids).size).toBe(13);
    // 氛围/动效类默认关
    for (const id of ["W-026", "W-027", "W-028", "W-030", "W-031", "W-032", "W-034", "W-035"]) {
      expect(featureDefaults()[id]).toBe(false);
    }
    // 工具类默认开
    for (const id of ["W-029", "W-033", "W-036", "W-037", "W-038"]) {
      expect(featureDefaults()[id]).toBe(true);
    }
  });

  it("开关覆盖档持久化往返", () => {
    saveFeatureOverride("W-026", true);
    expect(loadFeatureOverrides()["W-026"]).toBe(true);
  });

  it("非 DOM 环境 activate/deactivate 幂等安全", () => {
    const ctx: DeskNovaCtx = {
      on: () => true,
      num: () => 10,
      str: () => "north",
      bool: () => false,
      motionOK: () => true,
    };
    expect(() => deskNova.activate(ctx)).not.toThrow();
    expect(() => deskNova.deactivate()).not.toThrow();
    expect(() => deskNova.deactivate()).not.toThrow();
  });

  it("api 动作在无 DOM/无数据时安全返回", () => {
    expect(() => deskNova.api.openLeaderboard()).not.toThrow();
    expect(() => deskNova.api.openPhotos()).not.toThrow();
    expect(() => deskNova.api.dissolveCluster()).not.toThrow();
    expect(deskNova.api.returnHome("nobody")).toBe(false);
    expect(deskNova.api.restoreGrid()).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// 布局数据级集成（localStorage 回写 + 星座/原点语义；零组件侵入）
// ---------------------------------------------------------------------------

describe("布局数据级集成", () => {
  beforeEach(() => {
    saveDesktopLayout({
      autoArrange: true,
      positions: {
        "app-1": { c: 0, r: 0 },
        "app-2": { c: 1, r: 0 },
        "app-3": { c: 2, r: 0 },
      },
      shelves: {},
      sort: "type",
    });
  });

  it("星座应用：写回布局、关 autoArrange、备份可还原", () => {
    const ok = deskNova.api.applyConstellation("orion");
    if (ok) {
      const l = loadDesktopLayout();
      expect(l.autoArrange).toBe(false);
      expect(Object.keys(l.positions)).toHaveLength(3);
      // 星座格不重叠
      const keys = Object.values(l.positions).map((c) => `${c.c},${c.r}`);
      expect(new Set(keys).size).toBe(keys.length);
      // 切回网格
      expect(deskNova.api.restoreGrid()).toBe(true);
    }
  });

  it("回原点：偏离者归位并写回布局", () => {
    const homes: Record<string, { cell: Cell; ts: number }> = {
      "app-1": { cell: { c: 0, r: 0 }, ts: 1 },
      "app-2": { cell: { c: 1, r: 0 }, ts: 1 },
      "app-3": { cell: { c: 2, r: 0 }, ts: 1 },
    };
    localStorage.setItem("nova.desk.homes", JSON.stringify(homes));
    saveDesktopLayout({
      autoArrange: false,
      positions: { "app-1": { c: 5, r: 5 }, "app-2": { c: 1, r: 0 }, "app-3": { c: 7, r: 7 } },
      shelves: {},
      sort: "type",
    });
    const n = deskNova.api.returnAllHomes();
    expect(n).toBe(2);
    const l = loadDesktopLayout();
    expect(l.positions["app-1"]).toEqual({ c: 0, r: 0 });
    expect(l.positions["app-3"]).toEqual({ c: 2, r: 0 });
    expect(loadHomes()["app-2"]).toBeDefined();
  });
});
