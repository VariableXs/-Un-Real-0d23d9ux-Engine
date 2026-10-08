/**
 * I 通用域 · AI-U3 运行时（桌面事件管线总装，F501-F550 前端生效面）。
 *
 * 生效面声明（诚实边界）：
 * - 本运行时在 Varix 桌面窗口内接管：F513 Ctrl 定位涟漪、F514 声音视觉
 *   光带、F519 Caps 提示音、F523 打字隐藏指针、F537 瞥桌面、F511 剪贴板
 *   清空热键、F535 Win+数字动作出口、F542 ClickLock 真鼠标管线、F520
 *   标题栏中键出口（data-u3-titlebar 声明元素）、F505 蓝牙动态锁计时、
 *   F545 蓝牙电量平滑推进。
 * - 系统级生效（真实蓝牙 RSSI、锁屏截图通道拦截、录屏黑块合成）依赖内核
 *   侧实装层（kernel/varix/src/ustar3/ v1）与宿主集成，随闸门登记。
 * - 事件出口：交互动作统一走 dispatchU3Action（actions.ts 两级路由 +
 *   未处理显性化）；窗口系统按 `--vx-u3-window-opacity` CSS 变量消费瞥
 *   桌面透明度；桌面标题栏加 data-u3-titlebar 标记即接入中键最小化
 *   （开放扩展点——十四章，不越权改写桌面 DOM）。
 */

import React, { useEffect, useRef, useState } from "react";
import { u3Store } from "./u3store";
import { ctrlFindTick, rippleStarts, CTRL_FIND_TOTAL_MS, type CtrlFindRt } from "./pointerfx";
import { soundLightPolicy, SOUND_EVENT_COLORS, SV_EDGE_PX, SV_PULSES, SV_PULSE_MS, type SoundEventKind, type DisturbMode } from "./pointerfx";
import { CAPS_TONE_HZ, type CapsTone } from "./pointerfx";
import { typeHideOpacity, TYPE_HIDE_RESUME_MS, type TypeHideRt } from "./pointerfx";
import { peekStyle, PEEK_FADE_MS } from "./winkeys";
import { btLockTick, type BtLockRuntimeState } from "./locksec";
import { clickLockStep, smoothBattery, type ClickLockRt } from "./sysdev";
import { dispatchU3Action } from "./actions";
import { pushToast } from "../../state/uiStore";
import type { Toast } from "../../state/uiStore";

/* ------------------------------- 涟漪层 ------------------------------- */

interface Ripple { id: number; x: number; y: number; startedAt: number }

function CtrlRippleLayer({ ripples }: { ripples: Ripple[] }) {
  if (ripples.length === 0) return <></>;
  const starts = rippleStarts();
  return (
    <div aria-hidden style={{ position: "fixed", inset: 0, pointerEvents: "none", zIndex: 2147483000 }}>
      {ripples.map((r) => (
        <div key={r.id} style={{ position: "fixed", left: r.x, top: r.y }}>
          {[0, 1, 2].map((i) => {
            const delay = starts[i] ?? 0;
            return (
              <span
                key={i}
                className="u3-ctrl-ripple"
                style={{ animationDelay: `${delay}ms`, animationDuration: `${(CTRL_FIND_TOTAL_MS - delay) + 200}ms` }}
              />
            );
          })}
        </div>
      ))}
    </div>
  );
}

/* ------------------------------- 边缘光带 ------------------------------- */

function SoundLightEdge({ event }: { event: { id: number; kind: SoundEventKind } | null }) {
  if (!event) return <></>;
  const color = SOUND_EVENT_COLORS[event.kind];
  const pulse = `${SV_PULSE_MS}ms`;
  return (
    <div aria-hidden style={{ position: "fixed", inset: 0, pointerEvents: "none", zIndex: 2147482900 }}>
      {/* 四边光带：边缘 8px、2 次脉冲（in+out ×2） */}
      {(["top", "bottom", "left", "right"] as const).map((side) => (
        <span
          key={side}
          className="u3-sound-edge"
          data-side={side}
          style={{
            ["--u3-edge-color" as string]: color,
            ["--u3-edge-px" as string]: `${SV_EDGE_PX}px`,
            animationIterationCount: SV_PULSES,
            animationDuration: pulse,
          }}
        />
      ))}
    </div>
  );
}

/* ------------------------------- Caps 提示音 ------------------------------- */

/** WebAudio 双音（开=880/988Hz 高双音、关=440/494Hz 低双音——盲打可辨）。 */
function playCapsTone(tone: CapsTone): void {
  try {
    const Ctx = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!Ctx) return;
    const ctx = new Ctx();
    const [f1, f2] = CAPS_TONE_HZ[tone];
    const play = (freq: number, at: number) => {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.frequency.value = freq;
      osc.type = "sine";
      gain.gain.setValueAtTime(0.0001, ctx.currentTime + at);
      gain.gain.exponentialRampToValueAtTime(0.12, ctx.currentTime + at + 0.02);
      gain.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + at + 0.12);
      osc.connect(gain).connect(ctx.destination);
      osc.start(ctx.currentTime + at);
      osc.stop(ctx.currentTime + at + 0.14);
    };
    play(f1, 0);
    play(f2, 0.13);
    window.setTimeout(() => ctx.close().catch(() => undefined), 600);
  } catch (e) {
    console.error("[u3:F519] 提示音播放失败（不阻塞输入）", e);
  }
}

/* ------------------------------- 运行时总装 ------------------------------- */

export function U3Runtime(): React.ReactElement {
  const [ripples, setRipples] = useState<Ripple[]>([]);
  const [soundEvent, setSoundEvent] = useState<{ id: number; kind: SoundEventKind } | null>(null);
  const [peeking, setPeeking] = useState(false);
  const [, setPointerOpacity] = useState(1); // 值经 CSS 变量下发（documentElement），组件内不直读
  const rtCtrl = useRef<CtrlFindRt>({ downAtMs: null, otherKeyDown: false });
  const rtTypeHide = useRef<TypeHideRt>({ lastKeyMs: -1e9, mouseMovedMs: -1e9 });
  const rtBtLock = useRef<BtLockRuntimeState>({ lastSeenMs: Date.now(), awaySinceMs: null });
  const rtClickLock = useRef<ClickLockRt>({ state: "idle", pressAtMs: 0 });
  const rtBtBattery = useRef({ displayed: 80, lastLowWarnAt: {} as Record<string, number> });
  const rippleSeq = useRef(0);
  const mousePos = useRef({ x: innerWidth / 2, y: innerHeight / 2 });

  /* F513 Ctrl 定位 + F519 Caps 音 + F523 打字隐藏 + F537 瞥桌面 + F511 清空热键 */
  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      const now = performance.now();
      // F523 打字时间戳（键盘输入开始 → 指针淡出）
      if (e.key.length === 1 || e.key === "Backspace" || e.key === "Delete") {
        rtTypeHide.current.lastKeyMs = Date.now();
      }
      // F513 Ctrl 定位（组合键豁免：期间任何其他键落下即豁免复位）
      if (e.key === "Control" && !e.repeat) {
        const r = ctrlFindTick(rtCtrl.current, "ctrl", now);
        if (r.fired && u3Store.getWith("ctrlFind", "enabled", true)) {
          const id = ++rippleSeq.current;
          setRipples((prev) => [...prev.slice(-2), { id, x: mousePos.current.x, y: mousePos.current.y, startedAt: Date.now() }]);
          window.setTimeout(() => setRipples((prev) => prev.filter((x) => x.id !== id)), CTRL_FIND_TOTAL_MS + 300);
        }
      } else if (e.key !== "Control") {
        ctrlFindTick(rtCtrl.current, "other", now);
      }
      // F519 Caps/Num 提示音
      if (e.key === "CapsLock" || e.key === "NumLock") {
        const enabled = u3Store.getWith("capsSound", "enabled", false);
        if (enabled) {
          // on/off 音色按切换后状态：keydown 时读事件 getModifierState（切换后状态）
          playCapsTone(e.getModifierState("CapsLock") || e.getModifierState("NumLock") ? "on" : "off");
        }
      }
      // F511 Ctrl+Shift+Delete 剪贴板清空（含确认 toast）
      if (e.ctrlKey && e.shiftKey && e.key === "Delete") {
        e.preventDefault();
        const count = Number(u3Store.getWith("clipWipe", "historyCount", 0));
        void import("./filesec").then(({ wipeClipboard }) =>
          wipeClipboard(navigator.clipboard ?? null, []).then(() => {
            u3Store.set("clipWipe", { historyCount: 0 });
            pushToast("success" as Toast["kind"], "剪贴板已清空", count > 0 ? `连同历史 ${count} 条一并清除` : undefined);
          }).catch((err: unknown) => pushToast("error" as Toast["kind"], "剪贴板清空失败", String(err))),
        );
      }
      // F537 Win+, 瞥桌面（Meta 按住 + 逗号）
      if (e.key === "," && e.getModifierState("Meta")) {
        e.preventDefault();
        setPeeking(true);
      }
      // F535 Win+数字快捷启动（1-9,0 → winnum-N 别名动作，任务栏消费）
      if (e.getModifierState("Meta") && !e.ctrlKey && !e.altKey && /^[0-9]$/.test(e.key)) {
        const n = e.key === "0" ? 9 : Number(e.key) - 1;
        e.preventDefault();
        void dispatchU3Action(`winnum-${n}`, "runtime", { shift: e.shiftKey });
      }
      // F542 ClickLock 抓起态 Esc 放弃
      if (e.key === "Escape" && rtClickLock.current.state === "grabbed") {
        const r = clickLockStep(rtClickLock.current, { t: "esc", atMs: Date.now() });
        if (r.dropOrDrag === "cancel") void dispatchU3Action("clicklock.drop", "runtime", { how: "cancel" });
      }
      // F505 蓝牙动态锁：钥匙回连信号（真实 RSSI 源随闸门；键盘事件作为回连心跳占位不入判据）
    };
    const onKeyUp = (e: KeyboardEvent) => {
      if (e.key === "Control") ctrlFindTick(rtCtrl.current, "up", performance.now());
      if (e.key === ",") setPeeking(false);
    };
    const onMouseMove = (e: MouseEvent) => {
      mousePos.current = { x: e.clientX, y: e.clientY };
      rtTypeHide.current.mouseMovedMs = Date.now(); // F523 移动恢复即时
    };
    window.addEventListener("keydown", onKeyDown);
    window.addEventListener("keyup", onKeyUp);
    window.addEventListener("mousemove", onMouseMove, { passive: true });
    return () => {
      window.removeEventListener("keydown", onKeyDown);
      window.removeEventListener("keyup", onKeyUp);
      window.removeEventListener("mousemove", onMouseMove);
    };
  }, []);

  /* F514 声音视觉提示：订阅 vx-sound-event 事件源 */
  useEffect(() => {
    const onSound = (ev: Event) => {
      const ce = ev as CustomEvent<{ kind?: string; disturb?: string }>;
      const kind = (ce.detail?.kind as SoundEventKind) ?? "notify";
      if (!["notify", "warn", "battery"].includes(kind)) return;
      const perEvent = u3Store.getWith("soundLight", kind, true);
      const mode = (ce.detail?.disturb as DisturbMode) ?? "normal";
      const policy = soundLightPolicy(kind, mode, perEvent);
      if (!policy.flash) return; // 全静档：都不闪但中心记录（声音源已入中心）
      const id = Date.now();
      setSoundEvent({ id, kind });
      window.setTimeout(() => setSoundEvent(null), SV_PULSE_MS * SV_PULSES + 100);
    };
    window.addEventListener("vx-sound-event", onSound);
    return () => window.removeEventListener("vx-sound-event", onSound);
  }, []);

  /* F542 ClickLock 鼠标管线 + F520 标题栏中键出口（真实 DOM 事件驱动）。
   * 中键出口只对带 data-u3-titlebar 声明元素生效（桌面标题栏加标记即接入——
   * 开放扩展点，不越权改写桌面 DOM）；ClickLock 全局管线按 1.1s 阈值裁决。 */
  useEffect(() => {
    const onMouseDown = (e: MouseEvent) => {
      if (e.button !== 0) return;
      const r = clickLockStep(rtClickLock.current, { t: "press", atMs: Date.now() });
      if (r.state === "held-pending") {
        document.documentElement.setAttribute("data-u3-clicklock-pending", "1");
      }
    };
    const onMouseUp = (e: MouseEvent) => {
      if (e.button !== 0) return;
      const wasPending = rtClickLock.current.state === "held-pending";
      const r = clickLockStep(rtClickLock.current, { t: "release", atMs: Date.now() });
      document.documentElement.removeAttribute("data-u3-clicklock-pending");
      if (r.grabbed) {
        document.documentElement.setAttribute("data-u3-clicklock-grabbed", "1"); // 抓起光环（CSS 消费）
      } else if (wasPending) {
        // 短按（<1.1s）不进入抓起——普通点击语义，交给页面默认行为
      }
    };
    const onClick = (e: MouseEvent) => {
      // F520：中键点击带 data-u3-titlebar 标记的标题栏 → 最小化动作
      if (e.button === 1) {
        const bar = (e.target as HTMLElement | null)?.closest?.("[data-u3-titlebar]");
        if (bar && u3Store.getWith("midMinimize", "enabled", true)) {
          e.preventDefault();
          void dispatchU3Action("window.minimize-mid", "runtime", { windowId: bar.getAttribute("data-u3-titlebar") });
        }
        return;
      }
      if (e.button === 0 && rtClickLock.current.state === "grabbed") {
        const r = clickLockStep(rtClickLock.current, { t: "click", atMs: Date.now() });
        document.documentElement.removeAttribute("data-u3-clicklock-grabbed");
        if (r.dropOrDrag === "drop") void dispatchU3Action("clicklock.drop", "runtime", { how: "drop", x: e.clientX, y: e.clientY });
      }
    };
    window.addEventListener("mousedown", onMouseDown, true);
    window.addEventListener("mouseup", onMouseUp, true);
    window.addEventListener("click", onClick, true);
    window.addEventListener("auxclick", onClick, true);
    return () => {
      window.removeEventListener("mousedown", onMouseDown, true);
      window.removeEventListener("mouseup", onMouseUp, true);
      window.removeEventListener("click", onClick, true);
      window.removeEventListener("auxclick", onClick, true);
    };
  }, []);

  /* F523 打字隐藏指针：2s 恢复计时 + CSS 变量下发 */
  useEffect(() => {
    const timer = window.setInterval(() => {
      const enabled = u3Store.getWith("typeHide", "enabled", false);
      const op = typeHideOpacity(rtTypeHide.current, Date.now(), enabled, false);
      setPointerOpacity(op);
      document.documentElement.style.setProperty("--vx-u3-pointer-opacity", String(op));
    }, 200);
    return () => window.clearInterval(timer);
  }, []);

  /* F537 瞥桌面：CSS 变量下发（窗口容器消费） */
  useEffect(() => {
    const style = peekStyle(peeking);
    document.documentElement.style.setProperty("--vx-u3-window-opacity", String(style.opacity));
    document.documentElement.style.setProperty("--vx-u3-window-pe", style.pointerEvents);
    document.documentElement.style.setProperty("--vx-u3-window-transition", style.transition);
  }, [peeking]);

  /* F505 蓝牙动态锁 + F545 电量平滑：1s 心跳（真实蓝牙源随闸门登记） */
  useEffect(() => {
    const timer = window.setInterval(() => {
      const now = Date.now();
      // F505：钥匙在场信号 = 最近回连心跳（本会话以导航焦点为代表；真实 RSSI 随闸门）
      const keyPresent = document.hasFocus();
      const cfg = u3Store.get("btLock");
      if (cfg.enabled) {
        const r = btLockTick(rtBtLock.current, keyPresent, now);
        if (r.lock) {
          rtBtLock.current.awaySinceMs = null;
          pushToast("info" as Toast["kind"], "蓝牙动态锁：钥匙设备信号消失 30 秒", "已自动锁屏（F238）——回来输 PIN 快速解锁（F504）");
          window.dispatchEvent(new CustomEvent("vx-u3-lock-screen"));
        }
      }
      // F545：电量平滑（模拟源以恒值推进；真实 HID 电量上报随闸门）
      if (u3Store.getWith("btBattery", "enabled", false)) {
        smoothBattery(rtBtBattery.current, 80);
      }
    }, 1000);
    return () => window.clearInterval(timer);
  }, []);

  return (
    <>
      <CtrlRippleLayer ripples={ripples} />
      <SoundLightEdge event={soundEvent} />
    </>
  );
}

/** 指针透明度当前值（供 F335 优先平面副本层消费——与 J1Runtime 复用同一平面）。 */
export function currentPointerOpacity(): number {
  const v = document.documentElement.style.getPropertyValue("--vx-u3-pointer-opacity");
  const n = Number(v);
  return Number.isFinite(n) && v !== "" ? n : 1;
}

/** 瞥桌面窗口层样式（窗口容器直接展开本对象——只动透明度不动几何，恢复原位精度 <1px）。
 *  pointer-events 无法走 CSS 变量（非动画属性语义），由调用方按 peeking 状态二选一。 */
export function peekWindowStyle(peeking: boolean): React.CSSProperties {
  const s = peekStyle(peeking);
  return { opacity: s.opacity, pointerEvents: s.pointerEvents, transition: s.transition };
}

/** 松开恢复时长（判据：进出 120ms±20ms——恢复与进入对称）。 */
export const PEEK_RESUME_MS = PEEK_FADE_MS;
/** F523 停止输入恢复 2s（类型重导出——运行时与面板同源）。 */
export const TYPE_RESUME_MS = TYPE_HIDE_RESUME_MS;
