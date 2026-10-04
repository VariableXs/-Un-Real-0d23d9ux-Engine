/**
 * J1 深化批次三（v3）单测：动作路由中心 / 窗口无关运行时内核纯函数面 /
 * F616 生效参数通道 / F614 首交互建档 / 生命周期登记。
 *
 * 环境口径：node 环境（同 j1/j1deep 纪律）——DOM 依赖面只测纯函数；
 * createWindowRuntime 的挂载/退订用最小 hand-rolled DOM stub 做冒烟。
 */

import { describe, expect, it, beforeEach, vi, afterEach } from "vitest";
import {
  J1_ACTION_REGISTRY,
  _resetActionRegistryForTest,
  actionHandlerSnapshot,
  actionScopeOf,
  dispatchJ1Action,
  findActionMeta,
  installBuiltinHandlers,
  registerActionHandler,
} from "../actions";
import {
  LINE_HEIGHT,
  activeRuntimeSnapshot,
  createWindowRuntime,
  hoverDelayVars,
  normalizeWheelDelta,
  tiltFromWheelEvent,
  toVirtualPoint,
  tauriMonitorToInfo,
} from "../windowRuntime";
import {
  DeviceProfileManager,
  PRIMARY_DEVICE_KEY,
  deviceParamsOverride,
  effectiveParamsFor,
  ensurePrimaryDevice,
  upsertAppProfile,
  removeAppProfile,
  type DeviceProfile,
} from "../profiles";
import { j1Store, J1_DEFAULTS } from "../j1store";
import { j1Telemetry } from "../telemetry";
import { BUILTIN_GESTURES } from "../gestures";
import { SIDE_ACTIONS } from "../sideButtons";
import { HOVER_TOOLTIP_DEFAULT } from "../hoverTiming";

beforeEach(() => {
  j1Store.reset();
  _resetActionRegistryForTest();
});

/* ------------------------------- 动作登记表 ------------------------------- */

describe("F615/F617 动作登记表（一处一事实）", () => {
  it("登记表 = 12 手势动作 + 5 侧键动作，动作名全仓唯一", () => {
    expect(BUILTIN_GESTURES).toHaveLength(12);
    expect(SIDE_ACTIONS).toHaveLength(5);
    expect(J1_ACTION_REGISTRY).toHaveLength(17);
    const names = J1_ACTION_REGISTRY.map((a) => a.action);
    expect(new Set(names).size).toBe(names.length);
  });

  it("作用域映射按前缀裁决（nav./view./edit./window.→sys 兜底）", () => {
    expect(actionScopeOf("nav.back")).toBe("nav");
    expect(actionScopeOf("view.refresh")).toBe("view");
    expect(actionScopeOf("edit.copy")).toBe("edit");
    expect(actionScopeOf("window.close-tab")).toBe("window");
    expect(actionScopeOf("mute")).toBe("sys");
    expect(actionScopeOf("custom.zigzag")).toBe("sys");
  });

  it("findActionMeta：内置命中、未登记动作返回 null（不猜）", () => {
    expect(findActionMeta("edit.paste")?.via).toBe("gesture");
    expect(findActionMeta("taskview")?.via).toBe("side");
    expect(findActionMeta("custom.nothing")).toBeNull();
  });
});

/* ------------------------------- 两级路由 ------------------------------- */

describe("动作路由（应用作用域 > 全局 > 未处理显性化）", () => {
  it("应用作用域处理器优先命中；无应用档案时全局兜底", async () => {
    const hits: string[] = [];
    registerActionHandler("nav.back", () => void hits.push("global"));
    registerActionHandler("nav.back", () => void hits.push("app"), { appId: "app-code" });

    await dispatchJ1Action("nav.back", "side", "app-code");
    expect(hits).toEqual(["app"]);

    hits.length = 0;
    await dispatchJ1Action("nav.back", "side", "app-write");
    expect(hits).toEqual(["global"]);
  });

  it("处理器返回 false = 显式让位 → 继续向下路由", async () => {
    const hits: string[] = [];
    registerActionHandler("edit.copy", () => false, { appId: "app-write" });
    registerActionHandler("edit.copy", () => void hits.push("global"));
    const r = await dispatchJ1Action("edit.copy", "gesture", "app-write");
    expect(hits).toEqual(["global"]);
    expect(r).toEqual({ handled: true, via: "global" });
  });

  it("处理器抛错被隔离（扩展崩溃不拖垮本体），后续处理器照常接住", async () => {
    const hits: string[] = [];
    registerActionHandler("nav.up", () => {
      throw new Error("boom");
    });
    registerActionHandler("nav.up", () => void hits.push("second"));
    const r = await dispatchJ1Action("nav.up", "gesture", null);
    expect(hits).toEqual(["second"]);
    expect(r.handled).toBe(true);
  });

  it("无人处理：handled=false + 遥测 no-feedback（异常零静默）", async () => {
    j1Telemetry.clear();
    const r = await dispatchJ1Action("taskview", "gesture", "app-mind");
    expect(r).toEqual({ handled: false, via: "unhandled" });
    expect(j1Telemetry.size).toBe(1);
    expect(j1Telemetry.worstTen().length).toBe(0); // no-feedback 不属于挫败信号（仅事件层）
  });

  it("退订即撤销登记（快照同源）", async () => {
    const off = registerActionHandler("mute", () => true);
    expect(actionHandlerSnapshot()).toHaveLength(1);
    off();
    expect(actionHandlerSnapshot()).toHaveLength(0);
    const r = await dispatchJ1Action("mute", "side", null);
    expect(r.handled).toBe(false);
  });

  it("空 action 注册显性拒绝", () => {
    expect(() => registerActionHandler("   ", () => true)).toThrow(/\[mouse-j1:actions\]/);
  });
});

/* ------------------------------- 别名与内建装配 ------------------------------- */

describe("别名语义与内建处理器装配", () => {
  it("shortcut:/launch: 别名走 alias 通道（handled=true，不进处理器路由）", async () => {
    const r1 = await dispatchJ1Action("shortcut:Ctrl+Shift+V", "side", null);
    const r2 = await dispatchJ1Action("launch:app-write", "side", null);
    expect(r1).toEqual({ handled: true, via: "alias" });
    expect(r2).toEqual({ handled: true, via: "alias" });
  });

  it("内建装配：默认集（编辑三兄弟/声明式 nav.up/事件化三件）全登记", () => {
    const w = installBuiltinHandlers({});
    expect(w.wired).toEqual(
      expect.arrayContaining(["edit.copy", "edit.cut", "edit.paste", "nav.up", "file.new", "window.close-tab", "taskview", "desktop-toggle", "mute"]),
    );
    expect(w.wired).not.toContain("window.minimize"); // 无 Tauri 句柄不装配窗口管理
    w.dispose();
    expect(actionHandlerSnapshot()).toHaveLength(0);
  });

  it("提供 onNav/onRefresh/tauriWindow 时对应动作补入装配清单", () => {
    const w = installBuiltinHandlers({
      onNav: () => {},
      onRefresh: () => {},
      tauriWindow: { minimize: async () => {}, toggleMaximize: async () => {} },
    });
    expect(w.wired).toEqual(expect.arrayContaining(["nav.back", "nav.forward", "view.refresh", "view.refresh-hard", "window.minimize", "window.toggle-max"]));
    expect(w.wired).toHaveLength(15); // 9 基础 + 2 导航 + 2 刷新 + 2 窗口管理
    w.dispose();
  });
});

/* ------------------------------- 窗口内核纯函数面 ------------------------------- */

describe("deltaMode 归一化（行/页/像素 → 行语义）", () => {
  it("像素模式：24px = 1 行；行模式：1 delta = 1 行；页模式按视口高折算", () => {
    expect(normalizeWheelDelta({ deltaX: 0, deltaY: 48, deltaMode: 0 }).linesY).toBeCloseTo(2);
    expect(normalizeWheelDelta({ deltaX: 0, deltaY: 1, deltaMode: 1 }).linesY).toBe(1);
    const page = normalizeWheelDelta({ deltaX: 0, deltaY: 1, deltaMode: 2, viewportHeight: 960 });
    expect(page.linesY).toBeCloseTo(960 / LINE_HEIGHT);
  });

  it("横向分量符号保真（倾斜路径的输入口径）", () => {
    const r = normalizeWheelDelta({ deltaX: -72, deltaY: 0, deltaMode: 0 });
    expect(r.linesX).toBeCloseTo(-3);
  });
});

describe("F606 真实倾斜判定", () => {
  it("纯横向事件按倾斜档映射（3 列/档、方向=deltaX 符号）", () => {
    expect(tiltFromWheelEvent(12, 0, 3)).toEqual({ dir: 1, cols: 3 });
    expect(tiltFromWheelEvent(-12, 0, 3)).toEqual({ dir: -1, cols: 3 });
  });

  it("垂直/斜向/零位移不判倾斜（正常滚轮走 F605/F612 链）", () => {
    expect(tiltFromWheelEvent(0, 12, 3)).toBeNull();
    expect(tiltFromWheelEvent(12, 12, 3)).toBeNull();
    expect(tiltFromWheelEvent(0, 0, 3)).toBeNull();
  });

  it("列数钳制 1..12（非法配置不产生荒谬滚动量）", () => {
    expect(tiltFromWheelEvent(5, 0, 99)?.cols).toBe(12);
    expect(tiltFromWheelEvent(5, 0, 0)?.cols).toBe(3); // 0 → 默认 3
  });
});

describe("F610 变量通道映射（--hover-delay 偏离才接管）", () => {
  it("默认 tooltip 500ms = 基线：不覆盖全局令牌（M-71 零迁移）", () => {
    const v = hoverDelayVars(400, HOVER_TOOLTIP_DEFAULT);
    expect(v.hoverDelayOverride).toBeNull();
    expect(v.menu).toBe("400ms");
  });

  it("偏离基线：tooltip 300ms → --hover-delay=300ms；菜单旋钮独立", () => {
    const v = hoverDelayVars(200, 300);
    expect(v.hoverDelayOverride).toBe("300ms");
    expect(v.menu).toBe("200ms");
  });

  it("非档值收敛到最近档（两旋钮各按各的档位表）", () => {
    // tooltip 550 → 最近档 500（基线在档）；menu 550 → 最近档 600。
    expect(hoverDelayVars(550, 550).tooltip).toBe("500ms");
    expect(hoverDelayVars(550, 550).menu).toBe("600ms");
    expect(hoverDelayVars(550, 550).hoverDelayOverride).toBeNull();
  });
});

describe("物理像素虚拟桌面坐标（Tauri 多屏口径）", () => {
  const realWindow = globalThis.window;

  afterEach(() => {
    vi.unstubAllGlobals();
    (globalThis as { window?: unknown }).window = realWindow;
  });

  it("toVirtualPoint：窗口逻辑偏移×DPR + 窗口内坐标×DPR", () => {
    vi.stubGlobal("window", { screenX: 100, screenY: 50 });
    expect(toVirtualPoint(10, 20, 2)).toEqual({ x: 220, y: 140 });
  });

  it("Tauri Monitor → MonitorInfo（EDID 指纹 = 名称+物理分辨率键）", () => {
    const m = tauriMonitorToInfo({
      name: "DELL-U2723",
      position: { x: 1920, y: 0 },
      size: { width: 2560, height: 1440 },
      scaleFactor: 1.25,
    });
    expect(m).toEqual({
      id: "DELL-U2723",
      x: 1920,
      y: 0,
      width: 2560,
      height: 1440,
      edidFingerprint: "tauri:DELL-U2723:2560x1440",
      scale: 1.25,
    });
  });

  it("字段缺失时保守兜底（不抛错——探针环境差异不炸运行时）", () => {
    const m = tauriMonitorToInfo({});
    expect(m.id).toContain("mon-");
    expect(m.width).toBe(0);
    expect(m.scale).toBe(1);
  });
});

/* ------------------------------- F616 生效参数通道 ------------------------------- */

describe("F616 生效参数（effectiveParamsFor）", () => {
  it("无应用档案：全局曲线配置原样（默认单档案零干预）", () => {
    const out = effectiveParamsFor(null, { sens: 1.4, curve: "linear" });
    expect(out).toEqual({ sens: 1.4, curve: "linear" });
  });

  it("应用档案增量覆盖：只改声明字段，其余原地不动（速度连续性）", () => {
    upsertAppProfile({ appId: "app-fate", appName: "fate", source: "manual", overrides: { sens: 0.6 } });
    const out = effectiveParamsFor("app-fate", { sens: 1.4, curve: "classic" });
    expect(out.sens).toBe(0.6);
    expect(out.curve).toBe("classic");
    removeAppProfile("app-fate");
  });

  it("sens+curve 双声明全生效（应用档案的「性格切换」完整语义）", () => {
    upsertAppProfile({ appId: "app-draw", appName: "draw", source: "declared", overrides: { sens: 0.8, curve: "linear" } });
    const out = effectiveParamsFor("app-draw", { sens: 1.4, curve: "classic" });
    expect(out).toEqual({ sens: 0.8, curve: "linear" });
    removeAppProfile("app-draw");
  });
});

/* ------------------------------- F614 首交互建档 ------------------------------- */

describe("F614 首交互建档（ensurePrimaryDevice）", () => {
  it("首插克隆当前默认 + 返回 cloned；二插不再克隆", () => {
    const dm = new DeviceProfileManager();
    dm.currentDefault = { sens: 1.3, curve: "soft", wheelMode: "per-app" };
    const r1 = ensurePrimaryDevice(dm, 1000);
    expect(r1?.cloned).toBe(true);
    const r2 = ensurePrimaryDevice(dm, 2000);
    expect(r2?.cloned).toBe(false);
    const profiles = (j1Store.get("devices").profiles as DeviceProfile[]) ?? [];
    expect(profiles).toHaveLength(1);
    expect(profiles[0]!.deviceKey).toBe(PRIMARY_DEVICE_KEY);
    expect(profiles[0]!.params.sens).toBe(1.3);
    expect(profiles[0]!.params.curve).toBe("soft");
  });

  it("notifyOnClone 关闭：完全静默（null——建档照常但不通知，气泡开关被尊重）", () => {
    j1Store.set("devices", { notifyOnClone: false });
    const dm = new DeviceProfileManager();
    expect(ensurePrimaryDevice(dm, 1000)).toBeNull(); // 无档案：静默建档，不返回气泡数据
    expect(((j1Store.get("devices").profiles as DeviceProfile[]) ?? [])).toHaveLength(1); // 档案真实创建
    expect(ensurePrimaryDevice(dm, 2000)).toBeNull(); // 有档案：依旧零行为
  });

  it("deviceParamsOverride：建档后 sens/curve 覆盖全局；未建档空表", () => {
    expect(deviceParamsOverride()).toEqual({});
    const dm = new DeviceProfileManager();
    dm.currentDefault = { sens: 0.7, curve: "linear", wheelMode: "notch" };
    ensurePrimaryDevice(dm, 1000);
    expect(deviceParamsOverride()).toEqual({ sens: 0.7, curve: "linear" });
  });
});

/* ------------------------------- 生命周期登记（冒烟） ------------------------------- */

describe("createWindowRuntime 挂载/退订（最小 DOM stub 冒烟）", () => {
  const realWindow = globalThis.window;

  afterEach(() => {
    vi.unstubAllGlobals();
    (globalThis as { window?: unknown }).window = realWindow;
  });

  function stubDom(): void {
    const el = {
      style: { setProperty: vi.fn(), removeProperty: vi.fn() },
      setProperty: vi.fn(),
    };
    vi.stubGlobal("document", {
      documentElement: el,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      visibilityState: "visible",
      elementFromPoint: () => null,
      querySelectorAll: () => [],
    });
    vi.stubGlobal("window", {
      setTimeout,
      clearTimeout,
      setInterval,
      clearInterval,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      innerWidth: 1920,
      innerHeight: 1080,
      screenX: 0,
      screenY: 0,
      devicePixelRatio: 1,
      dispatchEvent: vi.fn(),
    });
  }

  it("挂载即登记、dispose 即注销（接线审计面板的数据源）", () => {
    stubDom();
    const monitors = [
      { id: "a", x: 0, y: 0, width: 1920, height: 1080, edidFingerprint: "tauri:A:1920x1080", scale: 1 },
      { id: "b", x: 1920, y: 0, width: 1920, height: 1080, edidFingerprint: "tauri:B:1920x1080", scale: 1.25 },
    ];
    const rt = createWindowRuntime(
      { entry: "test-win", appScope: "test", appClass: "list", replica: false, monitorsSource: () => monitors },
      {},
    );
    expect(rt.info).toEqual({ entry: "test-win", appScope: "test", appClass: "list", replica: false });
    const snap = activeRuntimeSnapshot();
    expect(snap.some((r) => r.entry === "test-win")).toBe(true);
    rt.dispose();
    expect(activeRuntimeSnapshot().some((r) => r.entry === "test-win")).toBe(false);
  });

  it("双 dispose 幂等（不重复摘监听、不抛错）", () => {
    stubDom();
    const rt = createWindowRuntime(
      { entry: "idem", appScope: "idem", appClass: "document", replica: false, monitorsSource: () => [] },
      {},
    );
    rt.dispose();
    expect(() => rt.dispose()).not.toThrow();
  });

  it("挂载即应用 F610 默认变量（--vx-menu-delay/--vx-tooltip-delay 写入、--hover-delay 不覆盖）", () => {
    stubDom();
    const setProperty = vi.fn();
    const removeProperty = vi.fn();
    vi.stubGlobal("document", {
      documentElement: { style: { setProperty, removeProperty } },
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      visibilityState: "visible",
      elementFromPoint: () => null,
      querySelectorAll: () => [],
    });
    const rt = createWindowRuntime(
      { entry: "timing", appScope: "timing", appClass: "list", replica: false, monitorsSource: () => [] },
      {},
    );
    const wrote = setProperty.mock.calls.map((c) => c[0]);
    expect(wrote).toEqual(expect.arrayContaining(["--vx-menu-delay", "--vx-tooltip-delay", "--vx-longpress-scale"]));
    expect(wrote).not.toContain("--hover-delay"); // 默认 500ms 基线：不接管 M-71 令牌（removeProperty 清残留属正常守卫）
    rt.dispose();
  });
});

/* ------------------------------- 默认值对账 ------------------------------- */

describe("v3 与 J1_DEFAULTS 的同源对账", () => {
  it("曲线默认档（classic 1.0）作为 F614 建档克隆源与 F616 打底一致", () => {
    const dm = new DeviceProfileManager();
    dm.currentDefault = {
      sens: (J1_DEFAULTS.curve as { sens: number }).sens,
      curve: (J1_DEFAULTS.curve as { id: "classic" }).id,
      wheelMode: (J1_DEFAULTS.wheelNotch as { mode: "per-app" }).mode,
    };
    ensurePrimaryDevice(dm, 1000);
    expect(deviceParamsOverride()).toEqual({ sens: 1, curve: "classic" });
  });
});
