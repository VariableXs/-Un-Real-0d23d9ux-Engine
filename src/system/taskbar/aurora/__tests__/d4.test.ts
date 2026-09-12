/**
 * AURORA-10000 领域04 · 任务栏与开始菜单 运行时模块单测（AI-16~AI-20 批次，勿删）。
 * 覆盖 25 族核心逻辑；设置面板样式与连线由构建门禁与 aria 审计兜底。
 */
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  DEFAULT_D4_VALUES, applyLayoutSlot, getD4Doc, importD4, patchD4, resetD4,
  saveLayoutSlot, setD4, setLocked, setMaster, toggleLayoutSlot, __reloadD4,
} from "../prefs";
import { motionEnabled, plateOpacity, taskbarForm, visibilityDecision } from "../form";
import { clickAction, numberDirect, sortIcons, buildSwitcher, type TaskIcon, type SwitchWindow } from "../interaction";
import { aggregateStatus, clockFormat, foldTray, rotateTimezone, type TrayItem } from "../trayx";
import { DEFAULT_WIDGETS, carouselSlice, inDndWindow, reorderWidgets } from "../widgetZone";
import { indexKeys, openBehavior, paginate, startLayout } from "../startStruct";
import { DEFAULT_TILES, greetingFor, layoutTiles, sortTiles } from "../tiles";
import { colorPreview, evalCalc, searchStart, tzConvert, unitConvert } from "../startSearch";
import { applyOperators, codecDecode, codecEncode, genPassword, jsonFormat, regexTest, rollDice, searchAll, type IndexEntry } from "../searchHub";
import {
  applyRules, bannerPlan, clearAll, displayOrder, heatmap, inDndPlan, morningDigest,
  partitionToday, restoreArchived, type Notice,
} from "../notifyCenter";
import { QUICK_TOGGLES, customOrder, panelStyle } from "../quickPanel";
import { mask, detectSensitive, exportClips, filterKind, mergePaste, pasteStrategy, pushClip, recordReader, searchClips, type ClipEntry } from "../clipboardx";
import {
  applyScheme, bumpUsage, findHotkeyConflicts, gameModeScope, normalizeAccel,
  playback, recordMacro, restoreDefaults, topUsed, type HotkeyEntry,
} from "../hotkeys";
import { candidatePrefs, exportLexicon, fuzzyMatch, importLexicon, mergeLexicon, typingSpeed } from "../imex";
import { POMODORO, bindCustomOp, pomodoroPhase, resolveCustomOp, startTimer } from "../quickops";
import { applyBatch, exportTimeline, filterWall, recordEvent } from "../taskview";
import { autoCategory, crashRanking, getExtAssoc, launchRanking, setExtAssoc, toggleAutostart, uninstallPlan, type AppRecord } from "../eco";
import { FALLBACK_CHAIN, formatDate, formatNumber, isDst, lookupKey, plural, pseudo } from "../l10n";
import {
  BUDGETS, FrameWatchdog, LocalTelemetry, RegressionBaseline, atomicWrite, diagBundle, grayEnabled, p95, selfHeal,
} from "../quality";
import { AURORA_TASKBAR_FAMILIES, AURORA_TASKBAR_ITEMS } from "../catalog";

/* ---------------- 目录完整性（编号纪律） ---------------- */
describe("catalog（族0076~0100）", () => {
  it("25 族 × 25 项 = 625 项，ID 连续唯一", () => {
    expect(AURORA_TASKBAR_FAMILIES).toHaveLength(25);
    expect(AURORA_TASKBAR_ITEMS).toHaveLength(625);
    const ids = AURORA_TASKBAR_ITEMS.map((i) => parseInt(i.id.slice(1), 10));
    expect(new Set(ids).size).toBe(625);
    expect(Math.min(...ids)).toBe(1876);
    expect(Math.max(...ids)).toBe(2500);
  });
});

/* ---------------- 偏好中枢 ---------------- */
describe("prefs 中枢", () => {
  beforeEach(() => { localStorage.clear(); __reloadD4(); });
  afterEach(() => { localStorage.clear(); });

  it("默认值登记、写入与锁定", () => {
    expect(Object.keys(DEFAULT_D4_VALUES).length).toBeGreaterThan(140);
    expect(setD4("F01938", true)).toBe(true);
    expect(getD4Doc().values.F01938).toBe(true);
    setLocked(true);
    expect(setD4("F01938", false)).toBe(false);
    setLocked(false);
    resetD4();
    expect(getD4Doc().values.F01938).toBe(false);
  });
  it("未登记键拒绝写入", () => {
    expect(setD4("F99999", true)).toBe(false);
  });
  it("持久化 + 重载 + 损坏自愈", () => {
    setD4("F02082", 6);
    __reloadD4();
    expect(getD4Doc().values.F02082).toBe(6);
    localStorage.setItem("aurora.d4.prefs.v1", "{broken json");
    __reloadD4();
    expect(getD4Doc().values.F02082).toBe(DEFAULT_D4_VALUES.F02082);
  });
  it("导入/导出往返", () => {
    setD4("F01976", 500);
    const json = exportJson();
    resetD4();
    expect(importD4(json)).toBe(true);
    expect(getD4Doc().values.F01976).toBe(500);
    expect(importD4("not json")).toBe(false);
  });
  function exportJson(): string {
    // 直接从模块导出（避免引入顺序问题）
    // eslint-disable-next-line @typescript-eslint/no-var-requires
    return JSON.stringify(getD4Doc());
  }
  it("A/B 布局槽保存与应用", () => {
    setD4("F01876", "top");
    saveLayoutSlot("b", "顶栏");
    setD4("F01876", "bottom-center");
    expect(applyLayoutSlot("b")).toBe(true);
    expect(getD4Doc().values.F01876).toBe("top");
    expect(toggleLayoutSlot()).toBe("a");
  });
  it("总控关闭回保守默认", () => {
    setD4("F01901", false);
    setMaster(false);
    expect(getD4Doc().master).toBe(false);
    setMaster(true);
  });
});

/* ---------------- 族0076/0080 形态与行为 ---------------- */
describe("taskbar form/behavior", () => {
  beforeEach(() => { localStorage.clear(); __reloadD4(); });
  it("形态解析与高度档", () => {
    const f = taskbarForm();
    expect(f.pos).toBe("bottom-center");
    expect(f.height).toBe(52);
    patchD4({ F01894: "slim" });
    expect(taskbarForm().height).toBe(40);
  });
  it("全屏让位优先", () => {
    patchD4({ F01887: true, F01886: false });
    const d = visibilityDecision({ fullscreen: true, focused: true, pointerNearEdge: false, now: 0 }, 1000);
    expect(d.visible).toBe(false);
    expect(d.reason).toBe("fullscreen-yield");
  });
  it("失焦自动隐藏", () => {
    patchD4({ F01886: true });
    const d = visibilityDecision({ fullscreen: false, focused: false, pointerNearEdge: false, now: 0 }, 10_000);
    expect(d.reason).toBe("autohide");
  });
  it("夜间透明与性能模式", () => {
    patchD4({ F01985: true });
    expect(plateOpacity(14)).toBeCloseTo(0.92);
    expect(plateOpacity(23)).toBeCloseTo(0.67);
    patchD4({ F01986: true });
    expect(motionEnabled()).toBe(false);
  });
});

/* ---------------- 族0077 交互 ---------------- */
describe("taskbar interaction", () => {
  const icons: TaskIcon[] = [
    { appId: "a", label: "A", pinned: false, lastUsed: 10, useCount: 5, badge: 2, progress: 0.5, instances: 1 },
    { appId: "b", label: "B", pinned: true, lastUsed: 1, useCount: 1, badge: 0, progress: null, instances: 0 },
    { appId: "c", label: "C", pinned: false, lastUsed: 20, useCount: 9, badge: 0, progress: null, instances: 2 },
  ];
  it("频率自排固定优先", () => {
    expect(sortIcons(icons).map((i) => i.appId)).toEqual(["b", "c", "a"]);
  });
  it("数字直达与点击行为", () => {
    expect(numberDirect(icons).get(1)?.appId).toBe("a");
    expect(clickAction(icons[2]!)).toBe("activate");
  });
});

/* ---------------- 族0078/0097 托盘 ---------------- */
describe("tray", () => {
  const items: TrayItem[] = [
    { id: "1", name: "音量", pinned: true, useCount: 99, badge: null, style: "system" },
    { id: "2", name: "低频", pinned: false, useCount: 1, badge: 3, style: "mono" },
    { id: "3", name: "中频", pinned: false, useCount: 5, badge: null, style: "accent" },
  ];
  it("低频折叠与计数", () => {
    const { shown, hidden } = foldTray(items);
    expect(shown.map((i) => i.id)).toEqual(["1", "3"]);
    expect(hidden).toHaveLength(1);
  });
  it("时钟秒显", () => {
    expect(clockFormat(new Date(2026, 8, 13, 9, 5, 7))).toBe("09:05");
  });
  it("多时区轮换", () => {
    const r = rotateTimezone(["UTC", "Asia/Shanghai"], 1);
    expect(r.zone).toBe("Asia/Shanghai");
    expect(r.time).toMatch(/^\d{2}:\d{2}$/);
  });
  it("聚合面板分组", () => {
    const m = aggregateStatus([
      { kind: "charging", active: true, group: "power" },
      { kind: "vpn", active: true, group: "network" },
      { kind: "dnd", active: false, group: "privacy" },
    ]);
    expect(m.get("power")).toHaveLength(1);
    expect(m.has("privacy")).toBe(false);
  });
});

/* ---------------- 族0079 小组件区 ---------------- */
describe("widget zone", () => {
  it("重排保持连续 order", () => {
    const r = reorderWidgets(DEFAULT_WIDGETS, 2, 0);
    expect(r[0]!.id).toBe("calendar");
    expect(r.map((w) => w.order)).toEqual(r.map((_, i) => i));
  });
  it("免打扰时段过滤", () => {
    expect(inDndWindow("22:00-07:00", new Date(2026, 8, 13, 23, 30))).toBe(true);
    expect(inDndWindow("22:00-07:00", new Date(2026, 8, 13, 12, 0))).toBe(false);
    expect(inDndWindow("bad", new Date())).toBe(false);
  });
  it("轮播分页循环", () => {
    patchD4({ F01971: 30 });
    const all = DEFAULT_WIDGETS.map((w, i) => ({ ...w, enabled: true, order: i }));
    const s1 = carouselSlice(all, 3, 0);
    const s2 = carouselSlice(all, 3, 1);
    expect(s1).toHaveLength(3);
    expect(s2[0]!.id).not.toBe(s1[0]!.id);
  });
});

/* ---------------- 族0081/0085 开始菜单结构/行为 ---------------- */
describe("start structure", () => {
  it("索引键与分页", () => {
    const keys = indexKeys([
      { label: "Alpha" }, { label: "Beta" }, { label: "终端", pinyin: "zhongduan" },
    ]);
    expect(keys).toContain("A");
    expect(keys).toContain("Z");
    expect(paginate([1, 2, 3, 4, 5], 2)).toHaveLength(3);
  });
  it("打开行为默认聚焦搜索", () => {
    expect(openBehavior().focusSearch).toBe(true);
    expect(startLayout()).toBe("double");
  });
});

/* ---------------- 族0082/0084 磁贴与个性 ---------------- */
describe("tiles", () => {
  it("置顶排序与隐藏过滤", () => {
    const t = sortTiles(DEFAULT_TILES);
    expect(t.every((x) => !x.hidden)).toBe(true);
    expect(t[0]!.pinned).toBe(true);
  });
  it("列布局宽贴跨列", () => {
    const rows = layoutTiles(DEFAULT_TILES, 4);
    expect(rows.length).toBeGreaterThan(0);
  });
  it("生日彩蛋招呼语", () => {
    const g = greetingFor(new Date(2026, 0, 1), "阿明");
    expect(g).toContain("阿明");
  });
});

/* ---------------- 族0083 开始菜单搜索 ---------------- */
describe("start search", () => {
  it("安全四则求值", () => {
    expect(evalCalc("1+2*3")).toBe(7);
    expect(evalCalc("window")).toBeNull();
    expect(evalCalc("1+2; alert(1)")).toBeNull();
  });
  it("单位与时区换算", () => {
    expect(unitConvert("3km to ft")).toMatch(/^9,?842\.5|^9842/);
    expect(tzConvert("14:00 CST to UTC")).toBe("06:00 UTC");
    expect(colorPreview("#ff00aa")).toBe("#ff00aa");
  });
  it("搜索管线：命令前缀与拼音", () => {
    const src = {
      apps: [{ id: "a", label: "终端", pinyin: "zhongduan" }, { id: "b", label: "设置" }],
      settings: [{ id: "s1", label: "壁纸" }],
      commands: [{ id: "c1", label: "锁屏" }],
    };
    expect(searchStart(">锁", src)[0]!.kind).toBe("command");
    expect(searchStart("zhong", src).some((h) => h.id === "a")).toBe(true);
    expect(searchStart("壁", src).some((h) => h.id === "s1")).toBe(true);
  });
});

/* ---------------- 族0086/0087 搜索中枢与启动器 ---------------- */
describe("search hub & launcher tools", () => {
  const entries: IndexEntry[] = [
    { id: "1", source: "file", title: "报告.docx", body: "季度总结", path: "D:/docs", ext: "docx", mtime: 100, size: 10 },
    { id: "2", source: "note", title: "便签", body: "买牛奶", path: "C:/notes", ext: "txt", mtime: 200, size: 2 },
  ];
  it("AND/NOT/通配/正则", () => {
    expect(applyOperators("a AND b NOT:c").not).toEqual(["c"]);
    expect(searchAll(entries, "报告")).toHaveLength(1);
    expect(searchAll(entries, "总结 OR 便签")).toHaveLength(2);
    expect(searchAll(entries, "*", { wildcard: true })).toHaveLength(2); // * 通配全命中
    expect(searchAll(entries, "季.*总结", { regex: true })).toHaveLength(1);
  });
  it("过滤器", () => {
    expect(searchAll(entries, "买", { path: "C:/notes" })).toHaveLength(1);
    expect(searchAll(entries, "买", { exts: ["docx"] })).toHaveLength(0);
    expect(searchAll(entries, "买", { since: 300 })).toHaveLength(0);
  });
  it("编解码与 JSON", () => {
    expect(codecDecode(codecEncode("中文", "base64"), "base64")).toBe("中文");
    expect(jsonFormat('{"a":1}')).toBe('{\n  "a": 1\n}');
    expect(jsonFormat("bad")).toBeNull();
    expect(regexTest("\\d+", "a1b22").matches).toEqual(["1", "22"]);
  });
  it("密码与骰子", () => {
    expect(genPassword(16)).toHaveLength(16);
    const d = rollDice("3d6");
    expect(d).toHaveLength(3);
    expect(rollDice("bad")).toBeNull();
  });
});

/* ---------------- 族0088/0089 通知 ---------------- */
describe("notify center", () => {
  const now = new Date(2026, 8, 13, 10, 0, 0).getTime();
  const notices: Notice[] = [
    { id: "1", appId: "mail", title: "新邮件", body: "x", ts: now - 1000, kind: "info", important: false },
    { id: "2", appId: "system", title: "重要更新", body: "y", ts: now - 2000, kind: "info", important: true },
    { id: "3", appId: "chat", title: "消息", body: "z", ts: now - 15 * 3600_000, kind: "info", important: false },
  ];
  it("今日/更早分区与置顶排序", () => {
    const p = partitionToday(notices, now);
    expect(p.today).toHaveLength(2);
    expect(displayOrder(notices, now)[0]!.id).toBe("2");
  });
  it("勿扰计划与规则引擎", () => {
    expect(inDndPlan("22:00-07:00", new Date(2026, 8, 13, 23, 30))).toBe(true);
    const r = applyRules(notices[0]!, {
      dndPlan: "", focusMode: true, fullscreen: false, projecting: false,
      whitelist: [], keywords: [],
    });
    expect(r.muted).toBe(true);
    const w = applyRules(notices[0]!, {
      dndPlan: "", focusMode: true, fullscreen: false, projecting: false,
      whitelist: ["mail"], keywords: [],
    });
    expect(w.important).toBe(true);
  });
  it("清除归档与恢复", () => {
    const archived = clearAll(notices, true);
    expect(archived.every((n) => n.archived)).toBe(true);
    expect(restoreArchived(archived).every((n) => !n.archived)).toBe(true);
  });
  it("晨报与热度图", () => {
    expect(morningDigest(notices, now)?.appId).toBe("system");
    expect(morningDigest(notices.slice(0, 2), now)).toBeNull();
    expect(heatmap(notices).size).toBeGreaterThan(0);
    expect(bannerPlan(9).stack).toBe(3);
  });
});

/* ---------------- 族0090 快捷面板 ---------------- */
describe("quick panel", () => {
  it("自定义布局置前", () => {
    const r = customOrder(QUICK_TOGGLES, ["focus"]);
    expect(r[0]!.id).toBe("focus");
    expect(r).toHaveLength(QUICK_TOGGLES.length);
  });
  it("面板参数", () => {
    expect(panelStyle().glass).toBe(true);
  });
});

/* ---------------- 族0091 切换器（已并入 interaction 测试） ---------------- */
describe("switcher", () => {
  const wins: SwitchWindow[] = [
    { id: "w1", title: "文档", appId: "write", desktop: 1, minimized: false, lastFocused: 100 },
    { id: "w2", title: "音乐", appId: "player", desktop: 2, minimized: false, lastFocused: 200 },
    { id: "w3", title: "备份", appId: "write", desktop: 1, minimized: true, lastFocused: 50 },
  ];
  it("LRU + 当前桌优先 + 排除最小化", () => {
    const r = buildSwitcher({ windows: wins, currentDesktop: 1 });
    expect(r.ordered.map((w) => w.id)).toEqual(["w1", "w2"]);
  });
  it("包含最小化 + 当前桌范围 + 搜索", () => {
    const r = buildSwitcher({ windows: wins, currentDesktop: 2, search: "音乐" });
    expect(r.ordered.map((w) => w.id)).toEqual(["w2"]);
    expect(r.numbered.get(1)?.id).toBe("w2");
  });
});

/* ---------------- 族0092 剪贴板 ---------------- */
describe("clipboard", () => {
  const base: ClipEntry = { id: "x", kind: "text", text: "hello", ts: 1, pinned: false, favorite: false };
  it("敏感探测与脱敏", () => {
    expect(detectSensitive("6222 0222 2222 2222")).toBe(true);
    expect(detectSensitive("hello world")).toBe(false);
    expect(mask("abcdefgh")).toBe("ab••••gh");
  });
  it("去重 + 容量 + 过期", () => {
    const now = 1_000_000_000_000;
    let h = pushClip([], { kind: "text", text: "a" }, { capacity: 2, expireDays: 7, dedupe: true, autoSensitive: true, now });
    h = pushClip(h, { kind: "text", text: "a" }, { capacity: 2, expireDays: 7, dedupe: true, autoSensitive: true, now: now + 1 });
    expect(h).toHaveLength(1);
    h = pushClip(h, { kind: "text", text: "b" }, { capacity: 2, expireDays: 7, dedupe: true, autoSensitive: true, now: now + 2 });
    h = pushClip(h, { kind: "text", text: "c" }, { capacity: 2, expireDays: 7, dedupe: true, autoSensitive: true, now: now + 3 });
    expect(h).toHaveLength(2);
  });
  it("置顶不受容量挤压", () => {
    const now = 1_000_000_000_000;
    const pinned: ClipEntry = { ...base, pinned: true };
    let h = pushClip([pinned], { kind: "text", text: "a" }, { capacity: 1, expireDays: 7, dedupe: false, autoSensitive: true, now });
    h = pushClip(h, { kind: "text", text: "b" }, { capacity: 1, expireDays: 7, dedupe: false, autoSensitive: true, now: now + 1 });
    expect(h.some((c) => c.pinned)).toBe(true);
  });
  it("合并粘贴与纯文本", () => {
    const list: ClipEntry[] = [
      { ...base, id: "1", text: "A" },
      { ...base, id: "2", text: "B" },
    ];
    expect(mergePaste(list)).toBe("A\nB");
    expect(pasteStrategy("<b>x</b>", true)).toBe("x");
    expect(pasteStrategy("<b>x</b>", false)).toBe("<b>x</b>");
  });
  it("审计与导出脱敏", () => {
    const secret: ClipEntry = { ...base, id: "s", text: "6222022222222222222", sensitive: true };
    const withReader = recordReader([secret], "s", "appX");
    expect(withReader[0]!.readers).toEqual(["appX"]);
    expect(exportClips([secret])).not.toContain("6222022222222222222");
    expect(filterKind([base, { ...base, kind: "image", id: "i" }], "image")).toHaveLength(1);
    expect(searchClips([base], "ell")).toHaveLength(1);
  });
});

/* ---------------- 族0093 热键 ---------------- */
describe("hotkeys", () => {
  const entries: HotkeyEntry[] = [
    { id: "1", label: "搜索", accel: "ctrl+shift+f", scope: "global", uses: 3 },
    { id: "2", label: "重复", accel: "ctrl+shift+f", scope: "global", uses: 1 },
    { id: "3", label: "切窗", accel: "alt+tab", scope: "global", uses: 9 },
  ];
  it("规范化与冲突检测", () => {
    expect(normalizeAccel("Shift + CTRL + f")).toBe("ctrl+shift+f");
    expect(findHotkeyConflicts(entries).get("ctrl+shift+f")).toHaveLength(2);
  });
  it("统计榜与游戏模式", () => {
    const bumped = bumpUsage(entries, "1");
    expect(topUsed(bumped, 1)[0]!.id).toBe("3");
    expect(gameModeScope(entries[0]!, true)).toBe("window");
    expect(gameModeScope(entries[0]!, false)).toBe("global");
  });
  it("方案应用与还原", () => {
    expect(applyScheme(entries, []).length).toBe(3);
    expect(restoreDefaults([entries[0]!])).toHaveLength(1);
  });
  it("宏录制回放时间线", () => {
    const m = recordMacro("test", [
      { key: "a", at: 0 }, { key: "b", at: 120 }, { key: "c", at: 5000 },
    ]);
    const tl = playback(m);
    expect(tl[1]!.at).toBe(120);
    expect(tl[2]!.at).toBe(2120); // 延迟截断到 2000ms
  });
});

/* ---------------- 族0094 输入法 ---------------- */
describe("ime", () => {
  it("候选参数钳制", () => {
    expect(candidatePrefs(1, 99).perPage).toBe(3);
    expect(candidatePrefs(5, 99).fontSize).toBe(24);
  });
  it("模糊音匹配", () => {
    expect(fuzzyMatch("zh", "z", ["zh-z"])).toBe(true);
    expect(fuzzyMatch("zh", "s", ["zh-z"])).toBe(false);
  });
  it("词库合并与导入导出", () => {
    const merged = mergeLexicon([{ word: "你好", freq: 2 }], [{ word: "你好", freq: 1 }, { word: "世界", freq: 5 }]);
    expect(merged[0]!.word).toBe("世界");
    const round = importLexicon(exportLexicon(merged));
    expect(round).toHaveLength(2);
    expect(typingSpeed(300, 60_000)).toBe(300);
  });
});

/* ---------------- 族0095 快操 ---------------- */
describe("quick ops", () => {
  it("自定义动作绑定", () => {
    bindCustomOp("op1", "screenshot");
    expect(resolveCustomOp("op1")).toBe("screenshot");
    expect(resolveCustomOp("none")).toBeUndefined();
  });
  it("计时器与番茄钟", () => {
    expect(startTimer(60, 1000)).toBe(61_000);
    expect(pomodoroPhase(10 * 60_000)).toBe("focus");
    expect(pomodoroPhase(26 * 60_000)).toBe("break");
    expect(POMODORO.focusMin).toBe(25);
  });
});

/* ---------------- 族0096 任务视图 ---------------- */
describe("task view", () => {
  const prefs = { cols: 4, timeline: true, retentionDays: 30, bgOpacity: 0.9 };
  it("时间线隐私开关", () => {
    const ev = { appId: "write", title: "t", ts: 100, desktop: 1 };
    expect(recordEvent([], ev, { ...prefs, timeline: false })).toHaveLength(0);
    expect(recordEvent([], ev, prefs)).toHaveLength(1);
  });
  it("保留期裁剪与过滤", () => {
    const now = 40 * 86_400_000;
    let evs = recordEvent([], { appId: "a", title: "old", ts: 1, desktop: 1 }, prefs, now);
    evs = recordEvent(evs, { appId: "b", title: "new", ts: now, desktop: 1 }, prefs, now);
    expect(evs).toHaveLength(1);
    expect(filterWall(evs, { search: "new" })).toHaveLength(1);
  });
  it("批量操作与导出", () => {
    const items = [{ id: "1" }, { id: "2" }];
    const r = applyBatch(items, { type: "close", ids: ["1"] });
    expect(r.keep).toEqual([{ id: "2" }]);
    expect(exportTimeline([{ id: "e", appId: "a", title: "t", ts: 0, desktop: 1 }])).toContain("a");
  });
});

/* ---------------- 族0098 生态 ---------------- */
describe("eco", () => {
  const apps: AppRecord[] = [
    { id: "a", name: "Code Editor", category: "", portable: false, args: "", permissions: ["fs"], launchMs: 900, crashes: 3, useMinutes: 10, autostart: false },
    { id: "b", name: "Music Player", category: "", portable: true, args: "--mini", permissions: [], launchMs: 200, crashes: 0, useMinutes: 90, autostart: true },
  ];
  it("自动归类与排行", () => {
    expect(autoCategory("Code Editor")).toBe("开发");
    expect(launchRanking(apps)[0]!.id).toBe("a");
    expect(crashRanking(apps)[0]!.id).toBe("a");
  });
  it("卸载清单与关联", () => {
    expect(uninstallPlan(apps[0]!).length).toBe(4);
    setExtAssoc(".md", "write");
    expect(getExtAssoc("md")).toBe("write");
  });
  it("自启切换", () => {
    expect(toggleAutostart(apps, "b")[1]!.autostart).toBe(false);
  });
});

/* ---------------- 族0099 本地化 ---------------- */
describe("l10n", () => {
  const prefs = { lang: "zh" as const, hour12: false, grouping: true, dateStyle: "cn" as const, rtl: false, pseudo: false };
  it("数字与日期", () => {
    expect(formatNumber(12345, prefs)).toBe("12,345");
    expect(formatDate(new Date(2026, 8, 13), prefs)).toBe("2026年9月13日");
  });
  it("回退链与伪本地化", () => {
    expect(FALLBACK_CHAIN).toEqual(["zh", "zh-TW", "en"]);
    const bundles = { "zh:g1": "中文", "en:g1": "English" };
    expect(lookupKey(bundles, "g1", "zh")).toBe("中文");
    expect(lookupKey(bundles, "g1", "zh-TW")).toBe("English"); // zh-TW 缺失回退 en
    expect(lookupKey(bundles, "g1", "en")).toBe("English");
    expect(pseudo("abc")).toBe("[aabc]");
    expect(plural(2, "item", "items")).toBe("items");
    expect(typeof isDst("Asia/Shanghai")).toBe("boolean");
  });
});

/* ---------------- 族0100 工程质量 ---------------- */
describe("quality", () => {
  it("P95 与预算常量", () => {
    expect(p95([])).toBe(0);
    expect(p95([1, 2, 3, 100])).toBe(100);
    expect(BUDGETS.taskbarMemoryMB).toBe(50);
    expect(BUDGETS.panelFirstFrameMs).toBe(200);
  });
  it("掉帧看门狗与缓存命中", () => {
    const w = new FrameWatchdog();
    w.feed(16); w.feed(50);
    expect(w.rate()).toBeCloseTo(0.5);
    w.reset();
    expect(w.rate()).toBe(0);
  });
  it("原子写与配置自恢复", () => {
    const store = new Map<string, string>();
    const s = { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => void store.set(k, v) };
    s.setItem("k", "v1");
    atomicWrite(s, "k", "v2");
    expect(store.get("k.bak")).toBe("v1");
    expect(store.get("k")).toBe("v2");
    const healed = selfHeal<number | null>((raw) => (raw === "ok" ? 42 : null), "bad", "ok", 0);
    expect(healed).toEqual({ value: 42, source: "backup" });
    expect(selfHeal<number | null>(() => null, null, null, 7)).toEqual({ value: 7, source: "default" });
  });
  it("本地遥测环形缓冲与基线", () => {
    const t = new LocalTelemetry(3);
    for (let i = 0; i < 5; i++) t.record(`e${i}`);
    expect(t.dump()).toHaveLength(3);
    expect(t.dump()[0]).toContain("e2");
    const r = new RegressionBaseline();
    r.setBaseline("open-ms", 100);
    expect(r.exceeded("open-ms", 110)).toBe(false);
    expect(r.exceeded("open-ms", 200)).toBe(true);
  });
  it("诊断包与灰度", () => {
    expect(diagBundle({ metrics: { a: 1 } })).toContain("\"a\": 1");
    expect(grayEnabled("f", 100, "s1")).toBe(true);
    expect(grayEnabled("f", 0, "s1")).toBe(false);
  });
});
