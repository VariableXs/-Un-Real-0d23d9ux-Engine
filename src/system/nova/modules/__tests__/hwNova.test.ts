/**
 * NOVA-200 S8 硬件感知路（AI-08）· hwNova 纯逻辑单测。
 * 行为层在 node 环境下 no-op（真实 DOM 激活需浏览器），此处验证：
 * manifest 完整性、潮汐映射、电池叙事模板、风铃计划、川流映射、分核聚合、
 * 巡逻灯限流、电流仪式、工时账本、风扇补偿、USB 电流印、生日书、分贝伴飞。
 */

import { describe, expect, it } from "vitest";
import {
  BOLT_DEBOUNCE_MS,
  BOLT_FLASH_MS,
  CHIME_MS,
  CHIME_VOLUME,
  CHORUS_BAR_MAX_PX,
  CHORUS_BAR_MIN_PX,
  CHORUS_MAX_COLUMNS,
  DB_HIGH,
  DB_LOW,
  FAN_GAIN_DB,
  FAN_HIGH_PCT,
  FAN_LOW_PCT,
  HW_NOVA_FEATURES,
  LED_AGGREGATE_MAX,
  LED_MIN_INTERVAL_MS,
  OLED_DWELL_HINT_MS,
  STREAM_WASH_PCT,
  TIDE_DISCRETE_CORES,
  USB_HUNGRY_MA,
  USB_HISTORY_MAX,
  aggregateGroups,
  batteryStory,
  birthdayCheck,
  boltDebounced,
  boltPlan,
  chorusColumnPct,
  chorusColumns,
  coreBarHeight,
  countChargeCycles,
  currentPrint,
  dbAdjust,
  dbRingAvg,
  dbRingPush,
  diskLedFlash,
  diskLampPlan,
  diskRateText,
  fanCompensateDb,
  fanHysteresis,
  hwNovaDomain,
  isNightHour,
  micChimePlan,
  chimeFreqs,
  novaEvent,
  oledHints,
  panelAccumulate,
  panelReset,
  streamBand,
  tideDensity,
  tideLevelDiscrete,
  tideParticleCount,
  usbHistoryPush,
} from "../hwNova";
import type { BatterySample, DiskActivity, HwBirthdayEntry, UsbCurrentRow } from "../hwNova";

const HOUR = 3_600_000;

/** 固定基准：2026-09-10 12:00 local（与装机周年样例对齐）。 */
const T0 = new Date(2026, 8, 10, 12, 0, 0).getTime();

// ---------------------------------------------------------------------------
// manifest：12 项齐、编号连续、域标识、overlay 与注册表对齐
// ---------------------------------------------------------------------------
describe("hwNova manifest", () => {
  it("W-090…W-101 共 12 项，编号连续无缺", () => {
    expect(HW_NOVA_FEATURES).toHaveLength(12);
    const ids = HW_NOVA_FEATURES.map((f) => Number(f.id.slice(2)));
    for (let i = 1; i < ids.length; i++) expect(ids[i]).toBe(ids[i - 1]! + 1);
    expect(ids[0]).toBe(90);
    expect(ids[ids.length - 1]).toBe(101);
  });

  it("每项必含中英标题/描述/降级说明，域标识 S8/AI-08", () => {
    for (const f of HW_NOVA_FEATURES) {
      expect(f.titleZh.length).toBeGreaterThan(0);
      expect(f.titleEn.length).toBeGreaterThan(0);
      expect(f.descZh.length).toBeGreaterThan(0);
      expect(f.degrade.length).toBeGreaterThan(0);
    }
    expect(hwNovaDomain.id).toBe("S8");
    expect(hwNovaDomain.route).toBe("AI-08");
    expect(hwNovaDomain.features).toBe(HW_NOVA_FEATURES);
  });

  it("overlay 工具窗与注册表对齐（battery/panelhours）", () => {
    const overlayOf = (id: string): string | undefined =>
      HW_NOVA_FEATURES.find((f) => f.id === id)?.overlay;
    expect(overlayOf("W-091")).toBe("nova-battery");
    expect(overlayOf("W-097")).toBe("nova-panelhours");
    expect(overlayOf("W-090")).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// W-090 CPU 潮汐
// ---------------------------------------------------------------------------
describe("W-090 CPU 潮汐", () => {
  it("密度映射单调（cpu 升密度不降；intensity 升密度不降）", () => {
    for (let cpu = 0; cpu <= 100; cpu += 5) {
      for (let amp = 0; amp <= 100; amp += 10) {
        expect(tideDensity(cpu, amp)).toBeGreaterThanOrEqual(tideDensity(Math.max(0, cpu - 5), amp));
        expect(tideDensity(cpu, amp)).toBeGreaterThanOrEqual(tideDensity(cpu, Math.max(0, amp - 10)));
      }
    }
  });

  it("密度边界：0/0 → 0.15 基线；100/100 → 1", () => {
    expect(tideDensity(0, 0)).toBe(0.15);
    expect(tideDensity(100, 100)).toBe(1);
  });

  it("三档离散潮位阈值（33/66）", () => {
    expect(tideLevelDiscrete(0)).toBe("low");
    expect(tideLevelDiscrete(32.9)).toBe("low");
    expect(tideLevelDiscrete(33)).toBe("mid");
    expect(tideLevelDiscrete(65.9)).toBe("mid");
    expect(tideLevelDiscrete(66)).toBe("high");
    expect(tideLevelDiscrete(100)).toBe("high");
  });

  it("粒子数量映射：基线 30%、满编制、基数 0 → 0", () => {
    expect(tideParticleCount(100, 0)).toBe(30);
    expect(tideParticleCount(100, 1)).toBe(100);
    expect(tideParticleCount(0, 1)).toBe(0);
    expect(tideParticleCount(60, 0.5)).toBe(39);
  });

  it("≤4 核降档常量与离散判定素材齐备", () => {
    expect(TIDE_DISCRETE_CORES).toBe(4);
  });
});

// ---------------------------------------------------------------------------
// W-091 电池健康叙事
// ---------------------------------------------------------------------------
describe("W-091 电池健康叙事", () => {
  const mk = (ts: number, pct: number, charging: boolean): BatterySample => ({ ts, pct, charging });

  it("循环计数（行业标准累计口径）：220% 充入 = 2 循环，放电段不清账", () => {
    const samples: BatterySample[] = [
      mk(0, 10, true),
      mk(HOUR, 60, true),
      mk(2 * HOUR, 100, true), // 段1 +90
      mk(3 * HOUR, 40, false), // 放电（不清账）
      mk(4 * HOUR, 40, true),
      mk(5 * HOUR, 100, true), // 段2 +60
      mk(6 * HOUR, 100, true), // 平台无增量
      mk(7 * HOUR, 30, false),
      mk(8 * HOUR, 30, true),
      mk(9 * HOUR, 90, true),
      mk(10 * HOUR, 100, true), // 段3 +70
    ];
    // 累计 220% → 2 循环
    expect(countChargeCycles(samples)).toBe(2);
  });

  it("循环计数：160% 充入 = 1 循环（不足 100% 尾数不虚计）", () => {
    const samples: BatterySample[] = [
      mk(0, 0, true),
      mk(HOUR, 50, true),
      mk(2 * HOUR, 100, true), // +100
      mk(3 * HOUR, 50, false),
      mk(4 * HOUR, 50, true),
      mk(5 * HOUR, 100, true), // +50
      mk(6 * HOUR, 90, false),
      mk(7 * HOUR, 100, true), // +10
    ];
    expect(countChargeCycles(samples)).toBe(1);
  });

  it("空样本如实 0 循环", () => {
    expect(countChargeCycles([])).toBe(0);
  });

  it("人话结论四分支（更累/正常损耗/平稳/回升）", () => {
    expect(batteryStory({ cyclesThisMonth: 10, healthNowPct: 90, healthPrevPct: 93, avgFullDrainPerDayPct: 50 }).headline).toContain("更累");
    expect(batteryStory({ cyclesThisMonth: 10, healthNowPct: 90, healthPrevPct: 91, avgFullDrainPerDayPct: 50 }).headline).toContain("正常损耗");
    expect(batteryStory({ cyclesThisMonth: 10, healthNowPct: 90, healthPrevPct: 90, avgFullDrainPerDayPct: 50 }).headline).toContain("平稳");
    expect(batteryStory({ cyclesThisMonth: 10, healthNowPct: 91, healthPrevPct: 90, avgFullDrainPerDayPct: 50 }).headline).toContain("回升");
  });

  it("上月样本不足 → 趋势如实写不足（不猜测）", () => {
    const s = batteryStory({ cyclesThisMonth: 10, healthNowPct: 90, healthPrevPct: null, avgFullDrainPerDayPct: null });
    expect(s.trendPct).toBeNull();
    expect(s.trendText).toContain("不足");
  });

  it("三条建议按数据规则生成，无异常时给通用一条", () => {
    const s = batteryStory({ cyclesThisMonth: 80, healthNowPct: 78, healthPrevPct: 80, avgFullDrainPerDayPct: 70 });
    expect(s.tips).toHaveLength(3);
    const s2 = batteryStory({ cyclesThisMonth: 10, healthNowPct: 95, healthPrevPct: 95, avgFullDrainPerDayPct: 30 });
    expect(s2.tips).toHaveLength(1);
    expect(s2.tips[0]).toContain("没有异常项");
  });
});

// ---------------------------------------------------------------------------
// W-092 麦克风风铃
// ---------------------------------------------------------------------------
describe("W-092 麦克风风铃", () => {
  it("启用瞬间：120ms / 20% 恒定", () => {
    expect(CHIME_MS).toBe(120);
    expect(CHIME_VOLUME).toBe(0.2);
    const plan = micChimePlan({ enabled: true, dnd: false });
    expect(plan).toEqual({ play: true, ms: 120, volume: 0.2 });
  });

  it("勿扰静默（灯通道保留，reason=dnd）", () => {
    const plan = micChimePlan({ enabled: true, dnd: true });
    expect(plan.play).toBe(false);
    expect(plan.reason).toBe("dnd");
  });

  it("未启用不响", () => {
    expect(micChimePlan({ enabled: false, dnd: false }).reason).toBe("not-enabled");
  });

  it("三材质基频对齐（木质 880/1320）", () => {
    expect(chimeFreqs("wood")).toEqual([880, 1320]);
    expect(chimeFreqs("bamboo")).not.toEqual(chimeFreqs("metal"));
  });
});

// ---------------------------------------------------------------------------
// W-093 内存川流带
// ---------------------------------------------------------------------------
describe("W-093 内存川流带", () => {
  it("占用升 → 流速加快（12s→3s）、亮度增强（0.35→1）", () => {
    const lo = streamBand(0);
    const hi = streamBand(100);
    expect(lo).toEqual({ speedSec: 12, brightness: 0.35, wash: false });
    expect(hi).toEqual({ speedSec: 3, brightness: 1, wash: true });
    for (let pct = 5; pct <= 100; pct += 5) {
      expect(streamBand(pct).speedSec).toBeLessThanOrEqual(streamBand(pct - 5).speedSec);
      expect(streamBand(pct).brightness).toBeGreaterThanOrEqual(streamBand(pct - 5).brightness);
    }
  });

  it("急流泛白阈值 85%", () => {
    expect(STREAM_WASH_PCT).toBe(85);
    expect(streamBand(84.9).wash).toBe(false);
    expect(streamBand(85).wash).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// W-094 核心合唱
// ---------------------------------------------------------------------------
describe("W-094 核心合唱", () => {
  it("≤32 逻辑核一核一柱", () => {
    const l8 = chorusColumns(8);
    expect(l8).toEqual({ columns: 8, paired: false, groups: [[0], [1], [2], [3], [4], [5], [6], [7]] });
    const l32 = chorusColumns(32);
    expect(l32.columns).toBe(32);
    expect(l32.paired).toBe(false);
  });

  it("64 逻辑核 → 32 列超线程对聚合（i 与 i+32）", () => {
    const l = chorusColumns(64);
    expect(l.columns).toBe(32);
    expect(l.paired).toBe(true);
    expect(l.groups[0]).toEqual([0, 32]);
    expect(l.groups[31]).toEqual([31, 63]);
  });

  it("48 逻辑核（偶数）→ 24 列超线程对聚合，覆盖全部核不丢不重", () => {
    const l = chorusColumns(48);
    expect(l.columns).toBe(24);
    expect(l.paired).toBe(true);
    expect(l.groups[0]).toEqual([0, 24]);
    expect(l.groups[23]).toEqual([23, 47]);
    const flat = l.groups.flat();
    expect(flat).toHaveLength(48);
    expect(new Set(flat).size).toBe(48);
  });

  it("33 逻辑核（奇数）→ 32 列诚实聚合，覆盖全部核不丢不重", () => {
    const l = chorusColumns(33);
    expect(l.columns).toBe(CHORUS_MAX_COLUMNS);
    const flat = l.groups.flat();
    expect(flat).toHaveLength(33);
    expect(new Set(flat).size).toBe(33);
  });

  it("波形柱高 2–14px 线性", () => {
    expect(CHORUS_BAR_MIN_PX).toBe(2);
    expect(CHORUS_BAR_MAX_PX).toBe(14);
    expect(coreBarHeight(0)).toBe(2);
    expect(coreBarHeight(50)).toBe(8);
    expect(coreBarHeight(100)).toBe(14);
  });

  it("列均值：组内均值、越界核按 0 计（诚实钳制）", () => {
    expect(chorusColumnPct([50, 90], [0, 1])).toBe(70);
    expect(chorusColumnPct([10], [0])).toBe(10);
    expect(chorusColumnPct([], [0])).toBe(0);
    expect(chorusColumnPct([80, 200], [0, 1])).toBe(90); // 200 钳到 100
  });

  it("aggregateGroups 均匀铺核", () => {
    const g = aggregateGroups(10, 4);
    expect(g.map((x) => x.length).sort()).toEqual([2, 2, 3, 3]);
  });
});

// ---------------------------------------------------------------------------
// W-095 磁盘巡逻灯
// ---------------------------------------------------------------------------
describe("W-095 磁盘巡逻灯", () => {
  it("闪频限流 4Hz（250ms）", () => {
    expect(LED_MIN_INTERVAL_MS).toBe(250);
    expect(diskLedFlash(false, 0, 1000)).toBe(false);
    expect(diskLedFlash(true, 900, 1000)).toBe(false); // 距上次 100ms
    expect(diskLedFlash(true, 750, 1000)).toBe(true); // 距上次 250ms
  });

  it("≤4 盘逐盘点亮；>4 盘聚合单灯 + 数字", () => {
    const mk = (name: string, busy = false): DiskActivity => ({ name, busy, rateBps: null });
    const four = [mk("C"), mk("D"), mk("E"), mk("F")];
    expect(diskLampPlan(four)).toEqual({ aggregate: false, shown: four, extra: 0 });
    const six = [mk("C"), mk("D"), mk("E"), mk("F"), mk("G"), mk("H")];
    const plan = diskLampPlan(six);
    expect(plan.aggregate).toBe(true);
    expect(plan.shown).toHaveLength(1);
    expect(plan.extra).toBe(5);
    expect(LED_AGGREGATE_MAX).toBe(4);
  });

  it("速率文案：不可读 N/A，B/KB/MB 三阶", () => {
    expect(diskRateText(null)).toBe("N/A");
    expect(diskRateText(undefined)).toBe("N/A");
    expect(diskRateText(512)).toBe("512 B/s");
    expect(diskRateText(2048)).toBe("2.0 KB/s");
    expect(diskRateText(3 * 1024 * 1024)).toBe("3.0 MB/s");
  });
});

// ---------------------------------------------------------------------------
// W-096 电流仪式
// ---------------------------------------------------------------------------
describe("W-096 电流仪式", () => {
  it("插入 → 落雷 200ms + 提示音；拔出 → 反向细流无声", () => {
    expect(BOLT_FLASH_MS).toBe(200);
    const plug = boltPlan("plug", "thunder", false);
    expect(plug).toEqual({ flash: "thunder", ms: 200, sound: true });
    const unplug = boltPlan("unplug", "thunder", false);
    expect(unplug.flash).toBe("trickle");
    expect(unplug.sound).toBe(false);
  });

  it("静默制式无光无声；夜间一律不响（W-004 联动）", () => {
    expect(boltPlan("plug", "silence", false)).toEqual({ flash: null, ms: 0, sound: false });
    expect(boltPlan("plug", "thunder", true).sound).toBe(false);
    expect(boltPlan("unplug", "waterfall", true).sound).toBe(false);
  });

  it("夜间时段 22:00–07:00", () => {
    expect(isNightHour(23)).toBe(true);
    expect(isNightHour(3)).toBe(true);
    expect(isNightHour(0)).toBe(true);
    expect(isNightHour(7)).toBe(false);
    expect(isNightHour(12)).toBe(false);
    expect(isNightHour(21)).toBe(false);
    expect(isNightHour(22)).toBe(true);
  });

  it("插拔 2s 防抖", () => {
    expect(BOLT_DEBOUNCE_MS).toBe(2000);
    expect(boltDebounced(1000, null)).toBe(true);
    expect(boltDebounced(1000, 500)).toBe(false);
    expect(boltDebounced(1000, 500 - BOLT_DEBOUNCE_MS - 1)).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// W-097 屏幕工时簿
// ---------------------------------------------------------------------------
describe("W-097 屏幕工时簿", () => {
  const rec = { hours: 1, oled: true, staticSinceMs: null, updated: 0 };

  it("工时累计幂等加法（60s = 0.02h 舍入）", () => {
    const next = panelAccumulate(rec, 60, T0);
    expect(next.hours).toBeCloseTo(1.02, 2);
    expect(next.updated).toBe(T0);
    expect(rec.hours).toBe(1); // 原记录不可变
  });

  it("清零可还原", () => {
    const zero = panelReset({ ...rec, hours: 42.5 }, T0);
    expect(zero.hours).toBe(0);
    expect(zero.staticSinceMs).toBeNull();
  });

  it("非 OLED 如实跳过烧屏建议", () => {
    expect(oledHints({ ...rec, oled: false, staticSinceMs: T0 - OLED_DWELL_HINT_MS - 1 }, T0)).toEqual([]);
  });

  it("OLED 驻留超 4h → 轮换建议；点亮 ≥8h → 追加留意", () => {
    expect(OLED_DWELL_HINT_MS).toBe(4 * HOUR);
    const hints = oledHints({ ...rec, staticSinceMs: T0 - OLED_DWELL_HINT_MS - 1, hours: 8.2 }, T0);
    expect(hints).toHaveLength(2);
    expect(hints[0]).toContain("轮换");
    expect(hints[1]).toContain("8 小时");
    expect(oledHints({ ...rec, staticSinceMs: T0 - HOUR, hours: 3 }, T0)).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// W-098 风扇声纹补偿
// ---------------------------------------------------------------------------
describe("W-098 风扇声纹补偿", () => {
  it("高速 ≥70% → +6dB；夜间/关闭恒 0", () => {
    expect(FAN_HIGH_PCT).toBe(70);
    expect(FAN_GAIN_DB).toBe(6);
    expect(fanCompensateDb(70, false, true)).toBe(6);
    expect(fanCompensateDb(69, false, true)).toBe(0);
    expect(fanCompensateDb(100, true, true)).toBe(0);
    expect(fanCompensateDb(100, false, false)).toBe(0);
  });

  it("迟滞回线：70 触发 / 55 解除，防临界抖动", () => {
    expect(FAN_LOW_PCT).toBe(55);
    expect(fanHysteresis(false, 69)).toBe(false);
    expect(fanHysteresis(false, 70)).toBe(true);
    expect(fanHysteresis(true, 56)).toBe(true);
    expect(fanHysteresis(true, 55)).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// W-099 USB 电流印
// ---------------------------------------------------------------------------
describe("W-099 USB 电流印", () => {
  it("电流行：协商/实际，不可读一侧如实 N/A", () => {
    const r = currentPrint("移动硬盘", 900, 480);
    expect(r.row).toBe("协商 900mA · 实际 480mA");
    expect(r.hungry).toBe(false);
    const na = currentPrint("U 盘", null, null);
    expect(na.row).toBe("协商 N/A · 实际 N/A");
    expect(na.hungry).toBe(false);
  });

  it("吃电大户阈值 900mA", () => {
    expect(USB_HUNGRY_MA).toBe(900);
    expect(currentPrint("2.5 寸硬盘", 900, 900).hungry).toBe(true);
    expect(currentPrint("键鼠", 100, 80).hungry).toBe(false);
  });

  it("历史 FIFO 上限 5", () => {
    expect(USB_HISTORY_MAX).toBe(5);
    let list: UsbCurrentRow[] = [];
    for (let i = 0; i < 7; i++) list = usbHistoryPush(list, currentPrint(`D${i}`, 500, 100));
    expect(list).toHaveLength(5);
    expect(list[0]!.name).toBe("D2");
    expect(list[4]!.name).toBe("D6");
  });
});

// ---------------------------------------------------------------------------
// W-100 硬件生日书
// ---------------------------------------------------------------------------
describe("W-100 硬件生日书", () => {
  const entries: HwBirthdayEntry[] = [
    { id: "ssd", name: "三星 SSD", installMs: new Date(2023, 8, 10).getTime(), healthLine: "健康度 92%，仍能打" },
    { id: "ram", name: "内存条", installMs: new Date(2026, 8, 10).getTime() }, // 当日装机（0 周年）
    { id: "gpu", name: "显卡", installMs: new Date(2021, 8, 11).getTime() }, // 明日周年
    { id: "bad", name: "幽灵盘", installMs: Number.NaN }, // 读不到日期
  ];

  it("周年当日入册（年数 ≥1），蛋糕文案 + 健康一句话透传", () => {
    const list = birthdayCheck(entries, T0);
    expect(list).toHaveLength(1);
    expect(list[0]!.id).toBe("ssd");
    expect(list[0]!.line).toContain("装机 3 周年");
    expect(list[0]!.healthLine).toContain("92%");
  });

  it("0 周年 / 非当日 / 无日期：永不入册（不编造）", () => {
    const list = birthdayCheck(entries, T0);
    expect(list.find((a) => a.id === "ram")).toBeUndefined();
    expect(list.find((a) => a.id === "gpu")).toBeUndefined();
    expect(list.find((a) => a.id === "bad")).toBeUndefined();
  });
});

// ---------------------------------------------------------------------------
// W-101 环境分贝伴飞
// ---------------------------------------------------------------------------
describe("W-101 环境分贝伴飞", () => {
  it("3s 滚动窗口：过期样本丢弃，纯内存", () => {
    const t = 1_000_000;
    let ring = dbRingPush([], 50, t);
    ring = dbRingPush(ring, 60, t + 1000);
    ring = dbRingPush(ring, 70, t + 2500);
    expect(ring).toHaveLength(3);
    ring = dbRingPush(ring, 80, t + 4500); // 窗口 [t+1500, t+4500]：50/60 过期
    expect(ring).toHaveLength(2);
    expect(ring[0]!.db).toBe(70);
  });

  it("3s 均值（空样本如实 null）", () => {
    expect(dbRingAvg([])).toBeNull();
    expect(dbRingAvg([{ db: 50, ts: 0 }, { db: 70, ts: 1000 }])).toBe(60);
  });

  it("阈值判定：>65 提振 / <35 微降 / 中间不动 / null 全不动", () => {
    expect(DB_HIGH).toBe(65);
    expect(DB_LOW).toBe(35);
    expect(dbAdjust(null)).toEqual({ notifyBoost: false, mediaDuck: false });
    expect(dbAdjust(65)).toEqual({ notifyBoost: false, mediaDuck: false });
    expect(dbAdjust(65.1)).toEqual({ notifyBoost: true, mediaDuck: false });
    expect(dbAdjust(35)).toEqual({ notifyBoost: false, mediaDuck: false });
    expect(dbAdjust(34.9)).toEqual({ notifyBoost: false, mediaDuck: true });
    expect(dbAdjust(50)).toEqual({ notifyBoost: false, mediaDuck: false });
  });
});

// ---------------------------------------------------------------------------
// 环境安全（node 下事件派发不抛错）
// ---------------------------------------------------------------------------
describe("环境安全", () => {
  it("novaEvent 在无 window 环境静默 no-op", () => {
    expect(() => novaEvent("tide", { density: 0.5 })).not.toThrow();
  });
});
