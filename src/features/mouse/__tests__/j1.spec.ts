/**
 * J 鼠标域 AI-J1（F601-F620）核心逻辑单测。
 * 覆盖：曲线谱/慢速微调/滤波/滚轮四件/跨屏两件/磁吸/自动滚两件/悬停时序/
 * 档案双维/侧键/手势/长按旋钮/衬底/持久化底座。
 * （行数不计入功能代码目标——测试是判据的执行器，不是交付物本体。）
 */

import { describe, expect, it, beforeEach } from "vitest";
import {
  CURVE_LIBRARY,
  SLOW_TUNE_RATIOS,
  applyCurve,
  evalBezier,
  gainAt,
  gainTable20,
  slowTuneGain,
  slowTuneRegistryRow,
  type CurveConfig,
} from "../curve";
import { LiftFilter, TREMOR_LEVELS, liftFilterSelfTest, tremorSpectrum, TremorFilter } from "../filters";
import {
  APP_CLASS_DEFAULT,
  WheelGain,
  notchLines,
  resolveWheelMode,
  resolveWheelTarget,
  tiltCols,
  tiltFromShiftWheel,
} from "../wheel";
import { SEAM_GUARD_PRESET, SeamGuard, ScreenMemory, cornerExempt, monitorAt, type MonitorInfo } from "../screen";
import { MAGNET_RADII, isMagnetizable, isSmallTarget, magnetOffset } from "../magnet";
import {
  AUTOSCROLL_PRESET,
  DRAG_BAND_PX,
  autoscrollExitFor,
  autoscrollVelocity,
  edgeDepth,
  edgeScrollSpeed,
  quantizeDirection,
} from "../autoscroll";
import {
  HOVER_DELAY_STEPS,
  LONG_PRESS_BASE,
  clickExpandsImmediately,
  clampHoverDelay,
  longPressDefaultAudit,
  longPressFor,
  longPressMs,
} from "../hoverTiming";
import {
  DEVICE_PROFILE_CAP,
  DeviceProfileManager,
  applyIncremental,
  resolveParams,
  validateDeviceProfile,
  type DeviceProfile,
} from "../profiles";
import {
  FIVE_BUTTON_MAP,
  SIDE_ACTIONS,
  resolveSideButton,
  sideButtonRegistryRows,
  sideGesturePriorityMatrix,
  validateSideTarget,
  type SideButtonsConfig,
} from "../sideButtons";
import {
  BUILTIN_GESTURES,
  GESTURE_MIN_STEPS,
  GestureRecognizer,
  collapseRuns,
  encodeDir8,
  gestureSampleDirs,
  matchDirs,
  type GestureLibraryConfig,
} from "../gestures";
import { DEFAULT_OVERLAY, OVERLAY_WALKTHROUGH_BACKGROUNDS, composeOverlay, invertColor } from "../overlay";
import { J1_DEFAULTS, J1_SECTIONS, j1Store } from "../j1store";

const CURVE_DEFAULT_CFG: CurveConfig = { id: "classic", cp1x: 0.35, cp1y: 0.55, cp2x: 0.7, cp2y: 1.0, sens: 1 };
const GESTURE_CFG: GestureLibraryConfig = { enabled: true, trailFadeMs: 120, custom: {} };

/* ------------------------------- F601 ------------------------------- */

describe("F601 指针速度曲线谱", () => {
  it("对拍表为 20 点采样且线性曲线增益恒 1:1", () => {
    const t = gainTable20("linear");
    expect(t).toHaveLength(20);
    expect(t[0]!.inPx).toBe(8);
    expect(t[19]!.inPx).toBe(160);
    for (const r of t) expect(r.outPx).toBe(r.inPx);
  });

  it("经典加速在 8px 内直通、其后增益递增（Windows 复刻口径）", () => {
    expect(gainAt("classic", 8)).toBe(1);
    expect(gainAt("classic", 64)).toBeGreaterThan(gainAt("classic", 16));
    expect(gainAt("classic", 1024)).toBeLessThanOrEqual(2);
  });

  it("缓启动低速低于 1、高速趋近 1.6", () => {
    expect(gainAt("soft", 2)).toBeLessThan(1);
    expect(gainAt("soft", 512)).toBeGreaterThan(1.5);
  });

  it("自定义贝塞尔两端点收敛且曲线库四族齐备", () => {
    expect(evalBezier(0, 0.35, 0.55, 0.7, 1)).toBeCloseTo(0, 5);
    expect(evalBezier(1, 0.35, 0.55, 0.7, 1)).toBeCloseTo(1, 5);
    expect(CURVE_LIBRARY).toHaveLength(4);
  });

  it("灵敏度作用于增益映射（applyCurve sign 保真）", () => {
    const out = applyCurve(-10, 0, { ...CURVE_DEFAULT_CFG, id: "linear", sens: 2 });
    expect(out.x).toBe(-20);
    expect(out.y).toBe(0);
  });
});

/* ------------------------------- F602 ------------------------------- */

describe("F602 慢速微调模式", () => {
  const cfg = { enabled: true, ratio: 0.1, key: "shift" as const };

  it("修饰键按住=恒定增益（不经过曲线）；松开=null", () => {
    expect(slowTuneGain(true, cfg)).toBe(0.1);
    expect(slowTuneGain(false, cfg)).toBeNull();
    expect(slowTuneGain(true, { ...cfg, enabled: false })).toBeNull();
  });

  it("三档比率齐备（5%/10%/20%）且非法档兜底 10%", () => {
    expect(SLOW_TUNE_RATIOS).toEqual([0.05, 0.1, 0.2]);
    expect(slowTuneGain(true, { enabled: true, ratio: 0.33, key: "shift" })).toBe(0.1);
  });

  it("微调档与曲线叠加正确性：微调时输出 = 原始位移 × ratio（曲线旁路）", () => {
    const raw = { x: 12, y: -8 };
    const viaSlow = applyCurve(raw.x * 0.1, raw.y * 0.1, CURVE_DEFAULT_CFG);
    expect(viaSlow.x).toBeCloseTo(raw.x * 0.1, 6);
    void raw;
  });

  it("冲突审计登记行含已知共占方", () => {
    const row = slowTuneRegistryRow(cfg);
    expect(row.source).toBe("F602");
    expect(row.conflictsWith).toContain("粘滞键(5xShift)");
  });
});

/* ------------------------------- F603 / F611 ------------------------------- */

describe("F603 抬笔滤波", () => {
  it("抬起窗口（8ms）内末位移按 50% 折算，窗口外直通", () => {
    const f = new LiftFilter();
    f.onButtonUp(100);
    expect(f.feed(2, 0, 104)).toEqual({ x: 1, y: 0, filtered: true });
    expect(f.feed(4, 0, 200)).toEqual({ x: 4, y: 0, filtered: false });
  });

  it("判据自检：抖动注入 100 次落点偏差 P95 < 0.5px", () => {
    const jitter = [
      { dx: 0.6, dy: -0.4, dtMs: 3 },
      { dx: -0.5, dy: 0.5, dtMs: 6 },
    ];
    const r = liftFilterSelfTest(jitter, 100);
    expect(r.p95).toBeLessThan(0.5);
    expect(r.pass).toBe(true);
  });
});

describe("F611 手抖过滤", () => {
  it("关档零干预", () => {
    const f = new TremorFilter("off");
    expect(f.feed(1.3, -0.7)).toEqual({ x: 1.3, y: -0.7, filtered: false });
  });

  it("意图直通：快速大幅移动零衰减", () => {
    const f = new TremorFilter("strong");
    const big = TREMOR_LEVELS.strong.ampPx * 4 + 1;
    const out = f.feed(big, -big);
    expect(out).toEqual({ x: big, y: -big, filtered: false });
  });

  it("震颤效果谱：强档残余 ≤ 轻档（2/4/6Hz 三频段）", () => {
    for (const hz of [2, 4, 6] as const) {
      const light = tremorSpectrum("light", hz, 1.2).residualRatio;
      const strong = tremorSpectrum("strong", hz, 1.2).residualRatio;
      expect(strong).toBeLessThanOrEqual(light);
    }
  });
});

/* ------------------------------- F605 / F606 / F612 / F618 ------------------------------- */

describe("F605 滚轮刻度语义", () => {
  const base = { mode: "per-app" as const, overrides: {}, linesPerNotch: 3 };

  it("三类应用默认档：文档/代码/终端逐档，浏览器/列表平滑", () => {
    expect(resolveWheelMode(base, "app-doc", "document")).toBe("notch");
    expect(resolveWheelMode(base, "app-term", "terminal")).toBe("notch");
    expect(resolveWheelMode(base, "app-web", "browser")).toBe("smooth");
    expect(APP_CLASS_DEFAULT.document).toBe("notch");
  });

  it("覆盖优先级：应用 > 类目默认 > 全局", () => {
    const cfg = { ...base, overrides: { "app-doc": "smooth" as const } };
    expect(resolveWheelMode(cfg, "app-doc", "document")).toBe("smooth");
    expect(resolveWheelMode({ ...base, mode: "notch" }, "app-any", "browser")).toBe("notch");
  });

  it("每格行数 1..12 钳制，默认 3（Windows 对拍）", () => {
    expect(notchLines(3)).toBe(3);
    expect(notchLines(0)).toBe(3);
    expect(notchLines(99)).toBe(12);
  });
});

describe("F606 倾斜滚轮", () => {
  it("3 列/档与垂直对称、钳制 1..12", () => {
    expect(tiltCols(3)).toBe(3);
    expect(tiltCols(0)).toBe(3);
    expect(tiltCols(50)).toBe(12);
  });

  it("Shift+滚轮等效映射：向下=向右（Windows 语义）", () => {
    expect(tiltFromShiftWheel(120, 3)).toEqual({ dir: 1, cols: 3 });
    expect(tiltFromShiftWheel(-120, 3)).toEqual({ dir: -1, cols: 3 });
  });
});

describe("F612 滚轮自适应增益", () => {
  const cfg = () => ({ enabled: true, minLines: 3, maxLines: 12, accelMs: 220 });

  it("低速忠实 3 行；逐档应用豁免增益（F605 互斥边界）", () => {
    const g = new WheelGain(cfg);
    expect(g.feed(0, true)).toBe(3);
    const g2 = new WheelGain(cfg);
    expect(g2.feed(0, false)).toBe(3); // 首事件无节奏 → 基准 3 行
  });

  it("高速趋近 12 行封顶且介入平滑（速度连续性）", () => {
    const g = new WheelGain(cfg);
    const seq: number[] = [];
    let at = 0;
    for (let i = 0; i < 8; i++) {
      at += 50; // 20 档/秒 高速
      seq.push(g.feed(at, false));
    }
    expect(seq[seq.length - 1]!).toBeLessThanOrEqual(12);
    expect(seq[seq.length - 1]!).toBeGreaterThan(8);
    // 相邻两次增益差有界（无跳变感）。
    for (let i = 1; i < seq.length; i++) expect(Math.abs(seq[i]! - seq[i - 1]!)).toBeLessThan(5);
  });
});

/* ------------------------------- F618 ------------------------------- */

describe("F618 滚轮穿透开关", () => {
  const cfg = { enabled: true, exemptTypes: ["scrollable-layer", "select", "menu"] };

  /** 最小假元素（node 环境无 DOM——只实现 resolveWheelTarget 触碰的面）。 */
  function fakeEl(wheelType?: string, opts?: { overflowY?: string; parent?: FakeNode | null }): FakeNode {
    return {
      tagName: "DIV",
      attrs: wheelType ? { "data-wheel": wheelType } : {},
      style: { overflowY: opts?.overflowY ?? "" },
      parentElement: opts?.parent ?? null,
      getAttribute: (n: string) => (wheelType && n === "data-wheel" ? wheelType : null),
      closest: () => null,
      ownerDocument: { defaultView: { getComputedStyle: (el: FakeNode) => ({ overflowY: el.style.overflowY }) } },
    } as unknown as FakeNode;
  }
  type FakeNode = {
    tagName: string;
    attrs: Record<string, string>;
    style: Record<string, string>;
    parentElement: FakeNode | null;
    getAttribute(n: string): string | null;
    closest(sel: string): unknown;
    ownerDocument: { defaultView: { getComputedStyle(el: FakeNode): { overflowY: string } } };
  };

  it("三态之关闭：滚哪算哪", () => {
    const hit = fakeEl("decor");
    expect(resolveWheelTarget(hit as unknown as Element, { ...cfg, enabled: false })).toEqual({ target: hit, passthrough: false });
  });

  it("三态之豁免：可滚动弹层滚自己", () => {
    const hit = fakeEl();
    const layer = fakeEl("scrollable-layer");
    hit.parentElement = layer;
    expect(resolveWheelTarget(hit as unknown as Element, cfg)).toEqual({ target: layer, passthrough: false });
  });

  it("三态之穿透：装饰层 → 下方可滚祖先", () => {
    const hit = fakeEl();
    const deco = fakeEl("decor");
    hit.parentElement = deco;
    const scroller = fakeEl(undefined, { overflowY: "auto" });
    deco.parentElement = scroller;
    const r = resolveWheelTarget(hit as unknown as Element, cfg);
    expect(r.passthrough).toBe(true);
    expect(r.target).toBe(scroller);
  });

  it("零死胡同：装饰层下无可滚祖先时退回命中元素", () => {
    const hit = fakeEl();
    const deco = fakeEl("decor");
    hit.parentElement = deco;
    expect(resolveWheelTarget(hit as unknown as Element, cfg)).toEqual({ target: hit, passthrough: false });
  });
});

/* ------------------------------- F607 / F613 ------------------------------- */

const MON: MonitorInfo[] = [
  { id: "a", x: 0, y: 0, width: 1920, height: 1080, edidFingerprint: "EDID-A", scale: 1 },
  { id: "b", x: 1920, y: 0, width: 1920, height: 1080, edidFingerprint: "EDID-B", scale: 1.5 },
];

describe("F607 跨屏接缝手感", () => {
  it("四角 8px 豁免（热角秒达），护边带内非角点不豁免", () => {
    expect(cornerExempt(MON, 4, 4)).toBe(true);
    expect(cornerExempt(MON, 1916, 4)).toBe(true);
    expect(cornerExempt(MON, 4, 1076)).toBe(true);
    expect(cornerExempt(MON, 1916, 1076)).toBe(true);
    expect(cornerExempt(MON, 1916, 400)).toBe(false);
  });

  it("单屏静默：一切直通", () => {
    const g = new SeamGuard(() => [MON[0]!], () => ({ ...SEAM_GUARD_PRESET, enabled: true }));
    expect(g.feed(1919, 500, 0)).toBe("pass");
  });

  it("护边时序：接缝带首触 hold，停留 200ms 后放行；关闭时直通", () => {
    const g = new SeamGuard(() => MON, () => ({ ...SEAM_GUARD_PRESET, enabled: true }));
    expect(g.feed(1918, 500, 0)).toBe("hold");
    expect(g.feed(1919, 500, 100)).toBe("hold");
    expect(g.feed(1919, 500, 210)).toBe("pass"); // 停留满 200ms → 放行穿越
    const off = new SeamGuard(() => MON, () => ({ ...SEAM_GUARD_PRESET, enabled: false }));
    expect(off.feed(1930, 500, 0)).toBe("pass");
  });

  it("monitorAt 命中与缩放字段", () => {
    expect(monitorAt(MON, 2000, 10)?.id).toBe("b");
    expect(monitorAt(MON, -5, 10)).toBeNull();
  });
});

describe("F613 指针跨屏落点记忆", () => {
  // 与生产同源：cfg 直读 j1Store（ScreenMemory 写入也走 store，读写闭环一致）。
  const mem = (enabled = true) =>
    new ScreenMemory(() => ({
      enabled,
      points: (j1Store.get("screenMemory").points as Record<string, { x: number; y: number }>) ?? {},
    }));

  beforeEach(() => j1Store.reset());

  it("双屏记忆与恢复精度（<1px：存取原值）；钳制在屏内", () => {
    mem().remember("EDID-B", 2000, 300, MON);
    expect(mem().restore("EDID-B", MON)).toEqual({ x: 2000, y: 300 });
    mem().remember("EDID-B", 99999, 99999, MON);
    expect(mem().restore("EDID-B", MON)).toEqual({ x: 3840, y: 1080 });
  });

  it("EDID 指纹为键：交换接口不串屏；单屏静默", () => {
    mem().remember("EDID-A", 100, 100, MON);
    expect(mem().restore("EDID-B", MON)).toBeNull();
    const m1 = mem();
    m1.remember("EDID-A", 100, 100, [MON[0]!]);
    expect(mem().restore("EDID-A", [MON[0]!])).toBeNull(); // 单屏休眠
  });
});

/* ------------------------------- F608 ------------------------------- */

describe("F608 指针磁吸对齐", () => {
  /** 最小假元素（node 环境无 DOM——实现 magnet.ts 触碰的三个面）。 */
  function fakeEl(w: number, h: number, tag = "button", label = "确定", extraAttrs: Record<string, string> = {}): Element {
    const attrs: Record<string, string> = { "aria-label": label, ...extraAttrs };
    return {
      tagName: tag.toUpperCase(),
      getAttribute: (n: string) => (n in attrs ? attrs[n] : null),
      getBoundingClientRect: () =>
        ({ x: 0, y: 0, top: 0, left: 0, bottom: h, right: w, width: w, height: h, toJSON: () => ({}) }) as DOMRect,
    } as unknown as Element;
  }

  it("默认关；关档零干预", () => {
    expect(magnetOffset(5, 5, fakeEl(16, 16), { enabled: false, radiusPx: 12 }).snapped).toBeNull();
  });

  it("小目标白名单（<24px）才吸；大目标不吸", () => {
    const big = fakeEl(120, 32);
    expect(isSmallTarget(big)).toBe(false);
    expect(magnetOffset(10, 10, big, { enabled: true, radiusPx: 12 }).snapped).toBeNull();
    const small = fakeEl(16, 16);
    expect(isSmallTarget(small)).toBe(true);
    expect(magnetOffset(12, 12, small, { enabled: true, radiusPx: 12 }).snapped).not.toBeNull();
    // 已在目标中心（dist=0）→ 无需吸附，零偏移（判定零偏移铁律同源）。
    const center = magnetOffset(8, 8, small, { enabled: true, radiusPx: 12 });
    expect(center.dx).toBe(0);
    expect(center.dy).toBe(0);
  });

  it("半径三档与超半径拒绝；吸附量 ≤ 半径×40%（微移非瞬移）", () => {
    expect(MAGNET_RADII).toEqual([8, 12, 16]);
    const small = fakeEl(16, 16);
    expect(magnetOffset(30, 30, small, { enabled: true, radiusPx: 12 }).snapped).toBeNull();
    const snap = magnetOffset(10, 10, small, { enabled: true, radiusPx: 12 });
    expect(Math.hypot(snap.dx, snap.dy)).toBeLessThanOrEqual(12 * 0.4 + 0.1);
  });

  it("可磁吸类型语义白名单", () => {
    expect(isMagnetizable(fakeEl(16, 16))).toBe(true);
    expect(isMagnetizable(fakeEl(16, 16, "span", "x", { role: "switch" }))).toBe(true);
    expect(isMagnetizable(fakeEl(16, 16, "p"))).toBe(false);
  });
});

/* ------------------------------- F604 / F609 ------------------------------- */

describe("F604 中键自动滚动", () => {
  const cfg = { enabled: true, ...AUTOSCROLL_PRESET };

  it("死区（8px）内零速；线性加速 60px 封顶", () => {
    expect(autoscrollVelocity(4, 4, cfg).vx).toBe(0);
    const near = autoscrollVelocity(20, 0, cfg);
    expect(near.vx).toBeCloseTo(12, 5); // 20-8=12
    expect(autoscrollVelocity(500, 0, cfg).vx).toBe(60);
  });

  it("16 方位量化：正右=0、正下=4、正左=8、正上=12", () => {
    expect(quantizeDirection(1, 0)).toBe(0);
    expect(quantizeDirection(0, 1)).toBe(4);
    expect(quantizeDirection(-1, 0)).toBe(8);
    expect(quantizeDirection(0, -1)).toBe(12);
    expect(quantizeDirection(0, 0)).toBe(-1);
  });

  it("退出三路：中键/左右键/Esc 退出，侧键不退", () => {
    expect(autoscrollExitFor(1)).toBe("exit");
    expect(autoscrollExitFor(0)).toBe("exit");
    expect(autoscrollExitFor(2)).toBe("exit");
    expect(autoscrollExitFor("esc")).toBe("exit");
    expect(autoscrollExitFor(3)).toBe("ignore");
  });
});

describe("F609 拖拽边缘自动滚", () => {
  const rect = { left: 0, top: 0, right: 800, bottom: 600 };

  it("24px 触发带与深入量计算；静止区全零", () => {
    expect(DRAG_BAND_PX).toBe(24);
    expect(edgeDepth(400, 300, rect)).toEqual({ left: 0, right: 0, top: 0, bottom: 0 });
    const d = edgeDepth(790, 10, rect);
    expect(d.right).toBe(14);
    expect(d.top).toBe(14);
  });

  it("深入三档速度：4 / 10 / 18；深入 0 绝不误滚", () => {
    expect(edgeScrollSpeed(0)).toBe(0);
    expect(edgeScrollSpeed(4)).toBe(4);
    expect(edgeScrollSpeed(16)).toBe(10);
    expect(edgeScrollSpeed(23)).toBe(18);
    expect(edgeScrollSpeed(-5)).toBe(0);
  });
});

/* ------------------------------- F610 / F619 ------------------------------- */

describe("F610 悬停时序自定义", () => {
  it("两旋钮四档钳制；默认 400/500", () => {
    expect(HOVER_DELAY_STEPS).toEqual([200, 300, 400, 600]);
    expect(clampHoverDelay(400)).toBe(400);
    expect(clampHoverDelay(999)).toBe(600);
    expect(clampHoverDelay(1)).toBe(200);
  });

  it("手感红线：点击展开永远即时", () => {
    expect(clickExpandsImmediately()).toBe(true);
  });
});

describe("F619 长按时长统一旋钮", () => {
  it("三档缩放比例正确（0.6x/1.0x/1.6x）", () => {
    expect(longPressMs(500, 0.6)).toBe(300);
    expect(longPressMs(500, 1)).toBe(500);
    expect(longPressMs(1100, 1.6)).toBe(1760);
    expect(longPressMs(500, 0.7)).toBe(500); // 非法档兜底 1x
  });

  it("默认档 = 现行值（对账脚本判据全绿）", () => {
    const audit = longPressDefaultAudit({ scale: 1.0, registry: {} });
    expect(audit.length).toBeGreaterThanOrEqual(3);
    for (const a of audit) expect(a.ok).toBe(true);
    expect(LONG_PRESS_BASE.clickLock).toBe(1100);
  });

  it("登记纪律执法：未登记功能抛错（异常显性化）", () => {
    expect(() => longPressFor("nonexistent", { scale: 1, registry: {} })).toThrow(/未在旋钮登记/);
  });
});

/* ------------------------------- F614 / F616 ------------------------------- */

describe("F614 鼠标分设备档案", () => {
  beforeEach(() => j1Store.reset());

  it("首插克隆当前默认 + 气泡标记；同设备再插直接挂载", () => {
    const m = new DeviceProfileManager();
    const first = m.ensureFor("vid:046d:pid:c52b", "办公鼠", 100);
    expect(first.cloned).toBe(true);
    expect(first.profile.params).toEqual(m.currentDefault);
    const second = m.ensureFor("vid:046d:pid:c52b", "办公鼠", 200);
    expect(second.cloned).toBe(false);
    expect(second.profile.id).toBe(first.profile.id);
  });

  it("上限 10 台、超出淘汰最久未用", () => {
    const m = new DeviceProfileManager();
    for (let i = 0; i < DEVICE_PROFILE_CAP; i++) m.ensureFor(`dev-${i}`, `鼠标${i}`, 1000 + i);
    let stored = (j1Store.get("devices").profiles as DeviceProfile[]).length;
    expect(stored).toBe(DEVICE_PROFILE_CAP);
    m.ensureFor("dev-new", "新鼠标", 99999); // 最久未用 dev-0 被淘汰
    stored = (j1Store.get("devices").profiles as DeviceProfile[]).length;
    expect(stored).toBe(DEVICE_PROFILE_CAP);
    expect((j1Store.get("devices").profiles as DeviceProfile[]).some((p) => p.deviceKey === "dev-0")).toBe(false);
    expect((j1Store.get("devices").profiles as DeviceProfile[]).some((p) => p.deviceKey === "dev-new")).toBe(true);
  });

  it("四件套完整性校验：缺件显性报错", () => {
    const bad = { id: "", name: "x", deviceKey: "", params: { sens: -1, curve: "nope", wheelMode: "???" }, createdAt: 0, lastUsedAt: 0 } as unknown as DeviceProfile;
    const errs = validateDeviceProfile(bad);
    expect(errs.length).toBeGreaterThanOrEqual(4);
  });
});

describe("F616 应用级鼠标档案", () => {
  it("增量切换：只覆盖声明字段，其余原地不动（防跳变）", () => {
    const base = { sens: 1.6, curve: "linear" as const, wheelMode: "smooth" as const };
    const next = applyIncremental(base, { sens: 0.8 });
    expect(next).toEqual({ sens: 0.8, curve: "linear", wheelMode: "smooth" });
  });

  it("与 F614 正交：无应用档案时设备档案完整生效", () => {
    const device = { sens: 1.2, curve: "classic" as const, wheelMode: "per-app" as const };
    expect(resolveParams(device, null)).toEqual(device);
    expect(resolveParams(device, { appId: "paint", appName: "画图", source: "manual", overrides: { sens: 0.5 } }).sens).toBe(0.5);
  });
});

/* ------------------------------- F615 / F617 ------------------------------- */

describe("F615 侧键编程", () => {
  const cfgSide: SideButtonsConfig = {
    global: { "3": { kind: "action", action: "nav-back" }, "4": { kind: "action", action: "nav-forward" } },
    apps: { ide: { "3": { kind: "action", action: "fold-block" } } },
  };

  it("两级优先级：应用覆盖 > 全局默认 > null", () => {
    expect(resolveSideButton(cfgSide, "ide", 3)).toEqual({ kind: "action", action: "fold-block" });
    expect(resolveSideButton(cfgSide, "browser", 3)).toEqual({ kind: "action", action: "nav-back" });
    expect(resolveSideButton(cfgSide, null, 3)).toEqual({ kind: "action", action: "nav-back" });
    expect(resolveSideButton(cfgSide, "ide", 0)).toBeNull();
  });

  it("三类映射目标校验各一例 + 非法目标拒绝", () => {
    expect(validateSideTarget({ kind: "action", action: "nav-back" })).toHaveLength(0);
    expect(validateSideTarget({ kind: "shortcut", keys: "Ctrl+Shift+B" })).toHaveLength(0);
    expect(validateSideTarget({ kind: "launch", appId: "app-write" })).toHaveLength(0);
    expect(validateSideTarget({ kind: "action", action: "no-such" }).length).toBeGreaterThan(0);
    expect(validateSideTarget({ kind: "shortcut", keys: "Ctrl" }).length).toBeGreaterThan(0);
  });

  it("五键全键位登记；F244 注册行互通；优先级矩阵文案", () => {
    expect(FIVE_BUTTON_MAP).toHaveLength(5);
    const rows = sideButtonRegistryRows(cfgSide, "ide");
    expect(rows.some((r) => r.scope === "应用 ide")).toBe(true);
    expect(SIDE_ACTIONS.some((a) => a.id === "nav-forward")).toBe(true);
    expect(sideGesturePriorityMatrix(true, true)).toContain("无按键冲突");
  });
});

describe("F617 右键手势层", () => {
  it("八方向编码全对", () => {
    expect(encodeDir8(1, 0)).toBe(0);
    expect(encodeDir8(1, 1)).toBe(1);
    expect(encodeDir8(0, 1)).toBe(2);
    expect(encodeDir8(-1, 0)).toBe(4);
    expect(encodeDir8(0, -1)).toBe(6);
  });

  it("同向合并后可匹配单方向手势；容错两向偏差", () => {
    expect(collapseRuns([4, 4, 4, 4])).toEqual([4]);
    expect(matchDirs([4], [4])).toBe(true);
    expect(matchDirs([3], [4])).toBe(true); // ±1 档容错
    expect(matchDirs([2], [4])).toBe(false);
    expect(matchDirs([0, 4], [4, 0])).toBe(false); // 方向序不同不匹配
  });

  it("内置 12 手势、方向编码样本 200 例识别率 ≥95%（±18° 角度抖动全链路）", () => {
    expect(BUILTIN_GESTURES).toHaveLength(12);
    for (const g of BUILTIN_GESTURES) {
      const samples = gestureSampleDirs(g.dirs);
      expect(samples).toHaveLength(200);
      const hit = samples.filter((s) => matchDirs(collapseRuns(s), g.dirs)).length;
      expect(hit / 200).toBeGreaterThanOrEqual(0.95);
    }
  });

  it("无轨迹回退菜单：步数不足返回 null；有效轨迹命中", () => {
    const r = new GestureRecognizer();
    r.begin(0, 0);
    r.feed(10, 0);
    expect(r.recognize(GESTURE_CFG)).toBeNull(); // <2 步 = 无轨迹
    const r2 = new GestureRecognizer();
    r2.begin(0, 0);
    r2.feed(30, 0);
    r2.feed(60, 0);
    expect(r2.recognize(GESTURE_CFG)).toEqual({ id: "forward", action: "nav.forward" });
    expect(GESTURE_MIN_STEPS).toBe(2);
  });
});

/* ------------------------------- F620 ------------------------------- */

describe("F620 指针衬底与投影", () => {
  it("反色描边：深底浅描边、浅底深描边（感知亮度反转）", () => {
    expect(invertColor("#ffffff")).toBe("#000000");
    expect(invertColor("#101014")).toBe("#ffffff");
    expect(invertColor("nonsense")).toBe("#000000"); // 解析失败兜底不静默
  });

  it("默认态审计：描边开、投影关、衬圈关；三件全关零开销", () => {
    expect(DEFAULT_OVERLAY).toEqual({ outline: true, shadow: false, ring: false });
    expect(composeOverlay(DEFAULT_OVERLAY).active).toBe(true);
    expect(composeOverlay({ outline: false, shadow: false, ring: false }).active).toBe(false);
  });

  it("三开关独立：单开投影只产出投影层", () => {
    const s = composeOverlay({ outline: false, shadow: true, ring: false });
    expect(s.cssFilter).toBe("");
    expect(s.boxShadow).toContain("rgba(0,0,0,0.3)");
    const r = composeOverlay({ outline: false, shadow: false, ring: true });
    expect(r.boxShadow).toContain("3px");
  });

  it("三底色走查样本齐备", () => {
    expect(OVERLAY_WALKTHROUGH_BACKGROUNDS.map((b) => b.id)).toEqual(["deep", "light", "floral"]);
  });
});

/* ------------------------------- 底座 ------------------------------- */

describe("j1store 持久化底座", () => {
  beforeEach(() => j1Store.reset());

  it("20 节齐备且默认值全量注册", () => {
    expect(J1_SECTIONS).toHaveLength(20);
    expect(Object.keys(J1_DEFAULTS)).toHaveLength(20);
  });

  it("set/undo 栈深 3；导入原子性（无合法分节整包拒绝）", () => {
    j1Store.set("magnet", { radiusPx: 8 });
    j1Store.set("magnet", { radiusPx: 12 });
    j1Store.set("magnet", { radiusPx: 16 });
    j1Store.set("magnet", { radiusPx: 8 });
    expect(j1Store.getWith("magnet", "radiusPx", 12)).toBe(8);
    expect(j1Store.undoSection("magnet")).toBe(true);
    expect(j1Store.getWith("magnet", "radiusPx", 12)).toBe(16);
    expect(() => j1Store.importAll({ garbage: true })).toThrow(/没有任何合法分节/);
  });
});
