/**
 * NOVA-200 S1 · 启动剧场路纯逻辑单测（AI-01 路自证）。
 * 口径（实施总步骤 §S1）：温度制式判定、预报模型（20 次均值+磁盘因子）、
 * 情绪测温（lastExit→情绪档）、源流判定链、里程碑时间线、声场景规则等纯函数全覆盖。
 */

import { describe, expect, it } from "vitest";
import {
  BOOT_NOVA_FEATURES,
  FAST_WINDOW_MS,
  FORECAST_MAX_SAMPLES,
  FORECAST_MIN_SAMPLES,
  FOOTPRINT_MAX,
  MILESTONE_FREQS,
  bootNovaDomain,
  bootSoundPlan,
  cascadeDelays,
  constellationPoints,
  forecastBootMs,
  forecastErrorPct,
  forecastLine,
  fmtAge,
  hueFromDuration,
  ignitionRegime,
  judgeBootSource,
  milestoneTimeline,
  moodFromEvidence,
  nextBootPreviewLines,
  parseRiteStyle,
  pickConstellationFeatures,
  pushFootprint,
  riteToneSeq,
  type BootEvidence,
} from "../bootNova";

const NOW = 1_800_000_000_000;

function evidence(patch: Partial<BootEvidence>): BootEvidence {
  return { aliveTs: null, exitTs: null, navReload: false, now: NOW, ...patch };
}

describe("bootNova/W-012 源流判定链", () => {
  it("webview 重载 → RESUME（最高优先）", () => {
    const r = judgeBootSource(evidence({ navReload: true, exitTs: NOW - 1000 }));
    expect(r.source).toBe("RESUME");
    expect(r.chain.join(" ")).toContain("NAV RELOAD YES");
  });

  it("无任何历史 → COLD（首次运行）", () => {
    const r = judgeBootSource(evidence({}));
    expect(r.source).toBe("COLD");
    expect(r.chain).toContain("HISTORY NONE");
  });

  it("干净退出新鲜 → FAST", () => {
    const r = judgeBootSource(evidence({ exitTs: NOW - 60_000, aliveTs: NOW - 61_000 }));
    expect(r.source).toBe("FAST");
    expect(r.chain.join(" ")).toContain("CLEAN EXIT FRESH");
  });

  it("有存活心跳但无干净退出 → RECOVER（崩溃/断电）", () => {
    const r = judgeBootSource(evidence({ aliveTs: NOW - 3_600_000 }));
    expect(r.source).toBe("RECOVER");
    expect(r.chain).toContain("CLEAN EXIT NONE");
  });

  it("退出标记陈旧 → COLD", () => {
    const r = judgeBootSource(evidence({ exitTs: NOW - FAST_WINDOW_MS - 1 }));
    expect(r.source).toBe("COLD");
    expect(r.chain.join(" ")).toContain("CLEAN EXIT STALE");
  });

  it("窗口边界：恰在 10min 内算 FAST", () => {
    expect(judgeBootSource(evidence({ exitTs: NOW - FAST_WINDOW_MS })).source).toBe("FAST");
  });

  it("fmtAge 分档", () => {
    expect(fmtAge(30_000)).toBe("30S AGO");
    expect(fmtAge(5 * 60_000)).toBe("5M AGO");
    expect(fmtAge(4 * 3_600_000)).toBe("4H AGO");
    expect(fmtAge(3 * 86_400_000)).toBe("3D AGO");
  });
});

describe("bootNova/W-001 温度制式", () => {
  it("冷启熔炉 / 快启余烬 / 会话恢复无点火 / RECOVER 属真实冷启", () => {
    expect(ignitionRegime("COLD")).toBe("furnace");
    expect(ignitionRegime("FAST")).toBe("ember");
    expect(ignitionRegime("RESUME")).toBe("none");
    expect(ignitionRegime("RECOVER")).toBe("furnace");
  });
});

describe("bootNova/W-011 情绪测温", () => {
  it("非 RECOVER 一律标准版", () => {
    expect(moodFromEvidence("COLD", true)).toBe("standard");
    expect(moodFromEvidence("FAST", null)).toBe("standard");
    expect(moodFromEvidence("RESUME", false)).toBe("standard");
  });

  it("RECOVER：dump 证实崩溃 → 安抚版；断电/探针不可用 → 检视版", () => {
    expect(moodFromEvidence("RECOVER", true)).toBe("soothing");
    expect(moodFromEvidence("RECOVER", false)).toBe("inspect");
    expect(moodFromEvidence("RECOVER", null)).toBe("inspect");
  });
});

describe("bootNova/W-002 预报模型", () => {
  const base = [6000, 6100, 5900, 6000, 6200]; // 5 样本，均值 6040

  it("样本 <5 → LEARNING 不编造数字", () => {
    const r = forecastBootMs(base.slice(0, 4), null, 50);
    expect(r.learning).toBe(true);
    expect(r.ms).toBeNull();
  });

  it("≥5 样本 → 最近均值（无磁盘探针不加项）", () => {
    const r = forecastBootMs(base, null, 50);
    expect(r.learning).toBe(false);
    expect(r.ms).toBe(6040);
    expect(r.diskTerm).toBe(false);
  });

  it("磁盘因子：繁忙上浮 / 空闲下沉 / 因子 0 无影响", () => {
    const busy = forecastBootMs(base, 1, 100);
    const idle = forecastBootMs(base, 0, 100);
    const zero = forecastBootMs(base, 1, 0);
    expect(busy.ms!).toBeGreaterThan(6040);
    expect(idle.ms!).toBeLessThan(6040);
    expect(zero.ms!).toBe(6040);
    // 满因子满繁忙 = +35%
    expect(busy.ms!).toBe(Math.round(6040 * 1.35));
  });

  it("环形上限 20 样本", () => {
    expect(FORECAST_MAX_SAMPLES).toBe(20);
    expect(FORECAST_MIN_SAMPLES).toBe(5);
  });

  it("误差回写：正负与非法输入", () => {
    expect(forecastErrorPct(6000, 6300)).toBe(5);
    expect(forecastErrorPct(6000, 5700)).toBe(-5);
    expect(forecastErrorPct(null, 6000)).toBeNull();
    expect(forecastErrorPct(0, 6000)).toBeNull();
  });

  it("对账行文案（HUD 英文纪律）", () => {
    expect(forecastLine(6000, 6300, 12)).toBe("EXPECTED 6.0S · ACTUAL 6.3S · ERR +5%");
    expect(forecastLine(6000, 5700, 12)).toBe("EXPECTED 6.0S · ACTUAL 5.7S · ERR -5%");
    expect(forecastLine(null, 6100, 2)).toBe("EXPECTED LEARNING (2/5) · ACTUAL 6.1S");
    expect(forecastLine(6000, 6000, 3)).toContain("LEARNING");
  });
});

describe("bootNova/W-009 里程碑时间线", () => {
  it("从真实事件流提取四档首跨时刻", () => {
    const hits = milestoneTimeline([
      { progress: 0.1, elapsedMs: 800 },
      { progress: 0.26, elapsedMs: 1600 },
      { progress: 0.5, elapsedMs: 3000 },
      { progress: 0.76, elapsedMs: 4500 },
      { progress: 1, elapsedMs: 6000 },
    ]);
    expect(hits.map((h) => h.at)).toEqual([0.25, 0.5, 0.75, 1]);
    expect(hits.map((h) => h.elapsedMs)).toEqual([1600, 3000, 4500, 6000]);
  });

  it("未达满程 → 如实少记里程碑", () => {
    const hits = milestoneTimeline([
      { progress: 0.3, elapsedMs: 1000 },
      { progress: 0.6, elapsedMs: 2000 },
    ]);
    expect(hits.map((h) => h.at)).toEqual([0.25, 0.5]);
  });

  it("四音上行频率表", () => {
    expect(MILESTONE_FREQS).toHaveLength(4);
    expect([...MILESTONE_FREQS]).toEqual([...MILESTONE_FREQS].sort((a, b) => a - b));
  });
});

describe("bootNova/W-004 声场景规则", () => {
  it("本会话用户显式调大音量 → 不再干预", () => {
    const p = bootSoundPlan({ lastVolumePct: 5, hour: 1, nightSilent: true, userRaised: true });
    expect(p).toEqual({ gainScale: 1, haptic: false, reason: "USER OVERRIDE" });
  });

  it("深夜 + nightSilent → 静默 + 微振替身", () => {
    for (const hour of [23, 2, 5]) {
      const p = bootSoundPlan({ lastVolumePct: 60, hour, nightSilent: true, userRaised: false });
      expect(p.gainScale).toBe(0);
      expect(p.haptic).toBe(true);
      expect(p.reason).toBe("NIGHT SILENCE");
    }
  });

  it("深夜但未开 nightSilent → 不静默", () => {
    const p = bootSoundPlan({ lastVolumePct: 60, hour: 23, nightSilent: false, userRaised: false });
    expect(p.gainScale).toBe(1);
  });

  it("静音环境（≤10%）→ 30% 轻响", () => {
    const p = bootSoundPlan({ lastVolumePct: 8, hour: 14, nightSilent: true, userRaised: false });
    expect(p).toEqual({ gainScale: 0.3, haptic: false, reason: "QUIET ENV" });
  });

  it("muted 视为 0 → 走静音环境轻响；标准态全量", () => {
    expect(bootSoundPlan({ lastVolumePct: 0, hour: 14, nightSilent: true, userRaised: false }).gainScale).toBe(0.3);
    expect(bootSoundPlan({ lastVolumePct: 50, hour: 14, nightSilent: true, userRaised: false }).reason).toBe("STANDARD");
  });
});

describe("bootNova/W-005 多屏接力", () => {
  it("延迟序列与 stagger 参数", () => {
    expect(cascadeDelays(3, 150)).toEqual([0, 150, 300]);
  });

  it("单屏零行为（[0] 由调用方裁断）与 stagger 钳制", () => {
    expect(cascadeDelays(1, 150)).toEqual([0]);
    expect(cascadeDelays(4, 999)).toEqual([0, 400, 800, 1200]);
    expect(cascadeDelays(0, 150)).toEqual([]);
  });
});

describe("bootNova/W-006 足迹墙", () => {
  it("环形追加：最新在前、定长截断", () => {
    let list = pushFootprint([], { ts: 1, ms: 5000 });
    for (let i = 2; i <= 25; i++) list = pushFootprint(list, { ts: i, ms: 5000 + i });
    expect(list).toHaveLength(FOOTPRINT_MAX);
    expect(list[0]!.ts).toBe(25);
    expect(list[FOOTPRINT_MAX - 1]!.ts).toBe(6);
  });

  it("色温映射：快=冷白 250、慢=暖红 30、单调", () => {
    expect(hueFromDuration(5000, 5000, 15000)).toBe(250);
    expect(hueFromDuration(15000, 5000, 15000)).toBe(30);
    expect(hueFromDuration(10000, 5000, 15000)).toBe(140);
    expect(hueFromDuration(8000, 8000, 8000)).toBe(250); // 单样本冷白
  });
});

describe("bootNova/W-007 下次启动预览", () => {
  it("无自启登记 → CLEAN BOOT 如实", () => {
    const lines = nextBootPreviewLines([], 6200, 5900);
    expect(lines[0]).toBe("AUTOSTART · CLEAN BOOT");
    expect(lines).toContain("EXPECTED 6.2S");
    expect(lines).toContain("LAST 5.9S");
  });

  it("有登记列清单；无预报/无实测如实标注", () => {
    const lines = nextBootPreviewLines(["Writer", "Code"], null, null);
    expect(lines[0]).toBe("AUTOSTART · 2 APPS");
    expect(lines).toContain("· Writer");
    expect(lines).toContain("EXPECTED LEARNING");
    expect(lines).toContain("LAST —");
  });

  it("清单超 4 项折叠", () => {
    const lines = nextBootPreviewLines(["a", "b", "c", "d", "e", "f"], 6000, 6000);
    expect(lines).toContain("… +2");
  });
});

describe("bootNova/W-008 品牌星座", () => {
  it("星点布局确定性且在字标区带内", () => {
    const pts = constellationPoints(8);
    expect(pts).toHaveLength(8);
    for (const p of pts) {
      expect(p.xPct).toBeGreaterThanOrEqual(14);
      expect(p.xPct).toBeLessThanOrEqual(86);
      expect(p.yPct).toBeGreaterThan(20);
      expect(p.yPct).toBeLessThan(60);
    }
    expect(constellationPoints(8)).toEqual(constellationPoints(8));
  });

  it("星点数据源：等距采样、全域覆盖、逐项带 changelog 真源", () => {
    const feats = pickConstellationFeatures(8);
    expect(feats).toHaveLength(8);
    expect(new Set(feats.map((f) => f.id)).size).toBe(8);
    for (const f of feats) expect(f.changelog.length).toBeGreaterThan(4);
    expect(feats[0]!.id).toBe("W-001"); // 首星=域1首个能力
  });
});

describe("bootNova/W-010 电源仪式音", () => {
  it("三制式：弦乐三音下行 / 钟摆双脉冲 / 静默空序列", () => {
    const strings = riteToneSeq("strings");
    expect(strings).toHaveLength(3);
    expect(strings[0]!.freq).toBeGreaterThan(strings[2]!.freq);
    expect(strings.reduce((mx, t) => Math.max(mx, t.delayMs + t.durMs), 0)).toBeLessThanOrEqual(1200);
    expect(riteToneSeq("pendulum")).toHaveLength(2);
    expect(riteToneSeq("silence")).toEqual([]);
  });

  it("制式解析：非法值回退 strings", () => {
    expect(parseRiteStyle("pendulum")).toBe("pendulum");
    expect(parseRiteStyle("silence")).toBe("silence");
    expect(parseRiteStyle("jazz")).toBe("strings");
  });
});

describe("bootNova Hub 注册清单", () => {
  it("12 项功能卡齐备且与 W-001..W-012 对齐", () => {
    expect(BOOT_NOVA_FEATURES).toHaveLength(12);
    expect(BOOT_NOVA_FEATURES.map((f) => f.id)).toEqual(
      Array.from({ length: 12 }, (_, i) => `W-${String(i + 1).padStart(3, "0")}`),
    );
    for (const f of BOOT_NOVA_FEATURES) {
      expect(f.degrade.length).toBeGreaterThan(4);
      expect(f.descZh.length).toBeGreaterThan(8);
    }
  });

  it("域清单元信息完整", () => {
    expect(bootNovaDomain.id).toBe("S1");
    expect(bootNovaDomain.route).toBe("AI-01");
    expect(bootNovaDomain.features).toBe(BOOT_NOVA_FEATURES);
  });
});
