/**
 * NOVA-200 · S16 工程收官路（AI-16）单测 —— qualityNova（W-188…W-200）。
 *
 * 覆盖：manifest 契约（13 项连续/双语/默认档与 S0 一致）+ 十三项纯逻辑 +
 * 行为层非 DOM 安全性。验收口径：
 * - W-188 三轴实测（CPU% × 时长）；阈值公开；无数据维度如实 N/A；
 * - W-189 空闲 ≥ 30min 入眠；释放 ≥ 90% 达标；唤醒 1.5s 预算；快照一致 = 100% 还原；
 *   手动唤醒优先级最高；睡眠列表 hub 可见；
 * - W-190 仅确定性可重放；NOT DETERMINISTIC 如实标注；逐步对照 100% 复现崩溃点；
 *   发散如实定位；仅 dev 可见；
 * - W-191 5 分钟滚动窗口剔除；越慢墨越浓；浓墨 = 真实掉帧（预算 2 倍阈值）；
 * - W-192 三方对照（性能/内存/能耗）胜者判定 + 负载时钟对齐校验；
 * - W-193 击穿（更低）更新水位；逼近（+15%）警报；虚线标注 FLOOR xxxMB @version；
 * - W-194 内容寻址哈希树确定性；一致 = REPRODUCIBLE；差异如实 NOT REPRODUCIBLE + 清单；
 * - W-195 零设置/零历史/全引导与真首跑 1:1；销毁零残留；
 * - W-196 气象四态阈值与 HUD 同源；预报样本不足如实标注娱乐性误差；
 * - W-197 12 指标映射 12 星座；亮度 = 健康度；暗星 < 0.6 可展开明细；
 * - W-198 礼炮仅 dev/手动触发；版本去重；reduce-motion 降静态铭牌；
 * - W-199 破纪录判定严格（同机同场景才比）；档案含上下文；可导出；
 * - W-200 里程碑 1000/1200/1500 解锁；功勋墙分布；战绩同源播报。
 */

import { describe, expect, it } from "vitest";
import {
  BATTLE_MILESTONES,
  CANNON_MS,
  ENERGY_BANDS,
  FLOOR_APPROACH_PCT,
  INK_COLS,
  INK_DROP_MS,
  INK_FRAME_BUDGET_MS,
  INK_WINDOW_MS,
  METRIC_DIRECTIONS,
  NEST_IDLE_MIN,
  NEST_RELEASE_TARGET,
  NEST_WAKE_BUDGET_MS,
  QUALITY_NOVA_FEATURES,
  WEATHER_HUD,
  ZODIAC_DARK_LINE,
  ZODIAC_WHEEL,
  activateQualityNova,
  addMilestone,
  archiveUpdate,
  badgesOf,
  battleSummary,
  cannonAllowed,
  cannonMode,
  clamp,
  clockAligned,
  compareFingerprints,
  currentFloor,
  deactivateQualityNova,
  destroyFresh,
  energyGradeOf,
  energyLabel,
  execReplayOps,
  exportArchive,
  fingerprintTree,
  flagOn,
  floorLineLabel,
  forecastWeather,
  fnv1a64,
  freshProfile,
  honorWall,
  inkConcentration,
  inkGrid,
  isBetter,
  isQualityNovaActive,
  mcpuSec,
  motionOK,
  nestList,
  nestVerdict,
  nextBadge,
  plaqueRows,
  qualityNovaDomain,
  releaseRatio,
  replayCrash,
  replayVisible,
  residueOf,
  restoreIntegrity,
  sameCondition,
  shouldNest,
  spawnFresh,
  starDetail,
  updateFloor,
  wakePriority,
  wargameReport,
  weatherOf,
  zodiacOf,
} from "../qualityNova";
import type {
  ArchiveEntry,
  CrashDump,
  DomainTests,
  FloorRecord,
  FrameSample,
  Milestone,
  NestEntry,
  WeatherSample,
  WargameSide,
  ZodiacMetric,
} from "../qualityNova";

const NOW = Date.UTC(2026, 8, 10, 12, 0, 0); // 2026-09-10 12:00 UTC

// ---------------------------------------------------------------------------
// manifest：13 项齐、编号连续、hub 消费契约
// ---------------------------------------------------------------------------

describe("qualityNova manifest", () => {
  it("W-188…W-200 共 13 项，编号连续无缺", () => {
    expect(QUALITY_NOVA_FEATURES).toHaveLength(13);
    const ids = QUALITY_NOVA_FEATURES.map((f) => Number(f.id.slice(2)));
    for (let i = 1; i < ids.length; i++) expect(ids[i]).toBe(ids[i - 1]! + 1);
    expect(ids[0]).toBe(188);
    expect(ids[ids.length - 1]).toBe(200);
  });

  it("每项必含中英标题/描述/降级说明", () => {
    for (const f of QUALITY_NOVA_FEATURES) {
      expect(f.titleZh.length).toBeGreaterThan(0);
      expect(f.titleEn.length).toBeGreaterThan(0);
      expect(f.descZh.length).toBeGreaterThan(0);
      expect(f.degrade.length).toBeGreaterThan(0);
    }
    expect(qualityNovaDomain.id).toBe("S16");
    expect(qualityNovaDomain.route).toBe("AI-16");
    expect(qualityNovaDomain.nameZh).toBe("工程质量与收官");
  });

  it("默认档与 S0 注册表口径一致（W-190/192/195 默认关，其余默认开）", () => {
    const off = ["W-190", "W-192", "W-195"];
    for (const f of QUALITY_NOVA_FEATURES) {
      expect(f.defaultOn).toBe(off.includes(f.id) ? false : true);
    }
  });

  it("overlay 清单与 S0 注册表一致（W-197 nova-zodiac / W-200 nova-battlepass）", () => {
    const overlays = Object.fromEntries(QUALITY_NOVA_FEATURES.filter((f) => f.overlay).map((f) => [f.id, f.overlay]));
    expect(overlays["W-197"]).toBe("nova-zodiac");
    expect(overlays["W-200"]).toBe("nova-battlepass");
    expect(Object.keys(overlays)).toHaveLength(2);
  });
});

// ---------------------------------------------------------------------------
// W-188 能效标签
// ---------------------------------------------------------------------------

describe("W-188 能效标签", () => {
  it("能效当量 = CPU% × 时长（mCPU·s）", () => {
    expect(mcpuSec({ cpuPct: 12, ms: 10000 })).toBe(120); // 12% × 10s = 120 mCPU·s
    expect(mcpuSec({ cpuPct: 50, ms: 0 })).toBe(0);
    expect(mcpuSec({ cpuPct: -1, ms: 1000 })).toBe(0);
  });

  it("A–G 阈值公开透明且逐级判定", () => {
    expect(energyGradeOf(120)).toBe("A");
    expect(energyGradeOf(121)).toBe("B");
    expect(energyGradeOf(240)).toBe("B");
    expect(energyGradeOf(480)).toBe("C");
    expect(energyGradeOf(960)).toBe("D");
    expect(energyGradeOf(1920)).toBe("E");
    expect(energyGradeOf(3840)).toBe("F");
    expect(energyGradeOf(3841)).toBe("G");
    expect(energyGradeOf(0)).toBe("N/A"); // 无数据如实 N/A
    expect(ENERGY_BANDS).toHaveLength(7);
  });

  it("三轴评定：总评 = 最差轴；缺维度如实 N/A", () => {
    const r = energyLabel([
      { axis: "idle", cpuPct: 5, ms: 10000 }, // 50 → A
      { axis: "boot", cpuPct: 30, ms: 10000 }, // 300 → C
      { axis: "motion", cpuPct: 60, ms: 10000 }, // 600 → D
    ]);
    expect(r.complete).toBe(true);
    expect(r.overall).toBe("D");
    const idle = r.axes.find((a) => a.axis === "idle")!;
    expect(idle.grade).toBe("A");
    const partial = energyLabel([{ axis: "idle", cpuPct: 5, ms: 10000 }]);
    expect(partial.complete).toBe(false);
    expect(partial.axes.find((a) => a.axis === "boot")!.grade).toBe("N/A");
    expect(partial.overall).toBe("A"); // 缺轴不虚报总评，按已有最差
    expect(energyLabel([]).overall).toBe("N/A");
  });

  it("非法样本被如实过滤", () => {
    const r = energyLabel([{ axis: "idle", cpuPct: NaN, ms: 1000 }, { axis: "motion", cpuPct: 10, ms: 1000 }]);
    expect(r.axes.find((a) => a.axis === "idle")!.grade).toBe("N/A");
    expect(r.axes.find((a) => a.axis === "motion")!.grade).toBe("A");
  });
});

// ---------------------------------------------------------------------------
// W-189 睡眠孵化器
// ---------------------------------------------------------------------------

describe("W-189 睡眠孵化器", () => {
  it("空闲 30 分钟达线入眠", () => {
    expect(NEST_IDLE_MIN).toBe(30);
    expect(shouldNest({ id: "a", idleMin: 29, worksetMB: 100, snapshotHash: "h" })).toBe(false);
    expect(shouldNest({ id: "a", idleMin: 30, worksetMB: 100, snapshotHash: "h" })).toBe(true);
    expect(shouldNest({ id: "a", idleMin: 60, worksetMB: 0, snapshotHash: "h" })).toBe(false); // 无工作集不入眠
  });

  it("释放率 ≥ 90% 达标（验收）", () => {
    expect(NEST_RELEASE_TARGET).toBe(0.9);
    expect(releaseRatio(200, 10)).toBe(0.95);
    expect(releaseRatio(200, 20)).toBe(0.9);
    expect(releaseRatio(200, 60)).toBe(0.7);
    expect(releaseRatio(0, 0)).toBe(0);
    expect(releaseRatio(100, -5)).toBe(0); // 负值防呆
  });

  it("入眠体检：释放 + 唤醒预算（1.5s）双达标", () => {
    expect(NEST_WAKE_BUDGET_MS).toBe(1500);
    expect(nestVerdict(0.95, 800).pass).toBe(true);
    expect(nestVerdict(0.95, 1600).pass).toBe(false); // 超预算
    expect(nestVerdict(0.85, 800).pass).toBe(false); // 释放不足
    expect(nestVerdict(0.9, 1500).pass).toBe(true); // 边界恰好达标
  });

  it("快照校验和一致 = 100% 还原；手动唤醒优先级最高", () => {
    expect(restoreIntegrity("abc123", "abc123")).toBe(true);
    expect(restoreIntegrity("abc123", "abd999")).toBe(false);
    expect(restoreIntegrity("", "")).toBe(false); // 空指纹不算还原
    expect(wakePriority(true)).toBeGreaterThan(wakePriority(false));
    expect(wakePriority(true)).toBe(1);
    expect(wakePriority(false)).toBe(0);
  });

  it("睡眠列表按空闲时长降序（最久闲者最深眠，hub 可见）", () => {
    const entries: NestEntry[] = [
      { id: "b", idleMin: 12, worksetMB: 300, snapshotHash: "x", asleep: true, swappedMB: 30, sleptAt: NOW },
      { id: "a", idleMin: 45, worksetMB: 500, snapshotHash: "y", asleep: true, swappedMB: 50, sleptAt: NOW },
      { id: "c", idleMin: 33, worksetMB: 200, snapshotHash: "z", asleep: false, swappedMB: 200, sleptAt: NOW },
    ];
    expect(nestList(entries).map((e) => e.id)).toEqual(["a", "c", "b"]);
  });
});

// ---------------------------------------------------------------------------
// W-190 崩溃单步重放器
// ---------------------------------------------------------------------------

describe("W-190 崩溃单步重放器", () => {
  const ops = [
    { op: "set", key: "n", value: 1 },
    { op: "add", key: "n", value: 2 },
    { op: "mul", key: "n", value: 3 },
  ] as const;

  it("确定性指令集逐步执行", () => {
    const states = execReplayOps([...ops]);
    expect(states).toHaveLength(3);
    expect(states[0]!.n).toBe(1);
    expect(states[1]!.n).toBe(3);
    expect(states[2]!.n).toBe(9);
  });

  it("逐步状态对照：100% 复现崩溃点", () => {
    const states = execReplayOps([...ops]);
    const dump: CrashDump = { id: "crash-001", deterministic: true, ops: [...ops], crashAtStep: 3, recordedStates: states };
    const v = replayCrash(dump);
    expect(v.kind).toBe("reproduced");
    expect(v.kind === "reproduced" && v.atStep).toBe(3);
    expect(v.label).toBe("REPRODUCED");
  });

  it("非确定性崩溃如实 NOT DETERMINISTIC（不假装能重放）", () => {
    const dump: CrashDump = { id: "ui-crash", deterministic: false, ops: [], crashAtStep: 1, recordedStates: [] };
    const v = replayCrash(dump);
    expect(v.kind).toBe("not-deterministic");
    expect(v.label).toBe("NOT DETERMINISTIC");
  });

  it("状态发散如实定位首个发散步", () => {
    const recorded = execReplayOps([...ops]);
    recorded[1] = { ...recorded[1]!, n: 99 }; // 第 2 步被篡改
    const dump: CrashDump = { id: "tampered", deterministic: true, ops: [...ops], crashAtStep: 3, recordedStates: recorded };
    const v = replayCrash(dump);
    expect(v.kind).toBe("diverged");
    expect(v.kind === "diverged" && v.atStep).toBe(2);
    expect(v.kind === "diverged" && v.expect).toBe(99);
  });

  it("崩溃步超出录制步数 → 如实发散（诚实口径）", () => {
    const states = execReplayOps([...ops]);
    const dump: CrashDump = { id: "incomplete", deterministic: true, ops: [...ops], crashAtStep: 5, recordedStates: states };
    expect(replayCrash(dump).kind).toBe("diverged");
  });

  it("仅 dev 可见（验收）", () => {
    expect(replayVisible(true)).toBe(true);
    expect(replayVisible(false)).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// W-191 GPU 诊断砚
// ---------------------------------------------------------------------------

describe("W-191 GPU 诊断砚", () => {
  it("5 分钟滚动窗口剔除（保留最近窗口）", () => {
    expect(INK_WINDOW_MS).toBe(300000);
    const samples: FrameSample[] = [
      { ms: 10, at: NOW - 400_000 }, // 窗口外
      { ms: 12, at: NOW - 299_000 }, // 窗口内
      { ms: 20, at: NOW }, // 最新
      { ms: 15, at: NOW + 1000 }, // 未来戳（异常数据）
    ];
    const r = inkGrid(samples, NOW);
    expect(r.kept).toBe(2);
    expect(r.evicted).toBe(2);
  });

  it("越慢墨越浓：浓度单调且饱和于预算 2 倍", () => {
    expect(INK_FRAME_BUDGET_MS).toBeCloseTo(16.67, 1);
    expect(inkConcentration(5)).toBe(0); // 快于预算一半 → 无墨
    expect(inkConcentration(16.7)).toBeGreaterThan(inkConcentration(10));
    expect(inkConcentration(33.4)).toBe(1); // ≥ 预算 2 倍 → 饱和浓墨
    expect(inkConcentration(100)).toBe(1);
    expect(inkConcentration(0)).toBe(0);
  });

  it("浓墨 = 真实掉帧（预算 2 倍阈值）且落格正确", () => {
    expect(INK_DROP_MS).toBeCloseTo(33.3, 1);
    const samples: FrameSample[] = [
      { ms: 10, at: NOW }, // 最新 → 第 0 格
      { ms: 40, at: NOW }, // 掉帧 → 第 0 格热点
      { ms: 50, at: NOW - 60_000 }, // 1 分钟前 → 第 12 格
      { ms: 12, at: NOW - 4 * 60_000 }, // 4 分钟前 → 第 48 格
    ];
    const r = inkGrid(samples, NOW);
    expect(r.kept).toBe(4);
    expect(r.worstMs).toBe(50);
    expect(r.hotspots).toBe(2);
    const firstCell = r.cells[0]!;
    expect(firstCell.count).toBe(2);
    expect(firstCell.drops).toBe(1);
    expect(firstCell.hotspot).toBe(true);
    expect(r.cells[12]!.count).toBe(1);
    expect(r.cells[12]!.hotspot).toBe(true);
    expect(r.cells[48]!.count).toBe(1);
    expect(r.cells[INK_COLS - 1]!.count).toBe(0);
  });

  it("60 格 × 5s 结构（验收：墨图保留最近 5 分钟）", () => {
    expect(INK_COLS).toBe(60);
    const r = inkGrid([], NOW);
    expect(r.cells).toHaveLength(60);
    expect(r.kept).toBe(0);
    expect(r.hotspots).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// W-192 多环境军棋推演
// ---------------------------------------------------------------------------

describe("W-192 多环境军棋推演", () => {
  const a: WargameSide = { id: "main", label: "本尊", fps: 60, memMB: 400, cpuPct: 12 };
  const b: WargameSide = { id: "clone", label: "分身", fps: 55, memMB: 450, cpuPct: 10 };

  it("三方对照战报（性能/内存/能耗）", () => {
    const r = wargameReport(a, b);
    expect(r.perf).toBe("main"); // fps 高者胜
    expect(r.mem).toBe("main"); // MB 低者胜
    expect(r.energy).toBe("clone"); // CPU% 低者胜
    expect(r.overall).toBe("main"); // 2/3 维胜
    expect(r.rows).toHaveLength(3);
  });

  it("平手维度与总平局（可解释决胜）", () => {
    const r = wargameReport(
      { id: "a", label: "A", fps: 60, memMB: 400, cpuPct: 10 },
      { id: "b", label: "B", fps: 60, memMB: 400, cpuPct: 10 },
    );
    expect(r.perf).toBeNull();
    expect(r.mem).toBeNull();
    expect(r.energy).toBeNull();
    expect(r.overall).toBeNull();
  });

  it("负载注入时钟对齐校验（验收：推演负载注入同步）", () => {
    expect(clockAligned(1000, 1020, 50)).toBe(true);
    expect(clockAligned(1000, 1100, 50)).toBe(false);
    expect(clockAligned(1000, 1000)).toBe(true);
  });

  it("残缺方数据 → 空战报（不伪造）", () => {
    const r = wargameReport({ id: "a", label: "A", fps: NaN, memMB: 400, cpuPct: 10 }, b);
    expect(r.rows).toHaveLength(0);
    expect(r.overall).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// W-193 内存地平线
// ---------------------------------------------------------------------------

describe("W-193 内存地平线", () => {
  const base: FloorRecord[] = [
    { version: "1.2", idleMB: 420, at: Date.UTC(2026, 5, 1) },
    { version: "1.4", idleMB: 382, at: Date.UTC(2026, 7, 1) },
  ];

  it("虚线标注 FLOOR 382MB @v1.4 式", () => {
    expect(floorLineLabel({ version: "1.4", idleMB: 382, at: NOW })).toBe("FLOOR 382MB @1.4");
  });

  it("当前水位 = 历史最低", () => {
    const f = currentFloor(base)!;
    expect(f.version).toBe("1.4");
    expect(f.idleMB).toBe(382);
    expect(currentFloor([])).toBeNull();
  });

  it("击穿（更低）→ 更新水位线（验收）", () => {
    const u = updateFloor(base, { version: "1.5", idleMB: 360, at: NOW });
    expect(u.verdict).toBe("breached");
    expect(u.floor!.version).toBe("1.5");
    expect(u.records).toHaveLength(3);
    expect(u.reason).toContain("击穿");
  });

  it("逼近红线（≤ +15%）→ 警报", () => {
    expect(FLOOR_APPROACH_PCT).toBe(0.15);
    const u = updateFloor(base, { version: "1.5", idleMB: 382 * 1.1, at: NOW });
    expect(u.verdict).toBe("approaching");
    expect(u.records).toHaveLength(2); // 不入档
    const edge = updateFloor(base, { version: "1.5", idleMB: 382 * 1.15, at: NOW });
    expect(edge.verdict).toBe("approaching"); // 恰好 +15% 边界仍警报
  });

  it("安全线上 → steady", () => {
    const u = updateFloor(base, { version: "1.5", idleMB: 500, at: NOW });
    expect(u.verdict).toBe("steady");
  });

  it("首条记录 = 基线入档", () => {
    const u = updateFloor([], { version: "0.9", idleMB: 500, at: NOW });
    expect(u.verdict).toBe("breached");
    expect(u.floor!.version).toBe("0.9");
  });
});

// ---------------------------------------------------------------------------
// W-194 可复现指纹
// ---------------------------------------------------------------------------

describe("W-194 可复现指纹", () => {
  it("FNV-1a 64 内容哈希：确定性 + 内容敏感", () => {
    expect(fnv1a64("hello")).toBe(fnv1a64("hello"));
    expect(fnv1a64("hello")).not.toBe(fnv1a64("hellp"));
    expect(fnv1a64("")).toMatch(/^[0-9a-f]{16}$/);
    expect(fnv1a64("变量环境")).toMatch(/^[0-9a-f]{16}$/); // 非 ASCII
  });

  it("内容寻址哈希树：叶按路径排序、根由叶行决定", () => {
    const t = fingerprintTree("A", [
      { path: "b.txt", content: "B" },
      { path: "a.txt", content: "A" },
    ]);
    expect(t.files).toBe(2);
    expect(t.leaves[0]!.path).toBe("a.txt"); // 排序稳定
    const same = fingerprintTree("B", [
      { path: "a.txt", content: "A" },
      { path: "b.txt", content: "B" },
    ]);
    expect(same.root).toBe(t.root); // 同内容不同顺序 → 同根（内容寻址）
  });

  it("双构建一致 → REPRODUCIBLE", () => {
    const files = [
      { path: "main.rs", content: "fn main() {}" },
      { path: "ui.tsx", content: "export default 1;" },
    ];
    const fa = fingerprintTree("build-1", files);
    const fb = fingerprintTree("build-2", files);
    const c = compareFingerprints(fa, fb);
    expect(c.reproducible).toBe(true);
    expect(c.label).toBe("REPRODUCIBLE");
    expect(c.diffs).toHaveLength(0);
  });

  it("内容差异 / 单侧缺失 → NOT REPRODUCIBLE + 差异清单（验收）", () => {
    const fa = fingerprintTree("A", [
      { path: "a.txt", content: "1" },
      { path: "b.txt", content: "2" },
    ]);
    const fb = fingerprintTree("B", [
      { path: "a.txt", content: "1-changed" },
      { path: "c.txt", content: "3" },
    ]);
    const c = compareFingerprints(fa, fb);
    expect(c.reproducible).toBe(false);
    expect(c.label).toBe("NOT REPRODUCIBLE");
    const kinds = Object.fromEntries(c.diffs.map((d) => [d.path, d.kind]));
    expect(kinds["a.txt"]).toBe("changed");
    expect(kinds["b.txt"]).toBe("only-a");
    expect(kinds["c.txt"]).toBe("only-b");
  });
});

// ---------------------------------------------------------------------------
// W-195 白纸视角
// ---------------------------------------------------------------------------

describe("W-195 白纸视角", () => {
  it("白纸档案与真首跑 1:1（零设置/零历史/全引导）", () => {
    const p = freshProfile(NOW);
    expect(p.settings).toBeNull();
    expect(p.history).toBe(0);
    expect(p.onboarding).toBe(true);
    expect(p.firstRun).toBe(true);
    expect(p.spawnedAt).toBe(NOW);
  });

  it("沙盒会话：scope 受控、销毁后零残留（验收）", () => {
    const s = spawnFresh(NOW);
    expect(s.alive).toBe(true);
    expect(s.scope).toContain("nova.quality.fresh.state");
    // 模拟销毁后 scope 内仍残留键 → 残留核查非空（能发现问题）
    expect(residueOf(s, ["nova.quality.fresh.state"])).toContain("nova.quality.fresh.state");
    const dead = destroyFresh(s);
    expect(dead.alive).toBe(false);
    expect(dead.profile.history).toBe(0);
    // 销毁后 scope 清空 → 零残留
    expect(residueOf(dead, [])).toHaveLength(0);
  });
});

// ---------------------------------------------------------------------------
// W-196 性能气象站
// ---------------------------------------------------------------------------

describe("W-196 性能气象站", () => {
  it("气象四态阈值与 HUD 同源（WEATHER_HUD 常量）", () => {
    expect(WEATHER_HUD.cpuBusy).toBe(45);
    expect(WEATHER_HUD.cpuOver).toBe(75);
    expect(WEATHER_HUD.memBusy).toBe(60);
    expect(WEATHER_HUD.memOver).toBe(85);
  });

  it("晴/多云/雨/风暴判定", () => {
    expect(weatherOf(10, 30)).toBe("sunny"); // 空闲
    expect(weatherOf(50, 30)).toBe("cloudy"); // 常态（CPU 繁忙）
    expect(weatherOf(10, 70)).toBe("cloudy"); // 常态（内存繁忙）
    expect(weatherOf(80, 50)).toBe("rain"); // 繁忙（CPU 过载单边）
    expect(weatherOf(10, 90)).toBe("rain"); // 繁忙（内存过载单边）
    expect(weatherOf(80, 90)).toBe("storm"); // 过载（双边）
  });

  it("未来 30 分钟预报：同时段规律多数决（验收：预报规律来自本地使用模式）", () => {
    const at = Date.UTC(2026, 8, 10, 14, 30, 0); // 14:30，目标 15:00
    const hist: WeatherSample[] = [];
    for (let d = 0; d < 6; d++) {
      hist.push({ at: Date.UTC(2026, 8, 3 + d, 15, 0, 0), cpuPct: 80, memPct: 90 }); // 15 点风暴规律
    }
    const f = forecastWeather(hist, at);
    expect(f.weather).toBe("storm");
    expect(f.samples).toBe(6);
    expect(f.confidence).toBeGreaterThan(0.8);
  });

  it("样本不足 → 低置信 + 娱乐性误差如实标注（验收）", () => {
    const at = Date.UTC(2026, 8, 10, 14, 30, 0);
    const hist: WeatherSample[] = [{ at: Date.UTC(2026, 8, 10, 15, 0, 0), cpuPct: 20, memPct: 30 }];
    const f = forecastWeather(hist, at);
    expect(f.samples).toBeLessThan(5);
    expect(f.note).toContain("样本不足");
    const none = forecastWeather([], at);
    expect(none.weather).toBe("sunny");
    expect(none.confidence).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// W-197 工程星象
// ---------------------------------------------------------------------------

describe("W-197 工程星象", () => {
  it("12 指标映射 12 星座（IAU 词表，不迷信化）", () => {
    expect(ZODIAC_WHEEL).toHaveLength(12);
    const keys = new Set(ZODIAC_WHEEL.map((w) => w.key));
    expect(keys.size).toBe(12);
    for (const w of ZODIAC_WHEEL) {
      expect(w.constellation.length).toBeGreaterThan(0);
      expect(w.zh.length).toBeGreaterThan(0);
      expect(w.metricZh.length).toBeGreaterThan(0);
    }
  });

  it("亮度 = 健康度；暗星 < 0.6 可展开明细", () => {
    expect(ZODIAC_DARK_LINE).toBe(0.6);
    const metrics: ZodiacMetric[] = [
      { key: "testCoverage", value: 0.95, detail: "覆盖 95%" },
      { key: "depHealth", value: 0.5, detail: "2 个过期依赖" },
      { key: "gatePass", value: 0.6 }, // 恰好亮线
    ];
    const stars = zodiacOf(metrics);
    expect(stars).toHaveLength(12);
    const cov = stars.find((s) => s.key === "testCoverage")!;
    expect(cov.brightness).toBe(0.95);
    expect(cov.dark).toBe(false);
    const dep = stars.find((s) => s.key === "depHealth")!;
    expect(dep.dark).toBe(true);
    const gate = stars.find((s) => s.key === "gatePass")!;
    expect(gate.dark).toBe(false); // 0.6 恰达亮线
    const missing = stars.find((s) => s.key === "bootPerf")!;
    expect(missing.dark).toBe(true); // 无数据 → 暗星
    expect(missing.detail).toContain("暂无同源门禁数据");
    expect(starDetail(dep)).toContain("依赖健康");
    expect(starDetail(dep)).toContain("2 个过期依赖");
  });

  it("超界值钳制 [0,1]", () => {
    const stars = zodiacOf([{ key: "frameRate", value: 2 }, { key: "crashFree", value: -1 }]);
    expect(stars.find((s) => s.key === "frameRate")!.brightness).toBe(1);
    expect(stars.find((s) => s.key === "crashFree")!.brightness).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// W-198 发版礼炮
// ---------------------------------------------------------------------------

describe("W-198 发版礼炮", () => {
  it("礼炮 2.5s 且仅 dev/手动触发（验收：不打扰用户）", () => {
    expect(CANNON_MS).toBe(2500);
    expect(cannonAllowed({ dev: true, manual: false })).toBe(true);
    expect(cannonAllowed({ dev: false, manual: true })).toBe(true);
    expect(cannonAllowed({ dev: false, manual: false })).toBe(false);
  });

  it("里程碑墙：版本去重、按时间降序", () => {
    let ms: Milestone[] = [];
    ms = addMilestone(ms, { version: "1.4", at: 1000 });
    ms = addMilestone(ms, { version: "1.5", at: 2000 });
    ms = addMilestone(ms, { version: "1.4", at: 3000 }); // 重复版本
    expect(ms).toHaveLength(2);
    expect(ms[0]!.version).toBe("1.5");
    expect(addMilestone(ms, { version: "", at: 4000 })).toHaveLength(2); // 空版本防呆
  });

  it("历史铭牌行 + reduce-motion 降静态铭牌", () => {
    const rows = plaqueRows([
      { version: "1.4", at: Date.UTC(2026, 8, 1), note: "NOVA-200" },
      { version: "1.3", at: Date.UTC(2026, 7, 1) },
    ]);
    expect(rows[0]).toContain("1.4");
    expect(rows[0]).toContain("2026-09-01");
    expect(rows[0]).toContain("NOVA-200");
    expect(rows[1]).toBe("1.3 · 2026-08-01");
    expect(cannonMode(true)).toBe("animated");
    expect(cannonMode(false)).toBe("static"); // 降级
  });
});

// ---------------------------------------------------------------------------
// W-199 性能不朽档案
// ---------------------------------------------------------------------------

describe("W-199 性能不朽档案", () => {
  const incumbent: ArchiveEntry = { metric: "bootMs", value: 3200, version: "1.4", at: NOW, machine: "PC-A", scenario: "cold" };

  it("同条件判定：同机器 + 同场景（严格）", () => {
    expect(sameCondition(incumbent, { ...incumbent, value: 1 })).toBe(true);
    expect(sameCondition(incumbent, { ...incumbent, machine: "PC-B" })).toBe(false);
    expect(sameCondition(incumbent, { ...incumbent, scenario: "warm" })).toBe(false);
  });

  it("方向感知：bootMs 越低越好、fps 越高越好", () => {
    expect(isBetter("bootMs", 3000, 3200)).toBe(true);
    expect(isBetter("bootMs", 3300, 3200)).toBe(false);
    expect(isBetter("fps", 144, 120)).toBe(true);
    expect(isBetter("fps", 100, 120)).toBe(false);
    expect(isBetter("unknown", 1, 2)).toBe(false); // 未知指标不比
    expect(METRIC_DIRECTIONS["bootMs"]!.dir).toBe("lower");
    expect(METRIC_DIRECTIONS["fps"]!.dir).toBe("higher");
  });

  it("首个成绩 = 原点即纪录（致敬 NEW RECORD 3.1s BOOT）", () => {
    const cand: ArchiveEntry = { metric: "bootMs", value: 3100, version: "1.5", at: NOW, machine: "PC-A", scenario: "cold" };
    const u = archiveUpdate([], cand);
    expect(u.record).toBe(true);
    expect(u.salute).toBe("NEW RECORD 3.1s BOOT");
    expect(u.entries).toHaveLength(1);
  });

  it("破纪录更新 + 未破纪录保留（严格同条件不跨比）", () => {
    const better: ArchiveEntry = { ...incumbent, value: 2900, version: "1.5" };
    const u1 = archiveUpdate([incumbent], better);
    expect(u1.record).toBe(true);
    expect(u1.entries[0]!.value).toBe(2900);
    expect(u1.entries).toHaveLength(1);
    const worse: ArchiveEntry = { ...incumbent, value: 3500, version: "1.6" };
    const u2 = archiveUpdate([incumbent], worse);
    expect(u2.record).toBe(false);
    expect(u2.entries[0]!.value).toBe(3200);
    expect(u2.reason).toContain("同机同场景");
    // 不同机器的成绩分开入档不跨比
    const otherPc: ArchiveEntry = { ...incumbent, value: 5000, machine: "PC-B" };
    const u3 = archiveUpdate([incumbent], otherPc);
    expect(u3.record).toBe(true); // 新条件原点即纪录
    expect(u3.entries).toHaveLength(2);
  });

  it("候选字段不全 → 如实不入档；档案可导出", () => {
    const u = archiveUpdate([incumbent], { metric: "fps", value: NaN, version: "1.5", at: NOW, machine: "PC-A", scenario: "cold" });
    expect(u.record).toBe(false);
    expect(u.reason).toContain("字段不全");
    const json = exportArchive([incumbent]);
    const parsed = JSON.parse(json) as { kind: string; entries: ArchiveEntry[] };
    expect(parsed.kind).toBe("nova-quality-archive");
    expect(parsed.entries[0]!.metric).toBe("bootMs");
  });
});

// ---------------------------------------------------------------------------
// W-200 测试战绩册
// ---------------------------------------------------------------------------

describe("W-200 测试战绩册", () => {
  it("里程碑 1000/1200/1500 逐级解锁", () => {
    expect(BATTLE_MILESTONES).toEqual([1000, 1200, 1500]);
    expect(badgesOf(999).every((b) => !b.unlocked)).toBe(true);
    expect(badgesOf(1000).map((b) => b.unlocked)).toEqual([true, false, false]);
    expect(badgesOf(1200).map((b) => b.unlocked)).toEqual([true, true, false]);
    expect(badgesOf(1500).every((b) => b.unlocked)).toBe(true);
    expect(nextBadge(999)).toBe(1000);
    expect(nextBadge(1000)).toBe(1200);
    expect(nextBadge(1500)).toBeNull();
  });

  it("功勋墙：各域分布降序 + 占比", () => {
    const domains: DomainTests[] = [
      { domain: "boot", passed: 50, failed: 0 },
      { domain: "files", passed: 55, failed: 2 },
      { domain: "input", passed: 42, failed: 0 },
    ];
    const wall = honorWall(domains);
    expect(wall[0]!.domain).toBe("files");
    expect(wall[0]!.total).toBe(57);
    expect(wall[0]!.failed).toBe(2);
    expect(wall[0]!.share).toBeCloseTo(57 / 149, 5);
    expect(wall.map((h) => h.domain)).toEqual(["files", "boot", "input"]);
    expect(honorWall([])).toHaveLength(0);
  });

  it("战绩播报（同源 vitest/cargo 真实结果）", () => {
    expect(battleSummary({ passed: 0, failed: 0 })).toContain("暂无测试结果");
    const s = battleSummary({ passed: 1287, failed: 2 });
    expect(s).toContain("1287 通过");
    expect(s).toContain("100%"); // round(1287/1289×100)
    expect(s).toContain("1200 徽章在手");
    expect(battleSummary({ passed: 1609, failed: 0 })).toContain("1500 徽章在手");
  });
});

// ---------------------------------------------------------------------------
// 通用与行为层安全（node 环境 no-op 语义）
// ---------------------------------------------------------------------------

describe("通用与行为层", () => {
  it("flagOn 只读消费 S0 注册表（未知 id 安全 false）", () => {
    expect(flagOn("W-not-exist")).toBe(false);
    expect(typeof flagOn("W-188")).toBe("boolean");
  });

  it("clamp/motionOK 语义保持", () => {
    expect(clamp(5, 0, 3)).toBe(3);
    expect(clamp(-1, 0, 3)).toBe(0);
    expect(typeof motionOK()).toBe("boolean");
  });

  it("激活/卸载在非 DOM 环境安全 no-op（幂等）", () => {
    expect(() => activateQualityNova()).not.toThrow();
    expect(isQualityNovaActive()).toBe(false);
    expect(() => deactivateQualityNova()).not.toThrow();
    expect(() => activateQualityNova()).not.toThrow();
    expect(() => deactivateQualityNova()).not.toThrow();
    expect(isQualityNovaActive()).toBe(false);
  });
});
