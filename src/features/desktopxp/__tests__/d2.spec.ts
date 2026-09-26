/**
 * C 桌面体验域·后段 AI-D2（F093-F110）前端逻辑单测（核心面）。
 * 覆盖：d2store 底座 / F106 HUD / F107 IME 浮窗 / F110 OSK / F108 短语库。
 * （行数不计入功能代码目标——测试是判据的执行器，不是交付物本体。）
 */

import { describe, expect, it, beforeEach } from "vitest";
import { d2Store, D2_SECTIONS, D2_DEFAULTS, assertFrozenGuard, D2_FROZEN_ITEMS, D2_UNDO_STACK_DEPTH } from "../d2store";
import { KeyHud, lockLabel, lockIconShape, HOLD_MS, HOLD_TOLERANCE_MS, HUD_W_PX, SHOW_ANIM_MS, FADE_OUT_MS } from "../keyhud";
import { ImeFloat, imeLabel, toggleImeState, avoidanceSelfTest, defaultImeStates, FOLLOW_THROTTLE_MS, FLOAT_W_PX } from "../imefloat";
import { OnScreenKb, fullLayout, compactLayout, keyLabel, touchOk, FULL_LAYOUT_KEYS, NUMPAD_BASE_CODE } from "../osk";
import { PhraseBook, parseVars, renderVars, mergeCandidates, VAR_WHITELIST, PHRASE_CAP, ABBR_MAX_CHARS } from "../phrasebk";
import { ExperienceLog, FRUSTRATION_RULES } from "../d2telemetry";

/* ------------------------------ d2store 底座 ------------------------------ */

describe("d2store 底座", () => {
  beforeEach(() => {
    d2Store.reset();
  });

  it("分节清单 = 16 实现项（F101/F104 冻结不落配置面）", () => {
    expect(D2_SECTIONS.length).toBe(16);
    expect(assertFrozenGuard()).toBe(true);
    expect(D2_FROZEN_ITEMS).toEqual(["F101", "F104"]);
  });

  it("set/get/订阅广播逐节生效", () => {
    const seen: string[] = [];
    const unsub = d2Store.subscribe((section) => seen.push(section));
    d2Store.set("keyhud", { enabled: false });
    d2Store.set("osk", { opacity: 75 });
    expect(d2Store.getWith("keyhud", "enabled", true)).toBe(false);
    expect(d2Store.getWith("osk", "opacity", 90)).toBe(75);
    expect(seen).toContain("keyhud");
    expect(seen).toContain("osk");
    unsub();
  });

  it("undoSection 栈深 3——逐节回退", () => {
    for (let i = 1; i <= 5; i++) d2Store.set("calcx", { historyCap: i });
    expect(d2Store.getWith("calcx", "historyCap", 20)).toBe(5);
    for (let i = 0; i < D2_UNDO_STACK_DEPTH; i++) d2Store.undoSection("calcx");
    // 栈深 3：回退到底 = 第 2 次写入值（5→4→3→2）。
    expect(d2Store.getWith("calcx", "historyCap", 20)).toBe(2);
    expect(d2Store.undoSection("calcx")).toBe(false);
  });

  it("importAll 整包原子切换——无合法分节整包拒绝", () => {
    expect(() => d2Store.importAll({ junk: true })).toThrow(/合法分节/);
    d2Store.importAll({ phrasebk: { phrasePriority: false } });
    expect(d2Store.getWith("phrasebk", "phrasePriority", true)).toBe(false);
  });

  it("默认值齐全且为判据档", () => {
    for (const s of D2_SECTIONS) expect(Object.keys(D2_DEFAULTS[s] ?? {}).length).toBeGreaterThan(0);
    expect(D2_DEFAULTS.thumbeng.cacheCapMb).toBe(2048); // 2GB 判据
    expect(D2_DEFAULTS.mediainfo.hoverDelayMs).toBe(800); // 悬停 800ms
    expect(D2_DEFAULTS.calcx.historyCap).toBe(20); // 回填 20 轮
    expect(D2_DEFAULTS.clocksuite.alarmSnoozeMin).toBe(5); // 贪睡 5 分钟
    expect(D2_DEFAULTS.sticknote.cap).toBe(20); // 便签 20 张
    expect(D2_DEFAULTS.osk.opacity).toBe(90);
  });
});

/* ------------------------------ F106 HUD ------------------------------ */

describe("F106 键盘提示 HUD", () => {
  it("三键三态全对（标签与形状冗余互异）", () => {
    expect(lockLabel("caps", true)).toBe("大写锁定 开");
    expect(lockLabel("num", false)).toBe("数字锁定 关");
    expect(lockLabel("scroll", true)).toBe("滚动锁定 开");
    const shapes = [lockIconShape("caps"), lockIconShape("num"), lockIconShape("scroll")];
    expect(new Set(shapes).size).toBe(3); // 圆/方/三角互异（色弱可辨）
  });

  it("计时链：120ms 淡入 → 1s 驻留 → 300ms 淡出（±50ms 判线）", () => {
    const hud = new KeyHud();
    hud.toggle("caps", true, 0);
    hud.tick(50);
    expect(hud.state.kind).toBe("showing");
    hud.tick(SHOW_ANIM_MS);
    expect(hud.state.kind).toBe("holding");
    expect(hud.fadeoutDueAt()).toBe(SHOW_ANIM_MS + HOLD_MS);
    hud.tick(SHOW_ANIM_MS + HOLD_MS - HOLD_TOLERANCE_MS - 1);
    expect(hud.state.kind).toBe("holding");
    hud.tick(SHOW_ANIM_MS + HOLD_MS);
    expect(hud.state.kind).toBe("fading");
    hud.tick(SHOW_ANIM_MS + HOLD_MS + FADE_OUT_MS);
    expect(hud.state.kind).toBe("hidden");
    expect(hud.content).toBeNull();
  });

  it("驻留中再切重置计时", () => {
    const hud = new KeyHud();
    hud.toggle("caps", true, 0);
    hud.tick(120);
    hud.toggle("caps", false, 500); // 驻留中再切——超 200ms 窗 → 重新展示
    hud.tick(620); // 120ms 淡入完成
    expect(hud.state).toMatchObject({ kind: "holding", t0: 620 });
    expect(hud.fadeoutDueAt()).toBe(620 + HOLD_MS);
  });

  it("风暴合并：200ms 窗连发只换内容不重展（不闪屏）", () => {
    const hud = new KeyHud();
    for (let i = 0; i < 50; i++) hud.toggle("caps", i % 2 === 0, i * 20);
    expect(hud.stormMerged).toBe(49);
    expect(hud.showCount).toBe(1);
  });

  it("全屏降级角标几何 + 输入法复用 + 开关静默", () => {
    const hud = new KeyHud();
    hud.setFullscreen(true);
    const r = hud.rect(1920, 1080);
    expect(r).toEqual({ x: 1920 - 24 - 16, y: 1080 - 24 - 16, w: 24, h: 24 });
    hud.setFullscreen(false);
    expect(hud.rect(1920, 1080).w).toBe(HUD_W_PX);
    hud.imeSwitch(true, 0);
    expect(hud.content).toMatchObject({ kind: "ime", chinese: true });
    const off = new KeyHud();
    off.enabled = false;
    off.toggle("caps", true, 0);
    expect(off.content).toBeNull();
  });
});

/* ---------------------------- F107 输入法浮窗 ---------------------------- */

describe("F107 输入法状态浮窗", () => {
  it("三态 8 组合标签互异", () => {
    const labels = new Set<string>();
    for (let mask = 0; mask < 8; mask++) {
      const s = { chinese: (mask & 1) !== 0, fullwidth: (mask & 2) !== 0, cnPunct: (mask & 4) !== 0 };
      labels.add(imeLabel(s));
    }
    expect(labels.size).toBe(8);
  });

  it("16ms 节流：首移必应用（D14 修法在位），窗内移动合并", () => {
    const f = new ImeFloat();
    f.cursorMoved(10, 10, 0);
    expect(f.appliedMoves).toBe(1); // 首移旁路节流
    f.cursorMoved(11, 10, 5);
    expect(f.appliedMoves).toBe(1);
    expect(f.coalescedMoves).toBe(1);
    f.cursorMoved(20, 20, 16 + FOLLOW_THROTTLE_MS);
    expect(f.appliedMoves).toBe(2);
  });

  it("避让 20 例全对（四角/四边/象限/终极钳制）", () => {
    expect(avoidanceSelfTest(1920, 1080)).toBe(true);
    expect(avoidanceSelfTest(2560, 1440)).toBe(true);
    expect(avoidanceSelfTest(1366, 768)).toBe(true);
  });

  it("光标远出屏底仍整体在屏内（D15 终极钳制）", () => {
    const f = new ImeFloat();
    f.cursorMoved(5000, 4960, 0);
    const r = f.rect(1920, 1080);
    expect(r.x + r.w).toBeLessThanOrEqual(1920);
    expect(r.y + r.h).toBeLessThanOrEqual(1080);
  });

  it("密码框隐藏/收起/全屏三模式迁移", () => {
    const f = new ImeFloat();
    f.setPasswordFocus(true);
    expect(f.mode).toBe("hidden");
    f.setPasswordFocus(false);
    expect(f.mode).toBe("follow");
    f.collapse(true);
    expect(f.mode).toBe("taskbarChip");
    f.collapse(false);
    f.setFullscreen(true);
    expect(f.mode).toBe("cornerBadge");
    f.rect(1920, 1080); // 非跟随模式零几何
  });

  it("切换即改态 + 动画账（<16ms 判线：切换零重排）", () => {
    const f = new ImeFloat();
    const before = imeLabel(f.states);
    f.toggleState(0);
    expect(imeLabel(f.states)).not.toBe(before);
    expect(f.switchAnims).toBe(1);
    expect(defaultImeStates()).toEqual({ chinese: true, fullwidth: true, cnPunct: true });
  });

  it("toggleImeState 纯函数三路", () => {
    const s = defaultImeStates();
    expect(toggleImeState(s, 0).chinese).toBe(false);
    expect(toggleImeState(s, 1).fullwidth).toBe(false);
    expect(toggleImeState(s, 2).cnPunct).toBe(false);
    expect(FLOAT_W_PX).toBe(160);
  });
});

/* ------------------------------ F110 屏幕键盘 ------------------------------ */

describe("F110 屏幕键盘", () => {
  it("104 键等效布局：码位唯一、热区 ≥44px", () => {
    const keys = fullLayout();
    expect(keys.length).toBe(FULL_LAYOUT_KEYS);
    const codes = new Set(keys.map((k) => k.code));
    expect(codes.size).toBe(keys.length);
    expect(keys.every(touchOk)).toBe(true);
  });

  it("符号层 Shift⊕Caps：字母翻转与非字母符号层", () => {
    const keys = fullLayout();
    const q = keys.find((k) => k.base === "q")!;
    const one = keys.find((k) => k.base === "1")!;
    expect(keyLabel(q, false, false)).toBe("q");
    expect(keyLabel(q, true, false)).toBe("Q");
    expect(keyLabel(q, false, true)).toBe("Q"); // Caps 单独生效
    expect(keyLabel(q, true, true)).toBe("q"); // Shift⊕Caps 相消
    expect(keyLabel(one, true, true)).toBe("!"); // 非字母：Shift 层，Caps 不影响
  });

  it("按键上屏与单发 Shift 弹起", () => {
    const kb = new OnScreenKb();
    const keys = fullLayout();
    const shift = keys.find((k) => k.kind === "shift")!;
    const one = keys.find((k) => k.base === "1")!;
    kb.press(shift.code);
    expect(kb.shift).toBe(true);
    expect(kb.press(one.code)).toBe("!");
    expect(kb.shift).toBe(false); // 单发弹起
  });

  it("锁定键三态灯 + 小键盘 NumLock 门（码位段判定）", () => {
    const kb = new OnScreenKb();
    const keys = fullLayout();
    const numLock = keys.find((k) => k.kind === "numLock")!;
    const n5 = keys.filter((k) => k.base === "5").pop()!; // 末位 = 小键盘 5
    expect(n5.code).toBeGreaterThanOrEqual(NUMPAD_BASE_CODE);
    expect(kb.press(n5.code)).toBe("5");
    kb.press(numLock.code);
    expect(kb.numLock).toBe(false);
    expect(kb.press(n5.code)).toBe(""); // NumLock 关：小键盘数字不上屏
  });

  it("学习模式：物理键 → 码位高亮映射", () => {
    const kb = new OnScreenKb();
    expect(kb.highlightFromPhysical({ code: "KeyW", key: "w" })).toBe(fullLayout().find((k) => k.base === "w")!.code);
    expect(kb.highlightFromPhysical({ code: "Space", key: " " })).not.toBeNull();
    expect(kb.highlightFromPhysical({ code: "Numpad5", key: "5" })).not.toBeNull();
    kb.learning = false;
    expect(kb.highlightFromPhysical({ code: "KeyW", key: "w" })).toBeNull();
  });

  it("透明度可读下限 30% 诚实拒绝 + 屏边吸附", () => {
    const kb = new OnScreenKb();
    expect(kb.setOpacity(20)).toBe(false);
    expect(kb.opacity).toBe(90);
    expect(kb.setOpacity(30)).toBe(true);
    const s = OnScreenKb.snapToEdge(4, 4, 1920, 1080);
    expect(s.snapped).toBe(true);
    expect(s.x).toBe(0);
    expect(s.y).toBe(0);
  });

  it("紧凑布局 8 组 + 多点并发 held 集", () => {
    expect(compactLayout().length).toBe(8);
    const kb = new OnScreenKb();
    const keys = fullLayout();
    kb.press(keys[0]!.code);
    kb.press(keys[1]!.code);
    expect(kb.held.length).toBe(2);
    kb.release(keys[0]!.code);
    expect(kb.held).toEqual([keys[1]!.code]);
  });
});

/* ------------------------------ F108 短语库 ------------------------------ */

describe("F108 自定义短语库", () => {
  it("变量三族白名单：族名-格式符双重校验，白名单外拒绝", () => {
    expect(parseVars("今天 {date:yyyy-MM-dd}")).toEqual([{ tag: "date", fmt: "yyyy-MM-dd" }]);
    expect(parseVars("现在 {time:HH:mm}")).toEqual([{ tag: "time", fmt: "HH:mm" }]);
    expect(parseVars("周 {weekday:星期E}")).toEqual([{ tag: "weekday", fmt: "星期E" }]);
    // 族错配（date 族不认 HH:mm）与白名单外格式 → 忽略不静默展开。
    expect(parseVars("{date:HH:mm}")).toEqual([]);
    expect(parseVars("{date:yyyyMMdd}")).toEqual([]);
    expect(parseVars("{random:xyz}")).toEqual([]);
    expect(VAR_WHITELIST.length).toBe(9);
  });

  it("变量求值 20 例口径的代表样张", () => {
    expect(renderVars("今天是 {date:yyyy-MM-dd}", "2026-09-26", "星期六")).toBe("今天是 2026-09-26（yyyy-MM-dd）");
    expect(renderVars("{weekday:周E}", "x", "星期六")).toBe("星期六");
  });

  it("新建校验：边界与上限诚实拒绝", () => {
    const book = new PhraseBook();
    expect(book.add("", "内容", "默认").ok).toBe(false);
    expect(book.add("a".repeat(ABBR_MAX_CHARS + 1), "内容", "默认").ok).toBe(false);
    expect(book.add("yx", "", "默认").ok).toBe(false);
    expect(book.add("yx", "一句话", "常用").ok).toBe(true);
    // 同缩写覆盖（库内唯一）。
    expect(book.add("yx", "另一句", "常用").ok).toBe(true);
    expect(book.items.filter((p) => p.abbr === "yx").length).toBe(1);
  });

  it("上限 1000 诚实拒绝（不静默挤掉）", () => {
    const book = new PhraseBook();
    for (let i = 0; i < PHRASE_CAP; i++) book.add(`k${i}`, `内容${i}`, "批量");
    const r = book.add("overflow", "内容", "默认");
    expect(r.ok).toBe(false);
    expect(r.error).toContain("已满");
  });

  it("触发展开 + 使用计数（冷沉底 stable 排序）", () => {
    const book = new PhraseBook();
    book.add("yx", "一句话", "常用");
    book.add("rq", "{date:yyyy-MM-dd}", "日期");
    expect(book.expand("yx", "2026-09-26", "星期六")).toBe("一句话");
    expect(book.expand("nope", "x", "y")).toBeNull();
    expect(book.items[0]!.uses).toBe(1);
    book.add("cold", "冷短语", "常用");
    expect(book.ranked()[0]!.abbr).toBe("yx"); // 高频在前
  });

  it("导入导出 round-trip 无损 + 三策略", () => {
    const a = new PhraseBook();
    a.add("yx", "一句话", "常用");
    a.add("rq", "{date:yyyy-MM-dd}", "日期");
    const json = a.export();
    const b = new PhraseBook();
    const report = b.import(json, "overwrite");
    expect(report.added).toBe(2);
    expect(report.rejected).toEqual([]);
    expect(b.export()).toBe(json); // round-trip 无损
    // skip / keepBoth 策略。
    const c = new PhraseBook();
    c.add("yx", "旧句", "常用");
    const rSkip = c.import(json, "skip");
    expect(rSkip.skipped).toBe(1);
    expect(c.items[0]!.content).toBe("旧句");
    const rBoth = c.import(json, "keepBoth");
    expect(rBoth.added).toBe(2); // yx#2 与 rq
    expect(c.items.some((p) => p.abbr === "yx#2")).toBe(true);
    // 非法包诚实拒绝。
    const bad = new PhraseBook();
    expect(bad.import("not json", "overwrite").rejected.length).toBe(1);
    expect(bad.import(JSON.stringify({ items: [{ abbr: "", content: "" }] }), "overwrite").rejected.length).toBe(1);
  });

  it("候选混排：短语优先开关生效 + 徽标", () => {
    const ph = [{ abbr: "yx", content: "一句话", category: "常用", uses: 1 }];
    expect(mergeCandidates(ph, ["甲", "乙"], true)[0]).toMatchObject({ phrase: true });
    expect(mergeCandidates(ph, ["甲", "乙"], false)[0]).toMatchObject({ phrase: false });
    expect(mergeCandidates([], ["甲"], true).length).toBe(1);
  });
});

/* --------------------------- 十三章 体验日志 --------------------------- */

describe("D2 体验日志（十三/十三·补）", () => {
  function makeLog(): ExperienceLog {
    let t = 0;
    return new ExperienceLog(() => (t += 100));
  }

  it("狂点标记：同元素 1.2s 内 ≥5 次点击", () => {
    const log = makeLog();
    for (let i = 0; i < 5; i++) log.record("osk", "char-key", "click", null, "smooth");
    const fr = log.frustrations();
    expect(fr.length).toBe(1);
    expect(fr[0]!.frustration).toBe("rage-click");
  });

  it("浮层反复开关：10s 内 ≥4 次开关", () => {
    const log = makeLog();
    for (let i = 0; i < 4; i++) {
      log.record("term2", "palette", "open", 80, "smooth");
      log.record("term2", "palette", "close", null, "smooth");
    }
    const fr = log.frustrations();
    expect(fr.filter((e) => e.frustration === "overlay-flap").length).toBeGreaterThanOrEqual(1);
  });

  it("重试风暴标记 + 结论字段透传", () => {
    const log = makeLog();
    log.record("notepad", "save", "retry", null, "error");
    expect(log.frustrations()[0]!.frustration).toBe("repeat-spam");
    expect(log.frustrations()[0]!.verdict).toBe("error");
  });

  it("窗口外点击不误标 + 内存环上限逐出", () => {
    const log = makeLog();
    // 间隔超窗（每 tick +100ms → 5 次点击跨 400ms < 1.2s 窗——改用慢钟）。
    let slow = 0;
    const log2 = new ExperienceLog(() => (slow += FRUSTRATION_RULES.rageClickWindowMs));
    for (let i = 0; i < 6; i++) log2.record("osk", "char-key", "click", null, "smooth");
    expect(log2.frustrations().filter((e) => e.frustration === "rage-click").length).toBe(0);
    // 环上限：cap+100 条 → 只留最近 cap 条。
    for (let i = 0; i < FRUSTRATION_RULES.cap + 100; i++) log.record("album", "thumb", "click", null, "smooth");
    expect(log.size).toBe(FRUSTRATION_RULES.cap);
  });

  it("导出：开放格式 + 隐私结构（无内容字段）", () => {
    const log = makeLog();
    log.clear(); // 清掉其他用例经 localStorage 尾段恢复带入的事件（测试隔离）
    log.record("osk", "char-key", "key", null, "smooth");
    const pack = JSON.parse(log.export()) as { format: string; events: Array<Record<string, unknown>> };
    expect(pack.format).toBe("varix-d2-xlog");
    expect(pack.events.length).toBe(1);
    // 隐私红线：事件字段白名单——没有 text/content/value 类内容字段。
    const keys = Object.keys(pack.events[0]!);
    expect(keys.every((k) => ["seq", "t", "surface", "element", "kind", "ms", "verdict", "frustration"].includes(k))).toBe(true);
  });
});

describe("v4 EXIF 缩略直抽（thumbeng FE 移植面）", () => {
  function buildExifJpeg(): Uint8Array {
    const out: number[] = [0xff, 0xd8];
    const body: number[] = [];
    const push = (...b: number[]) => body.push(...b);
    // "Exif\0\0" + TIFF(LE)
    push(0x45, 0x78, 0x69, 0x66, 0x00, 0x00, 0x49, 0x49);
    push(0x2a, 0x00); // 42
    push(0x08, 0x00, 0x00, 0x00); // IFD0 @ 8
    push(0x02, 0x00); // 2 entries
    // 0x501B ThumbnailOffset LONG 1 = 16
    push(0x1b, 0x50, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x10, 0x00, 0x00, 0x00);
    // 0x501A ThumbnailLength LONG 1 = 8
    push(0x1a, 0x50, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, 0x08, 0x00, 0x00, 0x00);
    push(0x00, 0x00, 0x00, 0x00); // next IFD = 0
    push(0, 0, 0, 0, 0, 0, 0, 0); // 缩略数据 8 字节
    out.push(0xff, 0xe1, (body.length + 2) >> 8, (body.length + 2) & 0xff, ...body);
    out.push(0xff, 0xd9);
    return new Uint8Array(out);
  }

  it("EXIF 内嵌缩略定位与切片（零全解码）", async () => {
    const { exifThumbLocate, extractExifThumb } = await import("../thumbeng");
    const jpeg = buildExifJpeg();
    const loc = exifThumbLocate(jpeg);
    expect(loc).not.toBeNull();
    expect(loc!.length).toBe(8);
    const slice = extractExifThumb(jpeg);
    expect(slice).not.toBeNull();
    expect(slice!.length).toBe(8);
  });

  it("对抗样本零异常全拒绝", async () => {
    const { exifThumbLocate } = await import("../thumbeng");
    expect(exifThumbLocate(new Uint8Array([]))).toBeNull();
    expect(exifThumbLocate(new Uint8Array([0xff, 0xd8]))).toBeNull();
    expect(exifThumbLocate(new Uint8Array([0x89, 0x50, 0x4e, 0x47]))).toBeNull();
    expect(exifThumbLocate(new Uint8Array([0xff, 0xd8, 0xff, 0xe1, 0x00, 0x02, 0x00]))).toBeNull();
  });
});
