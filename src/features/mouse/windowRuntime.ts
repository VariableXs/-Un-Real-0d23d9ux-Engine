/**
 * J 鼠标域 · 窗口无关运行时内核（v3 深化批次三）。
 *
 * v2 的整条管线（滤波→曲线→积分 / 滚轮链 / 侧键 / 手势 / 自动滚 / 护边 / 记忆）
 * 只活在 J1Runtime.tsx（桌面窗 React 组件）里——taskbar/explorer/app-* 八个
 * MPA 窗口的滚轮语义、侧键、慢速微调、手势全部裸奔。本模块把逻辑层从
 * React 里解耦成 createWindowRuntime()：任何窗口给一个入口身份即可挂载，
 * 桌面窗走 React 薄壳（J1Runtime.tsx）+ 副本层渲染，其余窗口 headless 直挂。
 *
 * v3 新接的真实缺口（每条都是判据内的功能，非注水）：
 * - F606 真实倾斜路径：纯横向 wheel 事件（deltaY≈0）按倾斜档语义横向滚动；
 * - F604 滚轮打断：锚标接管期间滚轮输入即退出（Windows 语义补全）；
 * - F607/F613 Tauri 多屏实装：availableMonitors 异步缓存刷新（物理像素虚拟
 *   桌面坐标），护边与落点记忆在多屏真机上真实工作；单屏/浏览器 dev 自然降级；
 * - F614 首次交互建档：first-pointerdown ensureFor(primary) 克隆当前默认 +
 *   气泡回调（notifyOnClone 尊重）；
 * - F616 管线消费：指针增益每次移动经 effectiveParamsFor() 取「设备档案×
 *   应用档案」合成参数——应用档案的 sens/curve 从此真实改变手感；
 * - F610 --hover-delay 通道：tooltip 旋钮偏离 500ms 基线时改写全局悬停延迟
 *   令牌（tooltip.css 真实消费），回 500 即撤销覆盖（默认态零迁移）；
 * - F602 blur 复位：切窗后修饰键状态清零（粘 Shift 不再拖住指针）；
 * - deltaMode 归一化：行/页/像素三种滚轮单位统一到本域行语义。
 */

import { j1Store, type J1Section } from "./j1store";
import { applyCurve, slowTuneGain, type CurveConfig, type SlowTuneKey } from "./curve";
import {
  effectiveParamsFor,
  deviceParamsOverride,
  ensurePrimaryDevice,
  DeviceProfileManager,
  type DeviceProfileParams,
} from "./profiles";
import { LiftFilter, TremorFilter } from "./filters";
import {
  WheelGain,
  resolveWheelMode,
  resolveWheelTarget,
  type WheelNotchConfig,
  type TiltWheelConfig,
  type PassthroughConfig,
  type WheelGainConfig,
} from "./wheel";
import { WheelInertia, inertiaSuspension, INERTIA_DEFAULT, type InertiaConfig } from "./inertia";
import { j1Telemetry } from "./telemetry";
import {
  autoscrollVelocity,
  autoscrollExitFor,
  autoscrollRamp,
  edgeDepth,
  edgeScrollSpeed,
  AUTOSCROLL_ATTR,
  AUTOSCROLL_PRESET,
  type AutoscrollConfig,
  type DragScrollConfig,
} from "./autoscroll";
import { GestureRecognizer, resolveGestureAction, type GestureLibraryConfig } from "./gestures";
import { SeamGuard, ScreenMemory, type MonitorInfo, type SeamGuardConfig, type ScreenMemoryConfig } from "./screen";
import { resolveSideButton, type SideButtonsConfig } from "./sideButtons";
import { clampHoverDelay, clampTooltipDelay, HOVER_TOOLTIP_DEFAULT } from "./hoverTiming";
import { lerpToward } from "./magnet";
import { dispatchJ1Action } from "./actions";
import { logInfo } from "../../lib/logger";

/* ------------------------------- 配置读取 ------------------------------- */

function cfg<T>(section: J1Section, fallback: T): T {
  const s = j1Store.get(section);
  return { ...(fallback as unknown as Record<string, unknown>), ...s } as unknown as T;
}

/* ------------------------------- 纯函数面（测试同源） ------------------------------- */

/** 标准行高（3 行/格对拍 Windows 的基准；列宽同源——倾斜 3 列用同一格律）。 */
export const LINE_HEIGHT = 24;

export interface RawWheelDelta {
  deltaX: number;
  deltaY: number;
  deltaMode: number;
  viewportHeight?: number;
}

/**
 * deltaMode 归一化：0=像素、1=行、2=页——统一折算到「行」语义
 * （跨应用滚轮单位不一致是 F605 刻度语义的天敌，归一后才谈对拍）。
 */
export function normalizeWheelDelta(e: RawWheelDelta): { deltaX: number; deltaY: number; linesX: number; linesY: number } {
  let k = 1;
  if (e.deltaMode === 1) k = LINE_HEIGHT;
  else if (e.deltaMode === 2) k = Math.max(LINE_HEIGHT, e.viewportHeight ?? 800);
  return {
    deltaX: e.deltaX * k,
    deltaY: e.deltaY * k,
    linesX: e.deltaX * k / LINE_HEIGHT,
    linesY: e.deltaY * k / LINE_HEIGHT,
  };
}

/**
 * F606 真实倾斜判定：纯横向事件（|deltaX| 显著、|deltaY|≈0）= 倾斜轮。
 * 返回 null 表示不是倾斜事件（正常垂直/斜向滚轮走 F605/F612 链）。
 * 每个倾斜事件 = 一档：3 列（colsPerNotch，与垂直格律对称）。
 */
export function tiltFromWheelEvent(deltaX: number, deltaY: number, colsPerNotch: number): { dir: -1 | 1; cols: number } | null {
  if (Math.abs(deltaY) >= 1 || Math.abs(deltaX) < 1) return null;
  const cols = Math.max(1, Math.min(12, Math.round(colsPerNotch) || 3));
  return { dir: deltaX >= 0 ? 1 : -1, cols };
}

/**
 * F610 --hover-delay 通道映射：tooltip 旋钮=基线 500ms 时不覆盖全局令牌
 * （AI-18 M-71 的默认档零迁移）；偏离时才改写（tooltip.css 真实消费）。
 * menu 旋钮始终写 J1 自己的 --vx-menu-delay（菜单组件扩展点）。
 */
export function hoverDelayVars(menuMs: number, tooltipMs: number): { menu: string; tooltip: string; hoverDelayOverride: string | null } {
  const t = clampTooltipDelay(tooltipMs);
  return {
    menu: `${clampHoverDelay(menuMs)}ms`,
    tooltip: `${t}ms`,
    hoverDelayOverride: t === HOVER_TOOLTIP_DEFAULT ? null : `${t}ms`,
  };
}

/** 物理像素虚拟桌面坐标（Tauri 多屏口径）：窗口逻辑偏移×DPR + 窗口内坐标×DPR。 */
export function toVirtualPoint(clientX: number, clientY: number, dpr: number): { x: number; y: number } {
  const sx = (typeof window !== "undefined" ? window.screenX : 0) * dpr;
  const sy = (typeof window !== "undefined" ? window.screenY : 0) * dpr;
  return { x: Math.round(sx + clientX * dpr), y: Math.round(sy + clientY * dpr) };
}

/** Tauri Monitor → J1 MonitorInfo（EDID 指纹 = 名称+物理分辨率哈希键——换线不乱语义的同构）。 */
export function tauriMonitorToInfo(m: {
  name?: string | null;
  position?: { x: number; y: number };
  size?: { width: number; height: number };
  scaleFactor?: number;
}): MonitorInfo {
  const w = m.size?.width ?? 0;
  const h = m.size?.height ?? 0;
  return {
    id: m.name ?? `mon-${m.position?.x ?? 0}-${m.position?.y ?? 0}`,
    x: m.position?.x ?? 0,
    y: m.position?.y ?? 0,
    width: w,
    height: h,
    edidFingerprint: `tauri:${m.name ?? "?"}:${w}x${h}`,
    scale: m.scaleFactor ?? 1,
  };
}

/* ------------------------------- 回调与选项 ------------------------------- */

export interface WindowRuntimeOptions {
  /** 入口身份（审计与日志标签）。 */
  entry: string;
  /** 窗口根 data-app-id（F605 覆盖 / F616 应用档案 / 动作作用域的键）。 */
  appScope: string;
  /** 窗口根 data-app-class（per-app 滚轮档类目：document/code/browser/list…）。 */
  appClass: string;
  /** 桌面窗副本层（true=管线输出回调 onReplica；headless 窗口 false=零分配）。 */
  replica?: boolean;
  /** 多屏源覆盖（测试注入；缺省=Tauri availableMonitors + 单屏降级）。 */
  monitorsSource?: () => MonitorInfo[];
}

export interface WindowRuntimeCallbacks {
  /** 副本位置（仅 replica=true 时回调；null=隐藏副本层）。 */
  onReplica?: (p: { x: number; y: number } | null) => void;
  /** F604 锚标生命线（null=关闭）。 */
  onAnchor?: (p: { x: number; y: number } | null) => void;
  /** F617 识别成功墨迹（120ms 淡出由渲染层执行；null=清除）。 */
  onInk?: (pts: { x: number; y: number }[] | null) => void;
  /** F617 画中实时墨迹（轨迹跟手——v3 新增，识别前就能看见自己在画什么）。 */
  onLiveInk?: (pts: { x: number; y: number }[] | null) => void;
  /** F614 首插建档气泡（cloned=true 时回调一次）。 */
  onDeviceClone?: (info: { cloned: boolean; evicted?: string }) => void;
  /** 动作无人处理（面板/Toast 显性呈现钩子）。 */
  onActionUnhandled?: (action: string, source: "gesture" | "side") => void;
  /** F602 慢速微调激活态（HUD 跟随指针——100ms 反馈红线，v4）。 */
  onSlowTune?: (active: boolean, x: number, y: number) => void;
}

export interface WindowRuntime {
  dispose: () => void;
  /** 挂载身份（审计面板同源）。 */
  info: { entry: string; appScope: string; appClass: string; replica: boolean };
}

/** 活动运行时登记（接线审计面板与测试同源——谁在跑、什么身份，一处一事实）。 */
const activeRuntimes = new Set<WindowRuntime>();

/** 当前活动运行时快照（只读）。 */
export function activeRuntimeSnapshot(): { entry: string; appScope: string; appClass: string; replica: boolean }[] {
  return [...activeRuntimes].map((r) => ({ ...r.info }));
}

/**
 * 显示器清单安全获取（F607 屏对面板用）：Tauri availableMonitors → 降级单屏。
 * 独立于运行时实例（面板在设置打开时即需清单，不依赖桌面壳挂载态）。
 */
export async function listMonitorsSafe(): Promise<MonitorInfo[]> {
  try {
    const win = await import("@tauri-apps/api/window");
    const list = await win.availableMonitors();
    if (Array.isArray(list) && list.length > 0) return list.map(tauriMonitorToInfo);
  } catch {
    /* 非 Tauri：单屏降级 */
  }
  return typeof window !== "undefined"
    ? [{ id: "primary", x: 0, y: 0, width: window.innerWidth, height: window.innerHeight, edidFingerprint: `screen-${window.innerWidth}x${window.innerHeight}`, scale: window.devicePixelRatio || 1 }]
    : [];
}

/* ------------------------------- 内核 ------------------------------- */

export function createWindowRuntime(opts: WindowRuntimeOptions, cb: WindowRuntimeCallbacks = {}): WindowRuntime {
  // 多屏状态独立于 st（SeamGuard 闭包引用它——避免 st 自引用推断环）。
  const mstate = {
    monitors: null as MonitorInfo[] | null,
    scale: 1,
    tauri: false,
  };
  const st = {
    disposed: false,
    // 管线
    lastX: -1,
    lastY: -1,
    vx: 0,
    vy: 0,
    lift: new LiftFilter(),
    tremor: new TremorFilter("off"),
    keys: { shiftKey: false, ctrlKey: false, altKey: false, caps: false },
    // 滚轮
    gain: new WheelGain(() => cfg<WheelGainConfig>("wheelGain", WHEEL_GAIN_DEFAULT)),
    inertiaV: new WheelInertia(() => cfg<InertiaConfig>("wheelGain", INERTIA_DEFAULT)),
    inertiaH: new WheelInertia(() => cfg<InertiaConfig>("wheelGain", INERTIA_DEFAULT)), // 横向通道（F606/F204 对称余韵，v4）
    momentumRaf: 0,
    momentumEl: null as Element | null,
    momentumAxis: "y" as "y" | "x",
    // 跨屏（几何在 mstate；本表只挂守卫与记忆）
    guard: new SeamGuard(
      () => mstate.monitors ?? fallbackMonitors(),
      () => cfg<SeamGuardConfig>("seamGuard", SEAM_DEFAULT),
      (pairKey) => {
        // F607 屏对覆盖（v4 接线）：覆盖 > 全局（与 F605 同构）。
        const pairs = (j1Store.get("seamGuard").pairs as Record<string, { enabled: boolean }> | undefined) ?? {};
        return pairs[pairKey]?.enabled;
      },
    ),
    memory: new ScreenMemory(() => cfg<ScreenMemoryConfig>("screenMemory", MEMORY_DEFAULT)),
    memThrottleAt: 0,
    // F608 磁吸视觉平滑状态 + F602 HUD 态（v4）。
    magVis: { x: 0, y: 0 },
    slowActive: false,
    // 手势
    recognizer: new GestureRecognizer(),
    gestureLive: false,
    // 自动滚 / 拖拽
    anchor: null as { x: number; y: number; clientX: number; clientY: number; raf: number; openedAt: number } | null,
    dragActive: false,
    dragRaf: 0,
    // F616 当前应用作用域（pointerdown 时按命中链刷新；缺省=入口身份）
    currentScope: opts.appScope,
    // F614 首交互建档（一次性）
    deviceEnsured: false,
    deviceManager: new DeviceProfileManager(),
    // F610 旋钮缓存（避免每次 store 广播全量重算）
    timingApplied: false,
  };

  const dpr = (): number => (typeof window !== "undefined" ? window.devicePixelRatio || 1 : 1);

  /** 单屏降级源（浏览器 dev / Tauri 查询失败——功能自然休眠的诚实边界）。 */
  const fallbackMonitors = (): MonitorInfo[] => {
    const w = typeof window !== "undefined" ? window.innerWidth : 1920;
    const h = typeof window !== "undefined" ? window.innerHeight : 1080;
    return [{ id: "primary", x: 0, y: 0, width: w, height: h, edidFingerprint: `screen-${w}x${h}`, scale: dpr() }];
  };

  /* ---------- F607/F613 Tauri 多屏缓存刷新（异步，失败静默降级为单屏） ---------- */
  const refreshMonitors = async (): Promise<void> => {
    if (opts.monitorsSource) {
      mstate.monitors = opts.monitorsSource();
      mstate.tauri = mstate.monitors.length > 1;
      mstate.scale = 1;
      return;
    }
    try {
      const win = await import("@tauri-apps/api/window");
      const list = await win.availableMonitors();
      if (Array.isArray(list) && list.length > 0) {
        mstate.monitors = list.map(tauriMonitorToInfo);
        mstate.tauri = list.length > 1;
        mstate.scale = dpr(); // 物理像素口径（与 Tauri Monitor 边界一致）
        logInfo("mouse-j1", `多屏缓存刷新：${list.length} 块屏（护边/落点记忆坐标系=物理像素）`);
        return;
      }
    } catch {
      /* 非 Tauri 或查询失败：单屏降级（不报错刷屏——dev 常态） */
    }
    mstate.monitors = fallbackMonitors();
    mstate.tauri = false;
    mstate.scale = 1;
  };
  void refreshMonitors();

  /** 指针的虚拟桌面坐标（多屏物理像素 / 单屏窗口坐标——同一缓存内自洽）。 */
  const virtualPoint = (clientX: number, clientY: number): { x: number; y: number } => {
    if (!mstate.tauri) return { x: clientX, y: clientY };
    return toVirtualPoint(clientX, clientY, mstate.scale);
  };

  /* ---------- F616/F601 生效参数（设备档案×应用档案×全局曲线 合成） ---------- */
  const effectiveCurve = (): CurveConfig => {
    const base = cfg<CurveConfig>("curve", CURVE_DEFAULT);
    const dev = deviceParamsOverride();
    const global = { sens: dev.sens ?? base.sens, curve: dev.curve ?? base.id };
    const eff = effectiveParamsFor(st.currentScope, global);
    return { ...base, id: eff.curve, sens: eff.sens };
  };

  /* ---------- F610/F619 即时生效通道（CSS 变量 + --hover-delay 偏离映射） ---------- */
  const applyTimingVars = (): void => {
    const h = cfg<{ menuDelayMs: number; tooltipDelayMs: number }>("hoverTiming", HOVER_DEFAULT);
    const v = hoverDelayVars(h.menuDelayMs, h.tooltipDelayMs);
    const root = document.documentElement;
    root.style.setProperty("--vx-menu-delay", v.menu);
    root.style.setProperty("--vx-tooltip-delay", v.tooltip);
    if (v.hoverDelayOverride) root.style.setProperty("--hover-delay", v.hoverDelayOverride);
    else root.style.removeProperty("--hover-delay"); // 基线档=还政于 M-71 默认（零迁移）
    root.style.setProperty("--vx-longpress-scale", String(cfg<{ scale: number }>("longPress", LP_DEFAULT).scale));
    st.timingApplied = true;
  };
  applyTimingVars();

  /* ---------- 辅助 ---------- */

  const appScopeOf = (target: EventTarget | null): string | null => {
    const el = target instanceof Element ? target.closest("[data-app-id]") : null;
    return (el as HTMLElement | null)?.dataset.appId ?? null;
  };

  const roleOf = (target: EventTarget | null): string => {
    if (!(target instanceof Element)) return "unknown";
    const el = target as HTMLElement;
    return el.getAttribute?.("role") ?? el.tagName?.toLowerCase() ?? "unknown";
  };

  const scrollElement = (target: Element, top: number, left: number, behavior: ScrollBehavior): void => {
    const el = target as HTMLElement;
    if (typeof el.scrollBy === "function") el.scrollBy({ top, left, behavior });
    else el.scrollTop += top; // 非元素节点兜底（零死胡同）
  };

  const scrollContainerAt = (x: number, y: number): HTMLElement | null => {
    let cur: HTMLElement | null = document.elementFromPoint(x, y) as HTMLElement | null;
    while (cur) {
      const style = cur.ownerDocument.defaultView?.getComputedStyle(cur);
      if (style && (style.overflowY === "auto" || style.overflowY === "scroll")) return cur;
      if (cur.hasAttribute(AUTOSCROLL_ATTR)) return cur;
      cur = cur.parentElement;
    }
    return null;
  };

  const modifierActive = (key: SlowTuneKey): boolean => {
    switch (key) {
      case "shift": return st.keys.shiftKey;
      case "ctrl": return st.keys.ctrlKey;
      case "alt": return st.keys.altKey;
      case "capslock": return st.keys.caps;
      default: return false;
    }
  };

  /* ---------- F604 中键自动滚动 ---------- */

  const startAutoscroll = (container: HTMLElement, x: number, y: number): void => {
    stopAutoscroll();
    st.anchor = { x, y, clientX: x, clientY: y, raf: 0, openedAt: performance.now() };
    cb.onAnchor?.({ x, y });
    j1Telemetry.anchorLifecycle("open", x, y);
    const step = (): void => {
      if (!st.anchor) return;
      const v = autoscrollVelocity(st.lastX - st.anchor.clientX, st.lastY - st.anchor.clientY, cfg<AutoscrollConfig>("autoscroll", AUTO_DEFAULT));
      const ramp = autoscrollRamp(performance.now() - st.anchor.openedAt);
      if (v.vx !== 0 || v.vy !== 0) container.scrollBy({ left: v.vx * 0.25 * ramp, top: v.vy * 0.25 * ramp });
      st.anchor.raf = requestAnimationFrame(step);
    };
    st.anchor.raf = requestAnimationFrame(step);
  };

  const stopAutoscroll = (): void => {
    if (st.anchor) {
      cancelAnimationFrame(st.anchor.raf);
      j1Telemetry.anchorLifecycle("close", st.anchor.clientX, st.anchor.clientY);
      st.anchor = null;
      cb.onAnchor?.(null);
    }
  };

  /* ---------- F609 拖拽边缘自动滚 ---------- */

  const dragStep = (): void => {
    st.dragRaf = 0;
    const dcfg = cfg<DragScrollConfig>("dragScroll", DRAG_DEFAULT);
    if (!dcfg.enabled || !st.dragActive) return;
    for (const c of document.querySelectorAll<HTMLElement>(`[${AUTOSCROLL_ATTR}]`)) {
      const r = c.getBoundingClientRect();
      if (st.lastX < r.left || st.lastX > r.right || st.lastY < r.top || st.lastY > r.bottom) continue;
      const d = edgeDepth(st.lastX, st.lastY, { left: r.left, top: r.top, right: r.right, bottom: r.bottom }, dcfg.bandPx);
      const dy = edgeScrollSpeed(d.bottom, dcfg.bandPx) - edgeScrollSpeed(d.top, dcfg.bandPx);
      const dx = edgeScrollSpeed(d.right, dcfg.bandPx) - edgeScrollSpeed(d.left, dcfg.bandPx);
      if (dx !== 0 || dy !== 0) c.scrollBy({ left: dx, top: dy });
    }
    st.dragRaf = requestAnimationFrame(dragStep);
  };

  const ensureDragLoop = (): void => {
    if (st.dragActive && st.dragRaf === 0) st.dragRaf = requestAnimationFrame(dragStep);
  };

  /* ---------- 惯性 rAF（F204 平滑档余韵；纵轴/横轴双通道 v4） ---------- */

  const momentumStep = (): void => {
    st.momentumRaf = 0;
    if (inertiaSuspension(st.anchor !== null) === "suspended" || !st.momentumEl) return;
    const step = st.momentumAxis === "x" ? st.inertiaH.tick(performance.now()) : st.inertiaV.tick(performance.now());
    if (step !== 0) {
      if (st.momentumAxis === "x") scrollElement(st.momentumEl, 0, step, "auto");
      else scrollElement(st.momentumEl, step, 0, "auto");
      st.momentumRaf = requestAnimationFrame(momentumStep);
    }
  };

  /* ---------- 键盘：F602 修饰键 + Esc 退出 + blur 复位 ---------- */

  const trackKeys = (e: KeyboardEvent): void => {
    st.keys.shiftKey = e.getModifierState?.("Shift") ?? st.keys.shiftKey;
    st.keys.ctrlKey = e.getModifierState?.("Control") ?? st.keys.ctrlKey;
    st.keys.altKey = e.getModifierState?.("Alt") ?? st.keys.altKey;
    st.keys.caps = e.getModifierState?.("CapsLock") ?? st.keys.caps;
  };

  const onKeyDown = (e: KeyboardEvent): void => {
    trackKeys(e);
    if (e.key === "Escape" && st.anchor) stopAutoscroll(); // F604 退出三路之 Esc
  };
  const onKeyUp = (e: KeyboardEvent): void => trackKeys(e);
  const onBlur = (): void => {
    // F602 blur 复位：切窗后 OS 修饰键状态不可信，清零防「粘住慢速档」。
    st.keys = { shiftKey: false, ctrlKey: false, altKey: false, caps: false };
    stopAutoscroll();
    st.gestureLive = false;
    cb.onLiveInk?.(null);
  };

  /* ---------- 指针管线 ---------- */

  const onMove = (e: PointerEvent): void => {
    if ((e.buttons & 1) !== 0) st.dragActive = true;
    // F617 画中实时墨迹（右键按住期间跟手轨迹）。
    if (st.gestureLive && (e.buttons & 2) !== 0) {
      st.recognizer.feed(e.clientX, e.clientY);
      cb.onLiveInk?.(st.recognizer.trail);
    }

    const first = st.lastX === -1;
    const raw = { dx: e.clientX - (st.lastX === -1 ? e.clientX : st.lastX), dy: e.clientY - (st.lastY === -1 ? e.clientY : st.lastY) };
    st.lastX = e.clientX;
    st.lastY = e.clientY;
    if (first) {
      st.vx = e.clientX;
      st.vy = e.clientY;
    }

    // F607 护边（虚拟桌面坐标；单屏自然直通）。
    const vp = virtualPoint(e.clientX, e.clientY);
    if (st.guard.feed(vp.x, vp.y, performance.now()) === "hold") {
      j1Telemetry.log("seam-hold", "smooth", "seam-guard", e.clientX, e.clientY);
    }

    // F613 记忆节流（每 2s 一次）。
    const now = performance.now();
    if (now - st.memThrottleAt > 2000) {
      st.memThrottleAt = now;
      const mons = mstate.monitors ?? fallbackMonitors();
      const cur = mons.find((m) => vp.x >= m.x && vp.x <= m.x + m.width && vp.y >= m.y && vp.y <= m.y + m.height) ?? mons[0];
      if (cur) st.memory.remember(cur.edidFingerprint, vp.x, vp.y, mons);
    }

    // 副本层需要时才跑管线（headless 零开销）。
    const overlayCfg = cfg<PointerOverlayLite>("overlay", OVERLAY_DEFAULT);
    const magnetCfg = cfg<{ enabled: boolean; radiusPx: number }>("magnet", MAGNET_DEFAULT);
    const needReplica = Boolean(opts.replica) && (overlayActive(overlayCfg) || magnetCfg.enabled);
    if (!needReplica) {
      if (opts.replica) cb.onReplica?.(null);
      return;
    }
    const tremorCfg = cfg<{ level: "off" | "light" | "strong" }>("tremor", { level: "off" });
    if (st.tremor.level !== tremorCfg.level) st.tremor = new TremorFilter(tremorCfg.level);
    const a = st.tremor.feed(raw.dx, raw.dy);
    const b = st.lift.feed(a.x, a.y, now);
    const slowCfg = cfg<{ enabled: boolean; ratio: number; key: SlowTuneKey }>("slowTune", SLOW_DEFAULT);
    const slow = slowTuneGain(modifierActive(slowCfg.key), slowCfg);
    const applied = slow !== null ? { x: b.x * slow, y: b.y * slow } : applyCurve(b.x, b.y, effectiveCurve());
    // F602 慢速微调 HUD（v4：激活态变化即回调——100ms 反馈红线）。
    const slowActive = slow !== null;
    if (slowActive !== st.slowActive) {
      st.slowActive = slowActive;
      cb.onSlowTune?.(slowActive, e.clientX, e.clientY);
    } else if (slowActive) {
      cb.onSlowTune?.(true, e.clientX, e.clientY); // 跟随指针
    }
    st.vx += applied.x;
    st.vy += applied.y;
    // F608 磁吸：视觉微移 + 渐近平滑（v4——微滑不瞬移；真实判定零偏移不变）。
    let tx = 0;
    let ty = 0;
    if (magnetCfg.enabled) {
      const hit = document.elementFromPoint(e.clientX, e.clientY);
      if (hit && isMagnetizableLite(hit)) {
        const r = hit.getBoundingClientRect();
        const dxc = r.left + r.width / 2 - e.clientX;
        const dyc = r.top + r.height / 2 - e.clientY;
        const dist = Math.hypot(dxc, dyc);
        if (dist > 0 && dist <= Math.max(4, Math.min(32, magnetCfg.radiusPx || 12)) && r.width < 24 && r.height < 24) {
          tx = Math.round(dxc * 0.4 * 10) / 10;
          ty = Math.round(dyc * 0.4 * 10) / 10;
        }
      }
    }
    st.magVis = lerpToward(st.magVis, { x: tx, y: ty }, 0.35);
    cb.onReplica?.({ x: st.vx + st.magVis.x, y: st.vy + st.magVis.y });
  };

  /* ---------- 按下：建档 / 档案挂载 / 侧键 / 手势开始 / 中键锚标 ---------- */

  const onDown = (e: PointerEvent): void => {
    st.lift.onButtonUp(performance.now() + 1e6); // 按下=离开抬起窗
    j1Telemetry.log("click", "smooth", roleOf(e.target), e.clientX, e.clientY);

    // F614 首交互建档（一次性；notifyOnClone 关闭时静默）。
    if (!st.deviceEnsured) {
      st.deviceEnsured = true;
      const dm = st.deviceManager;
      dm.currentDefault = currentDefaultParams();
      const r = ensurePrimaryDevice(dm, Date.now());
      if (r?.cloned) {
        logInfo("mouse-j1", `F614 首插建档完成（${PRIMARY_DEVICE_LOG}）${r.evicted ? `；淘汰最久未用：${r.evicted}` : ""}`);
        cb.onDeviceClone?.({ cloned: r.cloned, evicted: r.evicted });
      }
    }

    // F616 前台档案挂载（命中链 data-app-id → 入口身份兜底）。
    st.currentScope = appScopeOf(e.target) ?? opts.appScope;
    const eff = effectiveCurve();
    if (eff.sens !== (cfg<CurveConfig>("curve", CURVE_DEFAULT).sens) || eff.id !== cfg<CurveConfig>("curve", CURVE_DEFAULT).id) {
      j1Telemetry.log("profile-switch", "smooth", `app:${st.currentScope}`, e.clientX, e.clientY);
    }

    // F615 侧键 → 动作路由中心（应用覆盖优先级在 resolveSideButton 内裁决）。
    if (e.button === 3 || e.button === 4) {
      const scfg = cfg<SideButtonsConfig>("sideButtons", SIDE_DEFAULT);
      const target = resolveSideButton(scfg, st.currentScope, e.button);
      if (target) {
        e.preventDefault();
        const action = target.kind === "action" ? target.action : target.kind === "shortcut" ? `shortcut:${target.keys}` : `launch:${target.appId}`;
        void dispatchJ1Action(action, "side", st.currentScope, { x: e.clientX, y: e.clientY }).then((r) => {
          if (!r.handled) cb.onActionUnhandled?.(action, "side");
        });
      }
    }

    // F617 手势开始（右键按住）。
    const gcfg = cfg<GestureLibraryConfig>("gestures", GESTURE_DEFAULT);
    if (e.button === 2 && gcfg.enabled) {
      st.recognizer.begin(e.clientX, e.clientY);
      st.gestureLive = true;
    }

    // F604 中键锚标。
    const acfg = cfg<AutoscrollConfig>("autoscroll", AUTO_DEFAULT);
    if (e.button === 1 && acfg.enabled) {
      const container = scrollContainerAt(e.clientX, e.clientY);
      if (container) {
        e.preventDefault();
        startAutoscroll(container, e.clientX, e.clientY);
      }
    }
  };

  /* ---------- 抬起：抬笔登记 / 手势识别 / 退出锚标 ---------- */

  const onUp = (e: PointerEvent): void => {
    st.lift.onButtonUp(performance.now());
    const gcfg = cfg<GestureLibraryConfig>("gestures", GESTURE_DEFAULT);
    if (e.button === 2 && gcfg.enabled && st.gestureLive) {
      st.gestureLive = false;
      cb.onLiveInk?.(null);
      const hit = st.recognizer.recognize(gcfg);
      const steps = st.recognizer.steps.length;
      const pts = st.recognizer.trail;
      st.recognizer.reset();
      j1Telemetry.gestureOutcome(!!hit, steps, e.clientX, e.clientY);
      if (hit) {
        e.preventDefault();
        e.stopPropagation();
        cb.onInk?.(pts); // 墨迹 120ms 淡出由渲染层执行
        // F617 重绑定解析（v4）：轨迹命中 → bindings 覆盖动作。
        const action = resolveGestureAction(hit, gcfg);
        void dispatchJ1Action(action, "gesture", st.currentScope, { x: e.clientX, y: e.clientY }).then((r) => {
          if (!r.handled) cb.onActionUnhandled?.(action, "gesture");
        });
      }
      // 无轨迹/无匹配 → 放行右键菜单（零误伤兜底）。
    }
    if (e.button === 0) st.dragActive = false;
    if (st.anchor && autoscrollExitFor(e.button) === "exit") stopAutoscroll();
  };

  const onPointerCancel = (): void => {
    // 打断路径：拖拽/手势/锚标全部安全落点（交互状态机完整出路——章三）。
    st.dragActive = false;
    st.gestureLive = false;
    cb.onLiveInk?.(null);
    stopAutoscroll();
  };

  /* ---------- 滚轮：锚标打断 → deltaMode 归一 → 倾斜 → 穿透 → 档位 → 增益/惯性 ---------- */

  const onWheel = (e: WheelEvent): void => {
    // F604 滚轮打断：锚标接管期间滚轮输入=用户改主意（Windows 语义）。
    if (st.anchor) stopAutoscroll();

    const hit = document.elementFromPoint(e.clientX, e.clientY);
    if (!hit) return;
    const norm = normalizeWheelDelta({ deltaX: e.deltaX, deltaY: e.deltaY, deltaMode: e.deltaMode, viewportHeight: window.innerHeight });

    // F606 真实倾斜路径：纯横向事件按倾斜档语义横滚 + 横向惯性余韵（v4 对称通道）。
    const tilt = tiltFromWheelEvent(e.deltaX, e.deltaY, cfg<TiltWheelConfig>("tiltWheel", TILT_DEFAULT).colsPerNotch);
    if (tilt && cfg<TiltWheelConfig>("tiltWheel", TILT_DEFAULT).enabled) {
      e.preventDefault();
      scrollElement(hit, 0, tilt.dir * tilt.cols * LINE_HEIGHT, "auto");
      st.inertiaH.feed(tilt.cols, tilt.dir as 1 | -1, performance.now());
      st.momentumEl = hit;
      st.momentumAxis = "x";
      if (st.momentumRaf === 0) st.momentumRaf = requestAnimationFrame(momentumStep);
      j1Telemetry.log("wheel", "smooth", "tilt", e.clientX, e.clientY);
      return;
    }

    // Shift+滚轮 → 倾斜等效（无倾斜轮设备的等效入口；同享横向惯性余韵）。
    if (e.shiftKey) {
      const tcfg = cfg<TiltWheelConfig>("tiltWheel", TILT_DEFAULT);
      if (tcfg.enabled) {
        e.preventDefault();
        const dir = norm.deltaY >= 0 ? 1 : -1;
        const cols = Math.max(1, Math.min(12, tcfg.colsPerNotch || 3));
        scrollElement(hit, 0, dir * cols * LINE_HEIGHT, "auto");
        st.inertiaH.feed(cols, dir as 1 | -1, performance.now());
        st.momentumEl = hit;
        st.momentumAxis = "x";
        if (st.momentumRaf === 0) st.momentumRaf = requestAnimationFrame(momentumStep);
        j1Telemetry.log("wheel", "smooth", "tilt-equivalent", e.clientX, e.clientY);
        return;
      }
    }

    // F618 穿透三态。
    const pcfg = cfg<PassthroughConfig>("passthrough", PASS_DEFAULT);
    const { target, passthrough } = resolveWheelTarget(hit, pcfg);
    // F605 档位解析（应用覆盖 > 类目默认 > 全局档）。
    const wcfg = cfg<WheelNotchConfig>("wheelNotch", WHEEL_NOTCH_DEFAULT);
    const appId = appScopeOf(hit) ?? opts.appScope;
    const appClass = (target.closest("[data-app-class]") as HTMLElement | null)?.dataset.appClass ?? opts.appClass;
    const mode = resolveWheelMode(wcfg, appId, appClass);
    const smooth = mode === "smooth";
    const lines = st.gain.feed(performance.now(), !smooth); // 逐档豁免增益（F605 互斥边界）
    const sign = norm.deltaY >= 0 ? 1 : -1;
    e.preventDefault();
    scrollElement(target, sign * lines * LINE_HEIGHT, 0, "auto");
    j1Telemetry.log("wheel", "smooth", `${mode}${passthrough ? "+穿透" : ""}`, e.clientX, e.clientY);
    // 平滑档：动量交惯性引擎（F204 余韵；纵轴通道）。
    if (smooth) {
      st.inertiaV.feed(lines, sign as 1 | -1, performance.now());
      st.momentumEl = target;
      st.momentumAxis = "y";
      if (st.momentumRaf === 0) st.momentumRaf = requestAnimationFrame(momentumStep);
    } else {
      st.inertiaV.reset();
    }
  };

  /* ---------- F613 唤醒/切回恢复 + 多屏缓存刷新 ---------- */

  const onVisibility = (): void => {
    if (document.visibilityState === "visible") {
      void refreshMonitors();
      const mcfg = cfg<ScreenMemoryConfig>("screenMemory", MEMORY_DEFAULT);
      if (mcfg.enabled && mstate.tauri) {
        const mons = mstate.monitors ?? fallbackMonitors();
        const cur = mons.find((m) => (window.screenX * mstate.scale) >= m.x && (window.screenX * mstate.scale) <= m.x + m.width) ?? mons[0];
        if (cur) {
          const p = st.memory.restore(cur.edidFingerprint, mons);
          if (p) {
            // 物理像素 → 窗口逻辑坐标（副本层与积分器口径）。
            st.vx = (p.x - window.screenX * mstate.scale) / mstate.scale;
            st.vy = (p.y - window.screenY * mstate.scale) / mstate.scale;
          }
        }
      }
    } else if (st.lastX >= 0) {
      const mons = mstate.monitors ?? fallbackMonitors();
      const vp = virtualPoint(st.lastX, st.lastY);
      const cur = mons.find((m) => vp.x >= m.x && vp.x <= m.x + m.width && vp.y >= m.y && vp.y <= m.y + m.height) ?? mons[0];
      if (cur) st.memory.remember(cur.edidFingerprint, vp.x, vp.y, mons);
    }
  };

  /* ---------- 装配与退订 ---------- */

  const unsub = j1Store.subscribe((section) => {
    if (section === "hoverTiming" || section === "longPress") applyTimingVars();
    if (section === "tremor") st.tremor = new TremorFilter(cfg<{ level: "off" }>("tremor", { level: "off" }).level);
    if (section === "devices") st.deviceManager.currentDefault = currentDefaultParams();
  });

  const listeners: [string, EventTarget, EventListenerOrEventListenerObject, (AddEventListenerOptions | boolean)?][] = [
    ["keydown", window, onKeyDown as EventListener],
    ["keyup", window, onKeyUp as EventListener],
    ["pointermove", window, onMove as EventListener, { passive: true }],
    ["pointerdown", window, onDown as EventListener],
    ["pointerup", window, onUp as EventListener],
    ["pointercancel", window, onPointerCancel as EventListener],
    ["wheel", window, onWheel as EventListener, { passive: false }],
    ["blur", window, onBlur as EventListener],
    ["pointermove", window, ensureDragLoop as EventListener, { passive: true }],
  ];
  for (const [name, tgt, fn, o] of listeners) tgt.addEventListener(name, fn, o);
  document.addEventListener("visibilitychange", onVisibility);

  const handle: WindowRuntime = {
    dispose: (): void => {
      if (st.disposed) return;
      st.disposed = true;
      activeRuntimes.delete(handle);
      unsub();
      stopAutoscroll();
      if (st.dragRaf) cancelAnimationFrame(st.dragRaf);
      if (st.momentumRaf) cancelAnimationFrame(st.momentumRaf);
      for (const [name, tgt, fn, o] of listeners) tgt.removeEventListener(name, fn, o as EventListenerOptions);
      document.removeEventListener("visibilitychange", onVisibility);
    },
    info: { entry: opts.entry, appScope: opts.appScope, appClass: opts.appClass, replica: Boolean(opts.replica) },
  };
  activeRuntimes.add(handle);
  logInfo("mouse-j1", `窗口运行时挂载：${opts.entry}（scope=${opts.appScope} · class=${opts.appClass} · replica=${Boolean(opts.replica)}）`);
  return handle;
}

/* ------------------------------- 内核轻量类型（与全量模块同语义，避免渲染依赖） ------------------------------- */

interface PointerOverlayLite {
  outline: boolean;
  shadow: boolean;
  ring: boolean;
}
function overlayActive(c: PointerOverlayLite): boolean {
  return c.outline || c.shadow || c.ring;
}
function isMagnetizableLite(el: Element): boolean {
  const tag = el.tagName.toLowerCase();
  if (tag === "button" || tag === "input" || tag === "a") return true;
  const role = el.getAttribute("role");
  return role === "button" || role === "checkbox" || role === "radio" || role === "switch";
}

/** F614 当前默认四件套（建档克隆源：全局曲线节 + 全局滚轮档——一处一事实）。 */
function currentDefaultParams(): DeviceProfileParams {
  const curve = cfg<CurveConfig>("curve", CURVE_DEFAULT);
  const wheel = cfg<WheelNotchConfig>("wheelNotch", WHEEL_NOTCH_DEFAULT);
  return { sens: curve.sens || 1, curve: curve.id, wheelMode: wheel.mode };
}

/* ------------------------------- 默认值镜像（与 j1store J1_DEFAULTS 同源） ------------------------------- */

const PRIMARY_DEVICE_LOG = "primary-webview";
const WHEEL_GAIN_DEFAULT: WheelGainConfig = { enabled: true, minLines: 3, maxLines: 12, accelMs: 220 };
const SEAM_DEFAULT: SeamGuardConfig = { enabled: true, edgePx: 4, dwellMs: 200, cornerPx: 8 };
const MEMORY_DEFAULT: ScreenMemoryConfig = { enabled: true, points: {} };
const SLOW_DEFAULT = { enabled: true, ratio: 0.1, key: "shift" as SlowTuneKey };
const CURVE_DEFAULT: CurveConfig = { id: "classic", cp1x: 0.35, cp1y: 0.55, cp2x: 0.7, cp2y: 1.0, sens: 1 };
const OVERLAY_DEFAULT: PointerOverlayLite = { outline: true, shadow: false, ring: false };
const MAGNET_DEFAULT = { enabled: false, radiusPx: 12 };
const GESTURE_DEFAULT: GestureLibraryConfig = { enabled: false, trailFadeMs: 120, custom: {}, bindings: {} };
const SIDE_DEFAULT: SideButtonsConfig = {
  global: { "3": { kind: "action", action: "nav-back" }, "4": { kind: "action", action: "nav-forward" } },
  apps: {},
};
const AUTO_DEFAULT: AutoscrollConfig = { enabled: true, ...AUTOSCROLL_PRESET };
const DRAG_DEFAULT: DragScrollConfig = { enabled: true, bandPx: 24 };
const WHEEL_NOTCH_DEFAULT: WheelNotchConfig = { mode: "per-app", overrides: {}, linesPerNotch: 3 };
const TILT_DEFAULT: TiltWheelConfig = { enabled: true, colsPerNotch: 3, repeatDelayMs: 350, repeatRateMs: 40, hasTilt: true };
const PASS_DEFAULT: PassthroughConfig = { enabled: true, exemptTypes: ["scrollable-layer", "select", "menu"] };
const HOVER_DEFAULT = { menuDelayMs: 400, tooltipDelayMs: 500 };
const LP_DEFAULT = { scale: 1.0, registry: {} };
