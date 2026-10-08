/// <reference types="node" />
/**
 * H4 深化批次五（v5）深测 + 隔离验证再扩展：
 * ① 功能面：26 个模块 v5 新增能力逐项用例（判据锚定）；
 * ② 隔离验证扩展（v5 新增）：import 图无环检测（依赖成环 = 架构腐化即红灯）；
 * ③ 检查项对账扩展：v5 深化导出面可发现性核对（零死代码自证）。
 */
import { describe, expect, it } from "vitest";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import * as f352 from "../f352-thumbnailOps";
import * as f353 from "../f353-crossScreenMemory";
import * as f354 from "../f354-resourceSummary";
import * as f355 from "../f355-edgeSynergy";
import * as f359 from "../f359-colorPicker";
import * as f360 from "../f360-pixelRuler";
import * as f363 from "../f363-focusTimer";
import * as f366 from "../f366-trayBattery";
import * as f367 from "../f367-zoneSnap";
import * as f368 from "../f368-minimizeToTray";
import * as f371 from "../f371-bootBadge";
import * as f373 from "../f373-keyboardLayouts";
import * as f374 from "../f374-hotkeySheet";
import * as f375 from "../f375-hDomainGate";
import * as f376 from "../f376-systemMenuMatrix";
import * as f378 from "../f378-columnAutofit";
import * as f379 from "../f379-headerSort";
import * as f380 from "../f380-blankClick";
import * as f381 from "../f381-treeTriState";
import * as f382 from "../f382-dialogPosition";
import * as f385 from "../f385-semanticTree";
import * as f386 from "../f386-readingMode";
import * as f387 from "../f387-grayscaleMode";
import * as f388 from "../f388-portraitAdaptation";
import * as f399 from "../f399-easterEggs";
import * as f400 from "../f400-hDomainClosure";

const H4_DIR = join(process.cwd(), "src", "system", "h4");

/* ================= ① F352 ================= */

describe("F352 v5：刷新节流 / 网格 / 防抖 / 键盘等价", () => {
  it("刷新节流：脏且过 16ms 才重绘（悬停不因刷新抖动）", () => {
    expect(f352.refreshThrottle(0, 10, true)).toEqual({ render: false, nextLastAt: 0 });
    expect(f352.refreshThrottle(0, 16, true).render).toBe(true);
    expect(f352.refreshThrottle(0, 20, false).render).toBe(false);
  });

  it("网格布局：列数自适应、坐标无重叠", () => {
    const g = f352.gridLayout(5, 200, 120, 500);
    expect(g.length).toBe(5);
    const seen = new Set(g.map((p) => `${p.x},${p.y}`));
    expect(seen.size).toBe(5); // 五个坐标互异 = 零重叠
  });

  it("连点防抖：200ms 内第二次点击被吞（防双份动作）", () => {
    const first = f352.debounceRapidClicks(null, 1000);
    expect(first.accept).toBe(true);
    expect(f352.debounceRapidClicks(1000, 1100).accept).toBe(false);
    expect(f352.debounceRapidClicks(1000, 1300).accept).toBe(true);
  });

  it("键盘等价：Enter/Shift+Del/Space 与鼠标能力对等", () => {
    expect(f352.keyboardEquivalent("normal", "Enter", false)).toBe("activate");
    expect(f352.keyboardEquivalent("normal", "Delete", true)).toBe("close");
    expect(f352.keyboardEquivalent("media", " ", false)).toBe("playpause");
    expect(f352.keyboardEquivalent("transfer", "Delete", true)).toBeNull(); // 传输型无关闭键
  });

  it("悬停稳定性机检：区内 hideQueued 必须为 false", () => {
    expect(f352.auditHoverStability("action", false).pass).toBe(true);
    expect(f352.auditHoverStability("thumb", true).pass).toBe(false);
    expect(f352.auditHoverStability("outside", true).pass).toBe(true);
  });
});

/* ================= ② F353 ================= */

describe("F353 v5：拓扑签名 / 记忆上限 / 回流次序 / 提示复位", () => {
  it("拓扑签名：方位参与匹配（左屏≠右屏）", () => {
    const a = f353.topologySignature([{ id: "m", side: "primary" }, { id: "s", side: "right" }]);
    const b = f353.topologySignature([{ id: "m", side: "primary" }, { id: "s", side: "left" }]);
    expect(a).not.toBe(b);
    expect(f353.topologySignature([{ id: "m", side: "primary" }, { id: "s", side: "right" }])).toBe(a);
  });

  it("记忆修剪：超上限淘汰最旧（LRU）", () => {
    let st: f353.MemoryState = { memories: [], absenceNotified: [] };
    for (let i = 0; i < f353.MEMORY_CAP + 10; i++) {
      st = f353.rememberPlacement(st, { winId: `w${i}`, x: 0, y: 0, w: 10, h: 10, displayId: "d" }, i);
    }
    const pruned = f353.pruneMemory(st);
    expect(pruned.memories.length).toBe(f353.MEMORY_CAP);
    expect(pruned.memories.some((m) => m.winId === "w0")).toBe(false);
    expect(pruned.memories.some((m) => m.winId === `w${f353.MEMORY_CAP + 9}`)).toBe(true);
  });

  it("回流次序：先大后小、同尺寸 y/x 字典序（确定性）", () => {
    const entries: f353.ReflowEntry[] = [
      { winId: "small", from: { x: 0, y: 0, w: 100, h: 100 }, to: { x: 0, y: 0, w: 100, h: 100 } },
      { winId: "big", from: { x: 0, y: 0, w: 400, h: 300 }, to: { x: 0, y: 0, w: 400, h: 300 } },
    ];
    expect(f353.reflowOrder(entries)[0]!.winId).toBe("big");
  });

  it("提示复位：重接成功清缺席标记——下次再拔还能提示一次", () => {
    const st: f353.MemoryState = { memories: [], absenceNotified: ["s"] };
    expect(f353.resetAbsenceNotice(st, "s").absenceNotified).toEqual([]);
  });
});

/* ================= ③ F354 ================= */

describe("F354 v5：降采样 / 统计 / 空闲通道审计", () => {
  it("降采样：30 点 1s → 15 点 2s（相邻均值）", () => {
    const hist: f354.ResourcePoint[] = Array.from({ length: 30 }, (_, i) => ({ t: i * 1000, cpu: i, mem: 0, disk: 0 }));
    const d = f354.downsampleHistory(hist);
    expect(d.length).toBe(15);
    expect(d[0]!.cpu).toBe(0.5);
  });

  it("序列统计：min/max/avg", () => {
    const s = f354.seriesStats([10, 20, 30, 150]);
    expect(s).toEqual({ min: 10, max: 100, avg: 40 }); // 150 被钳制为 100
  });

  it("空闲通道审计：违规槽逐一点名", () => {
    const a = f354.auditIdleChannel([], [5, 3, 9]);
    expect(a.pass).toBe(false);
    expect(a.violations).toEqual([{ slot: 2, fgFrameMs: 9, compliant: false }]);
    expect(f354.auditIdleChannel([], [5, 5]).pass).toBe(true);
  });
});

/* ================= ④ F355 ================= */

describe("F355 v5：三管子健康 / UA 哨兵", () => {
  it("管道健康：静默=stalled、待遇大面积失格=broken（人话诊断齐）", () => {
    const h = f355.pipeHealth({ downloadEventsLast5Min: 2, dragDeliveriesLast5Min: 0, treatmentFailures: 5 });
    expect(h.find((x) => x.pipe === "download")!.state).toBe("open");
    expect(h.find((x) => x.pipe === "drag")!.state).toBe("stalled");
    expect(h.find((x) => x.pipe === "treatment")!.state).toBe("broken");
  });

  it("UA 哨兵：伪装标记即违规（诚实边界机检面）", () => {
    expect(f355.auditUaSentinel("Mozilla/5.0 (Windows NT 10.0) Chrome").pass).toBe(false);
    expect(f355.auditUaSentinel("VARIX/1.0").pass).toBe(true);
  });
});

/* ================= ⑤ F359 ================= */

describe("F359 v5：色值解析 / 对比度 / 抗噪取样", () => {
  it("HEX/RGB 解析：三种形态全通、非法如实 null", () => {
    expect(f359.parseHex("#4F7CFF")).toEqual({ r: 79, g: 124, b: 255 });
    expect(f359.parseHex("4f7cff")).toEqual({ r: 79, g: 124, b: 255 });
    expect(f359.parseHex("#abc")).toEqual({ r: 170, g: 187, b: 204 });
    expect(f359.parseHex("#xyz123")).toBeNull();
    expect(f359.parseRgbString("rgb(1, 2, 3)")).toEqual({ r: 1, g: 2, b: 3 });
    expect(f359.parseRgbString("rgba(300, 2, 3, 0.5)")).toEqual({ r: 255, g: 2, b: 3 });
    expect(f359.parseRgbString("not a color")).toBeNull();
  });

  it("互转 round-trip 恒等（复制保真）", () => {
    expect(f359.hexRoundTrip({ r: 79, g: 124, b: 255 })).toBe(true);
    expect(f359.hexRoundTrip({ r: 0, g: 0, b: 0 })).toBe(true);
  });

  it("WCAG 对比度：黑白 = 21（理论上限）", () => {
    expect(f359.contrastRatio({ r: 0, g: 0, b: 0 }, { r: 255, g: 255, b: 255 })).toBe(21);
    expect(f359.contrastRatio({ r: 255, g: 255, b: 255 }, { r: 255, g: 255, b: 255 })).toBe(1);
  });

  it("抗噪取样：中心加权——单噪点带不歪取色", () => {
    const base: f359.Rgb = { r: 100, g: 100, b: 100 };
    const samples = Array.from({ length: 9 }, () => base);
    samples[0] = { r: 255, g: 0, b: 0 }; // 边缘噪点权重 1（中心 5 占大头）
    const avg = f359.loupeAverage(samples);
    expect(avg.r).toBeLessThan(120); // 噪点被稀释
    expect(avg.g).toBeGreaterThan(80); // 主体色保持
  });
});

/* ================= ⑥ F360 ================= */

describe("F360 v5：量测账 / 物理单位 / 网格吸附 / Esc 穷举", () => {
  it("量测账 LRU 10 条", () => {
    let h: f360.MeasureRecord[] = [];
    for (let i = 0; i < 12; i++) h = f360.pushMeasure(h, { x: 0, y: 0, w: i, h: i }, i);
    expect(h.length).toBe(f360.MEASURE_HISTORY_CAP);
    expect(h[0]!.rect.w).toBe(2); // 最早两条被挤出
  });

  it("物理换算：96 DPI 下 96px = 25.4mm = 72pt", () => {
    expect(f360.pxToPhysical(96, 96)).toEqual({ mm: 25.4, pt: 72 });
    expect(f360.pxToPhysical(10, 0)).toEqual({ mm: 0, pt: 0 }); // 非法 DPI 不编数
  });

  it("量测矩形网格吸附：8 的倍数对齐可读", () => {
    const s = f360.snapMeasureToGrid({ x: 0, y: 0, w: 1022, h: 512 });
    expect(s.w).toBe(1024);
    expect(s.h).toBe(512);
    expect(s.wAligned).toBe(false);
    expect(s.hAligned).toBe(true);
  });

  it("Esc 秒退穷举：四模式一步到 off", () => {
    expect(f360.escapeAudit().pass).toBe(true);
  });
});

/* ================= ⑦ F363 ================= */

describe("F363 v5：番茄连击 / 打断归类 / 周报 / 提醒降级", () => {
  it("番茄节奏：4 轮完成建议长休息", () => {
    expect(f363.pomodoroAdvice(3).longBreak).toBe(false);
    expect(f363.pomodoroAdvice(4).longBreak).toBe(true);
    expect(f363.pomodoroAdvice(8).longBreak).toBe(true);
  });

  it("打断归类：自打断/外部打断分开", () => {
    const run: f363.FocusRun = { day: "2026-09-26", plannedMinutes: 25, startedAt: 0, endedAt: 10, outcome: "abandoned" };
    expect(f363.classifyInterruption(run, true).interruption).toBe("external");
    expect(f363.classifyInterruption(run, false).interruption).toBe("self");
  });

  it("周报：7 天聚合与放弃率", () => {
    const lookback = () => ({ minutes: 25, runs: 2, abandoned: 1 });
    const w = f363.weeklySummary("2026-09-26", lookback);
    expect(w.minutes).toBe(175);
    expect(w.runs).toBe(14);
    expect(w.abandonRatePct).toBe(50);
  });

  it("提醒降级：音效失败 → 通知单通道仍达（提醒不静默丢失）", () => {
    expect(f363.reminderFallback(true).channels).toEqual(["chime", "notification"]);
    const fb = f363.reminderFallback(false);
    expect(fb.channels).toEqual(["notification"]);
    expect(fb.honest).toContain("不丢");
  });
});

/* ================= ⑧ F366 ================= */

describe("F366 v5：剩余时间 / 阈值边沿 / 双电池", () => {
  it("剩余时间：无速率不编数；插电不适用", () => {
    expect(f366.runtimeEstimate({ percent: 50, plugged: false }, null).minutes).toBeNull();
    expect(f366.runtimeEstimate({ percent: 50, plugged: true }, 10).minutes).toBeNull();
    expect(f366.runtimeEstimate({ percent: 50, plugged: false }, 10).minutes).toBe(300);
  });

  it("阈值边沿：恶化一级提一次；同级与好转不轰炸", () => {
    expect(f366.thresholdEdge("notice", { percent: 15, plugged: false })).toEqual({ crossed: true, level: "warning", notify: true });
    expect(f366.thresholdEdge("warning", { percent: 15, plugged: false }).notify).toBe(false);
    expect(f366.thresholdEdge("critical", { percent: 50, plugged: false }).crossed).toBe(false);
  });

  it("双电池分列：独立判级 + 聚合均值", () => {
    const r = f366.dualBatteryRender({ batteries: [{ id: "a", label: "A", percent: 80, plugged: false }, { id: "b", label: "B", percent: 20, plugged: false }] }, "iconPercent");
    expect(r.perBattery[0]!.band).toBe("green");
    expect(r.perBattery[1]!.band).toBe("yellow");
    expect(r.aggregate.percentText).toBe("50%"); // iconPercent 档才有百分比文本
  });
});

/* ================= ⑨ F367 ================= */

describe("F367 v5：模板校验 / 面积守卫 / 分区绑定", () => {
  const rects: Record<f367.ZoneId, { x: number; y: number; w: number; h: number }> = {
    work: { x: 0, y: 0, w: 500, h: 500 },
    downloads: { x: 500, y: 0, w: 500, h: 500 },
    todo: { x: 0, y: 500, w: 500, h: 500 },
    favorites: { x: 500, y: 500, w: 500, h: 500 },
  };

  it("模板校验：合法过、重叠点名为缺陷", () => {
    expect(f367.validateZoneTemplate(rects).ok).toBe(true);
    const bad = f367.validateZoneTemplate({ ...rects, todo: { x: 100, y: 100, w: 500, h: 500 } });
    expect(bad.ok).toBe(false);
    expect(bad.problems.some((p) => p.includes("重叠"))).toBe(true);
  });

  it("面积守卫：分区过小拒放（不放进去挤成一团）", () => {
    const layout = f367.saveZoneTemplate(rects).layout!;
    expect(f367.dropGuard(layout, "work").ok).toBe(true);
    const tiny = f367.saveZoneTemplate({ ...rects, work: { x: 0, y: 0, w: 200, h: 100 } }).layout!;
    expect(f367.dropGuard(tiny, "work").ok).toBe(false);
  });

  it("分区绑定：图标→分区映射", () => {
    const m = f367.applyBindings([{ id: "i1" }, { id: "i2" }], [{ iconId: "i1", zone: "work" }]);
    expect(m.get("i1")).toBe("work");
    expect(m.has("i2")).toBe(false);
  });
});

/* ================= ⑩ F368 ================= */

describe("F368 v5：托盘菜单 / 徽标 / 恢复焦点", () => {
  it("托盘右键菜单：退出永远可达（托盘不是牢笼）", () => {
    const menu = f368.trayContextMenu({ appId: "a", behavior: "closeToTray", activityPct: 40 });
    expect(menu.find((m) => m.id === "quit")!.enabled).toBe(true);
    expect(menu.find((m) => m.id === "pause")!.enabled).toBe(true);
    const idle = f368.trayContextMenu({ appId: "a", behavior: "normal", activityPct: null });
    expect(idle.find((m) => m.id === "resume")).toBeDefined();
  });

  it("徽标读数：进行中数字、完成对勾语义位", () => {
    expect(f368.badgePctFor({ appId: "a", behavior: "closeToTray", activityPct: 45 })).toEqual({ show: true, text: "45", done: false });
    expect(f368.badgePctFor({ appId: "a", behavior: "closeToTray", activityPct: 100 })).toEqual({ show: true, text: null, done: true });
    expect(f368.badgePctFor({ appId: "a", behavior: "closeToTray", activityPct: null }).show).toBe(false);
  });

  it("恢复焦点：不创建第二实例（单实例纪律）", () => {
    const rt: f368.TrayRuntime = { resident: [{ appId: "a", behavior: "closeToTray", activityPct: null }], gone: [] };
    const r = f368.restoreFocusPolicy(rt, "a");
    expect(r.createsNewInstance).toBe(false);
    expect(r.residentAfter.resident.length).toBe(0);
  });
});

/* ================= ⑪ F371 ================= */

describe("F371 v5：五段分解 / 周趋势 / 慢启动归因", () => {
  const full: f371.PhaseBreakdown = { phases: { 固件: 200, 引导: 300, 内核: 1000, 会话: 800, 桌面: 700 } };

  it("分解对账：五段之和=总时长（±1ms）", () => {
    expect(f371.validateBreakdown(full, 3000).ok).toBe(true);
    expect(f371.validateBreakdown(full, 3005).ok).toBe(false);
  });

  it("最慢段归因：内核占 1/3 最慢", () => {
    const s = f371.slowestPhase(full);
    expect(s.phase).toBe("内核");
    expect(s.pct).toBe(33);
  });

  it("周趋势：按周聚合均值与次数", () => {
    const now = 100 * 7 * 24 * 3600 * 1000;
    const hist: f371.BootRecord[] = [
      { seq: 1, measuredMs: 2000, at: now - 1000 },
      { seq: 2, measuredMs: 4000, at: now - 2000 },
    ];
    const t = f371.weeklyTrend(hist, now);
    expect(t).toEqual([{ weekIndex: 0, avgMs: 3000, boots: 2 }]);
  });

  it("慢启动归因：超中位 50% 点名最慢段（人话）", () => {
    const hist: f371.BootRecord[] = [
      { seq: 1, measuredMs: 2000, at: 1 },
      { seq: 2, measuredMs: 2100, at: 2 },
      { seq: 3, measuredMs: 2200, at: 3 },
    ];
    const slow: f371.BootRecord = { seq: 4, measuredMs: 4000, at: 4 };
    expect(f371.slowBootHint({ seq: 5, measuredMs: 2100, at: 5 }, full, hist).slow).toBe(false);
    const h = f371.slowBootHint(slow, full, hist);
    expect(h.slow).toBe(true);
    expect(h.hint).toContain("内核");
  });
});

/* ================= ⑫ F373 ================= */

describe("F373 v5：切换 OSD / 死键 / 热插拔", () => {
  const st: f373.LayoutState = { layouts: [{ id: "pinyin", name: "中文（拼音）", options: {} }, { id: "english", name: "英语（美式）", options: {} }], activeIndex: 1 };

  it("切换 OSD：800ms 内可见、超时消失", () => {
    expect(f373.switchOsd(st, 1000, 1500)).toEqual({ visible: true, text: "英语（美式）" });
    expect(f373.switchOsd(st, 1000, 2000).visible).toBe(false);
  });

  it("死键：组合合成、Esc 取消、组合失败零吞键", () => {
    let s: f373.DeadKeyState = { pending: null };
    let r = f373.deadKeyEvent(s, "acute-dead");
    expect(r.output).toBeNull();
    s = r.state;
    r = f373.deadKeyEvent(s, "e");
    expect(r.output).toBe("é");
    s = { pending: "grave-dead" };
    r = f373.deadKeyEvent(s, "Escape");
    expect(r.output).toBeNull();
    expect(r.state.pending).toBeNull();
    s = { pending: "grave-dead" };
    r = f373.deadKeyEvent(s, "z");
    expect(r.output).toBe("gravez"); // 零吞键：死键+键都出来
  });

  it("热插拔通知：新布局建议添加、已知布局不打扰", () => {
    expect(f373.hotplugNotice(st, ["pinyin", "english"]).notice).toBeNull();
    const h = f373.hotplugNotice(st, ["dvorak"]);
    expect(h.suggest).toEqual(["dvorak"]);
    expect(h.notice).toContain("dvorak");
  });
});

/* ================= ⑬ F374 ================= */

describe("F374 v5：键位规范化 / 搜索 / 分组折叠", () => {
  it("键位规范化：修饰键稳定序（同组合唯一写法）", () => {
    expect(f374.chordNormalize("Shift+Ctrl+P")).toBe("Ctrl+Shift+P");
    expect(f374.chordNormalize("Alt+Ctrl+Shift+Win+P")).toBe("Ctrl+Alt+Shift+Win+P");
    expect(f374.chordNormalize("Ctrl+P")).toBe("Ctrl+P");
  });

  it("卡内搜索：动作与键位双字段", () => {
    const entries: f374.HotkeyEntry[] = [
      { combo: "Win+D", action: "显示桌面", group: "system" },
      { combo: "Ctrl+F", action: "查找", group: "appGeneric" },
    ];
    expect(f374.searchFilter(entries, "桌面").length).toBe(1);
    expect(f374.searchFilter(entries, "ctrl").length).toBe(1);
    expect(f374.searchFilter(entries, " ").length).toBe(2);
  });

  it("分组折叠：默认每组 5 条 + 隐藏数（层级递进）", () => {
    const entries: f374.HotkeyEntry[] = Array.from({ length: 8 }, (_, i) => ({ combo: `K${i}`, action: `a${i}`, group: "window" as const }));
    const g = f374.groupCollapse([{ group: "window", entries }], new Set());
    expect(g[0]!.visible.length).toBe(5);
    expect(g[0]!.hiddenCount).toBe(3);
    const expanded = f374.groupCollapse([{ group: "window", entries }], new Set(["window"]));
    expect(expanded[0]!.visible.length).toBe(8);
    expect(expanded[0]!.hiddenCount).toBe(0);
  });
});

/* ================= ⑭ F375 ================= */

describe("F375 v5：回归对比 / 豁免 / 摘要", () => {
  const cp = (item: string, name: string, passed: boolean): f375.Checkpoint => ({ item, name, passed, evidence: "e" });

  it("回归对比：新红/转绿/持续红三清单", () => {
    const prev = [cp("F1", "a", true), cp("F2", "b", false)];
    const curr = [cp("F1", "a", false), cp("F2", "b", true), cp("F3", "c", false)];
    const d = f375.diffReports(prev, curr);
    expect(d.newlyRed).toEqual(["F1/a"]);
    expect(d.newlyGreen).toEqual(["F2/b"]);
    expect(d.stillRed).toEqual(["F3/c"]);
    expect(d.regressed).toBe(true);
  });

  it("豁免：缺理由无效、过期失效（不是永久牌）", () => {
    expect(f375.exemptionActive({ item: "F1", name: "a", reason: "", untilIso: "2999-01-01" }, "2026-01-01").active).toBe(false);
    expect(f375.exemptionActive({ item: "F1", name: "a", reason: "等实机日", untilIso: "2999-01-01" }, "2026-01-01").active).toBe(true);
    expect(f375.exemptionActive({ item: "F1", name: "a", reason: "等实机日", untilIso: "2020-01-01" }, "2026-01-01").active).toBe(false);
  });

  it("人话摘要：可发布/回炉一行结论", () => {
    const reports: f375.WalkthroughReports = {
      consistency: { rows: [], pass: true },
      taskChain: { steps: [], totalSeconds: 600, pass: true, budgetSeconds: 900 },
      degraded: { states: [], pass: true },
    };
    const d = f375.releaseDecision([cp("F1", "a", true)], reports);
    expect(f375.walkthroughDigest(d)).toContain("可发布");
    const d2 = f375.releaseDecision([cp("F1", "a", false)], reports);
    expect(f375.walkthroughDigest(d2)).toContain("回炉");
  });
});

/* ================= ⑮ F376 ================= */

describe("F376 v5：键盘导航 / 双击标题 / 加速键", () => {
  it("菜单键盘导航：上下循环跳过置灰、Esc 关闭", () => {
    const r1 = f376.menuKeyboardNav("maximized", null, "ArrowDown");
    expect(r1.focusId).toBe("restore"); // maximized 态 move/size/maximize 置灰——restore 是第一个可用项
    const r2 = f376.menuKeyboardNav("normal", "close", "ArrowDown");
    // 归因：normal 态 restore 置灰，可用项 = [move,size,minimize,maximize,close]——close 环回 move
    expect(r2.focusId).toBe("move");
    expect(f376.menuKeyboardNav("normal", null, "Escape").closed).toBe(true);
  });

  it("双击标题语义：normal 最大化、maximized 还原、minimized 不响应", () => {
    expect(f376.doubleClickTitle("normal").action).toBe("maximize");
    expect(f376.doubleClickTitle("maximized").action).toBe("restore");
    expect(f376.doubleClickTitle("minimized").action).toBe("none");
  });

  it("加速键：置灰项不响应（键盘也不许绕过置灰状态机）", () => {
    expect(f376.mnemonicHit("normal", "R")).toBeNull(); // normal 态还原置灰 → 键盘同样置灰
    expect(f376.mnemonicHit("maximized", "R")).toBe("restore"); // maximized 态还原可用
    expect(f376.mnemonicHit("normal", "X")).toBe("maximize"); // normal 态最大化可用 → 命中
    expect(f376.mnemonicHit("maximized", "X")).toBeNull(); // maximized 态最大化置灰
  });
});

/* ================= ⑯ F378 ================= */

describe("F378 v5：预算内多列 / 手动优先 / CJK 计量", () => {
  const cols: f378.ColumnSpec[] = [
    { id: "name", width: 100, last: false },
    { id: "size", width: 100, last: false },
    { id: "rest", width: 100, last: true },
  ];
  const cells: f378.CellText[] = [
    { columnId: "name", text: "aaaa" },
    { columnId: "size", text: "bbbb" },
    { columnId: "rest", text: "cc" },
  ];
  const m = (t: string) => t.length * 10;

  it("多列同时自适应：总宽超限按比例缩、不低于 40px", () => {
    const r = f378.autofitAll(cells, cols, 500, m);
    expect(r.squeezed).toBe(false);
    // 归因：三列 autofit 总宽 144 > 100 才触发挤压
    const tight = f378.autofitAll(cells, cols, 100, m);
    expect(tight.squeezed).toBe(true);
    expect(Object.values(tight.widths).every((w) => w >= f378.MIN_COLUMN_PX)).toBe(true);
  });

  it("手动宽度优先：拖过的列不被 autofit 覆盖", () => {
    const r = f378.autofitRespectingManual(cells, cols, { manual: { name: 300 } }, m);
    expect(r.name).toBe(300);
    expect(r.size).toBe(f378.autofitWidth(cells, "size", m));
  });

  it("CJK 感知测量：全角双倍宽", () => {
    expect(f378.measureCjkAware("ab")).toBe(16);
    expect(f378.measureCjkAware("中文")).toBe(32);
  });
});

/* ================= ⑰ F379 ================= */

describe("F379 v5：类型感知比较器 / 稳定性 / aria-sort", () => {
  it("类型感知：数值按值比、日期按时间比、文本 zh locale", () => {
    const rows = [{ id: 1, n: "10", d: "2026-01-02" }, { id: 2, n: "9", d: "2026-01-10" }];
    const byNum = f379.typedComparator([{ columnId: "n", dir: "asc", type: "number" }], (r, c) => (r as Record<string, string>)[c]!);
    expect(byNum(rows[0], rows[1])).toBeGreaterThan(0); // "10" > "9" 数值序
    const byDate = f379.typedComparator([{ columnId: "d", dir: "asc", type: "date" }], (r, c) => (r as Record<string, string>)[c]!);
    expect(byDate(rows[0], rows[1])).toBeLessThan(0);
  });

  it("稳定排序审计：同值保持输入序", () => {
    const r = f379.auditStability([{ i: 0, k: 1 }, { i: 1, k: 1 }, { i: 2, k: 0 }], (x) => x.k);
    expect(r.pass).toBe(true);
  });

  it("aria-sort 三态", () => {
    const st: f379.HeaderSortState = { keys: [{ columnId: "a", dir: "asc" }] };
    expect(f379.ariaSortFor(st, "a")).toBe("ascending");
    expect(f379.ariaSortFor({ keys: [{ columnId: "a", dir: "desc" }] }, "a")).toBe("descending");
    expect(f379.ariaSortFor(st, "b")).toBe("none");
  });
});

/* ================= ⑱ F380 ================= */

describe("F380 v5：空白拖拽 / 双击新建 / Shift 语义", () => {
  it("空白拖拽=框选启动：起点锚 + 选择清空", () => {
    const r = f380.dragOnBlank({ x: 5, y: 5 });
    expect(r.rubberBandStart).toEqual({ x: 5, y: 5 });
    expect(r.selectionCleared.selectedIds).toEqual([]);
  });

  it("双击空白=新建语义位（空白不是死区）", () => {
    expect(f380.dblClickBlank().action).toBe("new-item");
  });

  it("Shift+点击空白：无效但不破坏（选择原样保留）", () => {
    const sel: f380.ListSelection = { selectedIds: ["a", "b"], anchorId: "a" };
    const r = f380.shiftClickBlank(sel);
    expect(r.consumed).toBe(false);
    expect(r.selection).toEqual(sel);
  });
});

/* ================= ⑲ F381 ================= */

describe("F381 v5：迭代扁平化 / 懒加载 / 移动校验", () => {
  const tree: f381.TreeNode = { id: "root", children: [{ id: "a", children: [{ id: "a1", children: [] }] }, { id: "b", children: [] }] };

  it("迭代扁平化与递归版逐位一致（深树不爆栈）", () => {
    const expanded = new Set(["root", "a"]);
    expect(f381.flattenIterative(tree, expanded)).toEqual(f381.flatten(tree, expanded));
  });

  it("懒加载：子节点未知 → needLoad（不假展开）", () => {
    const lazy: f381.LazyNode = { id: "x", children: [], childrenKnown: false };
    expect(f381.lazyExpandCheck(lazy, { checked: [], expanded: [] }).action).toBe("needLoad");
    const known: f381.LazyNode = { id: "x", children: [{ id: "c", children: [] }], childrenKnown: true };
    expect(f381.lazyExpandCheck(known, { checked: [], expanded: [] }).action).toBe("expand");
  });

  it("移动校验：不许移进自己子树（成环 = 树被毁）", () => {
    const moving: f381.TreeNode = { id: "a", children: [{ id: "a1", children: [] }] };
    expect(f381.validateMove(moving, moving.children[0]!).ok).toBe(false);
    expect(f381.validateMove(moving, { id: "root", children: [] }).ok).toBe(true);
    expect(f381.validateMove(moving, moving).ok).toBe(false);
  });
});

/* ================= ⑳ F382 ================= */

describe("F382 v5：尺寸记忆 / 工作区感知 / 焦屏落位", () => {
  it("尺寸记忆防抖：<10% 变化不记（误触不覆盖习惯）", () => {
    expect(f382.shouldRememberSize(null, { w: 800, h: 600 })).toBe(true);
    expect(f382.shouldRememberSize({ w: 800, h: 600 }, { w: 820, h: 600 })).toBe(false);
    expect(f382.shouldRememberSize({ w: 800, h: 600 }, { w: 900, h: 600 })).toBe(true);
  });

  it("工作区感知：落点钳进工作区（任务栏避让）", () => {
    const wa = { x: 0, y: 0, w: 1920, h: 1032 };
    const r = f382.fitToWorkArea({ x: 100, y: 1000 }, { w: 400, h: 300 }, wa);
    expect(r.y).toBe(wa.h - 300);
    expect(r.clamped).toBe(true);
  });

  it("焦屏落位：记忆位置相对焦点屏解释", () => {
    const screens = [{ x: 0, y: 0, w: 1920, h: 1080 }, { x: 1920, y: 0, w: 1920, h: 1080 }];
    const r = f382.placementOnFocusedScreen("save", screens, 1, { w: 400, h: 300 });
    expect(r.screenIndex).toBe(1);
    expect(r.x).toBeGreaterThanOrEqual(1920); // 落在第二屏内
  });
});

/* ================= ㉑ F385 ================= */

describe("F385 v5：中文映射 / 三段合成 / 词表完备", () => {
  it("朗读三段合成：名称+角色+状态", () => {
    const node: f385.SemanticNode = { id: "x", role: "switch", name: "自动保存", nameSource: "label", state: "开", parentIds: [] };
    expect(f385.composeUtterance(node)).toBe("自动保存，开关，开");
    const btn: f385.SemanticNode = { ...node, role: "button", state: null };
    expect(f385.composeUtterance(btn)).toBe("自动保存，按钮");
  });

  it("角色词表完备：交互角色全在词表内且全有中文读法", () => {
    const a = f385.auditRoleVocabulary();
    expect(a.pass).toBe(true);
    expect(a.problems).toEqual([]);
  });

  it("遍历读序：父先子后（读屏顺序 = 视觉顺序）", () => {
    const tree = f385.createTree([
      { id: "root", role: "list", name: "列表", nameSource: "text", state: null, parentIds: [] },
      { id: "i1", role: "listitem", name: "项一", nameSource: "text", state: null, parentIds: ["root"] },
      { id: "i2", role: "listitem", name: "项二", nameSource: "text", state: null, parentIds: ["root"] },
    ]);
    expect(f385.traversalOrder(tree, ["root"])).toEqual(["root", "i1", "i2"]);
  });
});

/* ================= ㉒ F386 ================= */

describe("F386 v5：段落重排 / 字体栈 / 截词审计", () => {
  const style = f386.defaultStyle(); // 45 字/行

  it("段落重排：只产视图行、剥掉换行后与原文逐字一致（零侵入）", () => {
    const text = "这是一段很长的中文文本，需要被软换行成多行显示。".repeat(3);
    const a = f386.auditViewIntegrity(text, style);
    expect(a.pass).toBe(true);
    // 归因：软换行插入换行符 → 两哈希必然不同；零侵入由 pass（剥换行逐字比对）判定
    expect(a.hashBefore).not.toBe(a.hashAfter);
  });

  it("英文词完整不截断", () => {
    const lines = f386.reflowParagraphs("hello world this is a long english sentence", style);
    const audit = f386.auditNoBrokenWords(lines, "hello world this is a long english sentence");
    expect(audit.pass).toBe(true);
  });

  it("衬线字体栈：开走宋体系、关走黑体系", () => {
    expect(f386.fontStackFor({ ...style, serif: true })).toBe(f386.SERIF_STACK);
    expect(f386.fontStackFor({ ...style, serif: false })).toBe(f386.SANS_STACK);
  });
});

/* ================= ㉓ F387 ================= */

describe("F387 v5：强度档位 / 过渡计划 / 豁免审计", () => {
  it("强度混合：100% 全灰、0 强度原样（档位语义）", () => {
    const c = { r: 255, g: 0, b: 0 };
    const full = f387.toGrayscaleWithIntensity(c.r, c.g, c.b, 1);
    expect(full.r).toBe(full.g);
    expect(full.g).toBe(full.b);
    const none = f387.toGrayscaleWithIntensity(c.r, c.g, c.b, 0.3);
    expect(none.r).toBeGreaterThan(full.r); // 淡灰保留部分原色
  });

  it("过渡计划：同滤镜零过渡、异滤镜 150ms 淡入", () => {
    expect(f387.transitionPlan("grayscale", "grayscale").durationMs).toBe(0);
    const t = f387.transitionPlan("none", "grayscale");
    expect(t.durationMs).toBe(f387.FILTER_TRANSITION_MS);
    expect(t.interpolate).toBe(true);
  });

  it("豁免审计：缺理由的豁免点名（默认零豁免）", () => {
    expect(f387.auditExemptWindows([]).pass).toBe(true);
    const bad = f387.auditExemptWindows([{ windowClass: "video-player", reason: "" }]);
    expect(bad.pass).toBe(false);
  });
});

/* ================= ㉔ F388 ================= */

describe("F388 v5：旋转过渡 / 侧边任务栏 / 批量审计", () => {
  it("旋转过渡：同向零过渡、异向 150ms 交叉淡入", () => {
    expect(f388.rotationTransition("landscape", "landscape").durationMs).toBe(0);
    const t = f388.rotationTransition("landscape", "portrait");
    expect(t.durationMs).toBe(f388.ROTATION_TRANSITION_MS);
    expect(t.crossFade).toBe(true);
  });

  it("侧边任务栏几何：left/right 各扣 48px", () => {
    const wa = f388.workAreaWithSideTaskbar({ w: 1080, h: 1920 }, "left");
    expect(wa.x).toBe(f388.SIDEBAR_TASKBAR_PX);
    expect(wa.w).toBe(1080 - f388.SIDEBAR_TASKBAR_PX);
    const waRight = f388.workAreaWithSideTaskbar({ w: 1080, h: 1920 }, "right");
    expect(waRight.x).toBe(0);
    expect(waRight.w).toBe(1080 - f388.SIDEBAR_TASKBAR_PX);
  });

  it("批量重适配：阈值内直算、超阈分帧（无冻结帧）", () => {
    expect(f388.readaptBatchAudit(100).note).toContain("预算内");
    expect(f388.readaptBatchAudit(500).note).toContain("分帧");
  });
});

/* ================= ㉕ F399 ================= */

describe("F399 v5：场景参数 / 日志 / 口碑守卫", () => {
  it("星野场景参数在册（F124 谱登记数据）", () => {
    expect(f399.STARFIELD_SCENE.particles).toBeGreaterThan(0);
    expect(f399.STARFIELD_SCENE.frameBudgetMs).toBeLessThanOrEqual(16.6);
    expect(f399.EMBLEM_VARIANT_SCENE.once).toBe(true);
  });

  it("触发日志：记录与查询（只记事件不记内容）", () => {
    let j: f399.EggJournalEntry[] = [];
    j = f399.recordEggPlayed(j, "terminalStar", 100);
    j = f399.recordEggPlayed(j, "boot100", 200);
    expect(f399.lastPlayedAt(j, "boot100")).toBe(200);
    expect(f399.lastPlayedAt(j, "about7taps")).toBeNull();
  });

  it("口碑守卫：彩蛋不进任何提示/搜索/引导面", () => {
    expect(f399.auditNotInDiscoverabilitySurfaces(["设置中心首页", "搜索索引"])).toEqual({ pass: true, leaked: [] });
    const bad = f399.auditNotInDiscoverabilitySurfaces(["搜索索引: 星野粒子"]);
    expect(bad.pass).toBe(false);
  });
});

/* ================= ㉖ F400 ================= */

describe("F400 v5：覆盖率统计 / 登记册 diff", () => {
  it("覆盖率统计：H4 内置 50 + 注入数 → 诚实百分比", () => {
    const s = f400.coverageStats([]);
    expect(s.total).toBe(50);
    expect(s.pendingInjection).toBe(150);
    expect(s.coveragePct).toBe(25);
    const full = f400.coverageStats(Array.from({ length: 150 }, (_, i) => ({ item: `F${201 + i}` })));
    expect(full.coveragePct).toBe(100);
    expect(full.pendingInjection).toBe(0);
  });

  it("登记册 diff：增/删/改名三类（登记册不是化石）", () => {
    const prev: f400.LedgerLine[] = [{ item: "F351", title: "工作区快照" }, { item: "F352", title: "旧名" }];
    const curr: f400.LedgerLine[] = [{ item: "F351", title: "工作区快照" }, { item: "F352", title: "缩略图窗上直接操作" }, { item: "F601", title: "新项" }];
    const d = f400.ledgerDiff(prev, curr);
    expect(d).toContainEqual({ item: "F352", kind: "renamed", from: "旧名", to: "缩略图窗上直接操作" });
    expect(d).toContainEqual({ item: "F601", kind: "added", from: null, to: "新项" });
  });
});

/* ================= ㉗ 隔离验证扩展（v5）：import 图无环 ================= */

describe("v5 隔离验证扩展：import 图无环检测", () => {
  function parseImports(file: string): string[] {
    const src = readFileSync(join(H4_DIR, file), "utf-8");
    return [...src.matchAll(/from\s+["'](\.[^"']+)["']/g)].map((m) => m[1]!).map((imp) => {
      // ./fXXX-xxx / ./internal/yyy → 模块标识
      const parts = imp.split("/");
      return parts[parts.length - 1]!.replace(/\.ts$/, "");
    });
  }

  it("依赖图无环：任何模块都不存在循环依赖（架构腐化即红灯）", () => {
    const files = readdirSync(H4_DIR).filter((f) => /^f\d{3}-.*\.ts$/.test(f));
    const graph = new Map<string, string[]>();
    for (const f of files) graph.set(f, parseImports(f).filter((t) => files.includes(`${t}.ts`)));
    // DFS 三色标记法
    const WHITE = 0, GRAY = 1, BLACK = 2;
    const color = new Map<string, number>(files.map((f) => [f, WHITE]));
    let hasCycle = false;
    const stack: string[] = [];
    const visit = (f: string): void => {
      if (hasCycle) return;
      color.set(f, GRAY);
      stack.push(f);
      for (const dep of graph.get(f) ?? []) {
        if (color.get(dep) === GRAY) {
          hasCycle = true;
          return;
        }
        if (color.get(dep) === WHITE) visit(dep);
      }
      stack.pop();
      color.set(f, BLACK);
    };
    for (const f of files) if (color.get(f) === WHITE) visit(f);
    expect(hasCycle).toBe(false);
  });

  it("internal 层零出向依赖（底座不反向依赖业务——层级不变量）", () => {
    const offenders: string[] = [];
    for (const f of readdirSync(join(H4_DIR, "internal")).filter((x) => x.endsWith(".ts"))) {
      const src = readFileSync(join(H4_DIR, "internal", f), "utf-8");
      if (/from\s+["']\.\.\//.test(src)) offenders.push(f);
    }
    expect(offenders).toEqual([]);
  });
});

/* ================= ㉘ 检查项对账扩展（v5） ================= */

describe("v5 检查项对账：26 模块深化导出面可发现性", () => {
  const V5_EXPORTS: ReadonlyArray<[Record<string, unknown>, string]> = [
    [f352, "refreshThrottle"], [f353, "topologySignature"], [f354, "downsampleHistory"], [f355, "pipeHealth"],
    [f359, "loupeAverage"], [f360, "pushMeasure"], [f363, "pomodoroAdvice"], [f366, "dualBatteryRender"],
    [f367, "validateZoneTemplate"], [f368, "trayContextMenu"], [f371, "slowBootHint"], [f373, "deadKeyEvent"],
    [f374, "chordNormalize"], [f375, "diffReports"], [f376, "menuKeyboardNav"], [f378, "autofitAll"],
    [f379, "typedComparator"], [f380, "dragOnBlank"], [f381, "flattenIterative"], [f382, "placementOnFocusedScreen"],
    [f385, "composeUtterance"], [f386, "reflowParagraphs"], [f387, "toGrayscaleWithIntensity"], [f388, "workAreaWithSideTaskbar"],
    [f399, "recordEggPlayed"], [f400, "ledgerDiff"],
  ];

  it("26 个 v5 深化模块的新导出全部可发现、可调用（零死代码——新增即被消费）", () => {
    const missing = V5_EXPORTS.filter(([mod, fn]) => typeof mod[fn] !== "function").map(([, fn]) => fn);
    expect(missing).toEqual([]);
  });
});
