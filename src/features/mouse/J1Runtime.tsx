/**
 * J 鼠标域 · AI-J1 运行时（桌面窗口事件管线总装）。
 *
 * 管线序（固定，一处一事实）：原始位移 → F611 手抖过滤 → F603 抬笔滤波 →
 * F602 慢速微调（修饰键恒定增益）→ F601 速度曲线 → 位置积分。
 *
 * 生效面声明（诚实边界）：
 * - 本运行时在 Varix 桌面窗口内接管指针/滚轮/侧键/右键事件，驱动 F335 优先
 *   平面上的指针副本层（F620 衬底、F608 磁吸视觉、F604 锚标、F617 墨迹）。
 *   管线输出积分驱动副本位置——手抖过滤/抬笔滤波/慢速微调/速度曲线在副本层
 *   全部真实生效；点击判定永远走真实光标（F608 判定零偏移铁律同源）。
 * - 操作系统级指针增益经 F250 通道（ipc.mouseParamsWrite）同步——F601 曲线谱
 *   是其设计层与对拍表，两处共用同一套曲线参数（单一事实源）。
 * - 容器自动滚（F609）只对 data-autoscroll 声明容器生效（不越权）。
 *
 * 交互动作出口：手势/侧键触发统一派发 `vx-j1-action` CustomEvent
 * （{ action, source }）——系统组件按 action 订阅（开放性扩展点，十四章）。
 */

import React, { useEffect, useMemo, useRef, useState } from "react";
import { j1Store, type J1Section } from "./j1store";
import { applyCurve, slowTuneGain, type CurveConfig, type SlowTuneKey } from "./curve";
import { LiftFilter, TremorFilter } from "./filters";
import { WheelGain, resolveWheelMode, resolveWheelTarget, tiltFromShiftWheel, type WheelNotchConfig, type TiltWheelConfig, type PassthroughConfig, type WheelGainConfig } from "./wheel";
import { SeamGuard, ScreenMemory, type MonitorInfo, type SeamGuardConfig, type ScreenMemoryConfig } from "./screen";
import { magnetOffset, type MagnetConfig } from "./magnet";
import { autoscrollVelocity, autoscrollExitFor, edgeDepth, edgeScrollSpeed, AUTOSCROLL_ATTR, AUTOSCROLL_PRESET, type AutoscrollConfig, type DragScrollConfig } from "./autoscroll";
import { GestureRecognizer, TRAIL_FADE_MS, type GestureLibraryConfig } from "./gestures";
import { composeOverlay, type PointerOverlayConfig } from "./overlay";
import { resolveSideButton, type SideButtonsConfig, type SideTarget } from "./sideButtons";
import { clampHoverDelay } from "./hoverTiming";

/* ------------------------------- 配置读取 ------------------------------- */

function cfg<T>(section: J1Section, fallback: T): T {
  const s = j1Store.get(section);
  return { ...(fallback as unknown as Record<string, unknown>), ...s } as unknown as T;
}

/* ------------------------------- 动作出口 ------------------------------- */

function dispatchAction(action: string, source: "gesture" | "side"): void {
  window.dispatchEvent(new CustomEvent("vx-j1-action", { detail: { action, source } }));
}

function runSideTarget(t: SideTarget): void {
  if (t.kind === "action") dispatchAction(t.action, "side");
  else if (t.kind === "shortcut") dispatchAction(`shortcut:${t.keys}`, "side");
  else dispatchAction(`launch:${t.appId}`, "side");
}

/* ------------------------------- 指针副本层 ------------------------------- */

/** 指针箭头 1:1 复刻（标准箭头轮廓，viewBox 32 高精度——4K 放大不糊）。 */
const ARROW_PATH = "M2 2 L2 24.5 L8.2 19.6 L12.2 28.6 L15.8 27 L11.9 18.3 L19.4 17.6 Z";

function PointerReplica(props: { x: number; y: number; filter: string; shadow: string }): React.ReactElement {
  return (
    <div
      aria-hidden
      style={{
        position: "fixed",
        left: 0,
        top: 0,
        transform: `translate(${props.x}px, ${props.y}px)`,
        transformOrigin: "2px 2px",
        pointerEvents: "none",
        zIndex: 2147483000, // F335 优先平面（顶层，浮层之上）
        filter: props.filter || undefined,
        willChange: "transform",
      }}
    >
      <svg width={32} height={32} viewBox="0 0 32 32" style={{ display: "block", filter: props.shadow || undefined }}>
        <path d={ARROW_PATH} fill="var(--vx-pointer-core, #ffffff)" stroke="var(--vx-pointer-line, #1b1b1b)" strokeWidth={1} />
      </svg>
    </div>
  );
}

/* ------------------------------- 运行时 ------------------------------- */

export function J1Runtime(): React.ReactElement | null {
  const [replica, setReplica] = useState<{ x: number; y: number } | null>(null);
  const [anchorUi, setAnchorUi] = useState<{ x: number; y: number } | null>(null);
  const [ink, setInk] = useState<{ pts: { x: number; y: number }[] } | null>(null);

  const st = useRef({
    // 管线状态
    lastX: -1,
    lastY: -1,
    vx: 0, // 副本位置积分（管线输出累计）
    vy: 0,
    lift: new LiftFilter(),
    tremor: new TremorFilter("off"),
    keys: { shiftKey: false, ctrlKey: false, altKey: false, caps: false },
    // 滚轮
    gain: new WheelGain(() => cfg<WheelGainConfig>("wheelGain", WHEEL_GAIN_DEFAULT)),
    // 跨屏
    guard: new SeamGuard(() => monitors(), () => cfg<SeamGuardConfig>("seamGuard", SEAM_DEFAULT)),
    memory: new ScreenMemory(() => cfg<ScreenMemoryConfig>("screenMemory", MEMORY_DEFAULT)),
    // 手势
    recognizer: new GestureRecognizer(),
    // 自动滚
    anchor: null as { x: number; y: number; raf: number } | null,
    dragActive: false,
    dragRaf: 0,
    memThrottleAt: 0,
  }).current;

  useEffect(() => {
    // 桌面窗口专属运行时（App.tsx 仅在 appType==="desktop" 挂载——单一守卫）。

    /* ---------- 键盘修饰键状态（F602 唯一事实来源：真实键盘事件） ---------- */
    const trackKeys = (e: KeyboardEvent, down: boolean): void => {
      st.keys.shiftKey = e.getModifierState?.("Shift") ?? (down && e.key === "Shift");
      st.keys.ctrlKey = e.getModifierState?.("Control") ?? (down && e.key === "Control");
      st.keys.altKey = e.getModifierState?.("Alt") ?? (down && e.key === "Alt");
      st.keys.caps = e.getModifierState?.("CapsLock") ?? st.keys.caps;
    };
    const onKeyDown = (e: KeyboardEvent): void => {
      trackKeys(e, true);
      if (e.key === "Escape" && st.anchor) stopAutoscroll(); // F604 退出三路之 Esc
    };
    const onKeyUp = (e: KeyboardEvent): void => trackKeys(e, false);

    /* ---------- 指针管线 ---------- */
    const onMove = (e: PointerEvent): void => {
      // F609：左键拖拽中启动边缘滚动循环。
      if ((e.buttons & 1) !== 0) st.dragActive = true;

      const first = st.lastX === -1;
      const raw = { dx: e.clientX - (st.lastX === -1 ? e.clientX : st.lastX), dy: e.clientY - (st.lastY === -1 ? e.clientY : st.lastY) };
      st.lastX = e.clientX;
      st.lastY = e.clientY;
      if (first) {
        st.vx = e.clientX;
        st.vy = e.clientY;
      }

      // F607 护边时序（虚拟桌面坐标=窗口坐标；单屏环境自然静默直通）。
      st.guard.feed(e.clientX, e.clientY, performance.now());

      // F613 记忆节流（每 2s 一次）。
      const now = performance.now();
      if (now - st.memThrottleAt > 2000) {
        st.memThrottleAt = now;
        st.memory.remember(primaryMonitor().edidFingerprint, e.clientX, e.clientY, monitors());
      }

      // 副本需要时才跑管线（无层零开销）。
      const overlayCfg = cfg<PointerOverlayConfig>("overlay", OVERLAY_DEFAULT);
      const magnetCfg = cfg<MagnetConfig>("magnet", MAGNET_DEFAULT);
      const needReplica = composeOverlay(overlayCfg).active || magnetCfg.enabled;
      if (!needReplica) {
        if (replica !== null) setReplica(null);
        return;
      }
      const tremorCfg = cfg<{ level: "off" | "light" | "strong" }>("tremor", { level: "off" });
      if (st.tremor.level !== tremorCfg.level) st.tremor = new TremorFilter(tremorCfg.level);
      const a = st.tremor.feed(raw.dx, raw.dy);
      const b = st.lift.feed(a.x, a.y, now);
      const slowCfg = cfg<{ enabled: boolean; ratio: number; key: SlowTuneKey }>("slowTune", SLOW_DEFAULT);
      const slow = slowTuneGain(modifierActive(slowCfg.key, st.keys), slowCfg);
      const applied = slow !== null ? { x: b.x * slow, y: b.y * slow } : applyCurve(b.x, b.y, cfg<CurveConfig>("curve", CURVE_DEFAULT));
      st.vx += applied.x;
      st.vy += applied.y;
      // F608 磁吸：视觉微移（真实判定零偏移——hit 检测仍用真实坐标）。
      const hit = document.elementFromPoint(e.clientX, e.clientY);
      const mag = magnetOffset(e.clientX, e.clientY, hit, magnetCfg);
      setReplica({ x: st.vx + mag.dx, y: st.vy + mag.dy });
    };

    /* ---------- 按下：手势开始 / 侧键 / 中键自动滚 ---------- */
    const onDown = (e: PointerEvent): void => {
      st.lift.onButtonUp(performance.now() + 1e6); // 按下=离开抬起窗
      const gcfg = cfg<GestureLibraryConfig>("gestures", GESTURE_DEFAULT);
      if (e.button === 2 && gcfg.enabled) st.recognizer.begin(e.clientX, e.clientY);

      if (e.button === 3 || e.button === 4) {
        const scfg = cfg<SideButtonsConfig>("sideButtons", SIDE_DEFAULT);
        const target = resolveSideButton(scfg, appScopeOf(e.target), e.button);
        if (target) {
          e.preventDefault();
          runSideTarget(target);
        }
      }

      const acfg = cfg<AutoscrollConfig>("autoscroll", AUTO_DEFAULT);
      if (e.button === 1 && acfg.enabled) {
        const container = scrollContainerAt(e.clientX, e.clientY);
        if (container) {
          e.preventDefault();
          startAutoscroll(container, e.clientX, e.clientY);
        }
      }
    };

    /* ---------- 抬起：抬笔登记 / 手势识别 / 退出自动滚 ---------- */
    const onUp = (e: PointerEvent): void => {
      st.lift.onButtonUp(performance.now());
      const gcfg = cfg<GestureLibraryConfig>("gestures", GESTURE_DEFAULT);
      if (e.button === 2 && gcfg.enabled && st.recognizer.trail.length > 0) {
        const hit = st.recognizer.recognize(gcfg);
        const pts = st.recognizer.trail;
        st.recognizer.reset();
        if (hit) {
          e.preventDefault();
          e.stopPropagation();
          setInk({ pts }); // 墨迹 120ms 淡出
          window.setTimeout(() => setInk(null), TRAIL_FADE_MS);
          dispatchAction(hit.action, "gesture");
        }
        // 无轨迹/无匹配 → 放行右键菜单（零误伤兜底）
      }
      if (e.button === 0) st.dragActive = false;
      if (st.anchor && autoscrollExitFor(e.button) === "exit") stopAutoscroll();
    };

    /* ---------- 滚轮：F618 穿透 → F605 档位 → F612 增益 / F606 倾斜等效 ---------- */
    const onWheel = (e: WheelEvent): void => {
      const hit = document.elementFromPoint(e.clientX, e.clientY);
      if (!hit) return;
      // Shift+滚轮 → 倾斜等效横向滚动（无倾斜轮设备的等效入口）。
      if (e.shiftKey) {
        const tcfg = cfg<TiltWheelConfig>("tiltWheel", TILT_DEFAULT);
        if (tcfg.enabled) {
          e.preventDefault();
          const { dir, cols } = tiltFromShiftWheel(e.deltaY, tcfg.colsPerNotch);
          scrollElement(targetFor(hit), dir * cols * LINE_HEIGHT, 0, "auto");
          return;
        }
      }
      const pcfg = cfg<PassthroughConfig>("passthrough", PASS_DEFAULT);
      const { target, passthrough } = resolveWheelTarget(hit, pcfg);
      const wcfg = cfg<WheelNotchConfig>("wheelNotch", WHEEL_NOTCH_DEFAULT);
      const appId = appScopeOf(hit) ?? "desktop";
      const appClass = (target.closest("[data-app-class]") as HTMLElement | null)?.dataset.appClass;
      const mode = resolveWheelMode(wcfg, appId, appClass);
      const smooth = mode === "smooth";
      const lines = st.gain.feed(performance.now(), !smooth); // 逐档豁免增益（F605 互斥边界）
      const sign = e.deltaY >= 0 ? 1 : -1;
      e.preventDefault();
      scrollElement(target, sign * lines * LINE_HEIGHT, 0, smooth ? "smooth" : "auto");
      if (passthrough) return; // 穿透已生效（目标即下层容器）
    };

    /* ---------- F609 拖拽边缘自动滚（rAF 循环，仅声明容器） ---------- */
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

    /* ---------- F604 中键自动滚动 ---------- */
    const startAutoscroll = (container: HTMLElement, x: number, y: number): void => {
      stopAutoscroll();
      st.anchor = { x, y, raf: 0 };
      setAnchorUi({ x, y });
      const step = (): void => {
        if (!st.anchor) return;
        const v = autoscrollVelocity(st.lastX - st.anchor.x, st.lastY - st.anchor.y, cfg<AutoscrollConfig>("autoscroll", AUTO_DEFAULT));
        if (v.vx !== 0 || v.vy !== 0) container.scrollBy({ left: v.vx * 0.25, top: v.vy * 0.25 });
        st.anchor.raf = requestAnimationFrame(step);
      };
      st.anchor.raf = requestAnimationFrame(step);
    };
    const stopAutoscroll = (): void => {
      if (st.anchor) {
        cancelAnimationFrame(st.anchor.raf);
        st.anchor = null;
        setAnchorUi(null);
      }
    };

    /* ---------- F613 唤醒/切回恢复（单屏环境自然休眠） ---------- */
    const onVisibility = (): void => {
      if (document.visibilityState === "visible") {
        const mcfg = cfg<ScreenMemoryConfig>("screenMemory", MEMORY_DEFAULT);
        if (mcfg.enabled) {
          const p = st.memory.restore(primaryMonitor().edidFingerprint, monitors());
          if (p) {
            st.vx = p.x;
            st.vy = p.y;
          }
        }
      } else if (st.lastX >= 0) {
        st.memory.remember(primaryMonitor().edidFingerprint, st.lastX, st.lastY, monitors());
      }
    };

    const onBlur = (): void => stopAutoscroll();

    /* ---------- F610/F619 即时生效通道：CSS 变量（菜单/tooltip/长按同源消费） ---------- */
    const applyTimingVars = (): void => {
      const h = cfg<{ menuDelayMs: number; tooltipDelayMs: number }>("hoverTiming", HOVER_DEFAULT);
      document.documentElement.style.setProperty("--vx-menu-delay", `${clampHoverDelay(h.menuDelayMs)}ms`);
      document.documentElement.style.setProperty("--vx-tooltip-delay", `${clampHoverDelay(h.tooltipDelayMs)}ms`);
      document.documentElement.style.setProperty("--vx-longpress-scale", String(cfg<{ scale: number }>("longPress", LP_DEFAULT).scale));
    };
    applyTimingVars();

    const unsub = j1Store.subscribe((section) => {
      if (section === "hoverTiming" || section === "longPress") applyTimingVars();
      if (section === "tremor") st.tremor = new TremorFilter(cfg<{ level: "off" }>("tremor", { level: "off" }).level);
    });

    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("pointermove", onMove, { passive: true });
    window.addEventListener("pointerdown", onDown);
    window.addEventListener("pointerup", onUp);
    window.addEventListener("wheel", onWheel, { passive: false });
    window.addEventListener("blur", onBlur);
    document.addEventListener("visibilitychange", onVisibility);
    // 拖拽循环随 move 启动。
    window.addEventListener("pointermove", ensureDragLoop, { passive: true });
    return () => {
      unsub();
      stopAutoscroll();
      if (st.dragRaf) cancelAnimationFrame(st.dragRaf);
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("pointermove", onMove);
      window.removeEventListener("pointermove", ensureDragLoop);
      window.removeEventListener("pointerdown", onDown);
      window.removeEventListener("pointerup", onUp);
      window.removeEventListener("wheel", onWheel);
      window.removeEventListener("blur", onBlur);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [st, replica]);

  const overlay = useMemo(() => composeOverlay(cfg<PointerOverlayConfig>("overlay", OVERLAY_DEFAULT)), [replica, anchorUi]);
  if (!overlay.active && !anchorUi && !ink) return null;

  return (
    <>
      {overlay.active && replica && <PointerReplica x={replica.x} y={replica.y} filter={overlay.cssFilter} shadow={overlay.boxShadow} />}
      {anchorUi && <div aria-hidden className="j1-autoscroll-anchor" style={{ left: anchorUi.x, top: anchorUi.y }} />}
      {ink && (
        <svg aria-hidden className="j1-gesture-ink" style={{ position: "fixed", inset: 0, width: "100%", height: "100%", pointerEvents: "none", zIndex: 2147483001 }}>
          {ink.pts.length > 1 && (
            <polyline
              points={ink.pts.map((p) => `${p.x},${p.y}`).join(" ")}
              fill="none"
              stroke="var(--vx-ink, var(--vx-accent, #4f7cff))"
              strokeWidth={2.5}
              strokeLinecap="round"
              strokeLinejoin="round"
              opacity={0.9}
            />
          )}
        </svg>
      )}
    </>
  );
}

/* ------------------------------- 辅助 ------------------------------- */

const LINE_HEIGHT = 24; // 标准行高（3 行/格对拍 Windows 基准）

function modifierActive(key: SlowTuneKey, keys: { shiftKey: boolean; ctrlKey: boolean; altKey: boolean; caps: boolean }): boolean {
  switch (key) {
    case "shift": return keys.shiftKey;
    case "ctrl": return keys.ctrlKey;
    case "alt": return keys.altKey;
    case "capslock": return keys.caps;
    default: return false;
  }
}

/** 应用作用域识别：最近的 [data-app-id]（F605 覆盖 / F616 应用档案的键）。 */
function appScopeOf(target: EventTarget | null): string | null {
  const el = target instanceof Element ? target.closest("[data-app-id]") : null;
  return (el as HTMLElement | null)?.dataset.appId ?? null;
}

function scrollElement(target: Element, top: number, left: number, behavior: ScrollBehavior): void {
  const el = target as HTMLElement;
  if (typeof el.scrollBy === "function") el.scrollBy({ top, left, behavior });
  else el.scrollTop += top; // 非元素节点兜底（零死胡同）
}

function targetFor(hit: Element): Element {
  return hit;
}

function scrollContainerAt(x: number, y: number): HTMLElement | null {
  let cur: HTMLElement | null = document.elementFromPoint(x, y) as HTMLElement | null;
  while (cur) {
    const style = cur.ownerDocument.defaultView?.getComputedStyle(cur);
    if (style && (style.overflowY === "auto" || style.overflowY === "scroll")) return cur;
    if (cur.hasAttribute(AUTOSCROLL_ATTR)) return cur;
    cur = cur.parentElement;
  }
  return null;
}

/** 显示器源：当前环境单屏（多屏矩阵在 screen.ts 逻辑层全量测试覆盖）。 */
function monitors(): MonitorInfo[] {
  const s = window.screen;
  return [{ id: "primary", x: 0, y: 0, width: s.width, height: s.height, edidFingerprint: `screen-${s.width}x${s.height}`, scale: (s as Screen & { devicePixelRatio?: number }).devicePixelRatio ?? 1 }];
}
function primaryMonitor(): MonitorInfo {
  return monitors()[0]!;
}

/* ------------------------------- 默认值镜像（与 j1store J1_DEFAULTS 同源） ------------------------------- */

const WHEEL_GAIN_DEFAULT: WheelGainConfig = { enabled: true, minLines: 3, maxLines: 12, accelMs: 220 };
const SEAM_DEFAULT: SeamGuardConfig = { enabled: true, edgePx: 4, dwellMs: 200, cornerPx: 8 };
const MEMORY_DEFAULT: ScreenMemoryConfig = { enabled: true, points: {} };
const SLOW_DEFAULT = { enabled: true, ratio: 0.1, key: "shift" as SlowTuneKey };
const CURVE_DEFAULT: CurveConfig = { id: "classic", cp1x: 0.35, cp1y: 0.55, cp2x: 0.7, cp2y: 1.0, sens: 1 };
const OVERLAY_DEFAULT: PointerOverlayConfig = { outline: true, shadow: false, ring: false };
const MAGNET_DEFAULT: MagnetConfig = { enabled: false, radiusPx: 12 };
const GESTURE_DEFAULT: GestureLibraryConfig = { enabled: false, trailFadeMs: TRAIL_FADE_MS, custom: {} };
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
