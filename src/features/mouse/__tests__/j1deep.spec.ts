/**
 * J1 深化批次单测：惯性引擎 / 手势录制器 / 档案打包 / 遥测 / 证据引擎 /
 * 悬停编排器 / 护边分屏对 / 自动调谐 / 油门爬升 / F616 前台挂载 / F244 冲突。
 */

import { describe, expect, it, beforeEach, vi } from "vitest";
import { INERTIA_DEFAULT, WheelInertia, inertiaSuspension } from "../inertia";
import {
  CUSTOM_GESTURE_CAP,
  GestureRecorder,
  encodeTrail,
  exportCustomGestures,
  importCustomGestures,
  matchSequence,
  resampleTrail,
} from "../gestureRecorder";
import { exportPack, importPack, packSections, validatePack } from "../pack";
import { j1Telemetry } from "../telemetry";
import { auditEvidence, buildEvidencePack } from "../evidence";
import { MenuHoverController, clampHoverDelay } from "../hoverTiming";
import { seamGuardForPair, seamPairKey, setSeamPairOverride } from "../screen";
import { autoTuneTremor } from "../filters";
import { autoscrollRamp } from "../autoscroll";
import { importDeviceProfiles, trackCurrentApp, applyIncremental } from "../profiles";
import { sideButtonConflicts } from "../sideButtons";
import { simulateTrace, curveSmoothness, SENS_PRESETS } from "../curve";
import { J1_DEFAULTS, j1Store } from "../j1store";

/* ------------------------------- 惯性引擎 ------------------------------- */

describe("滚轮惯性（F204 平滑档余韵）", () => {
  const cfg = () => ({ ...INERTIA_DEFAULT });

  it("输入停歇前不释放（交给输入事件本身）；停歇后指数衰减", () => {
    const w = new WheelInertia(cfg);
    w.feed(9, 1, 0);
    expect(w.tick(50)).toBe(0); // 50ms < 66ms：还在输入节奏内
    const v1 = w.tick(200);
    expect(v1).toBeGreaterThan(0);
    const v2 = w.tick(400);
    expect(Math.abs(v2)).toBeLessThan(Math.abs(v1)); // 衰减
  });

  it("衰减到阈值以下自动归零（防无限微滚）", () => {
    const w = new WheelInertia(cfg);
    w.feed(1, 1, 0);
    let at = 100;
    let steps = 0;
    while (w.active && steps < 500) {
      at += 66;
      w.tick(at);
      steps += 1;
    }
    expect(w.active).toBe(false);
    expect(w.tick(at + 66)).toBe(0);
  });

  it("速度上限钳制；方向符号保真；互斥裁决", () => {
    const w = new WheelInertia(cfg);
    w.feed(12, -1, 0);
    const v = w.tick(300);
    expect(v).toBeLessThanOrEqual(0);
    expect(Math.abs(v)).toBeLessThanOrEqual(INERTIA_DEFAULT.maxPxPerFrame);
    expect(inertiaSuspension(true)).toBe("suspended");
    expect(inertiaSuspension(false)).toBe("active");
  });
});

/* ------------------------------- 手势录制器 ------------------------------- */

describe("F617 手势录制器", () => {
  it("重采样：任意快慢笔画归一为等距点列", () => {
    const slow: { x: number; y: number }[] = [];
    for (let i = 0; i <= 10; i++) slow.push({ x: i * 12, y: 0 });
    const rs = resampleTrail(slow);
    expect(rs).toHaveLength(32);
    expect(encodeTrail(slow)).toEqual([0]); // 水平右移 → 单方向「右」
  });

  it("无效笔画显性拒绝（总长不足/点数过少）", () => {
    const r = new GestureRecorder();
    r.begin();
    r.feed(3, 0);
    expect(r.finish()).toBe(false);
    expect(r.lastError).toContain("笔画无效");
    expect(r.state).toBe("idle");
  });

  it("录制→查重→入库全链；重复显性拒绝；上限拒绝", () => {
    const r = new GestureRecorder();
    r.begin();
    for (let i = 1; i <= 8; i++) r.feed(i * 30, 0); // 右划 → forward
    expect(r.finish()).toBe(true);
    const lib = [{ id: "forward", name: "前进", dirs: [0] as const }];
    expect(r.duplicateOf(lib as never)?.id).toBe("forward");
    // 与库不同（下右）可入库。
    const r2 = new GestureRecorder();
    r2.begin();
    for (let i = 1; i <= 4; i++) r2.feed(0, i * 30);
    for (let i = 1; i <= 4; i++) r2.feed(i * 30, 120);
    expect(r2.finish()).toBe(true);
    expect(r2.duplicateOf(lib as never)).toBeNull();
    const custom = {};
    expect(r2.commit("测试手势", "custom.test", custom).ok).toBe(true);
    expect(Object.keys(custom)).toHaveLength(1);
    // 上限拒绝。
    const full = Object.fromEntries(Array.from({ length: CUSTOM_GESTURE_CAP }, (_, i) => [`g${i}`, { name: `g${i}`, dirs: [0 as const], action: "x" }]));
    const r3 = new GestureRecorder();
    r3.begin();
    for (let i = 1; i <= 8; i++) r3.feed(0, i * 30);
    r3.finish();
    const res = r3.commit("超出", "x", full);
    expect(res.ok).toBe(false);
    expect(res.error).toContain("上限");
  });

  it("导出导入 round-trip；非法包显性列错（半套不收）", () => {
    const custom: Record<string, { name: string; dirs: import("../gestures").Dir8[]; action: string }> = { a: { name: "A", dirs: [0, 2], action: "custom.a" } };
    const json = exportCustomGestures(custom);
    const back = importCustomGestures(json);
    expect(back.ok).toBe(true);
    if (back.ok) expect(back.data.a!.dirs).toEqual([0, 2]);
    const bad = importCustomGestures(JSON.stringify({ format: "vx-gestures", version: 1, data: { bad: { name: "x", dirs: [9], action: "y" } } }));
    expect(bad.ok).toBe(false);
    if (!bad.ok) expect(bad.errors[0]).toContain("dirs 非法");
  });

  it("matchSequence 容差与编码一致性", () => {
    expect(matchSequence([0, 2], [1, 2])).toBe(true);
    expect(matchSequence([0], [0, 0])).toBe(false);
  });
});

/* ------------------------------- 档案打包 F623 ------------------------------- */

describe("F623 档案打包", () => {
  beforeEach(() => j1Store.reset());

  it("导出→校验→导入 round-trip；分节齐 20", () => {
    j1Store.set("magnet", { radiusPx: 16 });
    const pack = exportPack();
    expect(packSections(pack)).toHaveLength(20);
    const v = validatePack(pack);
    expect(v.ok).toBe(true);
    importPack(pack);
    expect(j1Store.getWith("magnet", "radiusPx", 12)).toBe(16);
  });

  it("三例判据：超速拒绝 / 超量降级警告 / 非法轨迹拒绝", () => {
    const base = exportPack();
    // 超速
    const speed = { ...base, devices: [{ id: "d1", name: "超速鼠", deviceKey: "k1", params: { sens: 99, curve: "linear", wheelMode: "notch" }, createdAt: 0, lastUsedAt: 0 }] };
    expect(validatePack(speed).errors.some((e) => e.includes("速度参数非法"))).toBe(true);
    // 超量 → 警告 + 导入保留最近 10 台
    const many = { ...base, devices: Array.from({ length: 13 }, (_, i) => ({ id: `d${i}`, name: `鼠${i}`, deviceKey: `k${i}`, params: { sens: 1, curve: "linear" as const, wheelMode: "notch" as const }, createdAt: 0, lastUsedAt: i })) };
    const vMany = validatePack(many);
    expect(vMany.ok).toBe(true);
    expect(vMany.warnings.some((w) => w.includes("超上限"))).toBe(true);
    const r = importPack(many);
    expect(r.evicted).toHaveLength(3);
    // 非法轨迹
    const badTrail = { ...base, config: { ...base.config, gestures: { ...base.config.gestures, custom: { bad: { name: "x", dirs: [42], action: "y" } } } } };
    expect(validatePack(badTrail).ok).toBe(false);
    // 校验失败整包拒绝（中断原子性）。
    expect(() => importPack(badTrail)).toThrow(/整包拒绝/);
  });
});

/* ------------------------------- 遥测 ------------------------------- */

describe("J1 体验日志（十三章）", () => {
  it("狂点/死点自动捕获；结论字段与摘要导出；清空生效", () => {
    j1Telemetry.clear();
    j1Telemetry.enabled = true;
    for (let i = 0; i < 3; i++) j1Telemetry.log("click", "no-feedback", "button", 10, 10);
    expect(j1Telemetry.frustrationCount).toBeGreaterThanOrEqual(1);
    expect(j1Telemetry.worstTen().some((f) => f.kind === "rage-click")).toBe(true);
    j1Telemetry.gestureOutcome(false, 3, 100, 100);
    expect(j1Telemetry.worstTen().some((f) => f.kind === "gesture-abandoned")).toBe(true);
    j1Telemetry.anchorLifecycle("open", 50, 50);
    j1Telemetry.anchorLifecycle("close", 50, 50); // 立即关 → anchor-bounce
    expect(j1Telemetry.worstTen().some((f) => f.kind === "anchor-bounce")).toBe(true);
    const timeline = JSON.parse(j1Telemetry.exportTimeline()) as { summary: { total: number } };
    expect(timeline.summary.total).toBeGreaterThan(0);
    j1Telemetry.clear();
    expect(j1Telemetry.size).toBe(0);
  });

  it("关闭态零采集（隐私控制入口）", () => {
    j1Telemetry.clear();
    j1Telemetry.enabled = false;
    j1Telemetry.log("click", "smooth", "button", 1, 1);
    expect(j1Telemetry.size).toBe(0);
    j1Telemetry.enabled = true;
  });
});

/* ------------------------------- 证据引擎 ------------------------------- */

describe("判据证据引擎", () => {
  it("证据包全节齐备且内建自检全绿", () => {
    const pack = buildEvidencePack();
    expect(pack.format).toBe("vx-j1-evidence");
    expect(pack.sections.f601_gain).toHaveLength(4);
    expect(pack.sections.f604_dirs16).toHaveLength(16);
    expect(pack.sections.f611_spectrum).toHaveLength(2);
    const audit = auditEvidence(pack);
    expect(audit.ok).toBe(true);
  });
});

/* ------------------------------- 悬停编排器 ------------------------------- */

describe("F610 MenuHoverController", () => {
  it("enter 延迟触发；click 立即；leave 取消；切换宿主关旧开新", () => {
    vi.useFakeTimers();
    const opened: string[] = [];
    const closed: string[] = [];
    const c = new MenuHoverController(() => 400, (k) => opened.push(k), (k) => closed.push(k));
    c.enter("a");
    expect(opened).toHaveLength(0);
    vi.advanceTimersByTime(400);
    expect(opened).toEqual(["a"]);
    c.enter("b");
    vi.advanceTimersByTime(400);
    expect(opened).toEqual(["a", "b"]);
    c.enter("c");
    c.leave();
    vi.advanceTimersByTime(500);
    expect(opened).toHaveLength(2); // leave 取消 pending
    c.click("d");
    expect(opened).toEqual(["a", "b", "d"]); // 点击即时（红线）
    c.dispose();
    vi.useRealTimers();
  });

  it("档位钳制回归", () => {
    expect(clampHoverDelay(350)).toBe(300);
  });
});

/* ------------------------------- 护边分屏对 / 调谐 / 油门 ------------------------------- */

describe("深化件杂项", () => {
  it("F607 分屏对覆盖：屏对键方向无关，覆盖优先于全局", () => {
    const key = seamPairKey("B", "A");
    expect(key).toBe(seamPairKey("A", "B"));
    const ov = setSeamPairOverride({}, "A", "B", false);
    expect(seamGuardForPair(true, ov, "A", "B")).toBe(false);
    expect(seamGuardForPair(false, {}, "A", "B")).toBe(false);
    expect(seamGuardForPair(true, {}, "A", "B")).toBe(true);
  });

  it("F611 自动调谐：高抖动样本推荐强档，干净样本推荐关", () => {
    const jittery = Array.from({ length: 40 }, (_, i) => ({ dx: (i % 2 ? 1 : -1) * 1.5, dy: 0 }));
    expect(autoTuneTremor(jittery).recommended).toBe("strong");
    const clean = Array.from({ length: 40 }, (_, i) => ({ dx: i * 5, dy: 0 }));
    expect(autoTuneTremor(clean).recommended).toBe("off");
    expect(autoTuneTremor([]).recommended).toBe("off");
  });

  it("F604 油门爬升：起步柔和、400ms 满速", () => {
    expect(autoscrollRamp(0)).toBe(0);
    expect(autoscrollRamp(200)).toBe(0.5);
    expect(autoscrollRamp(1000)).toBe(1);
  });

  it("F616 前台挂载：trackCurrentApp 记录当前应用并返回档案", () => {
    j1Store.reset();
    expect(trackCurrentApp("app-x")).toBeNull();
    expect((j1Store.get("appProfiles").currentApp as string)).toBe("app-x");
  });

  it("F614 导入校验：非法条目列出、合法条目收录", () => {
    const good = { id: "g", name: "好鼠", deviceKey: "k", params: { sens: 1, curve: "linear", wheelMode: "notch" }, createdAt: 0, lastUsedAt: 0 };
    const bad = { id: "", name: "坏鼠", deviceKey: "", params: {} };
    const r = importDeviceProfiles([good, bad]);
    expect(r.accepted).toHaveLength(1);
    expect(r.rejected[0]!.name).toBe("坏鼠");
    expect(() => importDeviceProfiles("nope")).toThrow(/不是数组/);
  });

  it("F615 冲突审计：快捷键与外部声明撞车即报", () => {
    const cfgSide = { global: { "3": { kind: "shortcut", keys: "Ctrl+Shift+B" } }, apps: {} };
    const conflicts = sideButtonConflicts(cfgSide as never, [{ key: "K", action: "折叠代码块 Ctrl+Shift+B" }]);
    expect(conflicts).toHaveLength(1);
    expect(conflicts[0]!.key).toBe("XButton1");
  });

  it("F601 示例轨迹与平滑度：线性曲线平滑度≈0", () => {
    const cfgC = J1_DEFAULTS.curve as unknown as { id: "linear"; sens: number; cp1x: number; cp1y: number; cp2x: number; cp2y: number };
    const trace = simulateTrace("linear", { ...cfgC, sens: 1 });
    expect(trace).toHaveLength(40);
    expect(trace.every((t) => Math.abs(t.outPx - t.inPx) < 0.01)).toBe(true);
    expect(curveSmoothness("linear", { ...cfgC, sens: 1 })).toBe(0);
    expect(SENS_PRESETS).toHaveLength(3);
  });

  it("F616 增量切换回归钉", () => {
    const next = applyIncremental({ sens: 1, curve: "classic", wheelMode: "per-app" }, { curve: "linear" });
    expect(next).toEqual({ sens: 1, curve: "linear", wheelMode: "per-app" });
  });
});
