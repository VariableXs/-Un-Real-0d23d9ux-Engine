import { describe, expect, it } from "vitest";
import {
  clockFace, isLeapYear, clockDrift, normalizeWeather, sanitizeSample,
  nextFetchAt, recordFetchFailure, recordFetchSuccess, freshness,
  newCostLedger, recordCost, costP95,
} from "../widget-data";
import {
  visibleLayers, wakeBudget, blurForWallpaper, greetingFor, GREETING_TEXT,
  retreatProgress, layoutDigest, UnlockMachine, DEFAULT_UNLOCK_CONFIG,
} from "../lockscreen-composer";
import {
  segmentSyllables, generateCandidates, compose, paginateCandidates,
  numberKeyToCandidate, compositionFlags,
} from "../ime-composition";

// ---------- widget-data ----------

describe("widget-data · 小组件数据引擎（F163 深化）", () => {
  it("时钟面：月历格数与首行空格正确（含闰年 2028 二月）", () => {
    const feb2028 = clockFace(new Date(2028, 1, 15));
    expect(isLeapYear(2028)).toBe(true);
    expect(feb2028.daysInMonth).toBe(29);
    const feb2029 = clockFace(new Date(2029, 1, 15));
    expect(isLeapYear(2029)).toBe(false);
    expect(feb2029.daysInMonth).toBe(28);
    expect(feb2028.firstWeekday).toBe(new Date(2028, 1, 1).getDay());
    expect(feb2028.isoDate).toBe("2028-02-15");
  });

  it("时钟漂移检测：容差内不报、超差报 delta", () => {
    expect(clockDrift(1000_000, 1000_500, 2000).drifted).toBe(false);
    const r = clockDrift(1000_000, 1000_500, 300);
    expect(r.drifted).toBe(true);
    expect(r.deltaMs).toBe(-500);
  });

  it("天气归一：华氏转摄氏、体感回退、脏值逐项拒绝", () => {
    const ok = normalizeWeather({ source: "s", raw: { temp: 75, unit: "F", feels: 70, humidity: 55, wind: 12, condition: "Sunny" } }, 1000);
    expect(ok.ok).toBe(true);
    if (ok.ok) {
      expect(ok.data.tempC).toBeCloseTo(23.9, 1);
      expect(ok.data.condition).toBe("sunny");
    }
    const fallbackFeel = normalizeWeather({ source: "s", raw: { temp: 20, humidity: 40, wind: 5, condition: "rain" } }, 1000);
    expect(fallbackFeel.ok).toBe(true);
    if (fallbackFeel.ok) expect(fallbackFeel.data.feelsC).toBe(20); // 体感缺失回退实际温度。
    const bad = normalizeWeather({ source: "s", raw: { temp: 999, humidity: 300, wind: "fast", condition: "volcano" } }, 1000);
    expect(bad.ok).toBe(false);
    if (!bad.ok) expect(bad.reasons.length).toBeGreaterThanOrEqual(3);
  });

  it("系统采样清洗：NaN/负值/越界全钳制且逐项登记", () => {
    const r = sanitizeSample({ cpuPct: 133.7, memPct: -4, diskPct: 61.2, netKbps: Number.NaN });
    expect(r.clean.cpuPct).toBe(100);
    expect(r.clean.memPct).toBe(0);
    expect(r.clean.netKbps).toBe(0);
    expect(r.fixed.length).toBeGreaterThan(0);
  });

  it("调度：抖动确定性与失败指数退避（封顶 8×）", () => {
    const base: Parameters<typeof nextFetchAt>[0] = { sourceId: "w", intervalMs: 60_000, jitterMs: 15_000, failures: 0, lastOkAt: null };
    expect(nextFetchAt(base, 1000, () => 0.5)).toBe(61_000);
    expect(nextFetchAt(base, 1000, () => 1)).toBe(76_000);
    expect(nextFetchAt(base, 1000, () => 0)).toBe(46_000);
    const f1 = recordFetchFailure({ ...base }, 0);
    expect(f1.failures).toBe(1);
    const f5 = recordFetchFailure(recordFetchFailure(recordFetchFailure(recordFetchFailure(f1, 0), 0), 0), 0);
    expect(nextFetchAt(f5, 0, () => 0.5)).toBe(8 * 60_000); // 2^5=32 → 封顶 8×。
    const ok = recordFetchSuccess(f5, 123);
    expect(ok.failures).toBe(0);
    expect(ok.lastOkAt).toBe(123);
  });

  it("新鲜度四级：live/stale/frozen/never（人话标注不装新）", () => {
    const plan = { sourceId: "w", intervalMs: 1000, jitterMs: 0, failures: 0, lastOkAt: 5000 };
    const fmt = (t: number): string => `@${t}`;
    expect(freshness(plan, 6500, fmt).level).toBe("live");
    expect(freshness(plan, 9000, fmt).level).toBe("stale");
    const fz = freshness(plan, 20000, fmt);
    expect(fz.level).toBe("frozen");
    expect(fz.label).toContain("连接中断");
    const never = freshness({ ...plan, lastOkAt: null }, 6500, fmt);
    expect(never.level).toBe("never");
    expect(never.label).toContain("暂无数据");
  });

  it("开销计量：滑窗封顶 + P95 排序口径", () => {
    let l = newCostLedger("w", 10);
    for (let i = 1; i <= 20; i++) l = recordCost(l, i);
    expect(l.samples).toHaveLength(10); // 窗口封顶，老的淘汰。
    expect(costP95(l)).toBe(20);
    expect(costP95(newCostLedger("w", 10))).toBe(0);
  });
});

// ---------- lockscreen-composer ----------

describe("lockscreen-composer · 锁屏合成（F164 深化）", () => {
  it("两拍渐进：第一拍恒可见、2s 后全部可见", () => {
    const at0 = visibleLayers(0);
    expect(at0).toContain("backdrop");
    expect(at0).toContain("clock");
    expect(at0).not.toContain("hint");
    const late = visibleLayers(2000);
    expect(late).toHaveLength(6);
  });

  it("唤醒预算：第一拍 <100ms、整体 <2s（≤2s 判据的拆账）", () => {
    const b = wakeBudget();
    expect(b.firstBeatMs).toBeLessThan(100);
    expect(b.fullMs).toBeLessThan(2000);
    expect(b.withinBudget).toBe(true);
  });

  it("虚化参数：亮壁纸压暗、暗壁纸不压（时钟可读性优先）", () => {
    const bright = blurForWallpaper(0.8);
    const dark = blurForWallpaper(0.1);
    expect(bright.brightness).toBeLessThan(1);
    expect(dark.brightness).toBe(1);
    expect(bright.radiusPx).toBeGreaterThan(0);
  });

  it("问候分段与双语（有人味的细节有契约）", () => {
    expect(greetingFor(2)).toBe("night");
    expect(greetingFor(6)).toBe("dawn");
    expect(greetingFor(9)).toBe("morning");
    expect(greetingFor(12)).toBe("noon");
    expect(greetingFor(15)).toBe("afternoon");
    expect(greetingFor(20)).toBe("evening");
    expect(GREETING_TEXT.evening.zh).toContain("晚上");
    expect(GREETING_TEXT.evening.en).toContain("evening");
  });

  it("让位插值：easeOutCubic、终点确定性（translateY/scale/opacity）", () => {
    const start = retreatProgress(0);
    const end = retreatProgress(1);
    expect(start.translateY).toBe(-0);
    expect(end.translateY).toBe(-64);
    expect(end.scale).toBeCloseTo(0.84, 5);
    expect(end.opacity).toBe(0.75);
    expect(retreatProgress(2).translateY).toBe(-64); // 越界钳制。
  });

  it("通知摘要：前 4 组 + 折叠合计（只计数不显内容）", () => {
    const d = layoutDigest([
      { appId: "a", icon: "a", count: 1 },
      { appId: "b", icon: "b", count: 5 },
      { appId: "c", icon: "c", count: 3 },
      { appId: "d", icon: "d", count: 2 },
      { appId: "e", icon: "e", count: 4 },
    ]);
    expect(d.shown).toHaveLength(4);
    expect(d.shown[0]!.count).toBe(5); // 计数降序。
    expect(d.overflowCount).toBe(1);
    expect(d.total).toBe(15);
  });

  it("解锁状态机：错 5 次进冷却（15s 起指数倍增）、冷却内提交被拒不延长、成功清零", () => {
    const m = new UnlockMachine();
    let now = 0;
    for (let i = 0; i < 4; i++) {
      const r = m.submit(false, now);
      expect(r.type).toBe("rejected");
      now += 100;
    }
    const fifth = m.submit(false, now);
    expect(fifth.type).toBe("cooldown-started");
    if (fifth.type === "cooldown-started") expect(fifth.durationMs).toBe(DEFAULT_UNLOCK_CONFIG.cooldownBaseMs);
    const during = m.submit(true, now + 1000);
    expect(during.type).toBe("cooldown-active"); // 冷却内提交拒绝（不延长——惩罚与提示分离）。
    m.cooldownExpired(now + 16_000);
    expect(m.current).toBe("idle");
    const ok = m.submit(true, now + 16_100);
    expect(ok.type).toBe("accepted");
    expect(m.remainingAttempts).toBe(DEFAULT_UNLOCK_CONFIG.maxAttempts); // 成功即信任——计数清零。
  });

  it("第二次冷却时长翻倍；lockOut 显性入口；冷却剩余随时间递减", () => {
    const cfg = { maxAttempts: 2, cooldownBaseMs: 10_000, cooldownMaxMs: 300_000 };
    const m = new UnlockMachine(cfg);
    let now = 0;
    m.submit(false, now);
    const first = m.submit(false, now + 10);
    expect(first.type).toBe("cooldown-started");
    if (first.type === "cooldown-started") expect(first.durationMs).toBe(10_000);
    m.cooldownExpired(now + 10_100);
    now += 10_100;
    // 冷却后 failures 保持 2（≥ maxAttempts）——下一次提交即触发第 2 轮冷却（20s，翻倍）。
    const second = m.submit(false, now);
    expect(second.type).toBe("cooldown-started");
    if (second.type === "cooldown-started") expect(second.durationMs).toBe(20_000);
    // 封顶验证：连错多轮时长不再超过 300s（每轮 2 次失败——第 2 次触发冷却）。
    const m3 = new UnlockMachine({ maxAttempts: 2, cooldownBaseMs: 60_000, cooldownMaxMs: 300_000 });
    let t = 0;
    let lastDuration = 0;
    for (let round = 0; round < 40; round++) {
      const a = m3.submit(false, t); t += 5; // failures 已 ≥ maxAttempts——每轮首个提交即触发冷却。
      const b = m3.submit(false, t); t += 5; // 冷却期内提交 → active（被拒不延长）。
      if (a.type === "cooldown-started") lastDuration = a.durationMs;
      if (b.type === "cooldown-started") lastDuration = b.durationMs;
      t += 601_000;
      m3.cooldownExpired(t);
    }
    expect(lastDuration).toBe(300_000); // 封顶。
    const m2 = new UnlockMachine();
    m2.lockOut();
    expect(m2.submit(true, 0).type).toBe("locked-out");
  });
});

// ---------- ime-composition ----------

describe("ime-composition · 拼音组合引擎（F166 深化）", () => {
  it("音节切分：nihao → ni'hao；zh/ch/sh 双字母边界正确", () => {
    const segs = segmentSyllables("nihao");
    expect(segs[0]).toEqual(["ni", "hao"]);
    const zh = segmentSyllables("zhu");
    expect(zh.some((s) => s.join("") === "zhu")).toBe(true);
    const pure = segmentSyllables("好"); // 非字母——清洗后为空输入 → 空切分（不炸不造假）。
    expect(pure).toEqual([]);
  });

  it("歧义串保留多解：xian 单音节与 xi'an 两解都在", () => {
    const segs = segmentSyllables("xian");
    const joined = segs.map((s) => s.join(""));
    expect(joined).toContain("xian");
    expect(joined).toContain("xian".slice(0, 2) + "an"); // xi+an。
  });

  it("候选生成：全匹配 + 前缀匹配、权重降序、封顶 limit", () => {
    const full = generateCandidates("nihao");
    expect(full[0]!.text).toBe("你好"); // 权重 100 居首。
    expect(full[0]!.syllables).toEqual(["ni", "hao"]); // 候选回指切分。
    const prefix = generateCandidates("xianz");
    expect(prefix.some((c) => c.text === "现在")).toBe(true); // 边打边选。
    expect(generateCandidates("shijian", 2)).toHaveLength(2);
    expect(generateCandidates("")).toEqual([]);
  });

  it("组合串编辑：中间插入、回删边界、delete、caret 钳制、commit 清空", () => {
    let s: { raw: string; caret: number } = { raw: "ni", caret: 2 };
    s = compose(s, { type: "input", ch: "H" });
    expect(s).toEqual({ raw: "nih", caret: 3 }); // 大写归一。
    s = compose(s, { type: "input", ch: "1" });
    expect(s.raw).toBe("nih"); // 非字母不进组合串（B-904）。
    s = compose(s, { type: "caret-move", to: 1 });
    s = compose(s, { type: "delete" });
    expect(s).toEqual({ raw: "nh", caret: 1 });
    s = compose(s, { type: "backspace" });
    expect(s).toEqual({ raw: "h", caret: 0 });
    expect(compose(s, { type: "backspace" })).toEqual({ raw: "h", caret: 0 }); // 边界：不动。
    s = compose(s, { type: "input", ch: "a" });
    expect(compose(s, { type: "commit", text: "a" })).toEqual({ raw: "", caret: 0 });
    s = { raw: "abc", caret: 99 };
    s = compose(s, { type: "caret-move", to: 99 });
    expect(s.caret).toBe(3); // 钳制。
  });

  it("候选分页：9/页、页号钳制、数字键直选与越界显性化", () => {
    const all = Array.from({ length: 23 }, (_, i) => ({ text: `c${i}`, syllables: ["x"], weight: 100 - i }));
    const p0 = paginateCandidates(all, 0);
    expect(p0.pageCount).toBe(3);
    expect(p0.items).toHaveLength(9);
    expect(p0.numberHints).toEqual([1, 2, 3, 4, 5, 6, 7, 8, 9]);
    expect(paginateCandidates(all, 99).page).toBe(2); // 钳制。
    expect(paginateCandidates(all, -1).page).toBe(0);
    const p2 = paginateCandidates(all, 2);
    expect(p2.items).toHaveLength(5);
    expect(numberKeyToCandidate(0, 9, 23)).toBe(8);
    expect(numberKeyToCandidate(1, 9, 23)).toBe(17); // 第 2 页第 9 个 = 9+8。
    expect(numberKeyToCandidate(2, 5, 23)).toBe(22); // 第 3 页第 5 个 = 18+4。
    expect(numberKeyToCandidate(0, 9, 5)).toBeNull(); // 越界。
    expect(numberKeyToCandidate(0, 0, 23)).toBeNull(); // 非 1-9。
  });

  it("组合期旗标：B-904 快捷键让位契约", () => {
    const empty = compositionFlags({ raw: "", caret: 0 }, []);
    expect(empty).toEqual({ composing: false, candidateWindow: false });
    const active = compositionFlags({ raw: "ni", caret: 2 }, [{ text: "你", syllables: ["ni"], weight: 1 }]);
    expect(active).toEqual({ composing: true, candidateWindow: true });
  });
});
