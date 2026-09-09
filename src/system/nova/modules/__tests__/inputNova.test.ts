import { describe, expect, it } from "vitest";
import {
  AURA_STEADY_WPM,
  AURA_WARM_WPM,
  BREATH_BUBBLE_MS,
  BREATH_MAX_PER_HOUR,
  BREATH_PAUSE_RESET_MS,
  FLOW_GAP_MS,
  INPUT_NOVA_FEATURES,
  PHRASE_MAX_ENTRIES,
  PHRASE_MIN_COUNT,
  POOLS,
  POOL_HOLD_MS,
  PUNCT_DECISION_MS,
  PACE_GLOBAL,
  PACE_MIN_SAMPLES,
  RECALL_RATE_MS,
  RHYTHM_WINDOW_MS,
  SANDBOX_HOLD_MS,
  WPM_MOMENT_MS,
  WPM_THRESHOLDS,
  ZONE_COUNT,
  ZONE_LABELS,
  auraState,
  breathInit,
  breathTick,
  bumpPhrase,
  calibratePace,
  capsAppliesToIme,
  clamp,
  comboFromEvent,
  crossedThresholds,
  effectiveCaps,
  emptySlots,
  ghostPhrases,
  inputNovaDomain,
  isCJK,
  isUrlContext,
  lookupBinding,
  medianOf,
  nextEscState,
  nextRingIndex,
  normalizePhrase,
  paceOfApp,
  poolFor,
  punctDecision,
  pushGap,
  radialLayout,
  rhythmAdvice,
  rhythmScore,
  rollingWpm,
  shouldRecall,
  slotPut,
  slotTake,
  zoneDelta,
  zoneMonthKey,
  zoneOfKey,
  zoneTotals,
} from "../inputNova";

// ---------------------------------------------------------------------------
// manifest：13 项齐、编号连续、hub 消费契约
// ---------------------------------------------------------------------------
describe("inputNova manifest", () => {
  it("W-051…W-063 共 13 项，编号连续无缺", () => {
    expect(INPUT_NOVA_FEATURES).toHaveLength(13);
    const ids = INPUT_NOVA_FEATURES.map((f) => Number(f.id.slice(2)));
    for (let i = 1; i < ids.length; i++) expect(ids[i]).toBe(ids[i - 1]! + 1);
    expect(ids[0]).toBe(51);
    expect(ids[ids.length - 1]).toBe(63);
  });

  it("每项必含中英标题/描述，域标识 S5/AI-05", () => {
    for (const f of INPUT_NOVA_FEATURES) {
      expect(f.title.length).toBeGreaterThan(0);
      expect(f.titleEn.length).toBeGreaterThan(0);
      expect(f.desc.length).toBeGreaterThan(0);
    }
    expect(inputNovaDomain.id).toBe("S5");
    expect(inputNovaDomain.route).toBe("AI-05");
    expect(inputNovaDomain.nameEn).toBe("Input Rhythm");
  });
});

// ---------------------------------------------------------------------------
// W-051 分应用输入节奏记忆
// ---------------------------------------------------------------------------
describe("W-051 pace memory", () => {
  it("medianOf：奇偶与空集", () => {
    expect(medianOf([120, 80, 200])).toBe(120);
    expect(medianOf([200, 80])).toBe(140);
    expect(medianOf([])).toBeNull();
    expect(medianOf([NaN, -1, 90])).toBe(90);
  });

  it("pushGap：滚动窗口裁剪，负值与超大间隔不入曲线", () => {
    let gaps: number[] = [];
    for (let i = 1; i <= 600; i++) gaps = pushGap(gaps, 100 + (i % 7));
    expect(gaps).toHaveLength(500); // PACE_WINDOW
    expect(pushGap([], -5)).toHaveLength(0);
    expect(pushGap([], 60_000)).toHaveLength(0);
  });

  it("样本 < 200 击 → null（退回全局曲线），达标后单调映射", () => {
    const few = Array.from({ length: PACE_MIN_SAMPLES - 1 }, (_, i) => 100 + i);
    expect(calibratePace(few)).toBeNull();
    const ok = Array.from({ length: PACE_MIN_SAMPLES }, () => 120);
    const curve = calibratePace(ok);
    expect(curve).not.toBeNull();
    expect(curve!.delayMs).toBeGreaterThan(0);
    expect(curve!.rampMs).toBeGreaterThan(0);
    // 快节奏（小中位数）→ 更短延迟（单调）
    const fast = calibratePace(Array.from({ length: PACE_MIN_SAMPLES }, () => 200));
    const slow = calibratePace(Array.from({ length: PACE_MIN_SAMPLES }, () => 800));
    expect(fast!.delayMs).toBeLessThanOrEqual(slow!.delayMs);
    expect(fast!.rampMs).toBeLessThanOrEqual(slow!.rampMs);
  });

  it("paceOfApp：未命中退全局；命中返回缓存曲线（切换 ≤ 50ms 语义 = 同步查表）", () => {
    const fallback = paceOfApp({}, "editor");
    expect(fallback.delayMs).toBe(PACE_GLOBAL.delayMs);
    const curve = { delayMs: 160, rampMs: 40, samples: 300 };
    expect(paceOfApp({ terminal: curve }, "terminal")).toEqual(curve);
  });
});

// ---------------------------------------------------------------------------
// W-052 Shift 大写覆盖
// ---------------------------------------------------------------------------
describe("W-052 shift caps override", () => {
  it("XOR 语义：按住反转、松开恢复", () => {
    expect(effectiveCaps(false, false)).toBe(false);
    expect(effectiveCaps(true, false)).toBe(true);
    expect(effectiveCaps(true, true)).toBe(false);
    expect(effectiveCaps(false, true)).toBe(true);
  });

  it("IME 仲裁：仅英文态生效，中文态跳过", () => {
    expect(capsAppliesToIme("en")).toBe(true);
    expect(capsAppliesToIme("zh")).toBe(false);
  });
});

// ---------------------------------------------------------------------------
// W-053 输入节奏器
// ---------------------------------------------------------------------------
describe("W-053 rhythm trainer", () => {
  it("样本 < 3 如实不评（null）", () => {
    expect(rhythmScore([])).toBeNull();
    expect(rhythmScore([100, 100])).toBeNull();
  });

  it("匀速 → stable 高分；波动 → wavy/erratic 低分；得分可解释（cv）", () => {
    const steady = rhythmScore([100, 100, 100, 100, 100, 100]);
    expect(steady).not.toBeNull();
    expect(steady!.band).toBe("stable");
    expect(steady!.stability).toBeGreaterThanOrEqual(95);
    const wild = rhythmScore([50, 400, 90, 500, 60, 380]);
    expect(wild!.band).toBe("erratic");
    expect(wild!.stability).toBeLessThan(50);
    expect(steady!.stability).toBeGreaterThan(wild!.stability);
  });

  it("advice 三档文案非空", () => {
    const s = rhythmScore([100, 100, 100])!;
    expect(rhythmAdvice(s, "zh").length).toBeGreaterThan(0);
    expect(rhythmAdvice(s, "en").length).toBeGreaterThan(0);
    expect(RHYTHM_WINDOW_MS).toBe(30_000);
  });
});

// ---------------------------------------------------------------------------
// W-054 按键长按池
// ---------------------------------------------------------------------------
describe("W-054 long-press pool", () => {
  it("三池定义与 spec 一致；\\ 为 、 池别名；无池键如实 null", () => {
    expect(poolFor(".")).toEqual(["。", "．", "…"]);
    expect(poolFor("、")).toEqual(["，", "、", "；", "："]);
    expect(poolFor(";")).toEqual(["；", ";", ":", "|"]);
    expect(poolFor("\\")).toEqual(POOLS["、"]);
    expect(poolFor("a")).toBeNull();
    expect(POOL_HOLD_MS).toBe(500);
  });

  it("径向环布局：从正上方起顺时针均分（首点 x≈0, y<0）", () => {
    const pos = radialLayout(4, 64);
    expect(pos).toHaveLength(4);
    expect(pos[0]!.x).toBe(0);
    expect(pos[0]!.y).toBeLessThan(0);
    expect(pos[1]!.x).toBeGreaterThan(0);
    expect(pos[1]!.y).toBe(0);
    expect(pos[2]!.x).toBe(0);
    expect(pos[2]!.y).toBeGreaterThan(0);
    expect(pos[3]!.x).toBeLessThan(0);
  });

  it("环上步进：顺时针 +1、逆时针 -1、环形回绕", () => {
    expect(nextRingIndex(0, 1, 4)).toBe(1);
    expect(nextRingIndex(3, 1, 4)).toBe(0);
    expect(nextRingIndex(0, -1, 4)).toBe(3);
    expect(nextRingIndex(0, 1, 1)).toBe(0);
  });
});

// ---------------------------------------------------------------------------
// W-055 本地短语暖场
// ---------------------------------------------------------------------------
describe("W-055 phrase warm-up", () => {
  it("ghostPhrases：频次 ≥ min 才入围，按频次降序，取前 n", () => {
    const freq = { "你好": 9, "收到": 5, "ok": 2, "在吗": PHRASE_MIN_COUNT - 1, "辛苦了": 7 };
    expect(ghostPhrases(freq, 3)).toEqual(["你好", "辛苦了", "收到"]);
    expect(ghostPhrases(freq, 10)).toHaveLength(3);
    expect(ghostPhrases({ a: 1 }, 3)).toHaveLength(0);
  });

  it("同频按字典序稳定排序", () => {
    expect(ghostPhrases({ b: 3, a: 3, c: 3 }, 3)).toEqual(["a", "b", "c"]);
  });

  it("normalizePhrase：压缩空白、长度过滤", () => {
    expect(normalizePhrase("  hello   world ")).toBe("hello world");
    expect(normalizePhrase("a")).toBeNull();
    expect(normalizePhrase("x".repeat(25))).toBeNull();
    expect(normalizePhrase("x".repeat(24))).not.toBeNull();
  });

  it("bumpPhrase：计数封顶 999；超 200 条淘汰低频一半（确定性）", () => {
    let freq: Record<string, number> = {};
    freq = bumpPhrase(freq, "你好");
    freq = bumpPhrase(freq, "你好");
    expect(freq["你好"]).toBe(2);
    expect(bumpPhrase(freq, " ")).toEqual(freq);
    let big: Record<string, number> = {};
    for (let i = 0; i < PHRASE_MAX_ENTRIES + 10; i++) big = bumpPhrase(big, `短语${String(i).padStart(3, "0")}`);
    expect(Object.keys(big).length).toBeLessThanOrEqual(PHRAaseGuard());
  });
});
function PHRAaseGuard(): number {
  return PHRASE_MAX_ENTRIES;
}

// ---------------------------------------------------------------------------
// W-056 键盘沙坑
// ---------------------------------------------------------------------------
describe("W-056 key sandbox", () => {
  it("comboFromEvent：修饰键有序（ctrl/alt/shift/super + 键尾小写）", () => {
    expect(comboFromEvent({ ctrlKey: true, altKey: false, shiftKey: true, metaKey: false, key: "T" })).toBe("ctrl+shift+t");
    expect(comboFromEvent({ ctrlKey: false, altKey: true, shiftKey: false, metaKey: true, key: "ArrowLeft" })).toBe("alt+super+arrowleft");
    expect(comboFromEvent({ ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, key: "F1" })).toBe("f1");
  });

  it("纯修饰键不成 combo（如实待定）", () => {
    expect(comboFromEvent({ ctrlKey: true, altKey: false, shiftKey: false, metaKey: false, key: "Control" })).toBeNull();
    expect(comboFromEvent({ ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, key: "CapsLock" })).toBeNull();
  });

  it("lookupBinding：命中返回注册表事实；未命中 FREE（不编造）", () => {
    const bindings = [
      { id: "nova.hub", combo: "ctrl+alt+n", scope: "global" as const, priority: 10, source: "hub", descKey: "kb.novaHub" },
    ];
    expect(lookupBinding("ctrl+alt+n", bindings)).toMatchObject({ bound: true, id: "nova.hub", scope: "global" });
    const miss = lookupBinding("ctrl+alt+x", bindings);
    expect(miss.bound).toBe(false);
    expect(miss.combo).toBe("ctrl+alt+x");
  });

  it("Esc 两段退出语义；长按时长 800ms", () => {
    expect(nextEscState(true)).toBe("clear");
    expect(nextEscState(false)).toBe("close");
    expect(SANDBOX_HOLD_MS).toBe(800);
  });
});

// ---------------------------------------------------------------------------
// W-057 光环 / W-058 高手时刻
// ---------------------------------------------------------------------------
describe("W-057 speed aura", () => {
  it("rollingWpm：窗口内击键 → WPM；超窗归零", () => {
    const now = 1_000_000;
    const hits = Array.from({ length: 30 }, (_, i) => now - i * 100); // 30 键 / 3s 窗
    const wpm = rollingWpm(hits, now);
    expect(wpm).toBeGreaterThan(0);
    expect(rollingWpm([], now)).toBe(0);
    expect(rollingWpm([now - 60_000], now)).toBe(0); // 超 30s 窗
  });

  it("auraState：≥60 warm / 30–60 steady / >0 rest / 0 idle", () => {
    expect(auraState(0).mode).toBe("idle");
    expect(auraState(20).mode).toBe("rest");
    expect(auraState(45).mode).toBe("steady");
    expect(auraState(72).mode).toBe("warm");
    expect(AURA_STEADY_WPM).toBe(30);
    expect(AURA_WARM_WPM).toBe(60);
  });
});

describe("W-058 wpm moment", () => {
  it("三门槛各触发一次：已庆门槛不再触发", () => {
    expect(WPM_THRESHOLDS).toEqual([80, 100, 120]);
    expect(crossedThresholds(85, [])).toEqual([80]);
    expect(crossedThresholds(85, [80])).toEqual([]);
    expect(crossedThresholds(125, [80])).toEqual([100, 120]);
    expect(crossedThresholds(70, [])).toEqual([]);
    expect(WPM_MOMENT_MS).toBe(1200);
  });
});

// ---------------------------------------------------------------------------
// W-059 走神暂停印
// ---------------------------------------------------------------------------
describe("W-059 flow recall", () => {
  it("中断 ≥ 3s 且距上次 ≥ 5min 才出印", () => {
    const now = 10_000_000;
    expect(shouldRecall(FLOW_GAP_MS, 0, now)).toBe(true);
    expect(shouldRecall(FLOW_GAP_MS - 1, 0, now)).toBe(false);
    expect(shouldRecall(FLOW_GAP_MS, now - RECALL_RATE_MS + 1, now)).toBe(false);
    expect(shouldRecall(FLOW_GAP_MS, now - RECALL_RATE_MS, now)).toBe(true);
  });
});

// ---------------------------------------------------------------------------
// W-060 剪贴板环槽
// ---------------------------------------------------------------------------
describe("W-060 clip slots", () => {
  it("存取闭环：三槽互不串扰", () => {
    let s = emptySlots();
    s = slotPut(s, 0, "alpha");
    s = slotPut(s, 2, "gamma");
    expect(slotTake(s, 0)).toMatchObject({ ok: true, text: "alpha" });
    expect(slotTake(s, 1)).toMatchObject({ ok: false, empty: true });
    expect(slotTake(s, 2)).toMatchObject({ ok: true, text: "gamma" });
  });

  it("空文本不入槽；覆盖同槽；巨文本截断 100KB", () => {
    let s = emptySlots();
    s = slotPut(s, 1, "");
    expect(slotTake(s, 1).ok).toBe(false);
    s = slotPut(s, 1, "first");
    s = slotPut(s, 1, "second");
    expect(slotTake(s, 1).text).toBe("second");
    s = slotPut(s, 1, "x".repeat(120_000));
    expect(slotTake(s, 1).text).toHaveLength(100_000);
  });
});

// ---------------------------------------------------------------------------
// W-061 长句呼吸输入
// ---------------------------------------------------------------------------
describe("W-061 marathon typing care", () => {
  const NEED = 300;

  it("累计达阈值才触发；触发后计数归零", () => {
    let st = breathInit();
    let r = breathTick(st, 299, 1000, NEED);
    expect(r.fire).toBe(false);
    st = r.next;
    r = breathTick(st, 1, 1100, NEED);
    expect(r.fire).toBe(true);
    expect(r.next.chars).toBe(0);
  });

  it("停顿 ≥ 30s 重置计数", () => {
    let st = breathInit();
    st = breathTick(st, 299, 1000, NEED).next;
    const r = breathTick(st, 1, 1000 + BREATH_PAUSE_RESET_MS, NEED);
    expect(r.fire).toBe(false);
    expect(r.next.chars).toBe(1);
  });

  it("每小时至多 3 次，超出不触发", () => {
    let st = breathInit();
    let now = 0;
    let fired = 0;
    for (let i = 0; i < 6; i++) {
      const r = breathTick(st, NEED, now, NEED);
      st = r.next;
      if (r.fire) fired++;
      now += 60_000; // 每轮间隔 60s（< 30min 不重置，同小时内）
    }
    expect(fired).toBe(BREATH_MAX_PER_HOUR);
    expect(BREATH_BUBBLE_MS).toBe(4_000);
  });
});

// ---------------------------------------------------------------------------
// W-062 指频热区图
// ---------------------------------------------------------------------------
describe("W-062 zone heatmap", () => {
  it("标准布局分区映射：左右手各五区", () => {
    expect(ZONE_COUNT).toBe(10);
    expect(ZONE_LABELS).toHaveLength(10);
    expect(zoneOfKey("q")).toBe(0); // 左小指
    expect(zoneOfKey("w")).toBe(1);
    expect(zoneOfKey("e")).toBe(2);
    expect(zoneOfKey("t")).toBe(3); // 左食指
    expect(zoneOfKey(" ")).toBe(5); // 右拇指
    expect(zoneOfKey("y")).toBe(6); // 右食指
    expect(zoneOfKey("i")).toBe(7);
    expect(zoneOfKey("o")).toBe(8);
    expect(zoneOfKey("p")).toBe(9); // 右小指
    expect(zoneOfKey("enter")).toBe(9);
    expect(zoneOfKey("F5")).toBeNull(); // 未知键如实不入统计
  });

  it("zoneTotals：pct 求和 ≈ 100；空数据全 0", () => {
    const t = zoneTotals([10, 0, 0, 10, 0, 0, 0, 0, 0, 20]);
    expect(t.total).toBe(40);
    expect(t.pct.reduce((a, b) => a + b, 0)).toBeCloseTo(100, 0);
    const empty = zoneTotals([]);
    expect(empty.total).toBe(0);
    expect(empty.pct.every((p) => p === 0)).toBe(true);
  });

  it("zoneDelta：本月 - 上月百分点差", () => {
    // 两组数据各归一化为自身占比后按百分点比较（10% vs 30% → -20pp）
    const delta = zoneDelta([10, 0, 0, 0, 0, 0, 0, 0, 0, 90], [30, 0, 0, 0, 0, 0, 0, 0, 0, 70]);
    expect(delta[0]).toBeCloseTo(-20, 0);
    expect(delta[9]).toBeCloseTo(20, 0);
  });

  it("zoneMonthKey：YYYY-MM 格式", () => {
    expect(zoneMonthKey(new Date(2026, 8, 10))).toBe("2026-09");
  });
});

// ---------------------------------------------------------------------------
// W-063 标点智断
// ---------------------------------------------------------------------------
describe("W-063 smart punct", () => {
  it("中文语境续中文 → cn；续英文 → en", () => {
    expect(punctDecision({ prevChar: "好", nextChar: "世", textBefore: "你好" })).toBe("cn");
    expect(punctDecision({ prevChar: "好", nextChar: "w", textBefore: "你好" })).toBe("en");
  });

  it("中文句末（行尾/空格）→ cn", () => {
    expect(punctDecision({ prevChar: "好", nextChar: null, textBefore: "你好" })).toBe("cn");
    expect(punctDecision({ prevChar: "好", nextChar: " ", textBefore: "你好" })).toBe("cn");
  });

  it("数字场景豁免（3.14）", () => {
    expect(punctDecision({ prevChar: "3", nextChar: "1", textBefore: "3" })).toBe("en");
    expect(punctDecision({ prevChar: "4", nextChar: null, textBefore: "3.14" })).toBe("en");
  });

  it("URL 场景豁免（内置名单）", () => {
    expect(isUrlContext("https://example.com")).toBe(true);
    expect(isUrlContext("see www.abc.com")).toBe(true);
    expect(isUrlContext("plain text.")).toBe(false);
    expect(punctDecision({ prevChar: "m", nextChar: "/", textBefore: "https://example.co" })).toBe("en");
  });

  it("前文非中文不干涉（纯英文）", () => {
    expect(punctDecision({ prevChar: "t", nextChar: " ", textBefore: "cat" })).toBe("en");
  });

  it("isCJK 与判定窗口常量", () => {
    expect(isCJK("中")).toBe(true);
    expect(isCJK("a")).toBe(false);
    expect(isCJK("")).toBe(false);
    expect(PUNCT_DECISION_MS).toBeLessThanOrEqual(30);
  });
});

// ---------------------------------------------------------------------------
// 通用工具
// ---------------------------------------------------------------------------
describe("utils", () => {
  it("clamp 边界", () => {
    expect(clamp(5, 0, 10)).toBe(5);
    expect(clamp(-1, 0, 10)).toBe(0);
    expect(clamp(11, 0, 10)).toBe(10);
  });
});
