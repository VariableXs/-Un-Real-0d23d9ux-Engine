import { describe, expect, it } from "vitest";
import { brandMoment, daySegment, greetable, isWake, paradeDue } from "../domains/bootDomain";
import { layoutPositions, tiltFromVel, vitalsOf, type WinRect } from "../domains/windowsDomain";
import { hoverTilt, iconCategory, rippleDelay, zoomStageClamp } from "../domains/desktopDomain";
import { flowMatch, thermoLevel, thermalHue, vaneIntensity as tbVane } from "../domains/taskbarDomain";
import {
  DEFAULT_FLOWS,
  chordWindowMs,
  inertiaGain,
  looksSecret,
  spaceFlowSpeed,
} from "../domains/inputDomain";
import { nameSuggestions, timeLensRange, weightTier } from "../domains/filesDomain";
import { dequeue, pomodoroPhase, selectionQualified } from "../domains/toolsDomain";
import {
  auroraIntensity,
  breathProfile,
  humidityState,
  tempHue,
  tempTrend,
  vaneIntensity as hwVane,
} from "../domains/hardwareDomain";
import { batteryTierCrossed, escortLabel, inBufferWindow } from "../domains/compatDomain";
import { looksSensitive, permVerdict, pruneHistory } from "../domains/privacyDomain";
import { nutritionRows, pluginGrade } from "../domains/ecoDomain";
import { SEASON_HUE, materialVars, parallaxShift, seasonOf } from "../domains/visualsDomain";
import { digestDue, isFlood, shouldMergeLate } from "../domains/soundDomain";
import { cvdFilter, editDistance, fuzzyHit, textScalePercent } from "../domains/a11yDomain";
import { ecgDrop, flameLayout, quotaWarn } from "../domains/qualityDomain";
import type { SinguPulseLike } from "../shared";

const DAY = 86_400_000;
const MB = 1024 * 1024;

function pulse(patch: Partial<SinguPulseLike>): SinguPulseLike {
  return {
    cpu_usage: 0,
    mem_used_mb: 0,
    mem_total_mb: 0,
    gpu_usage: null,
    cpu_temp_c: null,
    temp_estimated: false,
    fan_high: false,
    battery_percent: null,
    battery_ac: false,
    disks: [],
    net_up_bps: 0,
    net_down_bps: 0,
    ts_ms: 0,
    ...patch,
  };
}

// ---------------------------------------------------------------------------
// 域1 启动与品牌剧场
// ---------------------------------------------------------------------------
describe("singularity/bootDomain 纯逻辑", () => {
  it("daySegment：0-4 五档时段边界", () => {
    expect(daySegment(0)).toBe(4);
    expect(daySegment(5)).toBe(4);
    expect(daySegment(6)).toBe(0);
    expect(daySegment(10)).toBe(0);
    expect(daySegment(11)).toBe(1);
    expect(daySegment(13)).toBe(1);
    expect(daySegment(14)).toBe(2);
    expect(daySegment(17)).toBe(2);
    expect(daySegment(18)).toBe(3);
    expect(daySegment(22)).toBe(3);
    expect(daySegment(23)).toBe(4);
  });

  it("greetable：每会话一次 + 就绪延迟 1.5s", () => {
    expect(greetable(null, 2000, 0)).toBe(true);
    expect(greetable(null, 1500, 0)).toBe(false); // 未过就绪延迟
    expect(greetable(500, 99999, 0)).toBe(false); // 本会话已问候
    expect(greetable(500, 1600, 1000)).toBe(false); // 上一会话问候过但未到延迟
    expect(greetable(500, 2600, 1000)).toBe(true); // 上一会话问候过 + 已过延迟 → 再问候
  });

  it("brandMoment：元旦 / 跨年 23 点后 / 项目诞辰 / 平常 null", () => {
    expect(brandMoment(new Date(2026, 0, 1, 10))).toBe("newyear");
    expect(brandMoment(new Date(2026, 11, 31, 23, 30))).toBe("newyear");
    expect(brandMoment(new Date(2026, 11, 31, 22, 0))).toBeNull();
    expect(brandMoment(new Date(2026, 8, 9, 12))).toBe("birthday");
    expect(brandMoment(new Date(2026, 4, 15))).toBeNull();
  });

  it("isWake：距失焦超阈值分钟才算唤醒", () => {
    expect(isWake(null, Date.now(), 5)).toBe(false);
    expect(isWake(0, 5 * 60_000, 5)).toBe(true); // 恰好达标
    expect(isWake(0, 5 * 60_000 - 1, 5)).toBe(false);
  });

  it("paradeDue：仅升级首启触发巡礼", () => {
    expect(paradeDue(null, "1.5.0")).toBe(false); // 全新安装不打扰
    expect(paradeDue("1.5.0", "1.5.0")).toBe(false);
    expect(paradeDue("1.4.9", "1.5.0")).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// 域2 窗口与空间管理
// ---------------------------------------------------------------------------
describe("singularity/windowsDomain 纯逻辑", () => {
  const bounds: WinRect = { x: 0, y: 0, w: 300, h: 200 };

  it("layoutPositions：n≤0 返回空", () => {
    expect(layoutPositions(0, bounds, "grid")).toEqual([]);
    expect(layoutPositions(-1, bounds, "columns")).toEqual([]);
  });

  it("layoutPositions grid：5 窗 → 3×2 网格全覆盖", () => {
    const out = layoutPositions(5, bounds, "grid");
    expect(out).toHaveLength(5);
    expect(out[0]).toEqual({ x: 0, y: 0, w: 100, h: 100 });
    expect(out[1]).toEqual({ x: 100, y: 0, w: 100, h: 100 });
    expect(out[2]).toEqual({ x: 200, y: 0, w: 100, h: 100 });
    expect(out[3]).toEqual({ x: 0, y: 100, w: 100, h: 100 });
    expect(out[4]).toEqual({ x: 100, y: 100, w: 100, h: 100 });
    const area = out.reduce((s, r) => s + r.w * r.h, 0);
    expect(area).toBeCloseTo(5 * 100 * 100, 5); // 5 窗占 5 格（第 6 格留空）
  });

  it("layoutPositions columns：3 窗等宽满列", () => {
    const out = layoutPositions(3, bounds, "columns");
    expect(out.map((r) => r.w)).toEqual([100, 100, 100]);
    expect(out[2]!.x).toBe(200);
    expect(out.every((r) => r.h === 200 && r.y === 0)).toBe(true);
  });

  it("layoutPositions quadrants：4 窗占满四象限", () => {
    const out = layoutPositions(4, bounds, "quadrants");
    expect(out.map((r) => `${r.x},${r.y}`)).toEqual(["0,0", "150,0", "0,100", "150,100"]);
    expect(out.every((r) => r.w === 150 && r.h === 100)).toBe(true);
  });

  it("layoutPositions spiral：黄金螺旋精确铺满无重叠", () => {
    const out = layoutPositions(3, { x: 0, y: 0, w: 100, h: 100 }, "spiral");
    expect(out[0]!.w).toBeCloseTo(61.8, 5);
    expect(out[0]!.h).toBe(100);
    const area = out.reduce((s, r) => s + r.w * r.h, 0);
    expect(area).toBeCloseTo(10_000, 4); // 分割无浪费
    expect(out[2]!.w).toBeCloseTo(38.2, 3); // 末窗取剩余全部
  });

  it("tiltFromVel：速度→倾角（符号 / 限幅 / 零时长保护）", () => {
    expect(tiltFromVel(100, 100, 3)).toBeCloseTo(1.6, 5);
    expect(tiltFromVel(-100, 100, 3)).toBeCloseTo(-1.6, 5);
    expect(tiltFromVel(1000, 100, 3)).toBe(3); // 限幅
    expect(tiltFromVel(-1000, 100, 3)).toBe(-3);
    expect(tiltFromVel(100, 0, 3)).toBe(0); // dt≤0 防除零
  });

  it("vitalsOf：失焦静止 / 负载越高转越快越亮", () => {
    expect(vitalsOf(80, false)).toEqual({ spinSec: 0, opacity: 0.22 });
    expect(vitalsOf(80, true).spinSec).toBe(1.1);
    expect(vitalsOf(30, true).spinSec).toBe(2.2);
    expect(vitalsOf(10, true).spinSec).toBe(4);
    expect(vitalsOf(10, true).opacity).toBeCloseTo(0.55, 5);
    expect(vitalsOf(80, true).opacity).toBeCloseTo(0.95, 5);
  });
});

// ---------------------------------------------------------------------------
// 域3 桌面与图标表达
// ---------------------------------------------------------------------------
describe("singularity/desktopDomain 纯逻辑", () => {
  it("hoverTilt：朝向鼠标来向 ±3°", () => {
    expect(hoverTilt(100, true)).toBe(3);
    expect(hoverTilt(100, false)).toBe(-3);
  });

  it("zoomStageClamp：0.6..1.6 钳制", () => {
    expect(zoomStageClamp(0.5)).toBe(0.6);
    expect(zoomStageClamp(1)).toBe(1);
    expect(zoomStageClamp(2)).toBe(1.6);
  });

  it("rippleDelay：按距离每 30ms 一波取整（半向上）", () => {
    expect(rippleDelay(0, 0)).toBe(0);
    expect(rippleDelay(59, 0)).toBe(0);
    expect(rippleDelay(60, 0)).toBe(30); // 0.5 波 → Math.round 半向上
    expect(rippleDelay(120, 0)).toBe(30);
    expect(rippleDelay(240, 0)).toBe(60);
    expect(rippleDelay(0, 300)).toBe(90); // 2.5 波 → 3 波
  });

  it("iconCategory：扩展名八分类", () => {
    expect(iconCategory("Variable.exe")).toBe("app");
    expect(iconCategory("photo.JPG")).toBe("image");
    expect(iconCategory("clip.mkv")).toBe("video");
    expect(iconCategory("song.flac")).toBe("audio");
    expect(iconCategory("pack.7z")).toBe("archive");
    expect(iconCategory("doc.xlsx")).toBe("doc");
    expect(iconCategory("main.rs")).toBe("code");
    expect(iconCategory("weird.xyz")).toBe("other");
    expect(iconCategory("noext")).toBe("other");
  });
});

// ---------------------------------------------------------------------------
// 域4 任务栏与开始菜单
// ---------------------------------------------------------------------------
describe("singularity/taskbarDomain 纯逻辑", () => {
  it("thermoLevel：CPU 三档温度（>75 红 / ≥40 黄 / 绿）", () => {
    expect(thermoLevel(0)).toBe(0);
    expect(thermoLevel(39)).toBe(0);
    expect(thermoLevel(40)).toBe(1);
    expect(thermoLevel(75)).toBe(1);
    expect(thermoLevel(76)).toBe(2);
    expect(thermoLevel(100)).toBe(2);
  });

  it("flowMatch：子串 / 拼音首字母 / 大小写不敏感", () => {
    expect(flowMatch("", "计算器", "jsq")).toBe(false);
    expect(flowMatch("算器", "计算器", "jsq")).toBe(true); // 子串
    expect(flowMatch("jsq", "计算器", "jsq")).toBe(true); // 首字母前缀
    expect(flowMatch("JSQ", "计算器", "jsq")).toBe(true); // 大小写不敏感
    expect(flowMatch("jsq", "计算器", "")).toBe(false); // 无首字母表则不命中
    expect(flowMatch("xyz", "计算器", "jsq")).toBe(false);
  });

  it("thermalHue：40℃ 蓝(240) → 95℃ 红(0)，界外钳制", () => {
    expect(thermalHue(40)).toBe(240);
    expect(thermalHue(67.5)).toBeCloseTo(120, 5);
    expect(thermalHue(95)).toBeCloseTo(0, 5);
    expect(thermalHue(30)).toBe(240); // 低温钳制
    expect(thermalHue(100)).toBeCloseTo(0, 5); // 高温钳制
  });

  it("vaneIntensity（任务栏版）：0→0.05 保底，100MB/s 拉满", () => {
    expect(tbVane(0)).toBeCloseTo(0.05, 5);
    expect(tbVane(100 * 1024 * 1024)).toBeCloseTo(1, 5);
    expect(tbVane(1024 * 1024)).toBeGreaterThan(0.05);
    expect(tbVane(1024 * 1024)).toBeLessThan(1);
  });
});

// ---------------------------------------------------------------------------
// 域5 键盘与输入手感
// ---------------------------------------------------------------------------
describe("singularity/inputDomain 纯逻辑", () => {
  it("inertiaGain：0.4s 后线性加速并限幅", () => {
    expect(inertiaGain(0)).toBe(1);
    expect(inertiaGain(399)).toBe(1);
    expect(inertiaGain(400)).toBe(1);
    expect(inertiaGain(1400, 8)).toBe(8);
    expect(inertiaGain(1400, 4)).toBe(4);
    expect(inertiaGain(10_000, 8)).toBe(8); // 限幅
  });

  it("chordWindowMs：三档和弦时间窗", () => {
    expect(chordWindowMs("strict")).toBe(80);
    expect(chordWindowMs("standard")).toBe(200);
    expect(chordWindowMs("lenient")).toBe(400);
  });

  it("spaceFlowSpeed：按住渐升至满速", () => {
    expect(spaceFlowSpeed(0, 26)).toBeCloseTo(7.8, 5); // 30% 起步
    expect(spaceFlowSpeed(300, 26)).toBeCloseTo(26 * 0.65, 5);
    expect(spaceFlowSpeed(600, 26)).toBe(26);
    expect(spaceFlowSpeed(5000, 26)).toBe(26);
  });

  it("looksSecret：长度 + 字符类 + 常见 key 前缀启发式", () => {
    expect(looksSecret("short")).toBe(false);
    expect(looksSecret("a".repeat(65))).toBe(false); // 超长不判密
    expect(looksSecret("has space 1234")).toBe(false); // 含空白
    expect(looksSecret("Abcdefg1")).toBe(true); // 8 位三类
    expect(looksSecret("abcdefgh")).toBe(false); // 单类
    expect(looksSecret("sk-abcdef123456")).toBe(true); // API key 前缀
    expect(looksSecret("ghp_" + "x".repeat(12))).toBe(true);
  });

  it("DEFAULT_FLOWS：默认键击流结构完整", () => {
    expect(DEFAULT_FLOWS.length).toBeGreaterThan(0);
    for (const f of DEFAULT_FLOWS) {
      expect(f.keys).toBeTruthy();
      expect(f.label).toBeTruthy();
      expect(f.action).toBeTruthy();
    }
  });
});

// ---------------------------------------------------------------------------
// 域6 文件与数据能力
// ---------------------------------------------------------------------------
describe("singularity/filesDomain 纯逻辑", () => {
  it("weightTier：<1MB / <100MB / <1GB / ≥1GB 四档", () => {
    expect(weightTier(MB - 1)).toBe(0);
    expect(weightTier(MB)).toBe(1);
    expect(weightTier(100 * MB - 1)).toBe(1);
    expect(weightTier(100 * MB)).toBe(2);
    expect(weightTier(1024 * MB)).toBe(3);
  });

  it("timeLensRange：五种时间透镜区间", () => {
    const now = new Date(2026, 8, 9, 15, 30); // 2026-09-09
    const day0 = new Date(2026, 8, 9).getTime();
    expect(timeLensRange("today", now)).toEqual([day0, day0 + DAY]);
    expect(timeLensRange("yesterday", now)).toEqual([day0 - DAY, day0]);
    expect(timeLensRange("week", now)).toEqual([day0 - 6 * DAY, day0 + DAY]);
    expect(timeLensRange("month", now)).toEqual([new Date(2026, 8, 1).getTime(), day0 + DAY]);
    expect(timeLensRange("quarter", now)).toEqual([new Date(2026, 6, 1).getTime(), day0 + DAY]); // Q3 起 7 月
  });

  it("nameSuggestions：样本不足时如实不出建议", () => {
    expect(nameSuggestions(["a.txt"], "b.txt", new Date())).toEqual([]);
    expect(nameSuggestions([], "b.txt", new Date())).toEqual([]);
  });

  it("nameSuggestions：日期规律 ≥50% → 接续日期建议", () => {
    const out = nameSuggestions(
      ["2026-01-01 notes.txt", "2026-02-01.txt"],
      "notes.txt",
      new Date(2026, 8, 9),
    );
    expect(out[0]).toBe("2026-09-09.txt");
    expect(out).toContain("notes-new.txt"); // 兜底建议
    expect(out.length).toBeLessThanOrEqual(3);
  });

  it("nameSuggestions：序号规律（含扩展名）→ 取最大序号 +1", () => {
    const out = nameSuggestions(
      ["report(1).pdf", "report(2).pdf", "report(3).pdf"],
      "report.pdf",
      new Date(2026, 8, 9),
    );
    expect(out[0]).toBe("report(4).pdf");
    expect(nameSuggestions(["note(2)", "note(9)"], "note", new Date())[0]).toBe("note(10)"); // 无扩展名同样命中
  });

  it("nameSuggestions：无规律 → 仅 -new 兜底", () => {
    expect(nameSuggestions(["a.txt", "b.txt"], "c.txt", new Date(2026, 8, 9))).toEqual(["c-new.txt"]);
  });
});

// ---------------------------------------------------------------------------
// 域7 效率与工具中枢
// ---------------------------------------------------------------------------
describe("singularity/toolsDomain 纯逻辑", () => {
  it("pomodoroPhase：25+5 循环推进", () => {
    expect(pomodoroPhase(0, 25, 5)).toEqual({ phase: "focus", remainingMs: 25 * 60_000, cycle: 0 });
    expect(pomodoroPhase(26 * 60_000, 25, 5)).toEqual({
      phase: "break",
      remainingMs: 4 * 60_000,
      cycle: 0,
    });
    expect(pomodoroPhase(25 * 60_000, 25, 5)).toEqual({
      phase: "break",
      remainingMs: 5 * 60_000,
      cycle: 0,
    });
    expect(pomodoroPhase(30 * 60_000, 25, 5)).toEqual({
      phase: "focus",
      remainingMs: 25 * 60_000,
      cycle: 1,
    });
  });

  it("dequeue：FIFO 出队（rest 为拷贝）", () => {
    expect(dequeue([])).toBeNull();
    const q = [1, 2, 3];
    const r = dequeue(q);
    expect(r).toEqual({ item: 1, rest: [2, 3] });
    expect(q).toEqual([1, 2, 3]); // 原队列不被修改
  });

  it("selectionQualified：≥10 字符（trim 后）才浮现工具条", () => {
    expect(selectionQualified("123456789")).toBe(false);
    expect(selectionQualified("1234567890")).toBe(true);
    expect(selectionQualified("   1234567890   ")).toBe(true);
    expect(selectionQualified(" ".repeat(20))).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// 域8 系统集成与硬件
// ---------------------------------------------------------------------------
describe("singularity/hardwareDomain 纯逻辑", () => {
  it("auroraIntensity：无数据=0；GPU 直读；无 GPU 用 CPU×0.6 并注明", () => {
    expect(auroraIntensity(null)).toEqual({ v: 0, byCpu: false });
    expect(auroraIntensity(pulse({ gpu_usage: 80 }))).toEqual({ v: 0.8, byCpu: false });
    expect(auroraIntensity(pulse({ cpu_usage: 50 }))).toEqual({ v: 0.3, byCpu: true });
  });

  it("breathProfile：未知电量/插电常亮；满电慢呼吸冷色、低电急促暖色", () => {
    expect(breathProfile(null, false)).toEqual({ periodMs: 0, warm: 0 });
    expect(breathProfile(30, true)).toEqual({ periodMs: 0, warm: 0 });
    expect(breathProfile(100, false)).toEqual({ periodMs: 4000, warm: 0 });
    expect(breathProfile(0, false)).toEqual({ periodMs: 1200, warm: 1 });
    expect(breathProfile(50, false)).toEqual({ periodMs: 2600, warm: 0.5 });
  });

  it("tempHue：40℃ 210°(蓝) → 95℃ 0°(红) 线性，界外钳制", () => {
    expect(tempHue(40)).toBe(210);
    expect(tempHue(67.5)).toBeCloseTo(105, 5);
    expect(tempHue(95)).toBeCloseTo(0, 5);
    expect(tempHue(20)).toBe(210);
    expect(tempHue(120)).toBeCloseTo(0, 5);
  });

  it("tempTrend：样本不足=0；斜率 ±0.6 为趋势阈值", () => {
    expect(tempTrend([])).toBe(0);
    expect(tempTrend([50])).toBe(0);
    expect(tempTrend([50, 51])).toBe(0); // 两样本不足以判趋势
    expect(tempTrend([54, 53, 52, 51, 50])).toBe(1); // 快升
    expect(tempTrend([50, 51, 52])).toBe(-1); // 快降
    expect(tempTrend([51, 50.5, 51])).toBe(0); // 平稳
  });

  it("humidityState：占用率四态分档", () => {
    expect(humidityState(0)).toBe("dry");
    expect(humidityState(29)).toBe("dry");
    expect(humidityState(30)).toBe("moist");
    expect(humidityState(69)).toBe("moist");
    expect(humidityState(70)).toBe("damp");
    expect(humidityState(90)).toBe("damp");
    expect(humidityState(91)).toBe("tide");
  });

  it("vaneIntensity（硬件版）：对数感知，0=0，100MB/s=1，单调递增", () => {
    expect(hwVane(0)).toBe(0);
    expect(hwVane(100 * 1024 * 1024)).toBeCloseTo(1, 5);
    expect(hwVane(1024)).toBeGreaterThan(0);
    expect(hwVane(10 * 1024 * 1024)).toBeGreaterThan(hwVane(1024 * 1024));
    expect(hwVane(1024 * 1024)).toBeGreaterThan(hwVane(1024));
  });
});

// ---------------------------------------------------------------------------
// 域9 兼容性防线
// ---------------------------------------------------------------------------
describe("singularity/compatDomain 纯逻辑", () => {
  it("escortLabel：倒计时文案 / 到点回滚", () => {
    expect(escortLabel(15)).toBe("Keep new display settings? 15s");
    expect(escortLabel(0)).toBe("Reverting…");
    expect(escortLabel(-3)).toBe("Reverting…");
  });

  it("batteryTierCrossed：跨档判定（低电 1 档 / 临界 2 档）", () => {
    expect(batteryTierCrossed(null, 10, 20, 10)).toBeNull();
    expect(batteryTierCrossed(50, 25, 20, 10)).toBeNull(); // 未跨档
    expect(batteryTierCrossed(30, 20, 20, 10)).toBe(1); // 跨低电档
    expect(batteryTierCrossed(30, 15, 20, 10)).toBe(1);
    expect(batteryTierCrossed(15, 5, 20, 10)).toBe(2); // 跨临界档
    expect(batteryTierCrossed(30, 8, 20, 10)).toBe(2); // 直落两档记最深
  });

  it("inBufferWindow：150ms 默认窗口 / 自定义窗口", () => {
    expect(inBufferWindow(1000, 1100)).toBe(true);
    expect(inBufferWindow(1000, 1150)).toBe(true); // 边界含
    expect(inBufferWindow(1000, 1151)).toBe(false);
    expect(inBufferWindow(1000, 1200, 300)).toBe(true);
    expect(inBufferWindow(1000, 1301, 300)).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// 域10 安全与隐私
// ---------------------------------------------------------------------------
describe("singularity/privacyDomain 纯逻辑", () => {
  it("looksSensitive：key 名直判 + 长文本三类启发式", () => {
    expect(looksSensitive("12345678", "password")).toBe(true); // key 名命中
    expect(looksSensitive("12345678", "API-Key")).toBe(true);
    expect(looksSensitive("12345678", "user")).toBe(false); // key 名不命中且数字单类
    expect(looksSensitive("short", "password")).toBe(false); // 长度不足
    expect(looksSensitive("abcdefghijklmnop", "")).toBe(false); // 16 位但单类
    expect(looksSensitive("Abcd1234!@efghij", "")).toBe(true); // 16 位四类
  });

  it("permVerdict：未声明却调用 = 越权", () => {
    expect(permVerdict([{ perm: "fs", declared: true, calls: 9 }])).toBe("clean");
    expect(permVerdict([{ perm: "fs", declared: false, calls: 0 }])).toBe("clean");
    expect(permVerdict([{ perm: "net", declared: false, calls: 1 }])).toBe("over");
    expect(
      permVerdict([
        { perm: "fs", declared: true, calls: 3 },
        { perm: "net", declared: false, calls: 2 },
      ]),
    ).toBe("over");
  });

  it("pruneHistory：30 天滚动清理（边界保留）", () => {
    const now = 1_000_000_000_000;
    const items = [
      { id: "new", ts: now - 29 * DAY },
      { id: "edge", ts: now - 30 * DAY }, // 恰好 30 天：保留
      { id: "old", ts: now - 31 * DAY },
    ];
    expect(pruneHistory(items, now).map((i) => i.id)).toEqual(["new", "edge"]);
    const recent = [
      { id: "hour", ts: now - 3_600_000 },
      { id: "twoday", ts: now - 2 * DAY },
    ];
    expect(pruneHistory(recent, now, 1).map((i) => i.id)).toEqual(["hour"]);
    expect(pruneHistory(items, now, 31).map((i) => i.id)).toEqual(["new", "edge", "old"]);
  });
});

// ---------------------------------------------------------------------------
// 域11 开放生态
// ---------------------------------------------------------------------------
describe("singularity/ecoDomain 纯逻辑", () => {
  it("nutritionRows：未采样如实标注，绝不编造", () => {
    const rows = nutritionRows({ memMb: null, wakeupsPerMin: null, permRequests: null, sampled: false });
    expect(rows).toHaveLength(3);
    expect(rows.every((r) => r.value === "not sampled")).toBe(true);
  });

  it("nutritionRows：采样数据格式化 / 缺项 n/a", () => {
    const rows = nutritionRows({ memMb: 123.6, wakeupsPerMin: 2.5, permRequests: 3, sampled: true });
    expect(rows).toEqual([
      { key: "memory", value: "124 MB" },
      { key: "wakeups", value: "2.5/min" },
      { key: "permissions", value: "3" },
    ]);
    const partial = nutritionRows({ memMb: null, wakeupsPerMin: null, permRequests: null, sampled: true });
    expect(partial.every((r) => r.value === "n/a")).toBe(true);
  });

  it("pluginGrade：三轴评分 A–E", () => {
    expect(pluginGrade(0, 0, 0)).toBe("A");
    expect(pluginGrade(10, 10, 0.005)).toBe("A");
    expect(pluginGrade(21, 0, 0)).toBe("B");
    expect(pluginGrade(21, 16, 0)).toBe("C");
    expect(pluginGrade(21, 16, 0.011)).toBe("D");
    expect(pluginGrade(60, 50, 0.1)).toBe("E"); // 三轴全超 → 封顶
    expect(pluginGrade(0, 0, 0.06)).toBe("C"); // 单轴重度
  });
});

// ---------------------------------------------------------------------------
// 域12 视觉、个性化与氛围
// ---------------------------------------------------------------------------
describe("singularity/visualsDomain 纯逻辑", () => {
  it("seasonOf：月份 → 四季", () => {
    expect(seasonOf(new Date(2026, 2, 1))).toBe("spring"); // 3 月
    expect(seasonOf(new Date(2026, 5, 1))).toBe("summer"); // 6 月
    expect(seasonOf(new Date(2026, 8, 1))).toBe("autumn"); // 9 月
    expect(seasonOf(new Date(2026, 11, 1))).toBe("winter"); // 12 月
    expect(seasonOf(new Date(2026, 0, 15))).toBe("winter"); // 1 月
    expect(seasonOf(new Date(2026, 1, 15))).toBe("winter"); // 2 月
  });

  it("SEASON_HUE：四季各有色相", () => {
    for (const s of ["spring", "summer", "autumn", "winter"] as const) {
      expect(SEASON_HUE[s]).toBeGreaterThanOrEqual(0);
      expect(SEASON_HUE[s]).toBeLessThan(360);
    }
  });

  it("parallaxShift：三层 1× / 0.4× / 0.15× 景深", () => {
    const s = parallaxShift(1, 0, 100);
    expect(s.fg).toEqual([1, 0]);
    expect(s.base).toEqual([0.4, 0]);
    expect(s.far).toEqual([0.15, 0]);
    const half = parallaxShift(1, -1, 50);
    expect(half.fg).toEqual([0.5, -0.5]);
    expect(half.base[0]).toBeCloseTo(0.2, 5);
  });

  it("materialVars：四滑块 → CSS 变量（0-100 钳制为 0-1）", () => {
    const v = materialVars(50, 0, 100, 200);
    expect(v["--singu-mat-fog"]).toBe("0.5");
    expect(v["--singu-mat-refraction"]).toBe("0");
    expect(v["--singu-mat-grain"]).toBe("1");
    expect(v["--singu-mat-gloss"]).toBe("1"); // 超界钳制
    expect(materialVars(-10, 50, 50, 50)["--singu-mat-fog"]).toBe("0");
  });
});

// ---------------------------------------------------------------------------
// 域13 声音与通知
// ---------------------------------------------------------------------------
describe("singularity/soundDomain 纯逻辑", () => {
  it("isFlood：同源 1 秒 ≥3 条为洪流", () => {
    expect(isFlood(0)).toBe(false);
    expect(isFlood(2)).toBe(false);
    expect(isFlood(3)).toBe(true);
    expect(isFlood(10)).toBe(true);
  });

  it("digestDue：日报整点到点（分钟必须为 0）", () => {
    expect(digestDue(new Date(2026, 8, 9, 21, 0), 21)).toBe(true);
    expect(digestDue(new Date(2026, 8, 9, 21, 30), 21)).toBe(false);
    expect(digestDue(new Date(2026, 8, 9, 20, 0), 21)).toBe(false);
  });

  it("shouldMergeLate：>1 条才合流", () => {
    expect(shouldMergeLate(1)).toBe(false);
    expect(shouldMergeLate(2)).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// 域14 无障碍与本地化
// ---------------------------------------------------------------------------
describe("singularity/a11yDomain 纯逻辑", () => {
  it("editDistance：经典 DP（含长度差剪枝）", () => {
    expect(editDistance("abc", "abc")).toBe(0);
    expect(editDistance("abc", "abd")).toBe(1);
    expect(editDistance("kitten", "sitting")).toBe(3);
    expect(editDistance("a", "abcdef")).toBe(3); // 长度差 >2 快速剪枝
    expect(editDistance("", "")).toBe(0);
  });

  it("fuzzyHit：精确子串恒命中；off 不容错；low/high 分级容错", () => {
    expect(fuzzyHit("sett", "settings", "off")).toBe(true); // 精确子串优先
    expect(fuzzyHit("settimgs", "settings", "off")).toBe(false);
    expect(fuzzyHit("settimgs", "settings", "low")).toBe(true); // 1 错字
    expect(fuzzyHit("settimfs", "settings", "low")).toBe(false); // 2 错字超预算
    expect(fuzzyHit("settimfs", "settings", "high")).toBe(true);
  });

  it("fuzzyHit：拼音容错（jisuanqi → 计算器）", () => {
    expect(fuzzyHit("jisuanqi", "计算器", "low")).toBe(true);
    expect(fuzzyHit("jisuanqi", "文件管理器", "low")).toBe(false);
  });

  it("textScalePercent：85..130 钳制取整", () => {
    expect(textScalePercent(100)).toBe(100);
    expect(textScalePercent(84)).toBe(85);
    expect(textScalePercent(131)).toBe(130);
    expect(textScalePercent(99.6)).toBe(100); // 四舍五入
  });

  it("cvdFilter：三型色觉滤镜引用 / none 清空", () => {
    expect(cvdFilter("none")).toBe("");
    expect(cvdFilter("protan")).toBe("url(#singu-cvd-protan)");
    expect(cvdFilter("deutan")).toBe("url(#singu-cvd-deutan)");
    expect(cvdFilter("tritan")).toBe("url(#singu-cvd-tritan)");
  });
});

// ---------------------------------------------------------------------------
// 域15 工程质量、性能与收官
// ---------------------------------------------------------------------------
describe("singularity/qualityDomain 纯逻辑", () => {
  it("flameLayout：宽度∝耗时、x 累进、最小 4px 保底", () => {
    expect(flameLayout([], 500)).toEqual([]);
    const rows = flameLayout(
      [
        { name: "a", ms: 100 },
        { name: "b", ms: 300 },
        { name: "c", ms: 100 },
      ],
      500,
    );
    expect(rows.map((r) => r.w)).toEqual([100, 300, 100]);
    expect(rows.map((r) => r.x)).toEqual([0, 100, 400]);
    const tiny = flameLayout(
      [
        { name: "a", ms: 1 },
        { name: "b", ms: 999 },
      ],
      100,
    );
    expect(tiny[0]!.w).toBe(4); // 最小可见宽
    expect(tiny[1]!.x).toBe(4);
  });

  it("quotaWarn：>90% 预警（严格大于）；零配额不预警", () => {
    expect(quotaWarn(91, 100)).toBe(true);
    expect(quotaWarn(90, 100)).toBe(false);
    expect(quotaWarn(50, 100)).toBe(false);
    expect(quotaWarn(999, 0)).toBe(false);
  });

  it("ecgDrop：环比跌 >30% 判红（null/零基线不判）", () => {
    expect(ecgDrop(null, 10)).toBe(false);
    expect(ecgDrop(0, 10)).toBe(false);
    expect(ecgDrop(100, 69)).toBe(true);
    expect(ecgDrop(100, 70)).toBe(false); // 恰 30% 不算
    expect(ecgDrop(100, 100)).toBe(false);
  });
});
