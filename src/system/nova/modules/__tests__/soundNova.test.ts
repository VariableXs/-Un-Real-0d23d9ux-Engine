/**
 * NOVA-200 · S13 声音通知路（AI-13）单测 —— soundNova（W-152…W-163）。
 *
 * 覆盖：manifest 契约（12 项连续/双语/默认档与 S0 一致）+ 十二项纯逻辑 +
 * 行为层非 DOM 安全性。验收口径：
 * - W-152 导览 ≤180s、改音夹取（±600¢/-12…+6dB）；
 * - W-154 无 TTS 如实跳过（RADIO_UNAVAILABLE）、90s 预算；
 * - W-155 未校准 0dB、补偿 ±3dB 上限；
 * - W-156 雨滴条数守恒、420ms 间隔；
 * - W-157 三段曲线边界、手动让位 60min；
 * - W-158 会话外静默、每整点至多一声、会话始整点不报；
 * - W-162 队列上限 5 超出转存档零丢失；
 * - W-163 同频段 60ms 错峰、跨频段零延迟。
 */

import { describe, expect, it } from "vitest";
import {
  BALANCE_DB_LIMIT,
  CHIME_MS,
  DAYPART_MANUAL_YIELD_MS,
  HISTORY_CAP,
  PIANO_NOTE_ZH,
  RADIO_BUDGET_SEC,
  RAIN_DROP_GAP_MS,
  SOUND_LIBRARY,
  SOUND_NOVA_FEATURES,
  SPOTLIGHT_CAP,
  SPOTLIGHT_STAGE_MS,
  STAGGER_MS,
  TREMOR_MAX_PULSES,
  TREMOR_PULSE_MS,
  TWEAK_GAIN_MAX,
  TWEAK_GAIN_MIN,
  TWEAK_PITCH_LIMIT,
  activateSoundNova,
  applyBalance,
  applyTweak,
  balanceDb,
  balanceFactor,
  bandOf,
  chimeDue,
  chimeMark,
  clamp,
  clampTweak,
  dayCurve,
  daypartManual,
  daypartPct,
  deactivateSoundNova,
  emptyGrid,
  flagOn,
  heatColor,
  heatDeposit,
  heatLevel,
  heatMax,
  heatPeaks,
  historyLookup,
  hourFloor,
  isSoundNovaActive,
  motionOK,
  pianoKeys,
  pianoSoundId,
  pushHistory,
  radioScript,
  radioTotalSec,
  radioVerdict,
  rainDrops,
  rainGain,
  soundEvent,
  spotlightDequeue,
  spotlightEnqueue,
  spotlightTotal,
  staggerAssign,
  tourTimeline,
  tremorPulses,
  tremorTimeline,
  clampTweak as tweakClamp,
} from "../soundNova";
import { NOVA_FEATURES } from "../../registry";
import type { SpotlightLedger } from "../soundNova";

const HOUR = 3_600_000;
const T0 = Date.UTC(2026, 8, 10, 2, 0, 0); // 2026-09-10T02:00Z（周四 10:00 北京）

// ---------------------------------------------------------------------------
// manifest 契约
// ---------------------------------------------------------------------------

describe("SOUND_NOVA_FEATURES manifest", () => {
  it("12 项连续且与 S0 注册表对齐", () => {
    expect(SOUND_NOVA_FEATURES.map((f) => f.id)).toEqual([
      "W-152", "W-153", "W-154", "W-155", "W-156", "W-157",
      "W-158", "W-159", "W-160", "W-161", "W-162", "W-163",
    ]);
    for (const f of SOUND_NOVA_FEATURES) {
      const s0 = NOVA_FEATURES.find((x) => x.id === f.id);
      expect(s0, `${f.id} 必须在 S0 注册表`).toBeTruthy();
      expect(s0!.domain).toBe("sound");
      expect(f.defaultOn).toBe(s0!.on);
      expect(f.titleEn.length).toBeGreaterThan(0);
      expect(f.descZh.length).toBeGreaterThan(0);
      expect(f.degrade.length).toBeGreaterThan(0);
    }
  });

  it("W-154 opt-in 默认关；overlay 与 S0 一致", () => {
    expect(SOUND_NOVA_FEATURES.find((f) => f.id === "W-154")!.defaultOn).toBe(false);
    const ov = SOUND_NOVA_FEATURES.filter((f) => f.overlay);
    for (const f of ov) expect(NOVA_FEATURES.find((x) => x.id === f.id)!.overlay).toBe(f.overlay);
    expect(ov.map((f) => f.overlay)).toEqual(["nova-museum", "nova-heat", "nova-piano"]);
  });
});

// ---------------------------------------------------------------------------
// 工具
// ---------------------------------------------------------------------------

describe("工具", () => {
  it("clamp", () => {
    expect(clamp(5, 0, 3)).toBe(3);
    expect(clamp(-2, 0, 3)).toBe(0);
    expect(clamp(1, 0, 3)).toBe(1);
  });

  it("非 DOM 环境安全：soundEvent / motionOK / flagOn 不抛错", () => {
    expect(() => soundEvent("ping", { a: 1 })).not.toThrow();
    expect(typeof flagOn("W-152")).toBe("boolean");
    expect(typeof motionOK()).toBe("boolean");
  });
});

// ---------------------------------------------------------------------------
// W-152 声纹博物馆
// ---------------------------------------------------------------------------

describe("W-152 声纹博物馆", () => {
  it("十二展品、3 分钟导览恰满", () => {
    expect(SOUND_LIBRARY).toHaveLength(12);
    const { stops, totalSec } = tourTimeline();
    expect(stops).toHaveLength(12);
    expect(totalSec).toBeLessThanOrEqual(180);
    expect(totalSec).toBe(180);
    expect(stops[0]!.atSec).toBe(0);
    expect(stops[11]!.atSec).toBe(165);
    expect(stops.every((s) => s.durationSec === 15)).toBe(true);
  });

  it("就地改音：夹取 ±600¢ / -12…+6dB，半 dB 步进", () => {
    expect(clampTweak({ soundId: "s", pitchCents: 9999, gainDb: 99 })).toEqual({
      soundId: "s",
      pitchCents: TWEAK_PITCH_LIMIT,
      gainDb: TWEAK_GAIN_MAX,
    });
    expect(clampTweak({ soundId: "s", pitchCents: -9999, gainDb: -99 }).gainDb).toBe(TWEAK_GAIN_MIN);
    expect(clampTweak({ soundId: "s", pitchCents: 137, gainDb: 1.4 }).gainDb).toBe(1.5);
    const base = SOUND_LIBRARY[0]!;
    const mixed = applyTweak(base, { soundId: base.soundId, pitchCents: 200, gainDb: -3 });
    expect(mixed.tweak).toEqual({ soundId: base.soundId, pitchCents: 200, gainDb: -3 });
    expect(applyTweak(base, undefined).tweak.gainDb).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// W-153 通知时间热图
// ---------------------------------------------------------------------------

describe("W-153 通知时间热图", () => {
  it("7×24 沉积与五档热级", () => {
    const g = emptyGrid();
    expect(g).toHaveLength(7);
    expect(g[0]).toHaveLength(24);
    // T0 = 周四（getDay 由本地时区决定，直接读回验证一致性）
    const d = new Date(T0);
    let g2 = heatDeposit(g, T0);
    g2 = heatDeposit(g2, T0 + 1);
    g2 = heatDeposit(g2, T0 + 2);
    expect(g2[d.getDay()]![d.getHours()]!).toBe(3);
    expect(heatMax(g2)).toBe(3);
    expect(heatLevel(0, 3)).toBe(0);
    expect(heatLevel(1, 4)).toBe(1);
    expect(heatLevel(1, 3)).toBe(2);
    expect(heatLevel(2, 3)).toBe(3);
    expect(heatLevel(3, 3)).toBe(4);
    expect(heatLevel(3, 0)).toBe(0);
  });

  it("高峰并列全列 + 色阶变量", () => {
    let g = emptyGrid();
    g = heatDeposit(g, T0);
    const peaks = heatPeaks(g);
    expect(peaks).toHaveLength(1);
    expect(peaks[0]!.count).toBe(1);
    expect(heatPeaks(emptyGrid())).toEqual([]);
    expect(heatColor(3)).toBe("var(--nova-sound-heat-3)");
  });
});

// ---------------------------------------------------------------------------
// W-154 晨间电台
// ---------------------------------------------------------------------------

describe("W-154 晨间电台", () => {
  const input = { weatherZh: "多云 18-26 度", agendaItems: ["站会", "评审"], quoteZh: "慢即是快。" };

  it("三段脚本恰 90s 预算", () => {
    const segs = radioScript(input);
    expect(segs.map((s) => s.key)).toEqual(["weather", "agenda", "quote"]);
    expect(radioTotalSec(segs)).toBe(RADIO_BUDGET_SEC);
  });

  it("长文案按 4 字/秒裁剪并留省略号", () => {
    const segs = radioScript({ weatherZh: "长".repeat(500), agendaItems: [], quoteZh: "短" });
    expect(segs[0]!.text.length).toBeLessThanOrEqual(30 * 4 - 2);
    expect(segs[0]!.text.endsWith("…")).toBe(true);
    expect(segs[1]!.text).toContain("没有排定的日程");
  });

  it("无 TTS 如实跳过（RADIO_UNAVAILABLE）；超预算拒播", () => {
    const segs = radioScript(input);
    const noTts = radioVerdict(false, segs);
    expect(noTts).toEqual({ ok: false, code: "RADIO_UNAVAILABLE", reason: expect.stringContaining("TTS") });
    expect(radioVerdict(true, segs).ok).toBe(true);
    const fat = segs.map((s) => ({ ...s, sec: s.sec + 10 }));
    expect(radioVerdict(true, fat)).toMatchObject({ ok: false, code: "RADIO_OVER_BUDGET" });
  });
});

// ---------------------------------------------------------------------------
// W-155 双耳平衡罗盘
// ---------------------------------------------------------------------------

describe("W-155 双耳平衡罗盘", () => {
  it("未校准 = 0dB（不猜）", () => {
    expect(balanceFactor([])).toBe(0);
    expect(balanceDb(0)).toEqual({ dbL: 0, dbR: 0 });
  });

  it("右占优补左、左占优补右，上限 ±3dB，0.5dB 步进", () => {
    const right = balanceFactor([
      { said: -1, actual: 1 },
      { said: 0, actual: 0 },
      { said: -1, actual: 0.5 },
    ]);
    expect(right).toBeGreaterThan(0);
    const comp = balanceDb(right);
    expect(comp.dbR).toBe(0);
    expect(comp.dbL).toBeGreaterThan(0);
    expect(comp.dbL).toBeLessThanOrEqual(BALANCE_DB_LIMIT);
    expect(Math.round(comp.dbL * 2)).toBe(comp.dbL * 2);
    expect(balanceDb(-1)).toEqual({ dbL: 0, dbR: 3 });
    expect(applyBalance(-6, { dbL: 1.5, dbR: 0 })).toEqual({ dbL: -4.5, dbR: -6 });
  });
});

// ---------------------------------------------------------------------------
// W-156 勿扰雨滴
// ---------------------------------------------------------------------------

describe("W-156 勿扰雨滴", () => {
  it("每条通知恰一声，420ms 间隔，条数守恒", () => {
    const notifs = Array.from({ length: 7 }, (_, i) => ({ id: `n${i}`, at: T0 }));
    const drops = rainDrops(notifs);
    expect(drops).toHaveLength(7);
    for (let i = 1; i < drops.length; i++) {
      expect(drops[i]!.atMs - drops[i - 1]!.atMs).toBe(RAIN_DROP_GAP_MS);
    }
    expect(new Set(drops.map((d) => d.ntfId))).toEqual(new Set(notifs.map((n) => n.id)));
    expect(rainDrops([])).toEqual([]);
  });

  it("gain 参数归一 0…0.4", () => {
    expect(rainGain(15)).toBe(0.15);
    expect(rainGain(999)).toBe(0.4);
    expect(rainGain(-5)).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// W-157 音量晨昏曲线
// ---------------------------------------------------------------------------

describe("W-157 音量晨昏曲线", () => {
  it("三段曲线边界（夜间地板 0.4 / 日间满格）", () => {
    expect(dayCurve(3)).toBe(0.4);
    expect(dayCurve(5)).toBe(0.4);
    expect(dayCurve(7)).toBeCloseTo(0.7, 5);
    expect(dayCurve(9)).toBe(1);
    expect(dayCurve(12)).toBe(1);
    expect(dayCurve(18)).toBe(1);
    expect(dayCurve(20.5)).toBeCloseTo(0.7, 5);
    expect(dayCurve(23)).toBe(0.4);
    expect(dayCurve(25)).toBe(0.4); // 越界回卷
    expect(dayCurve(-1)).toBe(0.4);
  });

  it("手动让位 60 分钟：窗口内 manual，之后 curve 接管", () => {
    const t = T0 + 10 * HOUR;
    const st = daypartManual({ base: 50, manualAt: 0 }, 80, t);
    expect(st).toEqual({ base: 80, manualAt: t });
    expect(daypartPct(st, t + 1000).source).toBe("manual");
    expect(daypartPct(st, t + 1000).pct).toBe(80);
    expect(daypartPct(st, t + DAYPART_MANUAL_YIELD_MS + 1).source).toBe("curve");
    // 让位期满后按曲线（时刻随本地时区动态核对）
    const hAfter = new Date(t + DAYPART_MANUAL_YIELD_MS + 1).getHours();
    expect(daypartPct(st, t + DAYPART_MANUAL_YIELD_MS + 1).pct).toBe(Math.round(80 * dayCurve(hAfter)));
    const late = daypartManual({ base: 100, manualAt: 0 }, 100, T0); // T0 本地钟点依时区
    const h = new Date(T0).getHours();
    expect(daypartPct(late, T0 + DAYPART_MANUAL_YIELD_MS + 1).pct).toBe(Math.round(100 * dayCurve(h)));
  });
});

// ---------------------------------------------------------------------------
// W-158 工作整点钟声
// ---------------------------------------------------------------------------

describe("W-158 工作整点钟声", () => {
  const H2 = 10 * HOUR; // 对齐整点（T0 = 02:00Z）
  it("会话外静默；会话始整点不报；跨整点恰响一声", () => {
    expect(chimeDue({ sessionStart: 0, sessionEnd: 0, lastChimeHour: 0 }, T0)).toBe(false);
    const st: { sessionStart: number; sessionEnd: number; lastChimeHour: number } = {
      sessionStart: H2 + 5 * 60_000,
      sessionEnd: 0,
      lastChimeHour: 0,
    };
    expect(chimeDue(st, H2 + 30 * 60_000)).toBe(false); // 同整点不报
    const h11 = H2 + HOUR + 1; // 11:00:00.001 跨整点
    expect(chimeDue(st, h11)).toBe(true);
    const marked = chimeMark(st, h11);
    expect(marked.lastChimeHour).toBe(H2 + HOUR);
    expect(chimeDue(marked, H2 + HOUR + 5000)).toBe(false); // 同整点防重
    expect(chimeDue(marked, H2 + 2 * HOUR + 1)).toBe(true);
  });

  it("会话结束即静默；进行中会话末尾跨整点仍可报", () => {
    const H2 = 10 * HOUR;
    const st = { sessionStart: H2 + 5 * 60_000, sessionEnd: H2 + 2 * HOUR + 30 * 60_000, lastChimeHour: 0 };
    expect(chimeDue(st, H2 + 2 * HOUR + 1)).toBe(true);
    expect(chimeDue(st, H2 + 2 * HOUR + 31 * 60_000)).toBe(false);
    expect(CHIME_MS).toBe(200);
  });

  it("hourFloor 对齐", () => {
    expect(hourFloor(10 * HOUR + 1234)).toBe(10 * HOUR);
    expect(hourFloor(9 * HOUR + 59 * 60_000 + 59_999)).toBe(9 * HOUR);
  });
});

// ---------------------------------------------------------------------------
// W-159 勿扰触觉回声
// ---------------------------------------------------------------------------

describe("W-159 勿扰触觉回声", () => {
  it("1 条 2 脉冲、封顶 4、90ms 步进", () => {
    expect(tremorPulses(0)).toBe(0);
    expect(tremorPulses(1)).toBe(2);
    expect(tremorPulses(3)).toBe(4);
    expect(tremorPulses(99)).toBe(TREMOR_MAX_PULSES);
    expect(tremorTimeline(3)).toEqual([0, TREMOR_PULSE_MS, 2 * TREMOR_PULSE_MS]);
    expect(tremorTimeline(0)).toEqual([]);
  });
});

// ---------------------------------------------------------------------------
// W-160 声纹钢琴
// ---------------------------------------------------------------------------

describe("W-160 声纹钢琴", () => {
  it("12 半音键位与展品同源", () => {
    const keys = pianoKeys();
    expect(keys).toHaveLength(12);
    expect(keys[0]!).toEqual({ key: "Key1", semitone: 0, soundId: SOUND_LIBRARY[0]!.soundId, note: "C" });
    expect(keys[11]!.note).toBe("B");
    expect(PIANO_NOTE_ZH).toHaveLength(12);
    expect(pianoSoundId(0)).toBe(SOUND_LIBRARY[0]!.soundId);
    expect(pianoSoundId(11)).toBe(SOUND_LIBRARY[11]!.soundId);
    expect(pianoSoundId(12)).toBeNull();
    expect(pianoSoundId(-1)).toBeNull();
    expect(pianoSoundId(3.6)).toBe(SOUND_LIBRARY[4]!.soundId); // 四舍五入
  });
});

// ---------------------------------------------------------------------------
// W-161 声纹历史
// ---------------------------------------------------------------------------

describe("W-161 声纹历史", () => {
  it("最近 3 声，新在前，FIFO 淘汰，去重幂等", () => {
    expect(HISTORY_CAP).toBe(3);
    let h = pushHistory([], { soundId: "a", at: 1 });
    h = pushHistory(h, { soundId: "b", at: 2 });
    h = pushHistory(h, { soundId: "c", at: 3 });
    expect(h.map((x) => x.soundId)).toEqual(["c", "b", "a"]);
    h = pushHistory(h, { soundId: "d", at: 4 });
    expect(h.map((x) => x.soundId)).toEqual(["d", "c", "b"]);
    const again = pushHistory(h, { soundId: "d", at: 4 });
    expect(again).toEqual(h);
    expect(historyLookup(h, 0)!.soundId).toBe("d");
    expect(historyLookup(h, 9)).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// W-162 通知队列聚光
// ---------------------------------------------------------------------------

describe("W-162 通知队列聚光", () => {
  const mk = (i: number): { ntfId: string; title: string; body: string; at: number } => ({
    ntfId: `n${i}`,
    title: `T${i}`,
    body: "b",
    at: T0 + i,
  });

  it("队列上限 5，超出转存档零丢失（总数守恒）", () => {
    let l: SpotlightLedger = { queue: [], archive: [] };
    for (let i = 0; i < 8; i++) l = spotlightEnqueue(l, mk(i));
    expect(l.queue).toHaveLength(SPOTLIGHT_CAP);
    expect(l.archive).toHaveLength(3);
    expect(spotlightTotal(l)).toBe(8);
    expect(l.queue.map((q) => q.ntfId)).toEqual(["n0", "n1", "n2", "n3", "n4"]);
    expect(l.archive.map((q) => q.ntfId)).toEqual(["n5", "n6", "n7"]);
  });

  it("幂等去重 + 逐条聚光", () => {
    let l: SpotlightLedger = { queue: [], archive: [] };
    l = spotlightEnqueue(l, mk(1));
    l = spotlightEnqueue(l, mk(1));
    expect(l.queue).toHaveLength(1);
    const step1 = spotlightDequeue(l);
    expect(step1.item?.ntfId).toBe("n1");
    expect(step1.ledger.queue).toHaveLength(0);
    const step2 = spotlightDequeue(step1.ledger);
    expect(step2.item).toBeNull();
    expect(step2.ledger).toBe(step1.ledger);
    expect(SPOTLIGHT_STAGE_MS).toBe(2400);
  });
});

// ---------------------------------------------------------------------------
// W-163 声纹和声分
// ---------------------------------------------------------------------------

describe("W-163 声纹和声分", () => {
  it("同频段依次让 60ms；跨频段零延迟；感知阈内", () => {
    const reqs = [
      { soundId: "sys-boot", band: "low" as const },
      { soundId: "sys-unlock", band: "mid" as const },
      { soundId: "sys-notify", band: "mid" as const },
      { soundId: "screenshot", band: "high" as const },
      { soundId: "mail-arrive", band: "high" as const },
      { soundId: "volume-step", band: "high" as const },
    ];
    const out = staggerAssign(reqs);
    expect(out.map((o) => o.offsetMs)).toEqual([0, 0, STAGGER_MS, 0, STAGGER_MS, 2 * STAGGER_MS]);
    expect(Math.max(...out.map((o) => o.offsetMs))).toBeLessThanOrEqual(2 * STAGGER_MS); // 无感
  });

  it("未知 soundId 回 mid 频段", () => {
    expect(bandOf("nope")).toBe("mid");
    expect(bandOf("sys-boot")).toBe("low");
  });
});

// ---------------------------------------------------------------------------
// 行为层（非 DOM 安全 + 激活幂等）
// ---------------------------------------------------------------------------

describe("行为层", () => {
  it("非 DOM 环境：activate/deactivate 安全 no-op", () => {
    expect(() => {
      activateSoundNova();
      activateSoundNova();
      deactivateSoundNova();
      deactivateSoundNova();
    }).not.toThrow();
    expect(isSoundNovaActive()).toBe(false);
  });

  it("tweakClamp 再导出一致（API 稳定性）", () => {
    expect(tweakClamp).toBe(clampTweak);
  });
});
