/**
 * NOVA-200 · S14 无障碍扩展路（AI-14）—— a11yNova 单测（W-164…W-175）。
 * 仓库惯例：node 环境只注入最小 window 事件总线（setup.ts 兜底），DOM 层全部
 * 以"事件 + 存储 + 纯函数"断言（与 privacyNova/ecoNova 同口径）。
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  A11Y_NOVA_FEATURES,
  ELDER_CONFIRMATIONS,
  ELDER_SCALE,
  GRAMMAR_RULES,
  SOUND_EVENTS,
  TERM_DICT,
  VOICE_COMMANDS,
  activateA11yNova,
  a11yNovaActive,
  a11yNovaDomain,
  annotateZh,
  auditFontStack,
  braille,
  brailleBadge,
  checkGrammar,
  confirmGate,
  currencySpoken,
  dateSpoken,
  deactivateA11yNova,
  elderPlan,
  matchVoiceCommand,
  mirrorKey,
  mirrorScheme,
  normalizeVoiceText,
  numberSpeech,
  pinyinOf,
  phoneSpoken,
  rewriteColorRecipe,
  simulateCvd,
  termLookup,
  translateSound,
  zoomPlan,
} from "../a11yNova";
import { getNovaState, resetNovaAll, setNovaOn } from "../../registry";

// ---- 最小 window 事件总线（node 环境兜底；仅本文件增强，不污染其他测试）----

type Fn = (e: { type: string; detail?: unknown }) => void;
const bus = new Map<string, Set<Fn>>();
const win = globalThis.window as unknown as Record<string, unknown> | undefined;
if (win && typeof win.addEventListener !== "function") {
  win.addEventListener = (t: string, fn: Fn) => {
    if (!bus.has(t)) bus.set(t, new Set());
    bus.get(t)!.add(fn);
  };
  win.removeEventListener = (t: string, fn: Fn) => {
    bus.get(t)?.delete(fn);
  };
  win.dispatchEvent = (e: { type: string; detail?: unknown }) => {
    for (const fn of [...(bus.get(e.type) ?? [])]) fn(e);
    return true;
  };
  win.dispatchEventId = win.dispatchEvent;
}

function on(type: string, fn: Fn): void {
  (globalThis.window as unknown as { addEventListener: (t: string, f: Fn) => void }).addEventListener(type, fn);
}

function fire(type: string, detail?: unknown): void {
  (globalThis.window as unknown as { dispatchEvent: (e: { type: string; detail?: unknown }) => boolean })
    .dispatchEvent({ type, detail });
}

function setOn(id: string, on_: boolean): void {
  setNovaOn(id, on_);
}

beforeEach(() => {
  resetNovaAll();
  localStorage.clear();
  bus.clear();
});

afterEach(() => {
  deactivateA11yNova();
});

// ---------------------------------------------------------------------------
// 清单
// ---------------------------------------------------------------------------

describe("A11Y_NOVA_FEATURES 清单", () => {
  it("12 项且编号连续 W-164…W-175", () => {
    expect(A11Y_NOVA_FEATURES).toHaveLength(12);
    expect(A11Y_NOVA_FEATURES.map((f) => Number(f.id.slice(2)))).toEqual(
      Array.from({ length: 12 }, (_, i) => 164 + i),
    );
  });

  it("每项含标题/描述与降级说明", () => {
    for (const f of A11Y_NOVA_FEATURES) {
      expect(f.titleZh).toBeTruthy();
      expect(f.titleEn).toBeTruthy();
      expect(f.descZh.length).toBeGreaterThan(10);
      expect(f.degrade).toBeTruthy();
    }
  });

  it("defaultOn 与 S0 注册表默认一致", () => {
    for (const f of A11Y_NOVA_FEATURES) {
      expect(getNovaState()[f.id]!.on).toBe(f.defaultOn);
    }
  });

  it("域描述符指向本清单", () => {
    expect(a11yNovaDomain.features).toBe(A11Y_NOVA_FEATURES);
    expect(a11yNovaDomain.id).toBe("S14");
  });
});

// ---------------------------------------------------------------------------
// W-164 声令官
// ---------------------------------------------------------------------------

describe("W-164 声令官", () => {
  it("20 条封闭语法", () => {
    expect(VOICE_COMMANDS).toHaveLength(20);
    expect(new Set(VOICE_COMMANDS.map((c) => c.id)).size).toBe(20);
  });

  it("精确命中（zh/en）", () => {
    expect(matchVoiceCommand("打开设置")?.action).toBe("system:open-settings");
    expect(matchVoiceCommand("Open Search")?.action).toBe("dock:open-search");
  });

  it("标点与空白归一化后仍命中", () => {
    expect(matchVoiceCommand("打开设置！")?.action).toBe("system:open-settings");
    expect(normalizeVoiceText("  显示  桌面。")).toBe("显示 桌面");
    expect(matchVoiceCommand("显示桌面")?.id).toBe("vc-show-desktop");
  });

  it("槽位语法带参数命中", () => {
    const m = matchVoiceCommand("音量调到50");
    expect(m?.action).toBe("sound:volume-set");
    expect(m?.slot).toBe("50");
  });

  it("槽位语法缺参数不命中（不猜）", () => {
    expect(matchVoiceCommand("音量调到")).toBeNull();
  });

  it("未命中返回 null", () => {
    expect(matchVoiceCommand("帮我写首诗")).toBeNull();
    expect(matchVoiceCommand("")).toBeNull();
  });

  it("关闭时 voice 事件不派发动作", () => {
    setOn("W-164", false);
    activateA11yNova();
    const spy = vi.fn();
    on("nova://a11y-voice-exec", spy);
    fire("nova://a11y-voice", { text: "打开设置" });
    expect(spy).not.toHaveBeenCalled();
  });

  it("开启时命中派发 voice-exec，未命中报 voice-nomatch", () => {
    setOn("W-164", true);
    activateA11yNova();
    const exec = vi.fn();
    const nomatch = vi.fn();
    on("nova://a11y-voice-exec", exec);
    on("nova://a11y-voice-nomatch", nomatch);
    fire("nova://a11y-voice", { text: "打开设置" });
    fire("nova://a11y-voice", { text: "胡言乱语" });
    expect(exec).toHaveBeenCalledTimes(1);
    expect(nomatch).toHaveBeenCalledTimes(1);
  });
});

// ---------------------------------------------------------------------------
// W-165 放大重锤
// ---------------------------------------------------------------------------

describe("W-165 放大重锤", () => {
  it("150% 计划：命中区抬升 + 列收缩 + 溢出守卫", () => {
    const p = zoomPlan(150);
    expect(p.scale).toBe(1.5);
    expect(p.hitMinPx).toBe(42);
    expect(p.maxCols).toBe(8);
    expect(p.spillGuard).toBe(true);
  });

  it("110% 不触发溢出守卫", () => {
    expect(zoomPlan(110).spillGuard).toBe(false);
  });

  it("超范围夹取", () => {
    expect(zoomPlan(50).scale).toBe(1.1);
    expect(zoomPlan(500).scale).toBe(2);
  });

  it("200% 命中区 56px", () => {
    expect(zoomPlan(200).hitMinPx).toBe(56);
  });
});

// ---------------------------------------------------------------------------
// W-166 色觉配方改写
// ---------------------------------------------------------------------------

describe("W-166 色觉配方改写", () => {
  it("CVD 模拟改变颜色且输出合法 hex", () => {
    const out = simulateCvd("#ff3040", "deuteranopia");
    expect(out).toMatch(/^#[0-9a-f]{6}$/);
    expect(out).not.toBe("#ff3040");
  });

  it("黑白经模拟仍近似黑白（守恒底线）", () => {
    expect(simulateCvd("#ffffff", "protanopia")).toBe("#ffffff");
    expect(simulateCvd("#000000", "protanopia")).toBe("#000000");
  });

  it("改写后对比度不塌方（>=4.5 或如实 UNCONSERVED）", () => {
    const tokens = { fg: "#445566", bg: "#ffffff", accent: "#8899aa", surface: "#ffffff" };
    const r = rewriteColorRecipe(tokens, [["fg", "bg"], ["accent", "surface"]], "deuteranopia");
    expect(r).not.toBeNull();
    for (const p of r!.pairs) {
      expect(p.after >= 4.5 || p.status === "unconserved").toBe(true);
    }
    expect(r!.status === "ok" || r!.status === "unconserved").toBe(true);
  });

  it("黑底白字经改写仍保持高对比", () => {
    const r = rewriteColorRecipe({ fg: "#ffffff", bg: "#101418" }, [["fg", "bg"]], "tritanopia");
    expect(r!.status).toBe("ok");
    expect(r!.pairs[0]!.after).toBeGreaterThan(7);
  });

  it("非法 token 返回 null（诚实）", () => {
    expect(rewriteColorRecipe({ fg: "#12345", bg: "#ffffff" }, [["fg", "bg"]], "protanopia")).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// W-167 听障听诊器
// ---------------------------------------------------------------------------

describe("W-167 听障听诊器", () => {
  it("30+ 声事件全量可转译", () => {
    expect(SOUND_EVENTS.length).toBeGreaterThanOrEqual(30);
    for (const s of SOUND_EVENTS) {
      expect(translateSound(s.id)?.captionZh).toBe(s.captionZh);
    }
  });

  it("三通道独立开关", () => {
    const t = translateSound("notify.msg", { flash: false, caption: true, vibr: false });
    expect(t?.channels.flash).toBe(false);
    expect(t?.channels.caption).toBe(true);
    expect(t?.channels.vibr).toBeNull();
  });

  it("未收录事件如实返回 null", () => {
    expect(translateSound("no.such.event")).toBeNull();
  });

  it("开启时 sound 事件产出振动事件并落微史", () => {
    setOn("W-167", true);
    activateA11yNova();
    const vib = vi.fn();
    on("nova://a11y-vibrate", vib);
    fire("nova://a11y-sound", { id: "notify.msg" });
    expect(vib).toHaveBeenCalledTimes(1);
    expect(vib.mock.calls[0]![0].detail.pattern.length).toBeGreaterThan(0);
    const log = JSON.parse(localStorage.getItem("nova.a11y.deaf-log") ?? "[]") as Array<{ id: string }>;
    expect(log.at(-1)?.id).toBe("notify.msg");
  });

  it("关闭时 sound 事件静默", () => {
    setOn("W-167", false);
    activateA11yNova();
    const vib = vi.fn();
    on("nova://a11y-vibrate", vib);
    fire("nova://a11y-sound", { id: "notify.msg" });
    expect(vib).not.toHaveBeenCalled();
    expect(localStorage.getItem("nova.a11y.deaf-log")).toBeNull();
  });
});

// ---------------------------------------------------------------------------
// W-168 长者大卡
// ---------------------------------------------------------------------------

describe("W-168 长者大卡", () => {
  it("1.3× / 双确认 / 36px 命中区", () => {
    expect(ELDER_SCALE).toBe(1.3);
    expect(ELDER_CONFIRMATIONS).toBe(2);
    const p = elderPlan();
    expect(p.cardScale).toBe(1.3);
    expect(p.confirmations).toBe(2);
    expect(p.hitMinPx).toBe(36);
  });

  it("双确认闸：首点武装、二点通过、过期重来", () => {
    const t0 = 1_000_000;
    expect(confirmGate({ armed: false, at: 0 }, t0).pass).toBe(false);
    expect(confirmGate({ armed: true, at: t0 }, t0 + 1000).pass).toBe(true);
    expect(confirmGate({ armed: true, at: t0 }, t0 + 5000).pass).toBe(false);
  });

  it("开启时 confirm-gate 事件不抛错且可关闭后静默", () => {
    setOn("W-168", true);
    activateA11yNova();
    expect(() => fire("nova://a11y-confirm-gate", { label: "清空回收站" })).not.toThrow();
    deactivateA11yNova();
    activateA11yNova();
    setOn("W-168", false);
    expect(() => fire("nova://a11y-confirm-gate", { label: "清空回收站" })).not.toThrow();
  });
});

// ---------------------------------------------------------------------------
// W-169 语法温柔墙
// ---------------------------------------------------------------------------

describe("W-169 语法温柔墙", () => {
  it("50+ 规则（zh ≥30，en ≥20）", () => {
    expect(GRAMMAR_RULES.length).toBeGreaterThanOrEqual(50);
    expect(GRAMMAR_RULES.filter((r) => r.lang === "zh").length).toBeGreaterThanOrEqual(30);
    expect(GRAMMAR_RULES.filter((r) => r.lang === "en").length).toBeGreaterThanOrEqual(20);
  });

  it("zh 重复字与标点建议", () => {
    const issues = checkGrammar("我的的电脑很棒呀");
    expect(issues.some((i) => i.ruleId === "zh-dedup-de")).toBe(true);
  });

  it("en 重复词与大写喊叫", () => {
    const issues = checkGrammar("the the quick BROWN FOX!!");
    expect(issues.some((i) => i.ruleId === "en-the-the")).toBe(true);
    expect(issues.some((i) => i.ruleId === "en-caps-shout")).toBe(true);
  });

  it("语言过滤", () => {
    const zhOnly = checkGrammar("the the 与 我的的", "zh");
    expect(zhOnly.every((i) => i.lang === "zh")).toBe(true);
  });

  it("建议不等于改写：只报位置与 hint，hint 不含'错误'", () => {
    const issues = checkGrammar("我的的");
    expect(issues[0]!.index).toBe(1);
    expect(issues[0]!.fix).toBe("的");
    expect(issues[0]!.hint).not.toContain("错误");
  });

  it("开启时 grammar 事件回 grammar-result", () => {
    setOn("W-169", true);
    activateA11yNova();
    const spy = vi.fn();
    on("nova://a11y-grammar-result", spy);
    fire("nova://a11y-grammar", { text: "the the" });
    expect(spy).toHaveBeenCalledTimes(1);
    expect((spy.mock.calls[0]![0].detail as { issues: unknown[] }).issues.length).toBeGreaterThan(0);
  });

  it("关闭时 grammar 事件静默", () => {
    setOn("W-169", false);
    activateA11yNova();
    const spy = vi.fn();
    on("nova://a11y-grammar-result", spy);
    fire("nova://a11y-grammar", { text: "the the" });
    expect(spy).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// W-170 数字报数
// ---------------------------------------------------------------------------

describe("W-170 数字报数", () => {
  it("电话 3-4-4 逐字分节（三五〇不读三百五十）", () => {
    expect(phoneSpoken("13035012345")).toBe("一三〇，三五〇一，二三四五");
  });

  it("日期读法", () => {
    expect(dateSpoken("2026", "9", "10")).toBe("二千零二十六年九月十日");
  });

  it("金额读法含角分与整", () => {
    expect(currencySpoken("100", null)).toBe("一百元整");
    expect(currencySpoken("100", "00")).toBe("一百元整");
    expect(currencySpoken("1025", "50")).toContain("一千零二十五元");
    expect(currencySpoken("1025", "50")).toContain("五角");
  });

  it("上下文识别三制式 + plain", () => {
    const segs = numberSpeech("联系电话13035012345，日期2026-09-10，付款¥1,025.5，共 3 台");
    const kinds = segs.map((s) => s.kind);
    expect(kinds).toContain("phone");
    expect(kinds).toContain("date");
    expect(kinds).toContain("currency");
    expect(kinds).toContain("plain");
    expect(segs.find((s) => s.kind === "currency")?.spoken).toContain("一千零二十五");
  });

  it("电话不与相邻数字混淆", () => {
    const segs = numberSpeech("订单号 5 电话13800138000");
    expect(segs.filter((s) => s.kind === "phone")).toHaveLength(1);
    expect(segs.filter((s) => s.kind === "plain").length).toBe(1);
  });

  it("开启时 number 事件产出 speak 事件", () => {
    setOn("W-170", true);
    activateA11yNova();
    const spy = vi.fn();
    on("nova://a11y-speak", spy);
    fire("nova://a11y-number", { text: "¥12.3" });
    expect(spy).toHaveBeenCalled();
    const parsed = vi.fn();
    on("nova://a11y-number-parsed", parsed);
    fire("nova://a11y-number", { text: "¥12.3" });
    expect(parsed).toHaveBeenCalledTimes(1);
  });
});

// ---------------------------------------------------------------------------
// W-171 左右手镜像
// ---------------------------------------------------------------------------

describe("W-171 左右手镜像", () => {
  it("行内中心对称：q↔p a↔l z↔m", () => {
    expect(mirrorKey("q")).toBe("P");
    expect(mirrorKey("P")).toBe("Q");
    expect(mirrorKey("a")).toBe("L");
    expect(mirrorKey("z")).toBe("M");
  });

  it("方向键左右反转", () => {
    expect(mirrorKey("ArrowLeft")).toBe("ArrowRight");
    expect(mirrorKey("ArrowRight")).toBe("ArrowLeft");
  });

  it("非字母键保持原样（诚实不猜）", () => {
    expect(mirrorKey("F5")).toBe("F5");
    expect(mirrorKey(" ")).toBe(" ");
  });

  it("方案镜像不动原方案", () => {
    const src = { copy: "C", quit: "Q", left: "ArrowLeft" };
    const out = mirrorScheme(src);
    expect(out["copy"]).toBe("B");
    expect(out["quit"]).toBe("P");
    expect(src["copy"]).toBe("C");
  });

  it("开启时 mirror-src 事件回 result 并投递 z14", () => {
    setOn("W-171", true);
    activateA11yNova();
    const spy = vi.fn();
    const z14 = vi.fn();
    on("nova://a11y-mirror-result", spy);
    on("nova://a11y-z14-propose", z14);
    fire("nova://a11y-mirror-src", { scheme: { copy: "C" } });
    expect(spy).toHaveBeenCalledTimes(1);
    expect(z14).toHaveBeenCalledTimes(1);
    const stored = JSON.parse(localStorage.getItem("nova.a11y.mirror") ?? "{}") as Record<string, string>;
    expect(stored["copy"]).toBe("B");
  });
});

// ---------------------------------------------------------------------------
// W-172 字体栈医生
// ---------------------------------------------------------------------------

describe("W-172 字体栈医生", () => {
  it("无泛型回退被点名", () => {
    const a = auditFontStack('"JetBrains Mono"');
    expect(a.issues.some((i) => i.kind === "no-generic")).toBe(true);
  });

  it("缺中文字体报 cjk-gap", () => {
    const a = auditFontStack("Arial, sans-serif");
    expect(a.issues.some((i) => i.kind === "cjk-gap")).toBe(true);
  });

  it("重复项去重诊断", () => {
    const a = auditFontStack("Consolas, Consolas, monospace");
    expect(a.issues.some((i) => i.kind === "duplicate")).toBe(true);
    expect(a.suggested.split("Consolas").length - 1).toBe(1);
  });

  it("健康栈零 issue", () => {
    const a = auditFontStack('"Segoe UI", "Microsoft YaHei", sans-serif');
    expect(a.issues).toHaveLength(0);
  });

  it("开启时 fontstack 事件回 fontstack-result", () => {
    setOn("W-172", true);
    activateA11yNova();
    const spy = vi.fn();
    on("nova://a11y-fontstack-result", spy);
    fire("nova://a11y-fontstack", { stack: "Arial" });
    expect(spy).toHaveBeenCalledTimes(1);
  });
});

// ---------------------------------------------------------------------------
// W-173 零术语词典
// ---------------------------------------------------------------------------

describe("W-173 零术语词典", () => {
  it("词典规模 ≥ 100 且白话解释非空", () => {
    expect(TERM_DICT.length).toBeGreaterThanOrEqual(100);
    for (const t of TERM_DICT) {
      expect(t.plain.length).toBeGreaterThan(4);
      expect(t.plain).not.toBe(t.term);
    }
  });

  it("长词优先命中", () => {
    const hits = termLookup("固态硬盘比硬盘快");
    expect(hits[0]!.entry.term).toBe("固态硬盘");
  });

  it("未命中返回空数组", () => {
    expect(termLookup("今天天气不错")).toHaveLength(0);
  });

  it("Ctrl+Alt+D 在开启时派发 dict-toggle", () => {
    setOn("W-173", true);
    activateA11yNova();
    const spy = vi.fn();
    on("nova://a11y-dict-toggle", spy);
    const ev = { type: "keydown", key: "d", ctrlKey: true, altKey: true };
    fire("keydown", undefined);
    (bus.get("keydown")?.values().next().value as Fn)(ev as never);
    expect(spy).toHaveBeenCalledTimes(1);
  });
});

// ---------------------------------------------------------------------------
// W-174 拼音北极星
// ---------------------------------------------------------------------------

describe("W-174 拼音北极星", () => {
  it("已收录字给出注音，未收录如实 null", () => {
    expect(pinyinOf("的")).toBe("de");
    expect(pinyinOf("魅")).toBeNull();
  });

  it("词级优先：设置 → shè zhì", () => {
    const parts = annotateZh("设置");
    expect(parts[0]).toEqual({ seg: "设置", py: "shè zhì" });
  });

  it("混排文本逐段拆分且不丢字", () => {
    const parts = annotateZh("打开W-175设置");
    const joined = parts.map((p) => p.seg).join("");
    expect(joined).toBe("打开W-175设置");
    expect(parts[0]!.py).toBe("dǎ kāi");
    expect(parts.find((p) => p.seg === "5")!.py).toBeNull();
  });

  it("单字逐字注音", () => {
    const parts = annotateZh("好的");
    expect(parts.map((p) => p.py)).toEqual(["hǎo", "de"]);
  });
});

// ---------------------------------------------------------------------------
// W-175 盲文徽章
// ---------------------------------------------------------------------------

describe("W-175 盲文徽章", () => {
  it("字母直映", () => {
    expect(braille("abc")).toBe("⠁⠃⠉");
  });

  it("数字加数符（0 用 j 形）", () => {
    expect(braille("1230")).toBe("⠼⠁⠃⠉⠚");
  });

  it("大写加前缀", () => {
    expect(braille("Ab")).toBe("⠠⠁⠃");
  });

  it("中文未收录跳过（不编造点阵）", () => {
    expect(braille("a中b")).toBe("⠁⠃");
  });

  it("双通道徽章：aria + 盲文（逐大写加前缀）", () => {
    const badge = brailleBadge("OK");
    expect(badge.aria).toBe("OK");
    expect(badge.braille).toBe("⠠⠕⠠⠅");
  });

  it("开启时 badge 事件回 badge-result", () => {
    setOn("W-175", true);
    activateA11yNova();
    const spy = vi.fn();
    on("nova://a11y-badge-result", spy);
    fire("nova://a11y-badge", { label: "ok" });
    expect(spy).toHaveBeenCalledTimes(1);
    expect((spy.mock.calls[0]![0].detail as { braille: string }).braille).toBe("⠕⠅");
  });

  it("关闭时 badge 事件静默", () => {
    setOn("W-175", false);
    activateA11yNova();
    const spy = vi.fn();
    on("nova://a11y-badge-result", spy);
    fire("nova://a11y-badge", { label: "ok" });
    expect(spy).not.toHaveBeenCalled();
  });
});

// ---------------------------------------------------------------------------
// 激活/卸载生命周期
// ---------------------------------------------------------------------------

describe("生命周期与降级", () => {
  it("幂等激活", () => {
    activateA11yNova();
    activateA11yNova();
    expect(a11yNovaActive()).toBe(true);
    deactivateA11yNova();
    expect(a11yNovaActive()).toBe(false);
  });

  it("卸载后事件不再响应", () => {
    setOn("W-167", true);
    activateA11yNova();
    deactivateA11yNova();
    const vib = vi.fn();
    on("nova://a11y-vibrate", vib);
    fire("nova://a11y-sound", { id: "notify.msg" });
    expect(vib).not.toHaveBeenCalled();
  });

  it("window 缺失时激活为空操作", () => {
    const win = globalThis.window;
    (globalThis as { window?: unknown }).window = undefined;
    try {
      expect(() => activateA11yNova()).not.toThrow();
      expect(a11yNovaActive()).toBe(false);
    } finally {
      (globalThis as { window?: unknown }).window = win;
    }
  });
});
