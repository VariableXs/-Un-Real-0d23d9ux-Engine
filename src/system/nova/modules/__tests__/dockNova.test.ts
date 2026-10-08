import { describe, expect, it } from "vitest";
import {
  BUCKET_MINUTES,
  BUCKETS_PER_DAY,
  DOCK_NOVA_FEATURES,
  MIN_HIT_PX,
  PERSONA_PRESET_RANK,
  SHARP_BTN_W,
  SHARP_GAP,
  TIDE_BASE_GAP,
  TIDE_HALF_OCCUPANCY,
  TIDE_MAX_SHRINK,
  TIDE_MIN_GAP,
  applyPresetOrder,
  arcEase,
  arcPoint,
  assignSlots,
  bucket30,
  clamp,
  colsFromTops,
  dayKeyOf,
  dayStartOf,
  detailFields,
  dockNovaDomain,
  driftTargetPx,
  entryKeyOf,
  launchableOf,
  microHistorySummary,
  mirrorIsUniform,
  mirrorPayload,
  mirrorSyncLatency,
  padButtonAction,
  padNavTarget,
  padRestoreDue,
  previewScale,
  pushActivity,
  sharpFits,
  sharpGain,
  springSettled,
  springStep,
  tideGapPx,
  tideOccupancy,
  tideScale,
  upsertPreset,
  yesterdayDiff,
  type ActivityEvent,
  type DockModuleDef,
} from "../dockNova";

const HOUR = 3_600_000;
const DAY = 24 * HOUR;

/** 固定"今日 0 点"基准（本地时区）；测试内时间一律相对该点。 */
const DAY0 = new Date(2026, 8, 10).getTime(); // 2026-09-10 00:00 local

// ---------------------------------------------------------------------------
// manifest：12 项齐、编号连续、hub 消费契约
// ---------------------------------------------------------------------------
describe("dockNova manifest", () => {
  it("W-039…W-050 共 12 项，编号连续无缺", () => {
    expect(DOCK_NOVA_FEATURES).toHaveLength(12);
    const ids = DOCK_NOVA_FEATURES.map((f) => Number(f.id.slice(2)));
    for (let i = 1; i < ids.length; i++) expect(ids[i]).toBe(ids[i - 1]! + 1);
    expect(ids[0]).toBe(39);
    expect(ids[ids.length - 1]).toBe(50);
  });

  it("每项必含中英标题/描述/降级说明，域标识 S4/AI-04", () => {
    for (const f of DOCK_NOVA_FEATURES) {
      expect(f.titleZh.length).toBeGreaterThan(0);
      expect(f.titleEn.length).toBeGreaterThan(0);
      expect(f.descZh.length).toBeGreaterThan(0);
      expect(f.degrade.length).toBeGreaterThan(0);
    }
    expect(dockNovaDomain.id).toBe("S4");
    expect(dockNovaDomain.route).toBe("AI-04");
    expect(dockNovaDomain.features).toBe(DOCK_NOVA_FEATURES);
  });
});

// ---------------------------------------------------------------------------
// 通用工具
// ---------------------------------------------------------------------------
describe("clamp", () => {
  it("边界钳制", () => {
    expect(clamp(5, 0, 10)).toBe(5);
    expect(clamp(-3, 0, 10)).toBe(0);
    expect(clamp(99, 0, 10)).toBe(10);
  });
});

// ---------------------------------------------------------------------------
// W-039 磁漂弹簧
// ---------------------------------------------------------------------------
describe("W-039 driftTargetPx / springStep / springSettled", () => {
  it("场外零影响，指针正上零位移", () => {
    expect(driftTargetPx(0, 200, 10)).toBe(0); // |d|=200 ≥ 64
    expect(driftTargetPx(64, 0, 10)).toBe(0); // 边界恰好 = radius
    expect(driftTargetPx(200, 200, 10)).toBe(0); // 同心
  });

  it("方向性：左负右正，衰减尾段离场越远越小（t·fall 在峰值后单调降）", () => {
    expect(driftTargetPx(180, 200, 10)).toBeLessThan(0); // 左侧迎
    expect(driftTargetPx(220, 200, 10)).toBeGreaterThan(0); // 右侧迎
    const tail1 = Math.abs(driftTargetPx(235, 200, 10));
    const tail2 = Math.abs(driftTargetPx(255, 200, 10));
    expect(tail1).toBeGreaterThan(tail2);
    expect(tail2).toBeGreaterThan(0);
    expect(Math.abs(driftTargetPx(180, 200, 10))).toBeLessThanOrEqual(10);
  });

  it("自定义半径生效", () => {
    expect(driftTargetPx(100, 0, 10, 128)).not.toBe(0);
    expect(driftTargetPx(100, 0, 10, 64)).toBe(0);
  });

  it("弹簧朝目标收敛且最终稳定（停 rAF 判据）", () => {
    let pos = 0;
    let vel = 0;
    const target = 8;
    for (let i = 0; i < 600; i++) {
      [pos, vel] = springStep(pos, vel, target, 1 / 60);
    }
    expect(springSettled(pos, vel, target)).toBe(true);
    expect(Math.abs(pos - target)).toBeLessThan(0.05);
  });

  it("半隐式欧拉单步：位移由新速度推进（np = pos + nv·dt）", () => {
    const [p1, v1] = springStep(0, 0, 10, 0.1);
    const expectV = (180 * 10 - 20 * 0) * 0.1;
    expect(v1).toBeCloseTo(expectV, 10);
    expect(p1).toBeCloseTo(expectV * 0.1, 10);
  });
});

// ---------------------------------------------------------------------------
// W-040 角色预设
// ---------------------------------------------------------------------------
describe("W-040 applyPresetOrder / upsertPreset", () => {
  it("rank 内存在者按预设序前置，其余保持相对顺序", () => {
    const cur = ["sys-explorer", "app-write", "tool-calc", "tool-notes"];
    const out = applyPresetOrder(cur, ["tool-calc", "app-write", "ghost-id"]);
    expect(out).toEqual(["tool-calc", "app-write", "sys-explorer", "tool-notes"]);
  });

  it("稳定：不新增不丢失（幽灵 id 与重复项忽略、无重复）", () => {
    const cur = ["a", "b", "c"];
    const out = applyPresetOrder(cur, ["b", "zzz", "a", "a"]);
    expect(out).toEqual(["b", "a", "c"]);
    expect(new Set(out).size).toBe(out.length);
  });

  it("三角色 rank 均非空且互不共享完整入口集合", () => {
    expect(PERSONA_PRESET_RANK.developer.length).toBeGreaterThan(0);
    expect(PERSONA_PRESET_RANK.creator.length).toBeGreaterThan(0);
    expect(PERSONA_PRESET_RANK.student.length).toBeGreaterThan(0);
  });

  it("upsert：空名/空布局拒绝，同名覆盖，order 拷贝防外泄", () => {
    expect(upsertPreset([], "  ", ["a"])).toEqual([]);
    expect(upsertPreset([], "x", [])).toEqual([]);
    const order = ["a", "b"];
    const next = upsertPreset([{ name: "x", order: ["old"], at: 1, custom: true }], " x ", order);
    expect(next).toHaveLength(1);
    expect(next[0]!.order).toEqual(["a", "b"]);
    order.push("c");
    expect(next[0]!.order).toEqual(["a", "b"]); // 深拷贝
  });

  it("上限 8 个：超出挤掉最旧自定义", () => {
    let list = upsertPreset([], "p0", ["a"], 0);
    for (let i = 1; i <= 8; i++) list = upsertPreset(list, `p${i}`, ["a"], i * 1000);
    expect(list).toHaveLength(8);
    expect(list.some((p) => p.name === "p0")).toBe(false); // 最旧被挤掉
    expect(list.some((p) => p.name === "p8")).toBe(true);
    // 覆盖已有不算新增，不触发挤掉
    const covered = upsertPreset(list, "p1", ["b"], 999_999);
    expect(covered).toHaveLength(8);
    expect(covered.some((p) => p.name === "p0")).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// W-041 潮汐条
// ---------------------------------------------------------------------------
describe("W-041 tideGapPx / tideScale / tideOccupancy", () => {
  it("间距：≤0.5 → 4px（默认），1.0 → 1px，中间线性", () => {
    expect(tideGapPx(0)).toBe(TIDE_BASE_GAP);
    expect(tideGapPx(TIDE_HALF_OCCUPANCY)).toBe(TIDE_BASE_GAP);
    expect(tideGapPx(1)).toBe(TIDE_MIN_GAP);
    expect(tideGapPx(0.75)).toBe(2.5);
    expect(tideGapPx(2)).toBe(TIDE_MIN_GAP); // 越界钳制
  });

  it("缩放：≤0.55 → 1，1.0 → 0.86，中间线性", () => {
    expect(tideScale(0)).toBe(1);
    expect(tideScale(0.55)).toBe(1);
    expect(tideScale(1)).toBe(TIDE_MAX_SHRINK);
    expect(tideScale(0.775)).toBeCloseTo(1 - (1 - TIDE_MAX_SHRINK) / 2, 2);
  });

  it("占用率 = Σ宽 + 间隙 / 容量，钳制 [0,1]；容量 0 或无图标 → 0", () => {
    expect(tideOccupancy([40, 40], 2, 1000)).toBeCloseTo((80 + TIDE_BASE_GAP) / 1000, 10);
    expect(tideOccupancy([1000], 1, 500)).toBe(1);
    expect(tideOccupancy([10], 0, 500)).toBe(0);
    expect(tideOccupancy([10], 1, 0)).toBe(0);
  });

  it("验收域：占用 0.5→1 期间间距单调收窄、缩放单调收缩", () => {
    let prevGap = Infinity;
    let prevScale = Infinity;
    for (let o = 0.5; o <= 1.0001; o += 0.05) {
      const g = tideGapPx(o);
      const s = tideScale(o);
      expect(g).toBeLessThanOrEqual(prevGap);
      expect(s).toBeLessThanOrEqual(prevScale);
      prevGap = g;
      prevScale = s;
    }
  });
});

// ---------------------------------------------------------------------------
// W-042 零点击详情卡
// ---------------------------------------------------------------------------
describe("W-042 detailFields", () => {
  it("meta 按 · 拆类型与时间，摘要截 240 防爆炸", () => {
    const f = detailFields("  笔记 A  ", "  内容…  ", " 文档 · 今天 14:00 · write ");
    expect(f).toEqual({ title: "笔记 A", snippet: "内容…", kind: "文档", meta: "今天 14:00 · write" });
    const long = detailFields("t", "x".repeat(300), "k");
    expect(long.snippet).toHaveLength(240);
    expect(long.kind).toBe("k");
    expect(long.meta).toBe("");
  });

  it("meta 缺失时 kind/meta 诚实为空", () => {
    expect(detailFields("t", "s", "")).toEqual({ title: "t", snippet: "s", kind: "", meta: "" });
  });
});

// ---------------------------------------------------------------------------
// W-043 时间戳微史
// ---------------------------------------------------------------------------
describe("W-043 bucket30 / activityBuckets / microHistorySummary / pushActivity", () => {
  it("30 分钟桶：0..47，越界钳制；常量自洽", () => {
    expect(BUCKET_MINUTES).toBe(30);
    expect(BUCKETS_PER_DAY).toBe(48);
    expect(bucket30(DAY0, DAY0)).toBe(0);
    expect(bucket30(DAY0 + 29 * 60_000, DAY0)).toBe(0);
    expect(bucket30(DAY0 + 30 * 60_000, DAY0)).toBe(1);
    expect(bucket30(DAY0 + DAY - 1, DAY0)).toBe(47);
    expect(bucket30(DAY0 - 5, DAY0)).toBe(0); // 上一日残留钳 0
    expect(bucket30(DAY0 + DAY + 5, DAY0)).toBe(47);
  });

  it("dayKeyOf/dayStartOf：本地日期键与当日 0 点", () => {
    expect(dayKeyOf(DAY0 + 5 * HOUR)).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(dayKeyOf(DAY0)).toBe("2026-09-10");
    expect(dayStartOf(DAY0 + 7 * HOUR + 1234)).toBe(DAY0);
  });

  it("直方图与摘要：峰值桶/总数/TopN 降序", () => {
    const events: ActivityEvent[] = [
      { kind: "app", id: "write", name: "写作", ts: DAY0 + 1 * HOUR },
      { kind: "app", id: "write", name: "写作", ts: DAY0 + 1.2 * HOUR },
      { kind: "app", id: "mindmap", name: "导图", ts: DAY0 + 1.5 * HOUR },
      { kind: "sys", id: "explorer", name: "文件", ts: DAY0 + 9 * HOUR },
    ];
    const s = microHistorySummary(events, DAY0, 2);
    expect(s.total).toBe(4);
    expect(s.peakBucket).toBe(bucket30(DAY0 + 1 * HOUR, DAY0));
    expect(s.peakCount).toBe(2); // 同桶（bucket2）双命中为峰值
    expect(s.buckets.reduce((a, b) => a + b, 0)).toBe(4);
    expect(s.top[0]).toEqual({ id: "write", name: "写作", count: 2 });
    expect(s.top).toHaveLength(2); // topN=2 截断
    const full = microHistorySummary(events, DAY0);
    expect(full.top).toHaveLength(3);
  });

  it("pushActivity：同 id 30s 内去重，容量裁剪保最新", () => {
    const e1: ActivityEvent = { kind: "app", id: "write", name: "写作", ts: DAY0 };
    const dup: ActivityEvent = { kind: "app", id: "write", name: "写作", ts: DAY0 + 29_000 };
    const e2: ActivityEvent = { kind: "app", id: "calc", name: "计算", ts: DAY0 + 31_000 };
    let list = pushActivity([], e1);
    list = pushActivity(list, dup);
    expect(list).toHaveLength(1); // 30s 内重复点击不刷直方图
    list = pushActivity(list, e2);
    expect(list).toHaveLength(2);
    for (let i = 0; i < 250; i++) {
      list = pushActivity(list, { kind: "tp", id: `t${i}`, name: `t${i}`, ts: DAY0 + 60_000 + i });
    }
    expect(list).toHaveLength(200); // 默认上限
    expect(list[list.length - 1]!.id).toBe("t249");
  });
});

// ---------------------------------------------------------------------------
// W-044 悬停窗景预览
// ---------------------------------------------------------------------------
describe("W-044 previewScale", () => {
  it("等比缩放完整放入且 ≤1（只缩不放），退化输入 → 1", () => {
    expect(previewScale(220, 140, 1100, 700)).toBe(0.2);
    expect(previewScale(220, 140, 200, 100)).toBe(1); // 比窗小 → 不放大
    expect(previewScale(220, 140, 100, 2000)).toBe(0.07);
    expect(previewScale(220, 140, 0, 700)).toBe(1);
    expect(previewScale(220, 140, 1100, 0)).toBe(1);
  });
});

// ---------------------------------------------------------------------------
// W-045 模块槽
// ---------------------------------------------------------------------------
describe("W-045 assignSlots", () => {
  const mod = (id: string, side: DockModuleDef["side"], order: number): DockModuleDef => ({
    id,
    titleZh: id,
    side,
    order,
    mount: () => {},
  });

  it("双端各 2 槽：同侧先占、any 填空位、侧内按 order 升序", () => {
    const r = assignSlots([
      mod("a", "any", 5),
      mod("b", "right", 1),
      mod("c", "left", 2),
      mod("d", "left", 1),
      mod("e", "right", 0),
    ]);
    expect(r.left.map((m) => m.id)).toEqual(["d", "c"]);
    expect(r.right.map((m) => m.id)).toEqual(["e", "b"]);
    expect(r.unplaced.map((m) => m.id)).toEqual(["a"]);
  });

  it("容量满诚实拒绝，绝不挤掉已放置者；any 在双侧全满时进 unplaced", () => {
    const r = assignSlots([mod("l1", "left", 0), mod("l2", "left", 1), mod("l3", "left", 2)]);
    expect(r.left.map((m) => m.id)).toEqual(["l1", "l2"]);
    expect(r.unplaced.map((m) => m.id)).toEqual(["l3"]);
    const full = assignSlots([mod("l", "left", 0), mod("l2", "left", 1), mod("r", "right", 0), mod("r2", "right", 1), mod("x", "any", 2)]);
    expect(full.unplaced.map((m) => m.id)).toEqual(["x"]);
  });
});

// ---------------------------------------------------------------------------
// W-046 昨日区
// ---------------------------------------------------------------------------
describe("W-046 yesterdayDiff / entryKeyOf / launchableOf", () => {
  it("差集 = 昨日有而今日无，保昨日序、去重、限 6 条", () => {
    const days = {
      "2026-09-09": ["app:write", "app:calc", "app:write", "sys:aihub", "app:a", "app:b", "app:c"],
      "2026-09-10": ["app:calc"],
    };
    const out = yesterdayDiff(days, "2026-09-10", "2026-09-09");
    expect(out).toEqual(["app:write", "sys:aihub", "app:a", "app:b", "app:c"]); // calc 今日用过被剔
    expect(yesterdayDiff(days, "2026-09-10", "2026-09-09", 3)).toEqual(["app:write", "sys:aihub", "app:a"]);
  });

  it("缺日/空日诚实为空", () => {
    expect(yesterdayDiff({}, "t", "y")).toEqual([]);
  });

  it("entryKeyOf ↔ launchableOf：app/tp 直通；sys 仅 explorer/recycle/taskman/tool-*；其余 null", () => {
    expect(entryKeyOf({ kind: "app", id: "write" })).toBe("app:write");
    expect(launchableOf("app:write", "w")).toEqual({ kind: "app", id: "write" });
    expect(launchableOf("tp:steam", "s")).toEqual({ kind: "tp", id: "steam" });
    expect(launchableOf("sys:explorer", "f")).toEqual({ kind: "sys", id: "explorer" });
    expect(launchableOf("sys:tool-calc", "c")).toEqual({ kind: "sys", id: "tool-calc" });
    expect(launchableOf("sys:aihub", "h")).toBeNull(); // 无 VWM 通道，诚实跳过
    expect(launchableOf("sys:", "x")).toBeNull();
    expect(launchableOf("nocolon", "x")).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// W-047 锋锐模式
// ---------------------------------------------------------------------------
describe("W-047 sharpFits / sharpGain", () => {
  it("32px 制式容量判定：恰好放下/差一点", () => {
    const need = (n: number) => n * SHARP_BTN_W + (n - 1) * SHARP_GAP + 16;
    expect(sharpFits(10, need(10))).toBe(true);
    expect(sharpFits(10, need(10) - 1)).toBe(false);
    expect(sharpFits(0, 16)).toBe(true);
  });

  it("同宽增益 = (40+4)/(32+2) ≈ 1.29（且常数自洽 ≥ MIN_HIT_PX）", () => {
    expect(SHARP_BTN_W).toBeGreaterThanOrEqual(MIN_HIT_PX);
    expect(sharpGain(1000)).toBeCloseTo(44 / 34, 2);
    expect(sharpGain(1000)).toBeGreaterThan(1.2);
  });
});

// ---------------------------------------------------------------------------
// W-048 启动弹道
// ---------------------------------------------------------------------------
describe("W-048 arcPoint / arcEase", () => {
  it("抛物线：端点精确、中点抬升 height", () => {
    const a = arcPoint(0, 600, 500, 300, 80, 0);
    expect(a).toEqual({ x: 0, y: 600 });
    const b = arcPoint(0, 600, 500, 300, 80, 1);
    expect(b).toEqual({ x: 500, y: 300 });
    const mid = arcPoint(0, 600, 500, 300, 80, 0.5);
    expect(mid.x).toBeCloseTo(250, 6);
    expect(mid.y).toBeCloseTo(0.25 * (600 + 300) + 0.5 * (300 - 80), 6); // 二次贝塞尔中点：¼(y0+y1) + ½cy
  });

  it("easeOutCubic：0→0、1→1、前快后慢单调增", () => {
    expect(arcEase(0)).toBe(0);
    expect(arcEase(1)).toBe(1);
    expect(arcEase(0.5)).toBeCloseTo(0.875, 10);
    expect(arcEase(0.25)).toBeGreaterThan(0.25); // 前段快
    let prev = -1;
    for (let t = 0; t <= 1.001; t += 0.05) {
      const v = arcEase(t);
      expect(v).toBeGreaterThanOrEqual(prev);
      prev = v;
    }
    expect(arcEase(-1)).toBe(0); // 越界钳制
    expect(arcEase(2)).toBe(1);
  });
});

// ---------------------------------------------------------------------------
// W-049 手柄友好菜单
// ---------------------------------------------------------------------------
describe("W-049 padNavTarget / colsFromTops / padButtonAction / padRestoreDue", () => {
  it("3 列网格导航：左右行内移动、上下跨列、越界 -1 不环绕", () => {
    // idx=4（第 2 行中间）of 7
    expect(padNavTarget("left", 3, 4, 7)).toBe(3);
    expect(padNavTarget("right", 3, 4, 7)).toBe(5);
    expect(padNavTarget("up", 3, 4, 7)).toBe(1);
    expect(padNavTarget("down", 3, 4, 7)).toBe(-1); // 4+3=7 ≥ count → 越界
    expect(padNavTarget("down", 3, 1, 7)).toBe(4);
    expect(padNavTarget("left", 3, 3, 7)).toBe(-1); // 行首
    expect(padNavTarget("right", 3, 2, 7)).toBe(-1); // 行尾
    expect(padNavTarget("up", 3, 1, 7)).toBe(-1); // 首行
    expect(padNavTarget("down", 3, 6, 7)).toBe(-1); // 末行
    expect(padNavTarget("down", 3, -1, 7)).toBe(-1);
    expect(padNavTarget("down", 3, 0, 0)).toBe(-1);
  });

  it("colsFromTops：首行同行计数（4px 容差），空 → 1", () => {
    expect(colsFromTops([100, 100, 102, 200, 201])).toBe(3);
    expect(colsFromTops([100, 105])).toBe(1);
    expect(colsFromTops([])).toBe(1);
  });

  it("标准手柄键位语义：A/B/X/Y + 十字键，其余 null", () => {
    expect(padButtonAction(0)).toBe("activate");
    expect(padButtonAction(1)).toBe("close");
    expect(padButtonAction(2)).toBe("first");
    expect(padButtonAction(3)).toBe("last");
    expect(padButtonAction(12)).toBe("up");
    expect(padButtonAction(13)).toBe("down");
    expect(padButtonAction(14)).toBe("left");
    expect(padButtonAction(15)).toBe("right");
    expect(padButtonAction(4)).toBeNull();
    expect(padButtonAction(99)).toBeNull();
  });

  it("断开 30s 还原：连接中/未记录时刻不还原，到点才还原", () => {
    expect(padRestoreDue(true, 1000, 60_000)).toBe(false);
    expect(padRestoreDue(false, null, 60_000)).toBe(false);
    expect(padRestoreDue(false, 10_000, 39_999)).toBe(false);
    expect(padRestoreDue(false, 10_000, 40_000)).toBe(true);
    expect(padRestoreDue(false, 10_000, 70_000, 5_000)).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// W-050 多屏镜像
// ---------------------------------------------------------------------------
describe("W-050 mirrorPayload / mirrorSyncLatency / mirrorIsUniform", () => {
  it("负载紧凑 JSON：e/s/t 三键，布尔编码 0/1", () => {
    expect(mirrorPayload({ enabled: true, sig: "a|b", ts: 5 })).toBe('{"e":1,"s":"a|b","t":5}');
    expect(mirrorPayload({ enabled: false, sig: "", ts: 0 })).toBe('{"e":0,"s":"","t":0}');
  });

  it("时延 = applied-sent（负值钳 0）；≤100ms 验收判据", () => {
    expect(mirrorSyncLatency(1000, 1099)).toBe(99);
    expect(mirrorSyncLatency(1000, 1000)).toBe(0);
    expect(mirrorSyncLatency(1000, 999)).toBe(0);
  });

  it("同构判定：全等且非空才成立", () => {
    expect(mirrorIsUniform(["a", "a", "a"])).toBe(true);
    expect(mirrorIsUniform(["a", "b"])).toBe(false);
    expect(mirrorIsUniform([])).toBe(false);
    expect(mirrorIsUniform(["a"])).toBe(true);
  });
});
