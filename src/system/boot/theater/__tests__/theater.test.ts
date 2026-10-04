// AURORA-10000：AI-01~AI-05 批次，勿删。
// theater.test.ts — 领域01 启动与品牌剧场（F00001~F00625）交付门禁：
// ID 唯一且连续、25 族 × 25 档、参数档确定性、配速/叙事/报告卡/静音档行为。

import { describe, expect, it } from "vitest";
import { THEATER_FAMILIES, THEATER_ALL_ITEMS, findTheaterItem } from "../registry";
import {
  itemParams, pacingProfile, a11yBootFlags, narrativeLine, NARRATIVE_PACKS,
  colorPipeline, soundProfile, reportStyle, eggSpec,
} from "../params";
import { buildReportCard, announceBootStage } from "../ceremonyFx";
import { playTheaterSound } from "../theaterSound";

describe("AURORA-10000 AI-01~AI-05 registry", () => {
  it("25 族 × 25 项 = 625 项，ID 恰好为 F00001~F00625 连续无重复", () => {
    expect(THEATER_FAMILIES).toHaveLength(25);
    for (const f of THEATER_FAMILIES) expect(f.items).toHaveLength(25);
    expect(THEATER_ALL_ITEMS).toHaveLength(625);
    const ids = THEATER_ALL_ITEMS.map((i) => i.id);
    expect(new Set(ids).size).toBe(625);
    const expected = Array.from({ length: 625 }, (_, k) => `F${String(k + 1).padStart(5, "0")}`);
    expect(ids).toEqual(expected);
  });

  it("kind 与族序一一对应（族0001=arc … 族0025=report）", () => {
    const kinds = ["arc", "breath", "narrative", "soundscape", "color", "diagnostic", "gauge", "trust", "recovery", "firmware", "splash", "transition", "countdown", "selector", "log", "wizard", "workshop", "mood", "shutdown", "wake", "pacing", "a11y", "soundId", "egg", "report"];
    expect(THEATER_FAMILIES.map((f) => f.kind)).toEqual(kinds);
  });

  it("每个条目都有非空名称与描述", () => {
    for (const it of THEATER_ALL_ITEMS) {
      expect(it.name.trim()).not.toBe("");
      expect(it.desc.trim()).not.toBe("");
    }
  });

  it("findTheaterItem 命中与未命中", () => {
    expect(findTheaterItem("F00001")?.name).toContain("极简细线");
    expect(findTheaterItem("F00625")).toBeDefined();
    expect(findTheaterItem("F00626")).toBeUndefined();
    expect(findTheaterItem("F00000")).toBeUndefined();
  });
});

describe("AURORA-10000 AI-01~AI-05 params", () => {
  it("itemParams 确定性：同 ID 恒同参，跨 ID 有差异", () => {
    expect(itemParams("F00001")).toEqual(itemParams("F00001"));
    const hues = new Set(THEATER_ALL_ITEMS.slice(0, 50).map((i) => itemParams(i.id).hue));
    expect(hues.size).toBeGreaterThan(10);
    for (const p of THEATER_ALL_ITEMS.map((i) => itemParams(i.id))) {
      expect(p.hue).toBeGreaterThanOrEqual(0);
      expect(p.hue).toBeLessThan(360);
      expect(p.durScale).toBeGreaterThanOrEqual(0.6);
      expect(p.durScale).toBeLessThanOrEqual(1.4);
    }
  });

  it("配速档：瞬时静默 F00501 全零（无动画/无声），标准 F00503 全 1，未知 ID 回落 1", () => {
    expect(pacingProfile("F00501")).toEqual({ enterScale: 0, holdScale: 0, animKeep: 0, sound: false });
    expect(pacingProfile("F00503")).toMatchObject({ enterScale: 1, holdScale: 1, animKeep: 1, sound: true });
    expect(pacingProfile("F99999")).toMatchObject({ enterScale: 1, holdScale: 1 });
    for (const p of THEATER_FAMILIES.find((f) => f.kind === "pacing")!.items.map((i) => pacingProfile(i.id))) {
      expect(p.enterScale).toBeGreaterThanOrEqual(0);
      expect(p.enterScale).toBeLessThanOrEqual(2);
    }
  });

  it("无障碍开机：读屏/大字/防闪/单键/跳过 各档开关正确映射", () => {
    expect(a11yBootFlags("F00526").announce).toBe(true);
    expect(a11yBootFlags("F00528")).toMatchObject({ largeText: true, highContrast: true });
    expect(a11yBootFlags("F00531").noFlicker).toBe(true);
    expect(a11yBootFlags("F00533").singleSwitch).toBe(true);
    expect(a11yBootFlags("F00549").skipAll).toBe(true);
    expect(a11yBootFlags("F00601")).toEqual(a11yBootFlags("F00601")); // 确定性
    // 非 a11y 族 ID 全关
    const allFalse = Object.values(a11yBootFlags("F00001")).every((v) => v === false);
    expect(allFalse).toBe(true);
  });

  it("叙事包：25 包 × 5 幕，progress 映射正确幕次", () => {
    expect(NARRATIVE_PACKS).toHaveLength(25);
    for (const p of NARRATIVE_PACKS) {
      expect(p.stages).toHaveLength(5);
      for (const s of p.stages) expect(s.trim()).not.toBe("");
    }
    const p0 = NARRATIVE_PACKS[0]!;
    expect(narrativeLine("F00051", 0)).toBe(p0.stages[0]);
    expect(narrativeLine("F00051", 0.99)).toBe(p0.stages[4]);
    expect(narrativeLine("F00051", 0.5)).toBe(p0.stages[2]);
    expect(narrativeLine("F00001", 0.5)).toBe(""); // 非叙事族回退空串
  });

  it("色彩管线 25 档齐全；F00125 纯黑省电=黑底，F00101=sRGB 无滤镜", () => {
    const items = THEATER_FAMILIES.find((f) => f.kind === "color")!.items;
    for (const it of items) expect(colorPipeline(it.id)).toBeDefined();
    expect(colorPipeline("F00101").filter).toBe("none");
    expect(colorPipeline("F00125").blackBase).toBe(true);
  });

  it("音景/声音 ID 参数档：F00100 绝对静音 = 零频零时长", () => {
    const s100 = soundProfile("F00100");
    expect(s100.freqs).toHaveLength(0);
    expect(s100.durMs).toBe(0);
    for (const it of [...THEATER_FAMILIES.find((f) => f.kind === "soundscape")!.items, ...THEATER_FAMILIES.find((f) => f.kind === "soundId")!.items]) {
      const p = soundProfile(it.id);
      expect(p.durMs).toBeGreaterThanOrEqual(0);
      expect(p.freqs.every((f) => f > 20 && f < 4000)).toBe(true); // 人声可闻域内
    }
  });

  it("报告卡 25 档版式齐全；彩蛋总控 F00600 = master", () => {
    const reports = THEATER_FAMILIES.find((f) => f.kind === "report")!.items;
    for (const it of reports) expect(reportStyle(it.id)).toBeDefined();
    expect(eggSpec("F00600").master).toBe(true);
    expect(eggSpec("F00591").trigger).toBe("combo");
    for (const it of THEATER_FAMILIES.find((f) => f.kind === "egg")!.items) {
      const e = eggSpec(it.id);
      if (e.trigger === "auto-low") expect(e.chance).toBeLessThanOrEqual(0.05); // 低频守卫
    }
  });
});

describe("AURORA-10000 AI-01~AI-05 runtime", () => {
  it("buildReportCard：真实统计 + 档位题头/版式", () => {
    const card = buildReportCard("F00602", {
      records: 12, mindmaps: 3, mediaFiles: 7, nodes: 40, elapsedMs: 1500, version: "1.0.0",
    });
    expect(card.title).toBe("系统体检单");
    expect(card.layout).toBe("table");
    expect(card.rows).toHaveLength(6);
    expect(card.rows.find(([k]) => k === "记录")?.[1]).toBe("12");
    expect(card.rows.find(([k]) => k === "耗时")?.[1]).toBe("1.5s");
  });

  it("announceBootStage：非 announce 档不产生 DOM 播报（jsdom 安全）", () => {
    expect(() => announceBootStage("F00001", "启动自检", true)).not.toThrow();
  });

  it("playTheaterSound：绝对静音档 F00100 永不创建音频（无 AudioContext 环境返回 null）", () => {
    expect(playTheaterSound("F00100", 0.5, false)).toBeNull();
    expect(playTheaterSound("F00100", 0.5, true)).toBeNull();
    expect(playTheaterSound("F00076", 0.5, true)).toBeNull(); // 全局静音尊重
  });
});
