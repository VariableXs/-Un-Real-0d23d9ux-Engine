import { describe, expect, it } from "vitest";
import {
  ALGEBRA_MAX,
  BIN_MAX,
  BORROW_OVERDUE_MS,
  FILES_NOVA_FEATURES,
  GHOST_MIN_PAGES,
  GHOST_OPACITY,
  GROOVE_CEIL_MS,
  GROOVE_FLOOR_MS,
  GROOVE_MIN_BYTES,
  GROWTH_MIN_DAYS,
  HAVEN_CAP,
  HAVEN_FADE_MS,
  INK_GIGABYTE,
  INK_MEGABYTE,
  INK_MEGALITH_BYTES,
  JOURNAL_DAYS,
  PULSE_LINGER_MS,
  PULSE_PERIOD_MS,
  SHELF_CAPACITY,
  WATERMARK_FREE_PCT,
  beatIntervalMs,
  binByApp,
  borrowBook,
  byLabel,
  censusExt,
  censusReport,
  censusTextReport,
  dayKeyOf,
  dropBin,
  dropBin as _dropBinAlias,
  evalAlgebra,
  filesNovaDomain,
  forecastGrowth,
  ghostApplies,
  ghostOverscroll,
  ghostPageVisible,
  grooveChord,
  grooveEligible,
  havenVisit,
  inDndHours,
  inkTier,
  journalTrail,
  msUntilSettle,
  nextMorningAt,
  overdueIds,
  parseAlgebra,
  pruneJournal,
  prunePulsePaths,
  provenanceOf,
  pulseActive,
  pulseGlow,
  recordBin,
  recordMove,
  returnBook,
  ringMonths,
  ringTotal,
  sectorAngles,
  settleMidnight,
  upsertAlgebraFolder,
  watermarkDue,
} from "../filesNova";
import type { BinRecord, BorrowRecord, GrowthSample, HavenEntry, MoveEdge } from "../filesNova";

const HOUR = 3_600_000;
const DAY = 24 * HOUR;

/** 固定基准：2026-09-10 12:00 local。 */
const T0 = new Date(2026, 8, 10, 12, 0, 0).getTime();

// ---------------------------------------------------------------------------
// manifest：13 项齐、编号连续、域标识
// ---------------------------------------------------------------------------
describe("filesNova manifest", () => {
  it("W-064…W-076 共 13 项，编号连续无缺", () => {
    expect(FILES_NOVA_FEATURES).toHaveLength(13);
    const ids = FILES_NOVA_FEATURES.map((f) => Number(f.id.slice(2)));
    for (let i = 1; i < ids.length; i++) expect(ids[i]).toBe(ids[i - 1]! + 1);
    expect(ids[0]).toBe(64);
    expect(ids[ids.length - 1]).toBe(76);
  });

  it("每项必含中英标题/描述/降级说明，域标识 S6/AI-06", () => {
    for (const f of FILES_NOVA_FEATURES) {
      expect(f.titleZh.length).toBeGreaterThan(0);
      expect(f.titleEn.length).toBeGreaterThan(0);
      expect(f.descZh.length).toBeGreaterThan(0);
      expect(f.degrade.length).toBeGreaterThan(0);
    }
    expect(filesNovaDomain.id).toBe("S6");
    expect(filesNovaDomain.route).toBe("AI-06");
    expect(filesNovaDomain.features).toBe(FILES_NOVA_FEATURES);
  });

  it("overlay 工具窗与注册表对齐（census/journal/seasonring）", () => {
    const overlayOf = (id: string): string | undefined =>
      FILES_NOVA_FEATURES.find((f) => f.id === id)?.overlay;
    expect(overlayOf("W-065")).toBe("nova-census");
    expect(overlayOf("W-069")).toBe("nova-journal");
    expect(overlayOf("W-074")).toBe("nova-seasonring");
  });
});

// ---------------------------------------------------------------------------
// W-064 文件脉搏
// ---------------------------------------------------------------------------
describe("W-064 文件脉搏", () => {
  it("活性窗口：最后一次写入 ≤5s 活，>5s 停息", () => {
    expect(pulseActive(T0, T0 + PULSE_LINGER_MS)).toBe(true);
    expect(pulseActive(T0, T0 + PULSE_LINGER_MS + 1)).toBe(false);
  });

  it("脉搏余弦呼吸：起点 0、半程峰值 1、离场恒 0，周期 2s", () => {
    expect(PULSE_PERIOD_MS).toBe(2000);
    expect(pulseGlow(T0, T0)).toBe(0);
    expect(pulseGlow(T0, T0 + 1000)).toBe(1);
    expect(pulseGlow(T0, T0 + 500)).toBeCloseTo(0.5, 2);
    expect(pulseGlow(T0, T0 + 1500)).toBeCloseTo(0.5, 2);
    expect(pulseGlow(T0, T0 + PULSE_LINGER_MS + 1)).toBe(0);
  });

  it("prunePulsePaths 剔除停息路径且不改原表", () => {
    const paths = { "a.log": T0 - 1000, "b.log": T0 - 9000 };
    const next = prunePulsePaths(paths, T0);
    expect(Object.keys(next)).toEqual(["a.log"]);
    expect(paths["b.log"]).toBe(T0 - 9000); // 纯函数：原表不变
  });
});

// ---------------------------------------------------------------------------
// W-065 目录人口普查
// ---------------------------------------------------------------------------
describe("W-065 目录人口普查", () => {
  it("扩展名归一：小写化、点文件/无扩展名如实 FILE", () => {
    expect(censusExt("A.TXT")).toBe("txt");
    expect(censusExt("report.PDF")).toBe("pdf");
    expect(censusExt("readme")).toBe("FILE");
    expect(censusExt(".gitignore")).toBe("FILE");
    expect(censusExt("archive.")).toBe("FILE");
  });

  it("报告：类型构成排序 + Top5 + 平均年龄 + 空文件 + 最大文件", () => {
    const entries = [
      { name: "a.txt", size: 100, mtime: T0 - 1 * DAY },
      { name: "b.txt", size: 0, mtime: T0 - 3 * DAY },
      { name: "c.png", size: 2 * INK_MEGABYTE, mtime: T0 - 5 * DAY },
      { name: "d.png", size: 5, mtime: T0 - 7 * DAY },
      { name: "e.png", size: 7, mtime: T0 - 9 * DAY },
      { name: "f.png", size: 9, mtime: T0 - 11 * DAY },
      { name: "g.log", size: 11, mtime: T0 - 13 * DAY },
    ];
    const r = censusReport(entries, T0);
    expect(r.total).toBe(7);
    expect(r.types[0]).toEqual({ type: "png", count: 4, pct: 57.1 });
    expect(r.types.map((t) => t.type)).toEqual(["png", "txt", "log"]);
    expect(r.top5).toHaveLength(3);
    expect(r.avgAgeDays).toBeCloseTo(7, 0);
    expect(r.emptyCount).toBe(1);
    expect(r.largest).toEqual({ name: "c.png", size: 2 * INK_MEGABYTE });
  });

  it("空目录 → 全零诚实空态", () => {
    const r = censusReport([], T0);
    expect(r).toEqual({ total: 0, types: [], top5: [], avgAgeDays: 0, emptyCount: 0, largest: null });
  });

  it("文本报告可导出（含头部与类型行）", () => {
    const r = censusReport([{ name: "a.txt", size: 1, mtime: T0 }], T0);
    const text = censusTextReport("/docs", r, T0);
    expect(text).toContain("CENSUS REPORT · /docs");
    expect(text).toContain("FILES 1");
    expect(text).toContain("txt");
  });
});

// ---------------------------------------------------------------------------
// W-066 借阅书架
// ---------------------------------------------------------------------------
describe("W-066 借阅书架", () => {
  const book = { id: "/d/f.txt", name: "f.txt", from: "/d" };

  it("借出：正常入架并记录来源与时间", () => {
    const res = borrowBook([], book, T0);
    expect(res.ok).toBe(true);
    if (res.ok) expect(res.shelf[0]).toMatchObject({ ...book, borrowedAt: T0 });
  });

  it("重复借出拒绝（dup）", () => {
    const { shelf } = { shelf: borrowBook([], book, T0).ok ? borrowBook([], book, T0).shelf! : [] };
    const res = borrowBook(shelf as BorrowRecord[], book, T0 + 1);
    expect(res).toMatchObject({ ok: false, reason: "dup" });
  });

  it("书架满 24 册拒绝（full）", () => {
    expect(SHELF_CAPACITY).toBe(24);
    const full: BorrowRecord[] = Array.from({ length: SHELF_CAPACITY }, (_, i) => ({
      id: `f${i}`,
      name: `f${i}`,
      from: "/d",
      borrowedAt: T0,
    }));
    expect(borrowBook(full, book, T0)).toMatchObject({ ok: false, reason: "full" });
  });

  it("归还：移出书架并带回完整记录", () => {
    const res0 = borrowBook([], book, T0);
    const shelf = res0.ok ? res0.shelf : [];
    const res = returnBook(shelf, book.id);
    expect(res.shelf).toHaveLength(0);
    expect(res.record).toMatchObject({ id: book.id, from: "/d" });
    expect(returnBook(shelf, "missing").record).toBeNull();
  });

  it("逾期：>48h 未归还进入逾期名单", () => {
    expect(BORROW_OVERDUE_MS).toBe(48 * HOUR);
    const shelf: BorrowRecord[] = [
      { id: "fresh", name: "fresh", from: "/d", borrowedAt: T0 - 47 * HOUR },
      { id: "stale", name: "stale", from: "/d", borrowedAt: T0 - 49 * HOUR },
    ];
    expect(overdueIds(shelf, T0)).toEqual(["stale"]);
  });
});

// ---------------------------------------------------------------------------
// W-067 文件墨阶
// ---------------------------------------------------------------------------
describe("W-067 文件墨阶", () => {
  it("三档单调：<1MB 常规、MB 300、GB 500+灰底", () => {
    expect(inkTier(512 * 1024)).toEqual({ tier: 0, weight: 400, gray: false, megalith: false });
    expect(inkTier(INK_MEGABYTE)).toEqual({ tier: 1, weight: 300, gray: false, megalith: false });
    expect(inkTier(INK_GIGABYTE)).toEqual({ tier: 2, weight: 500, gray: true, megalith: false });
  });

  it("巨石 ≥10GB 附微图标标记", () => {
    expect(inkTier(INK_MEGALITH_BYTES).megalith).toBe(true);
    expect(inkTier(INK_MEGALITH_BYTES - 1).megalith).toBe(false);
    expect(inkTier(INK_MEGALITH_BYTES).tier).toBe(2);
  });

  it("负数/0 如实按常规档处理", () => {
    expect(inkTier(-5).tier).toBe(0);
    expect(inkTier(0).tier).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// W-068 目录增长预报
// ---------------------------------------------------------------------------
describe("W-068 目录增长预报", () => {
  const sample = (daysAgo: number, gb: number): GrowthSample => ({
    t: T0 - daysAgo * DAY,
    bytes: gb * INK_GIGABYTE,
  });

  it("样本 <14 天如实数据不足", () => {
    expect(GROWTH_MIN_DAYS).toBe(14);
    expect(forecastGrowth([sample(0, 10), sample(13, 11)]).insufficient).toBe(true);
    expect(forecastGrowth([sample(0, 10), sample(14, 11)]).insufficient).toBe(false);
    expect(forecastGrowth([sample(0, 10)]).insufficient).toBe(true);
  });

  it("线性 + 加权双模型：匀速增长斜率一致、近期加速时加权更高", () => {
    // 匀速 +0.1GB/天
    const even = [sample(30, 10), sample(20, 11), sample(10, 12), sample(0, 13)];
    const f1 = forecastGrowth(even);
    expect(f1.insufficient).toBe(false);
    expect(f1.perDayLinear).toBeCloseTo(0.1 * INK_GIGABYTE, -3);
    expect(f1.perDayWeighted).toBeCloseTo(0.1 * INK_GIGABYTE, -3);

    // 近期加速：加权斜率 > 线性斜率
    const accel = [sample(30, 10), sample(20, 10.2), sample(10, 11), sample(0, 13)];
    const f2 = forecastGrowth(accel);
    expect(f2.perDayWeighted).toBeGreaterThan(f2.perDayLinear);
  });

  it("投影 = 末点 + 加权斜率 × 30 天，置信带非负", () => {
    const f = forecastGrowth([sample(30, 10), sample(20, 11), sample(10, 12), sample(0, 13)]);
    expect(f.projectedBytes).toBeCloseTo(16 * INK_GIGABYTE, -2);
    expect(f.bandBytes).toBeGreaterThanOrEqual(0);
  });

  it("红色水位：剩余 10% 阈值日期；下降趋势如实永不到达", () => {
    expect(WATERMARK_FREE_PCT).toBe(0.1);
    // 容量 20GB，已用 13GB，+0.1GB/天 → 18GB（=90%）还需 50 天
    const v = watermarkDue([sample(30, 10), sample(20, 11), sample(10, 12), sample(0, 13)], 20 * INK_GIGABYTE, T0);
    expect(v.insufficient).toBe(false);
    expect(v.dueAt).not.toBeNull();
    expect(v.dueAt! - T0).toBeCloseTo(50 * DAY, -6);

    const down = watermarkDue([sample(30, 13), sample(20, 12), sample(10, 11), sample(0, 10)], 20 * INK_GIGABYTE, T0);
    expect(down.dueAt).toBeNull(); // 下降趋势永不到达
  });

  it("已超水位 → 如实判 now；样本不足 → insufficient", () => {
    const over = watermarkDue([sample(30, 19.9), sample(0, 19.95)], 20 * INK_GIGABYTE, T0);
    expect(over.dueAt).toBe(T0);
    expect(watermarkDue([sample(0, 19)], 20 * INK_GIGABYTE, T0).insufficient).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// W-069 文件迁途志
// ---------------------------------------------------------------------------
describe("W-069 文件迁途志", () => {
  it("记录移动 → 90 天滚动裁剪 + 500 条上限", () => {
    expect(JOURNAL_DAYS).toBe(90);
    let j: MoveEdge[] = [];
    j = recordMove(j, "/a", "/b", T0);
    j = recordMove(j, "/b", "/c", T0 + 1);
    expect(j).toHaveLength(2);
    // 91 天前的边被裁掉
    j = pruneJournal([...j, { from: "/old", to: "/x", at: T0 - 91 * DAY }], T0);
    expect(j).toHaveLength(2);
    const many: MoveEdge[] = Array.from({ length: 600 }, (_, i) => ({
      from: `/f${i}`,
      to: `/f${i + 1}`,
      at: T0,
    }));
    expect(pruneJournal(many, T0)).toHaveLength(500);
  });

  it("轨迹链反查：b→c、a→b 串成 a→b→c（旧→新）", () => {
    let j: MoveEdge[] = [];
    j = recordMove(j, "/a.txt", "/b.txt", T0 - 10);
    j = recordMove(j, "/b.txt", "/c.txt", T0 - 5);
    const trail = journalTrail(j, "/c.txt");
    expect(trail.map((e) => e.from)).toEqual(["/a.txt", "/b.txt"]);
    expect(trail.map((e) => e.to)).toEqual(["/b.txt", "/c.txt"]);
    expect(trail[0]!.at).toBeLessThan(trail[1]!.at);
  });

  it("无关联路径 → 空链；自环不成死循环", () => {
    let j: MoveEdge[] = [];
    j = recordMove(j, "/x", "/x", T0);
    expect(journalTrail(j, "/x")).toHaveLength(1);
    expect(journalTrail(j, "/unrelated")).toEqual([]);
  });

  it("多段历史中取最新边（重命名覆盖重定向）", () => {
    let j: MoveEdge[] = [];
    j = recordMove(j, "/a", "/b1", T0 - 30);
    j = recordMove(j, "/a", "/b2", T0 - 20);
    j = recordMove(j, "/b2", "/c", T0 - 10);
    const trail = journalTrail(j, "/c");
    expect(trail.map((e) => e.from)).toEqual(["/a", "/b2"]);
  });
});

// ---------------------------------------------------------------------------
// W-070 标签代数
// ---------------------------------------------------------------------------
describe("W-070 标签代数", () => {
  const tags: Record<string, string[]> = {
    工作: ["/a", "/b", "/c"],
    未完成: ["/b", "/c", "/d"],
    已归档: ["/c"],
  };

  it("交集 ∩", () => {
    const r = parseAlgebra("工作 ∩ 未完成");
    expect(r.ok).toBe(true);
    if (r.ok) expect(evalAlgebra(r.ast, tags)).toEqual(["/b", "/c"]);
  });

  it("并集 ∪", () => {
    const r = parseAlgebra("工作 ∪ 未完成");
    expect(r.ok).toBe(true);
    if (r.ok) expect(evalAlgebra(r.ast, tags)).toEqual(["/a", "/b", "/c", "/d"]);
  });

  it("差集 - 与优先级：∩ - 同级左结合", () => {
    const r1 = parseAlgebra("工作 ∩ 未完成 - 已归档");
    expect(r1.ok).toBe(true);
    if (r1.ok) expect(evalAlgebra(r1.ast, tags)).toEqual(["/b"]); // (工作∩未完成)-已归档
    const r2 = parseAlgebra("工作 - 已归档 ∩ 未完成");
    expect(r2.ok).toBe(true);
    if (r2.ok) expect(evalAlgebra(r2.ast, tags)).toEqual(["/b"]); // (工作-已归档)∩未完成 = {a,b}∩{b,c,d} = {b}
  });

  it("括号改变优先级 + 全角括号支持", () => {
    const r = parseAlgebra("工作 - （未完成 ∩ 已归档）");
    expect(r.ok).toBe(true);
    if (r.ok) expect(evalAlgebra(r.ast, tags)).toEqual(["/a", "/b"]);
  });

  it("语法错误即时定位：未知符号/悬空运算符/未闭合括号/空表达式", () => {
    const bad1 = parseAlgebra("工作 & 未完成");
    expect(bad1).toMatchObject({ ok: false, errorAt: 3 });
    const bad2 = parseAlgebra("工作 ∩");
    expect(bad2).toMatchObject({ ok: false });
    const bad3 = parseAlgebra("(工作 ∩ 未完成");
    expect(bad3).toMatchObject({ ok: false });
    const bad4 = parseAlgebra("  ");
    expect(bad4).toMatchObject({ ok: false, errorAt: 0 });
    const bad5 = parseAlgebra("∩ 工作");
    expect(bad5).toMatchObject({ ok: false, errorAt: 0 });
  });

  it("代数文件夹：同名覆盖 + 上限 32", () => {
    expect(ALGEBRA_MAX).toBe(32);
    let list = upsertAlgebraFolder([], "todo", "工作 ∩ 未完成", T0);
    list = upsertAlgebraFolder(list, "todo", "工作 - 已归档", T0 + 1);
    expect(list).toHaveLength(1);
    expect(list[0]).toMatchObject({ name: "todo", expr: "工作 - 已归档" });
    for (let i = 0; i < ALGEBRA_MAX + 5; i++) {
      list = upsertAlgebraFolder(list, `f${i}`, "工作", T0 + i);
    }
    expect(list).toHaveLength(ALGEBRA_MAX);
  });
});

// ---------------------------------------------------------------------------
// W-071 预览幽灵页
// ---------------------------------------------------------------------------
describe("W-071 预览幽灵页", () => {
  it("长文档 ≥3 页才挂幽灵页；12% 透明度语义", () => {
    expect(GHOST_MIN_PAGES).toBe(3);
    expect(GHOST_OPACITY).toBeCloseTo(0.12, 5);
    expect(ghostPageVisible(2)).toBe(false);
    expect(ghostPageVisible(3)).toBe(true);
    expect(ghostPageVisible(120)).toBe(true);
  });

  it("橡皮筋阻尼：越拉越硬、永不超过上限、负值归零", () => {
    const s1 = ghostOverscroll(10);
    const s2 = ghostOverscroll(40);
    const s3 = ghostOverscroll(400);
    expect(s1).toBeLessThan(s2);
    expect(s2).toBeLessThan(s3);
    expect(s3).toBeLessThan(120);
    expect(ghostOverscroll(-5)).toBe(0);
    expect(ghostOverscroll(0)).toBe(0);
  });

  it("仅文档预览适用（图片/音视频如实不适用）", () => {
    expect(ghostApplies("doc")).toBe(true);
    expect(ghostApplies("image")).toBe(false);
    expect(ghostApplies("audio")).toBe(false);
    expect(ghostApplies("video")).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// W-072 传输律动声
// ---------------------------------------------------------------------------
describe("W-072 传输律动声", () => {
  it("仅 ≥100MB 传输开启", () => {
    expect(GROOVE_MIN_BYTES).toBe(100 * 1024 * 1024);
    expect(grooveEligible(GROOVE_MIN_BYTES)).toBe(true);
    expect(grooveEligible(GROOVE_MIN_BYTES - 1)).toBe(false);
  });

  it("速率→节拍单调：速率越高间隔越短（960→240ms 钳制）", () => {
    expect(GROOVE_CEIL_MS).toBe(960);
    expect(GROOVE_FLOOR_MS).toBe(240);
    const slow = beatIntervalMs(128 * 1024);
    const mid = beatIntervalMs(4 * 1024 * 1024);
    const fast = beatIntervalMs(32 * 1024 * 1024);
    expect(slow).toBeGreaterThan(mid);
    expect(mid).toBeGreaterThan(fast);
    expect(beatIntervalMs(64 * 1024)).toBe(960);
    expect(beatIntervalMs(64 * 1024 * 1024)).toBe(240);
    expect(beatIntervalMs(1024 * 1024 * 1024)).toBe(240); // 超高速钳制
    expect(beatIntervalMs(0)).toBe(960); // 零速率 → 最疏
  });

  it("终止音组：完成 = 大三和弦；失败 = 不和谐双音；进行中无声", () => {
    expect(grooveChord("done")).toEqual([523.25, 659.25, 783.99]);
    expect(grooveChord("failed")).toHaveLength(2);
    expect(grooveChord("failed")[0]!).not.toBeCloseTo(grooveChord("failed")[1]!, 0);
    expect(grooveChord("active")).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// W-073 子夜更钟
// ---------------------------------------------------------------------------
describe("W-073 子夜更钟", () => {
  it("结算：精确到字节的净变化", () => {
    const d = settleMidnight({ added: 12, removed: 5, bytesAdded: 3000, bytesRemoved: 800 });
    expect(d).toEqual({ addedFiles: 12, removedFiles: 5, netFiles: 7, netBytes: 2200 });
  });

  it("msUntilSettle：指向下一个本地 00:00", () => {
    const before = new Date(2026, 8, 10, 23, 0).getTime();
    const after = new Date(2026, 8, 10, 1, 0).getTime();
    expect(msUntilSettle(before)).toBe(1 * HOUR);
    expect(msUntilSettle(after)).toBe(23 * HOUR);
    expect(msUntilSettle(before, 6)).toBe(7 * HOUR);
  });

  it("勿扰时段：22:00–07:00 跨午夜判定；明晨首现 08:00", () => {
    expect(inDndHours(new Date(2026, 8, 10, 23, 0).getTime())).toBe(true);
    expect(inDndHours(new Date(2026, 8, 10, 3, 0).getTime())).toBe(true);
    expect(inDndHours(new Date(2026, 8, 10, 12, 0).getTime())).toBe(false);
    expect(inDndHours(new Date(2026, 8, 10, 7, 0).getTime())).toBe(false); // 7 点整出勿扰

    const night = nextMorningAt(new Date(2026, 8, 10, 23, 30).getTime());
    expect(new Date(night).getHours()).toBe(8);
    expect(dayKeyOf(night)).toBe("2026-09-11");
    const dawn = nextMorningAt(new Date(2026, 8, 10, 3, 0).getTime());
    expect(dayKeyOf(dawn)).toBe("2026-09-10"); // 凌晨 → 当天 8 点
  });

  it("dayKeyOf 本地日键格式", () => {
    expect(dayKeyOf(new Date(2026, 0, 5).getTime())).toBe("2026-01-05");
    expect(dayKeyOf(new Date(2026, 11, 31).getTime())).toBe("2026-12-31");
  });
});

// ---------------------------------------------------------------------------
// W-074 目录季节环
// ---------------------------------------------------------------------------
describe("W-074 目录季节环", () => {
  it("按本地月份分桶 12 扇区", () => {
    const jan = new Date(2026, 0, 15).getTime();
    const mar = new Date(2025, 2, 2).getTime(); // 跨年数据同样按月归桶
    const months = ringMonths([jan, jan, mar]);
    expect(months[0]).toBe(2);
    expect(months[2]).toBe(1);
    expect(ringTotal(months)).toBe(3);
  });

  it("扇区角：0 月从 -90° 起顺时针、角度和 360°、空环全 0", () => {
    const sectors = sectorAngles([3, 1, ...new Array<number>(10).fill(0)]);
    expect(sectors[0]).toEqual({ month: 0, startDeg: -90, sweepDeg: 270 });
    expect(sectors[1]).toEqual({ month: 1, startDeg: 180, sweepDeg: 90 });
    expect(sectors[2]!.sweepDeg).toBe(0);
    const sum = sectors.reduce((a, s) => a + s.sweepDeg, 0);
    expect(sum).toBeCloseTo(360, 5);
    const empty = sectorAngles(new Array<number>(12).fill(0));
    expect(empty.every((s) => s.sweepDeg === 0)).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// W-075 回收站出身簿
// ---------------------------------------------------------------------------
describe("W-075 回收站出身簿", () => {
  const rec = (id: string, by: string): BinRecord => ({ id, name: `${id}.tmp`, by, origin: `/src/${id}`, at: T0 });

  it("出身标签：无法识别如实 SYSTEM", () => {
    expect(byLabel({ by: "editor.exe" })).toBe("editor.exe");
    expect(byLabel({ by: "" })).toBe("SYSTEM");
    expect(byLabel({ by: "   " })).toBe("SYSTEM");
  });

  it("入站登记：去重置顶 + 200 条滚动", () => {
    expect(BIN_MAX).toBe(200);
    let list = [rec("x", "")];
    list = recordBin(list, rec("a", "app1"));
    list = recordBin(list, rec("a", "app1")); // 去重
    expect(list).toHaveLength(2);
    expect(list[0]!.id).toBe("a");
    for (let i = 0; i < BIN_MAX + 10; i++) list = recordBin(list, rec(`n${i}`, "app2"));
    expect(list).toHaveLength(BIN_MAX);
    expect(list[0]!.id).toBe(`n${BIN_MAX + 9}`);
  });

  it("溯源与按出身筛选（SYSTEM 归一）", () => {
    const list = [rec("a", "app1"), rec("b", ""), rec("c", "SYSTEM")];
    expect(provenanceOf(list, "b")?.by).toBe("");
    expect(provenanceOf(list, "zz")).toBeNull();
    expect(binByApp(list, "app1").map((r) => r.id)).toEqual(["a"]);
    expect(binByApp(list, "SYSTEM").map((r) => r.id)).toEqual(["b", "c"]);
    expect(binByApp(list, "app2")).toEqual([]);
  });

  it("出站清理：还原后移除出身记录（不拦截还原）", () => {
    const list = [rec("a", "app1"), rec("b", "app2")];
    expect(dropBin(list, ["a"]).map((r) => r.id)).toEqual(["b"]);
    expect(dropBin(list, [])).toHaveLength(2);
  });
});

// ---------------------------------------------------------------------------
// W-076 近踪动港
// ---------------------------------------------------------------------------
describe("W-076 近踪动港", () => {
  it("访问计数 + 按频率排序 + 上限 5 枚", () => {
    expect(HAVEN_CAP).toBe(5);
    let list: HavenEntry[] = [];
    for (let i = 0; i < 3; i++) list = havenVisit(list, "/hot", T0 + i);
    list = havenVisit(list, "/warm", T0);
    list = havenVisit(list, "/cold", T0);
    expect(list[0]).toMatchObject({ path: "/hot", count: 3 });
    for (let i = 0; i < 7; i++) list = havenVisit(list, `/p${i}`, T0 + DAY);
    expect(list.length).toBeLessThanOrEqual(HAVEN_CAP);
    expect(list.some((e) => e.path === "/hot")).toBe(true); // 高频留存
  });

  it("一周未访问淡出；边界 7 天整保留", () => {
    expect(HAVEN_FADE_MS).toBe(7 * DAY);
    let list: HavenEntry[] = [{ path: "/old", lastAt: T0 - 7 * DAY - 1, count: 9 }];
    list = havenVisit(list, "/new", T0);
    expect(list.some((e) => e.path === "/old")).toBe(false);
    let keep: HavenEntry[] = [{ path: "/edge", lastAt: T0 - 7 * DAY, count: 9 }];
    keep = havenVisit(keep, "/new", T0);
    expect(keep.some((e) => e.path === "/edge")).toBe(true); // 恰好 7 天未过期
  });

  it("重复访问同一路径只保留一条且计数累加", () => {
    let list = havenVisit([], "/a", T0);
    list = havenVisit(list, "/a", T0 + 1);
    expect(list).toHaveLength(1);
    expect(list[0]).toMatchObject({ path: "/a", count: 2, lastAt: T0 + 1 });
  });
});

// ---------------------------------------------------------------------------
// 模块卫生冒烟（组合自洽）
// ---------------------------------------------------------------------------
describe("module hygiene", () => {
  it("dayKeyOf 与 msUntilSettle 组合自洽", () => {
    const t = new Date(2026, 8, 10, 9, 0).getTime();
    expect(dayKeyOf(t + msUntilSettle(t))).toBe("2026-09-11");
  });
});
