/**
 * AURORA-10000 · AI-11~AI-15 车道 · 纯逻辑内核测试。
 */
import { describe, expect, it } from "vitest";
import {
  auditSpacing4, auditTypeScale, clutterIndex, craftCssFilter, dedupeWallpapers,
  dueHealthRules, effectiveRefresh, festivalOn, hammingHex, inQuietWindow,
  organizeLayout, qualityFlag, ritualDue, rotatePlan, seasonOf, snapToGrid, themePhase,
  type OrgItem,
} from "../logic";

const items = (n: number, category = "a"): OrgItem[] =>
  Array.from({ length: n }, (_, i) => ({ id: `i${i}`, x: i * 90, y: 0, category, openedAt: n - i, lastUsedAt: n - i }));

describe("族0064 organizeLayout", () => {
  it("free 保持原位", () => {
    const src = items(3);
    const out = organizeLayout("free", src, { cell: 90, cols: 6 });
    expect(out.get("i0")).toEqual({ x: 0, y: 0 });
    expect(out.get("i2")).toEqual({ x: 180, y: 0 });
  });

  it("grid9 每行 3 列", () => {
    const out = organizeLayout("grid9", items(9), { cell: 100, cols: 6 });
    expect(out.get("i1")).toEqual({ x: 100, y: 0 });
    expect(out.get("i3")).toEqual({ x: 0, y: 100 });
  });

  it("magnetic 同类相吸（同类落在同带）", () => {
    const src = [
      { id: "a", x: 0, y: 0, category: "work" },
      { id: "b", x: 500, y: 300, category: "work" },
      { id: "c", x: 900, y: 100, category: "fun" },
    ];
    const out = organizeLayout("magnetic", src, { cell: 100, cols: 6 });
    expect(out.get("a")!.x / 100).toBe(out.get("b")!.x / 100);
    expect(out.get("c")!.x).toBeGreaterThan(out.get("a")!.x);
  });

  it("timeline 按 openedAt 升序横排", () => {
    const out = organizeLayout("timeline", items(4), { cell: 100, cols: 10 });
    expect(out.get("i0")!.x).toBe(300); // openedAt 最小（最旧）在最后？ openedAt=n-i → i0=4 最大
    expect(out.get("i3")!.x).toBe(0);
  });

  it("minimal 只留 3 个，其余收抽屉(-1)", () => {
    const out = organizeLayout("minimal", items(6), { cell: 100, cols: 6 });
    let kept = 0;
    let drawer = 0;
    for (const [, p] of out) {
      if (p.x === -1) drawer++;
      else kept++;
    }
    expect(kept).toBe(3);
    expect(drawer).toBe(3);
  });

  it("drawer 全部收入", () => {
    const out = organizeLayout("drawer", items(4), { cell: 100, cols: 6 });
    expect([...out.values()].every((p) => p.x === -1)).toBe(true);
  });

  it("scatter 同种子可复现", () => {
    const a = organizeLayout("scatter", items(6), { cell: 100, cols: 6, seed: 5 });
    const b = organizeLayout("scatter", items(6), { cell: 100, cols: 6, seed: 5 });
    expect([...a.entries()]).toEqual([...b.entries()]);
  });

  it("白名单项不移动", () => {
    const src = items(4);
    const out = organizeLayout("grid9", src, { cell: 100, cols: 6, keepIds: ["i0"] });
    expect(out.get("i0")).toEqual({ x: 0, y: 0 });
  });

  it("clutterIndex 归一在 0~100", () => {
    expect(clutterIndex([], 90)).toBe(0);
    const aligned = organizeLayout("grid9", items(20), { cell: 90, cols: 6 });
    const rects = [...aligned.values()];
    const alignedItems = items(20).map((it, i) => ({ ...it, x: rects[i]!.x, y: rects[i]!.y }));
    const alignedScore = clutterIndex(alignedItems, 90);
    const messy = clutterIndex(items(60).map((it, i) => ({ ...it, x: i * 91 + 7 })), 90);
    expect(alignedScore).toBeLessThanOrEqual(100);
    expect(messy).toBeGreaterThanOrEqual(alignedScore);
  });
});

describe("族0063 snapToGrid", () => {
  it("磁吸范围内吸附", () => {
    expect(snapToGrid(103, 97, 50, 8)).toEqual({ x: 100, y: 100, snapped: true });
  });
  it("范围外保持原位", () => {
    expect(snapToGrid(124, 97, 50, 4).snapped).toBe(false);
  });
});

describe("族0065 健康", () => {
  const rules = [
    { entryId: "F01601", everyMin: 20 },
    { entryId: "F01603", everyMin: 45 },
  ];
  it("到期判定与排序", () => {
    const now = 1_000_000_000_000;
    const due = dueHealthRules(rules, { F01601: now - 30 * 60000 }, now);
    // 从未触发（无记录）视为超时更久 → 排前
    expect(due.map((d) => d.entryId)).toEqual(["F01603", "F01601"]);
    expect(due[1]?.overdueMin).toBe(30);
  });
  it("未到期不提醒", () => {
    const now = 1_000_000_000_000;
    const due = dueHealthRules(rules, { F01601: now - 10 * 60000, F01603: now - 40 * 60000 }, now);
    expect(due).toHaveLength(0);
  });
  it("跨午夜勿扰窗口", () => {
    expect(inQuietWindow(23, 23, 7)).toBe(true);
    expect(inQuietWindow(3, 23, 7)).toBe(true);
    expect(inQuietWindow(12, 23, 7)).toBe(false);
  });
});

describe("族0073 节日表（2024~2029）", () => {
  it("2024 春节", () => {
    expect(festivalOn(new Date(2024, 1, 10))).toContain("cny");
  });
  it("2025 端午", () => {
    expect(festivalOn(new Date(2025, 4, 31))).toContain("duanwu");
  });
  it("2026 中秋", () => {
    expect(festivalOn(new Date(2026, 8, 25))).toContain("zhongqiu");
  });
  it("2024 除夕 = 春节前一天", () => {
    expect(festivalOn(new Date(2024, 1, 9))).toContain("chuxi");
  });
  it("普通日无节日（除季节档外）", () => {
    const hits = festivalOn(new Date(2026, 6, 15)).filter((f) => f !== "sakura" && f !== "maple");
    expect(hits).toHaveLength(0);
  });
  it("季节档", () => {
    expect(seasonOf(new Date(2026, 3, 10))).toBe(0);
    expect(seasonOf(new Date(2026, 11, 10))).toBe(3);
  });
});

describe("族0067 明暗计划", () => {
  it("时段判定", () => {
    expect(themePhase(12, { dayFrom: 7, nightFrom: 19 })).toBe("day");
    expect(themePhase(21, { dayFrom: 7, nightFrom: 19 })).toBe("night");
    expect(themePhase(5, { dayFrom: 7, nightFrom: 19 })).toBe("night");
  });
});

describe("族0059 craftCssFilter", () => {
  it("编译滤镜栈", () => {
    expect(craftCssFilter([
      { kind: "saturate", value: 120 },
      { kind: "blur", value: 2.5 },
      { kind: "hue-rotate", value: -30 },
    ])).toBe("saturate(120%) blur(2.5px) hue-rotate(-30deg)");
  });
  it("空栈 = none；叠加层不计入", () => {
    expect(craftCssFilter([])).toBe("none");
    expect(craftCssFilter([{ kind: "vignette", value: 0.4 }])).toBe("none");
  });
});

describe("族0058 画库", () => {
  it("汉明距离", () => {
    expect(hammingHex("f", "0")).toBe(4);
    expect(hammingHex("ff", "00")).toBe(8);
    expect(hammingHex("abc", "ab")).toBe(64);
  });
  it("感知查重：收藏项优先保留", () => {
    const res = dedupeWallpapers([
      { id: "a", phash: "aaaaaaaaaaaaaaaa" },
      { id: "b", phash: "aaaaaaaaaaab" + "aaaa", favorite: true },
    ]);
    expect(res.find((r) => r.keep === "b")).toBeTruthy();
  });
  it("低质检测", () => {
    expect(qualityFlag({ id: "x", sharp: 0.1 }, 16 / 9)).toBe("blurry");
    expect(qualityFlag({ id: "x", w: 1000, h: 1000 }, 16 / 9)).toBe("stretched");
    expect(qualityFlag({ id: "x", sharp: 0.9, w: 1920, h: 1080 }, 16 / 9)).toBe("ok");
  });
  it("轮换计划与排除", () => {
    expect(rotatePlan(["a", "b", "c"], ["b"], 1)).toBe("c");
    expect(rotatePlan(["a"], ["a"], 0)).toBeNull();
  });
});

describe("族0061 智能刷新", () => {
  it("不可见刷新 8 倍间隔、下限 5s", () => {
    expect(effectiveRefresh(10, true)).toBe(10);
    expect(effectiveRefresh(10, false)).toBe(80);
    expect(effectiveRefresh(2, true)).toBe(5);
  });
});

describe("族0069 审计", () => {
  it("4 倍数间距审计", () => {
    expect(auditSpacing4([4, 8, 12, 10, 16, 22])).toEqual([10, 22]);
  });
  it("1.25 字阶审计", () => {
    expect(auditTypeScale([12, 15, 19, 24, 14, 33])).toEqual([14, 33]);
  });
});

describe("族0075 仪式触发", () => {
  it("每日小时触发", () => {
    const fires = ritualDue([{ entryId: "F01870", kind: "daily-hour", hour: 15 }], new Date(2026, 8, 13, 15, 0));
    expect(fires).toHaveLength(1);
    expect(ritualDue([{ entryId: "F01870", kind: "daily-hour", hour: 15 }], new Date(2026, 8, 13, 9, 0))).toHaveLength(0);
  });
  it("周内小时触发（周五 18 点告别）", () => {
    const d = new Date(2026, 8, 11, 18, 0); // 2026-09-11 周五
    expect(ritualDue([{ entryId: "F01852", kind: "weekday-hour", weekday: 5, hour: 18 }], d)).toHaveLength(1);
    expect(ritualDue([{ entryId: "F01852", kind: "weekday-hour", weekday: 5, hour: 18 }], new Date(2026, 8, 12, 18, 0))).toHaveLength(0);
  });
  it("勿扰窗口仪式（午休）", () => {
    expect(ritualDue([{ entryId: "F01872", kind: "quiet-window", from: 12, to: 13 }], new Date(2026, 8, 13, 12, 30))).toHaveLength(1);
  });
});
