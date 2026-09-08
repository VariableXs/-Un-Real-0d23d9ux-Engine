/**
 * AI-18 氛围与个性化组 — 纯逻辑单测（schema coerce / rhythm / dayAround / curated /
 * briefing / sessionRestore / pureMode / volumeFade / moodEngine / contrastGuard / lunar）。
 */

import { describe, expect, it } from "vitest";
import { coerceAmbience, DEFAULT_AMBIENCE } from "../schema";
import { computeDueRhythms, isQuietHour, rhythmRings, completeRhythm } from "../rhythm";
import { slotOfHour, minutesToNextBoundary, slotConfigured } from "../dayAround";
import { CURATED_WALLPAPERS, dailyIndex, nextIndex, localDayNumber } from "../curated";
import { isoWeek, nextTip, shouldShowBriefing, TIPS } from "../briefing";
import { parseSnapshot, snapshotFresh, type AmbientSnapshot } from "../sessionRestore";
import { enterPureMode, exitPureMode, pureLayerVisibility, PURE_MODE_OFF } from "../pureMode";
import { fadeCurve, sceneTargetVolume, VOLUME_FADE_MS } from "../volumeFade";
import { computeMood, moodToOffsets, offsetsInEnvelope, easeMood, circadianArousal } from "../../../lib/moodEngine";
import { contrastRatio, guardAccent, recommendAccents, WCAG_AA } from "../../../lib/contrastGuard";
import { solarToLunar, solarTermOfDay, festivalOfDay, lunarSummary } from "../../../lib/lunar";

const amb = structuredClone(DEFAULT_AMBIENCE);

// ---------------- schema ----------------
describe("AI-18 schema coerce", () => {
  it("默认值 = 全部关闭或现状（氛围是减法）", () => {
    expect(DEFAULT_AMBIENCE.glow).toBe("off");
    expect(DEFAULT_AMBIENCE.mood.enabled).toBe(false);
    expect(DEFAULT_AMBIENCE.screensaver).toBe("off");
    expect(DEFAULT_AMBIENCE.briefing).toBe(false);
    expect(DEFAULT_AMBIENCE.sessionRestore).toBe(false);
    expect(DEFAULT_AMBIENCE.curated.on).toBe(false);
    expect(DEFAULT_AMBIENCE.wallpaperFilter).toEqual({ saturation: 100, brightness: 100 });
    expect(DEFAULT_AMBIENCE.uiDensity).toBe("comfort");
    expect(DEFAULT_AMBIENCE.uiRadius).toBe("round");
    expect(DEFAULT_AMBIENCE.hoverLatency).toBe("std");
    expect(DEFAULT_AMBIENCE.iconTier).toBe(20);
  });

  it("坏值一律回落默认", () => {
    expect(coerceAmbience(null)).toEqual(DEFAULT_AMBIENCE);
    expect(coerceAmbience("junk")).toEqual(DEFAULT_AMBIENCE);
    const c = coerceAmbience({ glow: "disco", iconTier: 99, wallpaperFilter: { saturation: 999, brightness: -5 } });
    expect(c.glow).toBe("off");
    expect(c.iconTier).toBe(20);
    expect(c.wallpaperFilter.saturation).toBe(100);
    expect(c.wallpaperFilter.brightness).toBe(80);
  });

  it("昼夜边界必须严格递增才接受", () => {
    const ok = coerceAmbience({ dayAround: { boundaries: [7, 12, 18, 22] } });
    expect(ok.dayAround.boundaries).toEqual([7, 12, 18, 22]);
    const bad = coerceAmbience({ dayAround: { boundaries: [18, 12, 6, 22] } });
    expect(bad.dayAround.boundaries).toEqual([6, 12, 18, 21]);
  });
});

// ---------------- U-54 rhythm ----------------
describe("U-54 rhythm", () => {
  it("深夜时段判定（23..6 点）", () => {
    expect(isQuietHour(23)).toBe(true);
    expect(isQuietHour(2)).toBe(true);
    expect(isQuietHour(5)).toBe(true);
    expect(isQuietHour(6)).toBe(false);
    expect(isQuietHour(22)).toBe(false);
  });

  it("到期产生提醒并计数；焦点舱内顺延", () => {
    const s = structuredClone(amb);
    s.rhythm.drink = { enabled: true, intervalMin: 60, notify: "notify" };
    const t0 = 1000;
    const { due, next } = computeDueRhythms(s, t0 + 61 * 60_000, 14, false, { drink: { startMono: t0, lastNotified: 0, count: 0 } });
    expect(due).toEqual([{ kind: "drink", notify: "notify" }]);
    expect(next.drink?.count).toBe(1);
    // 焦点舱：全部顺延
    const inCabin = computeDueRhythms(s, t0 + 61 * 60_000, 14, true, { drink: { startMono: t0, lastNotified: 0, count: 0 } });
    expect(inCabin.due).toEqual([]);
  });

  it("深夜降级为 count；去重窗口内不重复", () => {
    const s = structuredClone(amb);
    s.rhythm.stretch = { enabled: true, intervalMin: 90, notify: "notify" };
    const t0 = 1000;
    const lateNight = computeDueRhythms(s, t0 + 91 * 60_000, 23, false, { stretch: { startMono: t0, lastNotified: 0, count: 0 } });
    expect(lateNight.due[0]?.notify).toBe("count");
    const again = computeDueRhythms(s, t0 + 91 * 60_000 + 60_000, 23, false, lateNight.next);
    expect(again.due).toEqual([]);
  });

  it("四环进度 0..1 + 手动完成重置计时", () => {
    const s = structuredClone(amb);
    s.rhythm.eye = { enabled: true, intervalMin: 20, notify: "badge" };
    const t0 = 1000;
    const rings = rhythmRings(s, t0 + 10 * 60_000, { eye: { startMono: t0, lastNotified: 0, count: 0 } });
    expect(rings.eye).toBeCloseTo(0.5, 5);
    const done = completeRhythm("eye", t0 + 10 * 60_000, { eye: { startMono: t0, lastNotified: 0, count: 0 } });
    expect(done.eye?.count).toBe(1);
  });
});

// ---------------- M-65 dayAround ----------------
describe("M-65 dayAround", () => {
  const b: [number, number, number, number] = [6, 12, 18, 21];
  it("时段判定（夜跨午夜环绕）", () => {
    expect(slotOfHour(7, b)).toBe("morning");
    expect(slotOfHour(15, b)).toBe("day");
    expect(slotOfHour(19, b)).toBe("dusk");
    expect(slotOfHour(22, b)).toBe("night");
    expect(slotOfHour(3, b)).toBe("night");
  });
  it("距下一边界分钟数", () => {
    expect(minutesToNextBoundary(5, 0, b)).toBe(60);
    expect(minutesToNextBoundary(22, 0, b)).toBe(8 * 60); // 到明早 6 点
  });
  it("未配置时段跳过", () => {
    const dirs = { morning: "", day: "D:/wp", dusk: "", night: "" };
    expect(slotConfigured(dirs, "day")).toBe(true);
    expect(slotConfigured(dirs, "morning")).toBe(false);
  });
});

// ---------------- V-71 curated ----------------
describe("V-71 curated", () => {
  it("10 张全部是 SVG data-URI（零网络零二进制）", () => {
    expect(CURATED_WALLPAPERS).toHaveLength(10);
    for (const w of CURATED_WALLPAPERS) {
      expect(w.svg.startsWith("data:image/svg+xml")).toBe(true);
      expect(w.zh.length).toBeGreaterThan(0);
      expect(w.en.length).toBeGreaterThan(0);
    }
  });
  it("每日轮换索引环回", () => {
    expect(dailyIndex(new Date(2026, 0, 1), 10)).toBe(localDayNumber(new Date(2026, 0, 1)) % 10);
    expect(nextIndex(9, 10)).toBe(0);
  });
});

// ---------------- M-69 briefing ----------------
describe("M-69 briefing", () => {
  it("每日首启判定", () => {
    const d = new Date(2026, 8, 8);
    expect(shouldShowBriefing("2026-09-07", d)).toBe(true);
    expect(shouldShowBriefing("2026-09-08", d)).toBe(false);
  });
  it("贴士池轮换，池尽重置", () => {
    const allShown = TIPS.map((t) => t.id);
    const [tip, next] = nextTip(allShown);
    expect(tip.id).toBe(1);
    expect(next).toEqual([1]);
    const [first] = nextTip([]);
    expect(first.id).toBe(1);
  });
  it("ISO 周数", () => {
    expect(isoWeek(new Date(2026, 0, 1))).toBe(1);
  });
});

// ---------------- M-72 sessionRestore ----------------
describe("M-72 sessionRestore", () => {
  const snap: AmbientSnapshot = {
    format: "ai18-session", version: 1, savedAt: Date.now(),
    wallpaper: "D:/wp/a.jpg", volume: 0.6, dnd: true,
    windows: [{ appId: "write", rect: { x: 1, y: 2, w: 3, h: 4 }, route: null }],
  };
  it("快照校验（坏值拒绝）", () => {
    expect(parseSnapshot(snap)).toEqual(snap);
    expect(parseSnapshot({ ...snap, format: "other" })).toBeNull();
    expect(parseSnapshot(null)).toBeNull();
  });
  it("7 天新鲜度", () => {
    expect(snapshotFresh(snap)).toBe(true);
    expect(snapshotFresh({ ...snap, savedAt: Date.now() - 8 * 86_400_000 })).toBe(false);
  });
});

// ---------------- M-67 pureMode ----------------
describe("M-67 pureMode", () => {
  it("进入记住原状，退出精确还原", () => {
    const st = enterPureMode({ icons: true, taskbar: false, banners: true });
    expect(pureLayerVisibility(st)).toEqual({ icons: false, taskbar: false, banners: false });
    expect(exitPureMode(st)).toEqual({ icons: true, taskbar: false, banners: true });
    expect(pureLayerVisibility(PURE_MODE_OFF).icons).toBe(true);
  });
});

// ---------------- M-66 volumeFade ----------------
describe("M-66 volumeFade", () => {
  it("30ms 常数 + 线性曲线", () => {
    expect(VOLUME_FADE_MS).toBe(30);
    expect(fadeCurve(0, 1, 3)).toEqual([0, 0.5, 1]);
  });
  it("音量表折算（*0.5 上限防炸）", () => {
    expect(sceneTargetVolume("rain", { ...amb, soundscapeVolumes: { ...amb.soundscapeVolumes, rain: 1 } })).toBe(0.5);
  });
});

// ---------------- N-33 moodEngine ----------------
describe("N-33 moodEngine", () => {
  it("时辰节律分段", () => {
    expect(circadianArousal(0)).toBeCloseTo(-0.8, 5);
    expect(circadianArousal(10)).toBeCloseTo(0.7, 5);
  });
  it("情绪向量在值域内", () => {
    const m = computeMood({ hour: 14, scene: "gaming", app: "game", manualArousal: 1, manualFocus: 1 });
    expect(m.arousal).toBeLessThanOrEqual(1);
    expect(m.focus).toBeLessThanOrEqual(1);
  });
  it("偏移在包络内；HC 禁用色温", () => {
    const off = moodToOffsets({ arousal: 1, focus: 0 }, false);
    expect(offsetsInEnvelope(off)).toBe(true);
    expect(off.hueShift).not.toBe(0);
    const hc = moodToOffsets({ arousal: 1, focus: 0 }, true);
    expect(hc.hueShift).toBe(0);
    expect(offsetsInEnvelope(hc)).toBe(true);
  });
  it("120s 缓动内插", () => {
    const a = { brightness: 0, saturation: 0, hueShift: 0, motionScale: 1, notifyPitch: 0 };
    const b = { brightness: 0.1, saturation: 0, hueShift: 0, motionScale: 1, notifyPitch: 0 };
    const mid = easeMood(a, b, 60);
    expect(mid.brightness).toBeCloseTo(0.05, 5);
  });
});

// ---------------- V-80 contrastGuard ----------------
describe("V-80 contrastGuard", () => {
  it("WCAG 对比度已知值", () => {
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 0);
    expect(contrastRatio("#000000", "#000000")).toBeCloseTo(1, 5);
    expect(contrastRatio("junk", "#fff")).toBeNull();
  });
  it("守护判定 + 推荐达标色", () => {
    const bad = guardAccent("#777777", "#f2f2f2", "#1a1a22");
    expect(bad.pass).toBe(false);
    const recs = recommendAccents("#777777", "#f2f2f2", "#1a1a22", 3);
    expect(recs.length).toBeGreaterThan(0);
    for (const r of recs) {
      expect(guardAccent(r, "#f2f2f2", "#1a1a22").pass).toBe(true);
    }
  });
  it("AA 阈值 4.5", () => {
    expect(WCAG_AA).toBe(4.5);
  });
});

// ---------------- V-73 lunar ----------------
describe("V-73 lunar", () => {
  it("2025-01-29 = 春节（乙巳年正月初一）", () => {
    const l = solarToLunar(new Date(2025, 0, 29));
    expect(l).not.toBeNull();
    expect(l?.month).toBe(1);
    expect(l?.day).toBe(1);
    expect(l?.leap).toBe(false);
  });
  it("节气：2026 年立春在 2 月 4 日附近", () => {
    const hit = ["2026-02-03", "2026-02-04", "2026-02-05"].some((ds) => {
      const [y, m, d] = ds.split("-").map(Number) as [number, number, number];
      return solarTermOfDay(new Date(y, m - 1, d)) === "立春";
    });
    expect(hit).toBe(true);
  });
  it("节日：端午 = 五月初五", () => {
    // 2025 端午 = 2025-05-31
    expect(festivalOfDay(new Date(2025, 4, 31))).toContain("端午");
  });
  it("表外日期返回 null（诚实边界）", () => {
    expect(solarToLunar(new Date(2018, 0, 1))).toBeNull();
    expect(solarToLunar(new Date(2050, 0, 1))).toBeNull();
  });
  it("lunarSummary 汇总", () => {
    const s = lunarSummary(new Date(2025, 0, 29));
    expect(s.text).toContain("正月");
    expect(s.festival).toContain("春节");
  });
});
