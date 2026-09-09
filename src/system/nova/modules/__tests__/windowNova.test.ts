import { describe, expect, it } from "vitest";
import {
  MAGNET_CONFIRM_PX,
  MAGNET_FIELD_PX,
  NOVA_RESERVED_HUES,
  adviceBlocked,
  alignSafeRect,
  areaRatioOf,
  crowdingOf,
  deskHue,
  echoEligible,
  foldGeometry,
  genealogyLayout,
  isLatticeRepeat,
  isSnappedLike,
  latticeRect,
  magnetState,
  massInertia,
  mosaicApplyRects,
  mosaicSuggestion,
  nearSame,
  nearestSnapDistance,
  normalizeLabel,
  pickRollback,
  pushSlot,
  railDecision,
  railGeom,
  snap8Rect,
  WINDOW_NOVA_FEATURES,
  windowNovaDomain,
  zElevationPx,
  type LatticeKey,
} from "../windowNova";

const WA = { x: 0, y: 0, w: 1920, h: 1040 };

// ---------------------------------------------------------------------------
// manifest：13 项齐、编号连续、hub 消费契约
// ---------------------------------------------------------------------------
describe("windowNova manifest", () => {
  it("W-013…W-025 共 13 项，编号连续无缺", () => {
    expect(WINDOW_NOVA_FEATURES).toHaveLength(13);
    const ids = WINDOW_NOVA_FEATURES.map((f) => Number(f.id.slice(2)));
    for (let i = 1; i < ids.length; i++) expect(ids[i]).toBe(ids[i - 1]! + 1);
    expect(ids[0]).toBe(13);
    expect(ids[ids.length - 1]).toBe(25);
  });

  it("每项必含中英标题/描述/降级说明，域标识 S2/AI-02", () => {
    for (const f of WINDOW_NOVA_FEATURES) {
      expect(f.titleZh.length).toBeGreaterThan(0);
      expect(f.titleEn.length).toBeGreaterThan(0);
      expect(f.descZh.length).toBeGreaterThan(0);
      expect(f.degrade.length).toBeGreaterThan(0);
    }
    expect(windowNovaDomain.id).toBe("S2");
    expect(windowNovaDomain.route).toBe("AI-02");
  });
});

// ---------------------------------------------------------------------------
// W-013 质量物理
// ---------------------------------------------------------------------------
describe("W-013 massInertia", () => {
  it("零面积 → 轻如纸（0/0），≥0.6 封顶 60/80", () => {
    expect(massInertia(0)).toEqual({ startDelayMs: 0, stopLingerMs: 0, heavy: false });
    expect(massInertia(0.6)).toMatchObject({ startDelayMs: 60, stopLingerMs: 80, heavy: true });
    expect(massInertia(1)).toMatchObject({ startDelayMs: 60, stopLingerMs: 80, heavy: true });
  });

  it("参数与面积单调不减（验收：惯性参数单调相关）", () => {
    let prevStart = -1;
    let prevStop = -1;
    for (let r = 0; r <= 1.0001; r += 0.05) {
      const m = massInertia(r);
      expect(m.startDelayMs).toBeGreaterThanOrEqual(prevStart);
      expect(m.stopLingerMs).toBeGreaterThanOrEqual(prevStop);
      prevStart = m.startDelayMs;
      prevStop = m.stopLingerMs;
    }
  });

  it("areaRatioOf：半屏窗口 ≈ 0.5", () => {
    expect(areaRatioOf({ w: 960, h: 1040 }, WA)).toBeCloseTo(0.5, 5);
    expect(areaRatioOf({ w: 100, h: 100 }, { x: 0, y: 0, w: 0, h: 0 })).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// W-014 磁吸手感
// ---------------------------------------------------------------------------
describe("W-014 magnetState / nearestSnapDistance", () => {
  it("场外零影响（不影响非吸附路径）", () => {
    expect(magnetState(MAGNET_FIELD_PX + 0.1)).toEqual({ inField: false, pullPx: 0, confirm: false });
  });

  it("拉力随接近增强，d=0 时 3px，d≤2 呼吸确认", () => {
    expect(magnetState(0).pullPx).toBe(3);
    expect(magnetState(3).pullPx).toBeCloseTo(1.5, 2);
    expect(magnetState(6).pullPx).toBe(0);
    expect(magnetState(MAGNET_CONFIRM_PX).confirm).toBe(true);
    expect(magnetState(MAGNET_CONFIRM_PX + 0.1).confirm).toBe(false);
    let prev = -1;
    for (let d = 5.5; d >= 0; d -= 0.5) {
      expect(magnetState(d).pullPx).toBeGreaterThan(prev);
      prev = magnetState(d).pullPx;
    }
  });

  it("最近吸附线：左缘/中线/顶缘识别", () => {
    expect(nearestSnapDistance({ x: 4, y: 200, w: 100, h: 100 }, WA)).toMatchObject({ edge: "left", axis: "x" });
    expect(nearestSnapDistance({ x: 964, y: 200, w: 100, h: 100 }, WA)).toMatchObject({ edge: "centerX" });
    expect(nearestSnapDistance({ x: 500, y: 3, w: 100, h: 100 }, WA)).toMatchObject({ edge: "top", axis: "y" });
  });
});

// ---------------------------------------------------------------------------
// W-015 族谱
// ---------------------------------------------------------------------------
describe("W-015 genealogyLayout", () => {
  it("父子深度与顺序正确（steam 收编链）", () => {
    const out = genealogyLayout([
      { id: "steam", ppid: null, label: "Steam" },
      { id: "game", ppid: "steam", label: "Game（收编）", adopted: true },
      { id: "friend", ppid: "game", label: "Friend Chat" },
      { id: "lone", ppid: null, label: "Notes" },
    ]);
    expect(out.find((n) => n.id === "steam")).toMatchObject({ depth: 0, parentId: null });
    expect(out.find((n) => n.id === "game")).toMatchObject({ depth: 1, parentId: "steam" });
    expect(out.find((n) => n.id === "friend")).toMatchObject({ depth: 2, parentId: "game" });
    expect(out.find((n) => n.id === "lone")).toMatchObject({ depth: 0, parentId: null });
    expect(out.map((n) => n.order)).toEqual([...out.keys()]);
  });

  it("父不在集合中 → 提升为根；环安全不死循环", () => {
    const orphan = genealogyLayout([{ id: "a", ppid: "ghost", label: "A" }]);
    expect(orphan[0]).toMatchObject({ depth: 0, parentId: null });
    const cycle = genealogyLayout([
      { id: "a", ppid: "b", label: "A" },
      { id: "b", ppid: "a", label: "B" },
      { id: "c", ppid: null, label: "C" },
    ]);
    expect(cycle).toHaveLength(3);
    expect(cycle.find((n) => n.id === "c")).toMatchObject({ depth: 0 });
    expect(cycle.find((n) => n.id === "a")).toMatchObject({ depth: 0 });
    expect(cycle.find((n) => n.id === "b")).toMatchObject({ depth: 1 });
  });
});

// ---------------------------------------------------------------------------
// W-016 Z 序阴影
// ---------------------------------------------------------------------------
describe("W-016 zElevationPx", () => {
  it("顶层最深、逐层单调递减、不触地穿帮", () => {
    expect(zElevationPx(0, 10)).toBe(24);
    let prev = 25;
    for (let rank = 0; rank < 10; rank++) {
      const px = zElevationPx(rank, 10);
      expect(px).toBeLessThanOrEqual(prev);
      expect(px).toBeGreaterThanOrEqual(2);
      prev = px;
    }
    expect(zElevationPx(9, 10)).toBe(2);
  });

  it("单窗恒定最深；static 双层分档由行为层按焦点取档", () => {
    expect(zElevationPx(0, 1)).toBe(24);
    expect(zElevationPx(5, 1)).toBe(24);
  });
});

// ---------------------------------------------------------------------------
// W-017 空间气味
// ---------------------------------------------------------------------------
describe("W-017 deskHue", () => {
  it("全部色相与三主题强调色错开 ≥20°", () => {
    const sep = (a: number, b: number): number => {
      const d = Math.abs(a - b) % 360;
      return Math.min(d, 360 - d);
    };
    for (let i = 0; i < 16; i++) {
      const hue = deskHue(i, NOVA_RESERVED_HUES);
      for (const r of NOVA_RESERVED_HUES) expect(sep(hue, r)).toBeGreaterThanOrEqual(20);
    }
  });

  it("确定性且不同索引色相互异", () => {
    expect(deskHue(3, NOVA_RESERVED_HUES)).toBe(deskHue(3, NOVA_RESERVED_HUES));
    const hues = new Set(Array.from({ length: 8 }, (_, i) => deskHue(i, NOVA_RESERVED_HUES)));
    expect(hues.size).toBeGreaterThanOrEqual(6);
  });
});

// ---------------------------------------------------------------------------
// W-018 键盘格阵
// ---------------------------------------------------------------------------
describe("W-018 latticeRect / isLatticeRepeat", () => {
  it("9 格覆盖全部屏幕区域且为整数", () => {
    const keys: LatticeKey[] = ["1", "2", "3", "4", "5", "6", "7", "8", "9"];
    for (const k of keys) {
      const r = latticeRect(k, WA);
      expect(Number.isInteger(r.x) && Number.isInteger(r.y) && Number.isInteger(r.w) && Number.isInteger(r.h)).toBe(true);
      expect(r.x).toBeGreaterThanOrEqual(WA.x);
      expect(r.y).toBeGreaterThanOrEqual(WA.y);
      expect(r.x + r.w).toBeLessThanOrEqual(WA.w);
      expect(r.y + r.h).toBeLessThanOrEqual(WA.h);
    }
  });

  it("1=左下、5=居中最大化（整工作区）、9=右上", () => {
    expect(latticeRect("5", WA)).toEqual(WA);
    const bl = latticeRect("1", WA);
    expect(bl.x).toBe(0);
    expect(bl.y).toBeGreaterThan(0);
    const tr = latticeRect("9", WA);
    expect(tr.x).toBeGreaterThan(0);
    expect(tr.y).toBe(0);
    expect(tr.x + tr.w).toBe(WA.w);
  });

  it("连按同格 900ms 内 = 二次恢复；换键/超时 = 首按", () => {
    expect(isLatticeRepeat("5", "5", 1000, 1500)).toBe(true);
    expect(isLatticeRepeat("5", "5", 1000, 2500)).toBe(false);
    expect(isLatticeRepeat("5", "7", 1000, 1500)).toBe(false);
    expect(isLatticeRepeat("5", null, 1000, 1500)).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// W-019 几何裁剪
// ---------------------------------------------------------------------------
describe("W-019 snap8Rect / alignSafeRect / isSnappedLike", () => {
  it("裁剪后坐标 mod 8 == 0（验收）", () => {
    const r = snap8Rect({ x: 13, y: 27, w: 817, h: 555 });
    expect(r.x % 8).toBe(0);
    expect(r.y % 8).toBe(0);
    expect(r.w % 8).toBe(0);
    expect(r.h % 8).toBe(0);
  });

  it("安全区对齐：钳入四边 8px 内框", () => {
    const r = alignSafeRect({ x: 0, y: 0, w: 1920, h: 1040 }, WA);
    expect(r.x).toBeGreaterThanOrEqual(8);
    expect(r.y).toBeGreaterThanOrEqual(8);
    expect(r.x + r.w).toBeLessThanOrEqual(WA.w - 8);
    expect(r.y + r.h).toBeLessThanOrEqual(WA.h - 8);
  });

  it("最大化/贴边跳过，浮窗不跳过", () => {
    expect(isSnappedLike({ state: "max", x: 0, y: 0, w: 1920, h: 1040 }, WA)).toBe(true);
    expect(isSnappedLike({ state: "normal", x: 0, y: 0, w: 960, h: 1040 }, WA)).toBe(true); // 半屏贴两边
    expect(isSnappedLike({ state: "normal", x: 120, y: 80, w: 700, h: 500 }, WA)).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// W-020 焦点回声
// ---------------------------------------------------------------------------
describe("W-020 echoEligible", () => {
  it("失焦 10s 门槛（≥10000ms）", () => {
    expect(echoEligible(9999)).toBe(false);
    expect(echoEligible(10_000)).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// W-021 马赛克顾问
// ---------------------------------------------------------------------------
describe("W-021 mosaicSuggestion / crowdingOf / adviceBlocked", () => {
  const three = [
    { id: "a", w: 600, h: 500 },
    { id: "b", w: 600, h: 500 },
    { id: "c", w: 600, h: 500 },
  ];

  it("<3 窗或拥挤度 ≤60% 不打扰", () => {
    expect(mosaicSuggestion(three.slice(0, 2), 80)).toBeNull();
    expect(mosaicSuggestion(three, 60)).toBeNull();
    expect(mosaicSuggestion(three, 61)).not.toBeNull();
  });

  it("宽窗给列、方窗给格、高窗给行（宽高比感知）", () => {
    const wide = [
      { id: "a", w: 1200, h: 300 },
      { id: "b", w: 1200, h: 300 },
      { id: "c", w: 1200, h: 300 },
    ];
    const tall = wide.map((w) => ({ ...w, w: 300, h: 1200 }));
    expect(mosaicSuggestion(wide, 80)?.mode).toBe("columns");
    expect(mosaicSuggestion(tall, 80)?.mode).toBe("rows");
    expect(mosaicSuggestion(three, 80)?.mode).toBe("grid");
  });

  it("忽略记忆 8h", () => {
    const now = 1_000_000_000;
    expect(adviceBlocked(null, now)).toBe(false);
    expect(adviceBlocked(now - 7 * 3_600_000, now)).toBe(true);
    expect(adviceBlocked(now - 9 * 3_600_000, now)).toBe(false);
  });

  it("crowdingOf 与 mosaicApplyRects 覆盖工作区", () => {
    const rects = mosaicApplyRects("columns", 3, WA);
    expect(rects).toHaveLength(3);
    expect(rects.reduce((a, r) => a + r.w, 0)).toBe(WA.w);
    expect(crowdingOf([{ x: 0, y: 0, w: 960, h: 1040 }], WA)).toBe(50);
  });
});

// ---------------------------------------------------------------------------
// W-022 仓位
// ---------------------------------------------------------------------------
describe("W-022 pushSlot / pickRollback / nearSame", () => {
  const A = { x: 0, y: 0, w: 800, h: 600 };
  const B = { x: 100, y: 100, w: 800, h: 600 };
  const C = { x: 300, y: 200, w: 800, h: 600 };

  it("nearSame：2px 容差判定", () => {
    expect(nearSame(A, { x: 2, y: -2, w: 798, h: 602 })).toBe(true);
    expect(nearSame(A, { x: 3, y: 0, w: 800, h: 600 })).toBe(false);
  });

  it("近同仓不重复记录；封顶 3 仓（裁最旧）", () => {
    let s = pushSlot([], A);
    expect(s).toHaveLength(1);
    s = pushSlot(s, { x: 1, y: 1, w: 801, h: 599 }); // ≤2px 视为同仓
    expect(s).toHaveLength(1);
    s = pushSlot(s, B);
    s = pushSlot(s, C);
    s = pushSlot(s, { x: 500, y: 300, w: 800, h: 600 });
    expect(s).toHaveLength(3);
    expect(s[0]).toEqual(B); // 最旧被裁
  });

  it("回滚取最近一仓且 ≠ 当前位置；取出即消费", () => {
    const slots = [A, B, C];
    const pick = pickRollback(slots, { x: 310, y: 210, w: 800, h: 600 });
    expect(pick?.rect).toEqual(C);
    expect(pick?.rest).toEqual([A, B]);
    expect(pickRollback(slots, C)).toEqual({ rect: B, rest: [A, C] });
    expect(pickRollback([C], C)).toBeNull();
    expect(pickRollback([], C)).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// W-023 轨道浮窗
// ---------------------------------------------------------------------------
describe("W-023 railDecision / railGeom", () => {
  it("顶端 8px 内松手入轨", () => {
    expect(railDecision(8, 0)).toBe("dock");
    expect(railDecision(9, 0)).toBe("none");
  });

  it("贴轨几何：高锁 72、原位钳制、不出工作区", () => {
    const g = railGeom(WA, 500, 900);
    expect(g.y).toBe(0);
    expect(g.h).toBe(72);
    expect(g.w).toBe(900);
    const edge = railGeom(WA, 1900, 900);
    expect(edge.x + edge.w).toBeLessThanOrEqual(WA.w);
    const wide = railGeom(WA, 100, 3000);
    expect(wide.w).toBe(WA.w);
  });
});

// ---------------------------------------------------------------------------
// W-024 折叠带
// ---------------------------------------------------------------------------
describe("W-024 foldGeometry", () => {
  it("右缘 36px 竖带、自顶向下、不溢出工作区", () => {
    const { ribbonX, cells } = foldGeometry(5, WA);
    expect(ribbonX).toBe(WA.w - 36);
    expect(cells).toHaveLength(5);
    for (const c of cells) {
      expect(c.x).toBe(ribbonX);
      expect(c.w).toBe(36);
    }
    expect(cells[1]!.y).toBe((cells[0]!.y as number) + 40);
  });

  it("窗口多到放不下时诚实截断", () => {
    const { cells } = foldGeometry(100, WA);
    expect(cells.length).toBeLessThan(100);
    expect(cells[cells.length - 1]!.y + cells[cells.length - 1]!.h).toBeLessThanOrEqual(WA.h);
    expect(foldGeometry(0, WA).cells).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// W-025 标签
// ---------------------------------------------------------------------------
describe("W-025 normalizeLabel", () => {
  it("≤12 字符保留（验收：标签 ≤ 12 字符）", () => {
    expect(normalizeLabel("参考资料 A")).toBe("参考资料 A");
    expect((normalizeLabel("初稿B") ?? "").length).toBeLessThanOrEqual(12);
  });

  it("超长按码点截断（emoji 不劈半）、空白压缩、空 → null", () => {
    expect(normalizeLabel("  a   b  ")).toBe("a b");
    expect(normalizeLabel("一二三四五六七八九十十一十")).toBe("一二三四五六七八九十十一");
    const emoji = normalizeLabel("🚀".repeat(13));
    expect(emoji).not.toBeNull();
    expect(emoji).toBe("🚀".repeat(12));
    expect(Array.from(emoji as string).length).toBe(12);
    expect(normalizeLabel("   ")).toBeNull();
  });
});
